use std::boxed::Box;
use std::hint::black_box;

use arena_b::{Arena, Pool};
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_arena_alloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_u64");
    group.bench_function("arena_alloc", |b| {
        let arena = Arena::with_capacity(1024);
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            let x = arena.alloc(black_box(42_u64));
            black_box(*x);
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        })
    });
    group.bench_function("box_new", |b| {
        b.iter(|| {
            let mut out: Vec<Box<u64>> = Vec::with_capacity(1);
            let x = Box::new(black_box(42_u64));
            out.push(x);
            black_box(out)
        });
    });
    group.finish();
}

fn bench_arena_alloc_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_var_sizes");
    for &len in &[8_usize, 256, 4096] {
        group.bench_with_input(format!("arena_u8_{}", len), &len, |b, &len| {
            let arena = Arena::with_capacity(len * 2);
            b.iter(|| {
                let checkpoint = arena.checkpoint();
                let slice = arena.alloc_slice_uninit::<u8>(len);
                black_box(slice);
                unsafe { arena.rewind_to_checkpoint(checkpoint) };
            })
        });
        group.bench_with_input(format!("vec_u8_{}", len), &len, |b, &len| {
            b.iter(|| {
                let mut out: Vec<Vec<u8>> = Vec::with_capacity(1);
                let v = vec![0_u8; len];
                out.push(v);
                // Ensure the allocation escapes the loop body.
                black_box(out);
            });
        });
    }
    group.finish();
}

fn bench_many_allocs(c: &mut Criterion) {
    let mut group = c.benchmark_group("many_allocs_u64");
    const N: usize = 1024;

    group.bench_function("arena_many", |b| {
        let arena = Arena::with_capacity(N * std::mem::size_of::<u64>() * 2);
        b.iter(|| {
            let checkpoint = arena.checkpoint();
            for i in 0..N {
                let x = arena.alloc(black_box(i as u64));
                black_box(*x);
            }
            unsafe { arena.rewind_to_checkpoint(checkpoint) };
        })
    });

    group.bench_function("box_many", |b| {
        b.iter(|| {
            let mut out: Vec<Box<u64>> = Vec::with_capacity(N);
            for i in 0..N {
                let x = Box::new(black_box(i as u64));
                out.push(x);
            }
            // Ensure allocations escape the loop body.
            black_box(out);
        });
    });

    group.bench_function("pool_many", |b| {
        b.iter(|| {
            let pool: Pool<u64> = Pool::with_capacity(N);
            for i in 0..N {
                let x = pool.alloc(black_box(i as u64));
                black_box(*x);
            }
        });
    });

    group.finish();
}

fn bench_reused_arena(c: &mut Criterion) {
    let mut group = c.benchmark_group("reused_arena_many_u64");
    const N: usize = 1024;

    group.bench_function("arena_reused_scope", |b| {
        let arena = Arena::builder()
            .initial_capacity(N * std::mem::size_of::<u64>() * 2)
            .build();

        b.iter(|| {
            arena.scope(|scope| {
                for i in 0..N {
                    let x = scope.alloc(black_box(i as u64));
                    black_box(*x);
                }
            });
        });
    });

    group.finish();
}

fn bench_reused_pool(c: &mut Criterion) {
    let mut group = c.benchmark_group("many_allocs_reused_pool");
    const N: usize = 1024;

    group.bench_function("pool_reused", |b| {
        let pool: Pool<u64> = Pool::with_capacity(N);
        b.iter(|| {
            for i in 0..N {
                let x = pool.alloc(black_box(i as u64));
                black_box(*x);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_arena_alloc,
    bench_arena_alloc_sizes,
    bench_many_allocs,
    bench_reused_arena,
    bench_reused_pool,
);
criterion_main!(benches);
