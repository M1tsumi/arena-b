#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unused_imports)]
#![allow(clippy::legacy_numeric_constants)]
#![allow(clippy::unwrap_or_default)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::let_and_return)]

use std::alloc::{alloc, dealloc, Layout};
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::mem::{self, MaybeUninit};
use std::ops::{Deref, DerefMut};
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex, RwLock,
};
use std::usize;
use std::vec::Vec;

// Re-export main types from arena module
pub use arena::{Arena, ArenaBuilder, ArenaStats, Scope};

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

// v0.5.0: Virtual memory strategy
#[cfg(feature = "virtual_memory")]
pub mod virtual_memory;

// v0.5.0: Thread-local caching for reduced contention
#[cfg(feature = "thread_local")]
pub mod thread_local;

// v0.5.0: Lock-free optimizations for reduced contention
#[cfg(feature = "lockfree")]
pub mod lockfree;

#[cfg(feature = "debug")]
pub mod debug;

const CHUNK_ALIGN: usize = 64;
const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;
const MIN_CHUNK_SIZE: usize = 4096;
const MAX_CHUNK_SIZE: usize = 16 * 1024 * 1024;
const ALIGNMENT_MASK: usize = CHUNK_ALIGN - 1;
const SIMD_THRESHOLD: usize = 1024;

// Fast-path optimizations
const FAST_ALLOC_THRESHOLD: usize = 1024; // Fast path for small allocations
const PREFETCH_WARMUP_SIZE: usize = 8; // Number of cache lines to prefetch

// Size classes for optimized allocation
const SIZE_CLASSES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

// Custom memory pool for frequently used sizes
#[repr(align(64))]
struct MemoryPool {
    pools: [Vec<NonNull<u8>>; SIZE_CLASSES.len()],
}

impl MemoryPool {
    fn new() -> Self {
        Self {
            pools: std::array::from_fn(|_| Vec::new()),
        }
    }

    fn get_pool_index(&self, size: usize) -> Option<usize> {
        SIZE_CLASSES
            .iter()
            .position(|&class_size| size <= class_size)
    }

    unsafe fn alloc(&mut self, size: usize) -> Option<NonNull<u8>> {
        if let Some(pool_idx) = self.get_pool_index(size) {
            if let Some(ptr) = self.pools[pool_idx].pop() {
                return Some(ptr);
            }
        }
        None
    }

    unsafe fn dealloc(&mut self, ptr: NonNull<u8>, size: usize) {
        if let Some(pool_idx) = self.get_pool_index(size) {
            // Limit pool size to prevent memory bloat
            if self.pools[pool_idx].len() < 64 {
                self.pools[pool_idx].push(ptr);
            } else {
                // Pool is full, actually deallocate
                let layout = Layout::from_size_align(size, 8).unwrap();
                std::alloc::dealloc(ptr.as_ptr(), layout);
            }
        }
    }
}

// Cache-line aware atomic operations
#[repr(align(64))]
struct AtomicCounter {
    _padding: [u8; 64],
}

// v0.5.0: Debug guard structures
#[cfg(feature = "debug")]
#[repr(C)]
struct DebugGuard {
    magic_before: u64,
    ptr: *mut u8,
    size: usize,
    checkpoint_id: u64,
    magic_after: u64,
}

#[cfg(feature = "debug")]
impl DebugGuard {
    fn new(ptr: *mut u8, size: usize, checkpoint_id: u64) -> Self {
        Self {
            magic_before: debug::GUARD_MAGIC,
            ptr,
            size,
            checkpoint_id,
            magic_after: debug::GUARD_MAGIC,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.magic_before != debug::GUARD_MAGIC || self.magic_after != debug::GUARD_MAGIC {
            Err("Debug guard corruption detected".to_string())
        } else {
            Ok(())
        }
    }
}

#[repr(C, align(64))]
struct Chunk {
    ptr: NonNull<u8>,
    capacity: usize,
    used: AtomicUsize,
    _padding: [u8; 64 - 3 * 8], // Cache line padding
}

// v0.5.0: Virtual memory chunk
#[cfg(feature = "virtual_memory")]
#[repr(C, align(64))]
struct VirtualChunk {
    region: UnsafeCell<virtual_memory::VirtualMemoryRegion>,
    capacity: usize,
    used: AtomicUsize,
    _padding: [u8; 64 - 3 * 8], // Cache line padding
}

#[cfg(feature = "virtual_memory")]
impl VirtualChunk {
    fn new(reserve_size: usize) -> Result<Self, &'static str> {
        let region = virtual_memory::VirtualMemoryRegion::new(reserve_size)?;
        Ok(Self {
            region: UnsafeCell::new(region),
            capacity: reserve_size,
            used: AtomicUsize::new(0),
            _padding: [0; 64 - 3 * 8],
        })
    }

