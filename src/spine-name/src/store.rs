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
use crate::logbook::{log_inception_key, LogBook, LogChange, LogOffer};
use crate::record::NameRecord;
use crate::succession::SuccessionLog;
use crate::uri::SpineUri;
use crate::NameError;

/// Outcome of offering a record to the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    /// Name was previously unknown.
    Inserted,
    /// Replaced an older version.
    Updated,
    /// Kept the existing record because it was newer or equal. This includes a
    /// record that a held tombstone for the same name outranks.
    Superseded,
    /// The store was full and this record was the least worth keeping.
    ///
    /// Distinct from an error: nothing was wrong with the record, and the
    /// caller has not been told its peer misbehaved. It simply belongs
    /// somewhere else in the keyspace more than it belongs here.
    Rejected,
}

/// How many records a store holds before it starts evicting.
///
/// A signature proves the publisher holds the key for a name, which costs an
/// attacker one keygen — so "verified" is not a bound on how much anyone may
/// store here. This is.
pub const DEFAULT_CAPACITY: usize = 10_000;

/// The share of a store's capacity that tombstones may hold: capacity divided by
/// this, and at least one. With the value 2, a store of 10 000 records holds at
/// most 5 000 tombstones. A tombstone beyond the share evicts the oldest-published
/// tombstone (ties: the smaller key), so a flood of revocations cannot push live
/// records out, and a live record is never refused because tombstones hold the
/// room. See [`RecordStore::put`] and spec 02, 10.10 decision 52.
pub const TOMBSTONE_SHARE_DIVISOR: usize = 2;

/// A verified, expiry-aware collection of name records.
#[derive(Debug, Clone)]
pub struct RecordStore {
    by_key: HashMap<NameKey, NameRecord>,
    /// Capability term -> the names advertising it.
    by_capability: BTreeMap<String, HashSet<NameKey>>,
    capacity: usize,
    /// This node's own keyspace position, when it has one.
    ///
    /// Eviction is by distance from here: a store under pressure should give up
    /// what is furthest from it first, because those are the records lookups are
    /// least likely to route to this node for. Without a home key there is no
    /// such ordering, so eviction falls back to dropping whatever expires
    /// soonest.
    home: Option<NameKey>,
    /// The succession logs and equivocation proofs of the names held here. Empty
    /// for a store that has never been offered a log, in which case admission is
    /// exactly [`NameRecord::verify`].
    logs: LogBook,
}

impl Default for RecordStore {
    fn default() -> Self {
        Self {
            by_key: HashMap::new(),
            by_capability: BTreeMap::new(),
            capacity: DEFAULT_CAPACITY,
            home: None,
            logs: LogBook::new(),
        }
    }
}

