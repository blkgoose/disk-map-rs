# DiskMap
---
Concurrent shared HashMap based on disk.

### usage
```rust
fn alter() {
    let d: DiskMap<String, i32> = DiskMap::open_new("/tmp/db").unwrap();

    d.insert("a".to_owned(), 12000).unwrap();
    d.insert("b".to_owned(), 2).unwrap();
    d.insert("c".to_owned(), 3).unwrap();

    d.alter(&"a".to_owned(), |_| 1).unwrap();

    let v = d.get(&"a".to_owned()).unwrap();

    assert_eq!(v, 1);
}
```
