//! Lock-free optimizations for better concurrent performance

extern crate alloc;

use alloc::alloc::{alloc, dealloc, Layout};
use alloc::sync::Arc;
use core::cmp;
use core::mem::MaybeUninit;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::cell::Cell;

thread_local! {
    static THREAD_SLAB: Cell<ThreadSlab> = Cell::new(ThreadSlab::new());
}

const LOCKFREE_BUFFER_SIZE: usize = 4096;

/// Align a value up to the given alignment.
#[inline]
fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// Thread-local slab allocator for reduced contention.
///
/// Each thread gets its own slab region carved from the lock-free buffer,
/// enabling zero-contention allocations within that region.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct ThreadSlab {
    /// Pointer to the owning LockFreeBuffer (for validation)
    owner: *const LockFreeBuffer,
    /// Base pointer of the slab region
    base: *mut u8,
    /// Start offset within the buffer
    start: usize,
    /// End offset within the buffer (exclusive)
    end: usize,
    /// Current allocation offset within the slab
    offset: usize,
    /// Generation counter to detect stale slabs after reset
    generation: usize,
}

impl ThreadSlab {
    /// Create a new empty thread slab.
    pub fn new() -> Self {
        Self {
            owner: core::ptr::null(),
            base: core::ptr::null_mut(),
            start: 0,
            end: 0,
            offset: 0,
            generation: 0,
        }
    }

    /// Check if this slab belongs to the given buffer and generation.
    #[inline]
    pub fn matches(&self, owner: *const LockFreeBuffer, generation: usize) -> bool {
        self.owner == owner && self.generation == generation && !self.base.is_null()
    }

    /// Set the slab region from the lock-free buffer.
    pub fn set_region(
        &mut self,
        owner: *const LockFreeBuffer,
        base: *mut u8,
        start: usize,
        end: usize,
        generation: usize,
    ) {
        self.owner = owner;
        self.base = base;
        self.start = start;
        self.end = end;
        self.offset = start;
        self.generation = generation;
    }

    /// Try to allocate from the thread-local slab.
    #[inline]
    pub fn try_alloc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        if self.base.is_null() {
            return None;
        }

        let aligned_offset = align_up(self.offset, align);
        let new_offset = aligned_offset + size;

        if new_offset <= self.end {
            self.offset = new_offset;
            Some(unsafe { self.base.add(aligned_offset) })
        } else {
            None
        }
    }

    /// Invalidate this slab (called when generation changes).
    pub fn invalidate(&mut self) {
        self.owner = core::ptr::null();
        self.base = core::ptr::null_mut();
        self.start = 0;
        self.end = 0;
        self.offset = 0;
        self.generation = 0;
    }

    /// Get remaining capacity in this slab.
    #[inline]
    pub fn remaining(&self) -> usize {
        if self.base.is_null() {
            0
        } else {
            self.end.saturating_sub(self.offset)
        }
    }

    /// Check if this slab is valid and has capacity.
    #[inline]
    pub fn is_valid(&self) -> bool {
        !self.base.is_null() && self.offset < self.end
    }
}

impl Default for ThreadSlab {
    fn default() -> Self {
        Self::new()
    }
}

// Safety: ThreadSlab is only accessed from its owning thread via thread_local!
unsafe impl Send for ThreadSlab {}

const LOCKFREE_ALIGNMENT: usize = 64;
const MAX_LOCKFREE_ALLOCATION: usize = 1024;
const SLAB_MIN_BLOCK: usize = 256;

// Lock-free buffer for small allocations
#[cfg(feature = "single_thread_fast")]
pub struct LockFreeBuffer {
    buffer: Cell<*mut u8>,
    offset: Cell<usize>,
    capacity: usize,
    stats: Arc<LockFreeStats>,
    generation: Cell<usize>,
}

