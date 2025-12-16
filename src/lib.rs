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

mod size_classes;

#[cfg(feature = "slab")]
mod slab;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

// v0.5.0: Virtual memory strategy
#[cfg(feature = "virtual_memory")]
mod virtual_memory {
    use super::*;

    #[cfg(windows)]
    use std::ffi::OsStr;
    #[cfg(windows)]
    use std::os::windows::ffi::OsStrExt;

    #[cfg(unix)]
    use std::os::unix::ffi::OsStrExt;

    const PAGE_SIZE: usize = 4096;
    const DEFAULT_RESERVE_SIZE: usize = 16 * 1024 * 1024; // 16MB
    const DEFAULT_COMMIT_SIZE: usize = 64 * 1024; // 64KB

    // Virtual memory region using reserve/commit pattern
    pub struct VirtualMemoryRegion {
        pub ptr: *mut u8,
        pub reserved_size: usize,
        pub committed_size: usize,
    }

    impl VirtualMemoryRegion {
        pub fn new(reserve_size: usize) -> Result<Self, &'static str> {
            let reserve_size = (reserve_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

            let ptr = unsafe {
                #[cfg(windows)]
                {
                    use std::os::windows::io::AsRawHandle;
                    use windows_sys::Win32::System::Memory::{
                        VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
                    };

                    VirtualAlloc(
                        std::ptr::null_mut(),
                        reserve_size,
                        MEM_RESERVE,
                        PAGE_READWRITE,
                    )
                }

                #[cfg(unix)]
                {
                    libc::mmap(
                        std::ptr::null_mut(),
                        reserve_size,
                        libc::PROT_NONE,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                        -1,
                        0,
                    )
                }
            };

            if ptr.is_null() {
                return Err("Failed to reserve virtual memory");
            }

            Ok(Self {
                ptr: ptr as *mut u8,
                reserved_size: reserve_size,
                committed_size: 0,
            })
        }

        pub fn commit(&mut self, size: usize) -> Result<*mut u8, &'static str> {
            let commit_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            let new_committed = self.committed_size + commit_size;

            if new_committed > self.reserved_size {
                return Err("Cannot commit beyond reserved size");
            }

            unsafe {
                #[cfg(windows)]
                {
                    use windows_sys::Win32::System::Memory::{
                        VirtualAlloc, MEM_COMMIT, PAGE_READWRITE,
                    };

                    let commit_ptr = VirtualAlloc(
                        self.ptr.add(self.committed_size) as *mut _,
                        commit_size,
                        MEM_COMMIT,
                        PAGE_READWRITE,
                    );

                    if commit_ptr.is_null() {
                        return Err("Failed to commit virtual memory");
                    }
                }

                #[cfg(unix)]
                {
                    let result = libc::mprotect(
                        self.ptr.add(self.committed_size) as *mut libc::c_void,
                        commit_size,
                        libc::PROT_READ | libc::PROT_WRITE,
                    );

                    if result != 0 {
                        return Err("Failed to change memory protection");
                    }
                }
            }

            self.committed_size = new_committed;
            unsafe { Ok(self.ptr.add(self.committed_size - commit_size)) }
        }

        pub fn decommit(&mut self, offset: usize, size: usize) {
            let decommit_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

            unsafe {
                #[cfg(windows)]
                {
                    use windows_sys::Win32::System::Memory::{VirtualFree, MEM_DECOMMIT};

                    VirtualFree(self.ptr.add(offset) as *mut _, decommit_size, MEM_DECOMMIT);
                }

                #[cfg(unix)]
                {
                    libc::mprotect(
                        self.ptr.add(offset) as *mut libc::c_void,
                        decommit_size,
                        libc::PROT_NONE,
                    );
                }
            }
        }

        pub fn reset(&mut self) {
            if self.committed_size > 0 {
                self.decommit(0, self.committed_size);
                self.committed_size = 0;
            }
        }

        pub fn committed_bytes(&self) -> usize {
            self.committed_size
        }
    }

    impl Drop for VirtualMemoryRegion {
        fn drop(&mut self) {
            unsafe {
                #[cfg(windows)]
                {
                    use windows_sys::Win32::System::Memory::{VirtualFree, MEM_RELEASE};

                    VirtualFree(self.ptr as *mut _, 0, MEM_RELEASE);
                }

                #[cfg(unix)]
                {
                    libc::munmap(self.ptr as *mut _, self.reserved_size);
                }
            }
        }
    }

    unsafe impl Send for VirtualMemoryRegion {}
    unsafe impl Sync for VirtualMemoryRegion {}
}

// v0.5.0: Thread-local caching for reduced contention
#[cfg(feature = "thread_local")]
mod thread_local_cache {
    use super::*;
    use std::cell::RefCell;
    use std::thread::LocalKey;

    const THREAD_CACHE_SIZE: usize = 1024; // 1KB per thread
    const MAX_THREAD_CACHE_SIZE: usize = 4096; // 4KB max per thread

    // Thread-local allocation cache
    thread_local! {
        static THREAD_CACHE: RefCell<ThreadCache> = RefCell::new(ThreadCache::new());
    }

    #[repr(C, align(64))]
    struct ThreadCache {
        buffer: *mut u8,
        capacity: usize,
        used: usize,
        arena_id: usize,
    }

    impl ThreadCache {
        fn new() -> Self {
            Self {
                buffer: std::ptr::null_mut(),
                capacity: 0,
                used: 0,
                arena_id: 0,
            }
        }

        fn reset(&mut self) {
            if !self.buffer.is_null() {
                unsafe {
                    std::alloc::dealloc(
                        self.buffer,
                        std::alloc::Layout::from_size_align(self.capacity, 64).unwrap(),
                    );
                }
                self.buffer = std::ptr::null_mut();
                self.capacity = 0;
                self.used = 0;
            }
        }

        fn ensure_capacity(&mut self, arena_id: usize) -> bool {
            if self.arena_id != arena_id {
                self.reset();
                self.arena_id = arena_id;
            }

            if self.buffer.is_null() {
                let layout = std::alloc::Layout::from_size_align(THREAD_CACHE_SIZE, 64).unwrap();
                self.buffer = unsafe { std::alloc::alloc(layout) };
                if self.buffer.is_null() {
                    std::alloc::handle_alloc_error(layout);
                }
                self.capacity = THREAD_CACHE_SIZE;
                self.used = 0;
                true
            } else {
                true
            }
        }

        fn try_alloc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
            if self.buffer.is_null() || self.capacity == 0 {
                return None;
            }

            let current = self.used;
            let aligned = (current + align - 1) & !(align - 1);
            let end = aligned + size;

            if end <= self.capacity {
                self.used = end;
                Some(unsafe { self.buffer.add(aligned) })
            } else {
                None
            }
        }

        fn grow(&mut self) -> bool {
            if self.capacity >= MAX_THREAD_CACHE_SIZE {
                return false;
            }

            let new_capacity = (self.capacity * 2).min(MAX_THREAD_CACHE_SIZE);
            let new_layout = std::alloc::Layout::from_size_align(new_capacity, 64).unwrap();

            let new_buffer = if self.buffer.is_null() {
                unsafe { std::alloc::alloc(new_layout) }
            } else {
                let old_layout = std::alloc::Layout::from_size_align(self.capacity, 64).unwrap();
                unsafe { std::alloc::realloc(self.buffer, old_layout, new_capacity) }
            };

            if new_buffer.is_null() {
                std::alloc::handle_alloc_error(new_layout);
            }

            self.buffer = new_buffer;
            self.capacity = new_capacity;
            true
        }

