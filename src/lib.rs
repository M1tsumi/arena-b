use core::cell::{Cell, UnsafeCell};
use core::marker::PhantomData;
use core::mem::{self, MaybeUninit};
use core::ptr;
use core::slice;
use std::alloc::{self, Layout};
use std::ptr::NonNull;
use std::sync::Mutex;
use std::vec::Vec;

const CHUNK_ALIGN: usize = 64;
/// Bump allocation arena using a single contiguous chunk of memory.
///
/// This arena is optimized for workloads where many values are allocated
/// and then freed all at once when the arena is dropped or reset.
///
/// Allocations are very fast O(1) pointer bumps and there is no per-value
/// deallocation.
///
/// # Examples
///
/// ```rust
/// use arena_b::Arena;
///
/// let arena = Arena::new();
/// let value = arena.alloc(42);
/// assert_eq!(*value, 42);
/// ```
struct Chunk {
    ptr: NonNull<u8>,
    capacity: usize,
    used: usize,
}

struct ArenaInner {
    chunks: Vec<Chunk>,
    current_chunk: usize,
}

pub struct Arena {
    inner: UnsafeCell<ArenaInner>,
    bytes_used: Cell<usize>,
    allocation_count: Cell<usize>,
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
    const DEFAULT_CAPACITY: usize = 64 * 1024;

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
        let first_chunk = unsafe { allocate_chunk(bytes) };
        let inner = ArenaInner {
            chunks: vec![first_chunk],
            current_chunk: 0,
        };
        Arena {
            inner: UnsafeCell::new(inner),
            bytes_used: Cell::new(0),
            allocation_count: Cell::new(0),
        }
    }

    #[inline]
    fn allocate_raw(&self, layout: Layout) -> *mut u8 {
        unsafe {
            debug_assert!(
                layout.align() <= CHUNK_ALIGN,
                "requested alignment {} exceeds CHUNK_ALIGN {}",
                layout.align(),
                CHUNK_ALIGN
            );
            let inner = &mut *self.inner.get();
            loop {
                let chunk = &mut inner.chunks[inner.current_chunk];
                let current = chunk.used;
                let aligned = align_up(current, layout.align());
                let end = aligned
                    .checked_add(layout.size())
                    .expect("arena size overflow");
                if end <= chunk.capacity {
                    chunk.used = end;
                    let delta = end - current;
                    self.record_allocation(delta);
                    return chunk.ptr.as_ptr().add(aligned);
                } else {
                    let new_capacity = next_chunk_capacity(chunk.capacity, &layout);
                    let new_chunk = allocate_chunk(new_capacity);
                    inner.chunks.push(new_chunk);
                    inner.current_chunk = inner.chunks.len() - 1;
                }
            }
        }
    }

    /// Allocates space for `value` in the arena and returns a mutable reference.
    ///
    /// The returned reference is valid for as long as the arena lives or until
    /// the arena is reset.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use arena_b::Arena;
    ///
    /// let arena = Arena::new();
    /// let value = arena.alloc(42);
    /// assert_eq!(*value, 42);
    /// ```
    #[inline]
    #[allow(clippy::mut_from_ref)]
    pub fn alloc<T>(&self, value: T) -> &mut T {
        if mem::size_of::<T>() == 0 {
            self.record_allocation(0);
            let ptr = NonNull::<T>::dangling().as_ptr();
            unsafe { &mut *ptr }
        } else {
            let layout = Layout::new::<T>();
            unsafe {
                let ptr = self.allocate_raw(layout).cast::<T>();
                ptr::write(ptr, value);
                &mut *ptr
            }
        }
    }

    /// Allocates a value using `T::default()` as the initializer.
    #[inline]
    pub fn alloc_default<T: Default>(&self) -> &mut T {
        self.alloc(T::default())
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
        let layout = Layout::array::<T>(slice.len()).unwrap();
        unsafe {
            let ptr = self.allocate_raw(layout).cast::<T>();
            ptr::copy_nonoverlapping(slice.as_ptr(), ptr, slice.len());
            slice::from_raw_parts(ptr, slice.len())
        }
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
                let idx = inner.current_chunk;
                (idx, inner.chunks[idx].used)
            };
            let start_bytes_used = self.bytes_used.get();
            let start_allocation_count = self.allocation_count.get();

            let scope = Scope {
                arena: self,
                _marker: PhantomData,
            };

            let result = f(&scope);

            self.bytes_used.set(start_bytes_used);
            self.allocation_count.set(start_allocation_count);

            let inner = &mut *self.inner.get();
            for (idx, chunk) in inner.chunks.iter_mut().enumerate() {
                if idx < start_chunk {
                    continue;
                }
                if idx == start_chunk {
                    chunk.used = start_used;
                } else {
                    chunk.used = 0;
                }
            }
            inner.current_chunk = start_chunk;

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
            chunk.used = 0;
        }
        inner.current_chunk = 0;
        self.bytes_used.set(0);
        self.allocation_count.set(0);
    }

    #[inline]
    fn record_allocation(&self, size: usize) {
        #[cfg(feature = "stats")]
        {
            if size > 0 {
                self.bytes_used.set(
                    self.bytes_used
                        .get()
                        .checked_add(size)
                        .expect("arena size overflow"),
                );
            }
            self.allocation_count.set(self.allocation_count.get() + 1);
        }

        #[cfg(not(feature = "stats"))]
        let _ = size;
    }

    /// Returns current allocation statistics for this arena.
    #[inline]
    pub fn stats(&self) -> ArenaStats {
        let inner = unsafe { &*self.inner.get() };
        let total_capacity: usize = inner.chunks.iter().map(|c| c.capacity).sum();
        ArenaStats {
            bytes_allocated: total_capacity,
            bytes_used: self.bytes_used.get(),
            allocation_count: self.allocation_count.get(),
            chunk_count: inner.chunks.len(),
        }
    }

    /// Returns the number of bytes currently used in the underlying chunk.
    #[inline]
    pub fn bytes_allocated(&self) -> usize {
        self.bytes_used.get()
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
        used: 0,
    }
}

fn next_chunk_capacity(prev_capacity: usize, layout: &Layout) -> usize {
    let align = layout.align();
    let size = layout.size();
    let min_needed = size.checked_add(align).expect("arena size overflow");
    let mut capacity = prev_capacity.saturating_mul(2);
    if capacity < min_needed {
        capacity = min_needed;
    }
    capacity
}

fn align_up(offset: usize, align: usize) -> usize {
    let mask = align - 1;
    (offset + mask) & !mask
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

    #[inline]
    pub fn alloc<'pool>(&'pool self, value: T) -> Pooled<'pool, T> {
        unsafe {
            let inner = &mut *self.inner.get();
            let index = match inner.free.pop() {
                Some(i) => i,
                None => {
                    let idx = inner.storage.len();
                    inner.storage.push(None);
                    idx
                }
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
