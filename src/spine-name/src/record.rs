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

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::link::Link;
use crate::uri::{Authority, SpineUri};
use crate::NameError;

/// Default record lifetime: 1 hour.
pub const DEFAULT_TTL_SECS: u32 = 3600;

/// Domain-separation prefix of the signed bytes. Bump it whenever the layout
/// in [`NameRecord::signing_bytes`] changes.
const SIGNING_DOMAIN: &[u8] = b"spine-name-record-v2\0";

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
    /// The resource id of the current representation, as computed by
    /// [`crate::resource_id`]: SHA-256 over a domain prefix, the content type
    /// and the bytes, not the plain [`crate::content_hash`] of the bytes. It
    /// serves as a strong cache validator and, because the content type is
    /// hashed in, authenticates the type as well. Check a fetched
    /// representation with [`crate::verify_resource`].
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

    /// A record for a `host:` name, derived from the name itself.
    ///
    /// This is the one binding that needs no publisher: `spine://host:h:9440/`
    /// already *contains* its endpoint, so resolving it is a restatement rather
    /// than a lookup. That is exactly why the escape hatch works before any DHT
    /// contact exists — and exactly why it is the weakest thing in the
    /// namespace.
    ///
    /// The result is unsigned and unsignable, since a hostname is not a key.
    /// [`NameRecord::verify`] will reject it, which is correct: nothing here has
    /// been attested, and whoever answers at that address is simply believed.
    /// Use it to reach a seed, then rely on what the seed proves about itself.
    pub fn for_host(name: SpineUri, now: u64, ttl_secs: u32) -> Result<Self, NameError> {
        let Authority::Host { host, port } = name.authority() else {
            return Err(NameError::InvalidAuthority(name.to_string()));
        };
        let address = match port {
            Some(p) => format!("{host}:{p}"),
            None => host.clone(),
        };
        Ok(Self {
            name,
            seq: 0,
            ttl_secs,
            published_at: now,
            endpoints: vec![Endpoint::new("tcp", address)],
            capabilities: Vec::new(),
            content_hash: None,
            links: Vec::new(),
            meta: BTreeMap::new(),
            signature: Vec::new(),
        })
    }

    /// Whether this record carries a signature that [`NameRecord::verify`] could
    /// accept — false for anything derived from a `host:` name.
    pub fn is_attested(&self) -> bool {
        matches!(self.name.authority(), Authority::Did(_)) && self.signature.len() == 64
    }

    pub fn with_ttl(mut self, ttl_secs: u32) -> Self {
        self.ttl_secs = ttl_secs;
        self
    }

    /// Renew with the resource owner's key, preserving endpoints and content.
    /// No replica can extend the lifetime without that key. An exhausted
    /// sequence is an error rather than a wraparound to an older record.
    pub fn renewed(&self, key: &SigningKey, now: u64, ttl_secs: u32) -> Result<Self, NameError> {
        self.verify()?;
        let mut record = self.clone();
        record.seq = record
            .seq
            .checked_add(1)
            .ok_or(NameError::SequenceExhausted)?;
        record.published_at = now;
        record.ttl_secs = ttl_secs;
        record.sign(key)?;
        Ok(record)
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

    /// The canonical bytes covered by the signature (layout `v2`).
    ///
    /// Built field-by-field rather than by serializing the struct: a
    /// serializer's field order or number formatting could change under a
    /// dependency bump and silently invalidate every record in existence.
    /// `BTreeMap` keeps metadata ordered for the same reason.
    ///
    /// Every field of the record except `signature` is covered. The encoding
    /// is injective: nothing is terminated by a sentinel, so no choice of
    /// string contents can move a field boundary.
    ///
    /// ```text
    /// "spine-name-record-v2\0"                      domain separation
    /// str(name)                                      canonical URI string
    /// u64 seq | u32 ttl_secs | u64 published_at      big-endian
    /// u32 n_endpoints, then per endpoint:
    ///     str(transport) str(address) u8 priority
    /// u32 n_capabilities, then per capability: str(term)
    /// u8 has_content_hash (0|1), then 32 hash bytes iff 1
    /// u32 n_links, then per link:
    ///     str(rel) str(target)
    ///     u8 has_title (0|1), then str(title) iff 1
    ///     u32 n_link_capabilities, then per term: str(term)
    /// u32 n_meta, then per entry in key order: str(key) str(value)
    /// ```
    ///
    /// where `str(x)` is a big-endian `u32` byte length followed by the UTF-8
    /// bytes, and every `u32 n_*` is a big-endian element count.
    pub fn signing_bytes(&self) -> Vec<u8> {
        fn put_str(out: &mut Vec<u8>, s: &str) {
            out.extend_from_slice(&(s.len() as u32).to_be_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        fn put_count(out: &mut Vec<u8>, n: usize) {
            out.extend_from_slice(&(n as u32).to_be_bytes());
        }

        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(SIGNING_DOMAIN);
        put_str(&mut out, &self.name.to_string());
        out.extend_from_slice(&self.seq.to_be_bytes());
        out.extend_from_slice(&self.ttl_secs.to_be_bytes());
        out.extend_from_slice(&self.published_at.to_be_bytes());

        put_count(&mut out, self.endpoints.len());
        for e in &self.endpoints {
            put_str(&mut out, &e.transport);
            put_str(&mut out, &e.address);
            out.push(e.priority);
        }

        put_count(&mut out, self.capabilities.len());
        for c in &self.capabilities {
            put_str(&mut out, c);
        }

        match &self.content_hash {
            Some(h) => {
                out.push(1);
                out.extend_from_slice(h);
            }
            None => out.push(0),
        }

        put_count(&mut out, self.links.len());
        for l in &self.links {
            put_str(&mut out, l.rel.as_str());
            put_str(&mut out, &l.target.to_string());
            match &l.title {
                Some(t) => {
                    out.push(1);
                    put_str(&mut out, t);
                }
                None => out.push(0),
            }
            put_count(&mut out, l.capabilities.len());
            for c in &l.capabilities {
                put_str(&mut out, c);
            }
        }

        put_count(&mut out, self.meta.len());
        for (k, v) in &self.meta {
            put_str(&mut out, k);
            put_str(&mut out, v);
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
        vk.verify_strict(&self.signing_bytes(), &Signature::from_bytes(&sig))
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
    /// Higher `seq` wins; equal `seq` breaks toward the later publication; an
    /// exact tie on both breaks toward the greater signature. The last step
    /// makes the order total over *distinct* records: without it, two different
    /// records with the same `seq` and `published_at` each lose to whichever a
    /// replica saw first, and replicas never converge.
    pub fn supersedes(&self, other: &NameRecord) -> bool {
        (self.seq, self.published_at, &self.signature)
            > (other.seq, other.published_at, &other.signature)
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

/// Most endpoints a record accepted from the network may carry.
pub const MAX_ENDPOINTS: usize = 16;
/// Most links a record accepted from the network may carry.
pub const MAX_LINKS: usize = 64;
/// Most capability terms a record accepted from the network may carry, counting
/// those attached to its links.
pub const MAX_CAPABILITIES: usize = 64;
/// Most metadata entries a record accepted from the network may carry.
pub const MAX_META_ENTRIES: usize = 64;
/// Longest any single string in a record may be, in bytes.
pub const MAX_FIELD_BYTES: usize = 1024;

impl NameRecord {
    /// Sanity-check the size of a record that came from a peer.
    ///
    /// A valid signature proves only who published a record, not that it is
    /// reasonable to hold: anyone can mint a key and sign an enormous record.
    /// This bounds every collection and string in it (see [`MAX_ENDPOINTS`],
    /// [`MAX_LINKS`], [`MAX_CAPABILITIES`], [`MAX_META_ENTRIES`] and
    /// [`MAX_FIELD_BYTES`]) so that accepting, storing and re-serving it costs a
    /// bounded amount. Call it before [`NameRecord::verify`] on anything
    /// received over the network. It does not check the signature.
    pub fn validate_size(&self) -> Result<(), NameError> {
        fn too_large(what: &str) -> NameError {
            NameError::RecordTooLarge(what.to_string())
        }
        fn check_str(s: &str, what: &str) -> Result<(), NameError> {
            if s.len() > MAX_FIELD_BYTES {
                Err(too_large(what))
            } else {
                Ok(())
            }
        }

        if self.endpoints.len() > MAX_ENDPOINTS {
            return Err(too_large("endpoints"));
        }
        if self.links.len() > MAX_LINKS {
            return Err(too_large("links"));
        }
        if self.capabilities.len() > MAX_CAPABILITIES {
            return Err(too_large("capabilities"));
        }
        if self.meta.len() > MAX_META_ENTRIES {
            return Err(too_large("metadata"));
        }
        check_str(&self.name.to_string(), "name")?;
        for endpoint in &self.endpoints {
            check_str(&endpoint.transport, "endpoint transport")?;
            check_str(&endpoint.address, "endpoint address")?;
        }
        for term in &self.capabilities {
            check_str(term, "capability")?;
        }
        let mut link_capabilities = self.capabilities.len();
        for link in &self.links {
            check_str(&link.target.to_string(), "link target")?;
            if let Some(title) = &link.title {
                check_str(title, "link title")?;
            }
            link_capabilities = link_capabilities.saturating_add(link.capabilities.len());
            if link_capabilities > MAX_CAPABILITIES {
                return Err(too_large("capabilities"));
            }
            for term in &link.capabilities {
                check_str(term, "link capability")?;
            }
        }
        for (key, value) in &self.meta {
            check_str(key, "metadata key")?;
            check_str(value, "metadata value")?;
        }
        Ok(())
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
    fn an_ordinary_record_passes_the_size_check() {
        let (_, rec) = signed(1, 1, 1_000);
        let rec = rec
            .with_endpoint(Endpoint::new("tcp", "127.0.0.1:1"))
            .with_capability("search")
            .with_meta("title", "x");
        assert_eq!(rec.validate_size(), Ok(()));
    }

    #[test]
    fn an_oversized_record_is_rejected() {
        let (_, base) = signed(2, 1, 1_000);

        let mut r = base.clone();
        for i in 0..=MAX_ENDPOINTS {
            r = r.with_endpoint(Endpoint::new("tcp", format!("h:{i}")));
        }
        assert!(matches!(
            r.validate_size(),
            Err(NameError::RecordTooLarge(_))
        ));

        let mut r = base.clone();
        for i in 0..=MAX_CAPABILITIES {
            r = r.with_capability(format!("cap{i}"));
        }
        assert!(matches!(
            r.validate_size(),
            Err(NameError::RecordTooLarge(_))
        ));

        let mut r = base.clone();
        for i in 0..=MAX_META_ENTRIES {
            r = r.with_meta(format!("k{i}"), "v");
        }
        assert!(matches!(
            r.validate_size(),
            Err(NameError::RecordTooLarge(_))
        ));

        let mut r = base.clone();
        for _ in 0..=MAX_LINKS {
            r = r.with_link(Link::new(Rel::Peer, base.name.clone()));
        }
        assert!(matches!(
            r.validate_size(),
            Err(NameError::RecordTooLarge(_))
        ));

        let r = base.with_meta("title", "x".repeat(MAX_FIELD_BYTES + 1));
        assert!(matches!(
            r.validate_size(),
            Err(NameError::RecordTooLarge(_))
        ));
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
        links
            .links
            .push(Link::new(Rel::Child, SpineUri::did([3u8; 32])));
        assert!(matches!(links.verify(), Err(NameError::BadSignature)));

        let mut meta = base.clone();
        meta.meta.insert("k".into(), "v".into());
        assert!(matches!(meta.verify(), Err(NameError::BadSignature)));
    }

    #[test]
    fn tampering_with_nested_link_and_endpoint_fields_invalidates_the_record() {
        let key = keypair(1);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut base = NameRecord::new(name, 1, 1_000)
            .unwrap()
            .with_endpoint(Endpoint::new("tcp", "a:1").with_priority(3))
            .with_link(
                Link::new(Rel::Provides, SpineUri::did([3u8; 32]))
                    .with_title("Search")
                    .with_capability("web.search"),
            );
        base.sign(&key).unwrap();
        assert!(base.verify().is_ok());

        let mut title = base.clone();
        title.links[0].title = Some("Other".into());
        assert!(matches!(title.verify(), Err(NameError::BadSignature)));

        let mut no_title = base.clone();
        no_title.links[0].title = None;
        assert!(matches!(no_title.verify(), Err(NameError::BadSignature)));

        let mut link_cap = base.clone();
        link_cap.links[0].capabilities.push("admin".into());
        assert!(matches!(link_cap.verify(), Err(NameError::BadSignature)));

        let mut link_cap_swap = base.clone();
        link_cap_swap.links[0].capabilities[0] = "web.crawl".into();
        assert!(matches!(
            link_cap_swap.verify(),
            Err(NameError::BadSignature)
        ));

        let mut rel = base.clone();
        rel.links[0].rel = Rel::Child;
        assert!(matches!(rel.verify(), Err(NameError::BadSignature)));

        let mut target = base.clone();
        target.links[0].target = SpineUri::did([4u8; 32]);
        assert!(matches!(target.verify(), Err(NameError::BadSignature)));

        let mut priority = base.clone();
        priority.endpoints[0].priority = 4;
        assert!(matches!(priority.verify(), Err(NameError::BadSignature)));

        let mut published = base.clone();
        published.published_at += 1;
        assert!(matches!(published.verify(), Err(NameError::BadSignature)));
    }

    #[test]
    fn field_boundaries_cannot_be_shifted_to_forge_equal_signing_bytes() {
        let (_, rec) = signed(1, 1, 0);

        // The old NUL-terminated layout encoded these identically.
        let a = rec
            .clone()
            .with_endpoint(Endpoint::new("tc\0p", "x"))
            .signing_bytes();
        let b = rec
            .clone()
            .with_endpoint(Endpoint::new("tc", "p\0x"))
            .signing_bytes();
        assert_ne!(a, b);

        let a = rec.clone().with_meta("k\0v", "w").signing_bytes();
        let b = rec.clone().with_meta("k", "v\0w").signing_bytes();
        assert_ne!(a, b);

        let a = rec
            .clone()
            .with_capability("a")
            .with_capability("b")
            .signing_bytes();
        let b = rec.clone().with_capability("a\0b").signing_bytes();
        assert_ne!(a, b);

        // A title moved into a link capability must not collide.
        let target = SpineUri::did([3u8; 32]);
        let a = rec
            .clone()
            .with_link(Link::new(Rel::Child, target.clone()).with_title("x"))
            .signing_bytes();
        let b = rec
            .clone()
            .with_link(Link::new(Rel::Child, target).with_capability("x"))
            .signing_bytes();
        assert_ne!(a, b);
    }

    #[test]
    fn signing_bytes_carry_the_v2_domain_prefix() {
        let (_, rec) = signed(1, 1, 0);
        assert!(rec.signing_bytes().starts_with(b"spine-name-record-v2\0"));
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
        assert!(
            c.supersedes(&b),
            "equal seq breaks toward later publication"
        );
        assert!(!b.supersedes(&b), "a record does not supersede itself");
    }

    #[test]
    fn distinct_records_with_equal_seq_and_time_have_exactly_one_winner() {
        let key = SigningKey::from_bytes(&[1; 32]);
        let name = SpineUri::did(key.verifying_key().to_bytes());
        let mut a = NameRecord::new(name.clone(), 2, 1_000)
            .unwrap()
            .with_ttl(60);
        a.sign(&key).unwrap();
        let mut b = NameRecord::new(name, 2, 1_000).unwrap().with_ttl(120);
        b.sign(&key).unwrap();
        assert_ne!(a.signature, b.signature);
        assert_ne!(
            a.supersedes(&b),
            b.supersedes(&a),
            "exactly one of two distinct records must win, whatever the arrival order"
        );
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
        assert!(
            back.verify().is_ok(),
            "signature must survive serialization"
        );
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
