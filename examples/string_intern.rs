use arena_b::Arena;
use std::collections::HashSet;

fn main() {
    let arena = Arena::with_capacity(64 * 1024);
    let mut interns: HashSet<&str> = HashSet::new();

    let words = ["apple", "banana", "apple", "orange", "banana", "pear"];

    for w in &words {
        let interned = match interns.get::<str>(w) {
            Some(&s) => s,
            None => {
                let s = arena.alloc_str(w);
                interns.insert(s);
                s
            }
        };
        println!("interned {:p}: {}", interned, interned);
    }

    println!("unique interned strings: {}", interns.len());
}
