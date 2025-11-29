//! Demonstrates all new v0.5.0 features of arena-b
//!
//! This example showcases:
//! - Fast Reset API with checkpoints
//! - Memory Safety debugging
//! - Virtual Memory strategy
//! - Thread-Local caching
//! - Lock-Free optimizations

use arena_b::Arena;

fn main() {
    println!("🚀 arena-b v0.5.0 Features Demo\n");

    // Basic arena usage
    basic_arena_demo();

    // Fast Reset API
    fast_reset_demo();

    // Memory Safety Debugging
    #[cfg(feature = "debug")]
    memory_safety_demo();

    // Virtual Memory Strategy
    #[cfg(feature = "virtual_memory")]
    virtual_memory_demo();

    // Thread-Local Caching
    #[cfg(feature = "thread_local")]
    thread_local_demo();

    // Lock-Free Optimizations
    #[cfg(feature = "lockfree")]
    lockfree_demo();

    println!("✅ All v0.5.0 features demonstrated successfully!");
}

fn basic_arena_demo() {
    println!("📦 Basic Arena Usage:");

    let arena = Arena::new();

    // Allocate various types
    let number = arena.alloc(42u32);
    let text = arena.alloc_str("Hello, arena-b!");
    let slice = arena.alloc_slice_copy(&[1, 2, 3, 4, 5]);

    println!("  Number: {}", number);
    println!("  Text: {}", text);
    println!("  Slice: {:?}", slice);
    println!(
        "  Arena stats: bytes_used={}, allocation_count={}",
        arena.stats().bytes_used,
        arena.stats().allocation_count
    );
    println!();
}

fn fast_reset_demo() {
    println!("🔄 Fast Reset API Demo:");

    let mut arena = Arena::new();

    // Create initial checkpoint
    let checkpoint1 = arena.checkpoint();
    println!("  Created checkpoint 1");

    // Make some allocations
    for i in 0..100 {
        arena.alloc(i);
    }
    println!("  Allocated 100 items");

    // Create nested checkpoint
    let checkpoint2 = arena.checkpoint();
    println!("  Created checkpoint 2 (nested)");

    // Make more allocations
    for i in 100..200 {
        arena.alloc(i);
    }
    println!("  Allocated 100 more items (total: 200)");

    // Rewind to nested checkpoint
    unsafe {
        arena.rewind_to_checkpoint(checkpoint2);
    }
    println!("  Rewound to checkpoint 2 (100 items deallocated instantly)");

    // Rewind to first checkpoint
    unsafe {
        arena.rewind_to_checkpoint(checkpoint1);
    }
    println!("  Rewound to checkpoint 1 (all remaining items deallocated)");

    // Demonstrate checkpoint stack
    let _cp1 = arena.push_checkpoint();
    arena.alloc(1);
    let _cp2 = arena.push_checkpoint();
    arena.alloc(2);
    arena.alloc(3);

    unsafe {
        arena.pop_and_rewind(); // Rewind to cp2
    }
    println!("  Used checkpoint stack: popped and rewound");

    unsafe {
        arena.pop_and_rewind(); // Rewind to cp1
    }

    println!();
}

#[cfg(feature = "debug")]
fn memory_safety_demo() {
    println!("🛡️ Memory Safety Debugging Demo:");

    let arena = Arena::new();
    let checkpoint = arena.checkpoint();

    // Allocate a value
    let value = arena.alloc(42u32);
    println!("  Allocated value: {}", value);

    // Check validity (should be valid)
    match unsafe { arena.check_valid(value) } {
        Ok(()) => println!("  ✅ Value is valid before rewind"),
        Err(e) => println!("  ❌ Unexpected error: {}", e),
    }

    // Rewind the arena
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }
    println!("  Rewound arena");

    // Check validity again (should detect use-after-rewind)
    match unsafe { arena.check_valid(value) } {
        Ok(()) => println!("  ❌ Value should be invalid after rewind"),
        Err(e) => println!("  ✅ Correctly detected use-after-rewind: {}", e),
    }

    // Check debug stats
    let debug_stats = arena.debug_stats();
    println!(
        "  Debug stats: total_allocations={}, active_checkpoints={}, corrupted_allocations={}",
        debug_stats.total_allocations,
        debug_stats.active_checkpoints,
        debug_stats.corrupted_allocations
    );

    println!();
}

