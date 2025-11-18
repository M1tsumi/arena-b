# Architecture

This document describes the internal architecture of `arena-b` and the invariants that keep the unsafe code sound.

## 1. High-level overview

`arena-b` provides three core building blocks:

- `Arena`: a bump allocator for grouping allocations and freeing them all at once.
- `Pool<T>`: a slot-based pool allocator for many values of the same type.
- `SyncArena`: a thread-safe wrapper around `Arena` using `Mutex`.

The public APIs are entirely safe; all `unsafe` code is confined to a small number of low-level functions.

## 2. Arena layout

### Chunks

`Arena` internally maintains a vector of `Chunk` structures:

```rust
struct Chunk {
    ptr: NonNull<u8>,
    capacity: usize,
    used: usize,
}
```

Each chunk is allocated using `alloc::alloc` with a fixed alignment `CHUNK_ALIGN` and a size equal to `capacity`.

### Bump pointer

Allocations use a simple bump-pointer strategy:

1. Compute an aligned offset (`align_up(used, layout.align())`).
2. Check that `aligned + layout.size() <= capacity`.
3. If it fits, bump `used` and return `ptr + aligned`.
4. If it does not fit, allocate a new chunk with a larger capacity and retry.

Multi-chunk support is implemented by appending new `Chunk` values to the internal vector and updating `current_chunk`.

### Scoped allocations

`Arena::scope` records the current chunk index and `used` offset before running the user closure. After the closure returns, it:

- Restores `bytes_used` and `allocation_count` (if stats are enabled).
- Resets `used` for the current chunk and any later chunks to their previous values.

This ensures that all allocations made during the scope are logically freed while preserving previous allocations.

## 3. Invariants and unsafe code

### Allocation invariants

- Every pointer returned by `allocate_raw` points into a valid `Chunk` allocation.
- `align_up` is used to satisfy `Layout::align()` requirements, and a debug assertion ensures `layout.align() <= CHUNK_ALIGN`.
- `used` is always <= `capacity` for each chunk.
- `bytes_used` and `allocation_count` are updated only via `record_allocation`.

### Deallocation invariants

- Each chunk is deallocated exactly once in `Drop` using the same `Layout` that was used to allocate it (same `capacity` and `CHUNK_ALIGN`).

### Concurrency invariants

- `Arena` is marked `Send` but not `Sync`. It must not be shared across threads without external synchronization.
- `SyncArena` wraps an `Arena` in a `Mutex`, providing synchronized access to allocations and scopes.

### Pool invariants

- `Pool<T>` stores values in a `Vec<Option<T>>` plus a free list of indices.
- `Pooled<T>` holds an index and a reference to the parent pool.
- On drop, `Pooled<T>` calls `Pool::put_back`, which:
  - Sets the slot to `None`.
  - Decrements `in_use`.
  - Pushes the index onto the free list.

All access to the slots goes through `UnsafeCell<PoolInner<T>>`, with bounds-checked indexing.

## 4. Testing and CI

The project includes:

- Unit tests for `Arena`, `Pool`, and `SyncArena`.
- Property tests for arena behavior using `proptest`.
- Criterion benchmarks in `benches/arena_vs_box.rs`.
- GitHub Actions workflow to run:
  - `cargo fmt`, `cargo clippy`.
  - `cargo test` (with and without default features).
  - `cargo doc`.
  - A short benchmark run.

## 5. Future work

Potential future extensions include:

- Slab allocator with multiple size classes.
- More advanced debugging and visualization helpers (e.g., exporting chunk usage as JSON).
- Optional `no_std` support.
- Async- and FFI-friendly APIs.
