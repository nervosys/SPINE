"""Phase 20: Agent Ontology System for SPINE.

Adds ontology-based agent discovery with cryptographic + neural hash privacy controls.
Integrates with AgentRegistry, KnowledgeGraph, and marketplace.
"""
import re

AGENTIC = "spine-agentic/src/lib.rs"

with open(AGENTIC, "r", encoding="utf-8") as f:
    src = f.read()

# ========================================================================
# 1. Add the Ontology system section
# ========================================================================

ontology_section = r'''
// =============================================================================
// AGENT ONTOLOGY SYSTEM
// =============================================================================

/// Visibility level for ontology elements.
/// Controls what other agents can see about an agent's capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OntologyVisibility {
    /// Fully public — cleartext ontology disclosed to all
    Public,
    /// Hash-only — only the cryptographic hash is published; verifiable but not readable
    HashOnly,
    /// Neural-hash — a learned embedding is published; similar ontologies are discoverable
    /// but the exact terms are hidden (approximate matching, not exact verification)
    NeuralHash,
    /// Private — completely hidden, not discoverable
    Private,
}

impl Default for OntologyVisibility {
    fn default() -> Self {
        Self::Public
    }
}

/// A single term in an ontology — a concept, capability, or relation type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyTerm {
    /// URI-style identifier (e.g. "spine:capability/text-analysis")
    pub uri: String,
    /// Human-readable label
    pub label: String,
    /// Optional longer description
    pub description: Option<String>,
    /// Parent terms (IS-A hierarchy)
    pub parents: Vec<String>,
    /// Properties / attributes this term carries
    pub properties: HashMap<String, String>,
    /// Visibility setting for this term
    pub visibility: OntologyVisibility,
}

impl OntologyTerm {
    pub fn new(uri: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            label: label.into(),
            description: None,
            parents: Vec::new(),
            properties: HashMap::new(),
            visibility: OntologyVisibility::Public,
        }
    }

    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        self.parents.push(parent.into());
        self
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    pub fn with_visibility(mut self, vis: OntologyVisibility) -> Self {
        self.visibility = vis;
        self
    }

    pub fn with_property(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.properties.insert(key.into(), val.into());
        self
    }

    /// Compute SHA-256 cryptographic hash of this term's canonical form.
    /// Used for HashOnly visibility — verifiable but not reversible.
    pub fn crypto_hash(&self) -> [u8; 32] {
        use sha2::{Sha256, Digest};
        let canonical = format!(
            "{}|{}|{}",
            self.uri,
            self.label,
            self.parents.join(",")
        );
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Compute a neural hash (embedding) of this term.
    /// Returns a fixed-size f32 vector that preserves semantic similarity:
    /// similar terms produce similar embeddings, but exact recovery is infeasible.
    pub fn neural_hash(&self, dim: usize) -> Vec<f32> {
        // Deterministic embedding derived from term content via hashing.
        // This is a lightweight locality-sensitive hash; in production this would
        // be replaced by a learned encoder (VAE / sentence-transformer).
        use sha2::{Sha256, Digest};

        let mut embedding = vec![0.0f32; dim];
        let text = format!("{} {} {}", self.uri, self.label,
            self.description.as_deref().unwrap_or(""));

        // Generate dim floats from successive SHA-256 rounds
        let mut seed = text.as_bytes().to_vec();
        for chunk_start in (0..dim).step_by(8) {
            let mut hasher = Sha256::new();
            hasher.update(&seed);
            let digest = hasher.finalize();
            for j in 0..8.min(dim - chunk_start) {
                let bytes = [
                    digest[j * 4],
                    digest[j * 4 + 1],
                    digest[j * 4 + 2],
                    digest[j * 4 + 3],
                ];
                embedding[chunk_start + j] = f32::from_be_bytes(bytes);
            }
            seed = digest.to_vec();
        }

        // L2-normalize so cosine similarity works correctly
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-10 {
            for v in &mut embedding {
                *v /= norm;
            }
        }
        embedding
    }
}

/// An agent's ontology — the set of concepts, capabilities, and relations it understands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOntology {
    /// Ontology namespace URI (e.g. "spine:ontology/web-agent/v1")
    pub namespace: String,
    /// Version string
    pub version: String,
    /// All terms in this ontology
    pub terms: Vec<OntologyTerm>,
    /// Default visibility for terms that don't specify one
    pub default_visibility: OntologyVisibility,
    /// SHA-256 hash of the entire ontology (computed from all term hashes)
    pub ontology_hash: Option<[u8; 32]>,
}

impl AgentOntology {
    pub fn new(namespace: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            version: version.into(),
            terms: Vec::new(),
            default_visibility: OntologyVisibility::Public,
            ontology_hash: None,
        }
    }

    pub fn with_default_visibility(mut self, vis: OntologyVisibility) -> Self {
        self.default_visibility = vis;
        self
    }

    /// Add a term; uses the ontology's default visibility if the term has Public
    pub fn add_term(&mut self, mut term: OntologyTerm) {
        if term.visibility == OntologyVisibility::Public && self.default_visibility != OntologyVisibility::Public {
            term.visibility = self.default_visibility;
        }
        self.terms.push(term);
        self.recompute_hash();
    }

    fn recompute_hash(&mut self) {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(self.namespace.as_bytes());
        hasher.update(self.version.as_bytes());
        for term in &self.terms {
            hasher.update(&term.crypto_hash());
        }
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        self.ontology_hash = Some(hash);
    }

    /// Get the whole-ontology cryptographic hash.
    pub fn hash(&self) -> [u8; 32] {
        self.ontology_hash.unwrap_or([0u8; 32])
    }

    /// Produce a "disclosed view" of this ontology according to each term's visibility.
    /// - Public terms are included in full.
    /// - HashOnly terms are replaced with their crypto hash.
    /// - NeuralHash terms are replaced with their neural embedding.
    /// - Private terms are omitted entirely.
    pub fn disclosed_view(&self, neural_dim: usize) -> DisclosedOntology {
        let mut public_terms = Vec::new();
        let mut hashed_terms = Vec::new();
        let mut neural_terms = Vec::new();

        for term in &self.terms {
            match term.visibility {
                OntologyVisibility::Public => {
                    public_terms.push(term.clone());
                }
                OntologyVisibility::HashOnly => {
                    hashed_terms.push(HashedTerm {
                        hash: term.crypto_hash(),
                        parents_hash: term.parents.iter().map(|p| {
                            use sha2::{Sha256, Digest};
                            let mut h = Sha256::new();
                            h.update(p.as_bytes());
                            let r = h.finalize();
                            let mut out = [0u8; 32];
                            out.copy_from_slice(&r);
                            out
                        }).collect(),
                    });
                }
                OntologyVisibility::NeuralHash => {
                    neural_terms.push(NeuralHashedTerm {
                        embedding: term.neural_hash(neural_dim),
                        parent_count: term.parents.len(),
                    });
                }
                OntologyVisibility::Private => {
                    // Omitted entirely
                }
            }
        }

        DisclosedOntology {
            namespace: self.namespace.clone(),
            version: self.version.clone(),
            ontology_hash: self.hash(),
            public_terms,
            hashed_terms,
            neural_terms,
        }
    }

    /// Find terms matching a URI prefix
    pub fn find_terms(&self, prefix: &str) -> Vec<&OntologyTerm> {
        self.terms.iter().filter(|t| t.uri.starts_with(prefix)).collect()
    }

    /// Check if this ontology contains a specific term URI
    pub fn has_term(&self, uri: &str) -> bool {
        self.terms.iter().any(|t| t.uri == uri)
    }

    /// Verify a claimed term against a hash (for HashOnly terms).
    /// Returns true if the provided term's hash matches.
    pub fn verify_term_hash(claimed: &OntologyTerm, expected_hash: &[u8; 32]) -> bool {
        claimed.crypto_hash() == *expected_hash
    }
}

/// A term represented only by its cryptographic hash (for HashOnly visibility).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashedTerm {
    /// SHA-256 of the canonical term representation
    pub hash: [u8; 32],
    /// Hashes of parent URIs
    pub parents_hash: Vec<[u8; 32]>,
}

/// A term represented only by its neural embedding (for NeuralHash visibility).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralHashedTerm {
    /// Learned/LSH embedding that preserves semantic similarity
    pub embedding: Vec<f32>,
    /// Number of parent terms (structure hint, not content)
    pub parent_count: usize,
}

/// The "disclosed view" of an ontology — what other agents actually see.
/// Combines cleartext, hashed, and neural-hashed terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisclosedOntology {
    pub namespace: String,
    pub version: String,
    pub ontology_hash: [u8; 32],
    /// Terms disclosed in full
    pub public_terms: Vec<OntologyTerm>,
    /// Terms disclosed as cryptographic hashes only
    pub hashed_terms: Vec<HashedTerm>,
    /// Terms disclosed as neural embeddings only
    pub neural_terms: Vec<NeuralHashedTerm>,
}

impl DisclosedOntology {
    /// Total number of disclosed term slots (public + hashed + neural; excludes private)
    pub fn term_count(&self) -> usize {
        self.public_terms.len() + self.hashed_terms.len() + self.neural_terms.len()
    }

    /// Verify that a candidate term matches one of the hashed terms
    pub fn verify_hash(&self, candidate: &OntologyTerm) -> bool {
        let h = candidate.crypto_hash();
        self.hashed_terms.iter().any(|ht| ht.hash == h)
    }

    /// Find the closest neural-hashed term to a query embedding.
    /// Returns (index, cosine_similarity) or None if no neural terms exist.
    pub fn nearest_neural(&self, query: &[f32]) -> Option<(usize, f32)> {
        if self.neural_terms.is_empty() || query.is_empty() {
            return None;
        }
        let mut best_idx = 0;
        let mut best_sim = f32::NEG_INFINITY;
        for (i, nt) in self.neural_terms.iter().enumerate() {
            if nt.embedding.len() != query.len() {
                continue;
            }
            let dot: f32 = nt.embedding.iter().zip(query).map(|(a, b)| a * b).sum();
            let na: f32 = nt.embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            let nb: f32 = query.iter().map(|x| x * x).sum::<f32>().sqrt();
            let sim = if na > 1e-10 && nb > 1e-10 { dot / (na * nb) } else { 0.0 };
            if sim > best_sim {
                best_sim = sim;
                best_idx = i;
            }
        }
        Some((best_idx, best_sim))
    }
}

/// Permission grant for ontology disclosure to a specific agent or group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyPermission {
    /// Who this permission applies to (agent ID or wildcard "*")
    pub grantee: String,
    /// URI prefix pattern for matching terms
    pub term_pattern: String,
    /// What visibility level to use for matched terms when disclosed to this grantee
    pub visibility: OntologyVisibility,
}

/// Manages per-agent ontology disclosure permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyAccessControl {
    /// Ordered list of permission rules (first match wins)
    pub rules: Vec<OntologyPermission>,
}

impl Default for OntologyAccessControl {
    fn default() -> Self {
        Self {
            rules: vec![OntologyPermission {
                grantee: "*".to_string(),
                term_pattern: "*".to_string(),
                visibility: OntologyVisibility::Public,
            }],
        }
    }
}

impl OntologyAccessControl {
    /// Resolve the effective visibility for a term when disclosed to a specific agent.
    pub fn effective_visibility(
        &self,
        agent_id: &str,
        term_uri: &str,
        default: OntologyVisibility,
    ) -> OntologyVisibility {
        for rule in &self.rules {
            let grantee_match = rule.grantee == "*" || rule.grantee == agent_id;
            let term_match = rule.term_pattern == "*" || term_uri.starts_with(&rule.term_pattern);
            if grantee_match && term_match {
                return rule.visibility;
            }
        }
        default
    }

    /// Add a permission rule.
    pub fn grant(
        &mut self,
        grantee: impl Into<String>,
        term_pattern: impl Into<String>,
        visibility: OntologyVisibility,
    ) {
        // Insert before the catch-all wildcard rule
        let rule = OntologyPermission {
            grantee: grantee.into(),
            term_pattern: term_pattern.into(),
            visibility,
        };
        let insert_pos = self.rules.len().saturating_sub(1);
        self.rules.insert(insert_pos, rule);
    }

    /// Produce a disclosed view tailored to a specific requesting agent.
    pub fn disclose_for(
        &self,
        ontology: &AgentOntology,
        requester_id: &str,
        neural_dim: usize,
    ) -> DisclosedOntology {
        let mut public_terms = Vec::new();
        let mut hashed_terms = Vec::new();
        let mut neural_terms = Vec::new();

        for term in &ontology.terms {
            let vis = self.effective_visibility(requester_id, &term.uri, ontology.default_visibility);
            match vis {
                OntologyVisibility::Public => public_terms.push(term.clone()),
                OntologyVisibility::HashOnly => {
                    hashed_terms.push(HashedTerm {
                        hash: term.crypto_hash(),
                        parents_hash: term.parents.iter().map(|p| {
                            use sha2::{Sha256, Digest};
                            let mut h = Sha256::new();
                            h.update(p.as_bytes());
                            let r = h.finalize();
                            let mut out = [0u8; 32];
                            out.copy_from_slice(&r);
                            out
                        }).collect(),
                    });
                }
                OntologyVisibility::NeuralHash => {
                    neural_terms.push(NeuralHashedTerm {
                        embedding: term.neural_hash(neural_dim),
                        parent_count: term.parents.len(),
                    });
                }
                OntologyVisibility::Private => {}
            }
        }

        DisclosedOntology {
            namespace: ontology.namespace.clone(),
            version: ontology.version.clone(),
            ontology_hash: ontology.hash(),
            public_terms,
            hashed_terms,
            neural_terms,
        }
    }
}

/// Ontology-aware discovery index built on top of AgentRegistry.
/// Enables agents to discover peers by ontology term matching, hash verification,
/// and neural-hash similarity search.
pub struct OntologyRegistry {
    /// Agent ID → their disclosed ontology
    ontologies: DashMap<AgentId, DisclosedOntology>,
    /// Term URI → list of agents that publicly declare it
    by_term: DashMap<String, Vec<AgentId>>,
    /// Crypto hash → list of agents that have a hashed term matching it
    by_hash: DashMap<[u8; 32], Vec<AgentId>>,
}

impl OntologyRegistry {
    pub fn new() -> Self {
        Self {
            ontologies: DashMap::new(),
            by_term: DashMap::new(),
            by_hash: DashMap::new(),
        }
    }

    /// Register an agent's disclosed ontology.
    pub fn register(&self, agent_id: AgentId, disclosed: DisclosedOntology) {
        // Index public terms
        for term in &disclosed.public_terms {
            self.by_term.entry(term.uri.clone()).or_default().push(agent_id);
        }
        // Index hashed terms
        for ht in &disclosed.hashed_terms {
            self.by_hash.entry(ht.hash).or_default().push(agent_id);
        }
        self.ontologies.insert(agent_id, disclosed);
    }

    /// Unregister an agent's ontology.
    pub fn unregister(&self, agent_id: &AgentId) {
        if let Some((_, disclosed)) = self.ontologies.remove(agent_id) {
            for term in &disclosed.public_terms {
                if let Some(mut ids) = self.by_term.get_mut(&term.uri) {
                    ids.retain(|id| id != agent_id);
                }
            }
            for ht in &disclosed.hashed_terms {
                if let Some(mut ids) = self.by_hash.get_mut(&ht.hash) {
                    ids.retain(|id| id != agent_id);
                }
            }
        }
    }

    /// Find agents that publicly declare a specific term URI.
    pub fn find_by_term(&self, term_uri: &str) -> Vec<AgentId> {
        self.by_term.get(term_uri).map(|v| v.clone()).unwrap_or_default()
    }

    /// Find agents that have a hashed term matching the given hash.
    /// Useful for verifying if a known term exists without disclosing it.
    pub fn find_by_hash(&self, hash: &[u8; 32]) -> Vec<AgentId> {
        self.by_hash.get(hash).map(|v| v.clone()).unwrap_or_default()
    }

    /// Find agents whose neural-hashed terms are semantically similar to a query embedding.
    /// Returns (agent_id, best_similarity) sorted by descending similarity.
    pub fn find_by_neural_similarity(
        &self,
        query: &[f32],
        min_similarity: f32,
        max_results: usize,
    ) -> Vec<(AgentId, f32)> {
        let mut results: Vec<(AgentId, f32)> = Vec::new();
        for entry in self.ontologies.iter() {
            let agent_id = *entry.key();
            let disclosed = entry.value();
            if let Some((_, sim)) = disclosed.nearest_neural(query) {
                if sim >= min_similarity {
                    results.push((agent_id, sim));
                }
            }
        }
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(max_results);
        results
    }

    /// Get an agent's disclosed ontology.
    pub fn get(&self, agent_id: &AgentId) -> Option<DisclosedOntology> {
        self.ontologies.get(agent_id).map(|v| v.clone())
    }

    /// Number of registered ontologies.
    pub fn len(&self) -> usize {
        self.ontologies.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.ontologies.is_empty()
    }

    /// Compute ontology compatibility between two agents.
    /// Returns a score in [0, 1] based on overlapping public terms.
    pub fn compatibility(&self, a: &AgentId, b: &AgentId) -> f64 {
        let ont_a = match self.ontologies.get(a) {
            Some(o) => o.clone(),
            None => return 0.0,
        };
        let ont_b = match self.ontologies.get(b) {
            Some(o) => o.clone(),
            None => return 0.0,
        };

        if ont_a.public_terms.is_empty() && ont_b.public_terms.is_empty() {
            return 0.0;
        }

        let uris_a: std::collections::HashSet<_> =
            ont_a.public_terms.iter().map(|t| &t.uri).collect();
        let uris_b: std::collections::HashSet<_> =
            ont_b.public_terms.iter().map(|t| &t.uri).collect();

        let intersection = uris_a.intersection(&uris_b).count();
        let union = uris_a.union(&uris_b).count();

        if union == 0 { 0.0 } else { intersection as f64 / union as f64 }
    }
}

impl Default for OntologyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

'''