#[cfg(feature = "virtual_memory")]
fn virtual_memory_demo() {
    println!("💾 Virtual Memory Strategy Demo:");

    // Create arena with virtual memory backing (16MB reserve)
    let arena = Arena::with_virtual_memory(16 * 1024 * 1024);
    println!("  Created arena with 16MB virtual memory reserve");

    // Large allocation that benefits from virtual memory
    let large_data = arena.alloc(vec![0u8; 100_000]); // Use Vec to avoid stack allocation
    println!("  Allocated 100KB array: {} bytes", large_data.len());

    // Regular allocations still work
    let small_value = arena.alloc(42u32);
    println!("  Allocated small value: {}", small_value);

    // Many small allocations
    for i in 0..1000 {
        arena.alloc(i);
    }
    println!("  Allocated 1000 small values");

    println!(
        "  Arena stats: bytes_used={}, allocation_count={}",
        arena.stats().bytes_used,
        arena.stats().allocation_count
    );

    println!();
}

#[cfg(feature = "thread_local")]
fn thread_local_demo() {
    println!("🧵 Thread-Local Caching Demo:");

    let arena = Arena::new();

    // Small allocations will use thread-local cache
    println!("  Making small allocations (using thread-local cache)...");
    for i in 0..1000 {
        let value = arena.alloc(i);
        if i % 200 == 0 {
            println!("    Allocated value: {}", value);
        }
    }

    // Large allocations bypass thread-local cache
    println!("  Making large allocation (bypassing thread-local cache)...");
    let large_data = arena.alloc([0u8; 10000]);
    println!("  Allocated large array: {} bytes", large_data.len());

    // More small allocations
    println!("  More small allocations...");
    for i in 1000..1200 {
        arena.alloc(i);
    }

    println!(
        "  Arena stats: bytes_used={}, allocation_count={}",
        arena.stats().bytes_used,
        arena.stats().allocation_count
    );

    println!();
}

#[cfg(feature = "lockfree")]
fn lockfree_demo() {
    println!("⚡ Lock-Free Optimizations Demo:");

    let arena = Arena::new();

    // Small-to-medium allocations use lock-free buffer
    println!("  Making small-to-medium allocations (using lock-free buffer)...");
    for i in 0..1000 {
        let value = arena.alloc(i);
        if i % 200 == 0 {
            println!("    Allocated value: {}", value);
        }
    }

    // Check lock-free statistics
    let (allocations, cache_hits, cache_misses, contention) = arena.lockfree_stats();
    println!("  Lock-free stats:");
    println!("    Total allocations: {}", allocations);
    println!("    Cache hits: {}", cache_hits);
    println!("    Cache misses: {}", cache_misses);
    println!("    Contention events: {}", contention);

    if cache_hits > 0 {
        println!(
            "    Cache hit rate: {:.1}%",
            (cache_hits as f64 / allocations as f64) * 100.0
        );
    }

    // Large allocations bypass lock-free buffer
    println!("  Making large allocation (bypassing lock-free buffer)...");
    let large_data = arena.alloc([0u8; 5000]);
    println!("  Allocated large array: {} bytes", large_data.len());

    // More allocations to see contention
    println!("  Making more allocations to test contention...");
    for i in 1000..2000 {
        arena.alloc(i);
    }

    let final_stats = arena.lockfree_stats();
    println!("  Final lock-free stats:");
    println!("    Total allocations: {}", final_stats.0);
    println!("    Cache hits: {}", final_stats.1);
    println!("    Cache misses: {}", final_stats.2);
    println!("    Contention events: {}", final_stats.3);

    println!();
}
