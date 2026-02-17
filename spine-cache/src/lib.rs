//! # SPINE Cache
//!
//! Tiered caching system for SPINE with three levels:
//!
//! - **L1**: In-memory LRU cache — lowest latency (~10ns)
//! - **L2**: Memory-mapped file cache — medium latency (~1μs)
//! - **L3**: Remote/delegated cache — highest latency (~1ms)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │                 TieredCache                      │
//! │                                                  │
//! │  GET:  L1 → miss → L2 → miss → L3 → miss → None│
//! │  PUT:  Write to all tiers (write-through)        │
//! │                                                  │
//! │  ┌──────┐    ┌──────────┐    ┌───────────┐      │
//! │  │  L1  │    │    L2    │    │    L3     │      │
//! │  │ LRU  │    │  mmap'd  │    │  Remote   │      │
//! │  │ RAM  │    │  files   │    │ delegate  │      │
//! │  └──────┘    └──────────┘    └───────────┘      │
//! └─────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};

// ============================================================================
// Cache Entry
// ============================================================================

/// A cached value with metadata.
#[derive(Debug, Clone)]
struct CacheEntry {
    /// The cached data.
    value: Vec<u8>,
    /// When the entry was inserted.
    inserted_at: Instant,
    /// When the entry was last accessed.
    last_accessed: Instant,
    /// Access count.
    access_count: u64,
    /// Time-to-live (None = infinite).
    ttl: Option<Duration>,
}

impl CacheEntry {
    fn new(value: Vec<u8>, ttl: Option<Duration>) -> Self {
        let now = Instant::now();
        Self {
            value,
            inserted_at: now,
            last_accessed: now,
            access_count: 1,
            ttl,
        }
    }

    fn is_expired(&self) -> bool {
        match self.ttl {
            Some(ttl) => self.inserted_at.elapsed() > ttl,
            None => false,
        }
    }

    fn touch(&mut self) {
        self.last_accessed = Instant::now();
        self.access_count += 1;
    }
}

// ============================================================================
// L1: In-Memory LRU Cache
// ============================================================================

/// In-memory LRU cache with configurable capacity.
pub struct L1Cache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    max_entries: usize,
    max_bytes: usize,
    current_bytes: RwLock<usize>,
}

impl L1Cache {
    /// Create a new L1 cache.
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            max_entries,
            max_bytes,
            current_bytes: RwLock::new(0),
        }
    }

    /// Get a value from the cache.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let mut entries = self.entries.write().ok()?;
        let entry = entries.get_mut(key)?;
        if entry.is_expired() {
            let size = entry.value.len();
            entries.remove(key);
            if let Ok(mut bytes) = self.current_bytes.write() {
                *bytes = bytes.saturating_sub(size);
            }
            return None;
        }
        entry.touch();
        Some(entry.value.clone())
    }

    /// Put a value into the cache.
    pub fn put(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) {
        self.evict_if_needed(value.len());

        let size = value.len();
        let entry = CacheEntry::new(value, ttl);
        let mut entries = self.entries.write().unwrap();
        if let Some(old) = entries.insert(key.to_string(), entry) {
            if let Ok(mut bytes) = self.current_bytes.write() {
                *bytes = bytes.saturating_sub(old.value.len());
            }
        }
        if let Ok(mut bytes) = self.current_bytes.write() {
            *bytes += size;
        }
    }

    /// Remove a value from the cache.
    pub fn remove(&self, key: &str) -> bool {
        let mut entries = self.entries.write().unwrap();
        if let Some(entry) = entries.remove(key) {
            if let Ok(mut bytes) = self.current_bytes.write() {
                *bytes = bytes.saturating_sub(entry.value.len());
            }
            true
        } else {
            false
        }
    }

    /// Evict entries to make room for a new value.
    fn evict_if_needed(&self, needed_bytes: usize) {
        let mut entries = self.entries.write().unwrap();
        let mut bytes = self.current_bytes.write().unwrap();

        // First pass: remove expired entries
        let expired: Vec<String> = entries
            .iter()
            .filter(|(_, e)| e.is_expired())
            .map(|(k, _)| k.clone())
            .collect();
        for key in &expired {
            if let Some(entry) = entries.remove(key) {
                *bytes = bytes.saturating_sub(entry.value.len());
            }
        }

        // Second pass: LRU eviction if still over capacity
        while entries.len() >= self.max_entries || *bytes + needed_bytes > self.max_bytes {
            if entries.is_empty() {
                break;
            }
            // Find least recently used
            let lru_key = entries
                .iter()
                .min_by_key(|(_, e)| e.last_accessed)
                .map(|(k, _)| k.clone());
            if let Some(key) = lru_key {
                if let Some(entry) = entries.remove(&key) {
                    *bytes = bytes.saturating_sub(entry.value.len());
                }
            } else {
                break;
            }
        }
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let entries = self.entries.read().unwrap();
        let bytes = self.current_bytes.read().unwrap();
        CacheStats {
            entry_count: entries.len(),
            byte_count: *bytes,
            max_entries: self.max_entries,
            max_bytes: self.max_bytes,
        }
    }

    /// Clear all entries.
    pub fn clear(&self) {
        let mut entries = self.entries.write().unwrap();
        entries.clear();
        *self.current_bytes.write().unwrap() = 0;
    }
}

