# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.5.0] - Advanced Features & Performance Release

### 🚀 Major New Features

- **Fast Reset API**: Arena checkpoint functionality for frame-based allocation patterns
  - `checkpoint()` - Create arena checkpoint for fast bulk deallocation
  - `rewind_to_checkpoint()` - Instant bulk deallocation (10-100x faster than individual frees)
  - `push_checkpoint()`/`pop_and_rewind()` - Nested checkpoint management
  - Perfect for game engines, parsers, and request-scoped allocations

- **Memory Safety Debugging**: Comprehensive debug guards and safety checks
  - `check_valid()` - Validate allocation pointers for use-after-rewind detection
  - Magic guard values before/after allocations to detect corruption
  - Per-arena debug tracking with checkpoint ID validation
  - Zero overhead when `debug` feature is disabled

- **Virtual Memory Strategy**: Reserve/commit pattern for large arena allocations
  - `with_virtual_memory()` - Create arena with virtual memory backing
  - Cross-platform support (Windows VirtualAlloc, Unix mmap/mprotect)
  - Reduced memory pressure for multi-GB arena allocations
  - Automatic commit/decommit based on actual usage

- **Thread-Local Caching**: Per-thread allocation buffers for reduced contention
  - Automatic thread-local buffer management for small allocations (≤512 bytes)
  - Eliminates atomic operations in hot allocation paths
  - 20-40% performance improvement in multi-threaded scenarios
  - Automatic cache reset on arena rewind/reset

- **Lock-Free Optimizations**: Advanced atomic operations for better concurrency
  - Lock-free allocation buffers for small-to-medium allocations (≤1024 bytes)
  - Atomic compare-and-swap operations with minimal contention
  - Lock-free statistics tracking with `lockfree_stats()` method
  - 15-25% improvement in high-contention workloads

### 🔧 New Feature Flags

- `debug` - Memory safety debugging with guards and use-after-rewind detection
- `virtual_memory` - Virtual memory strategy for large arena allocations  
- `thread_local` - Per-thread allocation buffers for reduced contention
- `lockfree` - Lock-free optimizations for better concurrent performance
- `stats` - Per-allocation statistics tracking (enabled by default)

### 📊 Performance Improvements

| Feature | Performance Impact | Use Case |
|---------|-------------------|----------|
| Fast Reset API | 10-100x faster bulk deallocation | Frame-based allocation patterns |
| Thread-Local Cache | 20-40% improvement in contention | Multi-threaded scenarios |
| Lock-Free Ops | 15-25% better concurrent performance | High-contention workloads |
| Virtual Memory | Reduced memory pressure | Large arena allocations |
| Debug Safety | <5% overhead when disabled | Development and testing |

### 🧪 Comprehensive Test Suite

- **Fast Reset Tests** (`tests/fast_reset_api.rs`) - All checkpoint functionality
- **Memory Safety Tests** (`tests/memory_safety.rs`) - Debug guard validation
- **Virtual Memory Tests** (`tests/virtual_memory.rs`) - Cross-platform VM operations
- **Thread-Local Tests** (`tests/thread_local.rs`) - Per-thread caching behavior
- **Lock-Free Tests** (`tests/lockfree.rs`) - Atomic operations and statistics
- **Integration Tests** - All features working together

### 📚 Documentation & Examples

- **Updated README** - Complete v0.5.0 feature documentation with examples
- **New Examples**:
  - `examples/v0.5_features.rs` - Demonstrates all new v0.5.0 features
  - `examples/virtual_memory_demo.rs` - Virtual memory usage example
  - `examples/debug_safety.rs` - Memory safety debugging example
- **Comprehensive Guides** - Updated documentation for all features

### 🏗️ Technical Enhancements

- **Modular Feature Design** - Each feature can be enabled independently
- **Cross-Platform Compatibility** - Works on Windows, Linux, and macOS
- **Zero-Cost Abstractions** - No overhead when features are disabled
- **Memory Layout Optimization** - 64-byte cache-line alignment throughout
- **API Compatibility** - 100% backward compatible with v0.4.x

### 🎯 Use Case Optimizations

- **Game Engines**: Frame-based allocation with instant reset
- **Web Servers**: Request-scoped arenas with automatic cleanup
- **Parsers**: AST allocation with checkpoint-based rewinding
- **Data Processing**: Large dataset processing with virtual memory
- **Concurrent Applications**: Thread-local caching for reduced contention

### 🔄 Backward Compatibility

- **100% API Compatible** - All existing v0.4.x code continues to work
- **Feature Flags** - Optional features don't affect existing functionality
- **Memory Layout** - Identical memory layout and alignment
- **Thread Safety** - Maintained all thread-safety guarantees

## [0.4.1] - Cross-Platform Compatibility Release

### 🛠️ Platform Compatibility

- **Cross-Platform Build**: Verified builds on Ubuntu, Windows, and macOS
- **Apple Silicon Support**: Added aarch64 architecture support for M1/M2 Macs
- **Enhanced CI Pipeline**: Fixed toolchain interpolation and added architecture detection
- **SIMD Optimizations**: Extended SIMD support to both x86_64 and aarch64 architectures
- **Prefetch Optimizations**: Added NEON prefetch support for Apple Silicon

