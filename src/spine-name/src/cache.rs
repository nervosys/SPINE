//! The resolver cache.
//!
//! Three properties matter more for agents than they did for browsers:
//!
//! - **Negative caching.** A swarm fans out; if a name is dead, every member
//!   will discover that independently and simultaneously. Without negative
//!   entries, one bad link in a widely-shared record turns into a retry storm.
//! - **Immutable entries never expire.** A `blob:` name is a content hash, so
//!   the answer cannot change. Caching it forever is not a heuristic.
//! - **Stale-while-revalidate.** An agent mid-task would rather act on a
//!   slightly stale endpoint than block. The cache distinguishes *fresh* from
//!   *usable-but-stale* instead of collapsing both into a hit.

use std::collections::HashMap;

use crate::key::NameKey;
use crate::record::NameRecord;
use crate::uri::SpineUri;

/// Default negative-entry lifetime. Short, because a name that does not resolve
/// now may be published a moment later; long enough to absorb a fan-out.
pub const DEFAULT_NEGATIVE_TTL_SECS: u64 = 30;

/// What a cache lookup found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheLookup {
    /// Fresh — usable with no network round trip.
    Fresh(NameRecord),
    /// Present but past its TTL. Usable while a refresh runs, never silently
    /// presented as fresh.
    Stale(NameRecord),
    /// Known not to resolve, and the negative entry is still valid.
    NegativeHit,
    /// Nothing known.
    Miss,
}

impl CacheLookup {
    /// Whether a record is available at all, fresh or not.
    pub fn record(&self) -> Option<&NameRecord> {
        match self {
            CacheLookup::Fresh(r) | CacheLookup::Stale(r) => Some(r),
            _ => None,
        }
    }

    pub fn is_fresh(&self) -> bool {
        matches!(self, CacheLookup::Fresh(_))
    }
}

#[derive(Debug, Clone)]
struct Entry {
    record: NameRecord,
    /// Insertion order stamp, for LRU-ish eviction without a clock.
    stamp: u64,
    /// Immutable entries (`blob:`) are never expired.
    immutable: bool,
}

/// Cache statistics — worth exposing because a resolver's hit rate is the
/// difference between a swarm that scales and one that melts its own network.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub stale_hits: u64,
    pub negative_hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

impl CacheStats {
    /// Fraction of lookups answered without a network round trip.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.stale_hits + self.negative_hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        (self.hits + self.stale_hits + self.negative_hits) as f64 / total as f64
    }
}

/// A bounded resolver cache.
#[derive(Debug)]
pub struct ResolverCache {
    entries: HashMap<NameKey, Entry>,
    /// Name key -> when the negative entry stops applying.
    negative: HashMap<NameKey, u64>,
    capacity: usize,
    negative_ttl: u64,
    counter: u64,
    stats: CacheStats,
}

