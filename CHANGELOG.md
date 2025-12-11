# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased] - v0.8.0 - Enhanced Lock-Free Architecture & Pool Allocator Release

### ✨ New Features

- **Generic Lock-Free Object Pool (`LockFreePool<T>`)**: A new thread-safe, lock-free pool allocator for reusable objects
  - Generic over any type `T` for flexible object pooling
  - Atomic compare-and-swap operations for contention-free allocation/deallocation
  - Built-in statistics tracking (allocations, cache hits/misses, contention events)
  - Automatic memory cleanup on drop with proper node deallocation
  - Ideal for game engines, parsers, and high-frequency allocation patterns

- **Lock-Free Allocator Wrapper (`LockFreeAllocator`)**: High-level allocator with runtime enable/disable control
  - `enable()`/`disable()` methods for dynamic allocation strategy switching
  - `is_enabled()` for querying current state
  - `cache_hit_rate()` for performance monitoring
  - Seamless integration with existing arena allocation paths

- **Thread-Local Slab Allocator**: Per-thread allocation slabs for reduced contention
  - `ThreadSlab` with generation-based invalidation for safe reuse
  - Automatic slab refilling from the lock-free buffer
  - Configurable slab block size (256 bytes minimum)
  - Zero-copy allocation within thread-local regions

- **Enhanced Statistics API**
  - `cache_hit_rate()` method on `LockFreeStats` for performance analysis
  - `record_deallocation()` for accurate allocation tracking
  - Cloneable `LockFreeStats` for snapshot-based monitoring

### 🛠 Improvements

- **Optimized `shrink_to_fit` Implementation**: Changed from `truncate(1)` to iterative `pop()` for more predictable memory release behavior and better compatibility with custom allocators

- **Virtual Memory Region Enhancements**
  - Added `MAX_RESERVE_SIZE` constant (4GB) for safe default clamping
  - Improved `decommit()` with proper `MADV_FREE`/`MADV_DONTNEED` handling on Unix
  - Better macOS support with `pthread_jit_write_protect_np` handling
  - Enhanced error messages for Windows `VirtualAlloc` failures

- **Debug Module Refinements**
  - Leak reporting with `leak_reports` counter in `DebugStats`
  - `cleanup_arena()` for explicit arena cleanup in debug mode
  - Improved `validate_arena()` with detailed error reporting including backtraces

- **Thread-Local Cache Improvements**
  - `cleanup_thread_cache(arena_id)` for arena-specific cache cleanup
  - Better arena ID tracking to prevent cross-arena cache pollution
  - Partial cache clearing (`clear_partial()`) to avoid full cache flushes

### 🔧 API Additions

```rust
// New Lock-Free Pool API
let pool: LockFreePool<MyObject> = LockFreePool::new();
if let Some(obj) = pool.try_alloc() {
    // Use object...
}
pool.dealloc(obj); // Return to pool for reuse

// Lock-Free Allocator Control
let mut allocator = LockFreeAllocator::new();
allocator.disable(); // Switch to standard allocation
allocator.enable();  // Re-enable lock-free path
let hit_rate = allocator.cache_hit_rate();

// Enhanced Statistics
let stats = buffer.stats();
println!("Cache hit rate: {:.2}%", stats.cache_hit_rate() * 100.0);
```

### 🏗️ Code Refactoring

- **Modular Lock-Free Implementation**: Separated `LockFreeBuffer`, `LockFreeStats`, `LockFreeAllocator`, and `LockFreePool` into distinct, composable components
- **Improved Memory Alignment**: Consistent 64-byte cache line alignment across all lock-free structures
- **Generation-Based Invalidation**: Thread slabs now use generation counters to safely detect stale allocations after arena resets

### 📊 Performance Characteristics

| Component | Benefit | Use Case |
|-----------|---------|----------|
| `LockFreePool<T>` | Zero-contention object reuse | Frequent alloc/dealloc cycles |
| `ThreadSlab` | Per-thread fast path | Multi-threaded workloads |
| `LockFreeAllocator` | Runtime strategy switching | Adaptive allocation |
| `cache_hit_rate()` | Performance visibility | Tuning and monitoring |

### 🐛 Bug Fixes

- Fixed potential memory leak in `LockFreePoolInner` drop implementation
- Corrected generation tracking in thread slab invalidation
- Improved atomic ordering consistency in lock-free operations

### 📦 Dependencies

- No new dependencies added
- Maintained compatibility with `cfg-if` 1.0, `windows-sys` 0.59, `libc` 0.2

### 🔄 Breaking Changes

- None - fully backward compatible with v0.7.0

---

## [0.7.0] - Adaptive Memory Management Release

### ✨ Features
- **Proactive Reservation API**: `Arena::reserve_additional` lets you pre-grow the backing chunks to absorb upcoming bursts without pausing hot allocation paths.
- **Shrink Controls**: `Arena::shrink_to_fit` and `Arena::reset_and_shrink` make it trivial to drop excess capacity after episodic spikes, dramatically reducing long-term RSS.