impl RecordStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store that evicts by distance from `home`, holding at most `capacity`.
    pub fn with_limits(home: NameKey, capacity: usize) -> Self {
        Self {
            home: Some(home),
            capacity: capacity.max(1),
            ..Self::default()
        }
    }

    /// A store with no keyspace position, holding at most `capacity`. Under
    /// pressure it gives up the oldest-published record first (see
    /// [`RecordStore::with_limits`] for the store that evicts by distance).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            ..Self::default()
        }
    }

    /// How many records this store will hold.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Rank a key by how readily this store would give it up — larger is worse.
    fn eviction_rank(&self, key: &NameKey, record: &NameRecord) -> (u64, [u8; 32]) {
        match &self.home {
            // Furthest from home goes first. The distance is the whole ordering;
            // the key breaks ties so the choice is deterministic.
            Some(home) => (0, home.distance(key).0),
            // No position to measure from, so shortest remaining life goes first.
            None => (u64::MAX - record.published_at, *key.as_bytes()),
        }
    }

    /// The live (non-tombstone) record this store would give up first, if it
    /// holds any. A tombstone is never a candidate here.
    fn eviction_candidate(&self) -> Option<(NameKey, (u64, [u8; 32]))> {
        self.by_key
            .iter()
            .filter(|(_, r)| !r.is_tombstone())
            .map(|(k, r)| (*k, self.eviction_rank(k, r)))
            .max_by(|a, b| a.1.cmp(&b.1))
    }

    /// How many tombstones this store may hold: [`TOMBSTONE_SHARE_DIVISOR`]
    /// of its capacity, and at least one.
    pub fn tombstone_limit(&self) -> usize {
        (self.capacity / TOMBSTONE_SHARE_DIVISOR).max(1)
    }

    /// How many tombstones are held (expired ones included until swept). Counted
    /// by a scan, so there is no second count to keep in step with the map at
    /// insert, replace, sweep, remove and load.
    pub fn tombstone_count(&self) -> usize {
        self.by_key.values().filter(|r| r.is_tombstone()).count()
    }

    /// The oldest-published tombstone, other than `except`; the smaller key
    /// breaks a tie in `published_at`, so the choice does not depend on
    /// arrival or map order.
    fn oldest_tombstone(&self, except: Option<NameKey>) -> Option<NameKey> {
        self.by_key
            .iter()
            .filter(|(k, r)| r.is_tombstone() && Some(**k) != except)
            .min_by(|a, b| {
                a.1.published_at
                    .cmp(&b.1.published_at)
                    .then_with(|| a.0.as_bytes().cmp(b.0.as_bytes()))
            })
            .map(|(k, _)| *k)
    }

    fn evict(&mut self, key: &NameKey) {
        if let Some(evicted) = self.by_key.remove(key) {
            self.deindex(key, evicted.capabilities);
        }
    }

    /// Evict the oldest tombstones, never `keep`, until at most `allowed` remain.
    fn trim_tombstones(&mut self, allowed: usize, keep: Option<NameKey>) {
        while self.tombstone_count() > allowed {
            match self.oldest_tombstone(keep) {
                Some(oldest) => self.evict(&oldest),
                None => break,
            }
        }
    }

    /// The succession logs and equivocation proofs this store holds.
    pub fn logs(&self) -> &LogBook {
        &self.logs
    }

    /// Plan what offering `log` would do (see [`LogBook::plan`]), without
    /// changing anything: a caller with a durable backend writes the result
    /// first and then calls [`RecordStore::apply_log`]. The names this store
    /// holds records of are protected from eviction.
    pub fn plan_log(&self, log: SuccessionLog) -> Result<LogOffer, NameError> {
        let Some(key) = log_inception_key(&log) else {
            return Ok(LogOffer::Unchanged);
        };
        let protected: HashSet<[u8; 32]> = if self.logs.is_full() {
            self.by_key
                .values()
                .filter_map(|r| r.name.public_key().copied())
                .collect()
        } else {
            HashSet::new()
        };
        self.logs
            .plan(&key, log, &protected)
            .map_err(|e| NameError::InvalidSuccessionLog(e.reason().to_string()))
    }

    /// Carry out a plan from [`RecordStore::plan_log`]. Records of the name that
    /// the new state no longer authorises (signed by a superseded key, with a
    /// stale claim, of a retired or compromised name) are dropped; the number is
    /// returned with the change.
    pub fn apply_log(&mut self, offer: LogOffer) -> (LogChange, usize) {
        let change = self.logs.apply(offer);
        let key = match change {
            LogChange::Unchanged => return (change, 0),
            LogChange::Adopted { inception_key } | LogChange::Compromised { inception_key } => {
                inception_key
            }
        };
        let dead: Vec<NameKey> = self
            .by_key
            .iter()
            .filter(|(_, r)| r.name.public_key() == Some(&key) && self.logs.authorise(r).is_err())
            .map(|(k, _)| *k)
            .collect();
        for k in &dead {
            if let Some(removed) = self.by_key.remove(k) {
                self.deindex(k, removed.capabilities);
            }
        }
        (change, dead.len())
    }

    /// Offer a succession log: [`RecordStore::plan_log`] then
    /// [`RecordStore::apply_log`], for a store with no backend of its own.
    /// A log that cannot be taken because a table is full is
    /// [`NameError::SuccessionCapacity`].
    pub fn offer_log(&mut self, log: SuccessionLog) -> Result<LogChange, NameError> {
        let offer = self.plan_log(log)?;
        if matches!(offer, LogOffer::Full | LogOffer::ProofsFull) {
            return Err(NameError::SuccessionCapacity);
        }
        Ok(self.apply_log(offer).0)
    }

    /// Restore a persisted log or proof before any record is put, so that
    /// records of rotated names are admitted against it.
    pub fn restore_log(&mut self, inception_key: [u8; 32], log: SuccessionLog) -> bool {
        self.logs.restore_log(inception_key, log)
    }

    /// See [`RecordStore::restore_log`].
    pub fn restore_proof(
        &mut self,
        inception_key: [u8; 32],
        proof: crate::succession::Equivocation,
    ) -> bool {
        self.logs.restore_proof(inception_key, proof)
    }

    /// Offer a record to the store at time `now` (the caller's clock, Unix
    /// seconds).
    ///
    /// The signature is verified here rather than at a call site, so there is no
    /// path that admits an unverified record: [`LogBook::authorise`], which for
    /// a name with no succession log is exactly [`NameRecord::verify`] and for a
    /// name with one is the check of spec 02, 10.6 against the log held (a record
    /// of a name that needs a log not held is [`NameError::NeedSuccessionLog`]).
    /// Records whose name is not self-certifying are rejected outright. The
    /// lifetime bound ([`NameRecord::verify`]) and the bound on `published_at`
    /// against `now` ([`NameRecord::validate_not_postdated`]) are enforced here
    /// too.
    ///
    /// A held tombstone keeps its name revoked: a record of the same or lower
    /// `seq` does not supersede it ([`NameRecord::supersedes`]) and is answered
    /// with [`PutOutcome::Superseded`].
    ///
    /// # Eviction (spec 02, 10.10 decisions 48 and 52)
    ///
    /// When the store is full and a new name arrives, in this order: the
    /// records lapsed at `now` are dropped (a lapsed record, tombstone or not, is
    /// no longer served or enforced); then, for a live record, the furthest
    /// *live* record is the victim if the arrival is nearer than it, else the
    /// arrival is [`PutOutcome::Rejected`], whatever tombstones are held; a
    /// tombstone is the victim only when no live record is held, and then the
    /// oldest-published. A tombstone arriving is admitted by evicting the
    /// furthest live record, whatever the distance (a revocation should be hard
    /// to lose, and the share below bounds the price). Tombstones hold at most
    /// [`RecordStore::tombstone_limit`] places ([`TOMBSTONE_SHARE_DIVISOR`]): a
    /// tombstone that would exceed it evicts the oldest-published tombstone
    /// first (also when the store is not full). A tombstone that replaces a
    /// record of its own name is an update and evicts nothing for room, but
    /// counts toward the share afterwards, and then the oldest *other*
    /// tombstone goes, never the one just stored.
    ///
    /// An evicted tombstone no longer blocks the older record it revoked: the
    /// name can be resurrected. Only a `retire` entry of a succession log is
    /// permanent.
    pub fn put(&mut self, record: NameRecord, now: u64) -> Result<PutOutcome, NameError> {
        record.validate_not_postdated(now)?;
        self.logs.authorise(&record)?;
        let key = record.name.key();

        if let Some(existing) = self.by_key.get(&key) {
            if !record.supersedes(existing) {
                return Ok(PutOutcome::Superseded);
            }
            self.deindex(&key, existing.capabilities.clone());
            self.index(&key, &record);
            let tombstone = record.is_tombstone();
            self.by_key.insert(key, record);
            if tombstone {
                self.trim_tombstones(self.tombstone_limit(), Some(key));
            }
            return Ok(PutOutcome::Updated);
        }

        // A tombstone beyond its share evicts the oldest tombstones first. Lapsed
        // records are dropped before the choice, so an expired tombstone is
        // never preferred to a live one for a place.
        let tombstone = record.is_tombstone();
        if tombstone && self.tombstone_count() >= self.tombstone_limit() {
            self.sweep_expired(now);
            self.trim_tombstones(self.tombstone_limit() - 1, None);
        }

        // Full: lapsed records go first, whatever their distance. They are
        // already invisible to `get_fresh` and `providers_of`, so dropping them
        // changes nothing a reader can see. An unexpired record, tombstone
        // included, is not touched by this step.
        if self.by_key.len() >= self.capacity {
            self.sweep_expired(now);
        }

        // Still full: something has to go, and it should be whatever this node is
        // least suited to hold. If that is the arriving record, it is refused
        // rather than displacing a record nearer to us — otherwise a stream of
        // distant names would evict exactly the ones we are responsible for.
        // Only a live record is a victim; a tombstone goes only when no live
        // record is held, and a tombstone arriving evicts the furthest live
        // record whatever its distance.
        if self.by_key.len() >= self.capacity {
            let incoming = self.eviction_rank(&key, &record);
            let victim = match self.eviction_candidate() {
                Some((worst_key, worst_rank)) if tombstone || worst_rank > incoming => {
                    Some(worst_key)
                }
                Some(_) => None,
                None => self.oldest_tombstone(None),
            };
            match victim {
                Some(victim) => self.evict(&victim),
                None => return Ok(PutOutcome::Rejected),
            }
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

    /// Whether a fresh tombstone is held for `uri`.
    pub fn is_revoked(&self, uri: &SpineUri, now: u64) -> bool {
        self.get_fresh(uri, now).is_some_and(|r| r.is_tombstone())
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

    /// The store clock in these tests; every record is published at 1_000.
    const NOW: u64 = 1_000;

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
        assert_eq!(s.put(rec.clone(), NOW).unwrap(), PutOutcome::Inserted);
        assert_eq!(s.get(&rec.name), Some(&rec));
        assert_eq!(s.get_by_key(&rec.name.key()), Some(&rec));
        assert_eq!(s.len(), 1);
    }

    fn tombstone(seed: u8, seq: u64, at: u64) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::tombstone(name, seq, at).unwrap();
        rec.sign(&key).unwrap();
        rec
    }

    #[test]
    fn refuses_an_over_long_or_post_dated_record() {
        let mut s = RecordStore::new();
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());

        // Signed around `sign`, which would refuse it.
        let mut over = NameRecord::new(name, 1, NOW)
            .unwrap()
            .with_ttl(crate::MAX_RECORD_TTL_SECS + 1);
        let sig: ed25519_dalek::Signature =
            ed25519_dalek::Signer::sign(&key, &over.signing_bytes());
        over.signature = sig.to_bytes().to_vec();
        assert!(matches!(s.put(over, NOW), Err(NameError::TtlTooLong(_))));

        let ahead = record(1, 1, NOW + crate::MAX_CLOCK_SKEW_SECS + 1, &[], 0);
        assert!(matches!(
            s.put(ahead, NOW),
            Err(NameError::PostDated { .. })
        ));
        let edge = record(1, 1, NOW + crate::MAX_CLOCK_SKEW_SECS, &[], 0);
        assert_eq!(s.put(edge, NOW).unwrap(), PutOutcome::Inserted);
    }

    #[test]
    fn a_held_tombstone_blocks_lower_and_equal_seq_but_not_a_higher_one() {
        let mut s = RecordStore::new();
        s.put(record(1, 5, NOW, &["web.search"], 1), NOW).unwrap();
        assert_eq!(
            s.put(tombstone(1, 6, NOW), NOW).unwrap(),
            PutOutcome::Updated
        );
        let name = tombstone(1, 6, NOW).name;
        assert!(s.is_revoked(&name, NOW));
        assert!(s.providers_of("web.search", NOW).is_empty());
        assert!(s.capability_terms().is_empty());

        // Lower seq, equal seq (even published later), and a replayed old record.
        for seq in [1, 5, 6] {
            assert_eq!(
                s.put(record(1, seq, NOW + 100, &["web.search"], 1), NOW + 100)
                    .unwrap(),
                PutOutcome::Superseded
            );
        }
        assert!(s.get(&name).unwrap().is_tombstone());

        assert_eq!(
            s.put(record(1, 7, NOW, &[], 0), NOW).unwrap(),
            PutOutcome::Updated
        );
        assert!(!s.is_revoked(&name, NOW));
    }

    #[test]
    fn a_tombstone_replaces_a_same_seq_record_in_either_arrival_order() {
        let mut a = RecordStore::new();
        a.put(record(1, 5, NOW + 50, &[], 0), NOW + 50).unwrap();
        a.put(tombstone(1, 5, NOW), NOW + 50).unwrap();
        let mut b = RecordStore::new();
        b.put(tombstone(1, 5, NOW), NOW + 50).unwrap();
        b.put(record(1, 5, NOW + 50, &[], 0), NOW + 50).unwrap();
        let name = tombstone(1, 5, NOW).name;
        assert!(a.get(&name).unwrap().is_tombstone());
        assert!(b.get(&name).unwrap().is_tombstone());
    }

    #[test]
    fn refuses_an_unsigned_or_tampered_record() {
        let mut s = RecordStore::new();
        let mut rec = record(1, 1, 1_000, &[], 0);
        rec.capabilities.push("admin".into()); // invalidates the signature
        assert!(matches!(s.put(rec, NOW), Err(NameError::BadSignature)));
        assert!(s.is_empty(), "nothing unverified may enter the store");
    }

    #[test]
    fn a_newer_version_replaces_an_older_one() {
        let mut s = RecordStore::new();
        s.put(record(1, 1, 1_000, &[], 0), NOW).unwrap();
        assert_eq!(
            s.put(record(1, 2, 1_000, &[], 0), NOW).unwrap(),
            PutOutcome::Updated
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s.get(&record(1, 2, 1_000, &[], 0).name).unwrap().seq, 2);
    }

    #[test]
    fn an_older_version_cannot_roll_the_store_back() {
        let mut s = RecordStore::new();
        s.put(record(1, 5, 1_000, &[], 0), NOW).unwrap();
        assert_eq!(
            s.put(record(1, 2, 1_000, &[], 0), NOW).unwrap(),
            PutOutcome::Superseded
        );
        assert_eq!(
            s.get_by_key(&record(1, 5, 1_000, &[], 0).name.key())
                .unwrap()
                .seq,
            5
        );
    }

    #[test]
    fn capability_lookup_finds_providers_without_a_directory() {
        let mut s = RecordStore::new();
        s.put(record(1, 1, 1_000, &["web.search"], 1), NOW).unwrap();
        s.put(record(2, 1, 1_000, &["web.search", "web.crawl"], 2), NOW)
            .unwrap();
        s.put(record(3, 1, 1_000, &["data.analyze"], 1), NOW)
            .unwrap();

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
        s.put(rec, NOW).unwrap();

        assert_eq!(s.providers_of("web.search", 1_030).len(), 1);
        assert!(s.providers_of("web.search", 1_100).is_empty());
    }

    #[test]
    fn updating_a_record_removes_its_stale_capability_index_entries() {
        let mut s = RecordStore::new();
        s.put(record(1, 1, 1_000, &["web.search"], 0), NOW).unwrap();
        assert_eq!(s.providers_of("web.search", 1_000).len(), 1);

        // v2 drops web.search and gains data.analyze.
        s.put(record(1, 2, 1_000, &["data.analyze"], 0), NOW)
            .unwrap();
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
        s.put(rec, NOW).unwrap();

        assert!(s.get_fresh(&name, 1_050).is_some());
        assert!(s.get_fresh(&name, 1_060).is_none());
        assert!(
            s.get(&name).is_some(),
            "get() still exposes the stale record"
        );
    }

    #[test]
    fn sweeping_removes_expired_records_and_their_index_entries() {
        let mut s = RecordStore::new();
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut short = record(1, 1, 1_000, &["web.search"], 0).with_ttl(60);
        short.sign(&key).unwrap();
        s.put(short, NOW).unwrap();
        s.put(record(2, 1, 1_000, &["web.crawl"], 0), NOW).unwrap();

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
        s.put(rec, NOW).unwrap();

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
            s.put(record(seed, 1, 1_000, &[], 0), NOW).unwrap();
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
        s.put(rec.clone(), NOW).unwrap();
        assert!(s.remove(&rec.name).is_some());
        assert!(s.is_empty());
        assert!(s.providers_of("web.search", 1_000).is_empty());
        assert!(s.remove(&rec.name).is_none());
    }

    /// A signature proves the publisher holds the key for a name, and minting a
    /// `did:` name costs one keygen — so verification is no bound on how much
    /// anyone may store here. The capacity is.
    #[test]
    fn a_full_store_stops_growing() {
        let mut s = RecordStore::with_limits(NameKey::of(b"home"), 3);
        for seed in 1..=8u8 {
            let _ = s.put(record(seed, 1, 1_000, &[], 0), NOW);
        }
        assert_eq!(s.len(), 3, "the cap has to hold under a flood");
    }

    /// What a store gives up first should be what it is least suited to hold.
    /// Records near this node are the ones lookups will actually route here for.
    #[test]
    fn eviction_gives_up_the_furthest_key_first() {
        // Pick a home adjacent to one record's key, so that record is nearest.
        let near = record(1, 1, 1_000, &[], 0);
        let home = near.name.key();

        let mut s = RecordStore::with_limits(home, 2);
        s.put(near.clone(), NOW).unwrap();
        for seed in 2..=6u8 {
            let _ = s.put(record(seed, 1, 1_000, &[], 0), NOW);
        }

        assert_eq!(s.len(), 2);
        assert!(
            s.get(&near.name).is_some(),
            "the record nearest home must survive a flood of distant ones"
        );
    }

    /// A record further away than everything held is refused outright, rather
    /// than displacing something nearer. Otherwise a stream of distant names
    /// would evict exactly the records this node is responsible for.
    #[test]
    fn a_record_further_than_all_of_them_is_refused_not_swapped_in() {
        let near = record(1, 1, 1_000, &[], 0);
        let home = near.name.key();
        let mut s = RecordStore::with_limits(home, 1);
        s.put(near.clone(), NOW).unwrap();

        // Find some record further from home than `near` — which is at distance
        // zero, so any other will do.
        let far = record(9, 1, 1_000, &[], 0);
        assert_eq!(s.put(far.clone(), NOW).unwrap(), PutOutcome::Rejected);
        assert!(s.get(&near.name).is_some(), "the nearer record stayed");
        assert!(s.get(&far.name).is_none());
    }

    fn short_lived(seed: u8, at: u64, ttl: u32) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let mut rec = record(seed, 1, at, &[], 0).with_ttl(ttl);
        rec.sign(&key).unwrap();
        rec
    }

    /// The seeds `1..=9` ordered nearest first by distance from `home`.
    fn by_distance(home: &NameKey) -> Vec<u8> {
        let mut seeds: Vec<u8> = (1..=9).collect();
        seeds.sort_by_key(|s| home.distance(&record(*s, 1, 1_000, &[], 0).name.key()));
        seeds
    }

    /// Lapsed records do not hold a place against a live one: a store full of
    /// them admits a record further from home than every one of them, which the
    /// distance rule alone would refuse.
    #[test]
    fn a_store_full_of_lapsed_records_admits_a_further_live_record() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 2);
        for &seed in &order[0..2] {
            s.put(short_lived(seed, 1_000, 60), NOW).unwrap();
        }
        assert_eq!(s.len(), 2);

        let later = 2_000;
        let far = short_lived(order[8], later, 600);
        assert_eq!(s.put(far.clone(), later).unwrap(), PutOutcome::Inserted);
        assert_eq!(s.len(), 1, "both lapsed records were dropped, one admitted");
        assert!(s.get(&far.name).is_some());
        assert!(s.get(&short_lived(order[0], 1_000, 60).name).is_none());
    }

    /// The nearer record is admitted too, and the lapsed ones are gone either
    /// way.
    #[test]
    fn a_store_full_of_lapsed_records_admits_a_nearer_live_record() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 2);
        for &seed in &order[4..6] {
            s.put(short_lived(seed, 1_000, 60), NOW).unwrap();
        }
        let later = 2_000;
        let near = short_lived(order[0], later, 600);
        assert_eq!(s.put(near.clone(), later).unwrap(), PutOutcome::Inserted);
        assert_eq!(s.len(), 1);
        assert!(s.get(&near.name).is_some());
    }

    /// Only lapsed records go: with one live and one lapsed record held, the
    /// lapsed one makes the room and the live one stays.
    #[test]
    fn dropping_lapsed_records_leaves_the_live_ones() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 2);
        let live = short_lived(order[1], 1_000, 5_000);
        s.put(live.clone(), NOW).unwrap();
        s.put(short_lived(order[2], 1_000, 60), NOW).unwrap();

        let later = 2_000;
        let far = short_lived(order[8], later, 600);
        assert_eq!(s.put(far.clone(), later).unwrap(), PutOutcome::Inserted);
        assert_eq!(s.len(), 2);
        assert!(s.get(&live.name).is_some(), "the live record was kept");
        assert!(s.get(&far.name).is_some());
    }

    /// An unexpired tombstone is not given up to make room by this path: the
    /// full store still refuses a record further than everything it holds.
    #[test]
    fn an_unexpired_tombstone_survives_the_lapsed_record_drop() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 2);
        let tomb = tombstone(order[0], 2, 1_000);
        s.put(tomb.clone(), NOW).unwrap();
        s.put(short_lived(order[1], 1_000, 60), NOW).unwrap();

        let later = 2_000;
        assert!(!tomb.is_expired(later));
        let far = short_lived(order[7], later, 600);
        assert_eq!(s.put(far.clone(), later).unwrap(), PutOutcome::Inserted);
        assert!(s.is_revoked(&tomb.name, later), "the tombstone stayed");
        assert_eq!(s.len(), 2);

        // Full again, of one tombstone and one live record: nothing lapsed, so
        // a record further than both is refused and neither is dropped.
        let further = short_lived(order[8], later, 600);
        assert_eq!(s.put(further, later).unwrap(), PutOutcome::Rejected);
        assert!(s.is_revoked(&tomb.name, later));
        assert!(s.get(&far.name).is_some());
        assert_eq!(s.len(), 2);
    }

    /// An expired tombstone is a lapsed record like any other.
    #[test]
    fn an_expired_tombstone_is_dropped_to_make_room() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 1);
        let tomb = tombstone(order[0], 2, 1_000);
        s.put(tomb.clone(), NOW).unwrap();

        let later = 1_000 + u64::from(crate::MAX_RECORD_TTL_SECS);
        assert!(tomb.is_expired(later));
        let far = short_lived(order[8], later, 600);
        assert_eq!(s.put(far.clone(), later).unwrap(), PutOutcome::Inserted);
        assert!(s.get(&tomb.name).is_none());
        assert_eq!(s.len(), 1);
    }

    /// A store with no home evicts the oldest-published record, and refuses an
    /// arrival older than everything held.
    #[test]
    fn a_store_without_a_home_evicts_the_oldest_published() {
        let mut s = RecordStore::with_capacity(2);
        assert_eq!(s.capacity(), 2);
        let oldest = record(1, 1, 1_000, &[], 0);
        let middle = record(2, 1, 1_100, &[], 0);
        s.put(oldest.clone(), 1_100).unwrap();
        s.put(middle.clone(), 1_100).unwrap();

        let newest = record(3, 1, 1_200, &[], 0);
        assert_eq!(s.put(newest.clone(), 1_200).unwrap(), PutOutcome::Inserted);
        assert_eq!(s.len(), 2);
        assert!(s.get(&oldest.name).is_none(), "the oldest was given up");
        assert!(s.get(&middle.name).is_some());

        let older = record(4, 1, 1_050, &[], 0);
        assert_eq!(s.put(older.clone(), 1_200).unwrap(), PutOutcome::Rejected);
        assert!(s.get(&older.name).is_none());
        assert_eq!(s.len(), 2);
    }

    // ---- tombstones under pressure (spec 02, 10.10 decision 52) --------------

    /// The store clock for these tests: after every `published_at` used below.
    const LATE: u64 = 1_100;

    fn tomb_at(seed: u8, at: u64) -> NameRecord {
        tombstone(seed, 2, at)
    }

    /// Case 1: while a live record can go, no tombstone is chosen, however far
    /// it is; and an arrival further than every live record is refused whatever
    /// tombstones are held.
    #[test]
    fn a_tombstone_is_not_chosen_while_a_live_record_can_go() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 4);
        // The furthest of all is the tombstone.
        s.put(tomb_at(order[8], 1_000), LATE).unwrap();
        for &seed in &order[3..6] {
            s.put(record(seed, 1, 1_000, &[], 0), LATE).unwrap();
        }
        assert_eq!(s.len(), 4);

        let near = record(order[0], 1, 1_000, &[], 0);
        assert_eq!(s.put(near.clone(), LATE).unwrap(), PutOutcome::Inserted);
        assert!(s.is_revoked(&tomb_at(order[8], 1_000).name, LATE));
        assert!(s.get(&record(order[5], 1, 1_000, &[], 0).name).is_none());
        assert_eq!(s.len(), 4);

        // Further than every live record (but nearer than the tombstone): refused,
        // exactly as in a store with no tombstone.
        let beyond = record(order[7], 1, 1_000, &[], 0);
        assert_eq!(s.put(beyond.clone(), LATE).unwrap(), PutOutcome::Rejected);
        assert!(s.get(&beyond.name).is_none());
        assert!(s.is_revoked(&tomb_at(order[8], 1_000).name, LATE));
        assert_eq!(s.len(), 4);
    }

    /// Case 2: a tombstone arriving at a store full of nearer live records is
    /// admitted, and the furthest live record goes.
    #[test]
    fn a_tombstone_arriving_at_a_full_store_evicts_the_furthest_live_record() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 4);
        for &seed in &order[0..4] {
            s.put(record(seed, 1, 1_000, &[], 0), LATE).unwrap();
        }
        let far = tomb_at(order[8], 1_000);
        assert_eq!(s.put(far.clone(), LATE).unwrap(), PutOutcome::Inserted);
        assert!(s.is_revoked(&far.name, LATE));
        assert!(s.get(&record(order[3], 1, 1_000, &[], 0).name).is_none());
        for &seed in &order[0..3] {
            assert!(s.get(&record(seed, 1, 1_000, &[], 0).name).is_some());
        }
        assert_eq!((s.len(), s.tombstone_count()), (4, 1));
    }

    /// Case 3: a tombstone that replaces a record of its own name is an update:
    /// it evicts nothing for room, counts toward the share, and if that exceeds
    /// it the oldest other tombstone goes, never the one just stored.
    #[test]
    fn a_tombstone_replacing_its_own_record_counts_toward_the_share_and_spares_itself() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 4);
        assert_eq!(s.tombstone_limit(), 2);
        s.put(tomb_at(order[1], 1_000), LATE).unwrap();
        s.put(tomb_at(order[2], 1_010), LATE).unwrap();
        s.put(record(order[3], 1, 1_000, &[], 0), LATE).unwrap();
        s.put(record(order[4], 1, 1_000, &[], 0), LATE).unwrap();
        assert_eq!((s.len(), s.tombstone_count()), (4, 2));

        // The store is full and at its share, yet this is an update: nothing is
        // evicted for room. It is the newest tombstone, and the oldest other one
        // (not itself, which would be the newest) is the one that goes.
        let newest = tomb_at(order[4], 1_020);
        assert_eq!(s.put(newest.clone(), LATE).unwrap(), PutOutcome::Updated);
        assert!(
            s.is_revoked(&newest.name, LATE),
            "the one just stored stays"
        );
        assert!(s.get(&tomb_at(order[1], 1_000).name).is_none());
        assert!(s.get(&tomb_at(order[2], 1_010).name).is_some());
        assert!(s.get(&record(order[3], 1, 1_000, &[], 0).name).is_some());
        assert_eq!((s.len(), s.tombstone_count()), (3, 2));

        // Even when the new tombstone is the oldest, it is the other that goes.
        let mut t = RecordStore::with_limits(home, 4);
        t.put(tomb_at(order[1], 1_010), LATE).unwrap();
        t.put(tomb_at(order[2], 1_020), LATE).unwrap();
        t.put(record(order[3], 1, 1_000, &[], 0), LATE).unwrap();
        let oldest = tomb_at(order[3], 1_000);
        assert_eq!(t.put(oldest.clone(), LATE).unwrap(), PutOutcome::Updated);
        assert!(t.is_revoked(&oldest.name, LATE));
        assert!(t.get(&tomb_at(order[1], 1_010).name).is_none());
        assert_eq!((t.len(), t.tombstone_count()), (2, 2));
    }

    /// Case 4: tombstones at their share leave the rest of the capacity to live
    /// records, and one more tombstone evicts a tombstone, not a live record.
    #[test]
    fn tombstones_at_their_share_leave_room_for_live_records() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 4);
        s.put(tomb_at(order[6], 1_000), LATE).unwrap();
        s.put(tomb_at(order[7], 1_010), LATE).unwrap();
        for &seed in &order[0..2] {
            assert_eq!(
                s.put(record(seed, 1, 1_000, &[], 0), LATE).unwrap(),
                PutOutcome::Inserted
            );
        }
        assert_eq!((s.len(), s.tombstone_count()), (4, 2));

        let third = tomb_at(order[8], 1_020);
        assert_eq!(s.put(third.clone(), LATE).unwrap(), PutOutcome::Inserted);
        assert!(
            s.get(&tomb_at(order[6], 1_000).name).is_none(),
            "oldest went"
        );
        assert!(s.is_revoked(&third.name, LATE));
        for &seed in &order[0..2] {
            assert!(s.get(&record(seed, 1, 1_000, &[], 0).name).is_some());
        }
        assert_eq!((s.len(), s.tombstone_count()), (4, 2));
    }

    /// A flood of tombstones never holds more than its share, and live records
    /// stay.
    #[test]
    fn a_tombstone_flood_cannot_exceed_its_share() {
        let mut s = RecordStore::with_capacity(4);
        let live = record(1, 1, 1_000, &[], 0);
        s.put(live.clone(), LATE).unwrap();
        for seed in 2..=9u8 {
            s.put(tomb_at(seed, 1_000 + u64::from(seed)), LATE).unwrap();
            assert!(s.tombstone_count() <= s.tombstone_limit());
            assert!(s.get(&live.name).is_some());
        }
        assert_eq!(s.tombstone_count(), 2);
        for seed in [8u8, 9] {
            assert!(s.get(&tomb_at(seed, 0).name).is_some(), "the newest stay");
        }
    }

    /// A one-place store: the tombstone is within its share, and a live arrival
    /// still takes the place when no live record is held.
    #[test]
    fn a_live_record_can_always_be_admitted_over_tombstones_alone() {
        let mut s = RecordStore::with_capacity(1);
        s.put(tomb_at(1, 1_000), LATE).unwrap();
        let live = record(2, 1, 1_000, &[], 0);
        assert_eq!(s.put(live.clone(), LATE).unwrap(), PutOutcome::Inserted);
        assert!(s.get(&live.name).is_some());
        assert_eq!(s.tombstone_count(), 0);
    }

    /// Case 5: a lapsed tombstone is dropped by the sweep before any live
    /// record is evicted for room.
    #[test]
    fn a_lapsed_tombstone_goes_before_a_live_record() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let mut s = RecordStore::with_limits(home, 2);
        s.put(tomb_at(order[0], 1_000), LATE).unwrap();
        let later = 1_000 + u64::from(crate::MAX_RECORD_TTL_SECS);
        let far = short_lived(order[7], later, 600);
        s.put(far.clone(), later).unwrap();
        assert_eq!(s.len(), 2);

        let near = short_lived(order[1], later, 600);
        assert_eq!(s.put(near.clone(), later).unwrap(), PutOutcome::Inserted);
        assert!(s.get(&tomb_at(order[0], 1_000).name).is_none());
        assert!(s.get(&far.name).is_some(), "the live record was kept");
        assert!(s.get(&near.name).is_some());

        // Likewise for a tombstone arriving at a lapsed tombstone's share.
        let mut t = RecordStore::with_limits(home, 2);
        t.put(tomb_at(order[0], 1_000), LATE).unwrap();
        t.put(short_lived(order[7], later, 600), later).unwrap();
        let fresh = tombstone(order[2], 2, later);
        assert_eq!(t.put(fresh.clone(), later).unwrap(), PutOutcome::Inserted);
        assert!(t.get(&tomb_at(order[0], 1_000).name).is_none());
        assert!(t.get(&short_lived(order[7], later, 600).name).is_some());
    }

    /// Case 6: the victim does not depend on the order records arrived in, nor
    /// on the map's order; equal publication times fall to the smaller key.
    #[test]
    fn the_victim_choice_is_deterministic() {
        let home = NameKey::of(b"home");
        let order = by_distance(&home);
        let live = [order[2], order[5]];
        let tomb = tomb_at(order[8], 1_000);
        let near = record(order[0], 1, 1_000, &[], 0);
        let mut results = Vec::new();
        for reverse in [false, true] {
            let mut items = vec![
                record(live[0], 1, 1_000, &[], 0),
                record(live[1], 1, 1_000, &[], 0),
                tomb.clone(),
            ];
            if reverse {
                items.reverse();
            }
            let mut s = RecordStore::with_limits(home, 3);
            for item in items {
                s.put(item, LATE).unwrap();
            }
            s.put(near.clone(), LATE).unwrap();
            let mut held: Vec<NameKey> = s.records().map(|r| r.name.key()).collect();
            held.sort_by_key(|k| *k.as_bytes());
            results.push(held);
        }
        assert_eq!(results[0], results[1]);
        assert!(results[0].contains(&tomb.name.key()));
        assert!(!results[0].contains(&record(live[1], 1, 1_000, &[], 0).name.key()));

        // Tombstones published at the same time: the smaller key goes first, in
        // either arrival order.
        let (a, b, c) = (tomb_at(1, 1_000), tomb_at(2, 1_000), tomb_at(3, 1_000));
        let smaller = [&a, &b]
            .into_iter()
            .min_by_key(|r| *r.name.key().as_bytes())
            .unwrap()
            .clone();
        for pair in [[&a, &b], [&b, &a]] {
            let mut s = RecordStore::with_capacity(4);
            for r in pair {
                s.put(r.clone(), LATE).unwrap();
            }
            s.put(c.clone(), LATE).unwrap();
            assert!(s.get(&smaller.name).is_none());
            assert_eq!(s.tombstone_count(), 2);
        }
    }
}
