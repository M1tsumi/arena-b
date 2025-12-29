#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(unused_imports)]
#![allow(clippy::legacy_numeric_constants)]
#![allow(clippy::unwrap_or_default)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::let_and_return)]
#![allow(clippy::collapsible_else_if)]

use cfg_if::cfg_if;
use std::alloc::{alloc, dealloc, Layout};
#[cfg(feature = "single_thread_fast")]
use std::cell::Cell;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::mem::{self, MaybeUninit};
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use std::vec::Vec;

// Declare external modules
#[cfg(feature = "arena_module")]
pub mod arena;
pub mod core;
pub mod size_classes;

/// Diagnostics sink type used for optional diagnostic callbacks
pub type DiagnosticsSink = Box<dyn Fn(&str) + Send + Sync + 'static>;

pub mod error;

#[cfg(feature = "lockfree")]
pub mod lockfree;

#[cfg(feature = "thread_local")]
pub mod thread_local;

#[cfg(feature = "virtual_memory")]
pub mod virtual_memory;

#[cfg(feature = "debug")]
pub mod debug;

#[cfg(feature = "slab")]
pub mod slab;

// Re-export core types
pub use crate::core::{ArenaCheckpoint, ArenaStats, AtomicCounter, Chunk, DebugStats, MemoryPool};

// Re-export Arena from the appropriate module
#[cfg(feature = "arena_module")]
pub use crate::arena::{Arena, ArenaBuilder, Scope};

// Re-export crate error type for consumers
pub use crate::error::ArenaError;

#[cfg(not(feature = "arena_module"))]
pub use self::legacy_arena::{Arena, ArenaBuilder, FeatureBundle, Scope};

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

// Legacy arena module for when arena_module feature is not enabled
mod legacy_arena;

// Constants
const CHUNK_ALIGN: usize = 64;
const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;
const MIN_CHUNK_SIZE: usize = 4096;
const MAX_CHUNK_SIZE: usize = 16 * 1024 * 1024;
const ALIGNMENT_MASK: usize = CHUNK_ALIGN - 1;
const SIMD_THRESHOLD: usize = 1024;

cfg_if! {
    if #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))] {
        const HAS_NATIVE_SIMD: bool = true;
    } else {
        const HAS_NATIVE_SIMD: bool = false;
    }
}

// Fast-path optimizations
const FAST_ALLOC_THRESHOLD: usize = 1024; // Fast path for small allocations
const PREFETCH_WARMUP_SIZE: usize = 8; // Number of cache lines to prefetch

use size_classes::SIZE_CLASSES;

#[repr(transparent)]
struct UsedCounter {
    #[cfg(not(feature = "single_thread_fast"))]
    inner: AtomicUsize,
    #[cfg(feature = "single_thread_fast")]
    inner: Cell<usize>,
}

impl UsedCounter {
    #[inline]
    fn new(value: usize) -> Self {
        Self {
            #[cfg(not(feature = "single_thread_fast"))]
            inner: AtomicUsize::new(value),
            #[cfg(feature = "single_thread_fast")]
            inner: Cell::new(value),
        }
    }

    #[inline]
    fn load(&self, ordering: Ordering) -> usize {
        #[cfg(not(feature = "single_thread_fast"))]
        {
            self.inner.load(ordering)
        }
        #[cfg(feature = "single_thread_fast")]
        {
            self.inner.get()
        }
    }

    #[inline]
    fn store(&self, value: usize, ordering: Ordering) {
        #[cfg(not(feature = "single_thread_fast"))]
        {
            self.inner.store(value, ordering);
        }
        #[cfg(feature = "single_thread_fast")]
        {
            self.inner.set(value);
        }
    }

    #[inline]
    fn fetch_add(&self, value: usize, ordering: Ordering) -> usize {
        #[cfg(not(feature = "single_thread_fast"))]
        {
            self.inner.fetch_add(value, ordering)
        }
        #[cfg(feature = "single_thread_fast")]
        {
            let current = self.inner.get();
            self.inner.set(current + value);
            current
        }
    }
}

#[inline]
fn align_up(offset: usize, align: usize) -> usize {
    (offset + align - 1) & !(align - 1)
}

// Branch prediction hint for common case
#[inline]
fn likely(b: bool) -> bool {
    // Note: std::intrinsics::likely is unstable, so we use a simple hint
    // The compiler will optimize this based on profiling data
    b
}

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

#[cfg(not(feature = "single_thread_fast"))]
pub struct SyncArena {
    inner: Mutex<Arena>,
}

#[cfg(not(feature = "single_thread_fast"))]
impl Default for SyncArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "single_thread_fast"))]
impl SyncArena {
    pub fn new() -> Self {
        SyncArena {
            inner: Mutex::new(Arena::new()),
        }
    }

    pub fn with_capacity(bytes: usize) -> Self {
        SyncArena {
            inner: Mutex::new(Arena::with_capacity(bytes)),
        }
    }

    pub fn scope<F, R>(&self, f: F) -> R
    where
        F: for<'scope, 'arena> FnOnce(&Scope<'scope, 'arena>) -> R,
    {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.scope(f)
    }

    pub fn stats(&self) -> crate::core::ArenaStats {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        crate::core::ArenaStats {
            bytes_used: crate::core::AtomicCounter::new(guard.stats().bytes_used),
            bytes_allocated: crate::core::AtomicCounter::new(guard.stats().bytes_allocated),
            allocation_count: crate::core::AtomicCounter::new(guard.stats().allocation_count),
            chunk_count: guard.stats().chunk_count,
        }
    }

    pub fn bytes_allocated(&self) -> usize {
        let guard = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.stats().bytes_allocated
    }
}

// ============================================================================
// v0.8.0: Public API Exports
// ============================================================================

/// Re-export lock-free types when the feature is enabled.
#[cfg(feature = "lockfree")]
pub use lockfree::{LockFreeAllocator, LockFreeBuffer, LockFreePool, LockFreeStats, ThreadSlab};

/// Re-export thread-local cache types when the feature is enabled.
#[cfg(feature = "thread_local")]
pub use thread_local::{
    cleanup_thread_cache, clear_thread_cache, reset_thread_cache, try_thread_local_alloc,
};

/// Re-export virtual memory types when the feature is enabled.
#[cfg(feature = "virtual_memory")]
pub use virtual_memory::VirtualMemoryRegion;

/// Re-export debug types when the feature is enabled.
#[cfg(feature = "debug")]
pub use debug::{AllocationInfo, DEBUG_STATE, FREED_MAGIC, GUARD_MAGIC};
