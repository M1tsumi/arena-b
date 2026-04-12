use arena_b::{Arena, Pool};
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

fn bench_fast_path_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("fast_path_allocation");

    group.bench_function("arena_fast_path", |b| {
        let arena = Arena::with_capacity(1024 * 1024); // Large arena to avoid chunk allocation
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..1000 {
                let x = arena.alloc(black_box(i as u64));
                black_box(*x);
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.bench_function("arena_fast_path_small", |b| {
        let arena = Arena::with_capacity(1024 * 1024);
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..1000 {
                let x = arena.alloc(black_box(i as u8));
                black_box(*x);
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.finish();
}

fn bench_large_slice_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_slice_allocation");

    let large_data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();

    group.bench_function("arena_large_slice", |b| {
        let arena = Arena::with_capacity(large_data.len() * 2);
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            let slice = arena.alloc_slice_copy(black_box(&large_data));
            black_box(slice);
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.bench_function("vec_large_slice", |b| {
        b.iter(|| {
            let v = large_data.clone();
            black_box(v);
        });
    });

    group.finish();
}

fn bench_mixed_size_allocations(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_size_allocations");

    group.bench_function("arena_mixed_sizes", |b| {
        let arena = Arena::with_capacity(1024 * 1024);
        b.iter(|| {
            let checkpoint = arena.checkpoint();

            // Mix of different allocation sizes
            for i in 0..100 {
                let small = arena.alloc(black_box(i as u8));
                black_box(*small);
            }

            for i in 0..50 {
                let medium = arena.alloc(black_box(i as u64));
                black_box(*medium);
            }

            for i in 0..20 {
                let large = arena.alloc(black_box([i as u8; 128]));
                black_box(*large);
            }

            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.finish();
}

fn bench_scope_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("scope_performance");

    group.bench_function("arena_scope_reuse", |b| {
        let arena = Arena::with_capacity(1024 * 1024);

        b.iter(|| {
            arena.scope(|scope| {
                for i in 0..1000 {
                    let x = scope.alloc(black_box(i as u64));
                    black_box(*x);
                }
            });
        });
    });

    group.bench_function("arena_new_each_iteration", |b| {
        b.iter(|| {
            let arena = Arena::with_capacity(1024 * 1024);
            for i in 0..1000 {
                let x = arena.alloc(black_box(i as u64));
                black_box(*x);
            }
        });
    });

    group.finish();
}

fn bench_chunk_growth_strategies(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunk_growth");

    group.bench_function("arena_chunk_growth", |b| {
        b.iter(|| {
            let arena = Arena::with_capacity(1024); // Start small to force growth

            // Allocate enough to trigger multiple chunk growths
            for i in 0..10000 {
                let x = arena.alloc(black_box([i as u8; 256])); // 256 bytes each
                if i % 1000 == 0 {
                    black_box(x);
                }
            }
        });
    });

    group.finish();
}

fn bench_pool_optimizations(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_optimizations");

    group.bench_function("pool_alloc_reuse", |b| {
        let pool: Pool<u64> = Pool::with_capacity(1000);

        b.iter(|| {
            let mut handles = Vec::new();
            for i in 0..1000 {
                let x = pool.alloc(black_box(i as u64));
                handles.push(x);
            }
            // All handles drop here, returning to pool
        });
    });

    group.bench_function("pool_version", |b| {
        let pool: Pool<u64> = Pool::with_capacity(1000);

        b.iter(|| {
            for i in 0..1000 {
                let x = pool.alloc(black_box(i as u64));
                black_box(*x);
            }
        });
    });

    group.bench_function("arena_version", |b| {
        let arena = Arena::with_capacity(1000 * 8);

        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..1000 {
                let x = arena.alloc(black_box(i as u64));
                black_box(*x);
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.finish();
}

fn bench_stats_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats_overhead");

    group.bench_function("arena_with_stats", |b| {
        let arena = Arena::with_capacity(1024 * 1024);
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..10000 {
                let x = arena.alloc(black_box(i as u64));
                black_box(*x);
            }
            let stats = arena.stats();
            black_box(stats);
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_fast_path_allocation,
    bench_large_slice_allocation,
    bench_mixed_size_allocations,
    bench_scope_performance,
    bench_chunk_growth_strategies,
    bench_pool_optimizations,
    bench_stats_overhead,
);
criterion_main!(benches);
