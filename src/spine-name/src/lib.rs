//! # spine-name — the agent web's namespace
//!
//! SPINE had a transport, a content format, user agents, and a trust model. It
//! did not have a *namespace*, and without one a stack is not a web: names are
//! what let a resource be referred to independently of the machine currently
//! serving it, linked to by someone who has never met its operator, and found by
//! an agent that does not already know where it lives.
//!
//! This crate supplies that layer:
//!
//! | Piece | Type | What it answers |
//! |---|---|---|
//! | The `spine://` scheme | [`SpineUri`] | *What is this resource called?* |
//! | Self-certifying authorities | [`Authority`] | *Who says so, and why should I believe them?* |
//! | Signed bindings | [`NameRecord`] | *What does the name resolve to?* |
//! | Shared keyspace | [`NameKey`], [`RoutingTable`] | *Which node would know?* |
//! | Typed edges | [`Link`], [`CrawlFrontier`] | *What else is reachable from here?* |
//! | Resolution | [`Resolver`], [`LocalResolver`] | *Turn a name into something actionable.* |
//!
//! ## Three departures from the WWW, all for agents
//!
//! **Names certify themselves.** A `did:` authority is an Ed25519 public key, so
//! a record verifies against the name with no certificate authority in the path.
//! The WWW bolted trust on a decade late and still routes it through ~150 root
//! CAs; here it is the naming primitive.
//!
//! **Capabilities are addressable.** `spine://cap:web.search/` names an *ability*
//! and resolves to ranked providers. Agents overwhelmingly want "something that
//! can do X" rather than "whatever is at this host" — the human web could only
//! express the latter, which is why it needed centralized search engines to
//! answer the former.
//!
//! **Content addressing is native.** `spine://blob:<sha256>/` is immutable by
//! construction, so it is cacheable forever with no revalidation, and every
//! [`NameRecord`] carries a content hash usable as a strong validator.
//!
//! ## Example: publish a name, then resolve it
//!
//! ```
//! use ed25519_dalek::SigningKey;
//! use spine_name::{Endpoint, LocalResolver, NameRecord, Resolver, SpineUri};
//!
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let key = SigningKey::from_bytes(&[7u8; 32]);
//! let name = SpineUri::did(key.verifying_key().to_bytes()).with_path("/tools/search");
//!
//! let mut record = NameRecord::new(name.clone(), 1, 1_000)?
//!     .with_endpoint(Endpoint::new("tcp", "10.0.0.4:9440"))
//!     .with_capability("web.search");
//! record.sign(&key)?;
//!
//! let resolver = LocalResolver::at_time(1_000);
//! resolver.publish(record)?;
//!
//! // Resolve by name...
//! let hit = resolver.resolve(&name).await?;
//! assert_eq!(hit.record.endpoints[0].address, "10.0.0.4:9440");
//!
//! // ...or by what you actually need done.
//! let providers = resolver.find_providers("web.search").await?;
//! assert_eq!(providers.len(), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! # }).unwrap();
//! ```
//!
//! [`Authority`]: crate::uri::Authority

pub mod base32;
pub mod cache;
pub mod frontier;
pub mod key;
pub mod link;
pub mod logbook;
pub mod record;
pub mod resolver;
pub mod routing;
pub mod store;
pub mod succession;
pub mod uri;

pub use cache::{CacheLookup, CacheStats, ResolverCache};
pub use frontier::{CrawlBudget, CrawlFrontier, Skipped, Visit};
pub use key::{Distance, NameKey};
pub use link::{Link, Rel};
pub use logbook::{
    authorise_record, log_inception_key, log_within_bounds, publication_log, publication_value,
    LogBook, LogChange, LogOffer, MAX_EQUIVOCATION_PROOFS, MAX_SUCCESSION_LOGS,
    PUBLICATION_LOG_FIELD,
};
pub use record::{
    Endpoint, NameRecord, DEFAULT_TTL_SECS, MAX_CLOCK_SKEW_SECS, MAX_RECORD_TTL_SECS,
    REVOKED_META_KEY,
};
pub use resolver::{LocalResolver, Provenance, Resolution, Resolver};
pub use routing::{NodeInfo, RoutingTable, DEFAULT_K};
pub use store::{PutOutcome, RecordStore, DEFAULT_CAPACITY};
pub use succession::{
    detect_equivocation, key_commitment, record_authorised, record_head_claim, Current,
    Equivocation, HeadClaim, RecordAuthority, Refusal, State as SuccessionState, Succession,
    SuccessionError, SuccessionKind, SuccessionLog, MAX_SUCCESSION_ENTRIES, SUCCESSION_META_KEY,
};
pub use uri::{Authority, SpineUri, SCHEME};

use sha2::{Digest, Sha256};

/// SHA-256 over bytes — the hash behind `blob:` names and content validators.
pub fn content_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Domain-separation prefix of a resource id. It is hashed first, so a resource
/// id can never equal the plain [`content_hash`] of any byte string that does
/// not itself begin with this prefix and the same framing.
pub const RESOURCE_ID_DOMAIN: &[u8] = b"spine/resource-id/v1\0";

