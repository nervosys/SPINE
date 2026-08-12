//! Signed name records — what a `spine://` name actually resolves *to*.
//!
//! A record is the agent web's equivalent of a DNS record and an HTTP response
//! head fused into one signed object. Fusing them is deliberate: an agent
//! resolving a name almost always wants the endpoints, the capabilities, the
//! cache validator, and the outbound links together, and splitting them across
//! layers is what forces the human web into a resolve-then-HEAD-then-GET chain
//! before any useful work starts.
//!
//! Records are signed by the `did:` key that names them, so verification needs
//! no certificate authority — the name *is* the key.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::link::Link;
use crate::uri::{Authority, SpineUri};
use crate::NameError;

/// Default record lifetime: 1 hour.
pub const DEFAULT_TTL_SECS: u32 = 3600;

/// A transport address a named resource can be reached at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// Transport identifier: `tcp`, `ws`, `wss`, `quic`, `grpc`.
    pub transport: String,
    /// Address in that transport's own spelling (`host:port`, `wss://…`).
    pub address: String,
    /// Selection preference; lower is tried first.
    #[serde(default)]
    pub priority: u8,
}

impl Endpoint {
    pub fn new(transport: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            transport: transport.into(),
            address: address.into(),
            priority: 0,
        }
    }

    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }
}

/// A signed binding from a name to everything needed to use it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameRecord {
    /// The name being bound. Must have a `did:` authority — only a key can sign.
    pub name: SpineUri,
    /// Monotonic version. A higher `seq` supersedes a lower one, which is how
    /// replicas converge without a coordinator.
    pub seq: u64,
    /// Seconds the record stays fresh after `published_at`.
    pub ttl_secs: u32,
    /// Publication time, seconds since the Unix epoch.
    pub published_at: u64,
    /// Where to reach the resource.
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
    /// Ontology capability terms this resource offers. Indexed by the store so
    /// `cap:` names resolve without a central directory.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// SHA-256 of the current representation. Serves as a strong cache
    /// validator: an agent holding this hash needs no refetch, and a `blob:`
    /// name for it is immutable.
    #[serde(default)]
    pub content_hash: Option<[u8; 32]>,
    /// Outbound links. This is the field that makes the agent web a *graph*
    /// rather than a set of isolated endpoints.
    #[serde(default)]
    pub links: Vec<Link>,
    /// Free-form metadata (title, description, protocol versions).
    #[serde(default)]
    pub meta: BTreeMap<String, String>,
    /// Ed25519 signature over [`NameRecord::signing_bytes`].
    #[serde(default, with = "sig_bytes")]
    pub signature: Vec<u8>,
}

impl NameRecord {
    /// A new unsigned record for `name`. Returns an error unless `name` has a
    /// `did:` authority, since nothing else can produce a verifiable signature.
    pub fn new(name: SpineUri, seq: u64, published_at: u64) -> Result<Self, NameError> {
        if !matches!(name.authority(), Authority::Did(_)) {
            return Err(NameError::NotSignable(name.to_string()));
        }
        Ok(Self {
            name,
            seq,
            ttl_secs: DEFAULT_TTL_SECS,
            published_at,
            endpoints: Vec::new(),
            capabilities: Vec::new(),
            content_hash: None,
            links: Vec::new(),
            meta: BTreeMap::new(),
            signature: Vec::new(),
        })
    }

    pub fn with_ttl(mut self, ttl_secs: u32) -> Self {
        self.ttl_secs = ttl_secs;
        self
    }

    pub fn with_endpoint(mut self, endpoint: Endpoint) -> Self {
        self.endpoints.push(endpoint);
        self
    }

    pub fn with_capability(mut self, term: impl Into<String>) -> Self {
        self.capabilities.push(term.into().to_ascii_lowercase());
        self
    }

    pub fn with_content_hash(mut self, hash: [u8; 32]) -> Self {
        self.content_hash = Some(hash);
        self
    }

    pub fn with_link(mut self, link: Link) -> Self {
        self.links.push(link);
        self
    }

