//! The `spine://` URI scheme — SPINE's namespace.
//!
//! The WWW's URI bought one decisive thing: a name for a resource that is
//! independent of the machine currently serving it. SPINE needs that, plus two
//! properties HTTP URLs never had:
//!
//! 1. **Self-certification.** A `did:` authority *is* an Ed25519 public key, so
//!    a record published under that name verifies against the name itself. There
//!    is no certificate authority in the resolution path, and a name survives
//!    relocation without weakening its trust.
//! 2. **Capability addressing.** `cap:` names an *ability*, not an endpoint.
//!    Agents overwhelmingly want "something that can do `web.search`" rather
//!    than "whatever lives at this host", and a namespace that can only express
//!    the latter forces every agent to hard-code a directory.
//!
//! ```text
//! spine://did:<52-symbol base32 ed25519 key>/tools/search?q=rust#results
//! spine://blob:<52-symbol base32 sha-256>            (immutable, content-addressed)
//! spine://cap:web.search/                            (resolves to ranked providers)
//! spine://host:node.example.org:9440/                (bootstrap escape hatch)
//! ```

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

use crate::base32;
use crate::key::NameKey;
use crate::NameError;

/// The scheme prefix, including separator.
pub const SCHEME: &str = "spine://";

/// The authority of a [`SpineUri`] — *who or what* is being named.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Authority {
    /// An Ed25519 public key. The name is self-certifying: a [`crate::NameRecord`]
    /// published under it verifies against the authority with no external trust.
    Did([u8; 32]),
    /// A SHA-256 content hash. Immutable by construction, so it never needs
    /// revalidation and can be cached forever.
    Blob([u8; 32]),
    /// A capability term (an ontology term URI such as `web.search`). Resolves
    /// to a *set* of ranked providers rather than a single record.
    Capability(String),
    /// A transport-level host. The escape hatch used to bootstrap into the mesh
    /// before any self-certifying name is known; carries no cryptographic
    /// identity and is never treated as trusted.
    Host { host: String, port: Option<u16> },
}

impl Authority {
    /// Whether a record under this authority can be verified against the name
    /// itself, with no certificate authority or external trust anchor.
    pub fn is_self_certifying(&self) -> bool {
        matches!(self, Authority::Did(_) | Authority::Blob(_))
    }

    /// Whether the named bytes can never change, making the name safe to cache
    /// indefinitely without revalidation.
    pub fn is_immutable(&self) -> bool {
        matches!(self, Authority::Blob(_))
    }

    /// The canonical wire spelling (already normalized).
    pub fn as_canonical(&self) -> String {
        match self {
            Authority::Did(k) => format!("did:{}", base32::encode(k)),
            Authority::Blob(h) => format!("blob:{}", base32::encode(h)),
            Authority::Capability(t) => format!("cap:{t}"),
            Authority::Host { host, port } => match port {
                Some(p) => format!("host:{host}:{p}"),
                None => format!("host:{host}"),
            },
        }
    }

    fn parse(raw: &str) -> Result<Self, NameError> {
        let (tag, rest) = raw
            .split_once(':')
            .ok_or_else(|| NameError::InvalidAuthority(raw.to_string()))?;

        match tag.to_ascii_lowercase().as_str() {
            "did" => base32::decode_key(rest)
                .map(Authority::Did)
                .ok_or_else(|| NameError::InvalidAuthority(raw.to_string())),
            "blob" => base32::decode_key(rest)
                .map(Authority::Blob)
                .ok_or_else(|| NameError::InvalidAuthority(raw.to_string())),
            "cap" => {
                let term = rest.trim().to_ascii_lowercase();
                if term.is_empty() || term.contains(|c: char| c.is_whitespace() || c == '/') {
                    return Err(NameError::InvalidAuthority(raw.to_string()));
                }
                Ok(Authority::Capability(term))
            }
            "host" => {
                let rest = rest.trim();
                if rest.is_empty() {
                    return Err(NameError::InvalidAuthority(raw.to_string()));
                }
                // Split only on the last colon so IPv6 literals in brackets and
                // ordinary host:port both parse.
                match rest.rsplit_once(':') {
                    Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
                        let port: u16 = p
                            .parse()
                            .map_err(|_| NameError::InvalidAuthority(raw.to_string()))?;
                        if h.is_empty() {
                            return Err(NameError::InvalidAuthority(raw.to_string()));
                        }
                        Ok(Authority::Host {
                            host: h.to_ascii_lowercase(),
                            port: Some(port),
                        })
                    }
                    _ => Ok(Authority::Host {
                        host: rest.to_ascii_lowercase(),
                        port: None,
                    }),
                }
            }
            _ => Err(NameError::UnknownAuthorityKind(tag.to_string())),
        }
    }
}