    unsafe fn allocate(&self, size: usize, align: usize) -> Option<*mut u8> {
        let current = self.used.load(Ordering::Acquire);
        let aligned = (current + align - 1) & !(align - 1);
        let end = aligned + size;

        if end <= self.capacity {
            if self
                .used
                .compare_exchange_weak(current, end, Ordering::Release, Ordering::Relaxed)
                .is_ok()
            {
                let region = &mut *self.region.get();
                // Commit memory if needed
                if end > region.committed_size {
                    if region.commit(end - region.committed_size).is_ok() {
                        return Some(region.ptr.add(aligned));
                    } else {
                        // Rollback on failure
                        self.used.store(current, Ordering::Release);
                        return None;
                    }
                }
                return Some(region.ptr.add(aligned));
            }
        }
        None
    }

    fn reset(&self) {
        let region = unsafe { &mut *self.region.get() };
        region.reset();
        self.used.store(0, Ordering::Release);
    }
}

#[repr(C, align(64))]
struct ArenaInner {
    chunks: Vec<Chunk>,
    current_chunk: AtomicUsize,
    total_allocated: AtomicCounter,
    checkpoints: Vec<ArenaCheckpoint>, // v0.5.0: Fast reset checkpoints
    #[cfg(feature = "debug")]
    current_checkpoint_id: u64, // v0.5.0: Debug tracking
    #[cfg(feature = "virtual_memory")]
    virtual_chunk: Option<VirtualChunk>, // v0.5.0: Virtual memory chunk
    #[cfg(feature = "lockfree")]
    lockfree_buffer: Option<lockfree::LockFreeBuffer>, // v0.5.0: Lock-free allocation buffer
    _padding: [u8; 64 - 2 * 8 - 8 - 8], // Cache line padding
}

/// v0.5.0: Arena checkpoint for fast reset functionality
#[derive(Copy, Clone, Debug)]
pub struct ArenaCheckpoint {
    chunk_index: usize,
    chunk_offset: usize,
    allocation_count: usize,
    bytes_used: usize,
    #[cfg(feature = "debug")]
    checkpoint_id: u64, // v0.5.0: Debug tracking
}

/// Debug statistics for the arena when debug feature is enabled.
#[cfg(feature = "debug")]
#[derive(Debug, Clone)]
pub struct DebugStats {
    /// Total number of allocations being tracked
    pub total_allocations: usize,
    /// Number of active checkpoints
    pub active_checkpoints: usize,
    /// Current checkpoint ID
    pub current_checkpoint_id: u64,
    /// Number of corrupted allocations detected
    pub corrupted_allocations: usize,
}

pub mod arena;

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

