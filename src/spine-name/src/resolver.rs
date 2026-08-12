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
use crate::record::NameRecord;
use crate::store::RecordStore;
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
        }
    }

    /// A resolver with a fixed clock — for tests and deterministic replay.
    pub fn at_time(now: u64) -> Self {
        Self::new(move || now)
    }

    /// Current time according to the injected clock.
    pub fn now(&self) -> u64 {
        (self.clock)()
    }

    /// Publish a record into the local store. Verification happens in the store,
    /// so an invalid record cannot be published.
    pub fn publish(&self, record: NameRecord) -> Result<(), NameError> {
        let mut store = self.store.lock().unwrap();
        store.put(record)?;
        Ok(())
    }

    /// Seed the cache with a record learned elsewhere (e.g. from the mesh).
    pub fn cache_record(&self, record: NameRecord) -> Result<(), NameError> {
        record.verify()?;
        self.cache.lock().unwrap().put(record);
        Ok(())
    }

    /// Remember that a name did not resolve.
    pub fn cache_negative(&self, uri: &SpineUri) {
        let now = self.now();
        self.cache.lock().unwrap().put_negative(uri, now);
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

        match self.cache.lock().unwrap().get(uri, now) {
            CacheLookup::Fresh(record) => {
                return Ok(Resolution::new(record, Provenance::Cache));
            }
            CacheLookup::NegativeHit => return Err(NameError::NotFound(uri.to_string())),
            // A stale entry is a fallback, not an answer: fall through to the
            // store first, and only use it if nothing better exists.
            CacheLookup::Stale(record) => {
                if let Some(local) = self.store.lock().unwrap().get_fresh(uri, now) {
                    return Ok(Resolution::new(local.clone(), Provenance::Local));
                }
                return Ok(Resolution::new(record, Provenance::StaleCache));
            }
            CacheLookup::Miss => {}
        }

        if let Some(record) = self.store.lock().unwrap().get_fresh(uri, now) {
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
        let mut fresh = NameRecord::new(stale.name.clone(), 2, 2_000).unwrap().with_ttl(100);
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
        assert!(matches!(
            r.resolve(&uri).await,
            Err(NameError::NotFound(_))
        ));
        assert_eq!(r.cache_stats().negative_hits, 1);
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
        r.publish(signed(1, 1_000, 100, &["web.search"], 1)).unwrap();
        r.publish(signed(2, 1_000, 100, &["web.search"], 3)).unwrap();
        r.publish(signed(3, 1_000, 100, &["data.analyze"], 1)).unwrap();

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
}
