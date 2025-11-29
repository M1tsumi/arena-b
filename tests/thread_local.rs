#[cfg(feature = "thread_local")]
use arena_b::Arena;

#[cfg(feature = "thread_local")]
#[test]
fn test_thread_local_basic_allocation() {
    let arena = Arena::new();

    // Small allocations should use thread-local cache
    let value1 = arena.alloc(42u8);
    let value2 = arena.alloc(100u16);
    let value3 = arena.alloc(999u32);

    assert_eq!(*value1, 42);
    assert_eq!(*value2, 100);
    assert_eq!(*value3, 999);
}

#[cfg(feature = "thread_local")]
#[test]
fn test_thread_local_with_checkpoints() {
    let arena = Arena::new();

    let checkpoint = arena.checkpoint();

    // Make small allocations
    for i in 0..10 {
        arena.alloc(i);
    }

    // Verify allocations
    for i in 0..10 {
        // Can't easily verify after checkpoint in this test
        // but the allocation should succeed
        let _value = arena.alloc(i);
    }

    // Rewind should reset thread-local cache
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Should be able to allocate again
    let new_value = arena.alloc(999u32);
    assert_eq!(*new_value, 999);
}

#[cfg(feature = "thread_local")]
#[test]
fn test_thread_local_reset() {
    let mut arena = Arena::new();

    // Make allocations
    for i in 0..50 {
        arena.alloc(i);
    }

    // Reset should clear thread-local cache
    unsafe {
        arena.reset();
    }

    // Should be able to allocate again
    let new_value = arena.alloc(42u32);
    assert_eq!(*new_value, 42);
}

#[cfg(feature = "thread_local")]
#[test]
fn test_thread_local_multi_thread() {
    // Note: This test demonstrates thread-local caching concept
    // but Arena is currently not Send+Sync, so we test single-threaded behavior

    let arena = Arena::new();

    // Simulate multiple allocation patterns that would benefit from thread-local cache
    for thread_id in 0..4 {
        // Each "thread" makes small allocations
        for i in 0..10 {
            let value = arena.alloc((thread_id * 10 + i) as u32);
            assert_eq!(*value, (thread_id * 10 + i) as u32);
        }
    }
}

#[cfg(feature = "thread_local")]
#[test]
fn test_thread_local_large_allocations() {
    let arena = Arena::new();

    // Large allocations should bypass thread-local cache
    let large_data = arena.alloc([0u8; 10000]);
    assert_eq!(large_data.len(), 10000);

    // Small allocations should still work
    let small_value = arena.alloc(42u32);
    assert_eq!(*small_value, 42);
}

#[cfg(feature = "thread_local")]
#[test]
fn test_thread_local_mixed_sizes() {
    let arena = Arena::new();

    // Mix of small and large allocations
    let small1 = arena.alloc(1u8);
    let large = arena.alloc([0u8; 5000]);
    let small2 = arena.alloc(2u16);
    let small3 = arena.alloc(3u32);

    assert_eq!(*small1, 1);
    assert_eq!(large.len(), 5000);
    assert_eq!(*small2, 2);
    assert_eq!(*small3, 3);
}

#[test]
fn test_regular_arena_without_thread_local() {
    // Ensure regular arena functionality works without thread_local feature
    let arena = arena_b::Arena::new();

    let value = arena.alloc(42u32);
    assert_eq!(*value, 42);

    let slice = arena.alloc_slice_copy(&[1, 2, 3]);
    assert_eq!(slice, &[1, 2, 3]);
}
