#![cfg(test)]

use arena_b::Arena;

#[cfg(all(feature = "lockfree", not(feature = "single_thread_fast")))]
#[test]
fn lockfree_multithread_race_regression() {
    use std::sync::{Arc, Barrier, Mutex};
    use std::thread;

    const THREADS: usize = 8;
    const ALLOCS_PER_THREAD: usize = 64;

    let arena = Arc::new(Mutex::new(Arena::new()));
    let barrier = Arc::new(Barrier::new(THREADS));

    let mut handles = Vec::with_capacity(THREADS);
    for tid in 0..THREADS {
        let arena = Arc::clone(&arena);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for i in 0..ALLOCS_PER_THREAD {
                let value = (tid * 10_000 + i) as u32;
                let guard = arena.lock().unwrap();
                let slot = guard.alloc(value);
                assert_eq!(*slot, value);
            }
        }));
    }

    for handle in handles {
        handle.join().expect("lock-free worker thread panicked");
    }

    let expected = THREADS * ALLOCS_PER_THREAD;
    let stats = arena.lock().unwrap().lockfree_stats();
    assert!(
        stats.0 >= expected,
        "expected at least {} lock-free allocations, got {:?}",
        expected,
        stats
    );
}

#[cfg(feature = "virtual_memory")]
#[test]
fn virtual_memory_reset_does_not_leak_committed_pages() {
    let mut arena = Arena::with_virtual_memory(4 * 1024 * 1024);
    let payload = vec![0u8; 512 * 1024];

    let initial_committed = arena
        .virtual_memory_committed_bytes()
        .expect("virtual memory backing missing");

    for _ in 0..32 {
        let checkpoint = arena.checkpoint();
        let slice = arena.alloc_slice_copy(&payload);
        assert_eq!(slice.len(), payload.len());
        unsafe {
            arena.rewind_to_checkpoint(checkpoint);
        }

        // Each rewind should drop back to the initial committed footprint.
        let committed = arena.virtual_memory_committed_bytes().unwrap();
        assert!(
            committed <= initial_committed,
            "Committed bytes grew after rewind: initial={}, now={}",
            initial_committed,
            committed
        );
    }

    // Should still be able to allocate a large chunk after repeated resets
    let large = arena.alloc_slice_copy(&vec![1u8; 1_000_000]);
    assert_eq!(large.len(), 1_000_000);

    unsafe {
        arena.reset();
    }
    let final_committed = arena.virtual_memory_committed_bytes().unwrap();
    assert!(
        final_committed <= initial_committed,
        "Reset did not release committed pages: initial={}, final={}",
        initial_committed,
        final_committed
    );
}

#[cfg(target_pointer_width = "32")]
#[test]
fn high_alignment_on_32_bit_targets() {
    #[repr(C, align(64))]
    struct Aligned(u8);

    let arena = Arena::new();
    let value = arena.alloc(Aligned(0));
    let addr = value as *const Aligned as usize;
    assert_eq!(addr % 64, 0, "allocation was not 64-byte aligned on 32-bit");
}

#[test]
fn panic_during_scope_does_not_corrupt_arena() {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let arena = Arena::new();
    let checkpoint = arena.checkpoint();

    let result = catch_unwind(AssertUnwindSafe(|| {
        arena.scope(|scope| {
            let _ = scope.alloc(vec![1u8; 128]);
            panic!("simulated failure inside scope");
        });
    }));

    assert!(result.is_err(), "scope panic was not propagated");

    let value = arena.alloc(123u32);
    assert_eq!(*value, 123);

    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }
}
