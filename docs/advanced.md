# Advanced Usage

This guide covers advanced `arena-b` features: configuration, stats control, thread safety, and benchmarking.

## 1. Configuring arenas with `ArenaBuilder`

Use `ArenaBuilder` to tune capacities for your workload:

```rust
use arena_b::{Arena, ArenaBuilder};

fn make_arena() -> Arena {
    Arena::builder()
        .initial_capacity(256 * 1024)
        .chunk_size(128 * 1024)
        .thread_safe(false)
        .build()
}
```

### Fields

- `initial_capacity`: total bytes reserved for the first chunk.
- `chunk_size`: intended target size for subsequent chunks (reserved for future tuning in the current implementation).
- `thread_safe`: placeholder flag for future sync-aware builders. The current implementation always returns the non-thread-safe `Arena`.

## 2. Controlling stats overhead

By default, `arena-b` tracks per-allocation statistics:

- `bytes_used`
- `allocation_count`
- `chunk_count`

These are exposed via `Arena::stats() -> ArenaStats` and are useful for debugging and tuning.

### Disabling stats

You can compile without stats to minimize per-allocation overhead:

```bash
cargo bench --bench arena_vs_box --no-default-features
```

When the `stats` feature is disabled, `record_allocation` becomes a no-op; stats will always report zero. This is appropriate for hot production builds where every cycle matters.

## 3. Thread-safe arenas with `SyncArena`

`SyncArena` wraps an `Arena` in a `Mutex` so it can be safely shared between threads:

```rust
use arena_b::SyncArena;
use std::sync::Arc;
use std::thread;

fn main() {
    let arena = Arc::new(SyncArena::with_capacity(64 * 1024));

    let mut handles = Vec::new();
    for _ in 0..4 {
        let a = Arc::clone(&arena);
        handles.push(thread::spawn(move || {
            a.scope(|scope| {
                let v = scope.alloc(1_u32);
                assert_eq!(*v, 1);
            });
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}
```

`SyncArena` is ideal when you need a shared scratch arena for tasks running on a thread pool, but keep in mind that each operation must acquire a lock.

## 4. Benchmarking your workloads

Use the provided Criterion benchmark as a starting point:

```bash
cargo bench --bench arena_vs_box
```

This benchmark compares:

- Single allocations (`alloc_u64`).
- Slice allocations at different sizes (`alloc_var_sizes`).
- Many allocations per iteration (`many_allocs_u64`).
- Reused arenas (`reused_arena_many_u64`).
- Reused pools (`many_allocs_reused_pool`).

You can fork `benches/arena_vs_box.rs` and adapt it to your own data structures and allocation patterns.

## 5. Choosing configuration for performance

Guidelines:

- For arenas, set `initial_capacity` high enough to avoid frequent chunk growth.
- For pools, choose capacities that match typical peak usage.
- Consider disabling stats in performance-critical builds.
- Measure on your actual hardware; CPU cache and allocator behavior can vary.
