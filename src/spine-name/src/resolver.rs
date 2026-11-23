//! Resolution — turning a name into something an agent can act on.
//!
//! The interface is deliberately shaped around how agents actually resolve,
//! which differs from how browsers did:
//!
//! - **Batch by default.** An agent planning a task discovers a dozen names at
//!   once. [`Resolver::resolve_many`] exists so a transport can answer them in
//!   one round trip instead of twelve; the human web's one-URL-per-request
//!   default is why agent frameworks all end up hand-rolling a fan-out pool.
//! - **Capability resolution is a first-class verb.** [`Resolver::find_providers`]
//!   answers "who can do this" without a central index.
//! - **Resolution reports its own provenance.** A caller can tell a fresh cache
//!   hit from a stale one from a network answer, because an agent deciding
//!   whether to retry needs to know which it got.

use async_trait::async_trait;
use std::sync::Mutex;

use crate::cache::{CacheLookup, CacheStats, ResolverCache};
use crate::logbook::{LogChange, LogOffer};
use crate::record::NameRecord;
use crate::store::RecordStore;
use crate::succession::{Equivocation, State as SuccessionState, SuccessionLog};
use crate::uri::{Authority, SpineUri};
use crate::NameError;

/// Where a resolution came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Served fresh from cache — no round trip.
    Cache,
    /// Served from cache past its TTL, with a refresh advisable.
    StaleCache,
    /// Served from the local record store (this node publishes or holds it).
    Local,
    /// Fetched from the network.
    Network,
    /// Read straight out of a `host:` name, which carries its own address.
    ///
    /// Kept distinct from every other provenance because it is the only one that
    /// involved no attestation at all: nobody signed it and nobody was asked.
    /// A caller that treats this like a resolved `did:` name has silently
    /// dropped the namespace's trust model on the floor, so the distinction has
    /// to survive all the way to the caller rather than being flattened into
    /// `Local`.
    Address,
}

/// A resolved name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub record: NameRecord,
    pub provenance: Provenance,
}

impl Resolution {
    pub fn new(record: NameRecord, provenance: Provenance) -> Self {
        Self { record, provenance }
    }

    /// Whether the answer is known-current rather than possibly outdated.
    pub fn is_fresh(&self) -> bool {
        !matches!(self.provenance, Provenance::StaleCache)
    }

    /// Whether anyone actually vouched for this binding.
    ///
    /// False for a `host:` name, whose "resolution" is just its own address read
    /// back. Worth checking before treating a resolution as authoritative.
    pub fn is_attested(&self) -> bool {
        !matches!(self.provenance, Provenance::Address) && self.record.is_attested()
    }
}

/// Anything that can turn a name into a record.
#[async_trait]
pub trait Resolver: Send + Sync {
    /// Resolve one name.
    async fn resolve(&self, uri: &SpineUri) -> Result<Resolution, NameError>;

    /// Resolve many names.
    ///
    /// The default implementation walks them in order; a network-backed
    /// resolver should override this to issue one batched round trip. Each
    /// result is independent, so one bad name never fails the batch.
    async fn resolve_many(&self, uris: &[SpineUri]) -> Vec<Result<Resolution, NameError>> {
        let mut out = Vec::with_capacity(uris.len());
        for uri in uris {
            out.push(self.resolve(uri).await);
        }
        out
    }

    /// Find records advertising a capability term, best first.
    async fn find_providers(&self, term: &str) -> Result<Vec<NameRecord>, NameError>;
}

/// Storage namespace of the persisted succession logs: key is the 32-byte
/// inception key, value the log as serde JSON. Validated at load: a row whose
/// log does not verify from its key is dropped.
#[cfg(feature = "storage")]
pub const SUCCESSIONS_NAMESPACE: &str = "spine-name-successions-v1";

/// Storage namespace of the persisted equivocation proofs: key is the 32-byte
/// inception key, value the proof as serde JSON. Validated at load: a row whose
/// proof does not verify is dropped.
#[cfg(feature = "storage")]
pub const EQUIVOCATIONS_NAMESPACE: &str = "spine-name-equivocations-v1";

/// A resolver backed by a local [`RecordStore`], fronted by a [`ResolverCache`].
///
/// This is the whole resolution path for a node serving its own names, and the
/// base layer under a network resolver.
pub struct LocalResolver {
    store: Mutex<RecordStore>,
    cache: Mutex<ResolverCache>,
    /// Injected clock (seconds since epoch), so freshness is testable and this
    /// crate stays free of a time dependency.
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
    #[cfg(feature = "storage")]
    persistence: Option<std::sync::Arc<dyn spine_storage::StorageBackend>>,
}

impl std::fmt::Debug for LocalResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalResolver")
            .field("store", &self.store)
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

