# arena-b

[![Crates.io](https://img.shields.io/crates/v/arena-b.svg)](https://crates.io/crates/arena-b)
[![Docs.rs](https://docs.rs/arena-b/badge.svg)](https://docs.rs/arena-b)
[![CI](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/M1tsumi/arena-b/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/language-Rust-orange.svg)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

A fast, practical bump allocator for Rust. Perfect for parsers, game engines, and anywhere you need to allocate lots of temporary objects that can be cleaned up all at once.

The main idea is simple: allocate into a growing buffer, then reset the whole thing when you're done. No individual frees, no fragmentation worries, just raw speed.

## Quick Start

```rust
use arena_b::Arena;

fn main() {
    let arena = Arena::new();
    
    // Allocate as much as you want
    let numbers: Vec<&u32> = (0..1000)
        .map(|i| arena.alloc(i))
        .collect();
    
    // Everything gets freed at once when arena drops
    // Or call arena.reset() to do it manually
}
```

## Why arena-b?

I built arena-b because I was tired of choosing between safety and speed in allocation-heavy code. Traditional arenas are fast but basic, and memory pools are flexible but complex. arena-b hits the sweet spot:

- **Fast as hell**: Bump allocation with optimizations like thread-local caching and lock-free operations
- **Safe by default**: All the unsafe bits are carefully contained and tested
- **Actually useful**: Real-world features like checkpoints for frame-based allocation
- **Zero overhead**: No runtime penalties when you don't use the fancy features

## What's New in v0.5

This isn't just a bump allocator anymore. v0.5 adds some serious firepower:

- **Checkpoints**: Mark a point in time, allocate like crazy, then rewind back to that point instantly. Perfect for game loops or per-request data.
- **Debug mode**: Catch use-after-free bugs with guard patterns and validation
- **Virtual memory**: Handle massive allocations without actually committing the RAM until you need it
- **Thread-local caching**: Reduce contention in multi-threaded scenarios
- **Lock-free operations**: For when you need every last drop of performance

## Installation

Add it to your `Cargo.toml`:

```toml
[dependencies]
arena-b = "0.5"
```

Or use cargo add:

```bash
cargo add arena-b
```

### Features

Start with the basics, then enable what you need:

```toml
# Just the fast bump allocator
arena-b = "0.5"

# Add debug safety checks
arena-b = { version = "0.5", features = ["debug"] }

# Go all-out for maximum performance
arena-b = { version = "0.5", features = ["debug", "virtual_memory", "thread_local", "lockfree"] }
```

- `debug`: Memory safety checks (use in development!)
- `virtual_memory`: For handling huge allocations efficiently
- `thread_local`: Reduces contention in multi-threaded code
- `lockfree`: Lock-free operations for the speed demons
- `stats`: Allocation statistics (enabled by default)

## Usage Patterns

### Frame-Based Allocation

Perfect for game loops or per-request processing:

```rust
use arena_b::Arena;

fn game_loop() {
    let arena = Arena::new();
    
    loop {
        let checkpoint = arena.checkpoint();
        
        // Allocate everything for this frame
        let entities = allocate_entities(&arena);
        let particles = allocate_particles(&arena);
        
        // Process frame...
        
        // Clean up everything at once
        unsafe { arena.rewind_to_checkpoint(checkpoint); }
    }
}
```

### Parser AST Construction

Build complex data structures without worrying about cleanup:

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
    
    // Parse children, allocate them in the same arena
    // No need to think about dropping anything!
    
    node
}
```

### Thread-Safe Usage

Wrap it in a SyncArena for multi-threaded scenarios:

```rust
use std::sync::Arc;
use arena_b::SyncArena;

fn main() {
    let arena = Arc::new(SyncArena::new());
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let arena = Arc::clone(&arena);
            std::thread::spawn(move || {
                // Each thread can allocate safely
                let data = arena.scope(|scope| {
                    scope.alloc("thread data")
                });
                data
            })
        })
        .collect();
    
    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }
}
```

## Performance

Numbers don't lie (on my machine, YMMV):

- **vs Box**: 10-50x faster for allocation-heavy workloads
- **vs Vec**: 5-20x faster when you can reset in bulk
- **Memory overhead**: ~64 bytes per chunk, basically zero per allocation
- **Thread contention**: Minimal with thread-local features enabled

Run the benchmarks yourself:

```bash
cargo bench
```

## When to Use arena-b

**Perfect for:**
- Parsers and compilers (ASTs, symbol tables)
- Game engines (per-frame allocations)
- Web servers (per-request data)
- Any code with lots of short-lived objects

**Maybe not for:**
- Long-lived objects with varying lifespans
- Applications with very few allocations
- When you need precise memory control

## API Overview

```rust
use arena_b::Arena;

let arena = Arena::new();

// Basic allocation
let number = arena.alloc(42u32);
let string = arena.alloc_str("hello world");
let slice = arena.alloc_slice_copy(&[1, 2, 3, 4]);

// Scoped allocations
arena.scope(|scope| {
    let temp = scope.alloc("temporary");
    // All this gets cleaned up automatically
});

// Checkpoints for bulk cleanup
let checkpoint = arena.checkpoint();
// ... allocate lots of stuff ...
unsafe { arena.rewind_to_checkpoint(checkpoint); }

// Stats and info
println!("Allocated {} bytes", arena.bytes_allocated());
println!("Stats: {:?}", arena.stats());
```

## Documentation

Check out the `docs/` directory for deeper dives:

- `docs/guide.md` - Detailed usage guide
- `docs/strategies.md` - When to use what
- `docs/advanced.md` - Advanced configuration
- `docs/architecture.md` - How it works under the hood

## Examples

Real-world examples in `examples/`:

- `parser_expr.rs` - Expression parser with AST
- `game_loop.rs` - Game loop with frame allocation
- `graph_pool.rs` - Graph traversal with object pooling
- `string_intern.rs` - String interning
- `v0.5_features.rs` - All the new v0.5 features in action

## Contributing

I'm pretty happy with where this is, but there's always room for improvement:

- Bug reports and feature requests are welcome
- Performance improvements are especially appreciated
- Documentation fixes are always needed
- If you're adding major features, let's talk first

## License

MIT or Apache-2.0, your choice. See the LICENSE files for details.

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for the full history. Recent highlights:

### v0.5.0
- Added checkpoint/rewind API
- Debug mode with memory safety checks
- Virtual memory support for huge allocations
- Thread-local caching and lock-free operations
- Major performance improvements across the board

### v0.4.x
- Initial stable release
- Basic arena and pool allocators
- SyncArena for thread safety

---

Built with ❤️ for the Rust community. If arena-b makes your code faster, I'd love to hear about it!