impl fmt::Display for Authority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_canonical())
    }
}

/// A parsed, normalized `spine://` name.
///
/// Construction always normalizes, so two `SpineUri` values are equal exactly
/// when they name the same resource. That property is what lets the resolver
/// cache, the record store, and the crawl frontier all key off the same value
/// without each re-deriving its own notion of sameness.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpineUri {
    authority: Authority,
    /// Always begins with `/`.
    path: String,
    /// Sorted by key, so query order is not semantically significant.
    query: Vec<(String, String)>,
    fragment: Option<String>,
}

impl SpineUri {
    /// Build a URI from an authority, with the root path.
    pub fn new(authority: Authority) -> Self {
        Self {
            authority,
            path: "/".to_string(),
            query: Vec::new(),
            fragment: None,
        }
    }

    /// Name an Ed25519 public key.
    pub fn did(public_key: [u8; 32]) -> Self {
        Self::new(Authority::Did(public_key))
    }

    /// Name a SHA-256 content hash.
    pub fn blob(hash: [u8; 32]) -> Self {
        Self::new(Authority::Blob(hash))
    }

    /// Name the content of `bytes` by hashing it — the canonical way to mint an
    /// immutable name for a payload an agent is about to publish.
    pub fn blob_of(bytes: &[u8]) -> Self {
        Self::blob(crate::content_hash(bytes))
    }

    /// Name a capability term.
    pub fn capability(term: impl Into<String>) -> Self {
        Self::new(Authority::Capability(term.into().to_ascii_lowercase()))
    }

    /// Parse a `spine://` URI, normalizing it.
    pub fn parse(input: &str) -> Result<Self, NameError> {
        let trimmed = input.trim();
        let rest = trimmed
            .strip_prefix(SCHEME)
            .or_else(|| {
                // Accept a case-varied scheme without allocating in the common path.
                let (head, tail) = trimmed.split_at(trimmed.len().min(SCHEME.len()));
                head.eq_ignore_ascii_case(SCHEME).then_some(tail)
            })
            .ok_or_else(|| NameError::NotSpineScheme(trimmed.to_string()))?;

        if rest.is_empty() {
            return Err(NameError::InvalidAuthority(String::new()));
        }

        // Split off fragment, then query, then the authority/path boundary.
        let (rest, fragment) = match rest.split_once('#') {
            Some((r, f)) => (r, Some(f.to_string())),
            None => (rest, None),
        };
        let (rest, query_raw) = match rest.split_once('?') {
            Some((r, q)) => (r, Some(q)),
            None => (rest, None),
        };
        let (authority_raw, path_raw) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };

        let authority = Authority::parse(authority_raw)?;
        let path = normalize_path(path_raw);

        let mut query = Vec::new();
        if let Some(q) = query_raw {
            for pair in q.split('&').filter(|p| !p.is_empty()) {
                match pair.split_once('=') {
                    Some((k, v)) => query.push((k.to_string(), v.to_string())),
                    None => query.push((pair.to_string(), String::new())),
                }
            }
            query.sort();
        }

