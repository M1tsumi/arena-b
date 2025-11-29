//! Demonstrates memory safety debugging features
//!
//! This example shows how to use the debug feature to detect memory safety issues
//! like use-after-rewind and allocation corruption.

#[cfg(feature = "debug")]
use arena_b::Arena;

#[cfg(feature = "debug")]
fn main() {
    println!("🛡️ Memory Safety Debugging Demo\n");

    // Basic safety checking
    basic_safety_demo();

    // Use-after-rewind detection
    use_after_rewind_demo();

    // Nested checkpoint safety
    nested_checkpoint_demo();

    // Debug statistics
    debug_stats_demo();

    println!("✅ Memory safety debugging demo completed!");
}

#[cfg(feature = "debug")]
fn basic_safety_demo() {
    println!("🔍 Basic Safety Checking:");

    let arena = Arena::new();
    let checkpoint = arena.checkpoint();

    // Allocate a value
    let value = arena.alloc(42u32);
    println!("  Allocated value: {}", value);

    // Check validity (should be valid)
    match unsafe { arena.check_valid(value) } {
        Ok(()) => println!("  ✅ Value is valid"),
        Err(e) => println!("  ❌ Unexpected error: {}", e),
    }

    // Allocate more values
    let values: Vec<_> = (0..10).map(|i| arena.alloc(i)).collect();
    println!("  Allocated {} more values", values.len());

    // Check all values
    let mut valid_count = 0;
    for &value in &values {
        if unsafe { arena.check_valid(value) }.is_ok() {
            valid_count += 1;
        }
    }
    println!("  {} out of {} values are valid", valid_count, values.len());

    println!();
}

#[cfg(feature = "debug")]
fn use_after_rewind_demo() {
    println!("⚠️  Use-After-Rewind Detection:");

    let arena = Arena::new();
    let checkpoint = arena.checkpoint();

    // Allocate values
    let value1 = arena.alloc(100u32);
    let value2 = arena.alloc(200u32);
    let value3 = arena.alloc(300u32);

    println!("  Allocated values: {}, {}, {}", value1, value2, value3);

    // Check validity before rewind
    let valid_before = vec![value1, value2, value3]
        .iter()
        .filter(|&&v| unsafe { arena.check_valid(v) }.is_ok())
        .count();
    println!("  Valid values before rewind: {}", valid_before);

    // Rewind the arena
    unsafe {
        arena.rewind_to_checkpoint(checkpoint);
    }
    println!("  Rewound arena");

    // Check validity after rewind (should detect use-after-rewind)
    let valid_after = vec![value1, value2, value3]
        .iter()
        .filter(|&&v| unsafe { arena.check_valid(v) }.is_ok())
        .count();
    println!("  Valid values after rewind: {}", valid_after);

    // Try to access the values (this would be unsafe in real code)
    println!("  Attempting to validate each value:");
    for (i, &value) in [value1, value2, value3].iter().enumerate() {
        match unsafe { arena.check_valid(value) } {
            Ok(()) => println!("    Value {}: ❌ Still valid (should be invalid)", i + 1),
            Err(e) => println!(
                "    Value {}: ✅ Correctly detected as invalid: {}",
                i + 1,
                e
            ),
        }
    }

    println!();
}

#[cfg(feature = "debug")]
fn nested_checkpoint_demo() {
    println!("🔄 Nested Checkpoint Safety:");

    let arena = Arena::new();

    // First level
    let checkpoint1 = arena.checkpoint();
    let value1 = arena.alloc(1u32);
    println!("  Level 1: allocated value {}", value1);

    // Second level
    let checkpoint2 = arena.checkpoint();
    let value2 = arena.alloc(2u32);
    let value3 = arena.alloc(3u32);
    println!("  Level 2: allocated values {} and {}", value2, value3);

    // Third level
    let checkpoint3 = arena.checkpoint();
    let value4 = arena.alloc(4u32);
    let value5 = arena.alloc(5u32);
    println!("  Level 3: allocated values {} and {}", value4, value5);

    // Check all values at deepest level
    let values = [value1, value2, value3, value4, value5];
    let valid_count = values
        .iter()
        .filter(|&&v| unsafe { arena.check_valid(v) }.is_ok())
        .count();
    println!("  Valid values at level 3: {}", valid_count);

    // Rewind to level 2
    unsafe {
        arena.rewind_to_checkpoint(checkpoint3);
    }
    println!("  Rewound to level 2");

    // Check values after rewind to level 2
    let valid_after_l2 = [value1, value2, value3]
        .iter()
        .filter(|&&v| unsafe { arena.check_valid(v) }.is_ok())
        .count();
    println!("  Valid values after rewind to level 2: {}", valid_after_l2);

    // value4 and value5 should be invalid
    println!("  Checking level 3 values:");
    for (i, &value) in [value4, value5].iter().enumerate() {
        match unsafe { arena.check_valid(value) } {
            Ok(()) => println!("    Level 3 value {}: ❌ Still valid", i + 4),
            Err(e) => println!(
                "    Level 3 value {}: ✅ Correctly detected as invalid",
                i + 4
            ),
        }
    }

    // Rewind to level 1
    unsafe {
        arena.rewind_to_checkpoint(checkpoint2);
    }
    println!("  Rewound to level 1");

    // Only value1 should be valid
    match unsafe { arena.check_valid(value1) } {
        Ok(()) => println!("  Level 1 value: ✅ Still valid"),
        Err(e) => println!("  Level 1 value: ❌ Unexpectedly invalid: {}", e),
    }

    println!();
}

#[cfg(feature = "debug")]
fn debug_stats_demo() {
    println!("📊 Debug Statistics:");

    let arena = Arena::new();

    // Make some allocations
    let checkpoint1 = arena.checkpoint();
    for i in 0..100 {
        arena.alloc(i);
    }

    let checkpoint2 = arena.checkpoint();
    for i in 100..200 {
        arena.alloc(i);
    }

    // Get debug stats
    let stats = arena.debug_stats();
    println!("  Debug statistics:");
    println!("    Total allocations: {}", stats.total_allocations);
    println!("    Active checkpoints: {}", stats.active_checkpoints);
    println!("    Current checkpoint ID: {}", stats.current_checkpoint_id);
    println!("    Corrupted allocations: {}", stats.corrupted_allocations);

    // Validate all allocations
    let validation_result = arena.validate_all_allocations();
    match validation_result {
        Ok(()) => println!("  ✅ All allocations are valid"),
        Err(e) => println!("  ❌ Validation failed: {}", e),
    }

    // Rewind and check stats
    unsafe {
        arena.rewind_to_checkpoint(checkpoint1);
    }

    let stats_after_rewind = arena.debug_stats();
    println!("  After rewind:");
    println!(
        "    Total allocations: {}",
        stats_after_rewind.total_allocations
    );
    println!(
        "    Active checkpoints: {}",
        stats_after_rewind.active_checkpoints
    );
    println!(
        "    Current checkpoint ID: {}",
        stats_after_rewind.current_checkpoint_id
    );
    println!(
        "    Corrupted allocations: {}",
        stats_after_rewind.corrupted_allocations
    );

    println!();
}

#[cfg(not(feature = "debug"))]
fn main() {
    println!("❌ This example requires the 'debug' feature to be enabled.");
    println!("   Run with: cargo run --example debug_safety --features debug");
}
