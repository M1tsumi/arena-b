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
fn test_use_after_rewind_detection() {
    let arena = Arena::new();

    // Create checkpoint and allocate
    let checkpoint = arena.checkpoint();
    let value = arena.alloc(42u32);
    let slice = arena.alloc_slice_copy(&[1, 2, 3, 4, 5]);

    // Check validity before rewind - should be valid
    unsafe {
        arena.check_valid(value).unwrap();
        arena.check_valid(&slice[0]).unwrap();
    }

    // Verify arena state is healthy
    arena.validate_debug_state().unwrap();
    let stats = arena.debug_stats();
    assert_eq!(stats.total_allocations, 2);
    assert_eq!(stats.corrupted_allocations, 0);

    // Rewind to checkpoint
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Now check validity - should detect use-after-rewind
    unsafe {
        assert!(arena.check_valid(value).is_err());
        assert!(arena.check_valid(&slice[0]).is_err());
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
fn test_nested_checkpoint_safety() {
    let arena = Arena::new();

    let outer_checkpoint = arena.checkpoint();
    let outer_value = arena.alloc(100u32);

    // Valid before nested checkpoint
    unsafe {
        arena.check_valid(outer_value).unwrap();
    }

    // Create nested checkpoint
    let inner_checkpoint = arena.checkpoint();
    let inner_value = arena.alloc(200u32);

    // Both should be valid
    unsafe {
        arena.check_valid(outer_value).unwrap();
        arena.check_valid(inner_value).unwrap();
    }

    // Rewind to inner checkpoint
    unsafe {
        arena.rewind_to_checkpoint(inner_checkpoint);
    }

    // Outer should still be valid, inner should be invalid
    unsafe {
        arena.check_valid(outer_value).unwrap();
        assert!(arena.check_valid(inner_value).is_err());
    }

    // Rewind to outer checkpoint
    unsafe {
        arena.rewind_to_checkpoint(outer_checkpoint);
    }

    // Both should be invalid now
    unsafe {
        assert!(arena.check_valid(outer_value).is_err());
        assert!(arena.check_valid(inner_value).is_err());
    }
}

#[cfg(all(
    feature = "debug",
    not(any(feature = "thread_local", feature = "lockfree"))
))]
#[test]
fn test_debug_stats_tracking() {
    let arena = Arena::new();

    // Initial state
    let stats = arena.debug_stats();
    assert_eq!(stats.total_allocations, 0);
    assert_eq!(stats.active_checkpoints, 0);
    assert_eq!(stats.current_checkpoint_id, 1);
    assert_eq!(stats.corrupted_allocations, 0);

    // Create checkpoint
    let checkpoint = arena.checkpoint();
    let stats = arena.debug_stats();
    assert_eq!(stats.active_checkpoints, 1);

    // Make allocations
    arena.alloc(42u32);
    arena.alloc_slice_copy(&[1, 2, 3]);
    arena.alloc_str("hello world");

    let stats = arena.debug_stats();
    assert_eq!(stats.total_allocations, 3);
    assert_eq!(stats.corrupted_allocations, 0);

    // Rewind
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Stats should show rewound allocations as invalid
    let stats = arena.debug_stats();
    assert_eq!(stats.total_allocations, 0); // All allocations invalidated
    assert_eq!(stats.corrupted_allocations, 0);
}

#[cfg(all(
    feature = "debug",
    not(any(feature = "thread_local", feature = "lockfree"))
))]
#[test]
fn test_multiple_arenas_isolation() {
    let arena1 = Arena::new();
    let arena2 = Arena::new();

    // Allocate in different arenas
    let value1 = arena1.alloc(42u32);
    let value2 = arena2.alloc(100u32);

    // Each arena should only track its own allocations
    let stats1 = arena1.debug_stats();
    let stats2 = arena2.debug_stats();

    assert_eq!(stats1.total_allocations, 1);
    assert_eq!(stats2.total_allocations, 1);

    // Validity checks should work independently
    unsafe {
        arena1.check_valid(value1).unwrap();
        arena2.check_valid(value2).unwrap();
    }

    // Rewinding one arena shouldn't affect the other
    let checkpoint1 = arena1.checkpoint();
    let checkpoint2 = arena2.checkpoint();

    arena1.alloc(200u32);
    arena2.alloc(300u32);

    unsafe {
        arena1.rewind_to_checkpoint(checkpoint1);
        // Only arena1's new allocation should be invalid
        arena1.check_valid(value1).unwrap();
        arena2.check_valid(value2).unwrap();
    }
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

#[cfg(all(
    feature = "debug",
    not(any(feature = "thread_local", feature = "lockfree"))
))]
#[test]
fn test_comprehensive_safety_scenarios() {
    let arena = Arena::new();

    // Test various allocation types
    let checkpoint = arena.checkpoint();

    let simple = arena.alloc(42u32);
    let array = arena.alloc_array([1, 2, 3, 4, 5]);
    let slice = arena.alloc_slice_copy(&[10, 20, 30]);
    let string = arena.alloc_str("test string");
    let uninit = arena.alloc_array_uninit::<u32, 3>();

    // All should be valid
    unsafe {
        arena.check_valid(simple).unwrap();
        arena.check_valid(&array[0]).unwrap();
        arena.check_valid(&slice[0]).unwrap();
        arena.check_valid(&string.as_bytes()[0]).unwrap();
        arena.check_valid(&uninit[0]).unwrap();
    }

    // Rewind and check invalidation
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);

        assert!(arena.check_valid(simple).is_err());
        assert!(arena.check_valid(&array[0]).is_err());
        assert!(arena.check_valid(&slice[0]).is_err());
        assert!(arena.check_valid(&string.as_bytes()[0]).is_err());
        assert!(arena.check_valid(&uninit[0]).is_err());
    }

    // Arena should still be functional
    let fresh_alloc = arena.alloc(999u32);
    unsafe {
        arena.check_valid(fresh_alloc).unwrap();
    }
}
