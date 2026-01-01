use arena_b::Arena;
use proptest::prelude::*;

proptest! {
    #[test]
    fn alloc_slice_copy_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..512)) {
        let arena = Arena::with_capacity(4096);
        let out = arena.alloc_slice_copy(&data);
        prop_assert_eq!(out, &data[..]);
    }

    #[test]
    fn reset_frees_all_memory(sizes in proptest::collection::vec(1usize..64, 0..16)) {
        let mut arena = Arena::with_capacity(2048);
        for size in &sizes {
            let buf = vec![0_u8; *size];
            let _ = arena.alloc_slice_copy(&buf);
        }

        unsafe {
            arena.reset();
        }

        let stats = arena.stats();
        prop_assert_eq!(stats.bytes_used, 0);
        prop_assert_eq!(stats.allocation_count, 0);
    }
}
