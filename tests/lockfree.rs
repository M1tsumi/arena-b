#[cfg(feature = "lockfree")]
use arena_b::Arena;

#[cfg(feature = "lockfree")]
#[test]
fn test_lockfree_basic_allocation() {
    let arena = Arena::new();

    // Small allocations should use lock-free buffer
    let value1 = arena.alloc(42u8);
    let value2 = arena.alloc(100u16);
    let value3 = arena.alloc(999u32);

    assert_eq!(*value1, 42);
    assert_eq!(*value2, 100);
    assert_eq!(*value3, 999);

    // Check lock-free stats
    let (allocations, cache_hits, _cache_misses, _contention) = arena.lockfree_stats();
    assert!(allocations >= 3);
    assert!(cache_hits >= 3);
}

#[cfg(feature = "lockfree")]
#[test]
fn test_lockfree_large_allocations() {
    let arena = Arena::new();

    // Large allocations should bypass lock-free buffer
    let large_data = arena.alloc([0u8; 10000]);
    assert_eq!(large_data.len(), 10000);

    // Small allocations should still work
    let small_value = arena.alloc(42u32);
    assert_eq!(*small_value, 42);

    // Check stats - large allocation shouldn't use lock-free buffer
    let (allocations, cache_hits, _cache_misses, _contention) = arena.lockfree_stats();
    assert!(allocations >= 1);
    assert!(cache_hits >= 1);
}

#[cfg(feature = "lockfree")]
#[test]
fn test_lockfree_with_checkpoints() {
    let arena = Arena::new();

    let checkpoint = arena.checkpoint();

    // Make small allocations
    for i in 0..20 {
        arena.alloc(i);
    }

    let stats_before = arena.lockfree_stats();

    // Rewind should reset lock-free buffer
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Should be able to allocate again
    let new_value = arena.alloc(999u32);
    assert_eq!(*new_value, 999);

    // Buffer should be reset, so new allocation should succeed
    let stats_after = arena.lockfree_stats();
    assert!(stats_after.0 > stats_before.0); // More allocations
}

#[cfg(feature = "lockfree")]
#[test]
fn test_lockfree_reset() {
    let mut arena = Arena::new();

    // Make allocations
    for i in 0..50 {
        arena.alloc(i);
    }

    let stats_before = arena.lockfree_stats();

    // Reset should clear lock-free buffer
    unsafe {
        arena.reset();
    }

    // Should be able to allocate again
    let new_value = arena.alloc(42u32);
    assert_eq!(*new_value, 42);

    // Buffer should be reset
    let stats_after = arena.lockfree_stats();
    assert!(stats_after.0 > stats_before.0); // More allocations
}

#[cfg(feature = "lockfree")]
#[test]
fn test_lockfree_mixed_sizes() {
    let arena = Arena::new();

    // Mix of small and large allocations
    let small1 = arena.alloc(1u8);
    let medium = arena.alloc([0u8; 500]); // Should use lock-free
    let large = arena.alloc([0u8; 5000]); // Should bypass lock-free
    let small2 = arena.alloc(2u16);
    let small3 = arena.alloc(3u32);

    assert_eq!(*small1, 1);
    assert_eq!(medium.len(), 500);
    assert_eq!(large.len(), 5000);
    assert_eq!(*small2, 2);
    assert_eq!(*small3, 3);

    // Check that small/medium allocations used lock-free buffer
    let (allocations, cache_hits, _, _) = arena.lockfree_stats();
    assert!(allocations >= 3); // small1, medium, small2, small3
    assert!(cache_hits >= 3);
}

#[cfg(feature = "lockfree")]
#[test]
fn test_lockfree_contention() {
    let arena = Arena::new();

    // Fill up the lock-free buffer to cause contention
    for i in 0..1000 {
        arena.alloc(i);
    }

    let (allocations, cache_hits, _cache_misses, _contention) = arena.lockfree_stats();

    // Should have some allocations, possibly some cache misses as buffer fills
    assert!(allocations > 0);
    assert!(cache_hits > 0);
    // Some allocations might miss cache as buffer gets full
}

#[test]
fn test_regular_arena_without_lockfree() {
    // Ensure regular arena functionality works without lockfree feature
    let arena = arena_b::Arena::new();

    let value = arena.alloc(42u32);
    assert_eq!(*value, 42);

    let slice = arena.alloc_slice_copy(&[1, 2, 3]);
    assert_eq!(slice, &[1, 2, 3]);
}

#[cfg(feature = "lockfree")]
#[test]
fn test_lockfree_stats_accuracy() {
    let arena = Arena::new();

    // Make a known number of small allocations
    let initial_stats = arena.lockfree_stats();

    for i in 0..10 {
        arena.alloc(i);
    }

    let final_stats = arena.lockfree_stats();

    // Should have exactly 10 more allocations
    assert_eq!(final_stats.0, initial_stats.0 + 10);
    // Should have at least 10 cache hits (maybe more if buffer was empty)
    assert!(final_stats.1 >= initial_stats.1 + 10);
}
