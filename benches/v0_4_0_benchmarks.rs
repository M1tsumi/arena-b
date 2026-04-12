use arena_b::Arena;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;

fn bench_fast_path_allocations(c: &mut Criterion) {
    let mut group = c.benchmark_group("fast_path_improvements");

    // Test alloc_fast vs alloc for small types
    for size in [8, 16, 32, 64, 128, 256, 512, 1024].iter() {
        group.bench_with_input(BenchmarkId::new("alloc_fast", size), size, |b, &size| {
            let arena = Arena::with_capacity(1024 * 1024);
            b.iter(|| {
                let checkpoint = arena.checkpoint();
                for _ in 0..1000 {
                    black_box(arena.alloc_fast(size as u64));
                }
                unsafe { arena.rewind_to_checkpoint(checkpoint) };
            });
        });

        group.bench_with_input(
            BenchmarkId::new("alloc_standard", size),
            size,
            |b, &size| {
                let arena = Arena::with_capacity(1024 * 1024);
                b.iter(|| {
                    let checkpoint = arena.checkpoint();
                    for _ in 0..1000 {
                        black_box(arena.alloc(size as u64));
                    }
                    unsafe { arena.rewind_to_checkpoint(checkpoint) };
                });
            },
        );
    }

    group.finish();
}

fn bench_array_allocations(c: &mut Criterion) {
    let mut group = c.benchmark_group("array_optimizations");

    for size in [4, 8, 16, 32, 64].iter() {
        let data: Vec<u32> = (0..*size).map(|i| i as u32).collect();
        let array: [u32; 64] = core::array::from_fn(|i| i as u32);

        group.bench_with_input(BenchmarkId::new("alloc_array", size), size, |b, &size| {
            let arena = Arena::with_capacity(1024 * 1024);
            let slice = &array[..size];
            b.iter(|| {
                let checkpoint = arena.checkpoint();
                // Convert slice to array for benchmark
                let mut arr = [0u32; 64];
                arr[..size].copy_from_slice(slice);
                black_box(arena.alloc_array(arr));
                unsafe { arena.rewind_to_checkpoint(checkpoint) };
            });
        });

        group.bench_with_input(
            BenchmarkId::new("alloc_slice_copy", size),
            size,
            |b, &size| {
                let arena = Arena::with_capacity(1024 * 1024);
                let slice = &data[..size];
                b.iter(|| {
                    let checkpoint = arena.checkpoint();
                    black_box(arena.alloc_slice_copy(slice));
                    unsafe { arena.rewind_to_checkpoint(checkpoint) };
                });
            },
        );
    }

    group.finish();
}

fn bench_batch_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_operations");

    for batch_size in [10, 50, 100, 500].iter() {
        let data: Vec<u32> = (0..*batch_size).map(|i| i as u32).collect();

        group.bench_with_input(
            BenchmarkId::new("alloc_batch", batch_size),
            batch_size,
            |b, &batch_size| {
                let arena = Arena::with_capacity(1024 * 1024);
                let slice = &data[..batch_size];
                b.iter(|| {
                    let checkpoint = arena.checkpoint();
                    black_box(arena.alloc_batch(slice));
                    unsafe { arena.rewind_to_checkpoint(checkpoint) };
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("individual_allocs", batch_size),
            batch_size,
            |b, &batch_size| {
                let arena = Arena::with_capacity(1024 * 1024);
                b.iter(|| {
                    let checkpoint = arena.checkpoint();
                    for &value in &data[..batch_size] {
                        black_box(arena.alloc(value));
                    }
                    unsafe { arena.rewind_to_checkpoint(checkpoint) };
                });
            },
        );
    }

    group.finish();
}

fn bench_mixed_workloads(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_workloads");

    group.bench_function("parser_simulation", |b| {
        let arena = Arena::with_capacity(64 * 1024);
        b.iter(|| {
            let checkpoint = arena.checkpoint();

            // Simulate parser workload: many small allocations
            for i in 0..1000 {
                let _id = arena.alloc_fast(i as u32);
                let _name = arena.alloc_fast(format!("token_{}", i));
                let _value = arena.alloc_fast(i as f64);
            }

            // Some medium allocations
            for i in 0..100 {
                let _data = arena.alloc_array([i as u32; 16]);
            }

            // Some large allocations
            for i in 0..10 {
                let _large = arena.alloc_slice_copy(&[i as u8; 1024]);
            }

            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.finish();
}

fn bench_memory_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_patterns");

    // Test sequential allocation pattern
    group.bench_function("sequential_alloc", |b| {
        let arena = Arena::with_capacity(1024 * 1024);
        b.iter(|| {
            let checkpoint = arena.checkpoint();

            for i in 0..10000 {
                black_box(arena.alloc_fast(i as u64));
            }

            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    // Test mixed size pattern
    group.bench_function("mixed_size_pattern", |b| {
        let arena = Arena::with_capacity(1024 * 1024);
        b.iter(|| {
            let checkpoint = arena.checkpoint();

            for i in 0..1000 {
                match i % 4 {
                    0 => {
                        black_box(arena.alloc_fast(i as u8));
                    }
                    1 => {
                        black_box(arena.alloc_fast(i as u32));
                    }
                    2 => {
                        black_box(arena.alloc_fast(i as u64));
                    }
                    3 => {
                        black_box(arena.alloc_array([i as u32; 8]));
                    }
                    _ => unreachable!(),
                }
            }

            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_fast_path_allocations,
    bench_array_allocations,
    bench_batch_operations,
    bench_mixed_workloads,
    bench_memory_patterns
);
criterion_main!(benches);
