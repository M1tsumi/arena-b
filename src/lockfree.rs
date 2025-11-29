//! Lock-free optimizations for better concurrent performance

use alloc::alloc::{alloc, dealloc, Layout};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use alloc::sync::Arc;

const LOCKFREE_BUFFER_SIZE: usize = 4096;
const LOCKFREE_ALIGNMENT: usize = 64;
const MAX_LOCKFREE_ALLOCATION: usize = 1024;

// Lock-free buffer for small allocations
pub struct LockFreeBuffer {
    buffer: AtomicPtr<u8>,
    offset: AtomicUsize,
    capacity: usize,
    stats: Arc<LockFreeStats>,
}

impl LockFreeBuffer {
    pub fn new() -> Self {
        let layout = Layout::from_size_align(LOCKFREE_BUFFER_SIZE, LOCKFREE_ALIGNMENT)
            .expect("Invalid layout for lock-free buffer");
        
        let buffer = unsafe { alloc(layout) };
        
        Self {
            buffer: AtomicPtr::new(buffer),
            offset: AtomicUsize::new(0),
            capacity: LOCKFREE_BUFFER_SIZE,
            stats: Arc::new(LockFreeStats::new()),
        }
    }

    pub fn try_alloc(&self, size: usize, align: usize) -> Option<*mut u8> {
        if size > MAX_LOCKFREE_ALLOCATION {
            return None;
        }

        let current_offset = self.offset.load(Ordering::Acquire);
        let aligned_offset = (current_offset + align - 1) & !(align - 1);
        let new_offset = aligned_offset + size;

        if new_offset > self.capacity {
            self.stats.record_cache_miss();
            self.stats.record_contention();
            return None;
        }

        match self.offset.compare_exchange_weak(
            current_offset,
            new_offset,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                let buffer_ptr = self.buffer.load(Ordering::Acquire);
                if buffer_ptr.is_null() {
                    self.stats.record_cache_miss();
                    return None;
                }
                
                let ptr = unsafe { buffer_ptr.add(aligned_offset) };
                self.stats.record_allocation();
                self.stats.record_cache_hit();
                Some(ptr)
            }
            Err(_) => {
                self.stats.record_cache_miss();
                self.stats.record_contention();
                None
            }
        }
    }

    pub fn reset(&self) {
        // Reset offset to 0
        self.offset.store(0, Ordering::Release);
        
        // Zero out the buffer for security
        let buffer_ptr = self.buffer.load(Ordering::Acquire);
        if !buffer_ptr.is_null() {
            unsafe {
                std::ptr::write_bytes(buffer_ptr, 0, self.capacity);
            }
        }
        
        // Reset stats
        self.stats.reset();
    }

    pub fn stats(&self) -> &LockFreeStats {
        &self.stats
    }

    pub fn is_full(&self) -> bool {
        self.offset.load(Ordering::Acquire) >= self.capacity
    }
}

impl Drop for LockFreeBuffer {
    fn drop(&mut self) {
        let buffer_ptr = self.buffer.load(Ordering::Acquire);
        if !buffer_ptr.is_null() {
            unsafe {
                let layout = Layout::from_size_align_unchecked(self.capacity, LOCKFREE_ALIGNMENT);
                dealloc(buffer_ptr, layout);
            }
        }
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

    pub fn try_alloc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
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
    head: AtomicPtr<u8>,
    stats: LockFreeStats,
}

struct LockFreeNode<T> {
    data: T,
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
                    
                    // Extract the data and deallocate the node
                    let data = unsafe { std::ptr::read(&node.data) };
                    unsafe {
                        let layout = Layout::new::<LockFreeNode<T>>();
                        dealloc(head as *mut u8, layout);
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
        let layout = Layout::new::<LockFreeNode<T>>();
        let node_ptr = unsafe { alloc(layout) as *mut LockFreeNode<T> };
        if node_ptr.is_null() {
            return;
        }

        let node = unsafe { &mut *node_ptr };
        node.data = data;
        node.next.store(core::ptr::null_mut(), Ordering::Relaxed);

        loop {
            let head = self.pool.head.load(Ordering::Acquire);
            node.next.store(head, Ordering::Relaxed);

            match self.pool.head.compare_exchange_weak(
                head,
                node_ptr as *mut u8,
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
                let layout = Layout::new::<LockFreeNode<T>>();
                dealloc(head as *mut u8, layout);
            }
            
            head = next;
        }
    }
}