#[cfg(not(feature = "single_thread_fast"))]
pub struct LockFreeBuffer {
    buffer: AtomicPtr<u8>,
    offset: AtomicUsize,
    capacity: usize,
    stats: Arc<LockFreeStats>,
    generation: AtomicUsize,
}

impl LockFreeBuffer {
    pub fn new() -> Self {
        #[cfg(feature = "single_thread_fast")]
        {
            // Track the actual allocated capacity so Drop can deallocate correctly
            let (buffer_ptr, actual_capacity) = if let Ok(layout) =
                Layout::from_size_align(LOCKFREE_BUFFER_SIZE, LOCKFREE_ALIGNMENT)
            {
                (unsafe { alloc(layout) }, LOCKFREE_BUFFER_SIZE)
            } else {
                let fallback = Layout::from_size_align(LOCKFREE_ALIGNMENT, LOCKFREE_ALIGNMENT)
                    .expect("lockfree fallback layout invalid");
                (unsafe { alloc(fallback) }, LOCKFREE_ALIGNMENT)
            };

            Self {
                buffer: Cell::new(buffer_ptr),
                offset: Cell::new(0),
                capacity: actual_capacity,
                stats: Arc::new(LockFreeStats::new()),
                generation: Cell::new(0),
            }
        }

        #[cfg(not(feature = "single_thread_fast"))]
        {
            let (buffer_ptr, actual_capacity) = if let Ok(layout) =
                Layout::from_size_align(LOCKFREE_BUFFER_SIZE, LOCKFREE_ALIGNMENT)
            {
                (unsafe { alloc(layout) }, LOCKFREE_BUFFER_SIZE)
            } else {
                let fallback = Layout::from_size_align(LOCKFREE_ALIGNMENT, LOCKFREE_ALIGNMENT)
                    .expect("lockfree fallback layout invalid");
                (unsafe { alloc(fallback) }, LOCKFREE_ALIGNMENT)
            };

            Self {
                buffer: AtomicPtr::new(buffer_ptr),
                offset: AtomicUsize::new(0),
                capacity: actual_capacity,
                stats: Arc::new(LockFreeStats::new()),
                generation: AtomicUsize::new(0),
            }
        }
    }

    #[cfg(feature = "single_thread_fast")]
    pub fn allocate(&self, size: usize, align: usize) -> Option<*mut u8> {
        let current_offset = self.offset.get();
        let aligned_offset = align_up(current_offset, align);
        let new_offset = aligned_offset + size;

        if new_offset <= self.capacity {
            self.offset.set(new_offset);
            Some(unsafe { self.buffer.get().add(aligned_offset) })
        } else {
            None
        }
    }

    pub fn try_alloc(&self, size: usize, align: usize) -> Option<*mut u8> {
        if size > MAX_LOCKFREE_ALLOCATION {
            // Large allocations bypass lock-free buffer
            self.stats.record_allocation();
            self.stats.record_cache_miss();
            return None;
        }

        if let Some(ptr) = self.try_thread_slab(size, align) {
            self.stats.record_allocation();
            self.stats.record_cache_hit();
            return Some(ptr);
        }

        let result = self.refill_thread_slab_and_alloc(size, align);
        if let Some(ptr) = result {
            self.stats.record_allocation();
            self.stats.record_cache_hit();
            return Some(ptr);
        }

        // Record a cache miss for failed allocation attempts
        self.stats.record_cache_miss();
        None
    }

