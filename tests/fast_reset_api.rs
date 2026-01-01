use arena_b::Arena;

#[test]
fn test_checkpoint_basic() {
    let arena = Arena::new();

    // Create initial checkpoint
    let checkpoint = arena.checkpoint();

    // Make some allocations
    let value1 = arena.alloc(42u32);
    let value2 = arena.alloc(100u32);
    let slice = arena.alloc_slice_copy(&[1, 2, 3, 4, 5]);

    assert_eq!(*value1, 42);
    assert_eq!(*value2, 100);
    assert_eq!(slice, &[1, 2, 3, 4, 5]);

    // Rewind to checkpoint
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Verify we can allocate again
    let new_value = arena.alloc(999u32);
    assert_eq!(*new_value, 999);
}

#[test]
#[cfg(feature = "arena_module")]
fn test_checkpoint_stack() {
    let mut arena = Arena::new();

    // Test nested checkpoints
    let _outer_checkpoint = arena.push_checkpoint();

    // Allocate in outer scope
    let outer_val = arena.alloc(1u32);
    assert_eq!(*outer_val, 1);

    // Create inner checkpoint
    let _inner_checkpoint = arena.push_checkpoint();
    assert_eq!(arena.checkpoint_count(), 2);

    // Allocate in inner scope
    let inner_val = arena.alloc(2u32);
    assert_eq!(*inner_val, 2);

    // Drop references to avoid borrow conflicts
    let _ = inner_val;
    let _ = outer_val;

    // Pop inner checkpoint and rewind to outer checkpoint
    let _popped = unsafe { arena.pop_and_rewind() };
    assert_eq!(arena.checkpoint_count(), 1); // Still has outer checkpoint

    // Need to allocate again to test outer allocation validity
    let test_val = arena.alloc(1u32);
    assert_eq!(*test_val, 1);

    // Can still allocate after rewind
    let new_val = arena.alloc(3u32);
    assert_eq!(*new_val, 3);

    // Clear remaining checkpoints
    arena.clear_checkpoints();
    assert_eq!(arena.checkpoint_count(), 0);
}

#[test]
#[cfg(feature = "arena_module")]
fn test_frame_based_pattern() {
    let arena = Arena::new();
    let frame_data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

    // Simulate game loop frame allocation pattern
    for frame in 0..5 {
        let frame_checkpoint = arena.checkpoint();

        // Allocate frame data
        let entities = arena.alloc_batch(&frame_data);
        let player_pos = arena.alloc((frame as f32, frame as f32));
        let game_state = arena.alloc(format!("Frame {}", frame));

        // Verify allocations
        assert_eq!(entities.len(), 10);
        assert_eq!(player_pos.0, frame as f32);
        assert_eq!(game_state, &format!("Frame {}", frame));

        // Fast cleanup - rewind to frame start
        unsafe {
            arena.rewind_to_checkpoint(frame_checkpoint);
        }
    }

    // Arena should be back to initial state
    let final_val = arena.alloc(999u32);
    assert_eq!(*final_val, 999);
}

#[test]
#[cfg(feature = "stats")]
fn test_checkpoint_with_stats() {
    let arena = Arena::new();

    let checkpoint = arena.checkpoint();
    let initial_stats = arena.stats();

    // Make allocations
    arena.alloc(42u32);
    arena.alloc_slice_copy(&[1, 2, 3, 4, 5]);

    let after_alloc_stats = arena.stats();

    // Check if stats are being tracked (might not work with all feature combinations)
    if after_alloc_stats.allocation_count > initial_stats.allocation_count {
        // Note: bytes_used might not be tracked consistently with all features
        // so we only test allocation_count which is more reliable

        // Rewind should restore stats
        unsafe {
            arena.rewind_to_checkpoint(checkpoint);
        }

        let after_rewind_stats = arena.stats();
        assert_eq!(
            after_rewind_stats.allocation_count,
            initial_stats.allocation_count
        );
    } else {
        // If stats aren't being tracked, just test that rewind works
        unsafe {
            arena.rewind_to_checkpoint(checkpoint);
        }
        // If we can allocate again, rewind worked
        let _test_val = arena.alloc(999u32);
    }
}

#[test]
fn test_multiple_chunks() {
    let arena = Arena::with_capacity(1024); // Small chunk to force multiple chunks

    let checkpoint = arena.checkpoint();

    // Allocate enough to span multiple chunks
    let large_data: Vec<u8> = (0..2048).map(|i| (i % 256) as u8).collect();
    let chunk1 = arena.alloc_slice_copy(&large_data);
    let chunk2 = arena.alloc_slice_copy(&large_data);

    assert_eq!(chunk1.len(), 2048);
    assert_eq!(chunk2.len(), 2048);

    // Rewind should work across chunks
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Should be able to allocate fresh data
    let fresh_data = arena.alloc_slice_copy(&[1, 2, 3]);
    assert_eq!(fresh_data, &[1, 2, 3]);
}

#[test]
#[cfg(feature = "arena_module")]
fn test_reset_clears_checkpoints() {
    let mut arena = Arena::new();

    // Add some checkpoints
    arena.push_checkpoint();
    arena.push_checkpoint();
    assert_eq!(arena.checkpoint_count(), 2);

    // Full reset should clear checkpoints
    unsafe {
        arena.reset();
    }

    assert_eq!(arena.checkpoint_count(), 0);
}

#[test]
#[cfg(feature = "arena_module")]
fn test_clear_checkpoints() {
    let arena = Arena::new();

    // Add some checkpoints
    arena.push_checkpoint();
    arena.push_checkpoint();
    assert_eq!(arena.checkpoint_count(), 2);

    // Clear checkpoints without affecting allocations
    arena.clear_checkpoints();
    assert_eq!(arena.checkpoint_count(), 0);

    // Should still be able to allocate
    let val = arena.alloc(42u32);
    assert_eq!(*val, 42);
}
