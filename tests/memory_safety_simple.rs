#[cfg(all(
    feature = "debug",
    not(any(feature = "thread_local", feature = "lockfree"))
))]
use arena_b::Arena;

#[cfg(all(
    feature = "debug",
    not(any(feature = "thread_local", feature = "lockfree"))
))]
#[test]
fn test_basic_use_after_rewind_detection() {
    let arena = Arena::new();

    // Create checkpoint and allocate
    let checkpoint = arena.checkpoint();
    let value = arena.alloc(42u32);

    // Check validity before rewind - should be valid
    unsafe {
        arena.check_valid(value).unwrap();
    }

    // Rewind to checkpoint
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Now check validity - should detect use-after-rewind
    unsafe {
        assert!(arena.check_valid(value).is_err());
    }

    // Should still be able to allocate new valid data
    let new_value = arena.alloc(99u32);
    unsafe {
        arena.check_valid(new_value).unwrap();
    }
}

#[cfg(all(
    feature = "debug",
    not(any(feature = "thread_local", feature = "lockfree"))
))]
#[test]
fn test_debug_stats_basic() {
    let arena = Arena::new();

    // Initial state
    let stats = arena.debug_stats();
    assert_eq!(stats.total_allocations, 0);
    assert_eq!(stats.active_checkpoints, 0);
    assert_eq!(stats.corrupted_allocations, 0);

    // Make allocations
    arena.alloc(42u32);
    arena.alloc_slice_copy(&[1, 2, 3]);

    let stats = arena.debug_stats();
    assert_eq!(stats.total_allocations, 2);
    assert_eq!(stats.corrupted_allocations, 0);

    // Create checkpoint
    let checkpoint = arena.checkpoint();
    arena.alloc(100u32);

    let stats = arena.debug_stats();
    assert_eq!(stats.total_allocations, 3);
    assert_eq!(stats.active_checkpoints, 1);

    // Rewind
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Stats should show rewound allocations as invalid
    let stats = arena.debug_stats();
    assert_eq!(stats.total_allocations, 2); // Only allocations before checkpoint remain
    assert_eq!(stats.corrupted_allocations, 0);
}

#[test]
fn test_debug_feature_compiles() {
    // This test ensures the code compiles both with and without debug feature
    let arena = arena_b::Arena::new();
    let checkpoint = arena.checkpoint();
    let _value = arena.alloc(42u32);

    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Should be able to allocate after rewind
    let new_value = arena.alloc(99u32);
    assert_eq!(*new_value, 99);
}