impl<'scope, 'arena> Scope<'scope, 'arena>
where
    'arena: 'scope,
{
    pub fn alloc<T>(&'scope self, value: T) -> &'scope mut T {
        self.arena.alloc(value)
    }

    pub fn alloc_default<T: Default>(&'scope self) -> &'scope mut T {
        self.arena.alloc_default::<T>()
    }

    pub fn alloc_slice_copy<T: Copy>(&'scope self, slice: &[T]) -> &'scope [T] {
        self.arena.alloc_slice_copy(slice)
    }

    pub fn alloc_slice_uninit<T>(&'scope self, len: usize) -> &'scope mut [MaybeUninit<T>] {
        self.arena.alloc_slice_uninit(len)
    }

    pub fn alloc_str(&'scope self, s: &str) -> &'scope str {
        self.arena.alloc_str(s)
    }
}

/// Statistics for pool usage
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub capacity: usize,
    pub in_use: usize,
    pub free: usize,
}

struct PoolInner<T> {
    storage: Vec<Option<T>>,
    free: Vec<usize>,
    in_use: usize,
}

pub struct Pool<T> {
    inner: UnsafeCell<PoolInner<T>>,
}

pub struct Pooled<'pool, T> {
    index: usize,
    pool: &'pool Pool<T>,
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Pool<T> {
    #[inline]
    pub fn new() -> Self {
        Self::with_capacity(0)
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        let mut storage = Vec::with_capacity(capacity);
        storage.resize_with(capacity, || None);
        let mut free = Vec::with_capacity(capacity);
        for i in 0..capacity {
            free.push(i);
        }
        Pool {
            inner: UnsafeCell::new(PoolInner {
                storage,
                free,
                in_use: 0,
            }),
        }
    }

    pub fn alloc<'pool>(&'pool self, value: T) -> Pooled<'pool, T> {
        unsafe {
            let inner = &mut *self.inner.get();
            let index = if let Some(i) = inner.free.pop() {
                i
            } else {
                let idx = inner.storage.len();
                inner.storage.push(None);
                idx
            };
            debug_assert!(inner.storage[index].is_none());
            inner.storage[index] = Some(value);
            inner.in_use += 1;
            Pooled { index, pool: self }
        }
    }

    #[inline]
    pub fn alloc_default<'pool>(&'pool self) -> Pooled<'pool, T>
    where
        T: Default,
    {
        self.alloc(T::default())
    }

    #[inline]
    pub fn stats(&self) -> PoolStats {
        unsafe {
            let inner = &*self.inner.get();
            PoolStats {
                capacity: inner.storage.len(),
                in_use: inner.in_use,
                free: inner.free.len(),
            }
        }
    }

    #[inline]
    fn put_back(&self, index: usize) {
        unsafe {
            let inner = &mut *self.inner.get();
            if inner.storage[index].is_some() {
                inner.storage[index] = None;
                inner.in_use -= 1;
                inner.free.push(index);
            }
        }
    }
}

impl<'pool, T> std::ops::Deref for Pooled<'pool, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe {
            let inner = &*self.pool.inner.get();
            inner.storage[self.index]
                .as_ref()
                .expect("pooled slot empty")
        }
    }
}

impl<'pool, T> std::ops::DerefMut for Pooled<'pool, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            let inner = &mut *self.pool.inner.get();
            inner.storage[self.index]
                .as_mut()
                .expect("pooled slot empty")
        }
    }
}

impl<'pool, T> Drop for Pooled<'pool, T> {
    fn drop(&mut self) {
        self.pool.put_back(self.index);
    }
}

pub struct SyncArena {
    inner: Mutex<Arena>,
}

impl Default for SyncArena {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncArena {
    pub fn new() -> Self {
        SyncArena {
            inner: Mutex::new(Arena::new()),
        }
    }

    pub fn with_capacity(bytes: usize) -> Self {
        SyncArena {
            inner: Mutex::new(Arena::with_capacity(bytes).expect("Failed to create arena")),
        }
    }

    pub fn scope<F, R>(&self, f: F) -> R
    where
        F: for<'scope, 'arena> FnOnce(&crate::arena::Scope<'scope, 'arena>) -> R,
    {
        let guard = self.inner.lock().unwrap();
        guard.scope(f)
    }

    pub fn stats(&self) -> ArenaStats {
        let guard = self.inner.lock().unwrap();
        guard.stats()
    }

    pub fn bytes_allocated(&self) -> usize {
        let guard = self.inner.lock().unwrap();
        guard.bytes_allocated()
    }
}

impl ArenaBuilder {
    pub fn initial_capacity(mut self, bytes: usize) -> Self {
        self.initial_capacity = bytes;
        self
    }

    pub fn chunk_size(mut self, bytes: usize) -> Self {
        self.chunk_size = bytes;
        self
    }

    pub fn thread_safe(mut self, enabled: bool) -> Self {
        self.thread_safe = enabled;
        self
    }

    pub fn build(self) -> Arena {
        let capacity = if self.initial_capacity == 0 {
            DEFAULT_CHUNK_SIZE
        } else {
            self.initial_capacity
        };

        // thread_safe is currently ignored; a non-thread-safe Arena is always built.
        Arena::with_capacity(capacity).expect("Failed to create arena")
    }
}
