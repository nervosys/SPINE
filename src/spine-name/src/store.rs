//! The record store a node serves from.
//!
//! Two indexes, because agents ask two different questions. "What is at this
//! name" is a keyspace lookup. "Who can do `web.search`" is a capability
//! lookup — and it is the question agents actually ask most of the time. The
//! human web never answered the second one natively, which is why it needed
//! search engines as centralized intermediaries. Indexing capabilities at every
//! node makes the answer federated by construction.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::key::NameKey;
use crate::record::NameRecord;
use crate::uri::SpineUri;
use crate::NameError;

/// Outcome of offering a record to the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    /// Name was previously unknown.
    Inserted,
    /// Replaced an older version.
    Updated,
    /// Kept the existing record because it was newer or equal.
    Superseded,
}

/// A verified, expiry-aware collection of name records.
#[derive(Debug, Default)]
pub struct RecordStore {
    by_key: HashMap<NameKey, NameRecord>,
    /// Capability term -> the names advertising it.
    by_capability: BTreeMap<String, HashSet<NameKey>>,
}

impl RecordStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Offer a record to the store.
    ///
    /// The signature is verified here rather than at a call site, so there is no
    /// path that admits an unverified record. Records whose name is not
    /// self-certifying are rejected outright.
    pub fn put(&mut self, record: NameRecord) -> Result<PutOutcome, NameError> {
        record.verify()?;
        let key = record.name.key();

        if let Some(existing) = self.by_key.get(&key) {
            if !record.supersedes(existing) {
                return Ok(PutOutcome::Superseded);
            }
            self.deindex(&key, existing.capabilities.clone());
            self.index(&key, &record);
            self.by_key.insert(key, record);
            return Ok(PutOutcome::Updated);
        }

        self.index(&key, &record);
        self.by_key.insert(key, record);
        Ok(PutOutcome::Inserted)
    }

    /// Look up by name, ignoring expiry (the caller decides how to treat a
    /// stale-but-present record).
    pub fn get(&self, uri: &SpineUri) -> Option<&NameRecord> {
        self.by_key.get(&uri.key())
    }

    /// Look up by name, returning `None` once the record has gone stale.
    pub fn get_fresh(&self, uri: &SpineUri, now: u64) -> Option<&NameRecord> {
        self.get(uri).filter(|r| !r.is_expired(now))
    }

    /// Look up by routing key — what a node answers a DHT query with.
    pub fn get_by_key(&self, key: &NameKey) -> Option<&NameRecord> {
        self.by_key.get(key)
    }

    /// Every fresh record advertising a capability term.
    ///
    /// Ranked by endpoint count then by recency: a provider reachable over more
    /// transports is likelier to be usable, and a fresher record is likelier to
    /// still be true.
    pub fn providers_of(&self, term: &str, now: u64) -> Vec<&NameRecord> {
        let want = term.to_ascii_lowercase();
        let Some(keys) = self.by_capability.get(&want) else {
            return Vec::new();
        };
        let mut out: Vec<&NameRecord> = keys
            .iter()
            .filter_map(|k| self.by_key.get(k))
            .filter(|r| !r.is_expired(now))
            .collect();
        out.sort_by(|a, b| {
            b.endpoints
                .len()
                .cmp(&a.endpoints.len())
                .then(b.published_at.cmp(&a.published_at))
                .then(a.name.to_string().cmp(&b.name.to_string()))
        });
        out
    }

    /// The `n` records whose keys are closest to `target` — the answer a node
    /// gives when it does not hold the exact name but knows the neighborhood.
    pub fn closest(&self, target: &NameKey, n: usize) -> Vec<&NameRecord> {
        let mut all: Vec<&NameRecord> = self.by_key.values().collect();
        all.sort_by_key(|r| r.name.key().distance(target));
        all.into_iter().take(n).collect()
    }

    /// Drop expired records, returning how many were removed.
    pub fn sweep_expired(&mut self, now: u64) -> usize {
        let dead: Vec<(NameKey, Vec<String>)> = self
            .by_key
            .iter()
            .filter(|(_, r)| r.is_expired(now))
            .map(|(k, r)| (*k, r.capabilities.clone()))
            .collect();
        for (key, caps) in &dead {
            self.by_key.remove(key);
            self.deindex(key, caps.clone());
        }
        dead.len()
    }

    /// Records within `window` seconds of expiring — what a publisher should
    /// re-sign and re-announce before they lapse.
    pub fn needing_republish(&self, now: u64, window: u64) -> Vec<&NameRecord> {
        self.by_key
            .values()
            .filter(|r| !r.is_expired(now) && r.remaining_ttl(now) <= window)
            .collect()
    }

    pub fn remove(&mut self, uri: &SpineUri) -> Option<NameRecord> {
        let key = uri.key();
        let removed = self.by_key.remove(&key)?;
        self.deindex(&key, removed.capabilities.clone());
        Some(removed)
    }

    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Every capability term currently indexed.
    pub fn capability_terms(&self) -> Vec<&str> {
        self.by_capability.keys().map(|s| s.as_str()).collect()
    }

    pub fn records(&self) -> impl Iterator<Item = &NameRecord> {
        self.by_key.values()
    }

    fn index(&mut self, key: &NameKey, record: &NameRecord) {
        for cap in &record.capabilities {
            self.by_capability
                .entry(cap.to_ascii_lowercase())
                .or_default()
                .insert(*key);
        }
    }

    fn deindex(&mut self, key: &NameKey, capabilities: Vec<String>) {
        for cap in capabilities {
            let term = cap.to_ascii_lowercase();
            if let Some(set) = self.by_capability.get_mut(&term) {
                set.remove(key);
                if set.is_empty() {
                    self.by_capability.remove(&term);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Endpoint;
    use ed25519_dalek::SigningKey;

    fn record(seed: u8, seq: u64, now: u64, caps: &[&str], endpoints: usize) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, seq, now).unwrap();
        for c in caps {
            rec = rec.with_capability(*c);
        }
        for i in 0..endpoints {
            rec = rec.with_endpoint(Endpoint::new("tcp", format!("host{i}:9440")));
        }
        rec.sign(&key).unwrap();
        rec
    }

    #[test]
    fn stores_and_retrieves_a_record() {
        let mut s = RecordStore::new();
        let rec = record(1, 1, 1_000, &[], 0);
        assert_eq!(s.put(rec.clone()).unwrap(), PutOutcome::Inserted);
        assert_eq!(s.get(&rec.name), Some(&rec));
        assert_eq!(s.get_by_key(&rec.name.key()), Some(&rec));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn refuses_an_unsigned_or_tampered_record() {
        let mut s = RecordStore::new();
        let mut rec = record(1, 1, 1_000, &[], 0);
        rec.capabilities.push("admin".into()); // invalidates the signature
        assert!(matches!(s.put(rec), Err(NameError::BadSignature)));
        assert!(s.is_empty(), "nothing unverified may enter the store");
    }

    #[test]
    fn a_newer_version_replaces_an_older_one() {
        let mut s = RecordStore::new();
        s.put(record(1, 1, 1_000, &[], 0)).unwrap();
        assert_eq!(s.put(record(1, 2, 1_000, &[], 0)).unwrap(), PutOutcome::Updated);
        assert_eq!(s.len(), 1);
        assert_eq!(s.get(&record(1, 2, 1_000, &[], 0).name).unwrap().seq, 2);
    }

    #[test]
    fn an_older_version_cannot_roll_the_store_back() {
        let mut s = RecordStore::new();
        s.put(record(1, 5, 1_000, &[], 0)).unwrap();
        assert_eq!(
            s.put(record(1, 2, 1_000, &[], 0)).unwrap(),
            PutOutcome::Superseded
        );
        assert_eq!(s.get_by_key(&record(1, 5, 1_000, &[], 0).name.key()).unwrap().seq, 5);
    }

    #[test]
    fn capability_lookup_finds_providers_without_a_directory() {
        let mut s = RecordStore::new();
        s.put(record(1, 1, 1_000, &["web.search"], 1)).unwrap();
        s.put(record(2, 1, 1_000, &["web.search", "web.crawl"], 2)).unwrap();
        s.put(record(3, 1, 1_000, &["data.analyze"], 1)).unwrap();

        let providers = s.providers_of("web.search", 1_000);
        assert_eq!(providers.len(), 2);
        // Better-connected provider ranks first.
        assert_eq!(providers[0].endpoints.len(), 2);
        assert_eq!(s.providers_of("WEB.CRAWL", 1_000).len(), 1);
        assert!(s.providers_of("nothing.here", 1_000).is_empty());
    }

    #[test]
    fn expired_providers_are_not_offered() {
        let mut s = RecordStore::new();
        let rec = record(1, 1, 1_000, &["web.search"], 1).with_ttl(60);
        // Re-sign after mutating ttl.
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut rec = rec;
        rec.sign(&key).unwrap();
        s.put(rec).unwrap();

        assert_eq!(s.providers_of("web.search", 1_030).len(), 1);
        assert!(s.providers_of("web.search", 1_100).is_empty());
    }

    #[test]
    fn updating_a_record_removes_its_stale_capability_index_entries() {
        let mut s = RecordStore::new();
        s.put(record(1, 1, 1_000, &["web.search"], 0)).unwrap();
        assert_eq!(s.providers_of("web.search", 1_000).len(), 1);

        // v2 drops web.search and gains data.analyze.
        s.put(record(1, 2, 1_000, &["data.analyze"], 0)).unwrap();
        assert!(
            s.providers_of("web.search", 1_000).is_empty(),
            "a withdrawn capability must stop being advertised"
        );
        assert_eq!(s.providers_of("data.analyze", 1_000).len(), 1);
        assert_eq!(s.capability_terms(), vec!["data.analyze"]);
    }

    #[test]
    fn freshness_gate_hides_stale_records_from_get_fresh() {
        let mut s = RecordStore::new();
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut rec = record(1, 1, 1_000, &[], 0).with_ttl(60);
        rec.sign(&key).unwrap();
        let name = rec.name.clone();
        s.put(rec).unwrap();

        assert!(s.get_fresh(&name, 1_050).is_some());
        assert!(s.get_fresh(&name, 1_060).is_none());
        assert!(s.get(&name).is_some(), "get() still exposes the stale record");
    }

    #[test]
    fn sweeping_removes_expired_records_and_their_index_entries() {
        let mut s = RecordStore::new();
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut short = record(1, 1, 1_000, &["web.search"], 0).with_ttl(60);
        short.sign(&key).unwrap();
        s.put(short).unwrap();
        s.put(record(2, 1, 1_000, &["web.crawl"], 0)).unwrap();

        assert_eq!(s.sweep_expired(1_100), 1);
        assert_eq!(s.len(), 1);
        assert!(s.providers_of("web.search", 1_100).is_empty());
        assert_eq!(s.capability_terms(), vec!["web.crawl"]);
    }

    #[test]
    fn republish_window_surfaces_records_about_to_lapse() {
        let mut s = RecordStore::new();
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut rec = record(1, 1, 1_000, &[], 0).with_ttl(100);
        rec.sign(&key).unwrap();
        s.put(rec).unwrap();

        assert!(s.needing_republish(1_000, 30).is_empty());
        assert_eq!(s.needing_republish(1_080, 30).len(), 1);
        assert!(
            s.needing_republish(1_200, 30).is_empty(),
            "already-expired records are the sweeper's job, not the publisher's"
        );
    }

    #[test]
    fn closest_ranks_records_by_keyspace_distance() {
        let mut s = RecordStore::new();
        for seed in 1..=5u8 {
            s.put(record(seed, 1, 1_000, &[], 0)).unwrap();
        }
        let target = record(3, 1, 1_000, &[], 0).name.key();
        let closest = s.closest(&target, 3);
        assert_eq!(closest.len(), 3);
        assert_eq!(closest[0].name.key(), target);
    }

    #[test]
    fn removal_clears_both_indexes() {
        let mut s = RecordStore::new();
        let rec = record(1, 1, 1_000, &["web.search"], 0);
        s.put(rec.clone()).unwrap();
        assert!(s.remove(&rec.name).is_some());
        assert!(s.is_empty());
        assert!(s.providers_of("web.search", 1_000).is_empty());
        assert!(s.remove(&rec.name).is_none());
    }
}