    pub fn reset(&self) {
        // Reset offset to 0
        #[cfg(feature = "single_thread_fast")]
        {
            self.offset.set(0);
            self.generation.set(self.generation.get() + 1);

            // Zero out the buffer for security
            let buffer_ptr = self.buffer.get();
            if !buffer_ptr.is_null() {
                unsafe {
                    std::ptr::write_bytes(buffer_ptr, 0, self.capacity);
                }
            }
        }

        #[cfg(not(feature = "single_thread_fast"))]
        {
            self.offset.store(0, Ordering::Release);
            self.generation.fetch_add(1, Ordering::AcqRel);

            let buffer_ptr = self.buffer.load(Ordering::Acquire);
            if !buffer_ptr.is_null() {
                unsafe {
                    std::ptr::write_bytes(buffer_ptr, 0, self.capacity);
                }
            }
        }

        // Reset stats
        self.stats.reset();

        // Ensure thread slabs are invalidated
        THREAD_SLAB.with(|cell| {
            let mut slab = cell.get();
            slab.invalidate();
            // Persist invalidation so subsequent calls see the cleared slab
            cell.set(slab);
        });
    }

    fn try_thread_slab(&self, size: usize, align: usize) -> Option<*mut u8> {
        let owner = self as *const _;
        #[cfg(feature = "single_thread_fast")]
        let generation = self.generation.get();
        #[cfg(not(feature = "single_thread_fast"))]
        let generation = self.generation.load(Ordering::Acquire);
        THREAD_SLAB.with(|cell| {
            let mut slab = cell.get();
            if !slab.matches(owner, generation) {
                slab.invalidate();
                cell.set(slab);
                return None;
            }
            let ptr = slab.try_alloc(size, align);
            // Write back the updated offset so the slab progresses correctly
            cell.set(slab);
            ptr
        })
    }

    fn refill_thread_slab_and_alloc(&self, size: usize, align: usize) -> Option<*mut u8> {
        #[cfg(feature = "single_thread_fast")]
        let buffer_ptr = self.buffer.get();
        #[cfg(not(feature = "single_thread_fast"))]
        let buffer_ptr = self.buffer.load(Ordering::Acquire);
        if buffer_ptr.is_null() {
            return None;
        }

        let block_size = align_up(cmp::max(size, SLAB_MIN_BLOCK), LOCKFREE_ALIGNMENT);

        loop {
            #[cfg(feature = "single_thread_fast")]
            let current = self.offset.get();
            #[cfg(not(feature = "single_thread_fast"))]
            let current = self.offset.load(Ordering::Acquire);
            let start = align_up(current, LOCKFREE_ALIGNMENT);
            let end = start + block_size;

            if end > self.capacity {
                self.stats.record_cache_miss(); // Record miss if out of capacity
                return None;
            }

            #[cfg(feature = "single_thread_fast")]
            {
                if self.offset.get() == current {
                    self.offset.set(end);
                    let generation = self.generation.get();
                    return THREAD_SLAB.with(|cell| {
                        let mut slab = cell.get();
                        slab.set_region(self as *const _, buffer_ptr, start, end, generation);
                        self.stats.record_allocation(); // Record allocation
                        let ptr = slab.try_alloc(size, align);
                        // Persist slab state so future allocations reuse it
                        cell.set(slab);
                        ptr
                    });
                } else {
                    self.stats.record_contention(); // Record contention on failure
                    continue;
                }
            }

            #[cfg(not(feature = "single_thread_fast"))]
            {
                if self
                    .offset
                    .compare_exchange(current, end, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    let generation = self.generation.load(Ordering::Acquire);
                    return THREAD_SLAB.with(|cell| {
                        let mut slab = cell.get();
                        slab.set_region(self as *const _, buffer_ptr, start, end, generation);
                        self.stats.record_allocation(); // Record allocation
                        let ptr = slab.try_alloc(size, align);
                        // Persist slab state so future allocations reuse it
                        cell.set(slab);
                        ptr
                    });
                } else {
                    self.stats.record_contention(); // Record contention on failure
                    continue;
                }
            }
        }
    }

    pub fn stats(&self) -> &LockFreeStats {
        &self.stats
    }

    pub fn is_full(&self) -> bool {
        // Check if the buffer is full or if large allocations are overwhelming
        #[cfg(feature = "single_thread_fast")]
        {
            self.offset.get() >= self.capacity
        }
        #[cfg(not(feature = "single_thread_fast"))]
        {
            self.offset.load(Ordering::Acquire) >= self.capacity
        }
    }
}