impl LocalResolver {
    /// A resolver whose clock is supplied by the caller.
    pub fn new(clock: impl Fn() -> u64 + Send + Sync + 'static) -> Self {
        Self {
            store: Mutex::new(RecordStore::new()),
            cache: Mutex::new(ResolverCache::new(4096)),
            clock: Box::new(clock),
            #[cfg(feature = "storage")]
            persistence: None,
        }
    }

    /// A resolver with a fixed clock — for tests and deterministic replay.
    pub fn at_time(now: u64) -> Self {
        Self::new(move || now)
    }

    /// Restore publisher records from a durable backend. Signatures and keys
    /// are checked before accepting the snapshot. Expired records retain their
    /// sequence in storage for owner-driven renewal, but never resolve fresh.
    /// Learned cache entries, replicas and negative results are not persisted.
    #[cfg(feature = "storage")]
    pub fn persistent(
        clock: impl Fn() -> u64 + Send + Sync + 'static,
        backend: std::sync::Arc<dyn spine_storage::StorageBackend>,
    ) -> Result<Self, NameError> {
        let mut resolver = Self::new(clock);
        // Proofs and logs first, so that records of a rotated name are admitted
        // against the log they were signed under. A row that does not verify,
        // or that does not fit the bounds, is dropped.
        {
            let book = resolver.store.get_mut().unwrap();
            let proofs = backend
                .scan(EQUIVOCATIONS_NAMESPACE, None)
                .map_err(|e| NameError::Storage(e.to_string()))?;
            for (key, data) in proofs {
                if let (Ok(key), Ok(proof)) = (
                    <[u8; 32]>::try_from(key.as_slice()),
                    serde_json::from_slice::<Equivocation>(&data),
                ) {
                    book.restore_proof(key, proof);
                }
            }
            let logs = backend
                .scan(SUCCESSIONS_NAMESPACE, None)
                .map_err(|e| NameError::Storage(e.to_string()))?;
            for (key, data) in logs {
                if let (Ok(key), Ok(log)) = (
                    <[u8; 32]>::try_from(key.as_slice()),
                    serde_json::from_slice::<SuccessionLog>(&data),
                ) {
                    book.restore_log(key, log);
                }
            }
        }
        let records = backend
            .scan("spine-name-publishers-v1", None)
            .map_err(|e| NameError::Storage(e.to_string()))?;
        if records.len() > crate::DEFAULT_CAPACITY {
            return Err(NameError::Capacity);
        }
        for (key, data) in records {
            let record: NameRecord =
                serde_json::from_slice(&data).map_err(|e| NameError::Storage(e.to_string()))?;
            if key != record.name.to_string().as_bytes() {
                return Err(NameError::Storage(
                    "stored key does not match record name".into(),
                ));
            }
            // A record that no longer verifies (for example one signed under
            // an older signing-bytes layout) must not stop the node starting.
            // It is skipped here, never served, and replaced by the owner's
            // next publication. The same goes for a record that breaks the
            // lifetime rules (a `ttl_secs` over the maximum, written before the
            // bound existed) or is post-dated against this node's clock.
            let admitted = resolver.store.get_mut().unwrap().logs().authorise(&record);
            if record.validate(resolver.now()).is_err() || admitted.is_err() {
                continue;
            }
            if !record.is_expired(resolver.now()) {
                resolver.publish(record)?;
            }
        }
        resolver.persistence = Some(backend);
        Ok(resolver)
    }

    /// All records owned by this publisher, including expired sequence markers.
    #[cfg(feature = "storage")]
    pub fn published_records(&self) -> Result<Vec<NameRecord>, NameError> {
        let Some(backend) = &self.persistence else {
            return Ok(Vec::new());
        };
        let store = self.store.lock().unwrap();
        backend
            .scan("spine-name-publishers-v1", None)
            .map_err(|e| NameError::Storage(e.to_string()))?
            .into_iter()
            .map(|(_, bytes)| {
                serde_json::from_slice::<NameRecord>(&bytes)
                    .map_err(|e| NameError::Storage(e.to_string()))
            })
            .filter(|parsed| match parsed {
                Ok(record) => store.logs().authorise(record).is_ok(),
                Err(_) => true,
            })
            .collect()
    }