impl ResolverCache {
    /// A cache holding at most `capacity` positive entries.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            negative: HashMap::new(),
            capacity: capacity.max(1),
            negative_ttl: DEFAULT_NEGATIVE_TTL_SECS,
            counter: 0,
            stats: CacheStats::default(),
        }
    }

    pub fn with_negative_ttl(mut self, secs: u64) -> Self {
        self.negative_ttl = secs;
        self
    }

    /// Look up a name at time `now`.
    pub fn get(&mut self, uri: &SpineUri, now: u64) -> CacheLookup {
        let key = uri.key();

        if let Some(entry) = self.entries.get(&key) {
            // A blob: name pins a content hash, so its answer cannot go stale.
            if entry.immutable || !entry.record.is_expired(now) {
                self.stats.hits += 1;
                return CacheLookup::Fresh(entry.record.clone());
            }
            self.stats.stale_hits += 1;
            return CacheLookup::Stale(entry.record.clone());
        }

        if let Some(until) = self.negative.get(&key) {
            if now < *until {
                self.stats.negative_hits += 1;
                return CacheLookup::NegativeHit;
            }
            self.negative.remove(&key);
        }

        self.stats.misses += 1;
        CacheLookup::Miss
    }

    /// Cache a resolved record. Clears any negative entry for the same name.
    pub fn put(&mut self, record: NameRecord) {
        let key = record.name.key();
        let immutable = record.name.is_immutable();
        self.negative.remove(&key);
        self.counter += 1;
        self.entries.insert(
            key,
            Entry {
                record,
                stamp: self.counter,
                immutable,
            },
        );
        self.evict_if_needed();
    }

    /// Record that a name did not resolve.
    pub fn put_negative(&mut self, uri: &SpineUri, now: u64) {
        self.negative
            .insert(uri.key(), now.saturating_add(self.negative_ttl));
    }

    /// Whether a cached record still matches a content hash — the strong
    /// validator that lets a revalidation return "unchanged" without a body.
    pub fn matches_validator(&self, uri: &SpineUri, hash: &[u8; 32]) -> bool {
        self.entries
            .get(&uri.key())
            .and_then(|e| e.record.content_hash)
            .is_some_and(|h| h == *hash)
    }

    /// Refresh an entry's freshness in place after a revalidation confirmed it
    /// is unchanged, without re-transferring the record.
    pub fn touch(&mut self, uri: &SpineUri, published_at: u64) -> bool {
        let Some(entry) = self.entries.get_mut(&uri.key()) else {
            return false;
        };
        entry.record.published_at = published_at;
        true
    }

    pub fn remove(&mut self, uri: &SpineUri) -> Option<NameRecord> {
        self.entries.remove(&uri.key()).map(|e| e.record)
    }

    /// Drop expired positive and negative entries, returning how many went.
    pub fn sweep(&mut self, now: u64) -> usize {
        let before = self.entries.len() + self.negative.len();
        self.entries
            .retain(|_, e| e.immutable || !e.record.is_expired(now));
        self.negative.retain(|_, until| now < *until);
        before - (self.entries.len() + self.negative.len())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn negative_len(&self) -> usize {
        self.negative.len()
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.negative.clear();
    }

    fn evict_if_needed(&mut self) {
        while self.entries.len() > self.capacity {
            // Evict the oldest *mutable* entry first — immutable entries are
            // both cheap to keep correct and expensive to re-fetch.
            let victim = self
                .entries
                .iter()
                .filter(|(_, e)| !e.immutable)
                .min_by_key(|(_, e)| e.stamp)
                .map(|(k, _)| *k)
                .or_else(|| {
                    self.entries
                        .iter()
                        .min_by_key(|(_, e)| e.stamp)
                        .map(|(k, _)| *k)
                });
            match victim {
                Some(k) => {
                    self.entries.remove(&k);
                    self.stats.evictions += 1;
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::NameRecord;
    use ed25519_dalek::SigningKey;

    fn record(seed: u8, now: u64, ttl: u32) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, 1, now).unwrap().with_ttl(ttl);
        rec.sign(&key).unwrap();
        rec
    }

    #[test]
    fn a_fresh_entry_is_served_without_a_round_trip() {
        let mut c = ResolverCache::new(10);
        let rec = record(1, 1_000, 100);
        c.put(rec.clone());
        assert_eq!(c.get(&rec.name, 1_050), CacheLookup::Fresh(rec.clone()));
        assert_eq!(c.stats().hits, 1);
    }

    #[test]
    fn an_expired_entry_is_reported_stale_rather_than_fresh() {
        let mut c = ResolverCache::new(10);
        let rec = record(1, 1_000, 100);
        c.put(rec.clone());
        let hit = c.get(&rec.name, 1_200);
        assert_eq!(hit, CacheLookup::Stale(rec.clone()));
        assert!(!hit.is_fresh());
        assert!(hit.record().is_some(), "still usable while revalidating");
        assert_eq!(c.stats().stale_hits, 1);
    }

    #[test]
    fn a_miss_is_a_miss() {
        let mut c = ResolverCache::new(10);
        assert_eq!(c.get(&SpineUri::did([9u8; 32]), 0), CacheLookup::Miss);
        assert_eq!(c.stats().misses, 1);
    }

    #[test]
    fn negative_entries_absorb_a_fan_out_then_lapse() {
        let mut c = ResolverCache::new(10).with_negative_ttl(30);
        let uri = SpineUri::did([9u8; 32]);
        c.put_negative(&uri, 1_000);

        // Ten swarm members ask; none of them generates a lookup.
        for _ in 0..10 {
            assert_eq!(c.get(&uri, 1_010), CacheLookup::NegativeHit);
        }
        assert_eq!(c.stats().negative_hits, 10);

        // After the TTL the name is retried.
        assert_eq!(c.get(&uri, 1_030), CacheLookup::Miss);
        assert_eq!(c.negative_len(), 0, "lapsed entry is cleaned up on read");
    }

    #[test]
    fn resolving_a_name_clears_its_negative_entry() {
        let mut c = ResolverCache::new(10);
        let rec = record(1, 1_000, 100);
        c.put_negative(&rec.name, 1_000);
        c.put(rec.clone());
        assert!(c.get(&rec.name, 1_001).is_fresh());
        assert_eq!(c.negative_len(), 0);
    }

    #[test]
    fn immutable_blob_names_never_go_stale() {
        let mut c = ResolverCache::new(10);
        // Build a record whose *name* is a blob: authority by rewriting a signed
        // record's cached name — the cache keys on immutability of the name.
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let did_name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(did_name, 1, 1_000).unwrap().with_ttl(1);
        rec.sign(&key).unwrap();
        rec.name = SpineUri::blob_of(b"immutable payload");
        c.put(rec.clone());

        // Far past any TTL, still fresh: a content hash cannot change.
        assert!(c.get(&rec.name, u64::MAX / 2).is_fresh());
        assert_eq!(
            c.sweep(u64::MAX / 2),
            0,
            "immutable entries survive a sweep"
        );
    }

    #[test]
    fn content_hash_acts_as_a_strong_validator() {
        let mut c = ResolverCache::new(10);
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut rec = record(1, 1_000, 100).with_content_hash([7u8; 32]);
        rec.sign(&key).unwrap();
        c.put(rec.clone());

        assert!(c.matches_validator(&rec.name, &[7u8; 32]));
        assert!(!c.matches_validator(&rec.name, &[8u8; 32]));
        assert!(!c.matches_validator(&SpineUri::did([9u8; 32]), &[7u8; 32]));
    }

    #[test]
    fn touch_refreshes_freshness_without_refetching() {
        let mut c = ResolverCache::new(10);
        let rec = record(1, 1_000, 100);
        c.put(rec.clone());
        assert!(!c.get(&rec.name, 1_200).is_fresh());

        assert!(c.touch(&rec.name, 1_200));
        assert!(
            c.get(&rec.name, 1_250).is_fresh(),
            "a confirmed-unchanged record becomes fresh again with no transfer"
        );
        assert!(!c.touch(&SpineUri::did([9u8; 32]), 0));
    }

    #[test]
    fn capacity_is_enforced_by_evicting_the_oldest() {
        let mut c = ResolverCache::new(2);
        c.put(record(1, 1_000, 100));
        c.put(record(2, 1_000, 100));
        c.put(record(3, 1_000, 100));
        assert_eq!(c.len(), 2);
        assert_eq!(c.stats().evictions, 1);
        // The first inserted is gone; the newest remains.
        assert_eq!(c.get(&record(1, 1_000, 100).name, 1_000), CacheLookup::Miss);
        assert!(c.get(&record(3, 1_000, 100).name, 1_000).is_fresh());
    }

    #[test]
    fn eviction_prefers_mutable_entries_over_immutable_ones() {
        let mut c = ResolverCache::new(2);
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut blob =
            NameRecord::new(SpineUri::did(key.verifying_key().to_bytes()), 1, 1_000).unwrap();
        blob.sign(&key).unwrap();
        blob.name = SpineUri::blob_of(b"pinned");

        c.put(blob.clone()); // oldest, but immutable
        c.put(record(2, 1_000, 100));
        c.put(record(3, 1_000, 100));

        assert!(
            c.get(&blob.name, 1_000).is_fresh(),
            "immutable entry survives even though it was inserted first"
        );
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn sweeping_drops_expired_positive_and_negative_entries() {
        let mut c = ResolverCache::new(10).with_negative_ttl(30);
        c.put(record(1, 1_000, 50));
        c.put(record(2, 1_000, 5_000));
        c.put_negative(&SpineUri::did([9u8; 32]), 1_000);

        assert_eq!(
            c.sweep(1_100),
            2,
            "one expired record + one lapsed negative"
        );
        assert_eq!(c.len(), 1);
        assert_eq!(c.negative_len(), 0);
    }

    #[test]
    fn hit_rate_reflects_avoided_round_trips() {
        let mut c = ResolverCache::new(10);
        let rec = record(1, 1_000, 100);
        c.put(rec.clone());
        c.get(&rec.name, 1_000); // hit
        c.get(&rec.name, 1_000); // hit
        c.get(&SpineUri::did([9u8; 32]), 1_000); // miss
        let stats = c.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert!((stats.hit_rate() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(CacheStats::default().hit_rate(), 0.0);
    }
}
