//! Core arena allocation functionality

extern crate alloc;

use alloc::alloc::{alloc, dealloc, Layout};
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::{self, MaybeUninit};
use core::ptr::{self, NonNull};
use core::slice;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::usize;
use std::sync::Mutex;

#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

// Constants
const MIN_CHUNK_SIZE: usize = 64;
const MAX_CHUNK_SIZE: usize = 16 * 1024 * 1024; // 16MB
const ALIGNMENT_MASK: usize = 63; // 64-byte alignment - 1
const PREFETCH_DISTANCE: usize = 8;
const PREFETCH_WARMUP_SIZE: usize = 64;

// Memory pool for small allocations
pub struct MemoryPool {
    slots: Vec<NonNull<u8>>,
    size_class: usize,
    capacity: usize,
}

impl MemoryPool {
    pub fn new(size_class: usize, capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            size_class,
            capacity,
        }
    }

    pub fn alloc(&mut self) -> Option<NonNull<u8>> {
        self.slots.pop()
    }

    pub fn dealloc(&mut self, ptr: NonNull<u8>) {
        if self.slots.len() < self.capacity {
            self.slots.push(ptr);
        }
    }

    pub fn size_class(&self) -> usize {
        self.size_class
    }
}

// Atomic counter for statistics
pub struct AtomicCounter {
    value: AtomicUsize,
}

impl std::fmt::Debug for AtomicCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AtomicCounter({})",
            self.value.load(std::sync::atomic::Ordering::Relaxed)
        )
    }
}

impl AtomicCounter {
    pub fn new(value: usize) -> Self {
        Self {
            value: AtomicUsize::new(value),
        }
    }

    pub fn load(&self, ordering: Ordering) -> usize {
        self.value.load(ordering)
    }

    pub fn store(&self, value: usize, ordering: Ordering) {
        self.value.store(value, ordering);
    }

    pub fn fetch_add(&self, value: usize, ordering: Ordering) -> usize {
        self.value.fetch_add(value, ordering)
    }
}

impl PartialEq<usize> for AtomicCounter {
    fn eq(&self, other: &usize) -> bool {
        self.load(Ordering::Acquire) == *other
    }
}

impl PartialEq for AtomicCounter {
    fn eq(&self, other: &Self) -> bool {
        self.load(Ordering::Acquire) == other.load(Ordering::Acquire)
    }
}

impl PartialOrd<usize> for AtomicCounter {
    fn partial_cmp(&self, other: &usize) -> Option<std::cmp::Ordering> {
        self.load(Ordering::Acquire).partial_cmp(other)
    }
}

impl std::fmt::Display for AtomicCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.load(Ordering::Acquire))
    }
}

// Chunk for arena allocations
#[repr(C)]
pub struct Chunk {
    ptr: NonNull<u8>,
    capacity: usize,
    used: AtomicUsize,
}

impl Chunk {
    pub fn new(capacity: usize) -> Result<Self, &'static str> {
        if capacity == 0 {
            return Err("Capacity must be nonzero");
        }
        // Round up to next multiple of 64 for alignment
        let capacity = (capacity + 63) & !63;
        let layout = Layout::from_size_align(capacity, 64).map_err(|_| "Invalid layout")?;

        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            return Err("Failed to allocate memory");
        }

        Ok(Self {
            ptr: unsafe { NonNull::new_unchecked(ptr) },
            capacity,
            used: AtomicUsize::new(0),
        })
    }

    pub fn allocate(&self, layout: Layout) -> Option<*mut u8> {
        let size = layout.size();
        let align = layout.align();
        loop {
            let current_used = self.used.load(Ordering::Acquire);
            let start = (current_used + align - 1) & !(align - 1);
            let end = start.checked_add(size)?;

            if end > self.capacity {
                return None;
            }

            if self
                .used
                .compare_exchange_weak(current_used, end, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                unsafe {
                    let ptr = self.ptr.as_ptr().add(start);
                    // Prefetch the allocated memory
                    #[cfg(target_arch = "x86_64")]
                    if size >= PREFETCH_WARMUP_SIZE {
                        _mm_prefetch(ptr as *const i8, _MM_HINT_T0);
                    }
                    return Some(ptr);
                }
            }

            core::hint::spin_loop();
        }
    }

    pub fn reset(&self) {
        self.used.store(0, Ordering::Release);
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn ptr(&self) -> NonNull<u8> {
        self.ptr
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }

    pub fn set_used(&self, value: usize) {
        self.used.store(value, Ordering::Release);
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        unsafe {
            if self.capacity > 0 {
                if let Ok(layout) = Layout::from_size_align(self.capacity, 64) {
                    dealloc(self.ptr.as_ptr(), layout);
                } else {
                    // Fallback: deallocate using a minimal layout
                    let fallback =
                        Layout::from_size_align(64, 64).expect("64-byte fallback layout invalid");
                    dealloc(self.ptr.as_ptr(), fallback);
                }
            }
        }
    }
}

