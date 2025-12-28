#![cfg(feature = "arena_module")]
//! Main Arena interface and public API

extern crate alloc;

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::vec::Vec;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::ptr::NonNull;
use std::sync::Arc;

// Re-export core functionality
pub use crate::core::{
    ArenaCheckpoint, ArenaStats, DebugStats, ArenaBuilder as CoreArenaBuilder, Scope as CoreScope,
    MemoryPool, Chunk, AtomicCounter,
};

// Import specific types from core
use crate::core::ArenaInner;

// Import constants from lib.rs
use crate::{MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, DEFAULT_CHUNK_SIZE};

// Re-export feature modules
#[cfg(feature = "virtual_memory")]
pub use crate::virtual_memory::{VirtualMemoryRegion, VirtualChunk as VMChunk};

#[cfg(feature = "thread_local")]
pub use crate::thread_local::*;

#[cfg(feature = "lockfree")]
pub use crate::lockfree::{LockFreeBuffer, LockFreeStats};

#[cfg(feature = "debug")]
pub use crate::debug::{DEBUG_STATE, AllocationInfo, GUARD_MAGIC, FREED_MAGIC};


/// v0.5.0: Arena builder for customizing arena creation
pub struct ArenaBuilder {
    initial_capacity: usize,
    chunk_size: usize,
    thread_safe: bool,
    diagnostics_sink: Option<Box<dyn Fn(&str) + Send + Sync + 'static>>,
}

impl ArenaBuilder {
    pub fn new() -> Self {
        Self {
            initial_capacity: crate::DEFAULT_CHUNK_SIZE,
            chunk_size: crate::DEFAULT_CHUNK_SIZE,
            thread_safe: false,
            diagnostics_sink: None,
        }
    }
    
    pub fn initial_capacity(mut self, capacity: usize) -> Self {
        self.initial_capacity = capacity;
        self
    }
    
    pub fn chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size;
        self
    }
    
    pub fn thread_safe(mut self, thread_safe: bool) -> Self {
        self.thread_safe = thread_safe;
        self
    }
    pub fn perf_bundle(self) -> Self { self }

    pub fn safety_bundle(self) -> Self { self }

    pub fn debuggable_bundle(self) -> Self { self }

    pub fn server_bundle(self) -> Self { self }

    pub fn diagnostics_sink<F>(mut self, _sink: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.diagnostics_sink = Some(Box::new(_sink));
        self
    }

    pub fn enable_stats(mut self, _enable: bool) -> Self { self }
    pub fn enable_debug(mut self, _enable: bool) -> Self { self }
    pub fn enable_lockfree(mut self, _enable: bool) -> Self { self }
    pub fn enable_thread_local(mut self, _enable: bool) -> Self { self }
    pub fn enable_virtual_memory(mut self, _enable: bool) -> Self { self }

    pub fn build(self) -> Arena {
        // If a diagnostics sink was supplied, emit a short config message.
        if let Some(sink) = self.diagnostics_sink {
            let msg = format!("capacity: {}, chunk_size: {}, thread_safe: {}", self.initial_capacity, self.chunk_size, self.thread_safe);
            sink(&msg);
        }

        Arena::with_capacity(self.initial_capacity)
    }
}

impl Default for ArenaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Scope<'scope, 'arena> {
    arena: &'arena Arena,
    _marker: PhantomData<&'scope mut ()>,
    checkpoint: crate::ArenaCheckpoint,
}

impl<'scope, 'arena> Scope<'scope, 'arena>
where
    'arena: 'scope,
{
    pub fn new(arena: &'arena Arena) -> Self {
        let checkpoint = arena.checkpoint();
        Self {
            arena,
            _marker: PhantomData,
            checkpoint,
        }
    }

    pub fn alloc<T>(&self, value: T) -> &'scope mut T {
        self.arena.alloc(value)
    }

    pub fn alloc_str(&self, s: &str) -> &'scope str {
        self.arena.alloc_str(s)
    }
    

    pub fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &'scope mut [T] {
        self.arena.alloc_slice_copy(slice)
    }

    pub fn alloc_slice_uninit<T>(&self, len: usize) -> &'scope mut [MaybeUninit<T>] {
        self.arena.alloc_slice_uninit(len)
    }

    pub fn checkpoint(&self) -> ArenaCheckpoint {
        self.arena.checkpoint()
    }

    pub unsafe fn rewind_to_checkpoint(&self, checkpoint: ArenaCheckpoint) {
        self.arena.rewind_to_checkpoint(checkpoint);
    }

    pub fn reset(&self) {
        unsafe {
            self.arena.rewind_to_checkpoint(self.checkpoint);
        }
    }
}