// ============================================================================
// L2: File-Backed Cache
// ============================================================================

/// File-backed cache using the filesystem.
/// In production, this would use memory-mapped files for zero-copy access.
pub struct L2Cache {
    cache_dir: PathBuf,
    max_bytes: usize,
}

impl L2Cache {
    /// Create a new L2 cache backed by a directory.
    pub fn new(cache_dir: &Path, max_bytes: usize) -> Result<Self> {
        fs::create_dir_all(cache_dir)?;
        Ok(Self {
            cache_dir: cache_dir.to_path_buf(),
            max_bytes,
        })
    }

    /// Get a value from the file cache.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        let path = self.key_path(key);
        let meta_path = self.meta_path(key);

        // Check TTL from metadata
        if let Ok(meta_data) = fs::read_to_string(&meta_path) {
            if let Ok(meta) = serde_json::from_str::<FileMeta>(&meta_data) {
                if let Some(expires) = meta.expires_at_secs {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    if now > expires {
                        let _ = fs::remove_file(&path);
                        let _ = fs::remove_file(&meta_path);
                        return None;
                    }
                }
            }
        }

        fs::read(&path).ok()
    }

    /// Put a value into the file cache.
    pub fn put(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<()> {
        let path = self.key_path(key);
        let meta_path = self.meta_path(key);

        // Write value
        let mut file = fs::File::create(&path)?;
        file.write_all(value)?;

        // Write metadata
        let expires_at_secs = ttl.map(|d| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + d.as_secs()
        });
        let meta = FileMeta {
            size: value.len(),
            expires_at_secs,
        };
        fs::write(&meta_path, serde_json::to_string(&meta)?)?;

        Ok(())
    }

    /// Remove a value from the file cache.
    pub fn remove(&self, key: &str) -> bool {
        let path = self.key_path(key);
        let meta_path = self.meta_path(key);
        let _ = fs::remove_file(&meta_path);
        fs::remove_file(&path).is_ok()
    }

    /// Clear the entire file cache.
    pub fn clear(&self) -> Result<()> {
        if self.cache_dir.exists() {
            fs::remove_dir_all(&self.cache_dir)?;
            fs::create_dir_all(&self.cache_dir)?;
        }
        Ok(())
    }

    fn key_path(&self, key: &str) -> PathBuf {
        // Use hex-encoded key to avoid filesystem issues
        let hex: String = key.bytes().map(|b| format!("{:02x}", b)).collect();
        self.cache_dir.join(hex)
    }

    fn meta_path(&self, key: &str) -> PathBuf {
        let hex: String = key.bytes().map(|b| format!("{:02x}", b)).collect();
        self.cache_dir.join(format!("{}.meta", hex))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct FileMeta {
    size: usize,
    expires_at_secs: Option<u64>,
}

// ============================================================================
// L3: Remote/Delegated Cache
// ============================================================================

/// Trait for L3 remote cache backends.
pub trait RemoteCache: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn put(&self, key: &str, value: &[u8], ttl: Option<Duration>) -> Result<()>;
    fn remove(&self, key: &str) -> Result<bool>;
}

