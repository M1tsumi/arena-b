#![cfg(feature = "arena_module")]
//! Main Arena interface and public API

use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicUsize, Ordering};
use alloc::collections::Vec;
use alloc::vec::Vec;
use alloc::sync::Mutex;
use std::sync::MutexGuard;

// Re-export core functionality
pub use crate::core::{
    ArenaCheckpoint, ArenaStats, DebugStats, ArenaBuilder, Scope,
    MemoryPool, Chunk, VirtualChunk, AtomicCounter,
};

// Import specific types from core
use crate::core::{ArenaInner, AtomicStats};

// Import constants from lib.rs
use crate::{MIN_CHUNK_SIZE, MAX_CHUNK_SIZE, DEFAULT_CHUNK_SIZE};

// Re-export feature modules
#[cfg(feature = "virtual_memory")]
pub use crate::virtual_memory::VirtualMemoryRegion;

#[cfg(feature = "thread_local")]
pub use crate::thread_local::*;

#[cfg(feature = "lockfree")]
pub use crate::lockfree::{LockFreeBuffer, LockFreeStats};

#[cfg(feature = "debug")]
pub use crate::debug::{DEBUG_STATE, AllocationInfo, GUARD_MAGIC, FREED_MAGIC};

// Main Arena struct moved from lib.rs
pub struct Arena {
    inner: UnsafeCell<ArenaInner>,
    #[cfg(feature = "stats")]
    stats: AtomicStats,
    #[cfg(feature = "lockfree")]
    lockfree_stats: LockFreeStats,
    memory_pool: UnsafeCell<MemoryPool>,
    _padding: [u8; 64], // Cache line padding to avoid false sharing
}

/// v0.5.0: Arena builder for customizing arena creation
pub struct ArenaBuilder {
    initial_capacity: usize,
    chunk_size: usize,
    thread_safe: bool,
}

pub struct Scope<'scope, 'arena> {
    arena: &'arena Arena,
    _marker: PhantomData<&'scope mut ()>,
}

impl<'scope, 'arena> Scope<'scope, 'arena> {
    pub fn new(arena: &'arena Arena) -> Self {
        Self {
            arena,
            _marker: PhantomData,
        }
    }

    pub fn alloc<T>(&self, value: T) -> &'scope mut T {
        self.arena.alloc(value)
    }

    pub fn alloc_str(&self, s: &str) -> &'scope mut str {
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
        unsafe { self.arena.reset() };
    }
}
#[cfg(feature = "debug")]
pub use crate::debug::DebugAllocator;

#[cfg(feature = "virtual_memory")]
pub use crate::virtual_memory::{VirtualMemoryRegion, VirtualChunk as VMChunk};

#[cfg(feature = "thread_local")]
pub use crate::thread_local::ThreadLocalCache;