impl Drop for LockFreeBuffer {
    fn drop(&mut self) {
        #[cfg(feature = "single_thread_fast")]
        let buffer_ptr = self.buffer.get();
        #[cfg(not(feature = "single_thread_fast"))]
        let buffer_ptr = self.buffer.load(Ordering::Acquire);
        if !buffer_ptr.is_null() {
            unsafe {
                if self.capacity > 0 {
                    if let Ok(layout) = Layout::from_size_align(self.capacity, LOCKFREE_ALIGNMENT) {
                        dealloc(buffer_ptr, layout);
                    } else {
                        let fallback =
                            Layout::from_size_align(LOCKFREE_ALIGNMENT, LOCKFREE_ALIGNMENT)
                                .expect("lockfree fallback layout invalid");
                        dealloc(buffer_ptr, fallback);
                    }
                }
            }
        }
    }
}

impl Default for LockFreeBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// Lock-free statistics tracking
#[derive(Debug)]
pub struct LockFreeStats {
    allocations: AtomicUsize,
    cache_hits: AtomicUsize,
    cache_misses: AtomicUsize,
    contention_events: AtomicUsize,
}

impl LockFreeStats {
    pub fn new() -> Self {
        Self {
            allocations: AtomicUsize::new(0),
            cache_hits: AtomicUsize::new(0),
            cache_misses: AtomicUsize::new(0),
            contention_events: AtomicUsize::new(0),
        }
    }

