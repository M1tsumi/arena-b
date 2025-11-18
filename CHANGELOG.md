# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.1.0] - Beta Release

### Added

- Core bump `Arena` allocator with:
  - `alloc`, `alloc_default`.
  - `alloc_slice_copy`, `alloc_slice_uninit`, `alloc_str`.
  - Multi-chunk growth and `scope` support.
  - `reset`, `stats`, and `bytes_allocated`.
- `ArenaBuilder` for configuring initial capacity and future tuning knobs.
- `Pool<T>` allocator with `Pooled<T>` RAII wrapper and `PoolStats`.
- `SyncArena` as a thread-safe wrapper around `Arena` using `Mutex`.
- Feature flag `stats` to control per-allocation statistics overhead.
- Criterion benchmarks comparing `Arena`, `Pool`, `Box`, and `Vec` in several patterns.
- Property tests using `proptest` for arena invariants.
- Real-world inspired examples in `examples/`:
  - `parser_expr.rs` – expression parser building an AST in an arena.
  - `game_loop.rs` – per-frame allocations for a game loop.
  - `graph_pool.rs` – graph traversal using a pool allocator.
  - `string_intern.rs` – string interning backed by an arena.
- User documentation in `docs/`:
  - `guide.md` – getting started.
  - `strategies.md` – when to use arenas vs pools.
  - `advanced.md` – configuration, stats, and thread safety.
  - `architecture.md` – internal design and invariants.
- GitHub Actions CI workflow running fmt, clippy, tests, docs, and a short benchmark.