    /// Current time according to the injected clock.
    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    /// Publish a record into the local store. Verification happens in the store,
    /// so an invalid record cannot be published. A record that fails
    /// [`NameRecord::validate`] against this resolver's clock (too large, a
    /// `ttl_secs` over the maximum, a malformed tombstone, or `published_at`
    /// past the allowed skew) is refused. Publishing a tombstone withdraws the
    /// name: later resolutions fail with [`NameError::Revoked`] and a record of
    /// the same or lower `seq` is refused with `StalePublication`.
    pub fn publish(&self, record: NameRecord) -> Result<(), NameError> {
        record.validate(self.now())?;
        let mut store = self.store.lock().unwrap();
        // The admission check: exactly `verify` for a name with no succession
        // log (see `LogBook::authorise`).
        store.logs().authorise(&record)?;
        if let Some(previous) = store.get(&record.name) {
            if record != *previous && !record.supersedes(previous) {
                return Err(NameError::StalePublication);
            }
        }
        #[cfg(feature = "storage")]
        if let Some(backend) = &self.persistence {
            let name = record.name.to_string();
            if let Some(bytes) = backend
                .get("spine-name-publishers-v1", name.as_bytes())
                .map_err(|e| NameError::Storage(e.to_string()))?
            {
                let previous: NameRecord = serde_json::from_slice(&bytes)
                    .map_err(|e| NameError::Storage(e.to_string()))?;
                // A stored predecessor that no longer verifies cannot hold the
                // sequence number against a valid record.
                if store.logs().authorise(&previous).is_ok() && !record.supersedes(&previous) {
                    return if record == previous {
                        Ok(())
                    } else {
                        Err(NameError::StalePublication)
                    };
                }
            } else if backend
                .count("spine-name-publishers-v1")
                .map_err(|e| NameError::Storage(e.to_string()))?
                >= crate::DEFAULT_CAPACITY
            {
                return Err(NameError::Capacity);
            }
            let mut candidate = store.clone();
            if candidate.put(record.clone(), self.now())? == crate::PutOutcome::Rejected {
                return Err(NameError::Capacity);
            }
            let bytes =
                serde_json::to_vec(&record).map_err(|e| NameError::Storage(e.to_string()))?;
            backend
                .put("spine-name-publishers-v1", name.as_bytes(), &bytes)
                .map_err(|e| NameError::Storage(e.to_string()))?;
            *store = candidate;
            drop(store);
            self.cache.lock().unwrap().remove(&record.name);
            return Ok(());
        }
        let name = record.name.clone();
        if store.put(record, self.now())? == crate::PutOutcome::Rejected {
            return Err(NameError::Capacity);
        }
        drop(store);
        self.cache.lock().unwrap().remove(&name);
        Ok(())
    }

    /// Seed the cache with a record learned elsewhere (e.g. from the mesh).
    /// The record is checked like a published one ([`NameRecord::validate`]
    /// against this resolver's clock, then its signature). A tombstone is
    /// cached too, so a revoked name stays revoked here.
    pub fn cache_record(&self, record: NameRecord) -> Result<(), NameError> {
        record.validate(self.now())?;
        let admitted = self.store.lock().unwrap().logs().authorise(&record);
        admitted?;
        self.cache.lock().unwrap().put(record);
        Ok(())
    }

    /// Offer a succession log of a name (spec 02, section 10): learned from a
    /// publisher, a peer or a file. The log is verified from its inception key;
    /// the resolver keeps the best one per name, replaced only by a valid strict
    /// extension, and a valid conflicting log is kept as an equivocation proof,
    /// after which the name fails closed (`resolve` answers
    /// [`NameError::Equivocated`]). A retired name resolves as
    /// [`NameError::Retired`]. Stored and cached records the new log no longer
    /// authorises are dropped. With a backend the log (or proof) is written to
    /// the `spine-name-successions-v1` (or `-equivocations-v1`) namespace before
    /// it takes effect. An empty log, one the resolver already holds or a prefix
    /// of it changes nothing and does no signature work.
    ///
    /// Errors: [`NameError::InvalidSuccessionLog`] for a log that does not
    /// verify, [`NameError::SuccessionCapacity`] when the table is full and
    /// every held log has records held (or the proof table is full).
    pub fn offer_log(&self, log: SuccessionLog) -> Result<LogChange, NameError> {
        let mut store = self.store.lock().unwrap();
        let offer = store.plan_log(log)?;
        if matches!(offer, LogOffer::Full | LogOffer::ProofsFull) {
            return Err(NameError::SuccessionCapacity);
        }
        #[cfg(feature = "storage")]
        self.persist_offer(&offer)?;
        let (change, _) = store.apply_log(offer);
        drop(store);
        if let LogChange::Adopted { inception_key } | LogChange::Compromised { inception_key } =
            change
        {
            self.cache.lock().unwrap().remove_authority(&inception_key);
        }
        Ok(change)
    }

