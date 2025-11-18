# arena-b: Getting Started

This guide shows how to add `arena-b` to your project, create your first arena, and understand when to use it versus `Box` and `Vec`.

## 1. Installing

Add this to your `Cargo.toml` while developing locally:

```toml
[dependencies]
arena-b = { path = "." }
```

Once published to crates.io, you will instead depend on a version:

```toml
[dependencies]
arena-b = "0.1"
```

## 2. Your first arena

```rust
use arena_b::Arena;

fn main() {
    let arena = Arena::new();

    let x = arena.alloc(42_u32);
    let y = arena.alloc(7_u32);

    assert_eq!((*x, *y), (42, 7));
}
```

All allocations made from `arena` are freed at once when the arena is dropped (or when you call `reset`). There is **no per-value deallocation**.

## 3. Scoped allocations

Use `scope` to allocate temporary data and reclaim it automatically at the end of the scope:

```rust
use arena_b::Arena;

fn compute(arena: &Arena) {
    arena.scope(|scope| {
        let tmp = scope.alloc(String::from("hello"));
        // use `tmp` here
    }); // allocations from this scope are now logically freed
}
```

This pattern is ideal for per-frame or per-request scratch data: everything allocated in the scope is invalidated at the end.

## 4. When to use an arena

`Arena` is ideal when:

- You allocate many objects of varying lifetimes, but you can group them by phase or scope.
- You want to reduce allocator overhead and fragmentation.
- You are building trees/graphs/ASTs, or per-frame game data, and can tear down whole phases at once.

`Arena` is **not** ideal when:

- You need to free individual items at unpredictable times.
- You have very few allocations; `Box` and `Vec` will often be simpler and just as fast.

## 5. Pools

For many fixed-size allocations with reuse, use `Pool<T>`:

```rust
use arena_b::Pool;

fn example_pool() {
    let pool: Pool<u32> = Pool::with_capacity(1024);

    let value = pool.alloc(5_u32);
    assert_eq!(*value, 5);
    // When `value` is dropped, its slot returns to the pool.
}
```

Pools are great for reusing storage for nodes in graphs, lists, or other uniform data structures.

## 6. Thread-safe usage

For multi-threaded programs, wrap the arena in `SyncArena`:

```rust
use std::sync::Arc;
use std::thread;
use arena_b::SyncArena;

fn main() {
    let arena = Arc::new(SyncArena::with_capacity(64 * 1024));

    let mut handles = Vec::new();
    for _ in 0..4 {
        let a = Arc::clone(&arena);
        handles.push(thread::spawn(move || {
            a.scope(|s| {
                let v = s.alloc(1_u32);
                assert_eq!(*v, 1);
            });
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}
```

`SyncArena` uses a `Mutex` internally; it is safe but has locking overhead. Prefer `Arena` in single-threaded hot paths.

## 7. Real-world examples (in everyday English)

The `examples/` folder contains small programs that show how you might use `arena-b` in real applications. Here is what each one does and why it can be faster than normal allocations.

### 7.1 Parser example (`parser_expr.rs`)

This example parses a simple math expression like `1 + 2 * (3 + 4)` and builds an **expression tree** in an `Arena`.

- In normal code, every node in the tree might be allocated with `Box` or `Vec`, one by one.
- With `Arena`, all the nodes for a single parse live in one arena and are freed together when you drop or reset that arena.

**Why this helps performance:** fewer calls into the system allocator and better cache locality when walking the tree.

### 7.2 Game loop / rendering example (`game_loop.rs`)

This simulates a game loop where each frame needs temporary data, such as positions of objects.

- Each frame calls `arena.scope(...)` and allocates a bunch of small values inside that scope.
- At the end of the frame, everything allocated in the scope is dropped at once.

In a real rendering or game engine, this pattern means:

- You avoid thousands of small `malloc`/`free` calls every frame.
- You reduce memory fragmentation, which can otherwise cause stutters.
- You get more **predictable frame times**, because the allocator work per frame is very simple and repeatable.

### 7.3 Graph example with pools (`graph_pool.rs`)

This example builds a tiny graph using `Pool<Node>` and runs a breadth-first search.

- Each node is stored in a pool slot.
- When a `Pooled<Node>` is dropped, its slot is returned to the pool and reused later.

**Why this helps performance:** in long-running systems (servers, simulations, games), nodes come and go. A pool lets you **reuse memory for nodes** instead of constantly allocating and freeing, which reduces allocator pressure and keeps memory usage more stable.

### 7.4 String interning example (`string_intern.rs`)

This example stores many strings, but only wants one copy of each unique value.

- It uses an `Arena` to store the actual string data.
- It uses a `HashSet<&str>` to remember which string contents are already interned.

When you intern strings:

- You only allocate new memory when a **new unique string** appears.
- Comparisons between interned strings can be faster (pointer comparison or cheap hash) instead of comparing long byte sequences.

This pattern is common in compilers, scripting languages, and asset systems where the same names repeat often.