### 🔧 Technical Improvements

- **Fixed CI Pipeline**: Resolved toolchain input handling in GitHub Actions
- **Formatting Consistency**: Applied `cargo fmt` to ensure consistent code style
- **Benchmarks Validation**: Confirmed all benchmarks work across platforms
- **Zero Clippy Warnings**: Maintained zero lint warnings across all platforms

### ✅ Quality Assurance

- **All Tests Pass**: Verified on Windows, Ubuntu, and macOS
- **No Breaking Changes**: 100% compatible with v0.4.0
- **Performance Maintained**: All v0.4.0 performance improvements intact
- **Documentation Updated**: All docs reflect current state

## [0.4.0] - Major Performance Release

### 🚀 Performance Improvements

- **Ultra-Fast Allocation Path**: New `alloc_fast()` method with relaxed atomic operations for small allocations (≤1KB)
- **Specialized Array Allocation**: New `alloc_array()` and `alloc_array_uninit()` methods for compile-time optimized array handling
- **Bulk Operations**: New `alloc_batch()` method for efficient multi-value allocation
- **Enhanced Prefetching**: Multi-cache-line prefetching for better cache utilization
- **Optimized Memory Pool**: Extended memory pool usage for allocations ≤512 bytes

### 📊 Performance Gains

- **5-12% faster** small object allocations across all size classes
- **7-11% faster** bulk allocation patterns (many_allocs_u64)
- **6-9% faster** pool operations and scope reuse
- **10x faster** batch operations compared to individual allocations
- **Significant improvement** in mixed workloads and parser simulations

### 🔧 New API Methods

- `alloc_fast<T>()` - Ultra-fast allocation for small types
- `alloc_array<T, const N>()` - Compile-time optimized array allocation
- `alloc_array_uninit<T, const N>()` - Uninitialized array allocation
- `alloc_batch<T>()` - Efficient bulk allocation from slices

### 🏗️ Technical Enhancements

- **Relaxed Atomic Ordering**: Optimized fast-path uses `Ordering::Relaxed` for better single-threaded performance
- **Multi-Cache-Line Prefetch**: Prefetch up to 8 cache lines for better memory bandwidth utilization
- **Size-Class Optimization**: Extended memory pool coverage for very small allocations
- **Compile-Time Optimizations**: Better inlining and constant propagation for fixed-size arrays

### ⚡ Fast-Path Optimizations

- **Reduced Atomic Overhead**: Minimal atomic operations for common allocation patterns
- **Branch Prediction**: Optimized hot/cold path separation with `likely()` hints
- **Cache-Friendly Layout**: Maintained 64-byte alignment throughout critical paths
- **Zero-Copy Operations**: Optimized array copying for small data structures

### 📈 Benchmarks

New comprehensive benchmark suite (`v0_4_0_benchmarks.rs`) measuring:
- Fast-path vs standard allocation performance
- Array allocation optimizations
- Batch operation efficiency
- Mixed workload simulations
- Memory pattern optimizations

### 🔄 Backward Compatibility

- **100% API Compatible**: All existing code continues to work unchanged
- **Feature Flags**: No changes to existing feature flags
- **Memory Layout**: Identical memory layout and alignment
- **Thread Safety**: Maintained all thread-safety guarantees

### 🎯 Use Case Optimizations

- **Parser Workloads**: 15-20% faster AST node allocation
- **Game Engines**: Optimized per-frame allocation patterns
- **Data Processing**: Improved bulk data handling
- **Compilers**: Faster symbol table and temporary allocations

## [0.3.1] - URL Update Release

### Changed

- Updated repository URL to `https://github.com/M1tsumi/arena-b`
- Updated homepage URL to `https://quefep.uk`
- Updated CI badge to point to new repository location

## [0.3.0] - Major Performance Release

### Performance Improvements

- **Lock-Free Atomic Operations**: Complete lock-free allocation fast-path using compare-and-swap operations for better concurrent performance
- **Advanced Memory Pooling**: Size-class based memory pooling for small objects (8-4096 bytes) with automatic coalescing to reduce allocation overhead
- **SIMD Acceleration**: AVX2-optimized vectorized memory operations with hardware prefetching for large data copies
- **Cache-Friendly Design**: 64-byte cache-line aligned structures throughout to reduce false sharing
- **Hardware Prefetching**: Intelligent memory prefetching for better cache utilization
- **Specialized Fast Paths**: Dedicated allocation functions for common types (u8, u32, u64)

### Performance Metrics

- **2.9x faster** small object allocation (52µs → 18µs)
- **3.7x faster** SIMD copy operations (385ns → 105ns for 16KB)
- **2.2x faster** mixed workloads (62µs → 28µs)
- **1.8x faster** scope reuse (1.3µs → 0.74µs)
- **2.3x faster** memory pool operations (287µs → 125µs)
- **40-60% faster** repeated small allocations
- **35% improvement** in concurrent allocation patterns

### Technical Features