    /// Publish a record that arrived with its name's succession log (what
    /// `spine name publish --log` sends): the log is offered first
    /// ([`LocalResolver::offer_log`]), then the record is published against the
    /// best log held. A log for another name than the record's is refused with
    /// [`NameError::InvalidSuccessionLog`] and nothing changes. A valid log is
    /// kept even when the record is then refused: it is true on its own.
    pub fn publish_with_log(
        &self,
        record: NameRecord,
        log: SuccessionLog,
    ) -> Result<(), NameError> {
        if let Some(key) = crate::logbook::log_inception_key(&log) {
            if record.name.public_key() != Some(&key) {
                return Err(NameError::InvalidSuccessionLog(
                    "log is for another name".to_string(),
                ));
            }
        }
        self.offer_log(log)?;
        self.publish(record)
    }

    #[cfg(feature = "storage")]
    fn persist_offer(&self, offer: &LogOffer) -> Result<(), NameError> {
        let Some(backend) = &self.persistence else {
            return Ok(());
        };
        let storage = |e: &dyn std::fmt::Display| NameError::Storage(e.to_string());
        match offer {
            LogOffer::Adopt {
                inception_key,
                log,
                evict,
                ..
            } => {
                let bytes = serde_json::to_vec(log).map_err(|e| storage(&e))?;
                backend
                    .put(SUCCESSIONS_NAMESPACE, inception_key, &bytes)
                    .map_err(|e| storage(&e))?;
                if let Some(old) = evict {
                    backend
                        .delete(SUCCESSIONS_NAMESPACE, old)
                        .map_err(|e| storage(&e))?;
                }
            }
            LogOffer::Conflict(proof) => {
                let key = proof.first.inception_key;
                let bytes = serde_json::to_vec(proof.as_ref()).map_err(|e| storage(&e))?;
                backend
                    .put(EQUIVOCATIONS_NAMESPACE, &key, &bytes)
                    .map_err(|e| storage(&e))?;
                backend
                    .delete(SUCCESSIONS_NAMESPACE, &key)
                    .map_err(|e| storage(&e))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// The state of the best succession log held for the name whose inception
    /// key is `inception_key`, if any (none: the inception key signs).
    pub fn succession_state(&self, inception_key: &[u8; 32]) -> Option<SuccessionState> {
        self.store
            .lock()
            .unwrap()
            .logs()
            .state(inception_key)
            .cloned()
    }

    /// The best succession log held for a name, if any: what a publisher sends
    /// with the name's records.
    pub fn succession_log(&self, inception_key: &[u8; 32]) -> Option<SuccessionLog> {
        self.store
            .lock()
            .unwrap()
            .logs()
            .log(inception_key)
            .cloned()
    }

    /// The equivocation proof held for a name, if any.
    pub fn equivocation(&self, inception_key: &[u8; 32]) -> Option<Equivocation> {
        self.store
            .lock()
            .unwrap()
            .logs()
            .proof(inception_key)
            .cloned()
    }

    /// Whether this node holds a fresh tombstone for `uri` in its store.
    /// Owner renewal consults this so it never renews a revoked name.
    pub fn is_revoked(&self, uri: &SpineUri) -> bool {
        let now = self.now();
        self.store.lock().unwrap().is_revoked(uri, now)
    }

    /// Remember that a name did not resolve.
    pub fn cache_negative(&self, uri: &SpineUri) {
        let now = self.now();
        self.cache.lock().unwrap().put_negative(uri, now);
    }

    /// Whether a still-valid negative entry covers `uri`.
    ///
    /// Lets a caller tell a remembered absence from a plain miss, both of which
    /// `resolve` reports as `NotFound`. Only the former may skip a mesh walk.
    pub fn is_negative_cached(&self, uri: &SpineUri) -> bool {
        let now = self.now();
        self.cache.lock().unwrap().is_negative(uri, now)
    }

    /// Cache statistics.
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.lock().unwrap().stats()
    }

    /// Records this node holds.
    pub fn local_len(&self) -> usize {
        self.store.lock().unwrap().len()
    }

    /// Drop everything expired, from both store and cache.
    pub fn sweep(&self) -> usize {
        let now = self.now();
        let swept = self.store.lock().unwrap().sweep_expired(now);
        swept + self.cache.lock().unwrap().sweep(now)
    }

    /// Records within `window` seconds of lapsing, so a publisher can re-sign
    /// them before they do.
    pub fn needing_republish(&self, window: u64) -> Vec<NameRecord> {
        let now = self.now();
        self.store
            .lock()
            .unwrap()
            .needing_republish(now, window)
            .into_iter()
            .cloned()
            .collect()
    }
}

#[async_trait]
impl Resolver for LocalResolver {
    async fn resolve(&self, uri: &SpineUri) -> Result<Resolution, NameError> {
        // A capability name has no single record — it is a set, and resolving it
        // as if it were one name would quietly return an arbitrary member.
        if matches!(uri.authority(), Authority::Capability(_)) {
            return Err(NameError::NotASingleName(uri.to_string()));
        }

        let now = self.now();

        // A `host:` name resolves without consulting anything: the address is
        // in the name. Answered before the cache, since caching a restatement
        // of the input would only add a way for it to go stale.
        if matches!(uri.authority(), Authority::Host { .. }) {
            let record = NameRecord::for_host(uri.clone(), now, crate::DEFAULT_TTL_SECS)?;
            return Ok(Resolution::new(record, Provenance::Address));
        }

        // A name whose succession log retires it, or two of whose logs conflict,
        // answers with that, whatever the cache or store holds. For a name with
        // no log the book is empty and this is one comparison.
        let status = self.store.lock().unwrap().logs().status_error(uri);
        if let Some(error) = status {
            return Err(error);
        }

        // A fresh tombstone, cached or stored, is a distinct answer: the name was
        // withdrawn by its owner, which is not the same as nobody having heard
        // of it.
        match self.cache.lock().unwrap().get(uri, now) {
            CacheLookup::Fresh(record) => {
                if record.is_tombstone() {
                    return Err(NameError::Revoked(uri.to_string()));
                }
                return Ok(Resolution::new(record, Provenance::Cache));
            }
            CacheLookup::NegativeHit => return Err(NameError::NotFound(uri.to_string())),
            // A stale entry is a fallback, not an answer: fall through to the
            // store first, and only use it if nothing better exists.
            CacheLookup::Stale(record) => {
                if let Some(local) = self.store.lock().unwrap().get_fresh(uri, now) {
                    if local.is_tombstone() {
                        return Err(NameError::Revoked(uri.to_string()));
                    }
                    return Ok(Resolution::new(local.clone(), Provenance::Local));
                }
                // A lapsed tombstone is not an answer: the name is unknown again.
                if record.is_tombstone() {
                    return Err(NameError::NotFound(uri.to_string()));
                }
                return Ok(Resolution::new(record, Provenance::StaleCache));
            }
            CacheLookup::Miss => {}
        }

        if let Some(record) = self.store.lock().unwrap().get_fresh(uri, now) {
            if record.is_tombstone() {
                return Err(NameError::Revoked(uri.to_string()));
            }
            return Ok(Resolution::new(record.clone(), Provenance::Local));
        }
        Err(NameError::NotFound(uri.to_string()))
    }

    async fn find_providers(&self, term: &str) -> Result<Vec<NameRecord>, NameError> {
        let now = self.now();
        Ok(self
            .store
            .lock()
            .unwrap()
            .providers_of(term, now)
            .into_iter()
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Endpoint;
    use ed25519_dalek::SigningKey;

    fn signed(seed: u8, now: u64, ttl: u32, caps: &[&str], endpoints: usize) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, 1, now).unwrap().with_ttl(ttl);
        for c in caps {
            rec = rec.with_capability(*c);
        }
        for i in 0..endpoints {
            rec = rec.with_endpoint(Endpoint::new("tcp", format!("h{i}:9440")));
        }
        rec.sign(&key).unwrap();
        rec
    }

    fn tombstone(seed: u8, seq: u64, at: u64) -> NameRecord {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::tombstone(name, seq, at).unwrap();
        rec.sign(&key).unwrap();
        rec
    }

    #[test]
    fn publishing_refuses_an_over_long_or_post_dated_record() {
        let r = LocalResolver::at_time(1_000);
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut over = NameRecord::new(name, 1, 1_000)
            .unwrap()
            .with_ttl(crate::MAX_RECORD_TTL_SECS + 1);
        let sig: ed25519_dalek::Signature =
            ed25519_dalek::Signer::sign(&key, &over.signing_bytes());
        over.signature = sig.to_bytes().to_vec();
        assert!(matches!(
            r.publish(over.clone()),
            Err(NameError::TtlTooLong(_))
        ));
        assert!(matches!(
            r.cache_record(over),
            Err(NameError::TtlTooLong(_))
        ));

        let ahead = signed(1, 1_000 + crate::MAX_CLOCK_SKEW_SECS + 1, 100, &[], 1);
        assert!(matches!(
            r.publish(ahead.clone()),
            Err(NameError::PostDated { .. })
        ));
        assert!(matches!(
            r.cache_record(ahead),
            Err(NameError::PostDated { .. })
        ));
        assert_eq!(r.local_len(), 0);
        r.publish(signed(1, 1_000 + crate::MAX_CLOCK_SKEW_SECS, 100, &[], 1))
            .unwrap();
    }

    #[tokio::test]
    async fn a_published_tombstone_resolves_as_revoked_not_not_found() {
        let r = LocalResolver::at_time(1_000);
        let rec = signed(1, 1_000, 100, &["web.search"], 1);
        r.publish(rec.clone()).unwrap();
        r.cache_record(rec.clone()).unwrap();
        assert!(r.resolve(&rec.name).await.is_ok());

        let t = tombstone(1, 2, 1_000);
        r.publish(t.clone()).unwrap();
        assert!(r.is_revoked(&rec.name));
        assert!(matches!(
            r.resolve(&rec.name).await,
            Err(NameError::Revoked(_))
        ));
        assert!(r.find_providers("web.search").await.unwrap().is_empty());

        // The old record, and a same-seq one, can neither replace it nor be
        // re-published; a higher seq re-publishes the name.
        assert_eq!(r.publish(rec.clone()), Err(NameError::StalePublication));
        r.publish(t.clone()).unwrap();
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut reborn = NameRecord::new(rec.name.clone(), 3, 1_000)
            .unwrap()
            .with_endpoint(Endpoint::new("tcp", "new:1"));
        reborn.sign(&key).unwrap();
        r.publish(reborn.clone()).unwrap();
        assert!(!r.is_revoked(&rec.name));
        assert_eq!(r.resolve(&rec.name).await.unwrap().record, reborn);
    }

    #[tokio::test]
    async fn a_cached_tombstone_blocks_an_older_cached_record() {
        let r = LocalResolver::at_time(1_000);
        let old = signed(1, 1_000, 100, &[], 1);
        let t = tombstone(1, 2, 1_000);
        r.cache_record(t).unwrap();
        r.cache_record(old.clone()).unwrap();
        assert!(matches!(
            r.resolve(&old.name).await,
            Err(NameError::Revoked(_))
        ));
    }

    #[tokio::test]
    async fn a_lapsed_tombstone_leaves_the_name_unknown_not_revoked() {
        let t = tombstone(1, 2, 1_000);
        let after = 1_000 + u64::from(crate::MAX_RECORD_TTL_SECS);
        let r = LocalResolver::at_time(after);
        r.cache_record(t.clone()).unwrap();
        assert!(matches!(
            r.resolve(&t.name).await,
            Err(NameError::NotFound(_))
        ));
    }

    /// The escape hatch: a `host:` name resolves with nothing published and no
    /// network, because the address is in the name.
    #[tokio::test]
    async fn a_host_name_resolves_to_the_address_it_contains() {
        let resolver = LocalResolver::at_time(1_000);
        let uri = SpineUri::parse("spine://host:seed.example.org:9440/").unwrap();

        let hit = resolver.resolve(&uri).await.unwrap();
        assert_eq!(hit.provenance, Provenance::Address);
        assert_eq!(hit.record.endpoints.len(), 1);
        assert_eq!(hit.record.endpoints[0].address, "seed.example.org:9440");
    }

    /// The point of the separate provenance: this binding is nobody's word.
    #[tokio::test]
    async fn a_host_resolution_is_not_attested() {
        let resolver = LocalResolver::at_time(1_000);
        let uri = SpineUri::parse("spine://host:seed.example.org:9440/").unwrap();

        let hit = resolver.resolve(&uri).await.unwrap();
        assert!(!hit.is_attested(), "nothing signed a hostname");
        assert!(
            hit.record.verify().is_err(),
            "and it must not pass verification either"
        );
        // A did: name, by contrast, carries a signature that stands on its own.
        let signed = signed(3, 1_000, 3_600, &[], 1);
        resolver.publish(signed.clone()).unwrap();
        assert!(resolver.resolve(&signed.name).await.unwrap().is_attested());
    }

    /// A port-less host is still usable — the transport supplies its default —
    /// so it must not be silently dropped or turned into `host:0`.
    #[tokio::test]
    async fn a_host_name_without_a_port_keeps_the_bare_host() {
        let resolver = LocalResolver::at_time(1_000);
        let uri = SpineUri::parse("spine://host:seed.example.org/").unwrap();

        let hit = resolver.resolve(&uri).await.unwrap();
        assert_eq!(hit.record.endpoints[0].address, "seed.example.org");
    }

    #[tokio::test]
    async fn resolves_a_locally_published_name() {
        let r = LocalResolver::at_time(1_000);
        let rec = signed(1, 1_000, 100, &[], 1);
        r.publish(rec.clone()).unwrap();

        let res = r.resolve(&rec.name).await.unwrap();
        assert_eq!(res.provenance, Provenance::Local);
        assert_eq!(res.record, rec);
        assert!(res.is_fresh());
    }

    #[tokio::test]
    async fn an_unknown_name_is_not_found() {
        let r = LocalResolver::at_time(1_000);
        let err = r.resolve(&SpineUri::did([9u8; 32])).await.unwrap_err();
        assert!(matches!(err, NameError::NotFound(_)));
    }

    #[tokio::test]
    async fn a_cached_record_is_served_without_touching_the_store() {
        let r = LocalResolver::at_time(1_000);
        let rec = signed(1, 1_000, 100, &[], 1);
        r.cache_record(rec.clone()).unwrap();

        let res = r.resolve(&rec.name).await.unwrap();
        assert_eq!(res.provenance, Provenance::Cache);
        assert_eq!(r.local_len(), 0, "nothing was published locally");
    }

    #[tokio::test]
    async fn caching_an_invalid_record_is_refused() {
        let r = LocalResolver::at_time(1_000);
        let mut rec = signed(1, 1_000, 100, &[], 1);
        rec.capabilities.push("admin".into());
        assert!(matches!(r.cache_record(rec), Err(NameError::BadSignature)));
    }

    #[tokio::test]
    async fn a_stale_cache_entry_is_labelled_stale_not_fresh() {
        let r = LocalResolver::at_time(2_000);
        let rec = signed(1, 1_000, 100, &[], 1); // expired by 2_000
        r.cache_record(rec.clone()).unwrap();

        let res = r.resolve(&rec.name).await.unwrap();
        assert_eq!(res.provenance, Provenance::StaleCache);
        assert!(!res.is_fresh(), "the caller must be able to tell");
    }

    #[tokio::test]
    async fn a_fresh_local_record_is_preferred_over_a_stale_cached_one() {
        let r = LocalResolver::at_time(2_000);
        let stale = signed(1, 1_000, 100, &[], 1);
        r.cache_record(stale.clone()).unwrap();

        // Same name, freshly published locally.
        let key = SigningKey::from_bytes(&[1u8; 32]);
        let mut fresh = NameRecord::new(stale.name.clone(), 2, 2_000)
            .unwrap()
            .with_ttl(100);
        fresh.sign(&key).unwrap();
        r.publish(fresh.clone()).unwrap();

        let res = r.resolve(&stale.name).await.unwrap();
        assert_eq!(res.provenance, Provenance::Local);
        assert_eq!(res.record.seq, 2);
    }

    #[tokio::test]
    async fn a_negative_entry_short_circuits_resolution() {
        let r = LocalResolver::at_time(1_000);
        let uri = SpineUri::did([9u8; 32]);
        r.cache_negative(&uri);
        assert!(matches!(r.resolve(&uri).await, Err(NameError::NotFound(_))));
        assert_eq!(r.cache_stats().negative_hits, 1);
    }

    #[tokio::test]
    async fn a_miss_is_distinguishable_from_a_remembered_absence() {
        let r = LocalResolver::at_time(1_000);
        let first = signed(1, 1_000, 100, &[], 1);
        assert!(matches!(
            r.resolve(&first.name).await,
            Err(NameError::NotFound(_))
        ));
        assert!(!r.is_negative_cached(&first.name), "a plain miss");

        r.cache_negative(&first.name);
        assert!(r.is_negative_cached(&first.name));

        // Publishing invalidates the entry, so the walk-skipping path ends.
        r.publish(first.clone()).unwrap();
        assert!(!r.is_negative_cached(&first.name));
    }

    #[tokio::test]
    async fn a_capability_name_is_not_resolvable_as_a_single_record() {
        let r = LocalResolver::at_time(1_000);
        let err = r
            .resolve(&SpineUri::capability("web.search"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, NameError::NotASingleName(_)),
            "resolving a set as a name would return an arbitrary member"
        );
    }

    #[tokio::test]
    async fn capability_lookup_ranks_providers() {
        let r = LocalResolver::at_time(1_000);
        r.publish(signed(1, 1_000, 100, &["web.search"], 1))
            .unwrap();
        r.publish(signed(2, 1_000, 100, &["web.search"], 3))
            .unwrap();
        r.publish(signed(3, 1_000, 100, &["data.analyze"], 1))
            .unwrap();

        let found = r.find_providers("web.search").await.unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].endpoints.len(), 3, "best-connected first");
        assert!(r.find_providers("nothing").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn batch_resolution_returns_one_result_per_name_independently() {
        let r = LocalResolver::at_time(1_000);
        let good = signed(1, 1_000, 100, &[], 1);
        r.publish(good.clone()).unwrap();

        let names = vec![
            good.name.clone(),
            SpineUri::did([9u8; 32]),
            SpineUri::capability("web.search"),
        ];
        let results = r.resolve_many(&names).await;
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert!(matches!(results[1], Err(NameError::NotFound(_))));
        assert!(matches!(results[2], Err(NameError::NotASingleName(_))));
    }

    #[tokio::test]
    async fn expired_records_stop_resolving_and_are_swept() {
        let r = LocalResolver::at_time(1_000);
        r.publish(signed(1, 1_000, 100, &[], 1)).unwrap();
        assert_eq!(r.local_len(), 1);

        let later = LocalResolver::at_time(2_000);
        later.publish(signed(1, 1_000, 100, &[], 1)).unwrap();
        assert!(matches!(
            later.resolve(&signed(1, 1_000, 100, &[], 1).name).await,
            Err(NameError::NotFound(_))
        ));
        assert!(later.sweep() >= 1);
        assert_eq!(later.local_len(), 0);
    }

    #[tokio::test]
    async fn republish_window_surfaces_lapsing_records() {
        let r = LocalResolver::at_time(1_080);
        r.publish(signed(1, 1_000, 100, &[], 1)).unwrap();
        assert_eq!(r.needing_republish(30).len(), 1);
        assert!(r.needing_republish(5).is_empty());
    }

    #[tokio::test]
    async fn resolver_is_usable_as_a_trait_object() {
        let r: Box<dyn Resolver> = Box::new(LocalResolver::at_time(1_000));
        assert!(r.find_providers("web.search").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn publishing_invalidates_negative_and_older_positive_cache_entries() {
        let r = LocalResolver::at_time(1_000);
        let first = signed(1, 1_000, 100, &[], 1);
        r.cache_negative(&first.name);
        r.publish(first.clone()).unwrap();
        assert_eq!(r.resolve(&first.name).await.unwrap().record.seq, first.seq);
        r.cache_record(first.clone()).unwrap();
        let next = first
            .renewed(&SigningKey::from_bytes(&[1; 32]), 1_001, 200)
            .unwrap();
        r.publish(next.clone()).unwrap();
        assert_eq!(r.resolve(&first.name).await.unwrap().record.seq, next.seq);
    }

    #[cfg(feature = "storage")]
    #[tokio::test]
    async fn publications_survive_restart_and_expiry_preserves_renewal_sequence() {
        use spine_storage::sqlite::SqliteBackend;
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("names.sqlite");
        let open = |now| {
            LocalResolver::persistent(
                move || now,
                Arc::new(SqliteBackend::open(path.to_str().unwrap()).unwrap()),
            )
            .unwrap()
        };
        let first = signed(1, 1_000, 100, &["web.search"], 1);
        {
            let r = open(1_000);
            r.publish(first.clone()).unwrap();
        }
        {
            let r = open(1_010);
            assert_eq!(r.resolve(&first.name).await.unwrap().record, first);
        }
        let r = open(1_200);
        assert!(matches!(
            r.resolve(&first.name).await,
            Err(NameError::NotFound(_))
        ));
        let owned = r.published_records().unwrap();
        assert_eq!(owned, vec![first.clone()]);
        let renewed = owned[0]
            .renewed(&SigningKey::from_bytes(&[1; 32]), 1_200, 100)
            .unwrap();
        r.publish(renewed.clone()).unwrap();
        assert_eq!(r.publish(first.clone()), Err(NameError::StalePublication));
        r.publish(renewed.clone()).unwrap();
        assert_eq!(r.resolve(&first.name).await.unwrap().record, renewed);
    }

    #[cfg(feature = "storage")]
    #[test]
    fn corrupt_publisher_state_is_rejected_on_restore() {
        use spine_storage::{InMemoryBackend, StorageBackend};
        use std::sync::Arc;
        let backend = Arc::new(InMemoryBackend::new());
        backend
            .put("spine-name-publishers-v1", b"bad", b"{}")
            .unwrap();
        assert!(LocalResolver::persistent(|| 1_000, backend).is_err());
    }

    #[cfg(feature = "storage")]
    #[tokio::test]
    async fn a_stored_record_with_an_invalid_signature_is_skipped_not_fatal() {
        use spine_storage::{InMemoryBackend, StorageBackend};
        use std::sync::Arc;
        let good = signed(1, 1_000, 100, &["web.search"], 1);
        let mut bad = signed(2, 1_000, 100, &["web.search"], 1);
        bad.seq += 1; // signature no longer matches the signed bytes
        let backend = Arc::new(InMemoryBackend::new());
        for rec in [&good, &bad] {
            backend
                .put(
                    "spine-name-publishers-v1",
                    rec.name.to_string().as_bytes(),
                    &serde_json::to_vec(rec).unwrap(),
                )
                .unwrap();
        }
        let r = LocalResolver::persistent(|| 1_010, backend).unwrap();
        assert_eq!(r.resolve(&good.name).await.unwrap().record, good);
        assert!(matches!(
            r.resolve(&bad.name).await,
            Err(NameError::NotFound(_))
        ));
        assert_eq!(r.published_records().unwrap(), vec![good]);
    }
}
