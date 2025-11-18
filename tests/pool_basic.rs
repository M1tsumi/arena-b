use arena_b::{Pool, PoolStats};

#[test]
fn pool_alloc_basic() {
    let pool: Pool<u32> = Pool::new();
    let p = pool.alloc(1_u32);
    assert_eq!(*p, 1);
}

#[test]
fn pooled_returns_to_pool_on_drop() {
    let pool: Pool<u32> = Pool::with_capacity(1);
    let stats_before: PoolStats = pool.stats();
    assert_eq!(stats_before.capacity, 1);
    assert_eq!(stats_before.in_use, 0);
    assert_eq!(stats_before.free, 1);

    {
        let _p = pool.alloc(42_u32);
        let stats_during: PoolStats = pool.stats();
        assert_eq!(stats_during.capacity, 1);
        assert_eq!(stats_during.in_use, 1);
        assert_eq!(stats_during.free, 0);
    }

    let stats_after: PoolStats = pool.stats();
    assert_eq!(stats_after.capacity, 1);
    assert_eq!(stats_after.in_use, 0);
    assert_eq!(stats_after.free, 1);
}

#[test]
fn pool_reuses_slots() {
    let pool: Pool<u32> = Pool::with_capacity(1);
    let p1 = pool.alloc(1_u32);
    let addr1 = &*p1 as *const u32 as usize;
    drop(p1);

    let p2 = pool.alloc(2_u32);
    let addr2 = &*p2 as *const u32 as usize;

    assert_eq!(addr1, addr2);
    assert_eq!(*p2, 2);
}
