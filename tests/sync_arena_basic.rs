#![cfg(not(feature = "single_thread_fast"))]

use std::sync::Arc;
use std::thread;

use arena_b::SyncArena;

#[test]
fn sync_arena_multithread_basic() {
    let arena = Arc::new(SyncArena::with_capacity(16 * 1024));
    let mut handles = Vec::new();

    for _ in 0..4 {
        let a = Arc::clone(&arena);
        handles.push(thread::spawn(move || {
            a.scope(|scope| {
                for i in 0..256u32 {
                    let x = scope.alloc(i);
                    assert_eq!(*x, i);
                }
            });
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}