impl<'scope, 'arena> Drop for Scope<'scope, 'arena> {
    fn drop(&mut self) {
        unsafe {
            self.arena.rewind_to_checkpoint(self.checkpoint);
        }
    }
}
#[cfg(feature = "debug")]
pub use crate::debug::DebugAllocator;


#[cfg(feature = "thread_local")]
pub use crate::thread_local::ThreadLocalCache;

/// Chunk usage information returned by `Arena::chunk_usage()`.
pub struct ChunkUsage {
    pub capacity: usize,
    pub used: usize,
}

/// Feature capability summary.
#[derive(Debug, Clone)]
pub struct FeatureStatus {
    pub lockfree: bool,
    pub thread_local: bool,
    pub slab: bool,
    pub virtual_memory: bool,
    pub debug: bool,
    pub stats: bool,
}

/// Lightweight arena configuration snapshot.
#[derive(Debug, Clone)]
pub struct ArenaConfig {
    pub initial_capacity: usize,
    pub chunk_size: usize,
    pub reserve_size: Option<usize>,
    pub max_chunks: Option<usize>,
    pub fast_path_threshold: usize,
    pub prefetch_distance: usize,
    pub features: FeatureStatus,
}

// Main Arena type
pub struct Arena {
    inner: UnsafeCell<crate::core::ArenaInner>,
    #[cfg(feature = "debug")]
    debug_allocator: DebugAllocator,
    #[cfg(feature = "thread_local")]
    thread_cache: ThreadLocalCache,
    #[cfg(feature = "lockfree")]
    lockfree_stats: LockFreeStats,
}

unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
    /// Creates a new arena with the default capacity (64KB).
    ///
    /// This is a convenient shorthand for [`Arena::with_capacity`].
    #[inline]
    pub fn new() -> Self {
        Self::with_capacity(crate::DEFAULT_CHUNK_SIZE)
    }

    /// Creates a new arena with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let inner = ArenaInner::new(capacity).expect("Failed to create arena");
        Self {
            inner: UnsafeCell::new(inner),
            #[cfg(feature = "debug")]
            debug_allocator: DebugAllocator::new(),
            #[cfg(feature = "thread_local")]
            thread_cache: ThreadLocalCache::new(0), // Will be updated later
            #[cfg(feature = "lockfree")]
            lockfree_stats: LockFreeStats::new(),
        }
    }

    /// Compatibility shim: return a builder for the arena.
    pub fn builder() -> ArenaBuilder {
        ArenaBuilder::new()
    }

    /// Compatibility shim: fast-path allocation alias for `alloc`.
    pub fn alloc_fast<T>(&self, value: T) -> &mut T {
        // In v1 API `alloc` is the canonical method; keep shim for benches.
        self.alloc(value)
    }

    /// Convenience typed allocators for benches and legacy code.
    pub fn alloc_u8(&self, v: u8) -> &mut u8 { self.alloc(v) }
    pub fn alloc_u32(&self, v: u32) -> &mut u32 { self.alloc(v) }
    pub fn alloc_u64(&self, v: u64) -> &mut u64 { self.alloc(v) }

    /// Allocate an array by value and return a reference to it.
    pub fn alloc_array<T, const N: usize>(&self, arr: [T; N]) -> &mut [T; N] {
        self.alloc(arr)
    }

    /// Create an arena with virtual memory backing
    #[cfg(feature = "virtual_memory")]
    pub fn with_virtual_memory(reserve_size: usize) -> Self {
        let capacity = reserve_size.min(64 * 1024); // Start with 64KB committed
        let mut arena = Self::with_capacity(capacity);
        
        // Set up virtual memory region
        let inner = unsafe { &mut *arena.inner.get() };
        inner.virtual_region = Some(VirtualMemoryRegion::new(reserve_size)
            .expect("Failed to create virtual memory region"));
        
        arena
    }

    /// Allocate memory for a value
    pub fn alloc<T>(&self, value: T) -> &mut T {
        let layout = Layout::new::<T>();
        let ptr = self.allocate_raw(layout);
        
        unsafe {
            let ptr = ptr as *mut T;
            ptr.write(value);
            &mut *ptr
        }
    }

    /// Allocate memory for a default value
    pub fn alloc_default<T: Default>(&self) -> &mut T {
        self.alloc(T::default())
    }

    /// Allocates a slice by copying the contents of `slice` into the arena.
    ///
    /// The returned slice has the same length and contents as `slice`.
    #[inline]
    pub fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &mut [T] {
        if slice.is_empty() {
            // Return empty slice for empty input
            unsafe {
                slice::from_raw_parts_mut(NonNull::<T>::dangling().as_ptr(), 0)
            }
        } else {
            let len = slice.len();
            let layout = Layout::array::<T>(len).unwrap();
            let ptr = self.allocate_raw(layout);
            
            unsafe {
                let ptr = ptr as *mut T;
                let slice_ptr = slice::from_raw_parts_mut(ptr, len);
                slice_ptr.copy_from_slice(slice);
                slice_ptr
            }
        }
    }

    /// Allocate memory for an uninitialized slice
    pub fn alloc_slice_uninit<T>(&self, len: usize) -> &mut [MaybeUninit<T>] {
        if len == 0 {
            unsafe {
                return slice::from_raw_parts_mut(NonNull::<MaybeUninit<T>>::dangling().as_ptr(), 0);
            }
        }

        let layout = Layout::array::<T>(len).expect("Invalid layout");
        let ptr = self.allocate_raw(layout);

        unsafe { slice::from_raw_parts_mut(ptr as *mut MaybeUninit<T>, len) }
    }

    /// Allocate memory for a string
    pub fn alloc_str(&self, s: &str) -> &str {
        let slice = self.alloc_slice_copy(s.as_bytes());
        unsafe { std::str::from_utf8_unchecked(slice) }
    }

    /// Allocate raw memory
    #[inline]
    pub fn allocate_raw(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        if size == 0 {
            #[cfg(feature = "stats")]
            {
                let inner = unsafe { &*self.inner.get() };
                inner.stats().allocation_count.fetch_add(1, Ordering::Relaxed);
            }
            return ptr::NonNull::<u8>::dangling().as_ptr();
        }

        // v0.5.0: Try lock-free buffer for small allocations
        #[cfg(feature = "lockfree")]
        {
            if size <= 1024 {
                let inner = unsafe { &*self.inner.get() };
                if let Some(ref buffer) = inner.lockfree_buffer {
                        if let Some(ptr) = buffer.try_alloc(size, align) {
                            self.lockfree_stats.record_allocation();
                            self.lockfree_stats.record_cache_hit();

                            #[cfg(feature = "debug")]
                            {
                                let arena_id = self.debug_allocator.arena_id();
                                // Wrap the raw arena pointer with a debug guard allocation
                                let guarded = self.debug_allocator.allocate_with_guard(ptr, size, inner.current_checkpoint_id);
                                #[cfg(feature = "stats")]
                                {
                                    inner.stats().bytes_used.fetch_add(size, Ordering::Relaxed);
                                    inner.stats().allocation_count.fetch_add(1, Ordering::Relaxed);
                                }
                                return guarded;
                            }

                            #[cfg(not(feature = "debug"))]
                            {
                                #[cfg(feature = "stats")]
                                {
                                    inner.stats().bytes_used.fetch_add(size, Ordering::Relaxed);
                                    inner.stats().allocation_count.fetch_add(1, Ordering::Relaxed);
                                }
                                return ptr;
                            }
                    } else {
                        self.lockfree_stats.record_cache_miss();
                        self.lockfree_stats.record_contention();
                    }
                }
            }
        }

        // v0.5.0: Try thread-local cache first for very small allocations
        #[cfg(feature = "thread_local")]
        {
            if size <= 512 {
                #[cfg(feature = "debug")]
                let arena_id = self.debug_allocator.arena_id();
                #[cfg(not(feature = "debug"))]
                let arena_id = 0usize;

                if let Some(ptr) = crate::thread_local::try_thread_local_alloc(arena_id, size, align) {
                    #[cfg(feature = "debug")]
                    {
                        let inner = unsafe { &*self.inner.get() };
                        let guarded = self.debug_allocator.allocate_with_guard(ptr, size, inner.current_checkpoint_id);
                        #[cfg(feature = "stats")]
                        {
                            let inner = unsafe { &*self.inner.get() };
                            inner.stats().allocation_count.fetch_add(1, Ordering::Relaxed);
                        }
                        return guarded;
                    }

                    #[cfg(not(feature = "debug"))]
                    {
                        #[cfg(feature = "stats")]
                        {
                            let inner = unsafe { &*self.inner.get() };
                            inner.stats().allocation_count.fetch_add(1, Ordering::Relaxed);
                        }
                        return ptr;
                    }
                }
            }
        }

        // Try regular allocation
        let inner = unsafe { &mut *self.inner.get() };
        if let Some(ptr) = inner.allocate(layout) {
            #[cfg(feature = "debug")]
            {
                let arena_id = self.debug_allocator.arena_id();
                let guarded = self.debug_allocator.allocate_with_guard(ptr, size, inner.current_checkpoint_id);
                crate::debug::register_allocation(arena_id, guarded, size, inner.current_checkpoint_id);
                return guarded;
            }

            #[cfg(not(feature = "debug"))]
            {
                return ptr;
            }
        }

        // Need new chunk
        let new_capacity = next_chunk_capacity(size);
        let chunk_index = unsafe {
            let inner = &mut *self.inner.get();
            inner.add_chunk(new_capacity)
        };

        match chunk_index {
            Ok(_) => {
                // Try allocation again
                let inner = unsafe { &mut *self.inner.get() };
                if let Some(ptr) = inner.allocate(layout) {
                    #[cfg(feature = "debug")]
                    {
                        let guarded = self.debug_allocator.allocate_with_guard(ptr, size, inner.current_checkpoint_id);
                        return guarded;
                    }

                    #[cfg(not(feature = "debug"))]
                    {
                        ptr
                    }
                } else {
                    ptr::null_mut()
                }
            }
            Err(_) => ptr::null_mut(),
        }
    }

    /// Returns usage information for each chunk in the arena.
    pub fn chunk_usage(&self) -> Vec<ChunkUsage> {
        let inner = unsafe { &*self.inner.get() };
        inner
            .chunks
            .iter()
            .map(|c| ChunkUsage {
                capacity: c.capacity(),
                used: c.used(),
            })
            .collect()
    }

    /// Reset arena and shrink to a single chunk.
    pub fn reset_and_shrink_to_fit(&self) {
        let inner = unsafe { &mut *self.inner.get() };
        // Keep first chunk and drop the rest
        if inner.chunks.len() > 1 {
            inner.chunks.truncate(1);
        }
        // Reset used on remaining chunk
        if let Some(chunk) = inner.chunks.get_mut(0) {
            chunk.set_used(0);
        }
        inner.current_chunk.store(0, Ordering::Release);
        inner.checkpoints.clear();
        #[cfg(feature = "stats")]
        {
            inner.stats.bytes_used.store(0, Ordering::Release);
            inner.stats.bytes_allocated.store(0, Ordering::Release);
            inner.stats.allocation_count.store(0, Ordering::Release);
            inner.stats.chunk_count = inner.chunks.len();
        }
    }

    /// Return feature status for the current build.
    pub fn feature_status(&self) -> FeatureStatus {
        FeatureStatus {
            lockfree: cfg!(feature = "lockfree"),
            thread_local: cfg!(feature = "thread_local"),
            slab: cfg!(feature = "slab"),
            virtual_memory: cfg!(feature = "virtual_memory"),
            debug: cfg!(feature = "debug"),
            stats: cfg!(feature = "stats"),
        }
    }

    /// Return a lightweight config snapshot.
    pub fn build_config(&self) -> ArenaConfig {
        ArenaConfig {
            initial_capacity: crate::DEFAULT_CHUNK_SIZE,
            chunk_size: crate::DEFAULT_CHUNK_SIZE,
            reserve_size: None,
            max_chunks: None,
            fast_path_threshold: 1024,
            prefetch_distance: 8,
            features: self.feature_status(),
        }
    }

    

    /// Convenience: allocate a copy of a slice (batch allocate).
    pub fn alloc_batch<T: Copy>(&self, slice: &[T]) -> &mut [T] {
        self.alloc_slice_copy(slice)
    }

    /// Reset the arena, deallocating all memory
    pub unsafe fn reset(&mut self) {
        let inner = &mut *self.inner.get();
        for chunk in &mut inner.chunks {
            chunk.reset();
        }
        inner.current_chunk.store(0, Ordering::Release);

        #[cfg(feature = "stats")]
        {
            inner.stats().bytes_used.store(0, Ordering::Release);
            inner.stats().bytes_allocated.store(0, Ordering::Release);
            inner.stats().allocation_count.store(0, Ordering::Release);
        }
        
        // v0.5.0: Clear checkpoints on full reset
        inner.checkpoints.clear();
        
        // v0.5.0: Reset thread-local cache
        #[cfg(feature = "thread_local")]
        {
            crate::thread_local::reset_thread_cache();
        }
        
        // v0.5.0: Reset lock-free buffer
        #[cfg(feature = "lockfree")]
        {
            if let Some(ref buffer) = inner.lockfree_buffer {
                buffer.reset();
            }
        }
    }

    /// Create a checkpoint for arena reset
    pub fn checkpoint(&self) -> ArenaCheckpoint {
        let inner = unsafe { &mut *self.inner.get() };
        inner.checkpoint()
    }

    /// Rewind arena to checkpoint
    pub unsafe fn rewind_to_checkpoint(&self, checkpoint: ArenaCheckpoint) {
        let inner = unsafe { &mut *self.inner.get() };
        inner.rewind_to_checkpoint(checkpoint);
    }

    /// Pushes a checkpoint onto the arena's checkpoint stack.
    ///
    /// This is useful for nested scoping scenarios where you want to
    /// be able to rewind to the most recent checkpoint with `pop_checkpoint()`.
    ///
    /// # Returns
    ///
    /// Returns the checkpoint that was pushed.
    #[inline]
    pub fn push_checkpoint(&self) -> crate::ArenaCheckpoint {
        // `ArenaInner::checkpoint()` already records the checkpoint in
        // `inner.checkpoints`; call the inner helper to avoid double-pushing.
        let inner = unsafe { &mut *self.inner.get() };
        inner.push_checkpoint()
    }

    /// Pops and rewinds to the most recent checkpoint.
    ///
    /// This combines `pop_checkpoint()` and `rewind_to_checkpoint()` for
    /// convenient nested scoping.
    ///
    /// # Safety
    ///
    /// - All references allocated after the checkpoint become invalid
    /// - Must have a checkpoint on the stack (panics otherwise)
    /// - No other threads should be using the arena during rewind
    ///
    /// # Panics
    ///
    /// Panics if there are no checkpoints on the stack.
    #[inline]
    pub unsafe fn pop_and_rewind(&mut self) -> crate::ArenaCheckpoint {
        let inner = &mut *self.inner.get();
        let checkpoint = inner
            .checkpoints
            .pop()
            .expect("Cannot pop checkpoint: no checkpoints on stack");
        self.rewind_to_checkpoint(checkpoint);
        checkpoint
    }

    /// Returns the number of checkpoints currently on the stack.
    #[inline]
    pub fn checkpoint_count(&self) -> usize {
        let inner = unsafe { &*self.inner.get() };
        inner.checkpoints.len()
    }

    /// Clears all checkpoints from the stack.
    ///
    /// This is useful when you want to reset the checkpoint management
    /// without affecting the arena's allocated memory.
    #[inline]
    pub fn clear_checkpoints(&self) {
        let inner = unsafe { &mut *self.inner.get() };
        inner.checkpoints.clear();
    }

    /// Checks if a reference is still valid (use-after-rewind detection).
    ///
    /// This method is only available when the "debug" feature is enabled.
    /// It helps detect use-after-rewind errors by checking if the allocation
    /// was made after the current checkpoint.
    ///
    /// # Safety
    ///
    /// The reference must be from this arena.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the reference is valid, or `Err(String)` with
    /// an error message if use-after-rewind is detected.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use arena_b::Arena;
    /// #[cfg(feature = "debug")]
    /// {
    ///     let arena = Arena::new();
    ///     let checkpoint = arena.checkpoint();
    ///     
    ///     let value = arena.alloc(42u32);
    ///     
    ///     // Check validity before rewind
    ///     unsafe { arena.check_valid(value).unwrap(); }
    ///     
    ///     unsafe { arena.rewind_to_checkpoint(checkpoint); }
    ///     
    ///     // Note: Use-after-rewind detection may not work in doctest environment
    ///     // This is primarily for demonstration purposes
    /// }
    /// ```
    #[cfg(feature = "debug")]
    #[inline]
    pub unsafe fn check_valid<T>(&self, reference: &T) -> Result<(), String> {
        let arena_id = self as *const Arena as usize;
        let ptr = reference as *const T as *mut u8;
        let debug_state = crate::debug::DEBUG_STATE.read().unwrap();
        debug_state.check_use_after_rewind(arena_id, ptr as *mut u8)
    }

    /// Gets arena statistics including allocation count and memory usage.
    pub fn stats(&self) -> crate::legacy_arena::ArenaStats {
        let inner = unsafe { &*self.inner.get() };
        #[cfg(feature = "stats")]
        {
            let stats_ref = inner.stats();
            crate::legacy_arena::ArenaStats {
                bytes_allocated: stats_ref.bytes_allocated.load(std::sync::atomic::Ordering::Relaxed),
                bytes_used: stats_ref.bytes_used.load(std::sync::atomic::Ordering::Relaxed),
                allocation_count: stats_ref.allocation_count.load(std::sync::atomic::Ordering::Relaxed),
                chunk_count: stats_ref.chunk_count,
            }
        }
        #[cfg(not(feature = "stats"))]
        {
            crate::legacy_arena::ArenaStats {
                bytes_allocated: 0,
                bytes_used: 0,
                allocation_count: 0,
                chunk_count: 0,
            }
        }
    }


    /// Validates all allocations in the debug state.
    ///
    /// This method checks for corruption in the debug tracking system
    /// and returns detailed information about any issues found.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if all allocations are valid, or `Err(String)` with
    /// details about any corruption detected.
    #[cfg(feature = "debug")]
    #[inline]
    pub fn validate_debug_state(&self) -> Result<(), String> {
        let arena_id = self as *const Arena as usize;
        let debug_state = crate::debug::DEBUG_STATE.read().unwrap();
        let (total, corrupted) = debug_state.get_stats(arena_id);

        if corrupted > 0 {
            Err(format!("Found {} corrupted debug guards", corrupted))
        } else {
            Ok(())
        }
    }

    /// Returns lock-free allocation statistics.
    ///
    /// This method provides insight into the lock-free allocation performance
    /// and can help diagnose contention issues. Available only when the "lockfree" feature is enabled.
    ///
    /// # Returns
    ///
    /// Returns a tuple of (allocations, cache_hits, cache_misses, contention_count).
    #[cfg(feature = "lockfree")]
    #[inline]
    pub fn lockfree_stats(&self) -> (usize, usize, usize, usize) {
        self.lockfree_stats.get_stats()
    }

    /// Returns the number of bytes currently committed in the virtual memory region.
    ///
    /// Available only when the `virtual_memory` feature is enabled and the arena
    /// was constructed via [`Arena::with_virtual_memory`]. Returns `None` for
    /// arenas without virtual memory backing.
    #[cfg(feature = "virtual_memory")]
    #[inline]
    pub fn virtual_memory_committed_bytes(&self) -> Option<usize> {
        let inner = unsafe { &*self.inner.get() };
        #[cfg(feature = "virtual_memory")]
        {
            inner.virtual_region.as_ref().map(|region| region.committed_bytes())
        }
        #[cfg(not(feature = "virtual_memory"))]
        {
            None
        }
    }

    /// Returns debug statistics about allocations and checkpoints.
    ///
    /// This method provides insight into the debug tracking system
    /// and can help diagnose memory safety issues.
    #[cfg(feature = "debug")]
    #[inline]
    pub fn debug_stats(&self) -> crate::DebugStats {
        let arena_id = self as *const Arena as usize;
        let debug_state = crate::debug::DEBUG_STATE.read().unwrap();
        let inner = unsafe { &*self.inner.get() };

        let (total_allocations, corrupted_allocations) = debug_state.get_stats(arena_id);

        crate::DebugStats {
            total_allocations,
            active_checkpoints: debug_state
                .get_current_checkpoint_id(arena_id)
                .saturating_sub(1) as usize,
            current_checkpoint_id: debug_state.get_current_checkpoint_id(arena_id),
            corrupted_allocations,
            leak_reports: 0, // Will be populated by leak_report() calls
        }
    }

    /// Create a scope for RAII arena management
    pub fn scope<'a, F, R>(&'a self, f: F) -> R 
    where
        F: FnOnce(&Scope<'_, 'a>) -> R,
    {
        let scope = Scope::new(self);
        f(&scope)
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // Rely on Chunk::drop to free chunk memory; avoid double-free here.
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

// Helper function to calculate next chunk capacity
fn next_chunk_capacity(min_size: usize) -> usize {
    let mut capacity = MIN_CHUNK_SIZE;
    while capacity < min_size && capacity < MAX_CHUNK_SIZE {
        capacity *= 2;
    }
    capacity.max(min_size).min(MAX_CHUNK_SIZE)
}

// Pool allocator for reusable objects
pub struct Pool<T> {
    inner: PoolInner<T>,
}

struct PoolInner<T> {
    objects: Vec<Option<T>>,
    stats: PoolStats,
}

#[derive(Debug, Default)]
pub struct PoolStats {
    pub allocations: usize,
    pub deallocations: usize,
    pub peak_usage: usize,
}

impl<T> Pool<T> {
    pub fn new() -> Self {
        Self {
            inner: PoolInner {
                objects: Vec::new(),
                stats: PoolStats::default(),
            },
        }
    }

    pub fn alloc(&mut self, value: T) -> Pooled<'_, T> {
        let obj = self.inner.objects.pop().unwrap_or(Some(value));
        self.inner.stats.allocations += 1;
        self.inner.stats.peak_usage = self.inner.stats.peak_usage.max(
            self.inner.objects.len() + 1
        );
        
        Pooled {
            pool: &mut self.inner,
            value: obj,
        }
    }

    pub fn stats(&self) -> &PoolStats {
        &self.inner.stats
    }
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Pooled<'pool, T> {
    pool: &'pool mut PoolInner<T>,
    value: Option<T>,
}

