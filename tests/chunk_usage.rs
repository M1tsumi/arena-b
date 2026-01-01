use arena_b::Arena;

#[test]
#[cfg(feature = "arena_module")]
fn chunk_usage_reports_used_and_capacity() {
    let arena = Arena::with_capacity(4096);

    let before = arena.chunk_usage();
    assert_eq!(before.len(), 1);
    assert!(before[0].capacity >= 4096);
    assert_eq!(before[0].used, 0);

    // Use a size that bypasses thread-local and lock-free small-allocation caches,
    // so chunk usage reliably reflects arena chunk consumption across feature combos.
    let _a = arena.alloc([0u8; 2048]);
    let mid = arena.chunk_usage();
    assert_eq!(mid.len(), 1);
    assert!(mid[0].used >= 2048);

    // Force growth into multiple chunks
    let mut big = Vec::new();
    for _ in 0..200 {
        big.push(arena.alloc([0u8; 2048]));
    }

    let after = arena.chunk_usage();
    assert!(!after.is_empty());
    // At least one chunk should have non-zero usage
    assert!(after.iter().any(|c| c.used > 0));
    // All reported used must be <= capacity
    assert!(after.iter().all(|c| c.used <= c.capacity));
}
