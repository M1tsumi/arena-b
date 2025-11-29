# arena-b

[![Crates.io](https://img.shields.io/crates/v/arena-b.svg)](https://crates.io/crates/arena-b)
[![Docs.rs](https://docs.rs/arena-b/badge.svg)](https://docs.rs/arena-b)
[![CI](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/language-Rust-orange.svg)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

`arena-b` is a **high-performance** bump allocator / arena allocator and memory pool crate for Rust. It is designed for allocation-heavy workloads such as parsers, compilers, game engines, simulations, and data processing, while keeping a clean, idiomatic Rust API.

The core type is `arena_b::Arena`, a bump allocator that lets you allocate many values cheaply and reclaim them all at once when the arena is reset or dropped.

## ✨ v0.5.0 Highlights

- **🚀 Fast Reset API**: Arena checkpoint functionality for frame-based patterns
- **🛡️ Memory Safety**: Debug guards and use-after-rewind detection
- **💾 Virtual Memory**: Reserve/commit pattern using VirtualAlloc/mmap for large allocations
- **🧵 Thread-Local Caching**: Per-thread allocation buffers for reduced contention
- **⚡ Lock-Free Optimizations**: Atomic operations for better concurrent performance
- **🔄 100% Backward Compatible**: Drop-in upgrade from v0.4.x

## Installation

`arena-b` is published on [crates.io](https://crates.io/crates/arena-b).

Add it to your `Cargo.toml`:

```toml
[dependencies]
arena-b = "0.5"
```

Or, using `cargo add`:

```bash
cargo add arena-b
```

### Feature Flags

`arena-b` provides several optional features that can be enabled for specific use cases:

```toml
[dependencies]
arena-b = { version = "0.5", features = ["debug", "virtual_memory", "thread_local", "lockfree"] }
```

Available features:
- `debug` (default: disabled): Memory safety debugging with guards and use-after-rewind detection
- `virtual_memory` (default: disabled): Virtual memory strategy for large arena allocations
- `thread_local` (default: disabled): Per-thread allocation buffers for reduced contention
- `lockfree` (default: disabled): Lock-free optimizations for better concurrent performance
- `stats` (default: enabled): Per-allocation statistics tracking

### Using a local checkout (for contributors)

If you are hacking on `arena-b` itself, depend on it via a local path:

```toml
[dependencies]
arena-b = { path = "../bumper" }
```

## Getting started

The simplest way to use `arena-b` is with an arena:

```rust
use arena_b::Arena;

fn main() {
    let arena = Arena::new();
    let value = arena.alloc(42_u32);
    assert_eq!(*value, 42);
}
```

### 🚀 v0.5.0: Fast Reset API

Use the new checkpoint functionality for frame-based allocation patterns:

```rust
use arena_b::Arena;

fn main() {
    let arena = Arena::new();
    
    // Create a checkpoint for fast bulk deallocation
    let checkpoint = arena.checkpoint();
    
    // Make many allocations...
    for i in 0..1000 {
        let value = arena.alloc(i);
        // ... use value
    }
    
    // Fast rewind - much faster than individual deallocations
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }
}
```

### 🛡️ v0.5.0: Memory Safety Debugging

Enable debug mode for memory safety validation:

```rust
use arena_b::Arena;

#[cfg(feature = "debug")]
fn main() {
    let arena = Arena::new();
    let checkpoint = arena.checkpoint();
    
    let value = arena.alloc(42u32);
    
    // Check validity before rewind
    unsafe { arena.check_valid(value).unwrap(); }
    
    arena.rewind_to_checkpoint(checkpoint);
    
    // This will detect use-after-rewind
    unsafe { 
        assert!(arena.check_valid(value).is_err());
    }
}
```

### 💾 v0.5.0: Virtual Memory Strategy

Use virtual memory for large arena allocations:

```rust
#[cfg(feature = "virtual_memory")]
use arena_b::Arena;

fn main() {
    // Create arena with virtual memory backing (16MB reserve)
    let arena = Arena::with_virtual_memory(16 * 1024 * 1024);
    
    // Large allocations will use virtual memory efficiently
    let large_data = arena.alloc([0u8; 1_000_000]);
    
    // Regular allocations still work
    let small_value = arena.alloc(42u32);
}
```

### 🧵 v0.5.0: Thread-Local Caching

Enable thread-local caching for reduced contention:

```rust
#[cfg(feature = "thread_local")]
use arena_b::Arena;

fn main() {
    let arena = Arena::new();
    
    // Small allocations will use thread-local cache
    for i in 0..1000 {
        let value = arena.alloc(i); // Uses thread-local buffer
        // ... use value
    }
}
```

### ⚡ v0.5.0: Lock-Free Optimizations

Enable lock-free operations for better concurrent performance:

```rust
#[cfg(feature = "lockfree")]
use arena_b::Arena;

fn main() {
    let arena = Arena::new();
    
    // Small-to-medium allocations use lock-free buffer
    for i in 0..1000 {
        let value = arena.alloc(i); // Lock-free allocation
        // ... use value
    }
    
    // Check lock-free statistics
    let (allocations, cache_hits, cache_misses, contention) = arena.lockfree_stats();
    println!("Allocations: {}, Cache hits: {}, Misses: {}, Contention: {}", 
             allocations, cache_hits, cache_misses, contention);
}
```

### Scoped allocations

Use `Arena::scope` to allocate many temporary values and free them all at once:

```rust
use arena_b::Arena;

fn main() {
    let arena = Arena::new();

    arena.scope(|scope| {
        let buf = scope.alloc_slice_uninit::<u8>(1024);
        // initialize buf...
    });

    // all allocations done in the scope have been reclaimed here
}
```

### Pool allocator

Use `Pool<T>` when you have many values of the same type that are reused:

```rust
use arena_b::Pool;

fn main() {
    let pool = Pool::<String>::with_capacity(128);

    let mut name = pool.alloc(String::from("player"));
    name.push_str("_1");
} // `name` is returned to the pool on drop
```

## Features

- **Bump arena (`Arena`)**
  - `alloc`, `alloc_default`
  - `alloc_slice_copy`, `alloc_slice_uninit`, `alloc_str`
  - Multi-chunk growth when the arena is full
  - `scope` API for scoped allocations with automatic reclamation
  - `reset`, `stats`, and `bytes_allocated`
  - **NEW v0.5.0**: Fast reset API with checkpoints
  - **NEW v0.5.0**: Memory safety debugging
  - **NEW v0.5.0**: Virtual memory strategy
  - **NEW v0.5.0**: Thread-local caching
  - **NEW v0.5.0**: Lock-free optimizations

- **Configurable arenas (`ArenaBuilder`)**
  - Control `initial_capacity`
  - Hooks for future `chunk_size` and `thread_safe` configuration

- **Pool allocator (`Pool<T>`)**
  - Slot-based allocator for many values of the same type
  - `Pooled<T>` RAII wrapper that returns slots to the pool on drop
  - `PoolStats` for capacity and usage information

- **Thread-safe wrapper (`SyncArena`)**
  - Wraps `Arena` in a `Mutex` for multi-threaded use
  - Safe to share via `Arc<SyncArena>` across threads

- **Feature flags**
  - `debug`: Memory safety debugging with guards and use-after-rewind detection
  - `virtual_memory`: Virtual memory strategy for large allocations
  - `thread_local`: Per-thread allocation buffers
  - `lockfree`: Lock-free optimizations
  - `stats`: Per-allocation statistics (enabled by default)

- **Tooling and quality**
  - Criterion benchmarks comparing `Arena`, `Pool`, `Box`, and `Vec`
  - Property-based tests using `proptest`
  - Comprehensive test suite for all v0.5.0 features
  - GitHub Actions CI: fmt, clippy, tests, docs, and a short bench

## Documentation

See the `docs/` directory:

- `docs/guide.md` – Getting started with `Arena`, `Pool`, and `SyncArena`.
- `docs/strategies.md` – When to use an arena vs a pool.
- `docs/advanced.md` – Configuration, stats feature, thread safety, and benchmarking.
- `docs/architecture.md` – Internal design, invariants, and unsafe code strategy.

## Examples

Real-world inspired examples are in `examples/`:

- `examples/parser_expr.rs` – Expression parser building an AST in an arena.
- `examples/game_loop.rs` – Per-frame allocations in a game loop using scopes.
- `examples/graph_pool.rs` – Graph traversal using a pool allocator.
- `examples/string_intern.rs` – String interning backed by an arena.
- **NEW v0.5.0**: `examples/v0.5_features.rs` – Demonstrates all new v0.5.0 features
- **NEW v0.5.0**: `examples/virtual_memory_demo.rs` – Virtual memory usage example
- **NEW v0.5.0**: `examples/debug_safety.rs` – Memory safety debugging example

Run an example with:

```bash
cargo run --example parser_expr
cargo run --example v0.5_features
```

## Performance snapshot

**Version 0.5.0 - Advanced Features & Performance:**

The 0.5.0 release builds on the performance optimizations of v0.3.0 and adds advanced features for production use:

### Key Performance Improvements

- **Fast Reset API**: Checkpoint-based bulk deallocation (10-100x faster than individual frees)
- **Thread-Local Caching**: Per-thread buffers reduce atomic contention (20-40% improvement in multi-threaded scenarios)
- **Lock-Free Operations**: Atomic operations for better concurrent performance
- **Virtual Memory**: Reserve/commit pattern for large allocations (reduced memory pressure)
- **Memory Safety**: Debug guards with minimal overhead when disabled

### v0.5.0 Feature Performance

| Feature | Performance Impact | Use Case |
|---------|-------------------|----------|
| Fast Reset API | 10-100x faster bulk deallocation | Frame-based allocation patterns |
| Thread-Local Cache | 20-40% improvement in contention | Multi-threaded scenarios |
| Lock-Free Ops | 15-25% better concurrent performance | High-contention workloads |
| Virtual Memory | Reduced memory pressure | Large arena allocations |
| Debug Safety | <5% overhead when disabled | Development and testing |

### Previous Performance Improvements (v0.3.0)

- **Lock-Free Atomic Operations**: Lock-free allocation fast-path with compare-and-swap operations for better concurrent performance
- **Advanced Memory Pooling**: Size-class based memory pooling for small objects (8-4096 bytes) reduces allocation overhead
- **SIMD Acceleration**: AVX2-optimized vectorized memory operations with prefetching for large data copies
- **Cache-Friendly Design**: 64-byte cache-line aligned structures throughout to reduce false sharing
- **Hardware Prefetching**: Intelligent memory prefetching for better cache utilization
- **Specialized Fast Paths**: Dedicated allocation functions for common types (u8, u32, u64)

### Benchmark Results

**Small Object Performance:**
- **Small object allocation**: 2-3x faster than standard allocators
- **Memory pool efficiency**: 40-60% faster for repeated small allocations
- **Concurrent patterns**: 35% improvement with scope-based allocation
- **v0.5.0 thread-local**: Additional 20-40% improvement in multi-threaded scenarios

**Large Data Operations:**
- **Large slice copies**: Up to 3x faster for 16KB+ arrays using SIMD
- **Vectorized operations**: 256-bit AVX2 throughput optimization
- **Prefetching benefits**: 15-25% improvement in cache-bound workloads
- **v0.5.0 virtual memory**: Reduced memory pressure for large allocations

**Mixed Workloads:**
- **Realistic allocation patterns**: 50-70% overall performance improvement
- **Memory efficiency**: Reduced fragmentation and better locality
- **Zero-overhead stats**: No performance impact when disabled
- **v0.5.0 fast reset**: 10-100x faster bulk deallocation patterns

### Performance Comparison

| Operation | v0.2.0 | v0.3.0 | v0.5.0 | Improvement |
|-----------|--------|--------|--------|-------------|
| Small object alloc | 52µs | 18µs | 16µs | **3.3x** |
| SIMD copy (16KB) | 385ns | 105ns | 100ns | **3.9x** |
| Mixed workload | 62µs | 28µs | 24µs | **2.6x** |
| Scope reuse | 1.3µs | 0.74µs | 0.70µs | **1.9x** |
| Memory pool | 287µs | 125µs | 115µs | **2.5x** |
| Fast reset | N/A | N/A | 0.02µs | **~100x** vs individual frees |

Benchmarks are in `benches/arena_vs_box.rs`, `benches/optimization_benchmarks.rs`, and `benches/advanced_benchmarks.rs` and use [Criterion](https://crates.io/crates/criterion). Run:

```bash
cargo bench --bench arena_vs_box
cargo bench --bench optimization_benchmarks
cargo bench --bench advanced_benchmarks
cargo bench --bench v0.5_features
cargo bench --bench arena_vs_box --no-default-features
```

to compare `Arena`, `Pool`, `Box`, and `Vec` on your hardware, with and without stats.

## Rendering / game engine use case

In a renderer or game engine you often allocate a lot of **temporary data per frame** (transforms, scratch buffers, intermediate results) and then throw it away.

Using `Arena::scope` for per-frame scratch data lets you:

- Allocate many small objects per frame with very cheap pointer bumps.
- Free everything from that frame in one shot at the end of the scope.
- Avoid thousands of tiny heap allocations and deallocations every frame.
- Reduce heap fragmentation, which can cause random frame-time spikes.

**NEW v0.5.0**: Use the fast reset API for even more efficient frame-based allocation:

```rust
use arena_b::Arena;

struct GameFrame {
    arena: Arena,
    checkpoint: arena_b::ArenaCheckpoint,
}

impl GameFrame {
    fn new() -> Self {
        let arena = Arena::new();
        let checkpoint = arena.checkpoint();
        Self { arena, checkpoint }
    }
    
    fn reset(&mut self) {
        // Much faster than creating a new arena each frame
        unsafe {
            self.arena.rewind_to_checkpoint(self.checkpoint);
        }
        self.checkpoint = self.arena.checkpoint();
    }
}
```

The end result is more **stable and predictable frame times**, which translates into smoother rendering and fewer stutters, especially on long-running scenes.

## Status

- Implemented:
  - Bump `Arena` with multi-chunk support and scopes
  - `Pool<T>` allocator with RAII `Pooled<T>` and `PoolStats`
  - `SyncArena` for thread-safe use
  - `ArenaBuilder` and `stats` feature
  - Benchmarks, tests, CI, and docs
  - **NEW in v0.2.0**: Cache-optimized memory layout and SIMD acceleration
  - **NEW in v0.2.0**: Advanced chunk management and allocation fast-path optimizations
  - **NEW in v0.3.0**: Lock-free atomic operations for better concurrent performance
  - **NEW in v0.3.0**: Advanced memory pooling with size classes
  - **NEW in v0.3.0**: SIMD optimizations with prefetching
  - **NEW in v0.3.0**: Specialized allocation functions for common types
  - **NEW in v0.3.0**: Cache-friendly design with 64-byte alignment
  - **NEW in v0.5.0**: Fast reset API with checkpoints
  - **NEW in v0.5.0**: Memory safety debugging with guards
  - **NEW in v0.5.0**: Virtual memory strategy
  - **NEW in v0.5.0**: Thread-local caching
  - **NEW in v0.5.0**: Lock-free optimizations
  - **NEW in v0.5.0**: Comprehensive test suite for all features

- Planned (for future releases):
  - Allocation coalescing and defragmentation
  - Slab allocator with multiple size classes
  - More advanced debugging and visualization helpers
  - `no_std` support and async-friendly integrations
  - ARM NEON optimizations for slice copies

`arena-b` aims to be a fast, ergonomic Rust arena allocator and memory pool library that feels native to Rust while offering production-grade safety and documentation.

## License

Licensed under either of:

- MIT license
- Apache License, Version 2.0

at your option.

See the `LICENSE` file for details.
