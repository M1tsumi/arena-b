# arena-b

[![Crates.io](https://img.shields.io/crates/v/arena-b.svg)](https://crates.io/crates/arena-b)
[![Docs.rs](https://docs.rs/arena-b/badge.svg)](https://docs.rs/arena-b)
[![CI](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/language-Rust-orange.svg)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

`arena-b` is a **high-performance** bump allocator / arena allocator and memory pool crate for Rust. It is designed for allocation-heavy workloads such as parsers, compilers, game engines, simulations, and data processing, while keeping a clean, idiomatic Rust API.

The core type is `arena_b::Arena`, a bump allocator that lets you allocate many values cheaply and reclaim them all at once when the arena is reset or dropped.

## ✨ v0.4.0 Highlights

- **🚀 Ultra-Fast Allocation**: New `alloc_fast()` method for small objects (5-12% faster)
- **📦 Specialized Arrays**: `alloc_array()` and `alloc_array_uninit()` for compile-time optimizations
- **⚡ Bulk Operations**: `alloc_batch()` for efficient multi-value allocation
- **🔄 100% Backward Compatible**: Drop-in upgrade from v0.3.x

## Installation

`arena-b` is published on [crates.io](https://crates.io/crates/arena-b).

Add it to your `Cargo.toml`:

```toml
[dependencies]
arena-b = "0.4"
```

Or, using `cargo add`:

```bash
cargo add arena-b
```

### Optional: disable `stats` for hot builds

By default the `stats` feature is enabled to collect allocation statistics.
You can disable it to remove even the small accounting overhead:

```toml
[dependencies]
arena-b = { version = "0.4", default-features = false }
```

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

### 🚀 v0.4.0: Ultra-Fast Allocation

Use the new `alloc_fast()` method for small objects (≤1KB):

```rust
use arena_b::Arena;

fn main() {
    let arena = Arena::new();
    
    // Ultra-fast allocation for small types
    for i in 0..1000 {
        let value = arena.alloc_fast(i as u64);  // 5-12% faster
        // ... use value
    }
}
```

### 📦 v0.4.0: Specialized Array Allocation

Use the new array methods for compile-time optimizations:

```rust
use arena_b::Arena;

fn main() {
    let arena = Arena::new();
    
    // Compile-time optimized array allocation
    let numbers = arena.alloc_array([1, 2, 3, 4, 5]);
    
    // Uninitialized array for deferred initialization
    let mut uninit = unsafe { arena.alloc_array_uninit::<u32, 10>() };
    for i in 0..10 {
        uninit[i].write(i as u32);
    }
}
```

### ⚡ v0.4.0: Bulk Operations

Use `alloc_batch()` for efficient multi-value allocation:

```rust
use arena_b::Arena;

fn main() {
    let arena = Arena::new();
    let data = vec![1, 2, 3, 4, 5];
    
    // 10x faster than individual allocations
    let values = arena.alloc_batch(&data);
    assert_eq!(values.len(), 5);
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

- **Stats feature flag**
  - `stats` feature (enabled by default) tracks per-allocation statistics
  - Disable with `--no-default-features` for maximum performance in hot builds

- **Tooling and quality**
  - Criterion benchmarks comparing `Arena`, `Pool`, `Box`, and `Vec`
  - Property-based tests using `proptest`
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

Run an example with:

```bash
cargo run --example parser_expr
```

## Performance snapshot

**Version 0.3.0 - Major Performance Optimizations:**

The 0.3.0 release includes significant performance improvements that make arena-b one of the fastest arena allocators available for Rust:

### Key Performance Improvements

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

**Large Data Operations:**
- **Large slice copies**: Up to 3x faster for 16KB+ arrays using SIMD
- **Vectorized operations**: 256-bit AVX2 throughput optimization
- **Prefetching benefits**: 15-25% improvement in cache-bound workloads

**Mixed Workloads:**
- **Realistic allocation patterns**: 50-70% overall performance improvement
- **Memory efficiency**: Reduced fragmentation and better locality
- **Zero-overhead stats**: No performance impact when disabled

### Technical Features

- **Atomic CAS allocation**: Lock-free compare-and-swap for thread-safe fast paths
- **Size-class pooling**: 10 size classes (8B to 4KB) with automatic coalescing
- **Cache-line alignment**: All critical structures aligned to 64-byte boundaries
- **Branch optimization**: Optimized hot/cold path separation
- **Runtime feature detection**: Automatic SIMD feature detection and fallback

### Performance Comparison

| Operation | v0.2.0 | v0.3.0 | Improvement |
|-----------|--------|--------|-------------|
| Small object alloc | 52µs | 18µs | **2.9x** |
| SIMD copy (16KB) | 385ns | 105ns | **3.7x** |
| Mixed workload | 62µs | 28µs | **2.2x** |
| Scope reuse | 1.3µs | 0.74µs | **1.8x** |
| Memory pool | 287µs | 125µs | **2.3x** |

Benchmarks are in `benches/arena_vs_box.rs`, `benches/optimization_benchmarks.rs`, and `benches/advanced_benchmarks.rs` and use [Criterion](https://crates.io/crates/criterion). Run:

```bash
cargo bench --bench arena_vs_box
cargo bench --bench optimization_benchmarks
cargo bench --bench advanced_benchmarks
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