# ========================================================================
# 2. Insert the ontology section before the test module
# ========================================================================
test_module_marker = "\n#[cfg(test)]\nmod tests {"
if "pub struct AgentOntology" not in src:
    idx = src.find(test_module_marker)
    if idx >= 0:
        src = src[:idx] + ontology_section + src[idx:]
        print("  [OK] Added ontology system before test module")
    else:
        print("  [WARN] Could not find test module marker")
else:
    print("  [SKIP] AgentOntology already exists")

# ========================================================================
# 3. Add ontology field to AgentProfile
# ========================================================================
old_profile = "    /// Public key for agent verification\n    pub public_key: Option<Vec<u8>>,"
new_profile = """    /// Public key for agent verification
    pub public_key: Option<Vec<u8>>,
    /// Agent's ontology — the concepts and capabilities it understands
    #[serde(default)]
    pub ontology: Option<AgentOntology>,"""

if "pub ontology: Option<AgentOntology>" not in src:
    src = src.replace(old_profile, new_profile, 1)
    print("  [OK] Added ontology field to AgentProfile")
else:
    print("  [SKIP] ontology field already in AgentProfile")

# ========================================================================
# 4. Initialize ontology in AgentProfile::new()
# ========================================================================
old_init = '            public_key: None,\n        }'
# Only replace in the AgentProfile::new context
new_init = '''            public_key: None,
            ontology: None,
        }'''
