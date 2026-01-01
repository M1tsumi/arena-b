use arena_b::Arena;

#[test]
fn alloc_basic() {
    let arena = Arena::new();
    let x = arena.alloc(42);
    assert_eq!(*x, 42);
    *x = 7;
    assert_eq!(*x, 7);
}

#[test]
fn alloc_slice_copy_basic() {
    let arena = Arena::new();
    let src = [1_u32, 2, 3];
    let dst = arena.alloc_slice_copy(&src);
    assert_eq!(dst, &src);
}

#[test]
fn alloc_str_basic() {
    let arena = Arena::new();
    let s = arena.alloc_str("hello");
    assert_eq!(s, "hello");
}

#[test]
fn reset_allows_reuse() {
    let mut arena = Arena::with_capacity(128);
    let first = arena.alloc(1_u32);
    assert_eq!(*first, 1);
    #[cfg(feature = "arena_module")]
    unsafe {
        arena.reset();
    }
    #[cfg(not(feature = "arena_module"))]
    arena.reset();
    let second = arena.alloc(2_u32);
    assert_eq!(*second, 2);
}

#[cfg(all(
    feature = "stats",
    not(any(feature = "thread_local", feature = "lockfree"))
))]
#[test]
fn stats_track_usage() {
    let arena = Arena::with_capacity(1024);
    let _ = arena.alloc(1_u64);
    let stats = arena.stats();
    assert_eq!(stats.bytes_allocated, 1024);
    assert!(stats.bytes_used > 0);
    assert!(stats.allocation_count >= 1);
    assert_eq!(stats.chunk_count, 1);
}

#[test]
fn alloc_slice_copy_zero_len() {
    let arena = Arena::new();
    let src: [u32; 0] = [];
    let dst = arena.alloc_slice_copy(&src);
    assert_eq!(dst.len(), 0);
}

#[test]
fn alloc_slice_uninit_zero_len() {
    let arena = Arena::new();
    let slice = arena.alloc_slice_uninit::<u32>(0);
    assert_eq!(slice.len(), 0);
}

#[cfg(feature = "stats")]
#[test]
fn zst_allocation_does_not_consume_space() {
    let arena = Arena::with_capacity(16);
    let stats_before = arena.stats();
    let _ = arena.alloc(());
    let stats_after = arena.stats();
    assert_eq!(stats_before.bytes_allocated, stats_after.bytes_allocated);
    assert_eq!(stats_before.bytes_used, stats_after.bytes_used);
    assert!(stats_after.allocation_count > stats_before.allocation_count);
}

#[cfg(not(any(feature = "thread_local", feature = "lockfree")))]
#[test]
fn multi_chunk_allocation_grows_arena() {
    // Create a small arena that will definitely require chunk growth
    let arena = Arena::with_capacity(32); // 32 bytes, rounded up to 64
    let _ = arena.alloc([0_u8; 32]); // Uses 32 bytes, leaves 32
    let stats_after_first = arena.stats();
    let _ = arena.alloc([0_u8; 40]); // Needs 40 bytes, only 32 left - should trigger new chunk
    let stats_after_second = arena.stats();
    assert!(stats_after_second.chunk_count >= 2);
    assert!(stats_after_second.bytes_allocated >= stats_after_first.bytes_allocated);
}

#[cfg(all(
    feature = "stats",
    not(any(feature = "thread_local", feature = "lockfree"))
))]
#[test]
fn scope_reclaims_allocations() {
    let arena = Arena::with_capacity(128);
    let before = arena.stats();

    arena.scope(|scope| {
        let _ = scope.alloc([0_u8; 32]);
        let _ = scope.alloc([0_u8; 32]);
        let during = arena.stats();
        assert!(during.bytes_used > before.bytes_used);
    });

    let after = arena.stats();
    assert_eq!(before.bytes_used, after.bytes_used);
    assert_eq!(before.allocation_count, after.allocation_count);
}
