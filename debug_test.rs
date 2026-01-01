use arena_b::Arena;

fn main() {
    let arena = Arena::with_capacity(16);
    println!("Initial stats: {:?}", arena.stats());

    let _first = arena.alloc([0_u8; 16]);
    let stats_after_first = arena.stats();
    println!("After first allocation: {:?}", stats_after_first);

    let _second = arena.alloc([0_u8; 16]);
    let stats_after_second = arena.stats();
    println!("After second allocation: {:?}", stats_after_second);

    println!("Chunk count >= 2: {}", stats_after_second.chunk_count >= 2);
}