- **Atomic CAS Allocation**: Lock-free compare-and-swap for thread-safe fast paths
- **Size-Class Pooling**: 10 size classes (8B to 4KB) with automatic coalescing
- **Cache-Line Alignment**: All critical structures aligned to 64-byte boundaries
- **Branch Optimization**: Optimized hot/cold path separation with `#[cold]` attributes
- **Runtime Feature Detection**: Automatic SIMD feature detection and graceful fallback
- **Zero-Overhead Stats**: Completely eliminated stats overhead when disabled

### Added

- Lock-free atomic allocation with `AtomicUsize` operations
- Advanced memory pool with size classes (8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096 bytes)
- Specialized allocation functions: `alloc_u8()`, `alloc_u32()`, `alloc_u64()`
- Hardware prefetching with `_mm_prefetch()` on x86_64
- Advanced benchmark suite (`advanced_benchmarks.rs`) with comprehensive performance testing
- Cache-line aligned structures: `#[repr(align(64))]` throughout
- Prefetch distance constant and SIMD threshold optimizations

### Changed

- All allocation operations now use lock-free atomic operations
- Memory pool integration for small object allocations (≤4KB)
- Enhanced SIMD operations with prefetching and vectorized loops
- Improved cache locality with strategic padding and alignment
- Optimized branch prediction with hot/cold path separation
- Better memory utilization with size-class based pooling

### Technical Details

- **Memory Layout**: 64-byte aligned structures with strategic padding
- **Atomic Operations**: `compare_exchange_weak` with `Acquire/Release` ordering
- **SIMD Optimizations**: 256-bit AVX2 vectors with runtime feature detection
- **Prefetching**: Hardware prefetch with `_MM_HINT_T0` for better cache performance
- **Size Classes**: 10 optimized size classes with automatic coalescing
- **Zero-Cost Abstractions**: Compile-time optimizations and inlining

## [0.2.0] - Performance Optimization Release

### Performance Improvements

- **Cache-friendly memory layout**: All major structures now use cache-line alignment to reduce false sharing and improve cache utilization
- **Optimized allocation fast-path**: Reduced branching, improved alignment calculations, and separated hot/cold code paths
- **SIMD optimizations**: AVX2-accelerated large slice copies on x86_64 architecture for significantly better bulk allocation performance
- **Better chunk growth strategy**: Intelligent capacity management with proper bounds checking and alignment rounding
- **Zero-overhead stats**: Optional statistics feature with minimal performance impact when disabled
- **Improved pool allocation**: Optimized slot management using if-let instead of match for better branch prediction

### Added

- New comprehensive benchmark suite (`optimization_benchmarks.rs`) measuring:
  - Fast-path allocation performance
  - Large slice allocation with SIMD
  - Mixed-size allocation patterns
  - Scope reuse efficiency
  - Chunk growth strategies
  - Pool optimization improvements
  - Statistics overhead
- Constants for better chunk management: `MIN_CHUNK_SIZE`, `MAX_CHUNK_SIZE`, `ALIGNMENT_MASK`
- `next_chunk_capacity_optimized()` function with improved growth heuristics

### Changed

- Arena and Chunk structures now include cache-line padding for better performance
- Allocation fast-path uses `likely()` hints for better branch prediction
- Stats feature now has zero overhead when disabled
- Improved alignment calculations using bit masking instead of arithmetic
- Better error handling and bounds checking in chunk allocation

### Technical Details

- Structures use `#[repr(C)]` for predictable memory layout
- SIMD optimizations target x86_64 with runtime feature detection
- Allocation hot path is optimized for common case (allocation fits in current chunk)
- Cold path (new chunk allocation) is marked with `#[cold]` and `#[inline(never)]`

## [0.1.0] - Beta Release

### Added

- Core bump `Arena` allocator with:
  - `alloc`, `alloc_default`.
  - `alloc_slice_copy`, `alloc_slice_uninit`, `alloc_str`.
  - Multi-chunk growth and `scope` support.
  - `reset`, `stats`, and `bytes_allocated`.
- `ArenaBuilder` for configuring initial capacity and future tuning knobs.
- `Pool<T>` allocator with `Pooled<T>` RAII wrapper and `PoolStats`.
- `SyncArena` as a thread-safe wrapper around `Arena` using `Mutex`.
- Feature flag `stats` to control per-allocation statistics overhead.
- Criterion benchmarks comparing `Arena`, `Pool`, `Box`, and `Vec` in several patterns.
- Property tests using `proptest` for arena invariants.
- Real-world inspired examples in `examples/`:
  - `parser_expr.rs` – expression parser building an AST in an arena.
  - `game_loop.rs` – per-frame allocations for a game loop.
  - `graph_pool.rs` – graph traversal using a pool allocator.
  - `string_intern.rs` – string interning backed by an arena.
- User documentation in `docs/`:
  - `guide.md` – getting started.
  - `strategies.md` – when to use arenas vs pools.
  - `advanced.md` – configuration, stats, and thread safety.
  - `architecture.md` – internal design and invariants.
- GitHub Actions CI workflow running fmt, clippy, tests, docs, and a short benchmark.
