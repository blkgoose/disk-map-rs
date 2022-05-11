# DiskMap
---
Concurrent shared HashMap based on disk.

To allow concurred shared access to data every key is behind a
`RwLock` handled by the filesystem.

### Usage
#### simple example
```rust
fn main() {
    let d: DiskMap<String, i32> = DiskMap::open_new("/tmp/db").unwrap();

    d.insert("a".to_owned(), 12000).unwrap();
    d.insert("b".to_owned(), 2).unwrap();
    d.insert("c".to_owned(), 3).unwrap();

    d.alter(&"a".to_owned(), |_| 1).unwrap();

    let v = d.get(&"a".to_owned()).unwrap();

    assert_eq!(v, 1);
}
```

#### complex example
```rust
fn main() {
    let d: DiskMap<String, i32> = DiskMap::open_new("/tmp/db").unwrap();

    d.insert("a".to_owned(), 1).unwrap();
    d.insert("b".to_owned(), 2).unwrap();
    d.insert("c".to_owned(), 3).unwrap();

    let d1 = d.clone();
    thread::spawn(move || loop {
        let keys = d1.get_keys().unwrap();

        keys.iter().for_each(|e_ref| {
            d1.alter(e_ref, |_| 11).unwrap();
        });
    });

    let d2 = d.clone();
    thread::spawn(move || loop {
        let keys = d2.get_keys().unwrap();

        keys.iter().for_each(|e_ref| {
            d2.alter(e_ref, |_| 11).unwrap();
        });
    });

    // the two threads are accessing the same key and modifying the value without 
    // one colliding with the other
}
```