/// Longest accepted content type, in bytes.
pub const MAX_CONTENT_TYPE_LEN: usize = 256;

/// Check a content type. The rule is exact-match, not normalisation: a type is
/// accepted only if it is already in canonical form, so two spellings can never
/// name the same type. It must be 1 to [`MAX_CONTENT_TYPE_LEN`] bytes of
/// printable ASCII (0x20 through 0x7e), contain no uppercase letters, and have
/// no leading or trailing space.
pub fn validate_content_type(content_type: &str) -> Result<(), NameError> {
    let bytes = content_type.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= MAX_CONTENT_TYPE_LEN
        && bytes
            .iter()
            .all(|b| (0x20..=0x7e).contains(b) && !b.is_ascii_uppercase())
        && bytes[0] != b' '
        && bytes[bytes.len() - 1] != b' ';
    if valid {
        Ok(())
    } else {
        Err(NameError::InvalidContentType)
    }
}

/// The identity of a resource: SHA-256 over
/// `RESOURCE_ID_DOMAIN || u32_be(len(content_type)) || content_type ||
/// u64_be(len(bytes)) || bytes`. Because the content type is part of what is
/// hashed, a signed record that carries this id also authenticates the type.
/// This is the only function that computes a resource id; every publisher and
/// verifier calls it.
pub fn resource_id(content_type: &str, bytes: &[u8]) -> Result<[u8; 32], NameError> {
    validate_content_type(content_type)?;
    let mut hasher = Sha256::new();
    hasher.update(RESOURCE_ID_DOMAIN);
    hasher.update((content_type.len() as u32).to_be_bytes());
    hasher.update(content_type.as_bytes());
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
    Ok(hasher.finalize().into())
}

/// Check that `content_type` and `bytes` are exactly what `id` commits to.
/// An invalid type is [`NameError::InvalidContentType`]; a valid type with the
/// wrong bytes or the wrong type is [`NameError::ResourceIdMismatch`].
pub fn verify_resource(id: &[u8; 32], content_type: &str, bytes: &[u8]) -> Result<(), NameError> {
    if resource_id(content_type, bytes)? == *id {
        Ok(())
    } else {
        Err(NameError::ResourceIdMismatch)
    }
}