// Arena checkpoint for fast reset
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArenaCheckpoint {
    pub chunk_index: usize,
    pub chunk_offset: usize,
    pub checkpoint_id: usize,
    pub allocation_count: usize,
}

// Debug statistics
#[derive(Debug, Default, Clone)]
pub struct DebugStats {
    pub total_allocations: usize,
    pub active_checkpoints: usize,
    pub current_checkpoint_id: usize,
    pub corrupted_allocations: usize,
    /// Number of leak reports generated
    pub leak_reports: usize,
}

// Arena statistics
pub struct ArenaStats {
    pub bytes_used: AtomicCounter,
    pub bytes_allocated: AtomicCounter,
    pub allocation_count: AtomicCounter,
    pub chunk_count: usize,
}

impl std::fmt::Debug for ArenaStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArenaStats")
            .field("bytes_used", &self.bytes_used)
            .field("allocation_count", &self.allocation_count)
            .field("chunk_count", &self.chunk_count)
            .finish()
    }
}

impl ArenaStats {
    pub fn new() -> Self {
        Self {
            bytes_used: AtomicCounter::new(0),
            bytes_allocated: AtomicCounter::new(0),
            allocation_count: AtomicCounter::new(0),
            chunk_count: 0,
        }
    }
    pub fn bytes_used(&self) -> usize {
        self.bytes_used.load(Ordering::Acquire)
    }

    pub fn allocation_count(&self) -> usize {
        self.allocation_count.load(Ordering::Acquire)
    }
}

impl Default for ArenaStats {
    fn default() -> Self {
        Self::new()
    }
}

// Arena inner structure
pub struct ArenaInner {
    pub chunks: Vec<Chunk>,
    pub current_chunk: AtomicUsize,
    pub checkpoints: Vec<ArenaCheckpoint>,
    pub current_checkpoint_id: usize,
    pub pools: Vec<MemoryPool>,
    #[cfg(feature = "stats")]
    pub stats: ArenaStats,
    #[cfg(feature = "virtual_memory")]
    pub virtual_region: Option<crate::virtual_memory::VirtualMemoryRegion>,
    #[cfg(feature = "thread_local")]
    pub thread_cache_active: bool,
    #[cfg(feature = "lockfree")]
    pub lockfree_buffer: Option<crate::lockfree::LockFreeBuffer>,
    #[cfg(feature = "lockfree")]
    pub lockfree_stats: crate::lockfree::LockFreeStats,
}

impl ArenaInner {
    pub fn new(initial_capacity: usize) -> Result<Self, &'static str> {
        let chunk = Chunk::new(initial_capacity)?;

        let mut pools = Vec::new();
        let mut size = 8;
        while size <= 4096 {
            pools.push(MemoryPool::new(size, 64));
            size *= 2;
        }

