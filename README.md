# arena-b

[![Crates.io](https://img.shields.io/crates/v/arena-b.svg)](https://crates.io/crates/arena-b)
[![Docs.rs](https://docs.rs/arena-b/badge.svg)](https://docs.rs/arena-b)
[![CI](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/language-Rust-orange.svg)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

A high-performance bump allocator for Rust, designed for scenarios where you need to allocate many temporary objects and deallocate them all at once. Ideal for parsers, game engines, compilers, and request-scoped web server data.

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

- **High Performance**: Bump allocation with thread-local caching and lock-free fast paths
- **Memory Safety**: Unsafe internals are carefully encapsulated and thoroughly tested
- **Checkpoint API**: Save and restore allocation state for frame-based or request-scoped patterns
- **Zero-Cost Abstractions**: Optional features incur no overhead when disabled
- **Advanced Debugging** *(v0.6)*: Leak detection, validation hooks, and optional backtraces make it easier to audit arena usage during development
- **Virtual Memory Instrumentation** *(v0.6)*: Inspect committed bytes at runtime and rely on improved macOS/Windows handling for large reserves

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

Add to your `Cargo.toml`:

```toml
[dependencies]
arena-b = "0.5"
```

Or via the command line:

```bash
cargo add arena-b
```

### Feature Flags

Enable features based on your requirements:

```toml
# Basic bump allocator
arena-b = "0.5"

# With debug safety checks (recommended for development)
arena-b = { version = "0.5", features = ["debug"] }

# Full feature set for maximum performance
arena-b = { version = "0.5", features = ["debug", "virtual_memory", "thread_local", "lockfree"] }
```

| Feature | Description |
|---------|-------------|
| `debug` | Memory safety validation and use-after-free detection |
| `virtual_memory` | Efficient handling of large allocations via reserve/commit |
| `thread_local` | Per-thread allocation buffers to reduce contention |
| `lockfree` | Lock-free operations for concurrent workloads |
| `stats` | Allocation statistics tracking (enabled by default) |

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

Typical performance improvements over standard allocation:

- **vs `Box`**: 10–50× faster for allocation-intensive workloads
- **vs `Vec`**: 5–20× faster when bulk reset is applicable
- **Memory overhead**: ~64 bytes per chunk; negligible per-allocation overhead
- **Thread contention**: Minimal when thread-local features are enabled

Run benchmarks locally:

```bash
cargo bench
```

## When to Use arena-b

**Recommended Use Cases:**
- Parsers and compilers (ASTs, intermediate representations)
- Game engines (per-frame temporary allocations)
- Web servers (per-request data structures)
- Any workload with many short-lived objects

**Less Suitable For:**
- Long-lived objects with heterogeneous lifetimes
- Applications with minimal allocation requirements
- Scenarios requiring fine-grained memory control

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

### v0.5.0
- Checkpoint and rewind API for bulk deallocation
- Debug mode with memory safety validation
- Virtual memory support for large allocations
- Thread-local caching and lock-free operations
- Significant performance improvements

### v0.4.x
- Initial stable release
- Core arena and pool allocator implementations
- `SyncArena` for thread-safe allocation
