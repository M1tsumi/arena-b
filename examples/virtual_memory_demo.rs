//! Demonstrates virtual memory strategy for large arena allocations
//!
//! This example shows how to use the virtual memory feature for efficient
//! handling of large allocations and reduced memory pressure.

#[cfg(feature = "virtual_memory")]
use arena_b::Arena;

#[cfg(feature = "virtual_memory")]
fn main() {
    println!("💾 Virtual Memory Strategy Demo\n");

    // Compare regular arena vs virtual memory arena
    compare_arena_types();

    // Demonstrate large allocation patterns
    large_allocation_demo();

    // Show memory efficiency
    memory_efficiency_demo();

    println!("✅ Virtual memory demo completed!");
}

#[cfg(feature = "virtual_memory")]
fn compare_arena_types() {
    println!("📊 Comparing Arena Types:");

    // Regular arena
    let regular_arena = Arena::new();
    // Allocate on the heap first to avoid huge stack allocations
    let regular_heap = vec![0u8; 1_000_000];
    let regular_data = regular_arena.alloc(regular_heap);
    println!("  Regular arena: allocated {} bytes", regular_data.len());

    // Virtual memory arena
    let vm_arena = Arena::with_virtual_memory(16 * 1024 * 1024); // 16MB reserve
    let vm_heap = vec![0u8; 1_000_000];
    let vm_data = vm_arena.alloc(vm_heap);
    println!("  Virtual arena: allocated {} bytes", vm_data.len());

    // Both work the same for normal operations
    let small_regular = regular_arena.alloc(42u32);
    let small_vm = vm_arena.alloc(42u32);

    println!("  Regular small value: {}", small_regular);
    println!("  Virtual small value: {}", small_vm);

    println!();
}

#[cfg(feature = "virtual_memory")]
fn large_allocation_demo() {
    println!("🔢 Large Allocation Patterns:");

    let arena = Arena::with_virtual_memory(64 * 1024 * 1024); // 64MB reserve

    // Multiple large allocations
    let allocations: Vec<_> = (0..10)
        .map(|i| {
            let size = (i + 1) * 1_000_000; // 1MB, 2MB, 3MB, ..., 10MB
            let data = arena.alloc(vec![i as u8; size]);
            println!("  Allocated {}MB array with value {}", size / 1_000_000, i);
            data
        })
        .collect();

    // Verify allocations
    for (i, data) in allocations.iter().enumerate() {
        println!(
            "  Array {} has {} elements, first value: {}",
            i,
            data.len(),
            data[0]
        );
    }

    println!("  Total arena usage: {} bytes", arena.stats().bytes_used);
    println!();
}

#[cfg(feature = "virtual_memory")]
fn memory_efficiency_demo() {
    println!("📈 Memory Efficiency Demonstration:");

    let mut arena = Arena::with_virtual_memory(32 * 1024 * 1024); // 32MB reserve

    println!("  Initial arena stats:");
    print_stats(&arena);

    // Allocate a large amount of data
    println!("  Allocating 10MB of data...");
    let large_heap = vec![0u8; 10_000_000];
    let large_data = arena.alloc(large_heap);

    println!("  After large allocation:");
    print_stats(&arena);

    // Allocate many small objects
    println!("  Allocating 100,000 small objects...");
    let small_objects: Vec<_> = (0..100_000).map(|i| arena.alloc(i)).collect();

    println!("  After small objects:");
    print_stats(&arena);

    // Use the data
    println!("  Using allocated data:");
    println!("    Large array size: {}", large_data.len());
    println!("    Small objects count: {}", small_objects.len());
    println!("    First small object: {}", small_objects[0]);
    println!("    Last small object: {}", small_objects[99999]);

    // Reset and reuse
    println!("  Resetting arena...");
    unsafe {
        arena.reset();
    }

    println!("  After reset:");
    print_stats(&arena);

    // Reuse after reset
    println!("  Reusing arena after reset...");
    let new_heap = vec![1u8; 5_000_000];
    let new_data = arena.alloc(new_heap);
    println!("  New allocation size: {}", new_data.len());

    println!("  Final stats:");
    print_stats(&arena);

    println!();
}

#[cfg(feature = "virtual_memory")]
fn print_stats(arena: &Arena) {
    let stats = arena.stats();
    println!("    Bytes used: {}", stats.bytes_used);
    println!("    Allocation count: {}", stats.allocation_count);
    println!("    Chunk count: {}", stats.chunk_count);
}

#[cfg(not(feature = "virtual_memory"))]
fn main() {
    println!("❌ This example requires the 'virtual_memory' feature to be enabled.");
    println!("   Run with: cargo run --example virtual_memory_demo --features virtual_memory");
}