        Ok(Self {
            authority,
            path,
            query,
            fragment: fragment.filter(|f| !f.is_empty()),
        })
    }

    /// The authority component.
    pub fn authority(&self) -> &Authority {
        &self.authority
    }

    /// The normalized path, always beginning with `/`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Query pairs, sorted by key.
    pub fn query(&self) -> &[(String, String)] {
        &self.query
    }

    /// First value for a query key, if present.
    pub fn query_value(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The fragment, if any. Fragments are client-side selectors and never
    /// participate in resolution or in the routing key.
    pub fn fragment(&self) -> Option<&str> {
        self.fragment.as_deref()
    }

    /// Replace the path (normalizing it).
    pub fn with_path(mut self, path: &str) -> Self {
        self.path = normalize_path(path);
        self
    }

    /// Add a query parameter, keeping the query sorted.
    pub fn with_query(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.query.push((key.into(), value.into()));
        self.query.sort();
        self
    }

    /// Set the fragment.
    pub fn with_fragment(mut self, fragment: impl Into<String>) -> Self {
        let f = fragment.into();
        self.fragment = (!f.is_empty()).then_some(f);
        self
    }

    /// See [`Authority::is_self_certifying`].
    pub fn is_self_certifying(&self) -> bool {
        self.authority.is_self_certifying()
    }

    /// See [`Authority::is_immutable`].
    pub fn is_immutable(&self) -> bool {
        self.authority.is_immutable()
    }

    /// The Ed25519 public key this name certifies against, if it is a `did:`.
    pub fn public_key(&self) -> Option<&[u8; 32]> {
        match &self.authority {
            Authority::Did(k) => Some(k),
            _ => None,
        }
    }

    /// The content hash this name pins, if it is a `blob:`.
    pub fn content_hash(&self) -> Option<&[u8; 32]> {
        match &self.authority {
            Authority::Blob(h) => Some(h),
            _ => None,
        }
    }

    /// The capability term, if this is a `cap:`.
    pub fn capability_term(&self) -> Option<&str> {
        match &self.authority {
            Authority::Capability(t) => Some(t.as_str()),
            _ => None,
        }
    }

    /// The name stripped to its authority and root path — the identity of the
    /// *publisher*, which is what a record is signed under and stored at.
    pub fn origin(&self) -> SpineUri {
        SpineUri::new(self.authority.clone())
    }

    /// The DHT routing key: SHA-256 over authority and path.
    ///
    /// Query and fragment are excluded deliberately. A query selects *within* a
    /// resource and a fragment selects within a representation; including either
    /// would scatter one resource across the keyspace and defeat caching.
    pub fn key(&self) -> NameKey {
        NameKey::of(self.routing_form().as_bytes())
    }

    /// The bytes the routing key is taken over.
    fn routing_form(&self) -> String {
        format!("{}{}{}", SCHEME, self.authority.as_canonical(), self.path)
    }

    /// Resolve a reference against this name, the way a browser resolves an
    /// `href`. Absolute `spine://` references replace the base entirely;
    /// rooted references replace the path; everything else is relative to the
    /// base's directory. This is what makes a *link graph* walkable.
    pub fn join(&self, reference: &str) -> Result<SpineUri, NameError> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Ok(self.clone());
        }
        if reference.len() >= SCHEME.len()
            && reference[..SCHEME.len()].eq_ignore_ascii_case(SCHEME)
        {
            return SpineUri::parse(reference);
        }

        // Split the reference's own query/fragment before touching the path.
        let (head, fragment) = match reference.split_once('#') {
            Some((h, f)) => (h, Some(f.to_string())),
            None => (reference, None),
        };
        let (path_part, query_raw) = match head.split_once('?') {
            Some((p, q)) => (p, Some(q)),
            None => (head, None),
        };

        let joined_path = if path_part.is_empty() {
            self.path.clone()
        } else if path_part.starts_with('/') {
            normalize_path(path_part)
        } else {
            // Relative to the base's directory, not the base resource itself.
            let dir = match self.path.rfind('/') {
                Some(i) => &self.path[..=i],
                None => "/",
            };
            normalize_path(&format!("{dir}{path_part}"))
        };

        let mut out = SpineUri {
            authority: self.authority.clone(),
            path: joined_path,
            query: Vec::new(),
            fragment: fragment.filter(|f| !f.is_empty()),
        };

        if let Some(q) = query_raw {
            for pair in q.split('&').filter(|p| !p.is_empty()) {
                match pair.split_once('=') {
                    Some((k, v)) => out.query.push((k.to_string(), v.to_string())),
                    None => out.query.push((pair.to_string(), String::new())),
                }
            }
            out.query.sort();
        }
        Ok(out)
    }
}

