//! Tests for v0.8.0 features: LockFreePool, LockFreeAllocator, ThreadSlab

#[cfg(feature = "lockfree")]
mod lockfree_pool_tests {
    use arena_b::{LockFreeAllocator, LockFreePool, LockFreeStats, ThreadSlab};

    #[test]
    fn test_lockfree_pool_basic() {
        let pool: LockFreePool<u32> = LockFreePool::new();

        // Pool starts empty
        assert!(pool.try_alloc().is_none());

        // Add items to pool
        pool.dealloc(42);
        pool.dealloc(100);
        pool.dealloc(200);

        // Retrieve items (LIFO order)
        assert_eq!(pool.try_alloc(), Some(200));
        assert_eq!(pool.try_alloc(), Some(100));
        assert_eq!(pool.try_alloc(), Some(42));

        // Pool is empty again
        assert!(pool.try_alloc().is_none());
    }

    #[test]
    fn test_lockfree_pool_stats() {
        let pool: LockFreePool<i32> = LockFreePool::new();

        // Initial stats
        let (allocs, hits, _misses, _contention) = pool.stats();
        assert_eq!(allocs, 0);
        assert_eq!(hits, 0);

        // Miss on empty pool
        let _ = pool.try_alloc();
        let (_, _, misses, _) = pool.stats();
        assert_eq!(misses, 1);

        // Add and retrieve
        pool.dealloc(42);
        let _ = pool.try_alloc();
        let (_, hits, _, _) = pool.stats();
        assert_eq!(hits, 1);
    }

    #[test]
    fn test_lockfree_pool_with_struct() {
        #[derive(Debug, PartialEq)]
        struct TestStruct {
            x: i32,
            y: String,
        }

        let pool: LockFreePool<TestStruct> = LockFreePool::new();

        pool.dealloc(TestStruct {
            x: 1,
            y: "hello".to_string(),
        });
        pool.dealloc(TestStruct {
            x: 2,
            y: "world".to_string(),
        });

        let item = pool.try_alloc().unwrap();
        assert_eq!(item.x, 2);
        assert_eq!(item.y, "world");
    }

    #[test]
    fn test_lockfree_allocator_enable_disable() {
        let mut allocator = LockFreeAllocator::new();

        // Starts enabled
        assert!(allocator.is_enabled());

        // Can allocate when enabled
        let ptr = allocator.try_alloc(64, 8);
        assert!(ptr.is_some());

        // Disable
        allocator.disable();
        assert!(!allocator.is_enabled());

        // Cannot allocate when disabled
        let ptr = allocator.try_alloc(64, 8);
        assert!(ptr.is_none());

        // Re-enable
        allocator.enable();
        assert!(allocator.is_enabled());
    }

    #[test]
    fn test_lockfree_allocator_size_limit() {
        let allocator = LockFreeAllocator::new();

        // Small allocations work
        assert!(allocator.try_alloc(512, 8).is_some());

        // Large allocations (>1024) are rejected
        assert!(allocator.try_alloc(2048, 8).is_none());
    }

    #[test]
    fn test_lockfree_allocator_cache_hit_rate() {
        let allocator = LockFreeAllocator::new();

        // Initial rate is 0.0 (no operations)
        assert_eq!(allocator.cache_hit_rate(), 0.0);
    }

    #[test]
    fn test_lockfree_stats_clone() {
        let stats = LockFreeStats::new();
        stats.record_allocation();
        stats.record_cache_hit();

        let cloned = stats.clone();
        let (allocs, hits, _, _) = cloned.get_stats();
        assert_eq!(allocs, 1);
        assert_eq!(hits, 1);
    }

    #[test]
    fn test_lockfree_stats_cache_hit_rate() {
        let stats = LockFreeStats::new();

        // No operations = 0.0
        assert_eq!(stats.cache_hit_rate(), 0.0);

        // 2 hits, 2 misses = 0.5
        stats.record_cache_hit();
        stats.record_cache_hit();
        stats.record_cache_miss();
        stats.record_cache_miss();
        assert_eq!(stats.cache_hit_rate(), 0.5);
    }

    #[test]
    fn test_thread_slab_basic() {
        let mut slab = ThreadSlab::new();

        // New slab has no remaining capacity
        assert_eq!(slab.remaining(), 0);

        // Cannot allocate from empty slab
        assert!(slab.try_alloc(64, 8).is_none());
    }

    #[test]
    fn test_thread_slab_default() {
        let slab = ThreadSlab::default();
        assert_eq!(slab.remaining(), 0);
    }

    #[test]
    fn test_lockfree_pool_default() {
        let pool: LockFreePool<u32> = LockFreePool::default();
        assert!(pool.try_alloc().is_none());
    }

    #[test]
    fn test_lockfree_allocator_default() {
        let allocator = LockFreeAllocator::default();
        assert!(allocator.is_enabled());
    }

    #[test]
    fn test_lockfree_stats_default() {
        let stats = LockFreeStats::default();
        let (allocs, hits, misses, contention) = stats.get_stats();
        assert_eq!(allocs, 0);
        assert_eq!(hits, 0);
        assert_eq!(misses, 0);
        assert_eq!(contention, 0);
    }
}

#[cfg(feature = "debug")]
mod debug_stats_tests {
    use arena_b::Arena;

    #[test]
    fn test_debug_stats_has_leak_reports() {
        let arena = Arena::new();
        let _ = arena.alloc(42u32);

        let stats = arena.debug_stats();
        // leak_reports field exists and is initialized
        assert_eq!(stats.leak_reports, 0);
    }
}

#[cfg(feature = "thread_local")]
mod thread_local_tests {
    use arena_b::{clear_thread_cache, reset_thread_cache};

    #[test]
    fn test_thread_cache_functions_exist() {
        // These should compile and not panic
        reset_thread_cache();
        clear_thread_cache();
    }
}