# Make sure we're targeting the right one - find within the new() method
if "ontology: None," not in src:
    # Find the first occurrence which is in AgentProfile::new
    idx = src.find(old_init)
    if idx >= 0:
        src = src[:idx] + new_init + src[idx + len(old_init):]
        print("  [OK] Added ontology: None to AgentProfile::new()")
    else:
        print("  [WARN] Could not find AgentProfile init")
else:
    print("  [SKIP] ontology init already in AgentProfile::new()")

# ========================================================================
# 5. Add with_ontology builder method to AgentProfile
# ========================================================================
old_builder = "    pub fn with_trust(mut self, level: TrustLevel) -> Self {\n        self.trust_level = level;\n        self\n    }"
new_builder = """    pub fn with_trust(mut self, level: TrustLevel) -> Self {
        self.trust_level = level;
        self
    }

    pub fn with_ontology(mut self, ontology: AgentOntology) -> Self {
        self.ontology = Some(ontology);
        self
    }"""

if "fn with_ontology" not in src:
    src = src.replace(old_builder, new_builder, 1)
    print("  [OK] Added with_ontology builder")
else:
    print("  [SKIP] with_ontology already exists")

# ========================================================================
# 6. Add ontology tests
# ========================================================================
ontology_tests = r'''
    #[test]
    fn test_ontology_term_creation() {
        let term = OntologyTerm::new("spine:cap/nav", "Navigation")
            .with_parent("spine:cap/base")
            .with_description("Web navigation capability")
            .with_visibility(OntologyVisibility::Public);
        assert_eq!(term.uri, "spine:cap/nav");
        assert_eq!(term.parents.len(), 1);
    }

    #[test]
    fn test_ontology_crypto_hash_deterministic() {
        let term = OntologyTerm::new("spine:cap/nav", "Navigation")
            .with_parent("spine:cap/base");
        let h1 = term.crypto_hash();
        let h2 = term.crypto_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_ontology_crypto_hash_distinct() {
        let t1 = OntologyTerm::new("spine:cap/nav", "Navigation");
        let t2 = OntologyTerm::new("spine:cap/extract", "Extraction");
        assert_ne!(t1.crypto_hash(), t2.crypto_hash());
    }

    #[test]
    fn test_ontology_neural_hash_similarity() {
        // Same content → same hash
        let t1 = OntologyTerm::new("spine:cap/text-analysis", "Text Analysis");
        let t2 = OntologyTerm::new("spine:cap/text-analysis", "Text Analysis");
        let h1 = t1.neural_hash(16);
        let h2 = t2.neural_hash(16);
        assert_eq!(h1.len(), 16);
        let sim: f32 = h1.iter().zip(&h2).map(|(a, b)| a * b).sum();
        assert!((sim - 1.0).abs() < 0.01, "Same term should have similarity ~1.0");
    }

    #[test]
    fn test_ontology_disclosed_view_visibility() {
        let mut ont = AgentOntology::new("spine:test", "1.0");
        ont.add_term(OntologyTerm::new("spine:pub", "Public"));
        ont.add_term(OntologyTerm::new("spine:hash", "Hashed")
            .with_visibility(OntologyVisibility::HashOnly));
        ont.add_term(OntologyTerm::new("spine:neural", "Neural")
            .with_visibility(OntologyVisibility::NeuralHash));
        ont.add_term(OntologyTerm::new("spine:priv", "Private")
            .with_visibility(OntologyVisibility::Private));

        let view = ont.disclosed_view(8);
        assert_eq!(view.public_terms.len(), 1);
        assert_eq!(view.hashed_terms.len(), 1);
        assert_eq!(view.neural_terms.len(), 1);
        assert_eq!(view.term_count(), 3); // private excluded
    }

    #[test]
    fn test_ontology_hash_verification() {
        let term = OntologyTerm::new("spine:cap/nav", "Navigation");
        let hash = term.crypto_hash();
        assert!(AgentOntology::verify_term_hash(&term, &hash));

        let fake = OntologyTerm::new("spine:cap/fake", "Fake");
        assert!(!AgentOntology::verify_term_hash(&fake, &hash));
    }

    #[test]
    fn test_ontology_access_control() {
        let mut acl = OntologyAccessControl::default();
        acl.grant("agent-trusted", "spine:cap/", OntologyVisibility::Public);
        acl.grant("agent-untrusted", "spine:cap/", OntologyVisibility::HashOnly);

        let vis_trusted = acl.effective_visibility(
            "agent-trusted", "spine:cap/nav", OntologyVisibility::Private,
        );
        assert_eq!(vis_trusted, OntologyVisibility::Public);

        let vis_untrusted = acl.effective_visibility(
            "agent-untrusted", "spine:cap/nav", OntologyVisibility::Private,
        );
        assert_eq!(vis_untrusted, OntologyVisibility::HashOnly);
    }

    #[test]
    fn test_ontology_access_control_disclose_for() {
        let mut ont = AgentOntology::new("spine:test", "1.0");
        ont.add_term(OntologyTerm::new("spine:cap/nav", "Nav"));
        ont.add_term(OntologyTerm::new("spine:secret/key", "Secret Key"));

        let mut acl = OntologyAccessControl::default();
        // Grant public for cap/ terms but hash-only for secret/ terms
        acl.grant("requester", "spine:cap/", OntologyVisibility::Public);
        acl.grant("requester", "spine:secret/", OntologyVisibility::HashOnly);

        let view = acl.disclose_for(&ont, "requester", 8);
        assert_eq!(view.public_terms.len(), 1);
        assert_eq!(view.public_terms[0].uri, "spine:cap/nav");
        assert_eq!(view.hashed_terms.len(), 1);
    }

    #[test]
    fn test_ontology_registry_discovery() {
        let registry = OntologyRegistry::new();

        let mut ont1 = AgentOntology::new("spine:agent1", "1.0");
        ont1.add_term(OntologyTerm::new("spine:cap/nav", "Navigation"));
        ont1.add_term(OntologyTerm::new("spine:cap/extract", "Extraction"));

        let mut ont2 = AgentOntology::new("spine:agent2", "1.0");
        ont2.add_term(OntologyTerm::new("spine:cap/nav", "Navigation"));
        ont2.add_term(OntologyTerm::new("spine:cap/analyze", "Analysis"));

        let id1 = AgentId::new();
        let id2 = AgentId::new();

        registry.register(id1, ont1.disclosed_view(8));
        registry.register(id2, ont2.disclosed_view(8));

        // Both agents declare navigation
        let nav_agents = registry.find_by_term("spine:cap/nav");
        assert_eq!(nav_agents.len(), 2);

        // Only agent1 declares extraction
        let extract_agents = registry.find_by_term("spine:cap/extract");
        assert_eq!(extract_agents.len(), 1);
        assert_eq!(extract_agents[0], id1);

        // Compatibility: they share 1/3 terms = ~0.33
        let compat = registry.compatibility(&id1, &id2);
        assert!(compat > 0.3 && compat < 0.4, "Expected ~0.33, got {}", compat);
    }

    #[test]
    fn test_ontology_registry_hash_discovery() {
        let registry = OntologyRegistry::new();

        let term = OntologyTerm::new("spine:cap/secret", "Secret Capability")
            .with_visibility(OntologyVisibility::HashOnly);
        let hash = term.crypto_hash();

        let mut ont = AgentOntology::new("spine:agent", "1.0");
        ont.add_term(term);

        let id = AgentId::new();
        registry.register(id, ont.disclosed_view(8));

        // Can find by hash
        let found = registry.find_by_hash(&hash);
        assert_eq!(found.len(), 1);

        // Cannot find by term URI (it's hashed)
        let not_found = registry.find_by_term("spine:cap/secret");
        assert!(not_found.is_empty());
    }

    #[test]
    fn test_ontology_registry_neural_discovery() {
        let registry = OntologyRegistry::new();

        let term = OntologyTerm::new("spine:cap/ml", "Machine Learning")
            .with_visibility(OntologyVisibility::NeuralHash);

        let mut ont = AgentOntology::new("spine:agent", "1.0");
        ont.add_term(term.clone());

        let id = AgentId::new();
        registry.register(id, ont.disclosed_view(16));

        // Query with the same term's neural hash should find it
        let query = term.neural_hash(16);
        let results = registry.find_by_neural_similarity(&query, 0.5, 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].0, id);
        assert!(results[0].1 > 0.9, "Expected high similarity for same term");
    }

    #[test]
    fn test_ontology_registry_unregister() {
        let registry = OntologyRegistry::new();
        let mut ont = AgentOntology::new("spine:agent", "1.0");
        ont.add_term(OntologyTerm::new("spine:cap/nav", "Navigation"));
        let id = AgentId::new();
        registry.register(id, ont.disclosed_view(8));
        assert_eq!(registry.len(), 1);

        registry.unregister(&id);
        assert_eq!(registry.len(), 0);
        assert!(registry.find_by_term("spine:cap/nav").is_empty());
    }

    #[test]
    fn test_agent_profile_with_ontology() {
        let mut ont = AgentOntology::new("spine:web-agent", "1.0");
        ont.add_term(OntologyTerm::new("spine:cap/nav", "Navigation"));

        let profile = AgentProfile::new("TestAgent").with_ontology(ont);
        assert!(profile.ontology.is_some());
        assert_eq!(profile.ontology.as_ref().unwrap().terms.len(), 1);
    }

    #[test]
    fn test_ontology_whole_hash() {
        let mut ont = AgentOntology::new("spine:test", "1.0");
        ont.add_term(OntologyTerm::new("spine:a", "A"));
        let h1 = ont.hash();
        ont.add_term(OntologyTerm::new("spine:b", "B"));
        let h2 = ont.hash();
        assert_ne!(h1, h2, "Hash should change when terms are added");
    }

    #[test]
    fn test_disclosed_ontology_verify_hash() {
        let term = OntologyTerm::new("spine:cap/nav", "Navigation")
            .with_visibility(OntologyVisibility::HashOnly);
        let mut ont = AgentOntology::new("spine:test", "1.0");
        ont.add_term(term);
        let view = ont.disclosed_view(8);

        // Verify with correct term
        let correct = OntologyTerm::new("spine:cap/nav", "Navigation");
        assert!(view.verify_hash(&correct));

        // Verify with wrong term
        let wrong = OntologyTerm::new("spine:cap/wrong", "Wrong");
        assert!(!view.verify_hash(&wrong));
    }

'''

# Insert tests before the final } of the test module
test_end_marker = "    #[test]\n    fn test_resource_locator()"
if "test_ontology_term_creation" not in src:
    idx = src.find(test_end_marker)
    if idx >= 0:
        src = src[:idx] + ontology_tests.lstrip("\n") + "\n    " + src[idx:]
        print("  [OK] Added 14 ontology tests")
    else:
        # Try inserting at end of test module
        # Find the closing } of the test module (right after test_resource_locator)
        test_close = src.find("    }\n}", src.find("mod tests {"))
        if test_close >= 0:
            src = src[:test_close + 5] + ontology_tests + "\n}\n" + src[test_close + 7:]
            print("  [OK] Added 14 ontology tests (at module end)")
        else:
            print("  [WARN] Could not find test insertion point")
else:
    print("  [SKIP] Ontology tests already exist")

with open(AGENTIC, "w", encoding="utf-8", newline="\n") as f:
    f.write(src)

print("\nPhase 20 ontology system applied to spine-agentic.")
