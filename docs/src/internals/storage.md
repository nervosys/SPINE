# Persistent Storage

The `spine-storage` crate provides trait-based persistent storage backends for SPINE's knowledge base and cluster state.

## StorageBackend Trait

```rust
pub trait StorageBackend: Send + Sync {
    fn get(&self, namespace: &str, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn put(&self, namespace: &str, key: &[u8], value: &[u8]) -> Result<()>;
    fn delete(&self, namespace: &str, key: &[u8]) -> Result<()>;
    fn scan(&self, namespace: &str, prefix: Option<&[u8]>) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
    fn keys(&self, namespace: &str) -> Result<Vec<Vec<u8>>>;
    fn batch_put(&self, namespace: &str, entries: &[(&[u8], &[u8])]) -> Result<()>;
    fn count(&self, namespace: &str) -> Result<usize>;
    fn clear(&self, namespace: &str) -> Result<()>;
}
```

## Backends

| Backend     | Feature     | Use Case                     | Durability |
| ----------- | ----------- | ---------------------------- | ---------- |
| InMemory    | (default)   | Testing, ephemeral workloads | None       |
| SQLite      | `sqlite`    | Single-node production       | WAL        |
| RocksDB     | `rocksdb`   | High-throughput workloads    | LSM-tree   |

## TypedStorage

Generic wrapper providing `serde` serialization over any backend:

```rust
let typed: TypedStorage<MyConfig> = TypedStorage::new(backend, "configs");
typed.put("key1", &my_config)?;
let config: Option<MyConfig> = typed.get("key1")?;
```

## PersistentKnowledge

Adapter integrating `StorageBackend` with `spine-knowledge`:

- Episodes stored in `"episodes"` namespace
- Concepts in `"concepts"` namespace  
- Relations in `"relations"` namespace
- Knowledge entries in `"entries"` namespace
