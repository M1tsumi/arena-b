use crate::core::*;
use std::alloc::Layout;
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::sync::atomic::Ordering;

pub struct Arena {
    inner: UnsafeCell<ArenaInner>,
    allocation_counter: core::sync::atomic::AtomicUsize,
}

pub struct ArenaStats {
    pub bytes_allocated: usize,
    pub bytes_used: usize,
    pub allocation_count: usize,
    pub chunk_count: usize,
}

pub struct ArenaBuilder {
    // Capacity settings
    initial_capacity: usize,
    chunk_size: usize,
    reserve_size: Option<usize>,
    max_chunks: Option<usize>,

    // Performance tuning
    fast_path_threshold: usize,
    prefetch_distance: usize,

    // Feature configuration
    enable_stats: bool,
    enable_debug: bool,
    enable_lockfree: bool,
    enable_thread_local: bool,
    enable_virtual_memory: bool,

    // Feature bundle
    feature_bundle: Option<FeatureBundle>,

    // Diagnostics
    diagnostics_sink: Option<crate::DiagnosticsSink>,
}

#[derive(Debug, Clone, Copy)]
pub enum FeatureBundle {
    Perf,       // lockfree + thread_local + stats
    Safety,     // debug + stats
    Debuggable, // debug + stats + validation
    Server,     // lockfree + thread_local + virtual_memory + stats
}

impl FeatureBundle {
    pub fn apply_to_builder(self, builder: &mut ArenaBuilder) {
        match self {
            FeatureBundle::Perf => {
                builder.enable_lockfree = cfg!(feature = "lockfree");
                builder.enable_thread_local = cfg!(feature = "thread_local");
                builder.enable_stats = cfg!(feature = "stats");
                builder.fast_path_threshold = 1024;
            }
            FeatureBundle::Safety => {
                builder.enable_debug = cfg!(feature = "debug");
                builder.enable_stats = cfg!(feature = "stats");
            }
            FeatureBundle::Debuggable => {
                builder.enable_debug = cfg!(feature = "debug");
                builder.enable_stats = cfg!(feature = "stats");
                // Enable validation hooks
            }
            FeatureBundle::Server => {
                builder.enable_lockfree = cfg!(feature = "lockfree");
                builder.enable_thread_local = cfg!(feature = "thread_local");
                builder.enable_virtual_memory = cfg!(feature = "virtual_memory");
                builder.enable_stats = cfg!(feature = "stats");
                builder.reserve_size = Some(64 * 1024 * 1024); // 64MB
            }
        }
    }
}

pub struct Scope<'scope, 'arena> {
    arena: &'arena Arena,
    _marker: PhantomData<&'scope mut ()>,
}

#[derive(Debug, Clone)]
pub struct FeatureStatus {
    pub lockfree: bool,
    pub thread_local: bool,
    pub slab: bool,
    pub virtual_memory: bool,
    pub debug: bool,
    pub stats: bool,
}

#[derive(Debug, Clone)]
pub struct ArenaConfig {
    pub initial_capacity: usize,
    pub chunk_size: usize,
    pub reserve_size: Option<usize>,
    pub max_chunks: Option<usize>,
    pub fast_path_threshold: usize,
    pub prefetch_distance: usize,
    pub features: FeatureStatus,
}

impl Arena {
    pub fn feature_status(&self) -> FeatureStatus {
        FeatureStatus {
            lockfree: cfg!(feature = "lockfree"),
            thread_local: cfg!(feature = "thread_local"),
            slab: cfg!(feature = "slab"),
            virtual_memory: cfg!(feature = "virtual_memory"),
            debug: cfg!(feature = "debug"),
            stats: cfg!(feature = "stats"),
        }
    }

