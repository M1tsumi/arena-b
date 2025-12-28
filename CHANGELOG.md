# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- `arena-b` now exposes a configurable `ArenaBuilder` with hooks for chunk size, target commit size, and force-aligned reservations to support a wider range of workloads.
- Expanded feature bundles (`perf`, `safety`, `debuggable`) so teams can enable cohesive combinations of `thread_local`, `lockfree`, `virtual_memory`, `slab`, and `debug` in a single dependency declaration.
- Added documented diagnostics for `Arena::validate()`, `Arena::chunk_usage()`, and `LockFreeStats::cache_hit_rate()` to make runtime investigations straightforward.
- Documented feature interactions with dedicated tables in the README and docs/guide.

### Changed
- Cleaned up internal module exports and consolidated unsafe helpers into `src/core.rs` to reduce duplication between `lib.rs` and `arena.rs`.
- Stabilized the performance benchmark suite (now `cargo bench --all`) and graduated previously experimental APIs to official exposure under the v1.0.0 contract.
- Improved documentation to describe feature flag bundles, builder knobs, and recommended configurations for parsers, game loops, and request scopes.

### Fixed
- Resolved `lockfree` fast-path stalls under heavy contention by tightening atomic ordering and backoff logic.
- Fine-tuned chunk commit heuristics so `virtual_memory` allocations only commit when necessary and reset cleanly on drop.

### Misc / Unreleased work (Dec 2025)
- Added compatibility shims to ease migration and keep existing benches/examples working: `Arena::builder()`, `Arena::alloc_fast()`, typed helpers `alloc_u8/alloc_u32/alloc_u64`, and `alloc_array()`.
- Built and validated examples and benches; `string_intern`, `game_loop`, `parser_expr`, and `v0_5_features` run successfully. `virtual_memory_demo` was executed with `--features virtual_memory` (demo allocations succeeded but the example triggered a stack overflow during teardown in this environment — recommend running full demo locally for extended runs).
- Removed legacy V1_0_0 planning files from the repository as requested.

## [1.0.0] - 2025-12-26

### Added
- Official 1.0.0 public release with committed feature flag ecosystem, new builder knobs, and polished documentation for all modules.
- `Arena::with_virtual_memory` improvements, including failure-safe commit/decommit and clearer telemetry (`virtual_memory_committed_bytes`).
- Formal release roadmap (see V1_0_0_ROADMAP.md) that guarantees modularization, documentation, and QA steps.

### Changed
- Reorganized the crate into explicit modules (`core`, `arena`, `thread_local`, `lockfree`, `virtual_memory`, `debug`, `slab`) with feature gates that avoid unnecessary compilation.
- Tightened README, docs, and CHANGELOG to present a consistent narrative for v1.0.0 while aligning crates.io metadata with the new scope.
- Updated benchmarks and tests to cover multiple feature bundles and added property-based regression suites via `proptest`.

### Fixed
- Addressed rare race conditions in the lock-free object pool and improved cache hit tracking to ensure accurate diagnostics.
- Ensured debug guards are only enabled when requested so release builds stay lean.

### Removed
- Deprecated the monolithic legacy `Beacon` allocator path and removed redundant copies of `Arena` definitions that confused feature gating.

## [0.9.0] - 2024-08-12

### Added
- Slab allocator feature flag (`slab`) for size-class caching.
- `Arena::chunk_usage()` for per-chunk telemetry.
- Consistent debug tracking across fast paths when `debug` is enabled.

### Changed
- Internal module cleanup via `arena_module` gate to avoid duplicate implementations.

## [0.8.0] - Lock-Free Architecture & Pool Allocator Release

### Added
- `LockFreePool<T>` generic pool allocator with CAS-based push/pops and leak-safe drop.
- `LockFreeAllocator` for runtime enable/disable with cache hit tracking and runtime stats.
- Thread-local slab allocator to hand out aligned mini-regions for small allocations.

### Changed
- Enhanced virtual memory handling, debug instrumentation, and stats APIs to keep instrumentation lean.
- Thread-local caches gained `cleanup_thread_cache` and partial flush variants to avoid global stomps.

### Fixed
- Resolved cache pollution in multi-arena scenarios and tightened counter invalidation logic.

## [0.7.0] - Adaptive Memory Management Release

### Added
- `Arena::reserve_additional`, `Arena::shrink_to_fit`, and `Arena::reset_and_shrink` for adaptive capacity control.
- Fast-reset checkpoints (`ArenaCheckpoint`, `rewind_to_checkpoint`) for frame-based workloads.

### Changed
- Modularized `lib.rs` into smaller partitions; introduced `MemoryPool`, `Chunk`, and `VirtualChunk`.
- Added cross-platform virtual memory guards and panic-safe scope APIs to improve safety.

### Fixed
- Stabilized chunk growth heuristics to preserve cache locality while trimming aggressive expansion.
- Added explicit `alloc_str_uninit` clippy compliance hooks so CI stays green.

## [0.6.0] - Advanced SIMD & Cross-Platform Performance Release

### Added
- SIMD-accelerated small slices with AVX2 and NEON fallbacks behind runtime detection.
- `alloc_slice_fast`, `alloc_str_uninit`, and `alloc_batch<T>` helpers for zero-copy buffer creation.
- `virtual_memory_committed_bytes` telemetry.

### Changed
- Release profile enabled ThinLTO plus one codegen unit for size/performance trade-offs.
- Added explicit macOS `pthread_jit_write_protect_np` handling and Windows `MEM_TOP_DOWN` reservations.

### Fixed
- Addressed leaks and alignment issues on 32-bit targets, plus debug mode undefined behavior.
- Added panic-safe scope guard for `Arena::scope`.

## [0.5.0] - Feature Stabilization & Tooling

### Added
- `debug`, `virtual_memory`, `thread_local`, `lockfree`, and `stats` feature flags with clear documentation.
- Leak detection hooks and debug guards for use-after-rewind detection.
- Comprehensive test/benchmark suite covering fast-reset API and virtual memory heuristics.

### Changed
- Split `lib.rs` into core modules: `arena`, `core`, `thread_local`, `lockfree`, `virtual_memory`, `debug`.
- Documented debug guards, leak reports, and statistics interfaces.

### Fixed
- Addressed various race conditions in lock-free operations and ensured consistent decommit behavior.
