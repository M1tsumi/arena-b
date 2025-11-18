use arena_b::{Pool, PoolStats, Pooled};

#[derive(Debug)]
struct Node {
    value: u32,
    neighbors: Vec<usize>, // indices into `nodes` Vec
}

fn bfs(start: usize, nodes: &[Pooled<Node>]) -> Vec<u32> {
    let mut visited = vec![false; nodes.len()];
    let mut order = Vec::new();
    let mut queue = std::collections::VecDeque::new();

    visited[start] = true;
    queue.push_back(start);

    while let Some(idx) = queue.pop_front() {
        let node = &nodes[idx];
        order.push(node.value);
        for &n in &node.neighbors {
            if !visited[n] {
                visited[n] = true;
                queue.push_back(n);
            }
        }
    }

    order
}

fn main() {
    let pool: Pool<Node> = Pool::with_capacity(8);
    let mut nodes: Vec<Pooled<Node>> = Vec::new();

    let a = pool.alloc(Node {
        value: 1,
        neighbors: Vec::new(),
    });
    let b = pool.alloc(Node {
        value: 2,
        neighbors: Vec::new(),
    });
    let c = pool.alloc(Node {
        value: 3,
        neighbors: Vec::new(),
    });

    nodes.push(a);
    nodes.push(b);
    nodes.push(c);

    nodes[0].neighbors.push(1);
    nodes[1].neighbors.push(2);

    let order = bfs(0, &nodes);
    println!("BFS visit order: {:?}", order);

    let stats: PoolStats = pool.stats();
    println!(
        "Pool stats: capacity={}, in_use={}, free={}",
        stats.capacity, stats.in_use, stats.free
    );
}