/// Normalize a path: ensure a leading `/`, collapse repeated separators, and
/// resolve `.` and `..` segments so that two spellings of one path compare equal.
fn normalize_path(raw: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    let trailing = raw.ends_with('/') && !segments.is_empty();
    let mut out = String::with_capacity(raw.len() + 1);
    for s in &segments {
        out.push('/');
        out.push_str(s);
    }
    // An empty result means the path reduced to the root; a trailing separator
    // in the input is preserved. Both end in `/`.
    if out.is_empty() || trailing {
        out.push('/');
    }
    out
}

impl fmt::Display for SpineUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}{}", SCHEME, self.authority.as_canonical(), self.path)?;
        if !self.query.is_empty() {
            f.write_str("?")?;
            for (i, (k, v)) in self.query.iter().enumerate() {
                if i > 0 {
                    f.write_str("&")?;
                }
                if v.is_empty() {
                    write!(f, "{k}")?;
                } else {
                    write!(f, "{k}={v}")?;
                }
            }
        }
        if let Some(frag) = &self.fragment {
            write!(f, "#{frag}")?;
        }
        Ok(())
    }
}

impl FromStr for SpineUri {
    type Err = NameError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        SpineUri::parse(s)
    }
}

// Serialized as its canonical string: compact on the wire, and readable in the
// JSON that crosses the mesh and the gateway.
impl Serialize for SpineUri {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SpineUri {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        SpineUri::parse(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn did_uri() -> String {
        format!("{}did:{}/", SCHEME, base32::encode(&[9u8; 32]))
    }

    #[test]
    fn parses_and_roundtrips_a_did_name() {
        let uri = SpineUri::parse(&did_uri()).unwrap();
        assert_eq!(uri.public_key(), Some(&[9u8; 32]));
        assert!(uri.is_self_certifying());
        assert!(!uri.is_immutable());
        assert_eq!(SpineUri::parse(&uri.to_string()).unwrap(), uri);
    }

    #[test]
    fn blob_names_are_immutable_and_content_addressed() {
        let uri = SpineUri::blob_of(b"hello agents");
        assert!(uri.is_immutable());
        assert!(uri.is_self_certifying());
        assert_eq!(uri.content_hash(), Some(&crate::content_hash(b"hello agents")));
        // The same bytes always mint the same name — the basis for dedup.
        assert_eq!(SpineUri::blob_of(b"hello agents"), uri);
        assert_ne!(SpineUri::blob_of(b"other bytes"), uri);
    }

    #[test]
    fn capability_names_are_lowercased_and_not_self_certifying() {
        let uri = SpineUri::parse("spine://cap:Web.Search/").unwrap();
        assert_eq!(uri.capability_term(), Some("web.search"));
        assert!(!uri.is_self_certifying());
        assert_eq!(uri, SpineUri::capability("WEB.SEARCH"));
    }

    #[test]
    fn host_authority_parses_with_and_without_port() {
        let with = SpineUri::parse("spine://host:node.example.org:9440/").unwrap();
        assert_eq!(
            with.authority(),
            &Authority::Host {
                host: "node.example.org".into(),
                port: Some(9440)
            }
        );
        let without = SpineUri::parse("spine://host:node.example.org/").unwrap();
        assert_eq!(
            without.authority(),
            &Authority::Host {
                host: "node.example.org".into(),
                port: None
            }
        );
        assert!(!with.is_self_certifying());
    }

    #[test]
    fn scheme_and_authority_are_case_insensitive() {
        let upper = format!("SPINE://DID:{}/", base32::encode(&[9u8; 32]).to_uppercase());
        assert_eq!(SpineUri::parse(&upper).unwrap(), SpineUri::did([9u8; 32]));
    }

    #[test]
    fn rejects_non_spine_and_malformed_names() {
        assert!(matches!(
            SpineUri::parse("https://example.com/"),
            Err(NameError::NotSpineScheme(_))
        ));
        assert!(matches!(
            SpineUri::parse("spine://ftp:whatever/"),
            Err(NameError::UnknownAuthorityKind(_))
        ));
        // A did: whose key is not 32 bytes cannot certify anything.
        assert!(matches!(
            SpineUri::parse("spine://did:mzxw6/"),
            Err(NameError::InvalidAuthority(_))
        ));
        assert!(SpineUri::parse("spine://").is_err());
    }

    #[test]
    fn path_normalization_makes_equal_names_compare_equal() {
        let base = base32::encode(&[1u8; 32]);
        let a = SpineUri::parse(&format!("spine://did:{base}/a//b/./c")).unwrap();
        let b = SpineUri::parse(&format!("spine://did:{base}/a/b/x/../c")).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.path(), "/a/b/c");
        assert_eq!(a.key(), b.key());
    }

