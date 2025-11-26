use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::{self, MaybeUninit};
use core::ptr;
use core::slice;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::alloc::{self, Layout};
use std::ptr::NonNull;
use std::sync::Mutex;
use std::vec::Vec;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

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
                alloc::dealloc(ptr.as_ptr(), layout);
            }
        }
    }
}

// Cache-line aware atomic operations
#[repr(align(64))]
struct AtomicCounter {
    _padding: [u8; 64],
}

#[repr(C, align(64))]
struct Chunk {
    ptr: NonNull<u8>,
    capacity: usize,
    used: AtomicUsize,
    _padding: [u8; 64 - 3 * 8], // Cache line padding
}

#[repr(C, align(64))]
struct ArenaInner {
    chunks: Vec<Chunk>,
    current_chunk: AtomicUsize,
    total_allocated: AtomicCounter,
    _padding: [u8; 64 - 2 * 8 - 8], // Cache line padding
}

pub struct Arena {
    inner: UnsafeCell<ArenaInner>,
    #[cfg(feature = "stats")]
    stats: AtomicStats,
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
            _padding: [0; 64 - 2 * 8 - 8],
        };
        Arena {
            inner: UnsafeCell::new(inner),
            #[cfg(feature = "stats")]
            stats: AtomicStats {
                bytes_used: AtomicUsize::new(0),
                allocation_count: AtomicUsize::new(0),
                _padding: [0; 64 - 2 * 8],
            },
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

        // Fast path: try memory pool first for small allocations
        if size <= 4096 {
            if let Some(ptr) = unsafe { self.try_pool_alloc(size, align) } {
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
                    return chunk.ptr.as_ptr().add(aligned);
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
                    return chunk.ptr.as_ptr().add(aligned);
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
                // Misaligned, fallback to arena allocation
                pool.dealloc(ptr, size);
                return None;
            }

            self.record_allocation(size);
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
        chunk.ptr.as_ptr().add(aligned)
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
        let aligned = (current + align - 1) & !(align - 1);
        let end = aligned + size;

        if likely(end <= chunk.capacity) {
            // Fast-path: single atomic operation with relaxed ordering
            if chunk
                .used
                .compare_exchange_weak(current, end, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                self.record_allocation(end - current);
                return chunk.ptr.as_ptr().add(aligned);
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
            if total_bytes >= SIMD_THRESHOLD
                && cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
            {
                self.copy_large_slice_optimized(slice.as_ptr(), ptr, len, total_bytes);
            } else {
                ptr::copy_nonoverlapping(slice.as_ptr(), ptr, len);
            }
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

    pub fn scope<'arena, F, R>(&'arena self, f: F) -> R
    where
        F: for<'scope> FnOnce(&Scope<'scope, 'arena>) -> R,
    {
        unsafe {
            let (start_chunk, start_used) = {
                let inner = &mut *self.inner.get();
                let idx = inner.current_chunk.load(Ordering::Acquire);
                (idx, inner.chunks[idx].used.load(Ordering::Acquire))
            };

            #[cfg(feature = "stats")]
            let start_bytes_used = self.stats.bytes_used.load(Ordering::Acquire);
            #[cfg(feature = "stats")]
            let start_allocation_count = self.stats.allocation_count.load(Ordering::Acquire);

            let scope = Scope {
                arena: self,
                _marker: PhantomData,
            };

            let result = f(&scope);

            #[cfg(feature = "stats")]
            {
                self.stats
                    .bytes_used
                    .store(start_bytes_used, Ordering::Release);
                self.stats
                    .allocation_count
                    .store(start_allocation_count, Ordering::Release);
            }

            let inner = &mut *self.inner.get();
            for (idx, chunk) in inner.chunks.iter_mut().enumerate() {
                if idx < start_chunk {
                    continue;
                }
                if idx == start_chunk {
                    chunk.used.store(start_used, Ordering::Release);
                } else {
                    chunk.used.store(0, Ordering::Release);
                }
            }
            inner.current_chunk.store(start_chunk, Ordering::Release);

            result
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

        #[cfg(feature = "stats")]
        {
            self.stats.bytes_used.store(0, Ordering::Release);
            self.stats.allocation_count.store(0, Ordering::Release);
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
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe {
            let inner = &mut *self.inner.get();
            for chunk in &inner.chunks {
                let layout = Layout::from_size_align(chunk.capacity, CHUNK_ALIGN).unwrap();
                alloc::dealloc(chunk.ptr.as_ptr(), layout);
            }
        }
    }
}

unsafe impl Send for Arena {}

unsafe fn allocate_chunk(capacity: usize) -> Chunk {
    let layout = Layout::from_size_align(capacity, CHUNK_ALIGN).unwrap();
    let ptr = unsafe { alloc::alloc(layout) };
    if ptr.is_null() {
        alloc::handle_alloc_error(layout);
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
