//! Typed links — the edges of the agent web.
//!
//! The WWW's `<a href>` is untyped: a crawler cannot tell navigation from
//! citation from "this endpoint implements that capability" without reading the
//! surrounding prose. Agents pay a disproportionate price for that, because the
//! prose is exactly the part they must spend tokens on. SPINE links carry their
//! relation in the data, so a frontier can filter, prioritize, and budget a
//! traversal without fetching anything.

use serde::{Deserialize, Serialize};

use crate::uri::SpineUri;

/// The relation a link asserts between its source and target.
///
/// `Other` is for relations this crate does not name. A built-in relation has
/// one spelling only: deserializing `Other` with a built-in relation's exact
/// name yields the built-in variant (see [`Rel::other`]), because both sign as
/// the same bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", from = "RelRepr")]
pub enum Rel {
    /// Contained resource — the ordinary "walk downward" edge.
    Child,
    /// Containing resource.
    Parent,
    /// A peer at the same level.
    Peer,
    /// The target implements a capability the source advertises. The edge a
    /// capability lookup follows to find a concrete provider.
    Provides,
    /// The source needs the target to function — a dependency edge, which lets
    /// an agent pre-warm what it will inevitably need.
    Requires,
    /// A different representation of the same resource (another codec,
    /// modality, or dimensionality).
    Alternate,
    /// Documentation or schema describing the source.
    Describes,
    /// The immutable `blob:` snapshot of the source's current representation.
    Snapshot,
    /// Anything else, named by the publisher.
    Other(String),
}

/// The shape `Rel` deserializes through, so that [`Rel::Other`] can be
/// normalised on the way in.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RelRepr {
    Child,
    Parent,
    Peer,
    Provides,
    Requires,
    Alternate,
    Describes,
    Snapshot,
    Other(String),
}

impl From<RelRepr> for Rel {
    fn from(repr: RelRepr) -> Self {
        match repr {
            RelRepr::Child => Rel::Child,
            RelRepr::Parent => Rel::Parent,
            RelRepr::Peer => Rel::Peer,
            RelRepr::Provides => Rel::Provides,
            RelRepr::Requires => Rel::Requires,
            RelRepr::Alternate => Rel::Alternate,
            RelRepr::Describes => Rel::Describes,
            RelRepr::Snapshot => Rel::Snapshot,
            RelRepr::Other(s) => Rel::other(s),
        }
    }
}

impl Rel {
    /// A relation named by the publisher. If `name` is exactly the name of a
    /// built-in relation the built-in variant is returned, so `Other("child")`
    /// cannot come out of deserialization next to `Child` (they sign
    /// identically). Unlike [`Rel::parse`] this does not change case:
    /// `Other("Mirrors")` keeps its spelling, which is part of the signed bytes.
    pub fn other(name: impl Into<String>) -> Self {
        let name = name.into();
        match name.as_str() {
            "child" => Rel::Child,
            "parent" => Rel::Parent,
            "peer" => Rel::Peer,
            "provides" => Rel::Provides,
            "requires" => Rel::Requires,
            "alternate" => Rel::Alternate,
            "describes" => Rel::Describes,
            "snapshot" => Rel::Snapshot,
            _ => Rel::Other(name),
        }
    }

    /// Stable string form, used in signing bytes and on the wire.
    pub fn as_str(&self) -> &str {
        match self {
            Rel::Child => "child",
            Rel::Parent => "parent",
            Rel::Peer => "peer",
            Rel::Provides => "provides",
            Rel::Requires => "requires",
            Rel::Alternate => "alternate",
            Rel::Describes => "describes",
            Rel::Snapshot => "snapshot",
            Rel::Other(s) => s.as_str(),
        }
    }

    /// Parse from the wire form.
    pub fn parse(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "child" => Rel::Child,
            "parent" => Rel::Parent,
            "peer" => Rel::Peer,
            "provides" => Rel::Provides,
            "requires" => Rel::Requires,
            "alternate" => Rel::Alternate,
            "describes" => Rel::Describes,
            "snapshot" => Rel::Snapshot,
            other => Rel::Other(other.to_string()),
        }
    }

    /// Default traversal weight, lower being more urgent.
    ///
    /// A dependency is worth following before a sibling, because an agent will
    /// block on it; a `describes` edge is worth following last, because it is
    /// usually only needed once something has gone wrong.
    pub fn default_priority(&self) -> u8 {
        match self {
            Rel::Requires => 0,
            Rel::Provides => 1,
            Rel::Child => 2,
            Rel::Snapshot => 3,
            Rel::Alternate => 4,
            Rel::Peer => 5,
            Rel::Parent => 6,
            Rel::Describes => 7,
            Rel::Other(_) => 8,
        }
    }
}

