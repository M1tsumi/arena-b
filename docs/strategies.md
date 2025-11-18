# arena-b Strategies

This guide explains the allocation strategies implemented by `arena-b` and when to use each.

## 1. Bump Arena (`Arena`)

### Characteristics

- O(1) allocation: simple pointer bump.
- No per-object deallocation; free everything at once.
- Excellent cache locality.
- Supports multi-chunk growth when capacity is exceeded.
- Scoped allocation via `Arena::scope`.

### When it shines

- Building trees/ASTs for parsers and compilers.
- Per-frame allocations in game loops.
- Short-lived scratch buffers in numeric or simulation code.

### Example pattern

```rust
use arena_b::Arena;

fn build_scene(arena: &Arena) {
    arena.scope(|scope| {
        // allocate many nodes
        for i in 0..10_000 {
            let node = scope.alloc(i);
            // link `node` into your scene graph
            let _ = node;
        }
    }); // all nodes freed here
}
```

## 2. Pool Allocator (`Pool<T>`)

### Characteristics

- Fixed-size slots for values of type `T`.
- `Pooled<T>` RAII wrapper returns the slot to the pool on drop.
- Can grow if capacity is exhausted (current implementation), but reuses freed slots.

### When it shines

- Graphs and trees where nodes are frequently created and destroyed.
- Object pools for gameplay entities.
- Long-lived data with frequent reuse of slots.

### Example pattern

```rust
use arena_b::{Pool, PoolStats};

fn work_with_pool() {
    let pool: Pool<u32> = Pool::with_capacity(128);

    {
        let a = pool.alloc(1);
        let b = pool.alloc(2);
        assert_eq!((*a, *b), (1, 2));
    } // slots returned to pool

    let stats: PoolStats = pool.stats();
    assert_eq!(stats.in_use, 0);
}
```

## 3. Choosing between Arena and Pool

- Use **`Arena`** when:
  - You can free groups of allocations together.
  - You rarely need to drop individual items early.

- Use **`Pool<T>`** when:
  - You need to recycle slots as objects come and go.
  - All objects have the same type and a reasonably uniform size.

## 4. Performance considerations

- For `Arena`, tune `initial_capacity` with `Arena::builder()` to avoid frequent chunk growth.
- For `Pool<T>`, choose a capacity that covers typical in-flight objects.
- Use `cargo bench --bench arena_vs_box` to compare `Arena`, `Pool`, and `Box` for your patterns.

When in doubt, start with `Arena` for batch-like workloads and `Pool<T>` for highly dynamic ones.