    pub fn with_meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.meta.insert(key.into(), value.into());
        self
    }

    /// The canonical bytes covered by the signature.
    ///
    /// Built field-by-field with explicit separators rather than by serializing
    /// the struct: a serializer's field order or number formatting could change
    /// under a dependency bump and silently invalidate every record in
    /// existence. `BTreeMap` keeps metadata ordered for the same reason.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(b"spine-name-record-v1\0");
        out.extend_from_slice(self.name.to_string().as_bytes());
        out.push(0);
        out.extend_from_slice(&self.seq.to_be_bytes());
        out.extend_from_slice(&self.ttl_secs.to_be_bytes());
        out.extend_from_slice(&self.published_at.to_be_bytes());

        out.extend_from_slice(&(self.endpoints.len() as u32).to_be_bytes());
        for e in &self.endpoints {
            out.extend_from_slice(e.transport.as_bytes());
            out.push(0);
            out.extend_from_slice(e.address.as_bytes());
            out.push(0);
            out.push(e.priority);
        }

        out.extend_from_slice(&(self.capabilities.len() as u32).to_be_bytes());
        for c in &self.capabilities {
            out.extend_from_slice(c.as_bytes());
            out.push(0);
        }

        match &self.content_hash {
            Some(h) => {
                out.push(1);
                out.extend_from_slice(h);
            }
            None => out.push(0),
        }

        out.extend_from_slice(&(self.links.len() as u32).to_be_bytes());
        for l in &self.links {
            out.extend_from_slice(l.rel.as_str().as_bytes());
            out.push(0);
            out.extend_from_slice(l.target.to_string().as_bytes());
            out.push(0);
        }

        out.extend_from_slice(&(self.meta.len() as u32).to_be_bytes());
        for (k, v) in &self.meta {
            out.extend_from_slice(k.as_bytes());
            out.push(0);
            out.extend_from_slice(v.as_bytes());
            out.push(0);
        }
        out
    }

    /// Sign with the key that the name certifies against.
    ///
    /// Fails if `key` is not the key in the name — signing a record under
    /// someone else's name must be impossible, not merely discouraged.
    pub fn sign(&mut self, key: &SigningKey) -> Result<(), NameError> {
        let expected = self
            .name
            .public_key()
            .ok_or_else(|| NameError::NotSignable(self.name.to_string()))?;
        if key.verifying_key().to_bytes() != *expected {
            return Err(NameError::KeyMismatch);
        }
        let sig: Signature = key.sign(&self.signing_bytes());
        self.signature = sig.to_bytes().to_vec();
        Ok(())
    }

    /// Verify the signature against the public key embedded in the name. No
    /// external trust anchor is consulted, and none is needed.
    pub fn verify(&self) -> Result<(), NameError> {
        let pk_bytes = self
            .name
            .public_key()
            .ok_or_else(|| NameError::NotSignable(self.name.to_string()))?;
        if self.signature.len() != 64 {
            return Err(NameError::BadSignature);
        }
        let vk = VerifyingKey::from_bytes(pk_bytes).map_err(|_| NameError::BadSignature)?;
        let mut sig = [0u8; 64];
        sig.copy_from_slice(&self.signature);
        vk.verify(&self.signing_bytes(), &Signature::from_bytes(&sig))
            .map_err(|_| NameError::BadSignature)
    }

    /// When the record stops being fresh.
    pub fn expires_at(&self) -> u64 {
        self.published_at.saturating_add(u64::from(self.ttl_secs))
    }

    /// Whether the record is stale at `now`.
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at()
    }

    /// Seconds of freshness left at `now`.
    pub fn remaining_ttl(&self, now: u64) -> u64 {
        self.expires_at().saturating_sub(now)
    }

    /// Whether this record should replace `other`.
    ///
    /// Higher `seq` wins; equal `seq` breaks toward the later publication. Both
    /// are total and require no coordination, so independent replicas converge
    /// on the same winner regardless of arrival order.
    pub fn supersedes(&self, other: &NameRecord) -> bool {
        (self.seq, self.published_at) > (other.seq, other.published_at)
    }

    /// Endpoints ordered by preference.
    pub fn endpoints_by_priority(&self) -> Vec<&Endpoint> {
        let mut out: Vec<&Endpoint> = self.endpoints.iter().collect();
        out.sort_by_key(|e| e.priority);
        out
    }

    /// Whether the record advertises a capability term (case-insensitive).
    pub fn has_capability(&self, term: &str) -> bool {
        let want = term.to_ascii_lowercase();
        self.capabilities.contains(&want)
    }
}