/// A typed edge to another name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// What the edge means.
    pub rel: Rel,
    /// Where it points.
    pub target: SpineUri,
    /// Human- or agent-readable label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Capability terms the target is claimed to offer. A hint only — the
    /// target's own signed record is authoritative — but it lets an agent skip
    /// fetching edges that cannot possibly help.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
}

impl Link {
    pub fn new(rel: Rel, target: SpineUri) -> Self {
        Self {
            rel,
            target,
            title: None,
            capabilities: Vec::new(),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_capability(mut self, term: impl Into<String>) -> Self {
        self.capabilities.push(term.into().to_ascii_lowercase());
        self
    }

    /// Whether this edge claims a capability (case-insensitive).
    pub fn claims_capability(&self, term: &str) -> bool {
        let want = term.to_ascii_lowercase();
        self.capabilities.contains(&want)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_roundtrips_through_its_wire_form() {
        for rel in [
            Rel::Child,
            Rel::Parent,
            Rel::Peer,
            Rel::Provides,
            Rel::Requires,
            Rel::Alternate,
            Rel::Describes,
            Rel::Snapshot,
            Rel::Other("mirrors".into()),
        ] {
            assert_eq!(Rel::parse(rel.as_str()), rel);
        }
    }

    #[test]
    fn unknown_rels_are_preserved_not_discarded() {
        assert_eq!(Rel::parse("Mirrors"), Rel::Other("mirrors".into()));
        assert_eq!(Rel::parse("Mirrors").as_str(), "mirrors");
    }

    #[test]
    fn other_with_a_builtin_name_is_the_builtin() {
        assert_eq!(Rel::other("child"), Rel::Child);
        assert_eq!(Rel::other("snapshot"), Rel::Snapshot);
        assert_eq!(Rel::other("mirrors"), Rel::Other("mirrors".into()));
        assert_eq!(Rel::other("Child"), Rel::Other("Child".into()));
    }

    #[test]
    fn deserializing_other_normalises_builtin_names() {
        let alias: Rel = serde_json::from_str(r#"{"other":"child"}"#).unwrap();
        assert_eq!(alias, Rel::Child);
        let plain: Rel = serde_json::from_str(r#""child""#).unwrap();
        assert_eq!(plain, Rel::Child);
        let custom: Rel = serde_json::from_str(r#"{"other":"mirrors"}"#).unwrap();
        assert_eq!(custom, Rel::Other("mirrors".into()));
        // The spelling of a custom relation is preserved exactly.
        let cased: Rel = serde_json::from_str(r#"{"other":"Mirrors"}"#).unwrap();
        assert_eq!(cased.as_str(), "Mirrors");
        // Serialization is unchanged.
        assert_eq!(
            serde_json::to_string(&Rel::Other("mirrors".into())).unwrap(),
            r#"{"other":"mirrors"}"#
        );
        assert_eq!(serde_json::to_string(&Rel::Child).unwrap(), r#""child""#);
    }

    #[test]
    fn a_link_with_an_aliased_rel_deserializes_to_the_builtin() {
        let target = SpineUri::did([1u8; 32]).to_string();
        let json = format!(r#"{{"rel":{{"other":"child"}},"target":"{target}"}}"#);
        let link: Link = serde_json::from_str(&json).unwrap();
        assert_eq!(link.rel, Rel::Child);
    }

    #[test]
    fn dependencies_outrank_documentation_in_traversal_order() {
        assert!(Rel::Requires.default_priority() < Rel::Child.default_priority());
        assert!(Rel::Child.default_priority() < Rel::Describes.default_priority());
        assert!(Rel::Provides.default_priority() < Rel::Peer.default_priority());
    }

    #[test]
    fn capability_claims_match_case_insensitively() {
        let link = Link::new(Rel::Provides, SpineUri::did([1u8; 32])).with_capability("Web.Search");
        assert!(link.claims_capability("web.search"));
        assert!(link.claims_capability("WEB.SEARCH"));
        assert!(!link.claims_capability("web.crawl"));
    }

    #[test]
    fn serde_omits_empty_optional_fields() {
        let link = Link::new(Rel::Child, SpineUri::did([1u8; 32]));
        let json = serde_json::to_string(&link).unwrap();
        assert!(!json.contains("title"), "{json}");
        assert!(!json.contains("capabilities"), "{json}");
        assert_eq!(serde_json::from_str::<Link>(&json).unwrap(), link);
    }

    #[test]
    fn serde_roundtrips_a_fully_populated_link() {
        let link = Link::new(Rel::Provides, SpineUri::capability("web.search"))
            .with_title("Search provider")
            .with_capability("web.search");
        let json = serde_json::to_string(&link).unwrap();
        assert_eq!(serde_json::from_str::<Link>(&json).unwrap(), link);
    }
}
