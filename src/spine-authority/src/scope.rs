//! Resource names and resource patterns, and what it means for one to contain
//! another.
//!
//! A resource is a layer-2 name with a `did:` authority: the authority is the
//! owner, the path says which of the owner's resources. In a grant the path is
//! held as a list of segments, never as a string, so containment is decided
//! segment by segment and `/data` can never cover `/database`.
//!
//! A [`ResourcePattern`] is either exact (the one name) or a prefix (the name
//! and everything below it). There is no other wildcard.

use serde::{Deserialize, Serialize};
use spine_name::SpineUri;

/// Most path segments a resource name or pattern may have.
pub const MAX_PATH_SEGMENTS: usize = 16;

/// Longest path segment, in bytes of UTF-8.
pub const MAX_SEGMENT_LEN: usize = 128;

/// Why a name cannot be used as a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScopeError {
    /// The authority is not `did:`; only a key can own a resource.
    #[error("resource names must have a did: authority")]
    NotDid,
    /// The name has a query or a fragment.
    #[error("resource names must not carry a query or a fragment")]
    QueryOrFragment,
    /// The path ends in `/` and is not the root.
    #[error("resource paths must not end in a slash")]
    TrailingSlash,
    /// A segment is empty, is `.` or `..`, contains `/`, or contains a control
    /// character.
    #[error("a path segment is empty, dotted, or contains a slash or control character")]
    BadSegment,
    /// A segment is longer than the limit.
    #[error("a path segment is longer than the limit")]
    SegmentTooLong,
    /// More segments than the limit.
    #[error("the path has more segments than the limit")]
    TooDeep,
}

/// Checks a list of path segments against the rules above.
pub fn validate_segments(path: &[String]) -> Result<(), ScopeError> {
    if path.len() > MAX_PATH_SEGMENTS {
        return Err(ScopeError::TooDeep);
    }
    for segment in path {
        if segment.len() > MAX_SEGMENT_LEN {
            return Err(ScopeError::SegmentTooLong);
        }
        let bad = segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains('/')
            || segment.chars().any(|c| c.is_control());
        if bad {
            return Err(ScopeError::BadSegment);
        }
    }
    Ok(())
}

/// Splits the path of a name into segments. `/` is no segments; `/a/b` is
/// `["a", "b"]`; a trailing slash on a longer path is refused.
fn segments_of(path: &str) -> Result<Vec<String>, ScopeError> {
    if path == "/" {
        return Ok(Vec::new());
    }
    if path.ends_with('/') {
        return Err(ScopeError::TrailingSlash);
    }
    // The path begins with `/`, so the first piece is empty and is skipped.
    let segments: Vec<String> = path.split('/').skip(1).map(str::to_string).collect();
    validate_segments(&segments)?;
    Ok(segments)
}

/// Owner key and segments of a `did:` name with no query and no fragment.
fn split_uri(uri: &SpineUri) -> Result<([u8; 32], Vec<String>), ScopeError> {
    let owner: [u8; 32] = match uri.public_key() {
        Some(key) => *key,
        None => return Err(ScopeError::NotDid),
    };
    if !uri.query().is_empty() || uri.fragment().is_some() {
        return Err(ScopeError::QueryOrFragment);
    }
    let segments = segments_of(uri.path())?;
    Ok((owner, segments))
}

/// `true` when `prefix` is a leading run of `path`, compared segment by
/// segment.
fn starts_with_segments(path: &[String], prefix: &[String]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == *prefix
}

/// A single resource: an owner key and a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceName {
    /// The Ed25519 public key of the owning party.
    #[serde(with = "crate::bytes::fixed")]
    pub owner: [u8; 32],
    /// Path segments, outermost first. Empty is the root of the owner's names.
    pub path: Vec<String>,
}

impl ResourceName {
    /// Builds a resource name, checking the segments.
    pub fn new(owner: [u8; 32], path: Vec<String>) -> Result<Self, ScopeError> {
        validate_segments(&path)?;
        Ok(ResourceName { owner, path })
    }

    /// The resource a layer-2 name denotes. Refused unless the authority is
    /// `did:`, there is no query and no fragment, and the path is in segment
    /// form (see [`ScopeError`]).
    pub fn from_uri(uri: &SpineUri) -> Result<Self, ScopeError> {
        let (owner, path) = split_uri(uri)?;
        Ok(ResourceName { owner, path })
    }

    /// The layer-2 name of this resource.
    pub fn to_uri(&self) -> SpineUri {
        SpineUri::did(self.owner).with_path(&path_string(&self.path))
    }

    /// Checks the segments.
    pub fn validate(&self) -> Result<(), ScopeError> {
        validate_segments(&self.path)
    }
}

fn path_string(path: &[String]) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", path.join("/"))
    }
}

/// What a grant names: one resource, or a resource and everything below it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcePattern {
    /// The Ed25519 public key of the owning party.
    #[serde(with = "crate::bytes::fixed")]
    pub owner: [u8; 32],
    /// Path segments of the named resource, or of the base of the prefix.
    pub path: Vec<String>,
    /// `false`: exactly the resource at `path`. `true`: the resource at `path`
    /// and every resource whose path begins with those segments.
    pub prefix: bool,
}

impl ResourcePattern {
    /// The pattern that matches exactly the resource `uri` names.
    pub fn exact(uri: &SpineUri) -> Result<Self, ScopeError> {
        let (owner, path) = split_uri(uri)?;
        Ok(ResourcePattern {
            owner,
            path,
            prefix: false,
        })
    }

    /// The pattern that matches the resource `uri` names and everything below
    /// it, segment-wise.
    pub fn prefix(uri: &SpineUri) -> Result<Self, ScopeError> {
        let (owner, path) = split_uri(uri)?;
        Ok(ResourcePattern {
            owner,
            path,
            prefix: true,
        })
    }

    /// Checks the segments.
    pub fn validate(&self) -> Result<(), ScopeError> {
        validate_segments(&self.path)
    }

    /// Whether the resource lies inside this pattern: same owner, and either
    /// the same path (exact) or a path that begins with the pattern's segments
    /// (prefix). The prefix `/data` contains `/data` and `/data/x` but not
    /// `/database`.
    pub fn contains(&self, resource: &ResourceName) -> bool {
        if self.owner != resource.owner {
            return false;
        }
        if self.prefix {
            starts_with_segments(&resource.path, &self.path)
        } else {
            self.path == resource.path
        }
    }

    /// Whether every resource this pattern matches is also matched by `outer`.
    /// An exact pattern is covered by an exact pattern with the same path, or
    /// by a prefix pattern whose base is a leading run of its path. A prefix
    /// pattern is covered only by a prefix pattern whose base is a leading run
    /// of its base.
    pub fn covered_by(&self, outer: &ResourcePattern) -> bool {
        if self.owner != outer.owner {
            return false;
        }
        if outer.prefix {
            starts_with_segments(&self.path, &outer.path)
        } else {
            !self.prefix && self.path == outer.path
        }
    }
}
