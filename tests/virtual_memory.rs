#[cfg(feature = "virtual_memory")]
use arena_b::Arena;

#[cfg(feature = "virtual_memory")]
#[test]
fn test_virtual_memory_creation() {
    let arena = Arena::with_virtual_memory(1024 * 1024); // 1MB reserve

    // Should be able to allocate normally
    let value = arena.alloc(42u32);
    assert_eq!(*value, 42);

    let slice = arena.alloc_slice_copy(&[1, 2, 3, 4, 5]);
    assert_eq!(slice, &[1, 2, 3, 4, 5]);
}

#[cfg(feature = "virtual_memory")]
#[test]
fn test_virtual_memory_large_allocations() {
    let arena = Arena::with_virtual_memory(16 * 1024 * 1024); // 16MB reserve

    // Make many allocations to test commit behavior
    for i in 0..1000 {
        let value = arena.alloc(i);
        assert_eq!(*value, i);
    }

    // Allocate a large slice
    let large_slice = arena.alloc_slice_copy(&vec![42u8; 10000]);
    assert_eq!(large_slice.len(), 10000);
    assert_eq!(large_slice[0], 42);
    assert_eq!(large_slice[9999], 42);
}

#[cfg(feature = "virtual_memory")]
#[test]
fn test_virtual_memory_with_checkpoints() {
    let arena = Arena::with_virtual_memory(4 * 1024 * 1024); // 4MB reserve

    let checkpoint = arena.checkpoint();

    // Make allocations
    let value1 = arena.alloc(100u32);
    let slice1 = arena.alloc_slice_copy(&[1, 2, 3, 4, 5]);

    // Verify allocations
    assert_eq!(*value1, 100);
    assert_eq!(slice1, &[1, 2, 3, 4, 5]);

    // Rewind and verify
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }

    // Should be able to allocate new values
    let value2 = arena.alloc(200u32);
    assert_eq!(*value2, 200);
}

#[test]
fn test_regular_arena_still_works() {
    // Ensure regular arena functionality is not affected
    let arena = arena_b::Arena::new();

    let value = arena.alloc(42u32);
    assert_eq!(*value, 42);

    let slice = arena.alloc_slice_copy(&[1, 2, 3]);
    assert_eq!(slice, &[1, 2, 3]);
}

#[cfg(feature = "virtual_memory")]
#[test]
fn test_virtual_memory_reset() {
    let mut arena = Arena::with_virtual_memory(2 * 1024 * 1024); // 2MB reserve

    // Make allocations
    for i in 0..100 {
        arena.alloc(i);
    }

    // Reset the arena
    unsafe {
        arena.reset();
    }

    // Should be able to allocate again
    let new_value = arena.alloc(999u32);
    assert_eq!(*new_value, 999);
}
