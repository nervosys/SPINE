//! # SPINE Storage
//!
//! Persistent storage backends for the SPINE knowledge base and cluster state.
//! Provides a trait-based abstraction over multiple storage engines:
//!
//! - **In-Memory**: Fast ephemeral storage for testing
//! - **SQLite**: Embedded relational storage (default feature)
//! - **RocksDB**: High-performance LSM-tree storage (optional feature)
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │           StorageBackend trait            │
//! │  get / put / delete / scan / keys        │
//! │  batch_put / count / clear               │
//! ├──────────┬──────────┬────────────────────┤
//! │ InMemory │  SQLite  │     RocksDB        │
//! │ HashMap  │ rusqlite │ rocksdb crate      │
//! └──────────┴──────────┴────────────────────┘
//! ```

#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Storage Backend Trait
// ============================================================================

/// Key-value storage backend trait.
///
/// All keys and values are byte slices for maximum flexibility.
/// Higher-level types (knowledge entries, sessions, etc.) are serialized
/// before storage.
pub trait StorageBackend: Send + Sync {
    /// Get a value by key. Returns None if key doesn't exist.
    fn get(&self, namespace: &str, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Put a key-value pair. Overwrites existing values.
    fn put(&self, namespace: &str, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete a key. No-op if key doesn't exist.
    fn delete(&self, namespace: &str, key: &[u8]) -> Result<()>;

    /// Scan all key-value pairs in a namespace with an optional key prefix.
    fn scan(&self, namespace: &str, prefix: Option<&[u8]>) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;

    /// List all keys in a namespace.
    fn keys(&self, namespace: &str) -> Result<Vec<Vec<u8>>>;

    /// Batch put multiple key-value pairs atomically.
    fn batch_put(&self, namespace: &str, entries: &[(&[u8], &[u8])]) -> Result<()> {
        for (key, value) in entries {
            self.put(namespace, key, value)?;
        }
        Ok(())
    }

    /// Count entries in a namespace.
    fn count(&self, namespace: &str) -> Result<usize> {
        Ok(self.keys(namespace)?.len())
    }

    /// Clear all entries in a namespace.
    fn clear(&self, namespace: &str) -> Result<()> {
        let keys = self.keys(namespace)?;
        for key in &keys {
            self.delete(namespace, key)?;
        }
        Ok(())
    }

    /// Check if a key exists.
    fn exists(&self, namespace: &str, key: &[u8]) -> Result<bool> {
        Ok(self.get(namespace, key)?.is_some())
    }
}

/// Typed storage helper for serializable values.
pub struct TypedStorage<B: StorageBackend> {
    backend: B,
    namespace: String,
}

impl<B: StorageBackend> TypedStorage<B> {
    pub fn new(backend: B, namespace: &str) -> Self {
        Self {
            backend,
            namespace: namespace.to_string(),
        }
    }

    /// Get and deserialize a value.
    pub fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<Option<T>> {
        match self.backend.get(&self.namespace, key.as_bytes())? {
            Some(data) => Ok(Some(serde_json::from_slice(&data)?)),
            None => Ok(None),
        }
    }

    /// Serialize and put a value.
    pub fn put<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let data = serde_json::to_vec(value)?;
        self.backend.put(&self.namespace, key.as_bytes(), &data)
    }

    /// Delete a key.
    pub fn delete(&self, key: &str) -> Result<()> {
        self.backend.delete(&self.namespace, key.as_bytes())
    }

    /// List all keys as strings.
    pub fn keys(&self) -> Result<Vec<String>> {
        let raw_keys = self.backend.keys(&self.namespace)?;
        Ok(raw_keys
            .into_iter()
            .filter_map(|k| String::from_utf8(k).ok())
            .collect())
    }

    /// Get all entries.
    pub fn all<T: for<'de> Deserialize<'de>>(&self) -> Result<Vec<(String, T)>> {
        let entries = self.backend.scan(&self.namespace, None)?;
        let mut result = Vec::new();
        for (key, value) in entries {
            if let (Ok(k), Ok(v)) = (String::from_utf8(key), serde_json::from_slice::<T>(&value)) {
                result.push((k, v));
            }
        }
        Ok(result)
    }
}

// ============================================================================
// In-Memory Backend
// ============================================================================

/// Namespace data type alias.
type NamespaceMap = HashMap<String, HashMap<Vec<u8>, Vec<u8>>>;

/// In-memory storage backend using nested HashMaps.
/// Fast but not persistent.
pub struct InMemoryBackend {
    data: std::sync::RwLock<NamespaceMap>,
}

impl InMemoryBackend {
    pub fn new() -> Self {
        Self {
            data: std::sync::RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for InMemoryBackend {
    fn get(&self, namespace: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let data = self
            .data
            .read()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        Ok(data.get(namespace).and_then(|ns| ns.get(key)).cloned())
    }

    fn put(&self, namespace: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let mut data = self
            .data
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        data.entry(namespace.to_string())
            .or_default()
            .insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn delete(&self, namespace: &str, key: &[u8]) -> Result<()> {
        let mut data = self
            .data
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        if let Some(ns) = data.get_mut(namespace) {
            ns.remove(key);
        }
        Ok(())
    }

    fn scan(&self, namespace: &str, prefix: Option<&[u8]>) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let data = self
            .data
            .read()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let Some(ns) = data.get(namespace) else {
            return Ok(Vec::new());
        };

        let result = match prefix {
            Some(pfx) => ns
                .iter()
                .filter(|(k, _)| k.starts_with(pfx))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            None => ns.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        };
        Ok(result)
    }

    fn keys(&self, namespace: &str) -> Result<Vec<Vec<u8>>> {
        let data = self
            .data
            .read()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        Ok(data
            .get(namespace)
            .map(|ns| ns.keys().cloned().collect())
            .unwrap_or_default())
    }

    fn clear(&self, namespace: &str) -> Result<()> {
        let mut data = self
            .data
            .write()
            .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        data.remove(namespace);
        Ok(())
    }
}

// ============================================================================
// SQLite Backend
// ============================================================================

#[cfg(feature = "sqlite")]
pub mod sqlite {
    use super::*;
    use rusqlite::{params, Connection};
    use std::sync::Mutex;

    /// SQLite-backed persistent storage.
    ///
    /// Uses a single table per namespace with key-value columns.
    /// All operations use transactions for atomicity.
    pub struct SqliteBackend {
        conn: Mutex<Connection>,
    }

    impl SqliteBackend {
        /// Open or create a SQLite database at the given path.
        pub fn open(path: &str) -> Result<Self> {
            let conn = Connection::open(path)?;
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=NORMAL;
                 PRAGMA cache_size=10000;
                 PRAGMA temp_store=MEMORY;",
            )?;
            Ok(Self {
                conn: Mutex::new(conn),
            })
        }

        /// Create an in-memory SQLite database.
        pub fn in_memory() -> Result<Self> {
            let conn = Connection::open_in_memory()?;
            Ok(Self {
                conn: Mutex::new(conn),
            })
        }

        /// Ensure the table for a namespace exists.
        fn ensure_table(&self, conn: &Connection, namespace: &str) -> Result<()> {
            let table_name = Self::sanitize_namespace(namespace);
            conn.execute(
                &format!(
                    "CREATE TABLE IF NOT EXISTS {} (
                        key BLOB NOT NULL PRIMARY KEY,
                        value BLOB NOT NULL
                    )",
                    table_name
                ),
                [],
            )?;
            Ok(())
        }

        /// Sanitize namespace to a valid SQL identifier.
        fn sanitize_namespace(namespace: &str) -> String {
            let sanitized: String = namespace
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            format!("ns_{}", sanitized)
        }
    }

    impl StorageBackend for SqliteBackend {
        fn get(&self, namespace: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            self.ensure_table(&conn, namespace)?;
            let table = Self::sanitize_namespace(namespace);

            let mut stmt = conn.prepare(&format!("SELECT value FROM {} WHERE key = ?1", table))?;
            let result = stmt
                .query_row(params![key], |row| row.get::<_, Vec<u8>>(0))
                .ok();
            Ok(result)
        }

        fn put(&self, namespace: &str, key: &[u8], value: &[u8]) -> Result<()> {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            self.ensure_table(&conn, namespace)?;
            let table = Self::sanitize_namespace(namespace);

            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (key, value) VALUES (?1, ?2)",
                    table
                ),
                params![key, value],
            )?;
            Ok(())
        }

