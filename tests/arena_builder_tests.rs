#[cfg(test)]
#[cfg(feature = "arena_module")]
mod tests {
    use arena_b::ArenaBuilder;

    #[test]
    fn test_arena_builder_basic() {
        let arena = ArenaBuilder::new()
            .initial_capacity(1024)
            .chunk_size(512)
            .build();

        let stats = arena.stats();
        assert_eq!(stats.chunk_count, 1);
        assert!(stats.bytes_allocated >= 1024);
    }

    #[test]
    fn test_arena_builder_feature_bundles() {
        // Test perf bundle
        let arena = ArenaBuilder::new().perf_bundle().build();

        let status = arena.feature_status();
        assert_eq!(status.stats, cfg!(feature = "stats"));

        // Test safety bundle
        let arena = ArenaBuilder::new().safety_bundle().build();

        let status = arena.feature_status();
        assert_eq!(status.debug, cfg!(feature = "debug"));
        assert_eq!(status.stats, cfg!(feature = "stats"));
    }

    #[test]
    fn test_arena_builder_diagnostics() {
        use std::sync::Arc;
        use std::sync::Mutex;

        let messages = Arc::new(Mutex::new(Vec::new()));
        let messages_clone = messages.clone();

        let arena = ArenaBuilder::new()
            .diagnostics_sink(move |msg| {
                messages_clone.lock().unwrap().push(msg.to_string());
            })
            .initial_capacity(2048)
            .build();
        // Use the arena variable to avoid unused warning
        let _ = arena.stats();

        // Should have logged the build configuration
        let msgs = messages.lock().unwrap();
        assert!(!msgs.is_empty());
        assert!(msgs[0].contains("capacity"));
    }

    #[test]
    fn test_arena_builder_individual_features() {
        let arena = ArenaBuilder::new()
            .enable_stats(true)
            .enable_debug(true)
            .build();

        let config = arena.build_config();
        // These should match the compiled feature flags
        assert_eq!(config.features.stats, cfg!(feature = "stats"));
        assert_eq!(config.features.debug, cfg!(feature = "debug"));

        // Test that the arena was created successfully
        let stats = arena.stats();
        assert_eq!(stats.chunk_count, 1);
    }

    #[test]
    fn test_reset_and_shrink_to_fit() {
        let arena = ArenaBuilder::new().initial_capacity(1024).build();

        // Allocate enough to trigger multiple chunks (bigger than lockfree max)
        for _ in 0..10 {
            let _data = arena.alloc([0u8; 2000]);
        }

        let stats_before = arena.stats();
        assert!(stats_before.chunk_count >= 2);

        // Reset and shrink
        arena.reset_and_shrink_to_fit();

        let stats_after = arena.stats();
        assert_eq!(stats_after.chunk_count, 1);
        assert_eq!(stats_after.bytes_used, 0);
        assert_eq!(stats_after.allocation_count, 0);
    }
}
