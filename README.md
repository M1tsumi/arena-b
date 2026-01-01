# arena-b

[![Crates.io](https://img.shields.io/crates/v/arena-b.svg)](https://crates.io/crates/arena-b)
[![Docs.rs](https://docs.rs/arena-b/badge.svg)](https://docs.rs/arena-b)
[![CI](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20windows%20%7C%20macos-lightgrey.svg)]()

**Ultra-fast bump allocation arena for high-performance Rust applications**

`arena-b` is a compact, battle-tested bump allocator for workloads that allocate many short-lived objects and prefer bulk reclamation. Allocate objects quickly into contiguous chunks and free them all at once using checkpoints, scopes, or a full reset — eliminating per-object deallocation overhead and fragmentation.

## Overview

Arena allocation follows a simple principle: allocate objects sequentially into a contiguous buffer, then free everything simultaneously when the arena is reset or dropped. This approach eliminates per-object deallocation overhead and avoids memory fragmentation entirely.

## Quick Start

```rust
use arena_b::Arena;

fn main() {
    let arena = Arena::new();
    
    // Allocate objects into the arena
    let numbers: Vec<&u32> = (0..1000)
        .map(|i| arena.alloc(i))
        .collect();
    
    // All allocations are freed when the arena is dropped
    // Alternatively, call arena.reset() to free manually
}
```

## Features

- **Fast bump allocation**: Extremely low-overhead allocations by bumping a pointer inside chunks.
- **Thread-local caches**: Per-thread hand-offs for the smallest allocations to reduce contention in multithreaded workloads.
- **Lock-free fast-paths**: Optional lock-free buffer for very small allocations to reduce synchronization overhead.
- **Checkpoint & scopes**: Save/restore allocation state (`checkpoint`/`rewind_to_checkpoint`) and `scope()` for panic-safe temporary allocations.
- **Virtual memory backing** *(optional)*: Reserve large address spaces and commit pages on demand to keep the committed footprint small.
- **Slab allocator** *(optional)*: Size-class based caching for frequent small object sizes.
- **Debug tooling** *(optional)*: Guard-based use-after-rewind detection, leak reports and richer diagnostics when the `debug` feature is enabled.
- **Fine-grained feature flags**: Only enable what you need — `virtual_memory`, `thread_local`, `lockfree`, `slab`, `debug`, `stats`.

## What's New in v1.0.0

- Stabilized public API and feature-gated modules for predictable builds and smaller compile cost when optional features are disabled.
- `ArenaBuilder` to configure arenas declaratively (chunk size, reserve size, thread safety, diagnostics sink).
- Graceful virtual memory handling: `Arena::with_virtual_memory` logs and falls back in restricted environments — check logs if you require strict failure behavior.
- Improved runtime diagnostics: `Arena::chunk_usage()`, `virtual_memory_committed_bytes()`, and `LockFreeStats::cache_hit_rate()`.

See `CHANGELOG.md` for the complete release notes and migration tips.

## What's New in v0.8

- **Generic Lock-Free Pool**: `LockFreePool<T>` provides thread-safe object pooling with atomic CAS operations for game engines, parsers, and high-frequency allocation patterns
- **Lock-Free Allocator Control**: `LockFreeAllocator` with runtime `enable()`/`disable()` switching and `cache_hit_rate()` monitoring
- **Thread-Local Slabs**: `ThreadSlab` with generation-based invalidation for per-thread fast-path allocations
- **Enhanced Statistics**: `LockFreeStats` now supports `cache_hit_rate()`, `record_deallocation()`, and is `Clone`able for snapshots
- **Debug Improvements**: `DebugStats` includes `leak_reports` counter for tracking leak detection calls

## What's New in v0.7

- **Proactive Reservation**: Call `Arena::reserve_additional(bytes)` to pre-grow the underlying chunk before a known burst of allocations, reducing contention in hot paths.
- **Memory Trimming**: `Arena::shrink_to_fit` and `Arena::reset_and_shrink` reclaim any extra chunks after a spike, keeping long-running services lean.
- **Docs & Tooling**: README and changelog now cover the adaptive APIs, and CI stays green with the explicit Clippy allowance on `alloc_str_uninit`.

## What's New in v0.6

- **New Allocation APIs**: `alloc_slice_fast` accelerates small slice copies and `alloc_str_uninit` creates mutable UTF-8 buffers without extra allocations.
- **Virtual Memory Telemetry**: `virtual_memory_committed_bytes()` reports the current committed footprint, while rewinds/resets now guarantee proper decommit on every platform.
- **Lock-Free Overhaul**: Per-thread slab caches reduce contention and eliminate previously observed race conditions in the lock-free buffer.
- **Panic-Safe Scopes**: `Arena::scope` now rolls back automatically even if the scoped closure unwinds, ensuring arenas remain consistent under failure.
- **Enhanced Debugging**: Runtime validation hooks, leak reports, and optional `debug_backtrace` capture provide deep diagnostics when the `debug` feature is enabled.

## What's New in v0.5

- **Checkpoints**: Mark allocation points and rewind instantly for bulk deallocation
- **Debug Mode**: Detect use-after-free bugs with guard patterns and pointer validation
- **Virtual Memory**: Reserve large address spaces without committing physical memory upfront
- **Thread-Local Caching**: Reduce lock contention in multi-threaded workloads
- **Lock-Free Operations**: Minimize synchronization overhead in high-contention scenarios

## Installation

Add `arena-b` to your `Cargo.toml`:

```toml
[dependencies]
arena-b = "1.0.0"
```

### Quick Start

```bash
# Add to your project
cargo add arena-b

# Run examples
cargo run --example parser_expr
cargo run --example game_loop
```

### Feature Flags

`arena-b` uses feature flags to minimize compilation overhead:

```toml
# Basic bump allocator
arena-b = "1.0.0"

# Development with safety checks
arena-b = { version = "1.0.0", features = ["debug"] }

# Maximum performance for production
arena-b = { version = "1.0.0", features = ["virtual_memory", "thread_local", "lockfree", "slab"] }
```

| Feature | Description | Performance Impact | When to Use |
|---------|-------------|-------------------|-------------|
| `debug` | Memory safety validation and use-after-free detection | ~5% overhead | Development & testing |
| `virtual_memory` | Efficient handling of large allocations via reserve/commit | Memory efficient | Large arena allocations |
| `thread_local` | Per-thread allocation buffers to reduce contention | 20-40% faster | Multi-threaded workloads |
| `lockfree` | Lock-free operations for concurrent workloads | 15-25% faster | High-contention scenarios |
| `stats` | Allocation statistics tracking | Minimal overhead | Performance monitoring |
| `slab` | Size-class cache for small allocations | 10-20% faster | Mixed allocation sizes |

## Usage Examples

### Frame-Based Allocation

Suitable for game loops or per-request processing:

```rust
use arena_b::Arena;

fn game_loop() {
    let arena = Arena::new();
    
    loop {
        let checkpoint = arena.checkpoint();
        
        // Allocate frame-specific data
        let entities = allocate_entities(&arena);
        let particles = allocate_particles(&arena);
        
        // Process the frame...
        
        // Deallocate all frame data at once
        unsafe { arena.rewind_to_checkpoint(checkpoint); }
    }
}
```

### AST Construction for Parsers

Build complex data structures without manual memory management:

```rust
use arena_b::Arena;

struct AstNode<'a> {
    value: String,
    children: Vec<&'a AstNode<'a>>,
}

fn parse_expression<'a>(input: &str, arena: &'a Arena) -> &'a AstNode<'a> {
    let node = arena.alloc(AstNode {
        value: input.to_string(),
        children: Vec::new(),
    });
    
    // Child nodes are allocated in the same arena
    // All memory is freed when the arena is dropped
    
    node
}
```

### Thread-Safe Allocation

Use `SyncArena` for concurrent access:

```rust
use std::sync::Arc;
use arena_b::SyncArena;

fn main() {
    let arena = Arc::new(SyncArena::new());
    
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let arena = Arc::clone(&arena);
            std::thread::spawn(move || {
                arena.scope(|scope| {
                    scope.alloc("thread-local data")
                })
            })
        })
        .collect();
    
    for handle in handles {
        handle.join().unwrap();
    }
}
```

## Performance Characteristics

| Operation | arena-b | std::alloc Box | std::alloc Vec | Improvement |
|-----------|---------|----------------|----------------|-------------|
| Small allocations (≤64B) | ~5ns | ~50ns | ~25ns | **10x faster** |
| Medium allocations (≤4KB) | ~20ns | ~200ns | ~100ns | **10x faster** |
| Large allocations (>4KB) | ~100ns | ~500ns | ~300ns | **5x faster** |
| Bulk reset | ~10ns | N/A | N/A | **Instant** |
| Memory overhead | ~64B/chunk | ~16B/object | ~24B/object | **Minimal** |

### Benchmarks

Run the comprehensive benchmark suite:
```bash
cargo bench --all
```

View detailed performance reports in `benches/` directory.

## When to Use `arena-b`

Recommended use cases:
- Parsers and compilers (ASTs and intermediate representations)
- Game engines and simulations (per-frame or transient data)
- Web servers and request-oriented services (per-request temporary data)

Not suitable for:
- Long-lived objects with mixed lifetimes where individual free is required
- Programs that need fine-grained control of each allocation lifetime

## API Reference

```rust
use arena_b::Arena;

let arena = Arena::new();

// Basic allocation
let number = arena.alloc(42u32);
let string = arena.alloc_str("hello world");
let small = arena.alloc_slice_fast(&[1u8, 2, 3]);
let buf = arena.alloc_str_uninit(256); // mutable UTF-8 buffer

// Scoped allocation with automatic cleanup (panic-safe)
arena.scope(|scope| {
    let temp = scope.alloc("temporary data");
    // Automatically rewound even if this closure panics
    assert_eq!(temp, &"temporary data");
});

// Checkpoint-based bulk deallocation
let checkpoint = arena.checkpoint();
// ... perform allocations ...
unsafe { arena.rewind_to_checkpoint(checkpoint); }

// Virtual memory telemetry (feature = "virtual_memory")
#[cfg(feature = "virtual_memory")]
if let Some(bytes) = arena.virtual_memory_committed_bytes() {
    println!("Currently committed: {} bytes", bytes);
}

// Statistics
println!("Allocated: {} bytes", arena.bytes_allocated());
println!("Stats: {:?}", arena.stats());
```

## Documentation

Additional documentation is available in the `docs/` directory:

- `docs/guide.md` — Comprehensive usage guide
- `docs/strategies.md` — Allocation strategy selection
- `docs/advanced.md` — Advanced configuration options
- `docs/architecture.md` — Internal design and implementation details

## Examples

Working examples are provided in the `examples/` directory:

- `parser_expr.rs` — Expression parser with arena-allocated AST
- `game_loop.rs` — Game loop with frame-based allocation
- `graph_pool.rs` — Graph traversal with object pooling
- `string_intern.rs` — String interning implementation
- `v0.5_features.rs` — Demonstration of v0.5 features

## Contributing

Contributions are welcome. Please consider the following:

- Bug reports and feature requests via GitHub Issues
- Performance improvements with benchmark data
- Documentation corrections and improvements
- For significant changes, please open an issue for discussion first

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for complete version history.

### v0.9.0

- Slab allocator size-class cache (`slab` feature)
- `Arena::chunk_usage()` telemetry for per-chunk capacity/used
- Debug tracking consistency across fast allocation paths
