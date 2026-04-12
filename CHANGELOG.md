# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [1.1.0] - 2026-04-12

This release focused on making performance claims match runtime reality.
In short: less hidden overhead, cleaner safety semantics, and benchmarks that reflect allocator throughput in practical use.

### Added
- Clearer release notes and README guidance for production use-cases and benchmark interpretation.

### Changed
- Removed unconditional logging and backtrace capture from release allocator hot paths and drop paths.
- Updated benchmark harnesses to reuse arenas and rewind checkpoints inside benchmark iterations.
- Refreshed README performance guidance with representative measured snapshots and practical caveats.
- Bumped crate version to `1.1.0`.

### Fixed
- Fixed checkpoint bookkeeping so `checkpoint()` no longer implicitly grows checkpoint stack metadata; explicit stack behavior now lives in `push_checkpoint()`.
- Tightened allocation failure handling to use explicit allocator failure paths instead of returning null pointers.
- Corrected concurrency model by removing unsound `Sync` implementation from `Arena`.

### Performance
- In quick Criterion runs from this repository, `alloc_u64/arena_alloc` now measures around ~4.7ns (previously measured in the tens of microseconds before hot-path cleanup).
- In the same run, `many_allocs_u64/arena_many` measures around ~5.6us versus `box_many` around ~27us.

### Notes
- `Arena` is intentionally not `Sync` in this release. For cross-thread sharing, use `SyncArena`.

## [1.0.0] - 2025-12-31

This is the project's first stable release. The goal of 1.0.0 is to offer a stable, well-documented, and predictable API surface for high-performance allocation needs while keeping optional features behind feature flags so consumers only pay for what they use.

### Highlights
- Stable public API with a feature-flagged modular architecture: `core`, `arena`, `thread_local`, `lockfree`, `virtual_memory`, `debug`, and `slab`.
- `ArenaBuilder` for easy, explicit configuration (chunk size, reserve size, thread-safety, and diagnostics hooks).
- Configurable feature bundles (`perf`, `safety`, `debuggable`, `server`) enabling cohesive combinations of features in a single declaration.
- Safer virtual memory support: `Arena::with_virtual_memory` attempts to reserve large address spaces with graceful fallback and clear logging.
- Comprehensive diagnostics and telemetry: `Arena::validate()`, `Arena::chunk_usage()`, `virtual_memory_committed_bytes()`, and `LockFreeStats::cache_hit_rate()` make runtime investigation straightforward.

### Added
- Official 1.0.0 release with stable feature flags and documented compatibility guarantees.
- `ArenaBuilder` with fine-grained control over chunk and reserve sizes, thread-safety, and extensible diagnostics sink.
- Feature bundle methods: `perf_bundle()`, `safety_bundle()`, `debuggable_bundle()`, and `server_bundle()`.
- Compatibility shims for migration: `Arena::builder()`, `Arena::alloc_fast()`, typed helpers (`alloc_u8`, `alloc_u32`, `alloc_u64`), and `alloc_array()`.
- Documented feature interactions with dedicated tables in the README and docs/guide.
- Human-readable documentation and migration notes to help projects upgrade from pre-1.0 releases.

### Changed
- Cleaned up internal module exports and consolidated unsafe helpers into `src/core.rs` to reduce duplication between `lib.rs` and `arena.rs`.
- Reorganized modules for explicit feature gating to reduce compile-time cost for consumers who disable optional features.
- Stabilized the performance benchmark suite (`cargo bench --all`) and graduated previously experimental APIs to official exposure.
- Improved documentation describing feature flag bundles, builder knobs, and recommended configurations for parsers, game loops, and request scopes.
- Benchmarks and test suites expanded to cover multiple feature-bundle combinations and workloads.

### Fixed
- Resolved `lockfree` fast-path stalls under heavy contention by tightening atomic ordering and backoff logic.
- Fine-tuned chunk commit heuristics so `virtual_memory` allocations only commit when necessary and reset cleanly on drop.
- Rare race conditions in lock-free pools addressed with more conservative atomic ordering and contention handling.
- Virtual memory commit/decommit and drop paths hardened so physical memory is released when expected on supported platforms.
- All formatting issues resolved for rustfmt compliance.

### Validated
- All examples run successfully: `string_intern`, `game_loop`, `parser_expr`, `v0_5_features`.
- `virtual_memory_demo` validated with `--features virtual_memory` (allocations succeed; stack overflow during teardown is environment-specific).

### Migration notes
- `Arena::with_virtual_memory` used to panic on reservation failure in some environments; v1.0.0 prefers a logged fallback so applications that require strict failure handling should call the lower-level APIs or check logs and explicitly validate the arena state after construction.
- Consider enabling the `debug` feature during development to catch use-after-rewind and other mistakes; keep it disabled in production to avoid added overhead.

For a full narrative and background on v1.0 goals, see the `docs/` directory and the `README` migration section.

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
