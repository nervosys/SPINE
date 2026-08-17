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
pub mod record;
pub mod resolver;
pub mod routing;
pub mod store;
pub mod uri;

pub use cache::{CacheLookup, CacheStats, ResolverCache};
pub use frontier::{CrawlBudget, CrawlFrontier, Skipped, Visit};
pub use key::{Distance, NameKey};
pub use link::{Link, Rel};
pub use record::{Endpoint, NameRecord, DEFAULT_TTL_SECS};
pub use resolver::{LocalResolver, Provenance, Resolution, Resolver};
pub use routing::{NodeInfo, RoutingTable, DEFAULT_K};
pub use store::{PutOutcome, RecordStore, DEFAULT_CAPACITY};
pub use uri::{Authority, SpineUri, SCHEME};

use sha2::{Digest, Sha256};

/// SHA-256 over bytes — the hash behind `blob:` names and content validators.
pub fn content_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Everything that can go wrong naming or resolving.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NameError {
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