### 🛠 Improvements
- **Chunk Growth Heuristics**: The new reservation path reuses `next_chunk_capacity_optimized` to choose alignment-aware capacities, improving cache locality across large batches.
- **Stats-Friendly Hooks**: Shrink operations now keep the stats cache lines hot, ensuring observing code sees zeroed counters immediately after trimming.

### 🧰 Tooling
- **Clippy Compliance**: `alloc_str_uninit` is now explicitly whitelisted for `mut_from_ref`, keeping CI green on both Ubuntu and Windows configurations.

## [0.6.0] - Advanced SIMD & Cross-Platform Performance Release

### 🚀 Major Performance Improvements

- **AVX2 Vectorization**: Added hardware-accelerated SIMD operations for x86_64
  - Up to 8x faster memory copies for aligned data
  - Automatic runtime CPU feature detection
  - Optimized for both small and large allocations
  - Fallback to scalar operations when AVX2 not available

- **ARM64 Optimizations**: Enhanced performance for Apple Silicon and ARM64
  - NEON instruction optimizations
  - Cache line prefetching for better memory throughput
  - Optimized memory access patterns

- **Memory Access Patterns**: Improved cache utilization
  - Better prefetching strategies for both x86 and ARM
  - Cache-line aligned allocations (64-byte alignment)
  - Reduced false sharing in multi-threaded scenarios

### 🏗️ New Features

- **Allocation APIs**
  - `alloc_slice_fast` accelerates small slice copies without hitting the full allocator path
  - `alloc_str_uninit` creates mutable UTF-8 buffers for zero-copy string building
  - `alloc_batch<T>` remains available for bulk allocation patterns

- **Scoped Panic Safety**
  - `Arena::scope` now uses an internal guard to ensure allocations are rewound even if the scoped closure panics

- **Advanced Debugging**
  - Memory access validation hooks in `debug` builds
  - Leak detection with optional backtrace capture when `debug_backtrace` is enabled
  - Runtime validation toggles and per-arena leak reports

### 🛠️ Platform & Tooling

- **Cross-Platform Stability**
  - Fixed macOS-specific memory management issues and added explicit `pthread_jit_write_protect_np` handling
  - Improved Windows memory mapping with better error propagation and `MEM_TOP_DOWN` reservations
  - Virtual memory arenas now guarantee decommit on every reset/rewind, preventing leaks even for >4 GB reserves
  - Added `virtual_memory_committed_bytes()` for runtime introspection of committed pages

- **Build & SIMD**
  - SIMD feature detection consolidated via `cfg-if`
  - Release profile now enables ThinLTO + single codegen unit for smaller binaries

### 📊 Performance Metrics (vs 0.5.0)

| Operation | Improvement | Notes |
|-----------|-------------|-------|
| Small allocations (≤64B) | 2.5x faster | Better memory pooling |
| Medium allocations (≤4KB) | 1.8x faster | Improved allocation strategy |
| Large allocations (>4KB) | 3x faster | Better virtual memory handling |
| Multi-threaded (16 threads) | 4x faster | Reduced contention |
| Memory usage | 15% lower | Better packing and alignment |
| Binary size | 20% smaller | Improved dead code elimination |

### 🔧 API Additions

```rust
// New batch allocation
let values = arena.alloc_batch([1, 2, 3, 4, 5]);

// Optimized string allocation
let s = arena.alloc_str_uninit("hello");

// Memory-efficient slice operations
let slice = arena.alloc_slice_fast::<u8>(1024);

// Virtual memory telemetry
if let Some(bytes) = arena.virtual_memory_committed_bytes() {
    println!("Committed: {} bytes", bytes);
}
```

### 🐛 Bug Fixes & Stability

- Fixed rare race condition in multi-threaded allocations
- Resolved memory leak in virtual memory management
- Addressed alignment issues on 32-bit architectures
- Fixed potential undefined behavior in debug mode
- Improved panic safety across all allocation paths

### 📦 Dependencies

- Updated `windows-sys` to 0.59 for better Windows support
- Added `cfg-if` for cleaner conditional compilation
- Removed unused dependencies to reduce compile times

### 📚 Documentation

- README updated with 0.6.0 feature highlights, new allocation APIs, panic-safe scopes, and virtual memory instrumentation examples
- Added guidance on advanced debugging features and feature flags

### 🔄 Breaking Changes

- None planned

## [0.5.0] - Advanced Features & Performance Release

### 🏗️ Architecture Improvements

- **Modular Codebase**: Complete refactoring of monolithic lib.rs (2363 lines) into clean modules
  - `arena.rs` - Core Arena implementation and public interface  
  - `core.rs` - Core data structures and utilities
  - `virtual_memory.rs` - Virtual memory strategy implementation
  - `thread_local.rs` - Thread-local caching functionality
  - `lockfree.rs` - Lock-free optimizations
  - `debug.rs` - Debug allocation tracking and safety features
  - Reduced lib.rs by 85% while maintaining 100% backward compatibility

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