/// Everything that can go wrong naming or resolving.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NameError {
    #[error(
        "invalid content type: expected 1-256 printable ASCII bytes, lowercase, no edge spaces"
    )]
    InvalidContentType,

    #[error(
        "resource id mismatch: the bytes or the content type differ from what the id commits to"
    )]
    ResourceIdMismatch,

    #[error("name storage failed: {0}")]
    Storage(String),

    #[error("name record store is full")]
    Capacity,

    #[error("record sequence is exhausted")]
    SequenceExhausted,

    #[error("publication does not supersede the stored record")]
    StalePublication,

    #[error("not a spine:// name: {0}")]
    NotSpineScheme(String),

    #[error("malformed authority: {0}")]
    InvalidAuthority(String),

    #[error("unknown authority kind `{0}` (expected did, blob, cap, or host)")]
    UnknownAuthorityKind(String),

    #[error("name `{0}` cannot be signed: only did: names carry a key")]
    NotSignable(String),

    #[error("signing key does not match the key in the name")]
    KeyMismatch,

    #[error("signature verification failed")]
    BadSignature,

    #[error("name not found: {0}")]
    NotFound(String),

    #[error("`{0}` names a set, not a single record — use find_providers")]
    NotASingleName(String),

    #[error("resolution failed: {0}")]
    Transport(String),

    #[error("name record too large: {0}")]
    RecordTooLarge(String),

    #[error("record ttl_secs {0} exceeds the maximum record lifetime")]
    TtlTooLong(u32),

    #[error(
        "record published_at {published_at} is further ahead of the clock ({now}) than the allowed skew"
    )]
    PostDated { published_at: u64, now: u64 },

    #[error("malformed tombstone: {0}")]
    MalformedTombstone(String),

    /// The name has been withdrawn by its owner. Distinct from `NotFound`: a
    /// tombstone is held, and a resolver must not go looking for another answer.
    #[error("name revoked: {0}")]
    Revoked(String),

    /// The name was retired for good by a `retire` entry of its succession log
    /// (spec 02, section 10). Distinct from [`NameError::Revoked`], which a
    /// tombstone's lifetime bounds: nothing re-publishes a retired name.
    #[error("name retired: {0}")]
    Retired(String),

    /// The record is signed by a key that a held succession entry has
    /// superseded.
    #[error("record for {0} is signed by a superseded key")]
    SupersededKey(String),

    /// Two valid, conflicting succession logs of the name are held: one key
    /// signed two entries at one position. The name fails closed at this
    /// verifier and authorises no record (spec 02, 10.5).
    #[error("name {0} is compromised: its succession log equivocates")]
    Equivocated(String),

    /// The record's head claim names a log position beyond the log held, so the
    /// log must be obtained before the record can be judged.
    #[error(
        "record for {name} claims succession position {claimed} but only {held} entries are held"
    )]
    NeedSuccessionLog {
        name: String,
        claimed: u64,
        held: u64,
    },

    /// A succession rule other than the ones above refused the record: a
    /// malformed, stale or conflicting head claim, or a name that is not the
    /// log's.
    #[error("record for {name} refused by key succession: {reason}")]
    SuccessionRefused { name: String, reason: String },

    /// A succession log or equivocation proof does not verify, or is not for
    /// this name.
    #[error("invalid succession log: {0}")]
    InvalidSuccessionLog(String),

    /// The store holds as many succession logs, or equivocation proofs, as it
    /// is allowed to and cannot take another.
    #[error("succession log table is full")]
    SuccessionCapacity,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn content_hash_is_stable_and_distinguishing() {
        assert_eq!(content_hash(b"a"), content_hash(b"a"));
        assert_ne!(content_hash(b"a"), content_hash(b"b"));
    }

    #[test]
    fn resource_id_changes_when_only_the_content_type_changes() {
        let a = resource_id("text/plain", b"same bytes").unwrap();
        let b = resource_id("text/html", b"same bytes").unwrap();
        assert_ne!(a, b);
        assert_eq!(a, resource_id("text/plain", b"same bytes").unwrap());
        assert_ne!(a, content_hash(b"same bytes"));
    }

    #[test]
    fn resource_id_length_prefixing_prevents_boundary_shift() {
        assert_ne!(
            resource_id("a", b"bcdef").unwrap(),
            resource_id("ab", b"cdef").unwrap()
        );
    }

    #[test]
    fn verify_resource_distinguishes_its_failures() {
        let id = resource_id("text/plain", b"payload").unwrap();
        assert_eq!(verify_resource(&id, "text/plain", b"payload"), Ok(()));
        assert_eq!(
            verify_resource(&id, "text/html", b"payload"),
            Err(NameError::ResourceIdMismatch)
        );
        assert_eq!(
            verify_resource(&id, "text/plain", b"other"),
            Err(NameError::ResourceIdMismatch)
        );
        assert_eq!(
            verify_resource(&id, "Text/Plain", b"payload"),
            Err(NameError::InvalidContentType)
        );
    }

    #[test]
    fn content_types_are_exact_match_canonical() {
        for good in ["text/plain", "text/html; charset=utf-8", "a"] {
            assert_eq!(validate_content_type(good), Ok(()), "{good}");
        }
        let long = "a".repeat(MAX_CONTENT_TYPE_LEN + 1);
        for bad in [
            "",
            "Text/Plain",
            " text/plain",
            "text/plain ",
            "text/pl\u{e9}in",
            "text/plain\n",
            long.as_str(),
        ] {
            assert_eq!(
                validate_content_type(bad),
                Err(NameError::InvalidContentType),
                "{bad:?}"
            );
        }
        assert_eq!(
            validate_content_type(&"a".repeat(MAX_CONTENT_TYPE_LEN)),
            Ok(())
        );
    }

    /// The end-to-end path the crate exists to provide: mint a self-certifying
    /// name, publish a signed record under it, resolve it back, and walk the
    /// link graph it exposes — none of which required a CA, a registrar, or a
    /// central index.
    #[tokio::test]
    async fn a_name_can_be_published_resolved_and_walked() {
        let key = SigningKey::from_bytes(&[42u8; 32]);
        let root = SpineUri::did(key.verifying_key().to_bytes());
        let child = root.clone().with_path("/tools/search");

        let mut record = NameRecord::new(root.clone(), 1, 1_000)
            .unwrap()
            .with_endpoint(Endpoint::new("quic", "10.0.0.1:9440"))
            .with_capability("web.search")
            .with_content_hash(content_hash(b"root representation"))
            .with_link(Link::new(Rel::Child, child.clone()).with_title("search tool"));
        record.sign(&key).unwrap();

        let resolver = LocalResolver::at_time(1_000);
        resolver.publish(record.clone()).unwrap();

        // Resolve by name.
        let hit = resolver.resolve(&root).await.unwrap();
        assert_eq!(hit.provenance, Provenance::Local);
        assert!(hit.record.verify().is_ok());

        // Resolve by capability.
        let providers = resolver.find_providers("web.search").await.unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, root);

        // Walk the graph the record exposes.
        let mut frontier = CrawlFrontier::new(CrawlBudget::default());
        frontier.seed(root.clone());
        let visit = frontier.next_visit().unwrap();
        frontier.expand(&visit.uri, &hit.record.links, visit.depth);
        let next = frontier.next_visit().unwrap();
        assert_eq!(next.uri, child);
        assert_eq!(next.via, Some(Rel::Child));
    }

    #[test]
    fn a_blob_name_round_trips_from_bytes_to_name_and_back() {
        let payload = b"an immutable agent artifact";
        let name = SpineUri::blob_of(payload);
        assert_eq!(name.content_hash(), Some(&content_hash(payload)));
        assert!(name.is_immutable());
        assert_eq!(SpineUri::parse(&name.to_string()).unwrap(), name);
    }
}