#[cfg(feature = "lockfree")]
pub use crate::lockfree::{LockFreeBuffer, LockFreeStats};

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
    /// Create a new arena with default capacity
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CHUNK_SIZE).expect("Failed to create arena")
    }

    /// Create a new arena with specified initial capacity
    pub fn with_capacity(capacity: usize) -> Result<Self, &'static str> {
        let mut capacity = capacity.max(MIN_CHUNK_SIZE);
        while capacity < MIN_CHUNK_SIZE && capacity < MAX_CHUNK_SIZE {
            capacity *= 2;
        }
        capacity = capacity.max(MIN_CHUNK_SIZE).min(MAX_CHUNK_SIZE);

        let chunk = Chunk::new(capacity).map_err(|_| "Failed to allocate chunk")?;
        let checkpoint = ArenaCheckpoint {
            chunk_index: 0,
            chunk_offset: 0,
            allocation_count: 0,
            bytes_used: 0,
            #[cfg(feature = "debug")]
            checkpoint_id: 1,
        };

        let inner = ArenaInner {
            chunks: vec![chunk],
            current_chunk: AtomicUsize::new(0),
            total_allocated: AtomicCounter { _padding: [0; 64] },
            checkpoints: vec![checkpoint],
            #[cfg(feature = "debug")]
            current_checkpoint_id: 1,
            #[cfg(feature = "virtual_memory")]
            virtual_chunk: None,
            #[cfg(feature = "lockfree")]
            lockfree_buffer: None,
            _padding: [0; 64 - 2 * 8 - 8 - 8],
        };
        
        Ok(Self {
            inner: UnsafeCell::new(inner),
            #[cfg(feature = "stats")]
            stats: AtomicStats {
                bytes_used: AtomicUsize::new(0),
                allocation_count: AtomicUsize::new(0),
                _padding: [0; 64 - 2 * 8],
            },
            #[cfg(feature = "lockfree")]
            lockfree_stats: LockFreeStats::new(),
            memory_pool: UnsafeCell::new(MemoryPool::new()),
            _padding: [0; 64],
        })
    }

    /// Create an arena with virtual memory backing
    #[cfg(feature = "virtual_memory")]
    pub fn with_virtual_memory(reserve_size: usize) -> Self {
        let capacity = reserve_size.min(64 * 1024); // Start with 64KB committed
        let mut arena = Self::with_capacity(capacity).expect("Failed to create arena");
        
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

    /// Allocate memory for a slice copy
    pub fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &mut [T] {
        let layout = Layout::for_value(slice);
        let ptr = self.allocate_raw(layout);
        
        unsafe {
            let ptr = ptr as *mut [T];
            let slice_ptr = slice::from_raw_parts_mut(ptr as *mut T, slice.len());
            slice_ptr.copy_from_slice(slice);
            slice_ptr
        }
    }

    /// Allocate memory for an uninitialized slice
    pub fn alloc_slice_uninit<T>(&self, len: usize) -> &mut [MaybeUninit<T>] {
        let layout = Layout::array::<T>(len).expect("Invalid layout");
        let ptr = self.allocate_raw(layout);
        
        unsafe {
            slice::from_raw_parts_mut(ptr as *mut MaybeUninit<T>, len)
        }
    }

    /// Allocate memory for a string
    pub fn alloc_str(&self, s: &str) -> &mut str {
        let slice = self.alloc_slice_copy(s.as_bytes());
        unsafe {
            std::str::from_utf8_unchecked_mut(slice)
        }
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
                            crate::debug::register_allocation(arena_id, ptr, size, inner.current_checkpoint_id);
                        }
                        #[cfg(feature = "stats")]
                        {
                            inner.stats().bytes_used.fetch_add(size, Ordering::Relaxed);
                            inner.stats().allocation_count.fetch_add(1, Ordering::Relaxed);
                        }
                        return ptr;
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
                let arena_id = self.debug_allocator.arena_id();
                if let Some(ptr) = crate::thread_local::try_thread_local_alloc(arena_id, size, align) {
                    #[cfg(feature = "debug")]
                    {
                        let inner = unsafe { &*self.inner.get() };
                        crate::debug::register_allocation(arena_id, ptr, size, inner.current_checkpoint_id);
                    }
                    #[cfg(feature = "stats")]
                    {
                        let inner = unsafe { &*self.inner.get() };
                        inner.stats().allocation_count.fetch_add(1, Ordering::Relaxed);
                    }
                    return ptr;
                }
            }
        }

        // Try regular allocation
        let inner = unsafe { &*self.inner.get() };
        if let Some(ptr) = inner.allocate(layout) {
            #[cfg(feature = "debug")]
            {
                let arena_id = self.debug_allocator.arena_id();
                crate::debug::register_allocation(arena_id, ptr, size, inner.current_checkpoint_id);
            }
            return ptr;
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
                let inner = unsafe { &*self.inner.get() };
                if let Some(ptr) = inner.allocate(layout) {
                    #[cfg(feature = "debug")]
                    {
                        let arena_id = self.debug_allocator.arena_id();
                        crate::debug::register_allocation(arena_id, ptr, size, inner.current_checkpoint_id);
                    }
                    ptr
                } else {
                    ptr::null_mut()
                }
            }
            Err(_) => ptr::null_mut(),
        }
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

    /// Create a checkpoint for fast reset
    pub fn checkpoint(&self) -> ArenaCheckpoint {
        let inner = unsafe { &mut *self.inner.get() };
        inner.checkpoint()
    }

    /// Rewind to a checkpoint
    pub unsafe fn rewind_to_checkpoint(&self, checkpoint: ArenaCheckpoint) {
        let inner = &mut *self.inner.get();
        
        // Validate checkpoint
        assert!(checkpoint.chunk_index < inner.chunks.len(), 
                "Invalid checkpoint: chunk index out of bounds");
        assert!(checkpoint.chunk_offset <= inner.chunks[checkpoint.chunk_index].capacity(),
                "Invalid checkpoint: offset exceeds chunk capacity");
        
        // Reset current chunk and all subsequent chunks
        inner.current_chunk.store(checkpoint.chunk_index, Ordering::Release);
        for (idx, chunk) in inner.chunks.iter_mut().enumerate() {
            if idx < checkpoint.chunk_index {
                continue;
            }
            if idx == checkpoint.chunk_index {
                chunk.used.store(checkpoint.chunk_offset, Ordering::Release);
            } else {
                chunk.reset();
            }
        }
        
        // Remove checkpoints after this one
        inner.checkpoints.retain(|cp| {
            cp.chunk_index < checkpoint.chunk_index ||
            (cp.chunk_index == checkpoint.chunk_index && cp.chunk_offset <= checkpoint.chunk_offset)
        });

        // v0.5.0: Debug tracking for use-after-rewind detection
        #[cfg(feature = "debug")]
        {
            let arena_id = self.debug_allocator.arena_id();
            crate::debug::rewind_to_checkpoint(checkpoint.checkpoint_id);
            inner.current_checkpoint_id = checkpoint.checkpoint_id + 1;
        }
        
        // v0.5.0: Reset thread-local cache on rewind
        #[cfg(feature = "thread_local")]
        {
            crate::thread_local::reset_thread_cache();
        }
        
        // v0.5.0: Reset lock-free buffer on rewind
        #[cfg(feature = "lockfree")]
        {
            if let Some(ref buffer) = inner.lockfree_buffer {
                buffer.reset();
            }
        }
    }

    /// Push a checkpoint onto the stack
    pub fn push_checkpoint(&self) -> ArenaCheckpoint {
        let inner = unsafe { &mut *self.inner.get() };
        inner.push_checkpoint()
    }

    /// Pop and rewind to the last checkpoint
    pub unsafe fn pop_and_rewind(&self) -> Result<(), &'static str> {
        let inner = unsafe { &mut *self.inner.get() };
        inner.pop_and_rewind()
    }

    /// Get arena statistics
    pub fn stats(&self) -> ArenaStats {
        let inner = unsafe { &*self.inner.get() };
        #[cfg(feature = "stats")]
        {
            ArenaStats {
                bytes_used: inner.stats().bytes_used.clone(),
                allocation_count: inner.stats().allocation_count.clone(),
                chunk_count: inner.stats().chunk_count,
            }
        }
        #[cfg(not(feature = "stats"))]
        {
            ArenaStats::new()
        }
    }

    /// Get debug statistics
    #[cfg(feature = "debug")]
    pub fn debug_stats(&self) -> DebugStats {
        crate::debug::get_debug_stats()
    }

    /// Validate a pointer for use-after-rewind detection
    #[cfg(feature = "debug")]
    pub unsafe fn check_valid(&self, ptr: *mut u8) -> Result<(), &'static str> {
        self.debug_allocator.validate_pointer(ptr)
    }

    /// Validate all allocations
    #[cfg(feature = "debug")]
    pub unsafe fn validate_all_allocations(&self) -> Result<(), String> {
        let arena_id = self.debug_allocator.arena_id();
        crate::debug::validate_all_allocations(arena_id)
    }

    /// Get lock-free statistics
    #[cfg(feature = "lockfree")]
    pub fn lockfree_stats(&self) -> (usize, usize, usize, usize) {
        self.lockfree_stats.get()
    }

    /// Create a scope for RAII management
    pub fn scope<'a, F, R>(&'a self, f: F) -> R 
    where
        F: FnOnce(&'a Scope<'_, 'a>) -> R,
    {
        let scope = Scope::new(self);
        f(&scope)
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe {
            self.reset();
        }
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
        let obj = self.inner.objects.pop().unwrap_or(value);
        self.inner.stats.allocations += 1;
        self.inner.stats.peak_usage = self.inner.stats.peak_usage.max(
            self.inner.objects.len() + 1
        );
        
        Pooled {
            pool: &mut self.inner,
            value: Some(obj),
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
            self.pool.objects.push(value);
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

    pub fn with_capacity(capacity: usize) -> Result<Self, &'static str> {
        Ok(Self {
            arena: Mutex::new(Arena::with_capacity(capacity)?),
        })
    }

    pub fn alloc<T>(&self, value: T) -> MutexGuard<'_, T> {
        let arena = self.arena.lock().unwrap();
        let ptr = arena.alloc(value) as *mut T;
        unsafe {
            MutexGuard::new(arena, ptr)
        }
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