/// Serde helper keeping signatures compact and readable as base32.
mod sig_bytes {
    use crate::base32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base32::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let raw = String::deserialize(d)?;
        base32::decode(&raw).ok_or_else(|| serde::de::Error::custom("invalid base32 signature"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::{Link, Rel};
    use ed25519_dalek::SigningKey;

    fn keypair(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn signed(seed: u8, seq: u64, now: u64) -> (SigningKey, NameRecord) {
        let key = keypair(seed);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, seq, now).unwrap();
        rec.sign(&key).unwrap();
        (key, rec)
    }

    #[test]
    fn a_record_verifies_against_its_own_name_with_no_authority() {
        let (_, rec) = signed(1, 1, 1_000);
        assert!(rec.verify().is_ok());
    }

    #[test]
    fn only_a_did_name_can_be_signed() {
        let err = NameRecord::new(SpineUri::capability("web.search"), 1, 0).unwrap_err();
        assert!(matches!(err, NameError::NotSignable(_)));
        let err = NameRecord::new(SpineUri::blob_of(b"x"), 1, 0).unwrap_err();
        assert!(matches!(err, NameError::NotSignable(_)));
    }

    #[test]
    fn signing_under_someone_elses_name_is_rejected() {
        let mine = keypair(1);
        let theirs = keypair(2);
        let name = SpineUri::did(theirs.verifying_key().to_bytes());
        let mut rec = NameRecord::new(name, 1, 0).unwrap();
        assert!(matches!(rec.sign(&mine), Err(NameError::KeyMismatch)));
    }

    #[test]
    fn tampering_with_any_signed_field_invalidates_the_record() {
        let (_, base) = signed(1, 1, 1_000);

        let mut endpoints = base.clone();
        endpoints.endpoints.push(Endpoint::new("tcp", "evil:9440"));
        assert!(matches!(endpoints.verify(), Err(NameError::BadSignature)));

        let mut caps = base.clone();
        caps.capabilities.push("admin".into());
        assert!(matches!(caps.verify(), Err(NameError::BadSignature)));

        let mut seq = base.clone();
        seq.seq += 1;
        assert!(matches!(seq.verify(), Err(NameError::BadSignature)));

        let mut ttl = base.clone();
        ttl.ttl_secs += 1;
        assert!(matches!(ttl.verify(), Err(NameError::BadSignature)));

        let mut hash = base.clone();
        hash.content_hash = Some([9u8; 32]);
        assert!(matches!(hash.verify(), Err(NameError::BadSignature)));

        let mut links = base.clone();
        links.links.push(Link::new(Rel::Child, SpineUri::did([3u8; 32])));
        assert!(matches!(links.verify(), Err(NameError::BadSignature)));

        let mut meta = base.clone();
        meta.meta.insert("k".into(), "v".into());
        assert!(matches!(meta.verify(), Err(NameError::BadSignature)));
    }

    #[test]
    fn a_record_cannot_be_replayed_under_a_different_name() {
        let (_, rec) = signed(1, 1, 1_000);
        let mut moved = rec.clone();
        moved.name = SpineUri::did(keypair(2).verifying_key().to_bytes());
        assert!(matches!(moved.verify(), Err(NameError::BadSignature)));
    }

    #[test]
    fn a_malformed_signature_is_rejected_rather_than_panicking() {
        let (_, mut rec) = signed(1, 1, 1_000);
        rec.signature = vec![0u8; 10];
        assert!(matches!(rec.verify(), Err(NameError::BadSignature)));
        rec.signature = vec![0u8; 64];
        assert!(matches!(rec.verify(), Err(NameError::BadSignature)));
        rec.signature.clear();
        assert!(matches!(rec.verify(), Err(NameError::BadSignature)));
    }

    #[test]
    fn freshness_tracks_ttl() {
        let (_, rec) = signed(1, 1, 1_000);
        let rec = rec.with_ttl(60);
        assert_eq!(rec.expires_at(), 1_060);
        assert!(!rec.is_expired(1_059));
        assert!(rec.is_expired(1_060));
        assert_eq!(rec.remaining_ttl(1_030), 30);
        assert_eq!(rec.remaining_ttl(9_999), 0);
    }

    #[test]
    fn higher_seq_supersedes_and_ties_break_by_publication_time() {
        let (_, a) = signed(1, 1, 1_000);
        let (_, b) = signed(1, 2, 1_000);
        assert!(b.supersedes(&a));
        assert!(!a.supersedes(&b));

        let (_, c) = signed(1, 2, 2_000);
        assert!(c.supersedes(&b), "equal seq breaks toward later publication");
        assert!(!b.supersedes(&b), "a record does not supersede itself");
    }

    #[test]
    fn endpoints_sort_by_priority_and_capabilities_match_case_insensitively() {
        let (key, rec) = signed(1, 1, 0);
        let mut rec = rec
            .with_endpoint(Endpoint::new("tcp", "a:1").with_priority(5))
            .with_endpoint(Endpoint::new("quic", "b:2").with_priority(1))
            .with_capability("Web.Search");
        rec.sign(&key).unwrap();

        assert_eq!(rec.endpoints_by_priority()[0].transport, "quic");
        assert!(rec.has_capability("web.search"));
        assert!(rec.has_capability("WEB.SEARCH"));
        assert!(!rec.has_capability("web.crawl"));
    }

    #[test]
    fn signature_survives_a_json_roundtrip() {
        let (key, rec) = signed(1, 7, 1_234);
        let mut rec = rec
            .with_endpoint(Endpoint::new("tcp", "host:9440"))
            .with_capability("web.search")
            .with_content_hash([4u8; 32])
            .with_link(Link::new(Rel::Child, SpineUri::did([8u8; 32])))
            .with_meta("title", "Search agent");
        rec.sign(&key).unwrap();

        let json = serde_json::to_string(&rec).unwrap();
        let back: NameRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, rec);
        assert!(back.verify().is_ok(), "signature must survive serialization");
    }

    #[test]
    fn signing_bytes_are_stable_across_metadata_insertion_order() {
        let (_, rec) = signed(1, 1, 0);
        let a = rec
            .clone()
            .with_meta("b", "2")
            .with_meta("a", "1")
            .signing_bytes();
        let b = rec.with_meta("a", "1").with_meta("b", "2").signing_bytes();
        assert_eq!(a, b);
    }
}