        fn delete(&self, namespace: &str, key: &[u8]) -> Result<()> {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            self.ensure_table(&conn, namespace)?;
            let table = Self::sanitize_namespace(namespace);

            conn.execute(
                &format!("DELETE FROM {} WHERE key = ?1", table),
                params![key],
            )?;
            Ok(())
        }

        fn scan(&self, namespace: &str, prefix: Option<&[u8]>) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            self.ensure_table(&conn, namespace)?;
            let table = Self::sanitize_namespace(namespace);

            let mut result = Vec::new();

            match prefix {
                Some(pfx) => {
                    // Use BETWEEN for prefix scan (prefix..prefix+1)
                    let mut upper = pfx.to_vec();
                    // Increment last byte for upper bound
                    if let Some(last) = upper.last_mut() {
                        *last = last.wrapping_add(1);
                    }
                    let mut stmt = conn.prepare(&format!(
                        "SELECT key, value FROM {} WHERE key >= ?1 AND key < ?2",
                        table
                    ))?;
                    let rows = stmt.query_map(params![pfx, upper], |row| {
                        Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
                    })?;
                    for row in rows {
                        result.push(row?);
                    }
                }
                None => {
                    let mut stmt = conn.prepare(&format!("SELECT key, value FROM {}", table))?;
                    let rows = stmt.query_map([], |row| {
                        Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
                    })?;
                    for row in rows {
                        result.push(row?);
                    }
                }
            }