    #[test]
    fn dot_dot_cannot_escape_the_root() {
        let base = base32::encode(&[1u8; 32]);
        let uri = SpineUri::parse(&format!("spine://did:{base}/../../etc/passwd")).unwrap();
        assert_eq!(uri.path(), "/etc/passwd");
    }

    #[test]
    fn query_order_is_not_significant_but_is_preserved_in_output() {
        let base = base32::encode(&[2u8; 32]);
        let a = SpineUri::parse(&format!("spine://did:{base}/s?b=2&a=1")).unwrap();
        let b = SpineUri::parse(&format!("spine://did:{base}/s?a=1&b=2")).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.to_string(), format!("spine://did:{base}/s?a=1&b=2"));
        assert_eq!(a.query_value("b"), Some("2"));
    }

    #[test]
    fn query_and_fragment_do_not_change_the_routing_key() {
        let base = base32::encode(&[3u8; 32]);
        let plain = SpineUri::parse(&format!("spine://did:{base}/doc")).unwrap();
        let decorated =
            SpineUri::parse(&format!("spine://did:{base}/doc?q=1#section")).unwrap();
        assert_eq!(plain.key(), decorated.key());
        assert_ne!(plain, decorated);
    }

    #[test]
    fn join_resolves_relative_rooted_and_absolute_references() {
        let base = SpineUri::parse(&format!(
            "spine://did:{}/tools/search",
            base32::encode(&[4u8; 32])
        ))
        .unwrap();

        // Relative — resolves against the base's directory.
        assert_eq!(base.join("index").unwrap().path(), "/tools/index");
        // Parent traversal.
        assert_eq!(base.join("../about").unwrap().path(), "/about");
        // Rooted.
        assert_eq!(base.join("/top").unwrap().path(), "/top");
        // Absolute — replaces the authority too.
        let other = base.join("spine://cap:web.search/").unwrap();
        assert_eq!(other.capability_term(), Some("web.search"));
        // Fragment-only reference keeps the path.
        let frag = base.join("#results").unwrap();
        assert_eq!(frag.path(), "/tools/search");
        assert_eq!(frag.fragment(), Some("results"));
    }

    #[test]
    fn origin_strips_to_the_publisher_identity() {
        let uri = SpineUri::did([5u8; 32])
            .with_path("/a/b")
            .with_query("q", "1")
            .with_fragment("f");
        let origin = uri.origin();
        assert_eq!(origin.path(), "/");
        assert!(origin.query().is_empty());
        assert_eq!(origin.public_key(), uri.public_key());
    }

    #[test]
    fn serde_roundtrips_through_json_as_a_string() {
        let uri = SpineUri::did([6u8; 32]).with_path("/x").with_query("k", "v");
        let json = serde_json::to_string(&uri).unwrap();
        assert!(json.starts_with("\"spine://did:"));
        assert_eq!(serde_json::from_str::<SpineUri>(&json).unwrap(), uri);
    }

    #[test]
    fn serde_rejects_a_malformed_name_rather_than_accepting_it() {
        assert!(serde_json::from_str::<SpineUri>("\"https://evil.example\"").is_err());
    }
}