/// No-op remote cache (disables L3).
pub struct NoopRemoteCache;

impl RemoteCache for NoopRemoteCache {
    fn get(&self, _key: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }

    fn put(&self, _key: &str, _value: &[u8], _ttl: Option<Duration>) -> Result<()> {
        Ok(())
    }

    fn remove(&self, _key: &str) -> Result<bool> {
        Ok(false)
    }
}

// ============================================================================
// Tiered Cache
// ============================================================================

/// Cache statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub entry_count: usize,
    pub byte_count: usize,
    pub max_entries: usize,
    pub max_bytes: usize,
}

/// Tiered cache hit/miss statistics.
#[derive(Debug, Clone, Default)]
pub struct TieredCacheMetrics {
    pub l1_hits: u64,
    pub l1_misses: u64,
    pub l2_hits: u64,
    pub l2_misses: u64,
    pub l3_hits: u64,
    pub l3_misses: u64,
    pub total_gets: u64,
    pub total_puts: u64,
}

/// Configuration for the tiered cache.
#[derive(Debug, Clone)]
pub struct TieredCacheConfig {
    /// L1 maximum entries.
    pub l1_max_entries: usize,
    /// L1 maximum bytes.
    pub l1_max_bytes: usize,
    /// L2 cache directory path.
    pub l2_cache_dir: PathBuf,
    /// L2 maximum bytes.
    pub l2_max_bytes: usize,
    /// Whether to enable L2.
    pub l2_enabled: bool,
    /// Whether to enable L3.
    pub l3_enabled: bool,
    /// Default TTL for cache entries.
    pub default_ttl: Option<Duration>,
    /// Whether to promote L2/L3 hits to L1.
    pub promote_on_hit: bool,
}

impl Default for TieredCacheConfig {
    fn default() -> Self {
        Self {
            l1_max_entries: 10_000,
            l1_max_bytes: 256 * 1024 * 1024, // 256 MB
            l2_cache_dir: PathBuf::from("spine_cache"),
            l2_max_bytes: 1024 * 1024 * 1024, // 1 GB
            l2_enabled: false,
            l3_enabled: false,
            default_ttl: None,
            promote_on_hit: true,
        }
    }
}

/// Tiered cache with L1 (in-memory), L2 (file-backed), L3 (remote).
///
/// Reads check tiers in order: L1 → L2 → L3.
/// Writes go to all enabled tiers (write-through).
/// On L2/L3 hit, the value is promoted to L1 if `promote_on_hit` is true.
pub struct TieredCache {
    l1: L1Cache,
    l2: Option<L2Cache>,
    l3: Option<Box<dyn RemoteCache>>,
    config: TieredCacheConfig,
    metrics: RwLock<TieredCacheMetrics>,
}

impl TieredCache {
    /// Create a new tiered cache with the given configuration.
    pub fn new(config: TieredCacheConfig) -> Result<Self> {
        let l1 = L1Cache::new(config.l1_max_entries, config.l1_max_bytes);
        let l2 = if config.l2_enabled {
            Some(L2Cache::new(&config.l2_cache_dir, config.l2_max_bytes)?)
        } else {
            None
        };

        Ok(Self {
            l1,
            l2,
            l3: None,
            config,
            metrics: RwLock::new(TieredCacheMetrics::default()),
        })
    }

