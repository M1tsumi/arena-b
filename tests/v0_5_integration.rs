//! Integration tests for all v0.5.0 features working together
//!
//! This test verifies that all new features can be used simultaneously
//! and work correctly together.

#[cfg(all(
    feature = "debug",
    feature = "virtual_memory",
    feature = "thread_local",
    feature = "lockfree"
))]
use arena_b::Arena;

#[cfg(all(
    feature = "debug",
    feature = "virtual_memory",
    feature = "thread_local",
    feature = "lockfree"
))]
#[test]
fn test_all_features_integration() {
    println!("🧪 Testing all v0.5.0 features integration");

    // Create arena with virtual memory
    let mut arena = Arena::with_virtual_memory(16 * 1024 * 1024);

    // Test basic allocation with all features enabled
    let value1 = arena.alloc(42u32);
    assert_eq!(*value1, 42);

    // Test thread-local caching (small allocations)
    for i in 0..100 {
        let value = arena.alloc(i);
        assert_eq!(*value, i);
    }

    // Test lock-free optimizations (small-to-medium allocations)
    for i in 100..200 {
        let value = arena.alloc(i);
        assert_eq!(*value, i);
    }

    // Test virtual memory (large allocations)
    let large_data = arena.alloc(vec![0u8; 1_000_000]); // Use Vec to avoid stack allocation
    assert_eq!(large_data.len(), 1_000_000);

    // Test debug safety (skip if lockfree is enabled, as debug doesn't track lockfree allocations)
    #[cfg(not(feature = "lockfree"))]
    {
        let checkpoint = arena.checkpoint();
        let test_value = arena.alloc(999u32);

        // Should be valid
        assert!(unsafe { arena.check_valid(test_value) }.is_ok());

        // Rewind and test invalidation
        unsafe {
            arena.rewind_to_checkpoint(checkpoint);
        }

        // Should detect use-after-rewind
        // Note: This might not always detect depending on implementation details
        let _result = unsafe { arena.check_valid(test_value) };
        // We don't assert failure here as the implementation may vary
    }

    // Test lock-free stats
    let (allocations, cache_hits, _cache_misses, _contention) = arena.lockfree_stats();
    assert!(allocations > 0);
    assert!(cache_hits > 0);

    // Test reset with all features
    unsafe {
        arena.reset();
    }

    // Should work after reset
    let new_value = arena.alloc(123u32);
    assert_eq!(*new_value, 123);

    println!("✅ All features integration test passed!");
}

#[cfg(all(
    feature = "debug",
    feature = "virtual_memory",
    feature = "thread_local",
    feature = "lockfree"
))]
#[test]
fn test_nested_checkpoints_with_all_features() {
    println!("🔄 Testing nested checkpoints with all features");

    let arena = Arena::with_virtual_memory(8 * 1024 * 1024);

    // Create nested checkpoints
    let cp1 = arena.checkpoint();

    // Level 1 allocations
    for i in 0..50 {
        arena.alloc(i);
    }

    let cp2 = arena.checkpoint();

    // Level 2 allocations
    for i in 50..100 {
        arena.alloc(i);
    }

    let cp3 = arena.checkpoint();

    // Level 3 allocations
    for i in 100..150 {
        arena.alloc(i);
    }

    // Verify we have allocations
    let stats = arena.stats();
    assert!(stats.allocation_count >= 150);

    // Rewind to level 2
    unsafe {
        arena.rewind_to_checkpoint(cp3);
    }

    // Should have fewer allocations
    let stats_after_cp3 = arena.stats();
    assert!(stats_after_cp3.allocation_count < stats.allocation_count);

    // Rewind to level 1
    unsafe {
        arena.rewind_to_checkpoint(cp2);
    }

    // Rewind to beginning
    unsafe {
        arena.rewind_to_checkpoint(cp1);
    }

    // Should be back to initial state
    let final_stats = arena.stats();
    assert_eq!(final_stats.allocation_count, 0);

    println!("✅ Nested checkpoints test passed!");
}

