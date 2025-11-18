# bumper

[![Crates.io](https://img.shields.io/crates/v/bumper.svg)](https://crates.io/crates/bumper)
[![Docs.rs](https://docs.rs/bumper/badge.svg)](https://docs.rs/bumper)
[![CI](https://github.com/pawso/Bumper/actions/workflows/ci.yml/badge.svg)](https://github.com/pawso/Bumper/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/language-Rust-orange.svg)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

`bumper` is a high-performance **bump allocator** / **arena allocator** and **memory pool** crate for Rust. It is designed for allocation-heavy workloads such as parsers, compilers, game engines, simulations, and data processing, while keeping a clean, idiomatic Rust API.

The core type is `bumper::Arena`, a bump allocator that lets you allocate many values cheaply and reclaim them all at once when the arena is reset or dropped.

## Installation

`bumper` is published on [crates.io](https://crates.io/crates/bumper).

Add it to your `Cargo.toml`:

```toml
[dependencies]
bumper = "0.1"
```

Or, using `cargo add`:

```bash
cargo add bumper
```

### Optional: disable `stats` for hot builds

By default the `stats` feature is enabled to collect allocation statistics.
You can disable it to remove even the small accounting overhead:

```toml
[dependencies]
bumper = { version = "0.1", default-features = false }
```

### Using a local checkout (for contributors)

If you are hacking on `bumper` itself, depend on it via a local path:

```toml
[dependencies]
bumper = { path = "../bumper" }
```

## Getting started

The simplest way to use `bumper` is with an arena:

```rust
use bumper::Arena;

fn main() {
    let arena = Arena::new();
    let value = arena.alloc(42_u32);
    assert_eq!(*value, 42);
}
```

### Scoped allocations

Use `Arena::scope` to allocate many temporary values and free them all at once:

```rust
use bumper::Arena;

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
use bumper::Pool;

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

Benchmarks are in `benches/arena_vs_box.rs` and use [Criterion](https://crates.io/crates/criterion). On one development machine, example results include:

- **4 KiB slice allocation** (`alloc_var_sizes/arena_u8_4096` vs `vec_u8_4096`)
  - Arena: ~54 ns
  - Vec:   ~61 ns
  - ~12% faster for this size.

- **Many allocations per iteration** (`many_allocs_u64` and reused arena/pool benchmarks)
  - Show tradeoffs between `Arena`, `Pool`, and `Box` for batch-style workloads.

Actual performance will depend on your CPU and workload. Use:

```bash
cargo bench --bench arena_vs_box
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

- Planned (for future releases):
  - Slab allocator with multiple size classes
  - More advanced debugging and visualization helpers
  - `no_std` support and async-friendly integrations

`bumper` aims to be a fast, ergonomic Rust arena allocator and memory pool library that feels native to Rust while offering production-grade safety and documentation.

## License

Licensed under either of:

- MIT license
- Apache License, Version 2.0

at your option.

See the `LICENSE` file for details.