    pub fn build_config(&self) -> ArenaConfig {
        // Return default config for now - in a real implementation,
        // this would store the builder configuration
        ArenaConfig {
            initial_capacity: Self::DEFAULT_CAPACITY,
            chunk_size: crate::DEFAULT_CHUNK_SIZE,
            reserve_size: None,
            max_chunks: None,
            fast_path_threshold: 1024,
            prefetch_distance: 8,
            features: self.feature_status(),
        }
    }
    pub const DEFAULT_CAPACITY: usize = crate::DEFAULT_CHUNK_SIZE;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let inner = ArenaInner::new(capacity).expect("Failed to create arena");
        Self {
            inner: UnsafeCell::new(inner),
            allocation_counter: core::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[allow(clippy::mut_from_ref)]
    pub fn alloc<T>(&self, value: T) -> &mut T {
        let layout = Layout::new::<T>();
        let ptr = self.alloc_layout(layout);
        // Increment allocation counter for all allocations, including ZSTs
        self.allocation_counter.fetch_add(1, Ordering::Relaxed);
        unsafe {
            let ptr = ptr as *mut T;
            ptr.write(value);
            &mut *ptr
        }
    }

    pub fn alloc_default<T: Default>(&self) -> &mut T {
        self.alloc(T::default())
    }

    #[allow(clippy::mut_from_ref)]
    pub fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &mut [T] {
        let layout = match Layout::array::<T>(slice.len()) {
            Ok(l) => l,
            Err(_) => {
                eprintln!(
                    "legacy Arena::alloc_slice_copy: invalid layout for len={}",
                    slice.len()
                );
                return unsafe {
                    core::slice::from_raw_parts_mut(core::ptr::NonNull::<T>::dangling().as_ptr(), 0)
                };
            }
        };
        let ptr = self.alloc_layout(layout) as *mut T;
        unsafe {
            ptr.copy_from_nonoverlapping(slice.as_ptr(), slice.len());
            core::slice::from_raw_parts_mut(ptr, slice.len())
        }
    }

    #[allow(clippy::mut_from_ref)]
    pub fn alloc_slice_uninit<T>(&self, len: usize) -> &mut [core::mem::MaybeUninit<T>] {
        let layout = match Layout::array::<core::mem::MaybeUninit<T>>(len) {
            Ok(l) => l,
            Err(_) => {
                eprintln!(
                    "legacy Arena::alloc_slice_uninit: invalid layout for len={}",
                    len
                );
                return unsafe {
                    core::slice::from_raw_parts_mut(
                        core::ptr::NonNull::<core::mem::MaybeUninit<T>>::dangling().as_ptr(),
                        0,
                    )
                };
            }
        };
        let ptr = self.alloc_layout(layout) as *mut core::mem::MaybeUninit<T>;
        unsafe { core::slice::from_raw_parts_mut(ptr, len) }
    }

    pub fn alloc_str(&self, s: &str) -> &str {
        let bytes = self.alloc_slice_copy(s.as_bytes());
        unsafe { core::str::from_utf8_unchecked(bytes) }
    }

    pub fn alloc_layout(&self, layout: Layout) -> *mut u8 {
        unsafe {
            let inner = &mut *self.inner.get();
            let current_chunk_idx = inner.current_chunk.load(Ordering::Acquire);

            let chunk_idx = if current_chunk_idx >= inner.chunks.len() {
                inner.chunks.len().saturating_sub(1)
            } else {
                current_chunk_idx
            };

            if let Some(ptr) = inner.chunks[chunk_idx].allocate(layout) {
                ptr
            } else {
                let new_capacity = layout.size().saturating_mul(2).max(crate::MIN_CHUNK_SIZE);
                let mut new_chunk = Chunk::new(new_capacity).expect("Failed to allocate new chunk");

                if let Some(ptr) = new_chunk.allocate(layout) {
                    inner.chunks.push(new_chunk);
                    let idx = inner.chunks.len().saturating_sub(1);
                    inner.current_chunk.store(idx, Ordering::Release);
                    ptr
                } else {
                    panic!("Failed to allocate in new chunk");
                }
            }
        }
    }

    pub fn checkpoint(&self) -> ArenaCheckpoint {
        unsafe {
            let inner = &mut *self.inner.get();
            let current_chunk_idx = inner.current_chunk.load(Ordering::Acquire);
            let current_chunk = &inner.chunks[current_chunk_idx];

            ArenaCheckpoint {
                chunk_index: current_chunk_idx,
                chunk_offset: current_chunk.used(),
                checkpoint_id: inner.current_checkpoint_id,
                allocation_count: self.allocation_counter.load(Ordering::Relaxed),
            }
        }
    }

    pub fn push_checkpoint(&self) -> ArenaCheckpoint {
        let checkpoint = self.checkpoint();
        unsafe {
            let inner = &mut *self.inner.get();
            inner.checkpoints.push(checkpoint);
            inner.current_checkpoint_id += 1;
        }
        checkpoint
    }

    pub fn checkpoint_count(&self) -> usize {
        unsafe {
            let inner = &*self.inner.get();
            inner.checkpoints.len()
        }
    }

    pub fn clear_checkpoints(&self) {
        unsafe {
            let inner = &mut *self.inner.get();
            inner.checkpoints.clear();
        }
    }

    #[allow(clippy::missing_safety_doc)]
    pub unsafe fn rewind_to_checkpoint(&self, checkpoint: ArenaCheckpoint) {
        let inner = &mut *self.inner.get();

        for chunk in inner.chunks.iter_mut().skip(checkpoint.chunk_index + 1) {
            chunk.reset();
        }

        // Reset the checkpoint chunk to the specific offset
        inner.chunks[checkpoint.chunk_index].set_used(checkpoint.chunk_offset);
        inner
            .current_chunk
            .store(checkpoint.chunk_index, Ordering::Release);
    }

    pub fn scope<'scope, F, R>(&'scope self, f: F) -> R
    where
        F: FnOnce(&Scope<'scope, '_>) -> R,
    {
        let checkpoint = self.checkpoint();
        let allocation_count_before = self.allocation_counter.load(Ordering::Relaxed);
        let scope = Scope {
            arena: self,
            _marker: PhantomData,
        };
        let result = f(&scope);
        unsafe {
            self.rewind_to_checkpoint(checkpoint);
        }
        // Restore allocation counter to what it was before the scope
        self.allocation_counter
            .store(allocation_count_before, Ordering::Relaxed);
        result
    }