        fn reset_usage(&mut self) {
            self.used = 0;
        }
    }

    impl Drop for ThreadCache {
        fn drop(&mut self) {
            self.reset();
        }
    }

    // Public interface for thread-local caching
    pub fn try_thread_local_alloc(arena_id: usize, size: usize, align: usize) -> Option<*mut u8> {
        THREAD_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.ensure_capacity(arena_id) {
                // Try to allocate from current cache
                if let Some(ptr) = cache.try_alloc(size, align) {
                    return Some(ptr);
                }

                // If that fails, try to grow the cache and retry
                if cache.grow() {
                    cache.try_alloc(size, align)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    pub fn reset_thread_cache() {
        THREAD_CACHE.with(|cache| {
            cache.borrow_mut().reset_usage();
        });
    }

    pub fn clear_thread_cache() {
        THREAD_CACHE.with(|cache| {
            cache.borrow_mut().reset();
        });
    }
}

// v0.5.0: Lock-free optimizations for reduced contention
#[cfg(feature = "lockfree")]
mod lockfree {
    use super::*;
    use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

    // Lock-free per-thread allocation buffer
    #[repr(C, align(64))]
    pub struct LockFreeBuffer {
        ptr: AtomicPtr<u8>,
        capacity: AtomicUsize,
        used: AtomicUsize,
        next: AtomicPtr<LockFreeBuffer>,
    }

    impl LockFreeBuffer {
        pub fn new(capacity: usize) -> Self {
            let layout = std::alloc::Layout::from_size_align(capacity, 64).unwrap();
            let ptr = unsafe { std::alloc::alloc(layout) };
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }

            Self {
                ptr: AtomicPtr::new(ptr),
                capacity: AtomicUsize::new(capacity),
                used: AtomicUsize::new(0),
                next: AtomicPtr::new(std::ptr::null_mut()),
            }
        }

        pub fn try_alloc(&self, size: usize, align: usize) -> Option<*mut u8> {
            let current = self.used.load(Ordering::Acquire);
            let aligned = (current + align - 1) & !(align - 1);
            let end = aligned + size;
            let capacity = self.capacity.load(Ordering::Acquire);

            if end <= capacity {
                // Try to atomically claim the space
                if self
                    .used
                    .compare_exchange_weak(current, end, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    let ptr = self.ptr.load(Ordering::Acquire);
                    Some(unsafe { ptr.add(aligned) })
                } else {
                    None
                }
            } else {
                None
            }
        }

        pub fn reset(&self) {
            self.used.store(0, Ordering::Release);
        }

        pub fn is_full(&self, size: usize, align: usize) -> bool {
            let current = self.used.load(Ordering::Acquire);
            let aligned = (current + align - 1) & !(align - 1);
            let end = aligned + size;
            let capacity = self.capacity.load(Ordering::Acquire);
            end > capacity
        }
    }

    impl Drop for LockFreeBuffer {
        fn drop(&mut self) {
            let ptr = self.ptr.load(Ordering::Acquire);
            let capacity = self.capacity.load(Ordering::Acquire);
            if !ptr.is_null() && capacity > 0 {
                unsafe {
                    let layout = std::alloc::Layout::from_size_align(capacity, 64).unwrap();
                    std::alloc::dealloc(ptr, layout);
                }
            }
        }
    }

    unsafe impl Send for LockFreeBuffer {}
    unsafe impl Sync for LockFreeBuffer {}

    // Lock-free allocation statistics
    #[repr(C, align(64))]
    pub struct LockFreeStats {
        allocations: AtomicUsize,
        cache_hits: AtomicUsize,
        cache_misses: AtomicUsize,
        contention_count: AtomicUsize,
    }

    impl LockFreeStats {
        pub fn new() -> Self {
            Self {
                allocations: AtomicUsize::new(0),
                cache_hits: AtomicUsize::new(0),
                cache_misses: AtomicUsize::new(0),
                contention_count: AtomicUsize::new(0),
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
            self.contention_count.fetch_add(1, Ordering::Relaxed);
        }

        pub fn get_stats(&self) -> (usize, usize, usize, usize) {
            (
                self.allocations.load(Ordering::Relaxed),
                self.cache_hits.load(Ordering::Relaxed),
                self.cache_misses.load(Ordering::Relaxed),
                self.contention_count.load(Ordering::Relaxed),
            )
        }

        pub fn record_deallocation(&self) {
            self.allocations.fetch_sub(1, Ordering::Relaxed);
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
                contention_count: AtomicUsize::new(self.contention_count.load(Ordering::Relaxed)),
            }
        }
    }

    impl Default for LockFreeStats {
        fn default() -> Self {
            Self::new()
        }
    }

    unsafe impl Send for LockFreeStats {}
    unsafe impl Sync for LockFreeStats {}

    // v0.8.0: Thread-local slab allocator for reduced contention
    /// Thread-local slab allocator for reduced contention.
    ///
    /// Each thread gets its own slab region carved from the lock-free buffer,
    /// enabling zero-contention allocations within that region.
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
                owner: std::ptr::null(),
                base: std::ptr::null_mut(),
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

            let aligned_offset = (self.offset + align - 1) & !(align - 1);
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
            self.owner = std::ptr::null();
            self.base = std::ptr::null_mut();
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
    }

    impl Default for ThreadSlab {
        fn default() -> Self {
            Self::new()
        }
    }

    unsafe impl Send for ThreadSlab {}

    // v0.8.0: Lock-free allocator wrapper with runtime enable/disable
    /// High-level lock-free allocator with runtime enable/disable control.
    pub struct LockFreeAllocator {
        buffer: Option<LockFreeBuffer>,
        stats: LockFreeStats,
        enabled: bool,
    }

    impl LockFreeAllocator {
        pub fn new() -> Self {
            Self {
                buffer: Some(LockFreeBuffer::new(4096)),
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
            if !self.enabled || size > 1024 {
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
        }

        pub fn stats(&self) -> (usize, usize, usize, usize) {
            self.stats.get_stats()
        }

        pub fn cache_hit_rate(&self) -> f64 {
            self.stats.cache_hit_rate()
        }
    }

    impl Default for LockFreeAllocator {
        fn default() -> Self {
            Self::new()
        }
    }

    // v0.8.0: Generic lock-free object pool
    /// A thread-safe, lock-free pool allocator for reusable objects.
    pub struct LockFreePool<T> {
        head: AtomicPtr<LockFreeNode<T>>,
        stats: LockFreeStats,
    }

    struct LockFreeNode<T> {
        data: T,
        next: AtomicPtr<LockFreeNode<T>>,
    }

    impl<T> LockFreePool<T> {
        pub fn new() -> Self {
            Self {
                head: AtomicPtr::new(std::ptr::null_mut()),
                stats: LockFreeStats::new(),
            }
        }

        /// Try to get an object from the pool.
        pub fn try_alloc(&self) -> Option<T> {
            loop {
                let head = self.head.load(Ordering::Acquire);
                if head.is_null() {
                    self.stats.record_cache_miss();
                    return None;
                }

                let node = unsafe { &*head };
                let next = node.next.load(Ordering::Acquire);

                match self.head.compare_exchange_weak(
                    head,
                    next,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        self.stats.record_allocation();
                        self.stats.record_cache_hit();

                        let data = unsafe { std::ptr::read(&node.data) };
                        unsafe {
                            let layout = std::alloc::Layout::new::<LockFreeNode<T>>();
                            std::alloc::dealloc(head as *mut u8, layout);
                        }
                        return Some(data);
                    }
                    Err(_) => {
                        self.stats.record_contention();
                        continue;
                    }
                }
            }
        }

        /// Return an object to the pool for reuse.
        pub fn dealloc(&self, data: T) {
            let layout = std::alloc::Layout::new::<LockFreeNode<T>>();
            let node_ptr = unsafe { std::alloc::alloc(layout) as *mut LockFreeNode<T> };
            if node_ptr.is_null() {
                return;
            }

            unsafe {
                std::ptr::write(&mut (*node_ptr).data, data);
                (*node_ptr).next = AtomicPtr::new(std::ptr::null_mut());
            }

            loop {
                let head = self.head.load(Ordering::Acquire);
                unsafe {
                    (*node_ptr).next.store(head, Ordering::Relaxed);
                }

                match self.head.compare_exchange_weak(
                    head,
                    node_ptr,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => {
                        self.stats.record_deallocation();
                        return;
                    }
                    Err(_) => {
                        self.stats.record_contention();
                        continue;
                    }
                }
            }
        }

        pub fn stats(&self) -> (usize, usize, usize, usize) {
            self.stats.get_stats()
        }
    }

    impl<T> Default for LockFreePool<T> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<T> Drop for LockFreePool<T> {
        fn drop(&mut self) {
            let mut head = self.head.load(Ordering::Acquire);
            while !head.is_null() {
                let node = unsafe { &*head };
                let next = node.next.load(Ordering::Acquire);

                unsafe {
                    std::ptr::drop_in_place(&mut (*head).data);
                    let layout = std::alloc::Layout::new::<LockFreeNode<T>>();
                    std::alloc::dealloc(head as *mut u8, layout);
                }

                head = next;
            }
        }
    }

    unsafe impl<T: Send> Send for LockFreePool<T> {}
    unsafe impl<T: Send> Sync for LockFreePool<T> {}
}

#[cfg(feature = "debug")]
mod debug {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{LazyLock, RwLock};

    // Magic values for debug guards
    pub const GUARD_MAGIC: u64 = 0xDEADBEEFCAFEBABE;
    pub const FREED_MAGIC: u64 = 0xFEEDFACECAFEBABE;

    // Debug allocation metadata
    #[derive(Debug, Clone)]
    pub struct AllocationInfo {
        pub ptr: *mut u8,
        pub size: usize,
        pub checkpoint_id: u64,
        pub magic: u64,
    }

    unsafe impl Send for AllocationInfo {}
    unsafe impl Sync for AllocationInfo {}

    // Global debug state - per-arena approach
    pub static DEBUG_STATE: LazyLock<RwLock<DebugState>> =
        LazyLock::new(|| RwLock::new(DebugState::new()));

    #[derive(Debug)]
    pub struct DebugState {
        // Map from arena pointer to its allocation tracking
        // Use usize as key instead of raw pointer to avoid alignment issues
        pub allocations: HashMap<usize, HashMap<usize, AllocationInfo>>,
        pub current_checkpoint_ids: HashMap<usize, u64>,
    }

    unsafe impl Send for DebugState {}
    unsafe impl Sync for DebugState {}

    impl Default for DebugState {
        fn default() -> Self {
            Self::new()
        }
    }

    impl DebugState {
        pub fn new() -> Self {
            Self {
                allocations: HashMap::new(),
                current_checkpoint_ids: HashMap::new(),
            }
        }

        pub fn register_allocation(
            &mut self,
            arena_id: usize,
            ptr: *mut u8,
            size: usize,
            checkpoint_id: u64,
        ) {
            let arena_allocations = self
                .allocations
                .entry(arena_id)
                .or_insert_with(HashMap::new);
            let info = AllocationInfo {
                ptr,
                size,
                checkpoint_id,
                magic: GUARD_MAGIC,
            };
            arena_allocations.insert(ptr as usize, info);
        }

        pub fn check_use_after_rewind(
            &self,
            arena_id: usize,
            ptr: *const u8,
        ) -> Result<(), String> {
            if let Some(arena_allocations) = self.allocations.get(&arena_id) {
                if let Some(info) = arena_allocations.get(&(ptr as usize)) {
                    if info.magic != GUARD_MAGIC {
                        return Err(format!("Use after rewind detected at {:p}", ptr));
                    }
                    Ok(())
                } else {
                    Err(format!("Unknown allocation at {:p}", ptr))
                }
            } else {
                Err(format!("Unknown arena at {:p}", arena_id as *const ()))
            }
        }

        pub fn rewind_to_checkpoint(&mut self, arena_id: usize, checkpoint_id: u64) {
            self.current_checkpoint_ids
                .insert(arena_id, checkpoint_id + 1);

            if let Some(arena_allocations) = self.allocations.get_mut(&arena_id) {
                // Mark all allocations after this checkpoint as freed
                arena_allocations.retain(|_, info| {
                    if info.checkpoint_id <= checkpoint_id {
                        info.magic == GUARD_MAGIC
                    } else {
                        info.magic = FREED_MAGIC;
                        false
                    }
                });
            }
        }

        pub fn get_current_checkpoint_id(&self, arena_id: usize) -> u64 {
            self.current_checkpoint_ids
                .get(&arena_id)
                .copied()
                .unwrap_or(1)
        }

        pub fn get_stats(&self, arena_id: usize) -> (usize, usize) {
            if let Some(arena_allocations) = self.allocations.get(&arena_id) {
                let total = arena_allocations.len();
                let corrupted = arena_allocations
                    .values()
                    .filter(|info| info.magic != GUARD_MAGIC)
                    .count();
                (total, corrupted)
            } else {
                (0, 0)
            }
        }
    }

    pub fn rewind_arena_to_checkpoint(arena_id: usize, checkpoint_id: u64) {
        if let Ok(mut state) = DEBUG_STATE.write() {
            state.rewind_to_checkpoint(arena_id, checkpoint_id);
        }
    }
}

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

#[cfg(feature = "slab")]
use slab::SlabAllocator as MemoryPool;

// Custom memory pool for frequently used sizes
#[cfg(not(feature = "slab"))]
#[repr(align(64))]
struct MemoryPool {
    pools: [Vec<NonNull<u8>>; SIZE_CLASSES.len()],
}

#[cfg(not(feature = "slab"))]
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

    fn committed_bytes(&self) -> usize {
        let region = unsafe { &*self.region.get() };
        region.committed_bytes()
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
    /// Number of leak reports generated
    pub leak_reports: usize,
}

pub struct Arena {
    inner: UnsafeCell<ArenaInner>,
    #[cfg(feature = "stats")]
    stats: AtomicStats,
    #[cfg(feature = "lockfree")]
    lockfree_stats: lockfree::LockFreeStats, // v0.5.0: Lock-free statistics
    memory_pool: UnsafeCell<MemoryPool>,
    _padding: [u8; 64], // Cache line padding to avoid false sharing
}

#[repr(align(64))]
struct AtomicStats {
    bytes_used: AtomicUsize,
    allocation_count: AtomicUsize,
    _padding: [u8; 64 - 2 * 8],
}

/// Statistics describing arena usage.
///
/// These values are cheap to query and can be used for debugging and
/// performance tuning.
pub struct ArenaStats {
    pub bytes_allocated: usize,
    pub bytes_used: usize,
    pub allocation_count: usize,
    pub chunk_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArenaChunkUsage {
    pub capacity: usize,
    pub used: usize,
}

pub struct ArenaBuilder {
    initial_capacity: usize,
    chunk_size: usize,
    thread_safe: bool,
}

pub struct Scope<'scope, 'arena> {
    arena: &'arena Arena,
    _marker: PhantomData<&'scope mut ()>,
}

struct ScopeResetGuard<'a> {
    arena: &'a Arena,
    start_chunk: usize,
    start_used: usize,
    #[cfg(feature = "stats")]
    start_bytes_used: usize,
    #[cfg(feature = "stats")]
    start_allocation_count: usize,
}

impl<'a> ScopeResetGuard<'a> {
    unsafe fn capture(arena: &'a Arena) -> Self {
        let inner = &mut *arena.inner.get();
        let idx = inner.current_chunk.load(Ordering::Acquire);
        let used = inner.chunks[idx].used.load(Ordering::Acquire);

        Self {
            arena,
            start_chunk: idx,
            start_used: used,
            #[cfg(feature = "stats")]
            start_bytes_used: arena.stats.bytes_used.load(Ordering::Acquire),
            #[cfg(feature = "stats")]
            start_allocation_count: arena.stats.allocation_count.load(Ordering::Acquire),
        }
    }
}

impl Drop for ScopeResetGuard<'_> {
    fn drop(&mut self) {
        unsafe {
            #[cfg(feature = "stats")]
            {
                self.arena
                    .stats
                    .bytes_used
                    .store(self.start_bytes_used, Ordering::Release);
                self.arena
                    .stats
                    .allocation_count
                    .store(self.start_allocation_count, Ordering::Release);
            }

            let inner = &mut *self.arena.inner.get();
            for (idx, chunk) in inner.chunks.iter_mut().enumerate() {
                if idx < self.start_chunk {
                    continue;
                }
                if idx == self.start_chunk {
                    chunk.used.store(self.start_used, Ordering::Release);
                } else {
                    chunk.used.store(0, Ordering::Release);
                }
            }
            inner
                .current_chunk
                .store(self.start_chunk, Ordering::Release);
        }
    }
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

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Arena {
    const DEFAULT_CAPACITY: usize = DEFAULT_CHUNK_SIZE;

    /// Creates a new arena with the default capacity (64KB).
    ///
    /// This is a convenient shorthand for [`Arena::with_capacity`].
    #[inline]
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn builder() -> ArenaBuilder {
        ArenaBuilder {
            initial_capacity: Self::DEFAULT_CAPACITY,
            chunk_size: Self::DEFAULT_CAPACITY,
            thread_safe: false,
        }
    }

    /// Creates a new arena with virtual memory strategy.
    ///
    /// This method uses a reserve/commit pattern for better memory efficiency
    /// when the arena grows large. Available only when the "virtual_memory" feature is enabled.
    ///
    /// # Panics
    ///
    /// Panics if the virtual memory reservation fails.
    #[cfg(feature = "virtual_memory")]
    pub fn with_virtual_memory(reserve_size: usize) -> Self {
        assert!(reserve_size > 0);

        let first_chunk = unsafe { allocate_chunk(DEFAULT_CHUNK_SIZE) };
        let virtual_chunk =
            VirtualChunk::new(reserve_size).expect("Failed to create virtual memory chunk");

        let inner = ArenaInner {
            chunks: vec![first_chunk],
            current_chunk: AtomicUsize::new(0),
            total_allocated: AtomicCounter { _padding: [0; 64] },
            checkpoints: Vec::new(),
            #[cfg(feature = "debug")]
            current_checkpoint_id: 1,
            virtual_chunk: Some(virtual_chunk),
            #[cfg(feature = "lockfree")]
            lockfree_buffer: Some(lockfree::LockFreeBuffer::new(4096)), // v0.5.0: Initialize lock-free buffer
            _padding: [0; 64 - 2 * 8 - 8 - 8],
        };

        Arena {
            inner: UnsafeCell::new(inner),
            #[cfg(feature = "stats")]
            stats: AtomicStats {
                bytes_used: AtomicUsize::new(0),
                allocation_count: AtomicUsize::new(0),
                _padding: [0; 64 - 2 * 8],
            },
            #[cfg(feature = "lockfree")]
            lockfree_stats: lockfree::LockFreeStats::new(), // v0.5.0: Initialize lock-free stats
            memory_pool: UnsafeCell::new(MemoryPool::new()),
            _padding: [0; 64],
        }
    }

    /// Creates a new arena with a specific initial capacity in bytes.
    ///
    /// # Panics
    ///
    /// Panics if `bytes` is zero or if the allocation fails.
    #[inline]
    pub fn with_capacity(bytes: usize) -> Self {
        assert!(bytes > 0);
        // For compatibility with tests, use exact capacity for small sizes
        let capacity = (bytes + ALIGNMENT_MASK) & !ALIGNMENT_MASK;
        let first_chunk = unsafe { allocate_chunk(capacity) };
        let inner = ArenaInner {
            chunks: vec![first_chunk],
            current_chunk: AtomicUsize::new(0),
            total_allocated: AtomicCounter { _padding: [0; 64] },
            checkpoints: Vec::new(), // v0.5.0: Initialize checkpoints
            #[cfg(feature = "debug")]
            current_checkpoint_id: 1, // v0.5.0: Start with checkpoint ID 1
            #[cfg(feature = "virtual_memory")]
            virtual_chunk: None, // v0.5.0: Initialize virtual chunk as None
            #[cfg(feature = "lockfree")]
            lockfree_buffer: Some(lockfree::LockFreeBuffer::new(4096)), // v0.5.0: Initialize lock-free buffer
            _padding: [0; 64 - 2 * 8 - 8 - 8],
        };
        Arena {
            inner: UnsafeCell::new(inner),
            #[cfg(feature = "stats")]
            stats: AtomicStats {
                bytes_used: AtomicUsize::new(0),
                allocation_count: AtomicUsize::new(0),
                _padding: [0; 64 - 2 * 8],
            },
            #[cfg(feature = "lockfree")]
            lockfree_stats: lockfree::LockFreeStats::new(), // v0.5.0: Initialize lock-free stats
            memory_pool: UnsafeCell::new(MemoryPool::new()),
            _padding: [0; 64],
        }
    }

    #[inline]
    fn allocate_raw(&self, layout: Layout) -> *mut u8 {
        debug_assert!(
            layout.align() <= CHUNK_ALIGN,
            "requested alignment {} exceeds CHUNK_ALIGN {}",
            layout.align(),
            CHUNK_ALIGN
        );

        let size = layout.size();
        let align = layout.align();

        if size == 0 {
            #[cfg(feature = "stats")]
            self.stats.allocation_count.fetch_add(1, Ordering::Relaxed);
            return NonNull::<u8>::dangling().as_ptr();
        }

        // v0.5.0: Try lock-free buffer for small allocations
        #[cfg(feature = "lockfree")]
        {
            if size <= 1024 {
                // Use lock-free for small-to-medium allocations
                let inner = unsafe { &*self.inner.get() };
                if let Some(ref buffer) = inner.lockfree_buffer {
                    if let Some(ptr) = buffer.try_alloc(size, align) {
                        // v0.5.0: Record lock-free stats
                        self.lockfree_stats.record_allocation();
                        self.lockfree_stats.record_cache_hit();

                        // v0.5.0: Debug allocation tracking
                        #[cfg(feature = "debug")]
                        {
                            let arena_id = self as *const Arena as usize;
                            let mut debug_state = debug::DEBUG_STATE.write().unwrap();
                            debug_state.register_allocation(
                                arena_id,
                                ptr,
                                size,
                                inner.current_checkpoint_id,
                            );
                        }
                        #[cfg(feature = "stats")]
                        self.stats.allocation_count.fetch_add(1, Ordering::Relaxed);
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
                // Only cache very small allocations
                let arena_id = self as *const Arena as usize;
                if let Some(ptr) = thread_local_cache::try_thread_local_alloc(arena_id, size, align)
                {
                    // v0.5.0: Debug allocation tracking
                    #[cfg(feature = "debug")]
                    {
                        let inner = unsafe { &*self.inner.get() };
                        let mut debug_state = debug::DEBUG_STATE.write().unwrap();
                        debug_state.register_allocation(
                            arena_id,
                            ptr,
                            size,
                            inner.current_checkpoint_id,
                        );
                    }
                    #[cfg(feature = "stats")]
                    self.stats.allocation_count.fetch_add(1, Ordering::Relaxed);
                    return ptr;
                }
            }
        }

        // Fast path: try memory pool first for small allocations
        if size <= 4096 {
            if let Some(ptr) = unsafe { self.try_pool_alloc(size, align) } {
                // v0.5.0: Debug allocation tracking
                #[cfg(feature = "debug")]
                {
                    let arena_id = self as *const Arena as usize;
                    let inner = unsafe { &*self.inner.get() };
                    let mut debug_state = debug::DEBUG_STATE.write().unwrap();
                    debug_state.register_allocation(
                        arena_id,
                        ptr,
                        size,
                        inner.current_checkpoint_id,
                    );
                }
                return ptr;
            }
        }

        // Standard arena allocation path
        unsafe {
            let inner = &mut *self.inner.get();
            let current_chunk_idx = inner.current_chunk.load(Ordering::Acquire);
            let chunk = &mut inner.chunks[current_chunk_idx];

            // Prefetch the chunk data for better cache performance
            self.prefetch(chunk.ptr.as_ptr());

            // Atomic compare-and-swap for allocation
            let current = chunk.used.load(Ordering::Acquire);
            let aligned = (current + align - 1) & !(align - 1);
            let end = aligned + size;

            if likely(end <= chunk.capacity) {
                // Try to atomically claim the space
                if chunk
                    .used
                    .compare_exchange_weak(current, end, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    self.record_allocation(end - current);
                    let ptr = chunk.ptr.as_ptr().add(aligned);
                    // v0.5.0: Debug allocation tracking
                    #[cfg(feature = "debug")]
                    {
                        let arena_id = self as *const Arena as usize;
                        let mut debug_state = debug::DEBUG_STATE.write().unwrap();
                        debug_state.register_allocation(
                            arena_id,
                            ptr,
                            size,
                            inner.current_checkpoint_id,
                        );
                    }
                    return ptr;
                }
                // CAS failed, retry once (common case)
                let current = chunk.used.load(Ordering::Acquire);
                let aligned = (current + align - 1) & !(align - 1);
                let end = aligned + size;
                if end <= chunk.capacity
                    && chunk
                        .used
                        .compare_exchange_weak(current, end, Ordering::Release, Ordering::Relaxed)
                        .is_ok()
                {
                    self.record_allocation(end - current);
                    let ptr = chunk.ptr.as_ptr().add(aligned);
                    // v0.5.0: Debug allocation tracking
                    #[cfg(feature = "debug")]
                    {
                        let arena_id = self as *const Arena as usize;
                        let mut debug_state = debug::DEBUG_STATE.write().unwrap();
                        debug_state.register_allocation(
                            arena_id,
                            ptr,
                            size,
                            inner.current_checkpoint_id,
                        );
                    }
                    return ptr;
                }
            }

            // Slow path: need new chunk
            self.allocate_new_chunk(&layout)
        }
    }

    #[inline]
    unsafe fn try_pool_alloc(&self, size: usize, align: usize) -> Option<*mut u8> {
        let pool = &mut *self.memory_pool.get();
        if let Some(ptr) = pool.alloc(size) {
            // Ensure proper alignment
            let addr = ptr.as_ptr() as usize;
            let aligned_addr = (addr + align - 1) & !(align - 1);
            if aligned_addr != addr {
                // Misaligned for this request.
                // Do not put it back into the pool, otherwise we can repeatedly pop the same
                // unusable pointer for higher-alignment allocations.
                let layout = Layout::from_size_align(size, 8).unwrap();
                std::alloc::dealloc(ptr.as_ptr(), layout);
                return None;
            }

            self.record_allocation(size);

            // v0.5.0: Debug allocation tracking
            #[cfg(feature = "debug")]
            {
                let arena_id = self as *const Arena as usize;
                let inner = unsafe { &*self.inner.get() };
                let mut debug_state = debug::DEBUG_STATE.write().unwrap();
                debug_state.register_allocation(
                    arena_id,
                    ptr.as_ptr(),
                    size,
                    inner.current_checkpoint_id,
                );
            }

            return Some(ptr.as_ptr());
        }
        None
    }

    /// Optimized prefetch with better cache line handling
    #[inline]
    #[cfg(target_arch = "x86_64")]
    unsafe fn prefetch(&self, addr: *const u8) {
        if is_x86_feature_detected!("avx2") {
            // Prefetch multiple cache lines for better performance
            for i in 0..PREFETCH_WARMUP_SIZE {
                let prefetch_addr = addr.add(i * 64);
                _mm_prefetch(prefetch_addr as *const i8, _MM_HINT_T0);
            }
        }
    }

    /// Optimized prefetch for aarch64 (Apple Silicon)
    #[inline]
    #[cfg(target_arch = "aarch64")]
    unsafe fn prefetch(&self, addr: *const u8) {
        // Simple prefetch for aarch64 without feature detection
        for i in 0..PREFETCH_WARMUP_SIZE {
            let prefetch_addr = addr.add(i * 64);
            // Simple memory read to trigger cache loading
            std::ptr::read_volatile(prefetch_addr);
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    #[inline]
    unsafe fn prefetch(&self, _addr: *const u8) {
        // No prefetch on other architectures
    }

    #[cold]
    #[inline(never)]
    unsafe fn allocate_new_chunk(&self, layout: &Layout) -> *mut u8 {
        let inner = &mut *self.inner.get();
        let new_capacity = next_chunk_capacity_optimized(
            inner.chunks[inner.current_chunk.load(Ordering::Acquire)].capacity,
            layout.size(),
            layout.align(),
        );
        let new_chunk = allocate_chunk(new_capacity);
        let new_chunk_idx = inner.chunks.len();
        inner.chunks.push(new_chunk);
        inner.current_chunk.store(new_chunk_idx, Ordering::Release);

        // Allocate from new chunk
        let chunk = &mut inner.chunks[new_chunk_idx];
        let aligned = 0usize; // Start of new chunk is already aligned
        let end = aligned + layout.size();
        chunk.used.store(end, Ordering::Release);
        self.record_allocation(end);
        let ptr = chunk.ptr.as_ptr().add(aligned);

        // v0.5.0: Debug allocation tracking
        #[cfg(feature = "debug")]
        {
            let arena_id = self as *const Arena as usize;
            let inner = unsafe { &mut *self.inner.get() };
            let mut debug_state = debug::DEBUG_STATE.write().unwrap();
            debug_state.register_allocation(
                arena_id,
                ptr,
                layout.size(),
                inner.current_checkpoint_id,
            );
        }

        ptr
    }

    /// Ultra-fast allocation for small types (≤ FAST_ALLOC_THRESHOLD bytes)
    ///
    /// This method uses an optimized fast-path that minimizes atomic operations
    /// for small, frequently allocated types.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let arena = arena_b::Arena::new();
    /// let x = arena.alloc_fast(42);
    /// *x = 7;
    /// ```
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_fast<T>(&self, value: T) -> &mut T {
        let layout = Layout::new::<T>();
        let size = layout.size();

        // Use fast path for small allocations
        if size <= FAST_ALLOC_THRESHOLD {
            unsafe {
                let ptr = self.allocate_fast_path(layout).cast::<T>();
                ptr.write(value);
                &mut *ptr
            }
        } else {
            // Fallback to standard allocation for larger objects
            self.alloc(value)
        }
    }

    /// Optimized fast-path allocation for small objects
    ///
    /// This method reduces atomic operation overhead by using relaxed ordering
    /// and optimizing the common case where allocation fits in the current chunk.
    #[inline]
    unsafe fn allocate_fast_path(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        if size == 0 {
            #[cfg(feature = "stats")]
            self.stats.allocation_count.fetch_add(1, Ordering::Relaxed);
            return NonNull::<u8>::dangling().as_ptr();
        }

        // Try memory pool first for very small allocations
        if size <= 512 {
            if let Some(ptr) = self.try_pool_alloc(size, align) {
                return ptr;
            }
        }

        // Optimized arena fast-path with relaxed atomics
        let inner = &mut *self.inner.get();
        let current_chunk_idx = inner.current_chunk.load(Ordering::Relaxed);
        let chunk = &mut inner.chunks[current_chunk_idx];

        // Use relaxed ordering for better performance in single-threaded scenarios
        let current = chunk.used.load(Ordering::Relaxed);
        let aligned = match align_up_checked(current, align) {
            Some(v) => v,
            None => return self.allocate_raw(layout),
        };
        let end = match aligned.checked_add(size) {
            Some(v) => v,
            None => return self.allocate_raw(layout),
        };

        if likely(end <= chunk.capacity) {
            // Fast-path: single atomic operation with relaxed ordering
            if chunk
                .used
                .compare_exchange_weak(current, end, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                self.record_allocation(end - current);
                let ptr = chunk.ptr.as_ptr().add(aligned);

                // Keep debug tracking consistent across allocation paths.
                #[cfg(feature = "debug")]
                {
                    let arena_id = self as *const Arena as usize;
                    let inner = unsafe { &*self.inner.get() };
                    let mut debug_state = debug::DEBUG_STATE.write().unwrap();
                    debug_state.register_allocation(
                        arena_id,
                        ptr,
                        size,
                        inner.current_checkpoint_id,
                    );
                }

                return ptr;
            }
        }

        // Fallback to standard allocation
        self.allocate_raw(layout)
    }

    /// Allocate an array with known size at compile time
    ///
    /// This is optimized for fixed-size arrays where the size is known at compile time,
    /// allowing for better compiler optimizations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let arena = arena_b::Arena::new();
    /// let arr = arena.alloc_array([1, 2, 3, 4, 5]);
    /// assert_eq!(arr.len(), 5);
    /// ```
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_array<T: Copy, const N: usize>(&self, values: [T; N]) -> &[T] {
        if N == 0 {
            return &[];
        }

        let layout = Layout::array::<T>(N).unwrap();
        unsafe {
            let ptr = self.allocate_fast_path(layout).cast::<T>();
            // Use optimized copy for small arrays
            if N * mem::size_of::<T>() <= 256 {
                // Manual loop for very small arrays (better optimization)
                for (i, &item) in values.iter().enumerate().take(N) {
                    ptr.add(i).write(item);
                }
            } else {
                // Use optimized slice copy for larger arrays
                ptr::copy_nonoverlapping(values.as_ptr(), ptr, N);
            }
            slice::from_raw_parts(ptr, N)
        }
    }

    /// Allocate uninitialized array with known size
    ///
    /// Returns a mutable slice of uninitialized memory that can be initialized later.
    /// This is useful when you need to allocate first and initialize later.
    ///
    /// # Safety
    ///
    /// The caller must initialize all elements before reading from them.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let arena = arena_b::Arena::new();
    /// let mut arr = unsafe { arena.alloc_array_uninit::<u32, 10>() };
    /// for i in 0..10 {
    ///     arr[i].write(i as u32);
    /// }
    /// let initialized = unsafe { core::slice::from_raw_parts(arr.as_ptr(), 10) };
    /// ```
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_array_uninit<T, const N: usize>(&self) -> &mut [MaybeUninit<T>] {
        if N == 0 {
            return unsafe {
                slice::from_raw_parts_mut(NonNull::<MaybeUninit<T>>::dangling().as_ptr(), 0)
            };
        }

        let layout = Layout::array::<T>(N).unwrap();
        unsafe {
            let ptr = self.allocate_fast_path(layout).cast::<MaybeUninit<T>>();
            slice::from_raw_parts_mut(ptr, N)
        }
    }

    /// Allocate multiple values of the same type in a single operation
    ///
    /// This is optimized for bulk allocation patterns and reduces overhead
    /// by calculating layout once and performing optimized allocation.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let arena = arena_b::Arena::new();
    /// let values = arena.alloc_batch([1, 2, 3, 4, 5]);
    /// assert_eq!(values.len(), 5);
    /// ```
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_batch<T: Copy>(&self, values: impl AsRef<[T]>) -> &[T] {
        let slice = values.as_ref();
        if slice.is_empty() {
            return &[];
        }

        self.alloc_slice_copy(slice)
    }

    /// Allocate space for `value` in the arena and returns a mutable reference.
    ///
    /// The returned reference is valid for as long as the arena lives or until
    /// the arena is reset.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let arena = arena_b::Arena::new();
    /// let x = arena.alloc(42);
    /// *x = 7;
    /// ```
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc<T>(&self, value: T) -> &mut T {
        let layout = Layout::new::<T>();
        unsafe {
            let ptr = self.allocate_raw(layout).cast::<T>();
            ptr.write(value);
            &mut *ptr
        }
    }

    /// Allocates space for a default-initialized value in the arena.
    ///
    /// Returns a mutable reference to the newly allocated value.
    #[inline]
    pub fn alloc_default<T: Default>(&self) -> &mut T {
        self.alloc(T::default())
    }

    /// Optimized allocation for common small types
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_u8(&self, value: u8) -> &mut u8 {
        let layout = Layout::from_size_align(1, 1).unwrap();
        unsafe {
            let ptr = self.allocate_raw(layout).cast::<u8>();
            ptr.write(value);
            &mut *ptr
        }
    }

    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_u32(&self, value: u32) -> &mut u32 {
        let layout = Layout::from_size_align(4, 4).unwrap();
        unsafe {
            let ptr = self.allocate_raw(layout).cast::<u32>();
            ptr.write(value);
            &mut *ptr
        }
    }

    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_u64(&self, value: u64) -> &mut u64 {
        let layout = Layout::from_size_align(8, 8).unwrap();
        unsafe {
            let ptr = self.allocate_raw(layout).cast::<u64>();
            ptr.write(value);
            &mut *ptr
        }
    }

    /// Allocates a slice by copying the contents of `slice` into the arena.
    ///
    /// The returned slice has the same length and contents as `slice`.
    #[inline]
    pub fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &[T] {
        if slice.is_empty() {
            self.record_allocation(0);
            return unsafe { slice::from_raw_parts(NonNull::<T>::dangling().as_ptr(), 0) };
        }

        let len = slice.len();
        let layout = Layout::array::<T>(len).unwrap();
        unsafe {
            let ptr = self.allocate_raw(layout).cast::<T>();
            // Use optimized copy for large slices
            let total_bytes = mem::size_of_val(slice);
            if total_bytes >= SIMD_THRESHOLD && HAS_NATIVE_SIMD {
                self.copy_large_slice_optimized(slice.as_ptr(), ptr, len, total_bytes);
            } else {
                ptr::copy_nonoverlapping(slice.as_ptr(), ptr, len);
            }
            slice::from_raw_parts(ptr, len)
        }
    }

    /// Fast-path allocation for small slices that benefits from reduced allocator overhead.
    #[inline]
    pub fn alloc_slice_fast<T: Copy>(&self, slice: &[T]) -> &[T] {
        if slice.is_empty() {
            self.record_allocation(0);
            return unsafe { slice::from_raw_parts(NonNull::<T>::dangling().as_ptr(), 0) };
        }

        let total_bytes = mem::size_of_val(slice);
        if total_bytes > FAST_ALLOC_THRESHOLD {
            return self.alloc_slice_copy(slice);
        }

        let len = slice.len();
        let layout = Layout::array::<T>(len).unwrap();
        unsafe {
            let ptr = self.allocate_fast_path(layout).cast::<T>();
            ptr::copy_nonoverlapping(slice.as_ptr(), ptr, len);
            slice::from_raw_parts(ptr, len)
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn copy_large_slice_optimized<T: Copy>(
        &self,
        src: *const T,
        dst: *mut T,
        len: usize,
        total_bytes: usize,
    ) {
        if mem::size_of::<T>() == 1 && is_x86_feature_detected!("avx2") {
            // Aggressive SIMD for byte arrays
            let src_bytes = src as *const u8;
            let dst_bytes = dst as *mut u8;

            // Use 256-bit (32-byte) vectors for maximum throughput
            let vectors = total_bytes / 32;
            let remaining = total_bytes % 32;

            // Prefetch first few cache lines
            for i in 0..4.min(vectors) {
                if i * 32 < total_bytes {
                    _mm_prefetch(src_bytes.add(i * 32) as *const i8, _MM_HINT_T0);
                }
            }

            // Vectorized copy loop
            for i in 0..vectors {
                // Prefetch ahead
                if i + 4 < vectors {
                    _mm_prefetch(src_bytes.add((i + 4) * 32) as *const i8, _MM_HINT_T0);
                }

                let data = _mm256_loadu_si256(src_bytes.add(i * 32) as *const __m256i);
                _mm256_storeu_si256(dst_bytes.add(i * 32) as *mut __m256i, data);
            }

            // Handle remaining bytes
            for j in 0..remaining {
                *dst_bytes.add(vectors * 32 + j) = *src_bytes.add(vectors * 32 + j);
            }
        } else if mem::size_of::<T>() == 4 && is_x86_feature_detected!("avx2") {
            // Optimized for u32/i32 arrays
            let elements_per_vector = 8;
            let vectors = len / elements_per_vector;
            let remaining = len % elements_per_vector;

            for i in 0..vectors {
                let data = _mm256_loadu_si256(src.add(i * elements_per_vector) as *const __m256i);
                _mm256_storeu_si256(dst.add(i * elements_per_vector) as *mut __m256i, data);
            }

            for j in 0..remaining {
                *dst.add(vectors * elements_per_vector + j) =
                    *src.add(vectors * elements_per_vector + j);
            }
        } else {
            ptr::copy_nonoverlapping(src, dst, len);
        }
    }

    #[cfg(target_arch = "aarch64")]
    unsafe fn copy_large_slice_optimized<T: Copy>(
        &self,
        src: *const T,
        dst: *mut T,
        len: usize,
        _total_bytes: usize,
    ) {
        // Use standard copy for aarch64 to avoid NEON intrinsics
        // This still provides good performance on ARM64
        ptr::copy_nonoverlapping(src, dst, len);
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    unsafe fn copy_large_slice_optimized<T: Copy>(
        &self,
        src: *const T,
        dst: *mut T,
        len: usize,
        _total_bytes: usize,
    ) {
        ptr::copy_nonoverlapping(src, dst, len);
    }

    /// Allocates uninitialized memory for a slice of length `len`.
    ///
    /// The elements must be initialized by the caller before they are read.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_slice_uninit<T>(&self, len: usize) -> &mut [MaybeUninit<T>] {
        if len == 0 {
            self.record_allocation(0);
            return unsafe {
                slice::from_raw_parts_mut(NonNull::<MaybeUninit<T>>::dangling().as_ptr(), 0)
            };
        }
        let layout = Layout::array::<T>(len).unwrap();
        unsafe {
            let ptr = self.allocate_raw(layout).cast::<MaybeUninit<T>>();
            slice::from_raw_parts_mut(ptr, len)
        }
    }

    /// Allocates space for the UTF-8 bytes of `s` and returns a borrowed `str`.
    ///
    /// The returned string slice has the same contents as `s`.
    #[inline]
    pub fn alloc_str(&self, s: &str) -> &str {
        let bytes = self.alloc_slice_copy(s.as_bytes());
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }

    /// Allocates a mutable UTF-8 buffer initialized with `\0` bytes.
    ///
    /// Callers can mutate the returned string via [`str::as_bytes_mut`] to
    /// write valid UTF-8 data without any extra allocations or copies.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_str_uninit(&self, len: usize) -> &mut str {
        let buffer = self.alloc_slice_uninit::<u8>(len);
        for slot in buffer.iter_mut() {
            slot.write(0);
        }

        unsafe {
            let ptr = buffer.as_mut_ptr() as *mut u8;
            let bytes = std::slice::from_raw_parts_mut(ptr, buffer.len());
            std::str::from_utf8_unchecked_mut(bytes)
        }
    }

    /// Proactively reserves at least `additional` bytes of contiguous space in the current arena.
    ///
    /// This method can be used before predictable allocation spikes to avoid repeated
    /// chunk growth pauses in hot paths. If the current chunk already has enough space,
    /// the call is a no-op. Otherwise, a new chunk large enough to satisfy the request
    /// is appended eagerly.
    pub fn reserve_additional(&mut self, additional: usize) {
        if additional == 0 {
            return;
        }

        unsafe {
            let inner = &mut *self.inner.get();
            if inner.chunks.is_empty() {
                return;
            }

            let current_idx = inner.current_chunk.load(Ordering::Acquire);
            let (capacity, used) = {
                let chunk = &inner.chunks[current_idx];
                (chunk.capacity, chunk.used.load(Ordering::Acquire))
            };

            let available = capacity.saturating_sub(used);
            if available >= additional {
                return;
            }

            let needed = additional - available;
            let new_capacity = next_chunk_capacity_optimized(capacity, needed, CHUNK_ALIGN);
            let new_chunk = allocate_chunk(new_capacity);
            inner.chunks.push(new_chunk);
            inner
                .current_chunk
                .store(inner.chunks.len() - 1, Ordering::Release);

            #[cfg(feature = "stats")]
            {
                self.stats.bytes_used.fetch_add(0, Ordering::Relaxed); // touch to keep cache line warm
            }
        }
    }

    /// Releases all chunks beyond the first, reducing memory footprint after spikes.
    ///
    /// # Safety
    ///
    /// The caller must ensure no allocations from trimmed chunks are still in use.
    pub unsafe fn shrink_to_fit(&mut self) {
        let inner = &mut *self.inner.get();
        if inner.chunks.len() <= 1 {
            return;
        }

        inner.chunks.truncate(1);
        inner.current_chunk.store(0, Ordering::Release);
        inner.checkpoints.clear();

        #[cfg(feature = "stats")]
        {
            self.stats.bytes_used.store(0, Ordering::Release);
            self.stats.allocation_count.store(0, Ordering::Release);
        }
    }

    /// Combination helper that performs [`reset`](Self::reset) followed by [`shrink_to_fit`](Self::shrink_to_fit).
    ///
    /// # Safety
    ///
    /// Carries the same requirements as both `reset` and `shrink_to_fit`.
    pub unsafe fn reset_and_shrink(&mut self) {
        self.reset();
        self.shrink_to_fit();
    }

    pub fn scope<'arena, F, R>(&'arena self, f: F) -> R
    where
        F: for<'scope> FnOnce(&Scope<'scope, 'arena>) -> R,
    {
        unsafe {
            let _guard = ScopeResetGuard::capture(self);

            let scope = Scope {
                arena: self,
                _marker: PhantomData,
            };

            f(&scope)
        }
    }

    /// Resets the arena, making all previously allocated memory available again.
    ///
    /// # Safety
    ///
    /// All references previously returned from this arena must be considered
    /// invalid after calling `reset` and must not be used.
    #[inline]
    pub unsafe fn reset(&mut self) {
        let inner = &mut *self.inner.get();
        for chunk in &mut inner.chunks {
            chunk.used.store(0, Ordering::Release);
        }
        inner.current_chunk.store(0, Ordering::Release);

        #[cfg(feature = "virtual_memory")]
        {
            if let Some(ref mut vchunk) = inner.virtual_chunk {
                vchunk.reset();
            }
        }

        #[cfg(feature = "stats")]
        {
            self.stats.bytes_used.store(0, Ordering::Release);
            self.stats.allocation_count.store(0, Ordering::Release);
        }

        // v0.5.0: Clear checkpoints on full reset
        inner.checkpoints.clear();

        // v0.5.0: Reset thread-local cache
        #[cfg(feature = "thread_local")]
        {
            thread_local_cache::reset_thread_cache();
        }

        // v0.5.0: Reset lock-free buffer
        #[cfg(feature = "lockfree")]
        {
            if let Some(ref buffer) = inner.lockfree_buffer {
                buffer.reset();
            }
        }

        // v0.5.0: Clear debug state on full reset
        #[cfg(feature = "debug")]
        {
            let arena_id = self as *const Arena as usize;
            let mut debug_state = debug::DEBUG_STATE.write().unwrap();
            debug_state.allocations.remove(&arena_id);
            debug_state.current_checkpoint_ids.remove(&arena_id);
        }
    }

    #[inline]
    fn record_allocation(&self, size: usize) {
        #[cfg(feature = "stats")]
        {
            if size > 0 {
                self.stats.bytes_used.fetch_add(size, Ordering::Relaxed);
            }
            self.stats.allocation_count.fetch_add(1, Ordering::Relaxed);
        }

        #[cfg(not(feature = "stats"))]
        let _ = size;
    }

    /// Returns current allocation statistics for this arena.
    #[inline]
    pub fn stats(&self) -> ArenaStats {
        let inner = unsafe { &*self.inner.get() };
        let total_capacity: usize = inner.chunks.iter().map(|c| c.capacity).sum();

        #[cfg(feature = "stats")]
        {
            ArenaStats {
                bytes_allocated: total_capacity,
                bytes_used: self.stats.bytes_used.load(Ordering::Relaxed),
                allocation_count: self.stats.allocation_count.load(Ordering::Relaxed),
                chunk_count: inner.chunks.len(),
            }
        }

        #[cfg(not(feature = "stats"))]
        {
            let bytes_used = inner
                .chunks
                .iter()
                .map(|c| c.used.load(Ordering::Relaxed))
                .sum();
            ArenaStats {
                bytes_allocated: total_capacity,
                bytes_used,
                allocation_count: 0, // Not tracked without stats feature
                chunk_count: inner.chunks.len(),
            }
        }
    }

    #[inline]
    pub fn chunk_usage(&self) -> Vec<ArenaChunkUsage> {
        let inner = unsafe { &*self.inner.get() };
        inner
            .chunks
            .iter()
            .map(|c| ArenaChunkUsage {
                capacity: c.capacity,
                used: c.used.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// Returns the number of bytes currently used in the underlying chunk.
    #[inline]
    pub fn bytes_allocated(&self) -> usize {
        #[cfg(feature = "stats")]
        return self.stats.bytes_used.load(Ordering::Relaxed);

        #[cfg(not(feature = "stats"))]
        {
            let inner = unsafe { &*self.inner.get() };
            inner.chunks[inner.current_chunk.load(Ordering::Acquire)]
                .used
                .load(Ordering::Relaxed)
        }
    }

    // v0.5.0: Fast Reset API - Arena checkpoint functionality

    /// Creates a checkpoint of the current arena state.
    ///
    /// This saves the current allocation position and can be used later
    /// with `rewind_to_checkpoint()` for fast bulk deallocation.
    ///
    /// # Returns
    ///
    /// Returns an `ArenaCheckpoint` that can be used to rewind to this state.
    ///
    /// # Examples
    ///
    /// ```
    /// use arena_b::Arena;
    /// let arena = Arena::new();
    /// let checkpoint = arena.checkpoint();
    ///
    /// // Make allocations...
    /// let value1 = arena.alloc(42);
    /// let value2 = arena.alloc(100);
    ///
    /// // Fast rewind to checkpoint
    /// unsafe {
    ///     arena.rewind_to_checkpoint(checkpoint);
    /// }
    /// ```
    #[inline]
    pub fn checkpoint(&self) -> ArenaCheckpoint {
        let inner = unsafe { &mut *self.inner.get() };
        let current_chunk_idx = inner.current_chunk.load(Ordering::Acquire);
        let current_chunk = &inner.chunks[current_chunk_idx];
        let chunk_offset = current_chunk.used.load(Ordering::Relaxed);

        #[cfg(feature = "stats")]
        let (allocation_count, bytes_used) = (
            self.stats.allocation_count.load(Ordering::Relaxed),
            self.stats.bytes_used.load(Ordering::Relaxed),
        );

        #[cfg(not(feature = "stats"))]
        let (allocation_count, bytes_used) = (0, 0);

        #[cfg(feature = "debug")]
        let checkpoint_id = {
            let arena_id = self as *const Arena as usize;
            let id = inner.current_checkpoint_id;
            inner.current_checkpoint_id = inner.current_checkpoint_id.saturating_add(1);
            let mut debug_state = debug::DEBUG_STATE.write().unwrap();
            debug_state
                .current_checkpoint_ids
                .insert(arena_id, inner.current_checkpoint_id);
            id
        };

        let mut checkpoint = ArenaCheckpoint {
            chunk_index: current_chunk_idx,
            chunk_offset,
            allocation_count,
            bytes_used,
            #[cfg(feature = "debug")]
            checkpoint_id,
        };

        checkpoint
    }

    /// Rewinds the arena to a previous checkpoint state.
    ///
    /// This provides instant bulk deallocation by resetting the allocation
    /// position to the checkpoint. All allocations made after the checkpoint
    /// become invalid.
    ///
    /// # Safety
    ///
    /// - All references allocated after the checkpoint become invalid
    /// - The checkpoint must be from this arena
    /// - No other threads should be using the arena during rewind
    ///
    /// # Examples
    ///
    /// ```
    /// use arena_b::Arena;
    /// let arena = Arena::new();
    /// let checkpoint = arena.checkpoint();
    ///
    /// // Frame-based allocation pattern
    /// for i in 0..3 {
    ///     let frame_checkpoint = arena.checkpoint();
    ///     
    ///     // Allocate frame data...
    ///     let entities = arena.alloc_batch(&[1, 2, 3]);
    ///     
    ///     // Process frame...
    ///     
    ///     // Fast cleanup - rewind to frame start
    ///     unsafe {
    ///         arena.rewind_to_checkpoint(frame_checkpoint);
    ///     }
    /// }
    /// ```
    #[inline]
    pub unsafe fn rewind_to_checkpoint(&self, checkpoint: ArenaCheckpoint) {
        let inner = &mut *self.inner.get();

        // Validate checkpoint
        assert!(
            checkpoint.chunk_index < inner.chunks.len(),
            "Invalid checkpoint: chunk index out of bounds"
        );
        assert!(
            checkpoint.chunk_offset <= inner.chunks[checkpoint.chunk_index].capacity,
            "Invalid checkpoint: offset exceeds chunk capacity"
        );

        // Reset current chunk and all subsequent chunks
        inner
            .current_chunk
            .store(checkpoint.chunk_index, Ordering::Release);

        for (i, chunk) in inner.chunks.iter_mut().enumerate() {
            if i == checkpoint.chunk_index {
                chunk.used.store(checkpoint.chunk_offset, Ordering::Release);
            } else if i > checkpoint.chunk_index {
                chunk.used.store(0, Ordering::Release);
            }
            // Chunks before checkpoint remain unchanged
        }

        // Reset stats to checkpoint values
        #[cfg(feature = "stats")]
        {
            self.stats
                .allocation_count
                .store(checkpoint.allocation_count, Ordering::Release);
            self.stats
                .bytes_used
                .store(checkpoint.bytes_used, Ordering::Release);
        }

        // v0.5.0: Remove checkpoints that were created after this checkpoint
        inner.checkpoints.retain(|&cp| {
            cp.chunk_index < checkpoint.chunk_index
                || (cp.chunk_index == checkpoint.chunk_index
                    && cp.chunk_offset <= checkpoint.chunk_offset)
        });

        #[cfg(feature = "virtual_memory")]
        {
            if let Some(ref mut vchunk) = inner.virtual_chunk {
                vchunk.reset();
            }
        }

        #[cfg(feature = "debug")]
        {
            let arena_id = self as *const Arena as usize;
            crate::debug::rewind_arena_to_checkpoint(arena_id, checkpoint.checkpoint_id);
            (*self.inner.get()).current_checkpoint_id = checkpoint.checkpoint_id + 1;
        }

        // v0.5.0: Reset thread-local cache on rewind
        #[cfg(feature = "thread_local")]
        {
            thread_local_cache::reset_thread_cache();
        }

        // v0.5.0: Reset lock-free buffer on rewind
        #[cfg(feature = "lockfree")]
        {
            if let Some(ref buffer) = inner.lockfree_buffer {
                buffer.reset();
            }
        }
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
    pub fn push_checkpoint(&self) -> ArenaCheckpoint {
        let checkpoint = self.checkpoint();
        let inner = unsafe { &mut *self.inner.get() };
        inner.checkpoints.push(checkpoint);
        checkpoint
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
    pub unsafe fn pop_and_rewind(&mut self) -> ArenaCheckpoint {
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

    // v0.5.0: Memory safety debug API

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
        let ptr = reference as *const T as *const u8;
        let debug_state = debug::DEBUG_STATE.read().unwrap();
        debug_state.check_use_after_rewind(arena_id, ptr)
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
        let debug_state = debug::DEBUG_STATE.read().unwrap();
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
        inner
            .virtual_chunk
            .as_ref()
            .map(|chunk| chunk.committed_bytes())
    }

    /// Returns debug statistics about allocations and checkpoints.
    ///
    /// This method provides insight into the debug tracking system
    /// and can help diagnose memory safety issues.
    #[cfg(feature = "debug")]
    #[inline]
    pub fn debug_stats(&self) -> DebugStats {
        let arena_id = self as *const Arena as usize;
        let debug_state = debug::DEBUG_STATE.read().unwrap();
        let inner = unsafe { &*self.inner.get() };

        let (total_allocations, corrupted_allocations) = debug_state.get_stats(arena_id);

        DebugStats {
            total_allocations,
            active_checkpoints: debug_state.get_current_checkpoint_id(arena_id).saturating_sub(1)
                as usize,
            current_checkpoint_id: debug_state.get_current_checkpoint_id(arena_id),
            corrupted_allocations,
            leak_reports: 0, // Will be populated by leak_report() calls
        }
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe {
            let inner = &mut *self.inner.get();
            for chunk in &inner.chunks {
                let layout = Layout::from_size_align(chunk.capacity, CHUNK_ALIGN).unwrap();
                std::alloc::dealloc(chunk.ptr.as_ptr(), layout);
            }
        }
    }
}

unsafe impl Send for Arena {}

unsafe fn allocate_chunk(capacity: usize) -> Chunk {
    let layout = Layout::from_size_align(capacity, CHUNK_ALIGN).unwrap();
    let ptr = unsafe { std::alloc::alloc(layout) };
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    Chunk {
        ptr: unsafe { NonNull::new_unchecked(ptr) },
        capacity,
        used: AtomicUsize::new(0),
        _padding: [0; 64 - 3 * 8],
    }
}

fn next_chunk_capacity_optimized(prev_capacity: usize, size: usize, align: usize) -> usize {
    let min_needed = size + align - 1;

    // Exponential growth with reasonable bounds
    let mut capacity = prev_capacity.saturating_mul(2);

    // Ensure we have enough for this allocation
    if capacity < min_needed {
        capacity = min_needed;
    }

    // Apply reasonable bounds
    capacity = capacity.clamp(MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);

    // Round up to nearest multiple of CHUNK_ALIGN for better alignment
    (capacity + ALIGNMENT_MASK) & !ALIGNMENT_MASK
}

// Keep the old function for compatibility
#[allow(dead_code)]
fn next_chunk_capacity(prev_capacity: usize, layout: &Layout) -> usize {
    next_chunk_capacity_optimized(prev_capacity, layout.size(), layout.align())
}

#[inline]
fn align_up_checked(value: usize, align: usize) -> Option<usize> {
    if align <= 1 {
        return Some(value);
    }
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}

#[inline]
#[allow(dead_code)]
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
            inner: Mutex::new(Arena::with_capacity(bytes)),
        }
    }

    pub fn scope<F, R>(&self, f: F) -> R
    where
        F: for<'scope, 'arena> FnOnce(&Scope<'scope, 'arena>) -> R,
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
            Arena::DEFAULT_CAPACITY
        } else {
            self.initial_capacity
        };

        // thread_safe is currently ignored; a non-thread-safe Arena is always built.
        Arena::with_capacity(capacity)
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
pub use thread_local_cache::{clear_thread_cache, reset_thread_cache, try_thread_local_alloc};

/// Re-export virtual memory types when the feature is enabled.
#[cfg(feature = "virtual_memory")]
pub use virtual_memory::VirtualMemoryRegion;

/// Re-export debug types when the feature is enabled.
#[cfg(feature = "debug")]
pub use debug::{AllocationInfo, DEBUG_STATE, FREED_MAGIC, GUARD_MAGIC};