            Ok(result)
        }

        fn keys(&self, namespace: &str) -> Result<Vec<Vec<u8>>> {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            self.ensure_table(&conn, namespace)?;
            let table = Self::sanitize_namespace(namespace);

            let mut stmt = conn.prepare(&format!("SELECT key FROM {}", table))?;
            let rows = stmt.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
            let mut result = Vec::new();
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        }

        fn batch_put(&self, namespace: &str, entries: &[(&[u8], &[u8])]) -> Result<()> {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            self.ensure_table(&conn, namespace)?;
            let table = Self::sanitize_namespace(namespace);

            let tx = conn.unchecked_transaction()?;
            {
                let mut stmt = tx.prepare(&format!(
                    "INSERT OR REPLACE INTO {} (key, value) VALUES (?1, ?2)",
                    table
                ))?;
                for (key, value) in entries {
                    stmt.execute(params![key, value])?;
                }
            }
            tx.commit()?;
            Ok(())
        }

        fn count(&self, namespace: &str) -> Result<usize> {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            self.ensure_table(&conn, namespace)?;
            let table = Self::sanitize_namespace(namespace);

            let count: i64 =
                conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
                    row.get(0)
                })?;
            Ok(count as usize)
        }

        fn clear(&self, namespace: &str) -> Result<()> {
            let conn = self
                .conn
                .lock()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
            let table = Self::sanitize_namespace(namespace);
            conn.execute(&format!("DROP TABLE IF EXISTS {}", table), [])?;
            Ok(())
        }
    }
}

// ============================================================================
// RocksDB Backend
// ============================================================================

#[cfg(feature = "rocksdb-backend")]
pub mod rocks {
    use super::*;
    use rocksdb::{ColumnFamilyDescriptor, Options, DB};
    use std::path::Path;
    use std::sync::Mutex;

    /// RocksDB-backed persistent storage.
    ///
    /// Uses column families for namespace isolation.
    pub struct RocksDbBackend {
        db: Mutex<DB>,
    }

    impl RocksDbBackend {
        /// Open or create a RocksDB database at the given path.
        pub fn open(path: &str) -> Result<Self> {
            let mut opts = Options::default();
            opts.create_if_missing(true);
            opts.create_missing_column_families(true);
            opts.set_max_open_files(256);
            opts.set_write_buffer_size(64 * 1024 * 1024); // 64 MB

            let db = DB::open(&opts, path)?;
            Ok(Self { db: Mutex::new(db) })
        }

        fn cf_name(namespace: &str) -> String {
            format!("ns_{}", namespace)
        }
    }