    pub fn stats(&self) -> ArenaStats {
        unsafe {
            let inner = &*self.inner.get();
            let chunk_count = inner.chunks.len();
            let mut bytes_allocated = 0;
            let mut bytes_used = 0;

            for chunk in &inner.chunks {
                bytes_allocated += chunk.capacity();
                bytes_used += chunk.used();
            }

            let allocation_count = self.allocation_counter.load(Ordering::Relaxed);

            ArenaStats {
                bytes_allocated,
                bytes_used,
                allocation_count,
                chunk_count,
            }
        }
    }

    pub fn bytes_allocated(&self) -> usize {
        unsafe {
            let inner = &*self.inner.get();
            let mut total = 0;
            for chunk in &inner.chunks {
                total += chunk.capacity();
            }
            total
        }
    }

    pub fn reset(&self) {
        unsafe {
            let inner = &mut *self.inner.get();
            for chunk in &mut inner.chunks {
                chunk.reset();
            }
            inner.current_chunk.store(0, Ordering::Release);
            inner.checkpoints.clear();
        }
        // Reset allocation counter
        self.allocation_counter.store(0, Ordering::Release);
    }

    /// Reset the arena and deallocate excess chunks.
    ///
    /// This keeps only the first chunk (or a minimal set) and deallocates
    /// all others, reducing memory footprint after allocation spikes.
    pub fn reset_and_shrink_to_fit(&self) {
        unsafe {
            let inner = &mut *self.inner.get();

            // Reset all chunks
            for chunk in &mut inner.chunks {
                chunk.reset();
            }

            // Keep only the first chunk
            if inner.chunks.len() > 1 {
                let chunks_to_remove = inner.chunks.drain(1..);
                for chunk in chunks_to_remove {
                    // Chunk will be dropped and memory freed
                    drop(chunk);
                }
            }

            inner.current_chunk.store(0, Ordering::Release);
            inner.checkpoints.clear();
        }
        // Reset allocation counter
        self.allocation_counter.store(0, Ordering::Release);
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(feature = "single_thread_fast"))]
unsafe impl Send for Arena {}

