//! Thread-local caching for reduced contention

extern crate alloc;

use alloc::alloc::{alloc, dealloc, Layout};
use core::cell::RefCell;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashMap;

const THREAD_CACHE_SIZE: usize = 512;
const CACHE_ALIGNMENT: usize = 64;

// Thread-local cache entry
#[repr(C)]
#[derive(Copy, Clone)]
struct CacheEntry {
    ptr: *mut u8,
    size: usize,
    arena_id: usize,
}

// Thread-local cache structure
thread_local! {
    static THREAD_CACHE: RefCell<ThreadCache> = RefCell::new(ThreadCache::new());
}

struct ThreadCache {
    entries: [CacheEntry; 16], // 16 cache entries
    used: usize,
    total_bytes: usize,
    arena_id: usize,
}

impl ThreadCache {
    fn new() -> Self {
        Self {
            entries: [CacheEntry {
                ptr: core::ptr::null_mut(),
                size: 0,
                arena_id: 0,
            }; 16],
            used: 0,
            total_bytes: 0,
            arena_id: 0,
        }
    }

    fn alloc(&mut self, size: usize, arena_id: usize) -> Option<*mut u8> {
        if self.arena_id != arena_id {
            self.clear();
            self.arena_id = arena_id;
        }

        // Find a suitable entry
        for i in 0..self.used {
            if self.entries[i].size >= size && self.entries[i].arena_id == arena_id {
                let ptr = self.entries[i].ptr;
                // Remove entry
                self.entries[i] = self.entries[self.used - 1];
                self.used -= 1;
                self.total_bytes -= size;
                return Some(ptr);
            }
        }

        None
    }

    fn dealloc(&mut self, ptr: *mut u8, size: usize, arena_id: usize) {
        if self.arena_id != arena_id {
            self.clear();
            self.arena_id = arena_id;
        }

        if self.used >= self.entries.len() || self.total_bytes + size > THREAD_CACHE_SIZE {
            // Cache is full, clear some entries
            self.clear_partial();
        }

        if self.used < self.entries.len() && self.total_bytes + size <= THREAD_CACHE_SIZE {
            self.entries[self.used] = CacheEntry {
                ptr,
                size,
                arena_id,
            };
            self.used += 1;
            self.total_bytes += size;
        }
    }

    fn clear(&mut self) {
        for i in 0..self.used {
            // Deallocate all cached pointers (skipped)
            let size = self.entries[i].size;
            if size > 0 {
                // Do not deallocate cached pointers here. Cached entries
                // may point inside arena chunks or other owners and
                // freeing them can cause double-free. Skip deallocation
                // to avoid freeing memory we don't own.
            }
        }
        self.used = 0;
        self.total_bytes = 0;
    }

    fn clear_partial(&mut self) {
        // Clear half the entries
        let clear_count = self.used / 2;
        for i in 0..clear_count {
            let size = self.entries[i].size;
            if size > 0 {
                // See note in clear(): do not free cached pointers here.
            }
        }

        // Move remaining entries
        for i in clear_count..self.used {
            self.entries[i - clear_count] = self.entries[i];
        }

        self.used -= clear_count;
        self.total_bytes /= 2; // Approximate
    }
}

// Public interface for thread-local caching
pub fn try_thread_local_alloc(arena_id: usize, size: usize, _align: usize) -> Option<*mut u8> {
    THREAD_CACHE.with(|cache| cache.borrow_mut().alloc(size, arena_id))
}

pub fn thread_local_dealloc(ptr: *mut u8, size: usize, arena_id: usize) {
    THREAD_CACHE.with(|cache| {
        cache.borrow_mut().dealloc(ptr, size, arena_id);
    })
}

pub fn reset_thread_cache() {
    THREAD_CACHE.with(|cache| {
        cache.borrow_mut().clear();
    });
}

// Backwards-compatible alias used by tests/examples
pub fn clear_thread_cache() {
    reset_thread_cache();
}

// Per-arena thread-local cache management
pub struct ThreadLocalCache {
    arena_id: usize,
    enabled: bool,
}

impl ThreadLocalCache {
    pub fn new(arena_id: usize) -> Self {
        Self {
            arena_id,
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
        if !self.enabled || size > 512 {
            return None;
        }

        try_thread_local_alloc(self.arena_id, size, align)
    }

    pub fn dealloc(&self, ptr: *mut u8, size: usize) {
        if !self.enabled || size > 512 {
            return;
        }

        thread_local_dealloc(ptr, size, self.arena_id);
    }

    pub fn reset(&self) {
        if self.enabled {
            reset_thread_cache();
        }
    }
}

// Statistics for thread-local caching
#[derive(Debug, Default)]
pub struct ThreadLocalStats {
    pub cache_hits: AtomicUsize,
    pub cache_misses: AtomicUsize,
    pub cache_allocations: AtomicUsize,
    pub cache_deallocations: AtomicUsize,
}

impl ThreadLocalStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_allocation(&self) {
        self.cache_allocations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_deallocation(&self) {
        self.cache_deallocations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get(&self) -> (usize, usize, usize, usize) {
        (
            self.cache_hits.load(Ordering::Relaxed),
            self.cache_misses.load(Ordering::Relaxed),
            self.cache_allocations.load(Ordering::Relaxed),
            self.cache_deallocations.load(Ordering::Relaxed),
        )
    }
}

// Clear thread cache when arena is dropped
pub fn cleanup_thread_cache(arena_id: usize) {
    THREAD_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.arena_id == arena_id {
            cache.clear();
        }
    });
}