    pub fn record_allocation(&self) {
        self.allocations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_contention(&self) {
        self.contention_events.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_deallocation(&self) {
        self.allocations.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn get(&self) -> (usize, usize, usize, usize) {
        (
            self.allocations.load(Ordering::Relaxed),
            self.cache_hits.load(Ordering::Relaxed),
            self.cache_misses.load(Ordering::Relaxed),
            self.contention_events.load(Ordering::Relaxed),
        )
    }

    pub fn get_stats(&self) -> (usize, usize, usize, usize) {
        self.get()
    }

    pub fn reset(&self) {
        self.allocations.store(0, Ordering::Relaxed);
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
        self.contention_events.store(0, Ordering::Relaxed);
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let total = hits + self.cache_misses.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }
}

impl Clone for LockFreeStats {
    fn clone(&self) -> Self {
        Self {
            allocations: AtomicUsize::new(self.allocations.load(Ordering::Relaxed)),
            cache_hits: AtomicUsize::new(self.cache_hits.load(Ordering::Relaxed)),
            cache_misses: AtomicUsize::new(self.cache_misses.load(Ordering::Relaxed)),
            contention_events: AtomicUsize::new(self.contention_events.load(Ordering::Relaxed)),
        }
    }
}

impl Default for LockFreeStats {
    fn default() -> Self {
        Self::new()
    }
}

// Lock-free allocation strategy
pub struct LockFreeAllocator {
    buffer: Option<LockFreeBuffer>,
    stats: LockFreeStats,
    enabled: bool,
}

impl LockFreeAllocator {
    pub fn new() -> Self {
        Self {
            buffer: Some(LockFreeBuffer::new()),
            stats: LockFreeStats::new(),
            enabled: true,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn try_alloc(&self, size: usize, align: usize) -> Option<*mut u8> {
        if !self.enabled || size > MAX_LOCKFREE_ALLOCATION {
            return None;
        }

        if let Some(ref buffer) = self.buffer {
            buffer.try_alloc(size, align)
        } else {
            None
        }
    }

    pub fn reset(&mut self) {
        if let Some(ref buffer) = self.buffer {
            buffer.reset();
        }
        self.stats.reset();
    }

    pub fn stats(&self) -> (usize, usize, usize, usize) {
        if let Some(ref buffer) = self.buffer {
            buffer.stats().get()
        } else {
            self.stats.get()
        }
    }

    pub fn cache_hit_rate(&self) -> f64 {
        if let Some(ref buffer) = self.buffer {
            buffer.stats().cache_hit_rate()
        } else {
            0.0
        }
    }
}

impl Default for LockFreeAllocator {
    fn default() -> Self {
        Self::new()
    }
}

// Lock-free pool for reusable allocations
pub struct LockFreePool<T> {
    pool: Arc<LockFreePoolInner<T>>,
}

struct LockFreePoolInner<T> {
    head: AtomicPtr<LockFreeNode<T>>,
    stats: LockFreeStats,
}

#[repr(C)]
struct LockFreeNode<T> {
    data: MaybeUninit<T>,
    next: AtomicPtr<LockFreeNode<T>>,
}

impl<T> LockFreePool<T> {
    pub fn new() -> Self {
        Self {
            pool: Arc::new(LockFreePoolInner {
                head: AtomicPtr::new(core::ptr::null_mut()),
                stats: LockFreeStats::new(),
            }),
        }
    }

    pub fn try_alloc(&self) -> Option<T> {
        let head = self.pool.head.load(Ordering::Acquire);
        if head.is_null() {
            self.pool.stats.record_cache_miss();
            return None;
        }

        loop {
            let head = self.pool.head.load(Ordering::Acquire);
            if head.is_null() {
                self.pool.stats.record_cache_miss();
                return None;
            }

            let node = unsafe { &*head };
            let next = node.next.load(Ordering::Acquire);

            match self.pool.head.compare_exchange_weak(
                head,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.pool.stats.record_allocation();
                    self.pool.stats.record_cache_hit();

                    // Extract the data and free the node memory safely via Box
                    let data = unsafe { std::ptr::read(node.data.as_ptr()) };
                    unsafe {
                        // Convert the raw pointer back to a Box to free memory
                        let _boxed: Box<LockFreeNode<T>> = Box::from_raw(head);
                    }
                    return Some(data);
                }
                Err(_) => {
                    self.pool.stats.record_contention();
                    continue;
                }
            }
        }
    }

    pub fn dealloc(&self, data: T) {
        // Allocate a new node using Box to ensure correct construction and
        // destructor behavior for `T`.
        let boxed = Box::new(LockFreeNode {
            data: MaybeUninit::new(data),
            next: AtomicPtr::new(core::ptr::null_mut()),
        });
        let node_ptr = Box::into_raw(boxed);

        loop {
            let head = self.pool.head.load(Ordering::Acquire);
            unsafe {
                (*node_ptr).next.store(head, Ordering::Relaxed);
            }

            match self.pool.head.compare_exchange_weak(
                head,
                node_ptr,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.pool.stats.record_deallocation();
                    return;
                }
                Err(_) => {
                    self.pool.stats.record_contention();
                    continue;
                }
            }
        }
    }

    pub fn stats(&self) -> (usize, usize, usize, usize) {
        self.pool.stats.get()
    }
}

impl<T> Default for LockFreePool<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for LockFreePoolInner<T> {
    fn drop(&mut self) {
        // Deallocate all remaining nodes
        let mut head = self.head.load(Ordering::Acquire);
        while !head.is_null() {
            let node = unsafe { &*head };
            let next = node.next.load(Ordering::Acquire);
            unsafe {
                // Reconstruct the Box to free memory and drop the stored value.
                let boxed: Box<LockFreeNode<T>> = Box::from_raw(head);
                // SAFETY: `data` was stored as `MaybeUninit<T>`; we must drop it
                // properly if it's initialized. Attempt to read and drop it.
                let data_ptr = boxed.data.as_ptr();
                let _ = std::ptr::read(data_ptr);
                // `boxed` destructor will free the node memory (data already moved out)
            }

            head = next;
        }
    }
}
