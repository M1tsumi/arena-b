use arena_b::Arena;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;

fn bench_lockfree_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("lockfree_performance");

    for size in [8, 16, 32, 64, 128, 256, 512, 1024].iter() {
        group.bench_with_input(BenchmarkId::new("arena_alloc", size), size, |b, &size| {
            let arena = Arena::with_capacity(1024 * 1024);
            b.iter(|| {
                let checkpoint = arena.checkpoint();
                for i in 0..10000 {
                    let data = vec![i as u8; size];
                    let slice = arena.alloc_slice_copy(black_box(&data));
                    black_box(slice);
                }
                unsafe { arena.rewind_to_checkpoint(checkpoint) };
            });
        });

        group.bench_with_input(BenchmarkId::new("vec_alloc", size), size, |b, &size| {
            b.iter(|| {
                for i in 0..10000 {
                    let data = vec![i as u8; size];
                    black_box(data);
                }
            });
        });
    }

    group.finish();
}

fn bench_memory_pool_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_pool_performance");

    // Test small object allocation patterns
    group.bench_function("pool_u8_pattern", |b| {
        let arena = Arena::new();
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..50000 {
                let x = arena.alloc_u8(black_box(i as u8));
                black_box(*x);
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.bench_function("pool_u32_pattern", |b| {
        let arena = Arena::new();
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..50000 {
                let x = arena.alloc_u32(black_box(i as u32));
                black_box(*x);
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.bench_function("pool_u64_pattern", |b| {
        let arena = Arena::new();
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..50000 {
                let x = arena.alloc_u64(black_box(i as u64));
                black_box(*x);
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.bench_function("generic_alloc_pattern", |b| {
        let arena = Arena::new();
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..50000 {
                let x = arena.alloc(black_box(i as u64));
                black_box(*x);
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.finish();
}

fn bench_simd_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_performance");

    for size in [1024, 4096, 16384, 65536].iter() {
        let data: Vec<u8> = (0..*size).map(|i| (i % 256) as u8).collect();

        group.bench_with_input(
            BenchmarkId::new("arena_simd_copy", size),
            size,
            |b, &size| {
                let arena = Arena::with_capacity(size * 2);
                b.iter(|| {
                    let checkpoint = arena.checkpoint();
                    let slice = arena.alloc_slice_copy(black_box(&data));
                    black_box(slice);
                    unsafe { arena.rewind_to_checkpoint(checkpoint) };
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("vec_copy", size), size, |b, &_size| {
            b.iter(|| {
                let copied = data.clone();
                black_box(copied);
            });
        });
    }

    group.finish();
}

fn bench_concurrent_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_allocation");

    group.bench_function("arena_concurrent_single", |b| {
        let arena = Arena::with_capacity(1024 * 1024);
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for thread in 0..4 {
                for i in 0..2500 {
                    let x = arena.alloc((thread * 2500 + i) as u64);
                    black_box(*x);
                }
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.bench_function("arena_concurrent_scope", |b| {
        let arena = Arena::with_capacity(1024 * 1024);
        b.iter(|| {
            for thread in 0..4 {
                arena.scope(|scope| {
                    for i in 0..2500 {
                        let x = scope.alloc((thread * 2500 + i) as u64);
                        black_box(*x);
                    }
                });
            }
        });
    });

    group.finish();
}

fn bench_mixed_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_workload");

    group.bench_function("realistic_mixed", |b| {
        let arena = Arena::with_capacity(1024 * 1024);
        b.iter(|| {
            let checkpoint = arena.checkpoint();

            // Mix of different allocation patterns
            for i in 0..1000 {
                // Small allocations
                let small = arena.alloc_u8(black_box(i as u8));
                black_box(*small);

                // Medium allocations
                let medium = arena.alloc_u32(black_box(i as u32));
                black_box(*medium);

                // Large allocations (every 10 iterations)
                if i % 10 == 0 {
                    let large_data = vec![i as u64; 100];
                    let large_slice = arena.alloc_slice_copy(black_box(&large_data));
                    black_box(large_slice);
                }

                // String allocations (every 5 iterations)
                if i % 5 == 0 {
                    let s = format!("string_{}", i);
                    let arena_str = arena.alloc_str(black_box(&s));
                    black_box(arena_str);
                }
            }

            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.finish();
}

fn bench_vs_standard_allocators(c: &mut Criterion) {
    let mut group = c.benchmark_group("vs_standard_allocators");

    group.bench_function("arena_vs_box", |b| {
        let arena = Arena::new();
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..10000 {
                let arena_val = arena.alloc(black_box(i));
                black_box(*arena_val);
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.bench_function("box_allocations", |b| {
        b.iter(|| {
            for i in 0..10000 {
                let boxed = Box::new(black_box(i));
                black_box(*boxed);
            }
        });
    });

    group.bench_function("vec_vs_arena_slice", |b| {
        let arena = Arena::new();
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            let data: Vec<u32> = (0..1000).collect();
            let arena_slice = arena.alloc_slice_copy(black_box(&data));
            black_box(arena_slice);
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.bench_function("vec_clone", |b| {
        b.iter(|| {
            let data: Vec<u32> = (0..1000).collect();
            let cloned = data.clone();
            black_box(cloned);
        });
    });

    group.finish();
}

criterion_group!(
    advanced_benches,
    bench_lockfree_performance,
    bench_memory_pool_performance,
    bench_simd_performance,
    bench_concurrent_allocation,
    bench_mixed_workload,
    bench_vs_standard_allocators,
);
criterion_main!(advanced_benches);