impl<'pool, T> std::ops::Deref for Pooled<'pool, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value.as_ref().unwrap()
    }
}

impl<'pool, T> std::ops::DerefMut for Pooled<'pool, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_mut().unwrap()
    }
}

impl<'pool, T> Drop for Pooled<'pool, T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.pool.objects.push(Some(value));
            self.pool.stats.deallocations += 1;
        }
    }
}

// Thread-safe arena wrapper
pub struct SyncArena {
    arena: Mutex<Arena>,
}

impl SyncArena {
    pub fn new() -> Self {
        Self {
            arena: Mutex::new(Arena::new()),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            arena: Mutex::new(Arena::with_capacity(capacity)),
        }
    }

    pub fn alloc<T>(&self, value: T) -> std::sync::MutexGuard<'_, T> {
        let mut arena = self.arena.lock().unwrap();
        let ptr = arena.alloc(value) as *mut T;
        // This API needs redesign - returning MutexGuard<T> doesn't make sense
        // For now, panic to indicate the issue
        panic!("SyncArena::alloc API needs redesign - cannot return MutexGuard<T>")
    }
}

impl Default for SyncArena {
    fn default() -> Self {
        Self::new()
    }
}

// Re-export for backward compatibility
pub use self::Pool as ObjectPool;
pub use self::Pooled as PooledObject;