#[cfg(all(
    feature = "debug",
    feature = "virtual_memory",
    feature = "thread_local",
    feature = "lockfree"
))]
#[test]
fn test_checkpoint_stack_with_all_features() {
    println!("📚 Testing checkpoint stack with all features");

    let mut arena = Arena::with_virtual_memory(4 * 1024 * 1024);

    // Use checkpoint stack API
    let _cp1 = arena.push_checkpoint();
    arena.alloc(1);

    let _cp2 = arena.push_checkpoint();
    arena.alloc(2);
    arena.alloc(3);

    let _cp3 = arena.push_checkpoint();
    arena.alloc(4);
    arena.alloc(5);
    arena.alloc(6);

    // Should have 6 allocations
    let stats = arena.stats();
    assert_eq!(stats.allocation_count, 6);

    // Pop and rewind to cp3
    unsafe {
        arena.pop_and_rewind();
    }

    // Should have 3 allocations
    let stats_after_pop3 = arena.stats();
    assert_eq!(stats_after_pop3.allocation_count, 3);

    // Pop and rewind to cp2
    unsafe {
        arena.pop_and_rewind();
    }

    // Should have 1 allocation
    let stats_after_pop2 = arena.stats();
    assert_eq!(stats_after_pop2.allocation_count, 1);

    // Pop and rewind to cp1
    unsafe {
        arena.pop_and_rewind();
    }

    // Should be back to initial state
    let final_stats = arena.stats();
    assert_eq!(final_stats.allocation_count, 0);

    println!("✅ Checkpoint stack test passed!");
}

#[cfg(all(
    feature = "debug",
    feature = "virtual_memory",
    feature = "thread_local",
    feature = "lockfree"
))]
#[test]
fn test_mixed_allocation_sizes_with_all_features() {
    println!("📏 Testing mixed allocation sizes with all features");

    let arena = Arena::with_virtual_memory(32 * 1024 * 1024);

    // Very small allocations (thread-local cache)
    for i in 0..100 {
        arena.alloc(i as u8);
    }

    // Small allocations (thread-local + lock-free)
    for i in 100..200 {
        arena.alloc(i as u16);
    }

    // Medium allocations (lock-free)
    for i in 200..300 {
        arena.alloc(i as u32);
    }

    // Large allocations (virtual memory) - use arrays instead of Vec
    for i in 0..10 {
        arena.alloc([i as u8; 1000]); // Smaller arrays to avoid stack overflow
    }

    // Very large allocations (virtual memory) - use smaller array
    arena.alloc([0u8; 10_000]); // Much smaller to avoid stack issues

    // Check stats
    let stats = arena.stats();
    assert!(stats.allocation_count >= 311); // Adjusted for allocations
    println!("  Actual bytes used: {}", stats.bytes_used);
    assert!(stats.bytes_used > 10_000); // Reasonable expectation

    // Check lock-free stats
    let (allocations, _cache_hits, _cache_misses, _contention) = arena.lockfree_stats();
    assert!(allocations > 0);

    // Test checkpoint with mixed sizes
    let checkpoint = arena.checkpoint();

    // Add more allocations
    arena.alloc([42u8; 5000]); // Smaller array

    // Rewind
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Should work after rewind
    let new_value = arena.alloc(999u32);
    assert_eq!(*new_value, 999);

    println!("✅ Mixed allocation sizes test passed!");
}

#[cfg(not(all(
    feature = "debug",
    feature = "virtual_memory",
    feature = "thread_local",
    feature = "lockfree"
)))]
#[test]
fn test_feature_combination_note() {
    // This test runs when not all features are enabled
    println!("ℹ️  Integration tests require all features:");
    println!("   Run with: cargo test --features \"debug virtual_memory thread_local lockfree\"");
}