        let mut inner = Self {
            chunks: vec![chunk],
            current_chunk: AtomicUsize::new(0),
            checkpoints: Vec::new(),
            current_checkpoint_id: 0,
            pools,
            #[cfg(feature = "stats")]
            stats: ArenaStats::new(),
            #[cfg(feature = "virtual_memory")]
            virtual_region: None,
            #[cfg(feature = "thread_local")]
            thread_cache_active: true,
            #[cfg(feature = "lockfree")]
            lockfree_buffer: Some(crate::lockfree::LockFreeBuffer::new()),
            #[cfg(feature = "lockfree")]
            lockfree_stats: crate::lockfree::LockFreeStats::new(),
        };

        #[cfg(feature = "stats")]
        {
            inner
                .stats
                .bytes_allocated
                .store(initial_capacity, Ordering::Relaxed);
            inner.stats.chunk_count = inner.chunks.len();
        }

        Ok(inner)
    }

    pub fn allocate(&mut self, layout: Layout) -> Option<*mut u8> {
        let size = layout.size();

        // Try current chunk (use safe clamped index)
        let current_chunk_idx = self.current_chunk.load(Ordering::Acquire);
        let chunk_idx = if current_chunk_idx >= self.chunks.len() {
            self.chunks.len().saturating_sub(1)
        } else {
            current_chunk_idx
        };

        if let Some(chunk) = self.chunks.get(chunk_idx) {
            if let Some(ptr) = chunk.allocate(layout) {
                #[cfg(feature = "stats")]
                {
                    self.stats.bytes_used.fetch_add(size, Ordering::Relaxed);
                    self.stats.allocation_count.fetch_add(1, Ordering::Relaxed);
                }
                return Some(ptr);
            }
        }

        // Need new chunk
        None
    }

    pub fn add_chunk(&mut self, capacity: usize) -> Result<usize, &'static str> {
        let chunk = Chunk::new(capacity)?;
        let chunk_index = self.chunks.len();
        self.chunks.push(chunk);
        self.current_chunk.store(chunk_index, Ordering::Release);
        #[cfg(feature = "stats")]
        {
            self.stats.chunk_count = self.chunks.len();
            self.stats
                .bytes_allocated
                .fetch_add(capacity, Ordering::Relaxed);
        }
        Ok(chunk_index)
    }

    pub fn reset(&mut self) {
        for chunk in &mut self.chunks {
            chunk.reset();
        }
        self.current_chunk.store(0, Ordering::Release);
        self.checkpoints.clear();
        self.current_checkpoint_id = 0;

        #[cfg(feature = "stats")]
        {
            self.stats.bytes_used.store(0, Ordering::Release);
            self.stats.allocation_count.store(0, Ordering::Release);
        }

        #[cfg(feature = "thread_local")]
        {
            crate::thread_local::reset_thread_cache();
        }

        #[cfg(feature = "lockfree")]
        {
            if let Some(ref buffer) = self.lockfree_buffer {
                buffer.reset();
            }
        }
    }

    pub fn checkpoint(&mut self) -> ArenaCheckpoint {
        let current_chunk_idx = self.current_chunk.load(Ordering::Acquire);
        let current_chunk_idx = if current_chunk_idx >= self.chunks.len() {
            self.chunks.len().saturating_sub(1)
        } else {
            current_chunk_idx
        };
        let current_chunk = &self.chunks[current_chunk_idx];
        let chunk_offset = current_chunk.used();

        #[cfg(feature = "stats")]
        let alloc_count = self.stats.allocation_count.load(Ordering::Acquire);
        #[cfg(not(feature = "stats"))]
        let alloc_count = 0;

        let checkpoint = ArenaCheckpoint {
            chunk_index: current_chunk_idx,
            chunk_offset,
            checkpoint_id: self.current_checkpoint_id,
            allocation_count: alloc_count,
        };

        self.current_checkpoint_id += 1;

        checkpoint
    }

    pub fn rewind_to_checkpoint(&mut self, checkpoint: ArenaCheckpoint) {
        // Validate checkpoint
        assert!(
            checkpoint.chunk_index < self.chunks.len(),
            "Invalid checkpoint: chunk index out of bounds"
        );
        assert!(
            checkpoint.chunk_offset <= self.chunks[checkpoint.chunk_index].capacity(),
            "Invalid checkpoint: offset exceeds chunk capacity"
        );

        // Reset current chunk and all subsequent chunks
        self.current_chunk
            .store(checkpoint.chunk_index, Ordering::Release);
        for (idx, chunk) in self.chunks.iter_mut().enumerate() {
            if idx < checkpoint.chunk_index {
                continue;
            }
            if idx == checkpoint.chunk_index {
                unsafe {
                    let used_ptr = &mut chunk.used as *mut AtomicUsize;
                    (*used_ptr).store(checkpoint.chunk_offset, Ordering::Release);
                }
            } else {
                chunk.reset();
            }
        }

        // Keep only checkpoints that happened before this rewind target.
        self.checkpoints
            .retain(|cp| cp.checkpoint_id < checkpoint.checkpoint_id);

        // Update debug tracking
        #[cfg(feature = "debug")]
        {
            crate::debug::rewind_to_checkpoint(checkpoint.checkpoint_id);
            self.current_checkpoint_id = checkpoint.checkpoint_id + 1;
        }

        // Reset thread-local cache
        #[cfg(feature = "thread_local")]
        {
            crate::thread_local::reset_thread_cache();
        }

        // Reset lock-free buffer
        #[cfg(feature = "lockfree")]
        {
            if let Some(ref buffer) = self.lockfree_buffer {
                buffer.reset();
            }
        }

        #[cfg(feature = "stats")]
        {
            let mut bytes_used = 0;
            for chunk in self.chunks.iter().take(checkpoint.chunk_index + 1) {
                bytes_used += chunk.used();
            }
            self.stats.bytes_used.store(bytes_used, Ordering::Release);
            // bytes_allocated tracks total allocated chunk capacity; keep it unchanged here
            self.stats
                .allocation_count
                .store(checkpoint.allocation_count, Ordering::Release);
        }
    }

    pub fn push_checkpoint(&mut self) -> ArenaCheckpoint {
        let checkpoint = self.checkpoint();
        self.checkpoints.push(checkpoint);
        checkpoint
    }

    pub fn pop_and_rewind(&mut self) -> Result<(), &'static str> {
        if let Some(checkpoint) = self.checkpoints.pop() {
            self.rewind_to_checkpoint(checkpoint);
            Ok(())
        } else {
            Err("No checkpoint to pop")
        }
    }

    #[cfg(feature = "stats")]
    pub fn stats(&self) -> &ArenaStats {
        &self.stats
    }

    #[cfg(feature = "debug")]
    pub fn debug_stats(&self) -> DebugStats {
        crate::debug::get_debug_stats()
    }

    #[cfg(feature = "lockfree")]
    pub fn lockfree_stats(&self) -> (usize, usize, usize, usize) {
        self.lockfree_stats.get()
    }
}

// Arena builder
pub struct ArenaBuilder {
    initial_capacity: usize,
}

impl ArenaBuilder {
    pub fn new() -> Self {
        Self {
            initial_capacity: 4096,
        }
    }

    pub fn initial_capacity(mut self, capacity: usize) -> Self {
        self.initial_capacity = capacity;
        self
    }

    pub fn build(self) -> crate::Arena {
        crate::Arena::with_capacity(self.initial_capacity)
    }
}

impl Default for ArenaBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// Scope for RAII arena management
pub struct Scope<'scope, 'arena> {
    arena: &'arena mut crate::Arena,
    _phantom: PhantomData<&'scope ()>,
}

impl<'scope, 'arena> Scope<'scope, 'arena> {
    pub fn new(arena: &'arena mut crate::Arena) -> Self {
        Self {
            arena,
            _phantom: PhantomData,
        }
    }
}

impl<'scope, 'arena> Drop for Scope<'scope, 'arena> {
    fn drop(&mut self) {
        unsafe {
            self.arena.reset();
        }
    }
}
