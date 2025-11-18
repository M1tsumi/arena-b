use arena_b::Arena;
use std::mem::MaybeUninit;
use std::time::Instant;

type Vec3 = [f32; 3];

fn simulate_frame(arena: &Arena, frame: u32) {
    arena.scope(|scope| {
        let positions: &mut [MaybeUninit<Vec3>] = scope.alloc_slice_uninit(256);
        for (i, slot) in positions.iter_mut().enumerate() {
            let x = frame as f32 * 0.01;
            let y = i as f32 * 0.1;
            slot.write([x, y, 0.0]);
        }
        // Do work with initialized data (omitted for brevity).
    });
}

fn main() {
    let arena = Arena::with_capacity(64 * 1024);
    let start = Instant::now();

    for frame in 0..1_000 {
        simulate_frame(&arena, frame);
    }

    println!(
        "Simulated 1000 frames using a reused arena in {:?}",
        start.elapsed()
    );
}