impl ArenaBuilder {
    pub fn new() -> Self {
        Self {
            initial_capacity: 0,
            chunk_size: crate::DEFAULT_CHUNK_SIZE,
            reserve_size: None,
            max_chunks: None,
            fast_path_threshold: 1024,
            prefetch_distance: 8,
            enable_stats: cfg!(feature = "stats"),
            enable_debug: cfg!(feature = "debug"),
            enable_lockfree: cfg!(feature = "lockfree"),
            enable_thread_local: cfg!(feature = "thread_local"),
            enable_virtual_memory: cfg!(feature = "virtual_memory"),
            feature_bundle: None,
            diagnostics_sink: None,
        }
    }

    // Capacity methods
    pub fn initial_capacity(mut self, capacity: usize) -> Self {
        self.initial_capacity = capacity;
        self
    }
    pub fn reserve_size(mut self, size: usize) -> Self {
        self.reserve_size = Some(size);
        self
    }

    // Performance tuning
    pub fn fast_path_threshold(mut self, threshold: usize) -> Self {
        self.fast_path_threshold = threshold;
        self
    }

    pub fn prefetch_distance(mut self, distance: usize) -> Self {
        self.prefetch_distance = distance;
        self
    }

    // Feature bundles
    pub fn with_bundle(mut self, bundle: FeatureBundle) -> Self {
        self.feature_bundle = Some(bundle);
        bundle.apply_to_builder(&mut self);
        self
    }

    pub fn perf_bundle(self) -> Self {
        self.with_bundle(FeatureBundle::Perf)
    }

    pub fn safety_bundle(self) -> Self {
        self.with_bundle(FeatureBundle::Safety)
    }

    pub fn debuggable_bundle(self) -> Self {
        self.with_bundle(FeatureBundle::Debuggable)
    }

    pub fn server_bundle(self) -> Self {
        self.with_bundle(FeatureBundle::Server)
    }

    // Individual features
    pub fn enable_stats(mut self, enable: bool) -> Self {
        self.enable_stats = enable;
        self
    }

    pub fn enable_debug(mut self, enable: bool) -> Self {
        self.enable_debug = enable;
        self
    }

    pub fn enable_lockfree(mut self, enable: bool) -> Self {
        self.enable_lockfree = enable;
        self
    }

    pub fn enable_thread_local(mut self, enable: bool) -> Self {
        self.enable_thread_local = enable;
        self
    }

    pub fn enable_virtual_memory(mut self, enable: bool) -> Self {
        self.enable_virtual_memory = enable;
        self
    }

    // Diagnostics
    pub fn diagnostics_sink<F>(mut self, sink: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.diagnostics_sink = Some(Box::new(sink));
        self
    }

    // Build
    pub fn build(self) -> Arena {
        let capacity = if self.initial_capacity == 0 {
            Arena::DEFAULT_CAPACITY
        } else {
            self.initial_capacity
        };

        // Log configuration if diagnostics sink is provided
        if let Some(ref sink) = self.diagnostics_sink {
            sink(&format!(
                "Building arena with capacity: {}, chunk_size: {}",
                capacity, self.chunk_size
            ));
        }

        Arena::with_capacity(capacity)
    }
}

impl Default for ArenaBuilder {
    fn default() -> Self {
        Self::new()
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
}