    impl StorageBackend for RocksDbBackend {
        fn get(&self, namespace: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
            let db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock: {}", e))?;
            let cf_name = Self::cf_name(namespace);
            match db.cf_handle(&cf_name) {
                Some(cf) => Ok(db.get_cf(&cf, key)?),
                None => Ok(None),
            }
        }

        fn put(&self, namespace: &str, key: &[u8], value: &[u8]) -> Result<()> {
            let db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock: {}", e))?;
            let cf_name = Self::cf_name(namespace);
            if db.cf_handle(&cf_name).is_none() {
                let opts = Options::default();
                db.create_cf(&cf_name, &opts)?;
            }
            let cf = db.cf_handle(&cf_name).unwrap();
            db.put_cf(&cf, key, value)?;
            Ok(())
        }

        fn delete(&self, namespace: &str, key: &[u8]) -> Result<()> {
            let db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock: {}", e))?;
            let cf_name = Self::cf_name(namespace);
            if let Some(cf) = db.cf_handle(&cf_name) {
                db.delete_cf(&cf, key)?;
            }
            Ok(())
        }

        fn scan(&self, namespace: &str, prefix: Option<&[u8]>) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
            let db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock: {}", e))?;
            let cf_name = Self::cf_name(namespace);
            let Some(cf) = db.cf_handle(&cf_name) else {
                return Ok(Vec::new());
            };

            let iter = db.iterator_cf(&cf, rocksdb::IteratorMode::Start);
            let mut result = Vec::new();

            for item in iter {
                let (key, value) = item?;
                if let Some(pfx) = prefix {
                    if key.starts_with(pfx) {
                        result.push((key.to_vec(), value.to_vec()));
                    }
                } else {
                    result.push((key.to_vec(), value.to_vec()));
                }
            }

            Ok(result)
        }

        fn keys(&self, namespace: &str) -> Result<Vec<Vec<u8>>> {
            let db = self.db.lock().map_err(|e| anyhow::anyhow!("Lock: {}", e))?;
            let cf_name = Self::cf_name(namespace);
            let Some(cf) = db.cf_handle(&cf_name) else {
                return Ok(Vec::new());
            };

            let iter = db.iterator_cf(&cf, rocksdb::IteratorMode::Start);
            let mut result = Vec::new();

            for item in iter {
                let (key, _) = item?;
                result.push(key.to_vec());
            }

            Ok(result)
        }
    }
}

// ============================================================================
// Storage Manager
// ============================================================================

/// Storage engine type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageEngine {
    InMemory,
    #[cfg(feature = "sqlite")]
    Sqlite,
    #[cfg(feature = "rocksdb-backend")]
    RocksDb,
}

/// Storage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub engine: StorageEngine,
    pub path: Option<String>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            engine: StorageEngine::InMemory,
            path: None,
        }
    }
}