    /// Create a simple in-memory-only cache.
    pub fn in_memory(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            l1: L1Cache::new(max_entries, max_bytes),
            l2: None,
            l3: None,
            config: TieredCacheConfig::default(),
            metrics: RwLock::new(TieredCacheMetrics::default()),
        }
    }

    /// Set the L3 remote cache backend.
    pub fn with_remote(mut self, remote: Box<dyn RemoteCache>) -> Self {
        self.l3 = Some(remote);
        self.config.l3_enabled = true;
        self
    }

    /// Get a value from the cache, checking tiers in order.
    pub fn get(&self, key: &str) -> Option<Vec<u8>> {
        if let Ok(mut m) = self.metrics.write() {
            m.total_gets += 1;
        }

        // L1 check
        if let Some(value) = self.l1.get(key) {
            if let Ok(mut m) = self.metrics.write() {
                m.l1_hits += 1;
            }
            return Some(value);
        }
        if let Ok(mut m) = self.metrics.write() {
            m.l1_misses += 1;
        }

        // L2 check
        if let Some(l2) = &self.l2 {
            if let Some(value) = l2.get(key) {
                if let Ok(mut m) = self.metrics.write() {
                    m.l2_hits += 1;
                }
                if self.config.promote_on_hit {
                    self.l1.put(key, value.clone(), self.config.default_ttl);
                }
                return Some(value);
            }
            if let Ok(mut m) = self.metrics.write() {
                m.l2_misses += 1;
            }
        }

        // L3 check
        if let Some(l3) = &self.l3 {
            if let Ok(Some(value)) = l3.get(key) {
                if let Ok(mut m) = self.metrics.write() {
                    m.l3_hits += 1;
                }
                if self.config.promote_on_hit {
                    self.l1.put(key, value.clone(), self.config.default_ttl);
                    if let Some(l2) = &self.l2 {
                        let _ = l2.put(key, &value, self.config.default_ttl);
                    }
                }
                return Some(value);
            }
            if let Ok(mut m) = self.metrics.write() {
                m.l3_misses += 1;
            }
        }

        None
    }

    /// Put a value into all tiers (write-through).
    pub fn put(&self, key: &str, value: Vec<u8>) {
        self.put_with_ttl(key, value, self.config.default_ttl);
    }

    /// Put a value with a specific TTL.
    pub fn put_with_ttl(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) {
        if let Ok(mut m) = self.metrics.write() {
            m.total_puts += 1;
        }

        // Write to L1
        self.l1.put(key, value.clone(), ttl);

        // Write to L2
        if let Some(l2) = &self.l2 {
            let _ = l2.put(key, &value, ttl);
        }

        // Write to L3
        if let Some(l3) = &self.l3 {
            let _ = l3.put(key, &value, ttl);
        }
    }

    /// Remove a value from all tiers.
    pub fn remove(&self, key: &str) {
        self.l1.remove(key);
        if let Some(l2) = &self.l2 {
            l2.remove(key);
        }
        if let Some(l3) = &self.l3 {
            let _ = l3.remove(key);
        }
    }

    /// Clear all tiers.
    pub fn clear(&self) {
        self.l1.clear();
        if let Some(l2) = &self.l2 {
            let _ = l2.clear();
        }
    }

    /// Get L1 cache statistics.
    pub fn l1_stats(&self) -> CacheStats {
        self.l1.stats()
    }

    /// Get tiered cache metrics.
    pub fn metrics(&self) -> TieredCacheMetrics {
        self.metrics.read().unwrap().clone()
    }

    /// Get cache hit rate.
    pub fn hit_rate(&self) -> f64 {
        let m = self.metrics.read().unwrap();
        if m.total_gets == 0 {
            return 0.0;
        }
        let hits = m.l1_hits + m.l2_hits + m.l3_hits;
        hits as f64 / m.total_gets as f64
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_l1_basic_operations() {
        let cache = L1Cache::new(100, 1024 * 1024);

        cache.put("key1", b"value1".to_vec(), None);
        assert_eq!(cache.get("key1"), Some(b"value1".to_vec()));

        cache.remove("key1");
        assert_eq!(cache.get("key1"), None);
    }

    #[test]
    fn test_l1_ttl_expiration() {
        let cache = L1Cache::new(100, 1024 * 1024);

        cache.put("expire", b"data".to_vec(), Some(Duration::from_millis(50)));
        assert_eq!(cache.get("expire"), Some(b"data".to_vec()));

        thread::sleep(Duration::from_millis(100));
        assert_eq!(cache.get("expire"), None);
    }

    #[test]
    fn test_l1_lru_eviction() {
        let cache = L1Cache::new(3, 1024 * 1024);

        cache.put("a", b"1".to_vec(), None);
        cache.put("b", b"2".to_vec(), None);
        cache.put("c", b"3".to_vec(), None);

        // Access 'a' to make it recently used
        cache.get("a");

        // Adding 'd' should evict 'b' (least recently used)
        cache.put("d", b"4".to_vec(), None);

        assert!(cache.get("a").is_some()); // accessed recently
        assert!(cache.get("c").is_some()); // or 'c'
        assert!(cache.get("d").is_some()); // just added
    }

    #[test]
    fn test_l1_byte_limit() {
        let cache = L1Cache::new(1000, 20); // 20 bytes max

        cache.put("a", vec![0u8; 10], None); // 10 bytes
        cache.put("b", vec![0u8; 10], None); // 10 bytes — at limit

        // This should evict something
        cache.put("c", vec![0u8; 10], None);

        let stats = cache.stats();
        assert!(stats.byte_count <= 20);
    }

    #[test]
    fn test_l1_stats() {
        let cache = L1Cache::new(100, 1024 * 1024);

        cache.put("k1", b"v1".to_vec(), None);
        cache.put("k2", b"v2".to_vec(), None);

        let stats = cache.stats();
        assert_eq!(stats.entry_count, 2);
        assert_eq!(stats.max_entries, 100);
    }

    #[test]
    fn test_l1_clear() {
        let cache = L1Cache::new(100, 1024 * 1024);

        cache.put("k1", b"v1".to_vec(), None);
        cache.put("k2", b"v2".to_vec(), None);

        cache.clear();
        assert_eq!(cache.stats().entry_count, 0);
        assert_eq!(cache.stats().byte_count, 0);
    }

    #[test]
    fn test_l2_basic_operations() {
        let dir = std::env::temp_dir().join("spine_cache_test_l2");
        let _ = fs::remove_dir_all(&dir);
        let cache = L2Cache::new(&dir, 1024 * 1024).unwrap();

        cache.put("key1", b"value1", None).unwrap();
        assert_eq!(cache.get("key1"), Some(b"value1".to_vec()));

        cache.remove("key1");
        assert_eq!(cache.get("key1"), None);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_l2_ttl() {
        let dir = std::env::temp_dir().join("spine_cache_test_l2_ttl");
        let _ = fs::remove_dir_all(&dir);
        let cache = L2Cache::new(&dir, 1024 * 1024).unwrap();

        // Set a long TTL to verify it doesn't expire
        cache
            .put("persist", b"data", Some(Duration::from_secs(3600)))
            .unwrap();
        assert!(cache.get("persist").is_some());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tiered_cache_l1_only() {
        let cache = TieredCache::in_memory(100, 1024 * 1024);

        cache.put("k1", b"v1".to_vec());
        assert_eq!(cache.get("k1"), Some(b"v1".to_vec()));
        assert_eq!(cache.get("missing"), None);

        let m = cache.metrics();
        assert_eq!(m.l1_hits, 1);
        assert_eq!(m.l1_misses, 1);
        assert_eq!(m.total_gets, 2);
        assert_eq!(m.total_puts, 1);
    }

    #[test]
    fn test_tiered_cache_hit_rate() {
        let cache = TieredCache::in_memory(100, 1024 * 1024);

        cache.put("k1", b"v1".to_vec());
        cache.get("k1"); // hit
        cache.get("k2"); // miss

        let rate = cache.hit_rate();
        assert!((rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_tiered_cache_with_l2() {
        let dir = std::env::temp_dir().join("spine_cache_test_tiered");
        let _ = fs::remove_dir_all(&dir);

        let config = TieredCacheConfig {
            l1_max_entries: 2,
            l1_max_bytes: 100,
            l2_cache_dir: dir.clone(),
            l2_max_bytes: 1024 * 1024,
            l2_enabled: true,
            l3_enabled: false,
            default_ttl: None,
            promote_on_hit: true,
        };

        let cache = TieredCache::new(config).unwrap();

        // Write to both tiers
        cache.put("k1", b"v1".to_vec());

        // Clear L1 to force L2 read
        cache.l1.clear();

        // Should find in L2 and promote to L1
        assert_eq!(cache.get("k1"), Some(b"v1".to_vec()));
        let m = cache.metrics();
        assert_eq!(m.l1_misses, 1);
        assert_eq!(m.l2_hits, 1);

        // Now should be in L1 (promoted)
        assert_eq!(cache.get("k1"), Some(b"v1".to_vec()));
        let m = cache.metrics();
        assert_eq!(m.l1_hits, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_tiered_cache_with_l3() {
        struct MockRemote {
            data: RwLock<HashMap<String, Vec<u8>>>,
        }

        impl RemoteCache for MockRemote {
            fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
                Ok(self.data.read().unwrap().get(key).cloned())
            }

            fn put(&self, key: &str, value: &[u8], _ttl: Option<Duration>) -> Result<()> {
                self.data
                    .write()
                    .unwrap()
                    .insert(key.to_string(), value.to_vec());
                Ok(())
            }

            fn remove(&self, key: &str) -> Result<bool> {
                Ok(self.data.write().unwrap().remove(key).is_some())
            }
        }

        let remote = Box::new(MockRemote {
            data: RwLock::new(HashMap::new()),
        });

        let cache = TieredCache::in_memory(100, 1024 * 1024).with_remote(remote);

        // Write to all tiers
        cache.put("k1", b"remote_data".to_vec());

        // Clear L1 to force L3 read
        cache.l1.clear();

        assert_eq!(cache.get("k1"), Some(b"remote_data".to_vec()));
        let m = cache.metrics();
        assert_eq!(m.l3_hits, 1);
    }

    #[test]
    fn test_tiered_cache_remove() {
        let cache = TieredCache::in_memory(100, 1024 * 1024);
        cache.put("k1", b"v1".to_vec());
        cache.remove("k1");
        assert_eq!(cache.get("k1"), None);
    }

    #[test]
    fn test_tiered_cache_clear() {
        let cache = TieredCache::in_memory(100, 1024 * 1024);
        cache.put("k1", b"v1".to_vec());
        cache.put("k2", b"v2".to_vec());
        cache.clear();
        assert_eq!(cache.get("k1"), None);
        assert_eq!(cache.get("k2"), None);
    }

    #[test]
    fn test_tiered_cache_ttl() {
        let cache = TieredCache::in_memory(100, 1024 * 1024);
        cache.put_with_ttl("expire", b"data".to_vec(), Some(Duration::from_millis(50)));
        assert!(cache.get("expire").is_some());

        thread::sleep(Duration::from_millis(100));
        assert!(cache.get("expire").is_none());
    }

    #[test]
    fn test_noop_remote_cache() {
        let remote = NoopRemoteCache;
        assert!(remote.get("any").unwrap().is_none());
        assert!(remote.put("any", b"data", None).is_ok());
        assert!(!remote.remove("any").unwrap());
    }
}