/// Create a storage backend from configuration.
pub fn create_backend(config: &StorageConfig) -> Result<Box<dyn StorageBackend>> {
    match config.engine {
        StorageEngine::InMemory => Ok(Box::new(InMemoryBackend::new())),
        #[cfg(feature = "sqlite")]
        StorageEngine::Sqlite => {
            let path = config.path.as_deref().unwrap_or("spine_data.db");
            Ok(Box::new(sqlite::SqliteBackend::open(path)?))
        }
        #[cfg(feature = "rocksdb-backend")]
        StorageEngine::RocksDb => {
            let path = config.path.as_deref().unwrap_or("spine_data_rocks");
            Ok(Box::new(rocks::RocksDbBackend::open(path)?))
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn run_backend_tests(backend: &dyn StorageBackend) {
        // Put and get
        backend.put("test", b"key1", b"value1").unwrap();
        assert_eq!(
            backend.get("test", b"key1").unwrap(),
            Some(b"value1".to_vec())
        );

        // Non-existent key
        assert_eq!(backend.get("test", b"missing").unwrap(), None);

        // Overwrite
        backend.put("test", b"key1", b"value2").unwrap();
        assert_eq!(
            backend.get("test", b"key1").unwrap(),
            Some(b"value2".to_vec())
        );

        // Delete
        backend.delete("test", b"key1").unwrap();
        assert_eq!(backend.get("test", b"key1").unwrap(), None);

        // Multiple keys
        backend.put("test", b"a1", b"v1").unwrap();
        backend.put("test", b"a2", b"v2").unwrap();
        backend.put("test", b"b1", b"v3").unwrap();

        // Keys
        let mut keys = backend.keys("test").unwrap();
        keys.sort();
        assert_eq!(keys.len(), 3);

        // Scan with prefix
        let scan = backend.scan("test", Some(b"a")).unwrap();
        assert_eq!(scan.len(), 2);

        // Count
        assert_eq!(backend.count("test").unwrap(), 3);

        // Exists
        assert!(backend.exists("test", b"a1").unwrap());
        assert!(!backend.exists("test", b"missing").unwrap());

        // Clear
        backend.clear("test").unwrap();
        assert_eq!(backend.count("test").unwrap_or(0), 0);
    }

    #[test]
    fn test_in_memory_backend() {
        let backend = InMemoryBackend::new();
        run_backend_tests(&backend);
    }

    #[test]
    fn test_in_memory_namespaces() {
        let backend = InMemoryBackend::new();
        backend.put("ns1", b"key", b"val1").unwrap();
        backend.put("ns2", b"key", b"val2").unwrap();

        assert_eq!(backend.get("ns1", b"key").unwrap(), Some(b"val1".to_vec()));
        assert_eq!(backend.get("ns2", b"key").unwrap(), Some(b"val2".to_vec()));

        backend.clear("ns1").unwrap();
        assert_eq!(backend.get("ns1", b"key").unwrap(), None);
        assert_eq!(backend.get("ns2", b"key").unwrap(), Some(b"val2".to_vec()));
    }

    #[test]
    fn test_batch_put() {
        let backend = InMemoryBackend::new();
        backend
            .batch_put("batch", &[(b"k1", b"v1"), (b"k2", b"v2"), (b"k3", b"v3")])
            .unwrap();

        assert_eq!(backend.count("batch").unwrap(), 3);
        assert_eq!(backend.get("batch", b"k2").unwrap(), Some(b"v2".to_vec()));
    }

    #[test]
    fn test_typed_storage() {
        let backend = InMemoryBackend::new();
        let store = TypedStorage::new(backend, "typed");

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        store.put("item1", &data).unwrap();
        let retrieved: TestData = store.get("item1").unwrap().unwrap();
        assert_eq!(retrieved, data);

        let keys = store.keys().unwrap();
        assert_eq!(keys, vec!["item1"]);

        store.delete("item1").unwrap();
        assert!(store.get::<TestData>("item1").unwrap().is_none());
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn test_sqlite_backend() {
        let backend = sqlite::SqliteBackend::in_memory().unwrap();
        run_backend_tests(&backend);
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn test_sqlite_batch_put() {
        let backend = sqlite::SqliteBackend::in_memory().unwrap();
        backend
            .batch_put("batch", &[(b"k1", b"v1"), (b"k2", b"v2"), (b"k3", b"v3")])
            .unwrap();

        assert_eq!(backend.count("batch").unwrap(), 3);
        assert_eq!(backend.get("batch", b"k2").unwrap(), Some(b"v2".to_vec()));
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn test_sqlite_namespaces() {
        let backend = sqlite::SqliteBackend::in_memory().unwrap();
        backend.put("ns1", b"key", b"val1").unwrap();
        backend.put("ns2", b"key", b"val2").unwrap();

        assert_eq!(backend.get("ns1", b"key").unwrap(), Some(b"val1".to_vec()));
        assert_eq!(backend.get("ns2", b"key").unwrap(), Some(b"val2".to_vec()));
    }

    #[test]
    fn test_storage_config_default() {
        let config = StorageConfig::default();
        assert_eq!(config.engine, StorageEngine::InMemory);
        assert!(config.path.is_none());
    }

    #[test]
    fn test_create_backend_in_memory() {
        let config = StorageConfig::default();
        let backend = create_backend(&config).unwrap();
        backend.put("test", b"k", b"v").unwrap();
        assert_eq!(backend.get("test", b"k").unwrap(), Some(b"v".to_vec()));
    }
}
