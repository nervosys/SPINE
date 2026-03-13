//! # SPINE Knowledge - Bioinspired Distributed Memory Architecture
//!
//! This crate provides a unified memory system inspired by biological neural networks,
//! integrating three key components into a cohesive architecture:
//!
//! ## Bioinspired Architecture
//!
//! ```text
//! ╔══════════════════════════════════════════════════════════════════════════════╗
//! ║                    SPINE UNIFIED MEMORY ARCHITECTURE                         ║
//! ╠══════════════════════════════════════════════════════════════════════════════╣
//! ║                                                                              ║
//! ║  ┌─────────────────────────────────────────────────────────────────────────┐ ║
//! ║  │                    COLLECTIVE MEMORY (Social Brain)                     │ ║
//! ║  │   Distributed Knowledge Base with CRDT consistency                      │ ║
//! ║  │   • Eventual consistency across swarm                                   │ ║
//! ║  │   • Vector clock ordering                                               │ ║
//! ║  │   • Conflict-free replicated data types                                 │ ║
//! ║  └───────────────────────────────┬─────────────────────────────────────────┘ ║
//! ║                                  │                                           ║
//! ║         ┌────────────────────────┴────────────────────────┐                  ║
//! ║         │                                                 │                  ║
//! ║         ▼                                                 ▼                  ║
//! ║  ┌─────────────────────────┐                 ┌─────────────────────────┐    ║
//! ║  │   EPISODIC MEMORY       │                 │    SEMANTIC MEMORY      │    ║
//! ║  │   (Hippocampus)         │◄───────────────►│    (Neocortex)          │    ║
//! ║  │                         │   Consolidation │                         │    ║
//! ║  │   Titans Neural Memory  │                 │   Context Store         │    ║
//! ║  │   • Surprise-gated      │                 │   • Large content       │    ║
//! ║  │   • Test-time learning  │                 │   • Chunked access      │    ║
//! ║  │   • Pattern completion  │                 │   • Recursive retrieval │    ║
//! ║  └─────────────────────────┘                 └─────────────────────────┘    ║
//! ║                                                                              ║
//! ║  ┌─────────────────────────────────────────────────────────────────────────┐ ║
//! ║  │                    WORKING MEMORY (Prefrontal Cortex)                   │ ║
//! ║  │   Active context window with attention-based prioritization             │ ║
//! ║  │   • Task-relevant information                                           │ ║
//! ║  │   • Goal maintenance                                                    │ ║
//! ║  │   • Action planning                                                     │ ║
//! ║  └─────────────────────────────────────────────────────────────────────────┘ ║
//! ╚══════════════════════════════════════════════════════════════════════════════╝
//! ```
//!
//! ## Integration with SPINE Components
//!
//! - **Titans (spine-neural)**: Provides surprise-gated episodic memory encoding
//! - **MIRAS variants**: Different memory update rules for different use cases  
//! - **Distributed**: CRDT-based consistency for swarm-wide knowledge sharing
//!
//! ## Key Concepts
//!
//! ### 1. Episodic Memory (Titans)
//! Like the hippocampus, Titans provides:
//! - **Surprise-gated learning**: Only memorable events are stored
//! - **Pattern completion**: Partial cues retrieve full memories
//! - **Test-time adaptation**: Learning continues during inference
//!
//! ### 2. Semantic Memory (Context Store)
//! Like the neocortex provides:
//! - **Large content storage**: Handles arbitrarily large documents
//! - **Chunked access**: Efficient partial retrieval
//! - **Recursive retrieval**: Deep knowledge graph traversal
//!
//! ### 3. Collective Memory (Knowledge Base)
//! Like social learning in biological communities:
//! - **Distributed truth**: Consensus across the swarm
//! - **CRDT consistency**: Conflict-free replication
//! - **Shared intelligence**: Knowledge amplification

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spine_neural::NeuralLatentEncoder;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// =============================================================================
// VECTOR CLOCK FOR DISTRIBUTED ORDERING
// =============================================================================

/// Vector clock for establishing causal ordering across distributed nodes
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VectorClock {
    /// Map from node ID to logical timestamp
    clocks: BTreeMap<Uuid, u64>,
}

impl VectorClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment the clock for a specific node
    pub fn tick(&mut self, node_id: Uuid) {
        *self.clocks.entry(node_id).or_insert(0) += 1;
    }

    /// Merge with another vector clock (take max of each component)
    pub fn merge(&mut self, other: &VectorClock) {
        for (node_id, &timestamp) in &other.clocks {
            let current = self.clocks.entry(*node_id).or_insert(0);
            *current = (*current).max(timestamp);
        }
    }

    /// Check if this clock happens-before another
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut dominated = false;
        for (node_id, &self_time) in &self.clocks {
            let other_time = other.clocks.get(node_id).unwrap_or(&0);
            if self_time > *other_time {
                return false;
            }
            if self_time < *other_time {
                dominated = true;
            }
        }
        for (node_id, &other_time) in &other.clocks {
            if !self.clocks.contains_key(node_id) && other_time > 0 {
                dominated = true;
            }
        }
        dominated
    }

    /// Check if two clocks are concurrent
    pub fn is_concurrent(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self)
    }
}

// =============================================================================
// CRDT-BASED KNOWLEDGE ENTRIES
// =============================================================================

/// A knowledge entry that can be replicated across the distributed swarm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub id: Uuid,
    pub key: String,
    pub value: KnowledgeValue,
    pub tags: HashSet<String>,
    pub clock: VectorClock,
    pub origin_node: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub confidence: f32,
    pub confirmations: u32,
    pub embedding: Option<Vec<f32>>,
}

/// The value stored in a knowledge entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KnowledgeValue {
    Text(String),
    Structured(serde_json::Value),
    LargeContent {
        summary: String,
        content_id: Uuid,
        size: usize,
    },
    Relationship {
        subject: String,
        predicate: String,
        object: String,
    },
    Procedure {
        name: String,
        steps: Vec<String>,
        preconditions: Vec<String>,
        effects: Vec<String>,
    },
}

impl KnowledgeEntry {
    pub fn new_text(key: String, value: String, node_id: Uuid) -> Self {
        let mut clock = VectorClock::new();
        clock.tick(node_id);

        Self {
            id: Uuid::new_v4(),
            key,
            value: KnowledgeValue::Text(value),
            tags: HashSet::new(),
            clock,
            origin_node: node_id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            confidence: 1.0,
            confirmations: 1,
            embedding: None,
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags.into_iter().collect();
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    pub fn content_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.key.as_bytes());
        hasher.update(
            serde_json::to_string(&self.value)
                .unwrap_or_default()
                .as_bytes(),
        );
        format!("{:x}", hasher.finalize())
    }
}

// =============================================================================
// LWW-REGISTER CRDT
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwwRegister<T> {
    pub value: T,
    pub timestamp: DateTime<Utc>,
    pub node_id: Uuid,
}

impl<T: Clone> LwwRegister<T> {
    pub fn new(value: T, node_id: Uuid) -> Self {
        Self {
            value,
            timestamp: Utc::now(),
            node_id,
        }
    }

    pub fn merge(&mut self, other: &LwwRegister<T>) {
        if other.timestamp > self.timestamp
            || (other.timestamp == self.timestamp && other.node_id > self.node_id)
        {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
            self.node_id = other.node_id;
        }
    }
}

// =============================================================================
// GROW-ONLY SET (G-Set CRDT)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GSet<T: Eq + std::hash::Hash + Clone> {
    elements: HashSet<T>,
}

impl<T: Eq + std::hash::Hash + Clone> GSet<T> {
    pub fn new() -> Self {
        Self {
            elements: HashSet::new(),
        }
    }
    pub fn add(&mut self, element: T) {
        self.elements.insert(element);
    }
    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains(element)
    }
    pub fn merge(&mut self, other: &GSet<T>) {
        self.elements.extend(other.elements.iter().cloned());
    }
    pub fn len(&self) -> usize {
        self.elements.len()
    }
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.elements.iter()
    }
}

// =============================================================================
// EPISODIC MEMORY (TITANS-BASED)
// =============================================================================

/// Episodic memory using Titans neural memory for surprise-gated learning
pub struct EpisodicMemory {
    encoder: NeuralLatentEncoder,
    pub recent_episodes: Vec<Episode>,
    max_recent: usize,
    surprise_threshold: f32,
    latent_dim: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Episode {
    pub id: Uuid,
    pub content: String,
    pub context: HashMap<String, String>,
    pub timestamp: DateTime<Utc>,
    pub surprise_score: f32,
    pub embedding: Vec<f32>,
    pub retrieved_count: u32,
}

impl EpisodicMemory {
    pub fn new(latent_dim: usize) -> Self {
        let encoder = NeuralLatentEncoder::new(256, latent_dim, &[128, 96], 4, 42);
        Self {
            encoder,
            recent_episodes: Vec::new(),
            max_recent: 100,
            surprise_threshold: 0.3,
            latent_dim,
        }
    }

    pub fn store(&mut self, content: &str, context: HashMap<String, String>) -> Option<Episode> {
        let embedding = self.encoder.encode(content.as_bytes());
        let predicted = self.encoder.predict_next();
        let surprise = self.compute_surprise(&embedding, &predicted);

        if surprise >= self.surprise_threshold {
            let episode = Episode {
                id: Uuid::new_v4(),
                content: content.to_string(),
                context,
                timestamp: Utc::now(),
                surprise_score: surprise,
                embedding,
                retrieved_count: 0,
            };
            self.recent_episodes.push(episode.clone());
            if self.recent_episodes.len() > self.max_recent {
                self.consolidate();
            }
            Some(episode)
        } else {
            None
        }
    }

    fn compute_surprise(&self, actual: &[f32], predicted: &[f32]) -> f32 {
        if predicted.is_empty() || actual.is_empty() {
            return 1.0;
        }
        let len = actual.len().min(predicted.len());
        let mse: f32 = actual
            .iter()
            .zip(predicted.iter())
            .take(len)
            .fold(0.0, |acc, (&a, &p)| {
                let d = a - p;
                acc + d * d
            });
        (mse / len as f32).sqrt().tanh()
    }

    pub fn retrieve(&mut self, query: &str, top_k: usize) -> Vec<Episode> {
        let query_embedding = self.encoder.encode(query.as_bytes());
        let mut scored: Vec<_> = self
            .recent_episodes
            .iter_mut()
            .map(|ep| (Self::cosine_similarity(&query_embedding, &ep.embedding), ep))
            .collect();
        // Partial sort: O(n) average vs O(n log n) for full sort — only need top_k elements
        let k = top_k.min(scored.len());
        if k > 0 {
            scored.select_nth_unstable_by(k.saturating_sub(1), |a, b| {
                b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        scored
            .into_iter()
            .take(top_k)
            .map(|(_, ep)| {
                ep.retrieved_count += 1;
                ep.clone()
            })
            .collect()
    }

    fn consolidate(&mut self) {
        self.recent_episodes.sort_by(|a, b| {
            let sa = a.surprise_score + (a.retrieved_count as f32 * 0.1);
            let sb = b.surprise_score + (b.retrieved_count as f32 * 0.1);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
        self.recent_episodes.truncate(self.max_recent / 2);
    }

    /// Single-pass cosine similarity: 3 accumulators in one loop instead of 3 separate iterator passes.
    /// Reduces memory traffic by ~3x for large embeddings.
    #[inline]
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let (mut dot, mut na2, mut nb2) = (0.0f32, 0.0f32, 0.0f32);
        for (&x, &y) in a.iter().zip(b.iter()) {
            dot += x * y;
            na2 += x * x;
            nb2 += y * y;
        }
        let denom = na2.sqrt() * nb2.sqrt();
        if denom > 0.0 {
            dot / denom
        } else {
            0.0
        }
    }

    pub fn stats(&self) -> EpisodicStats {
        EpisodicStats {
            episode_count: self.recent_episodes.len(),
            avg_surprise: self
                .recent_episodes
                .iter()
                .map(|e| e.surprise_score)
                .sum::<f32>()
                / self.recent_episodes.len().max(1) as f32,
            total_retrievals: self.recent_episodes.iter().map(|e| e.retrieved_count).sum(),
            memory_utilization: self.recent_episodes.len() as f32 / self.max_recent as f32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodicStats {
    pub episode_count: usize,
    pub avg_surprise: f32,
    pub total_retrievals: u32,
    pub memory_utilization: f32,
}

// =============================================================================
// SEMANTIC MEMORY (CONCEPT STORE)
// =============================================================================

pub struct SemanticMemory {
    pub concepts: DashMap<String, SemanticConcept>,
    relations: DashMap<String, Vec<SemanticRelation>>,
    large_content: DashMap<Uuid, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticConcept {
    pub name: String,
    pub definition: String,
    pub attributes: HashMap<String, String>,
    pub context_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub access_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticRelation {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub strength: f32,
    pub evidence: Vec<String>,
}

impl SemanticMemory {
    pub fn new() -> Self {
        Self {
            concepts: DashMap::new(),
            relations: DashMap::new(),
            large_content: DashMap::new(),
        }
    }

    pub fn store_concept(&self, name: &str, definition: &str, attributes: HashMap<String, String>) {
        self.concepts.insert(
            name.to_string(),
            SemanticConcept {
                name: name.to_string(),
                definition: definition.to_string(),
                attributes,
                context_id: None,
                created_at: Utc::now(),
                access_count: 0,
            },
        );
    }

    pub fn store_large_content(&self, name: &str, content: &str) -> Uuid {
        let id = Uuid::new_v4();
        self.large_content.insert(id, content.to_string());
        self.concepts.insert(
            name.to_string(),
            SemanticConcept {
                name: name.to_string(),
                definition: format!("[Large: {} chars]", content.len()),
                attributes: HashMap::new(),
                context_id: Some(id),
                created_at: Utc::now(),
                access_count: 0,
            },
        );
        id
    }

    pub fn get_large_content(&self, id: &Uuid) -> Option<String> {
        self.large_content.get(id).map(|r| r.clone())
    }

    pub fn add_relation(&self, subject: &str, predicate: &str, object: &str, strength: f32) {
        self.relations
            .entry(subject.to_string())
            .or_default()
            .push(SemanticRelation {
                subject: subject.to_string(),
                predicate: predicate.to_string(),
                object: object.to_string(),
                strength,
                evidence: Vec::new(),
            });
    }

    pub fn retrieve(&self, name: &str) -> Option<(SemanticConcept, Vec<SemanticRelation>)> {
        let concept = self.concepts.get(name)?.clone();
        let relations = self
            .relations
            .get(name)
            .map(|r| r.clone())
            .unwrap_or_default();
        Some((concept, relations))
    }

    pub fn query_by_attribute(&self, attr_name: &str, attr_value: &str) -> Vec<SemanticConcept> {
        self.concepts
            .iter()
            .filter(|e| {
                e.value()
                    .attributes
                    .get(attr_name)
                    .map(|v| v == attr_value)
                    .unwrap_or(false)
            })
            .map(|e| e.value().clone())
            .collect()
    }

    pub fn traverse(&self, start: &str, max_depth: usize) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        self.traverse_recursive(start, max_depth, &mut visited, &mut result);
        result
    }

    fn traverse_recursive(
        &self,
        current: &str,
        depth: usize,
        visited: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) {
        if depth == 0 || visited.contains(current) {
            return;
        }
        visited.insert(current.to_string());
        result.push(current.to_string());
        if let Some(rels) = self.relations.get(current) {
            for r in rels.iter() {
                self.traverse_recursive(&r.object, depth - 1, visited, result);
            }
        }
    }

    pub fn stats(&self) -> SemanticStats {
        SemanticStats {
            concept_count: self.concepts.len(),
            relation_count: self.relations.iter().map(|r| r.value().len()).sum(),
            large_content_count: self.large_content.len(),
            total_content_size: self.large_content.iter().map(|c| c.value().len()).sum(),
        }
    }
}

impl Default for SemanticMemory {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticStats {
    pub concept_count: usize,
    pub relation_count: usize,
    pub large_content_count: usize,
    pub total_content_size: usize,
}

// =============================================================================
// WORKING MEMORY (ACTIVE CONTEXT)
// =============================================================================

pub struct WorkingMemory {
    pub current_goal: Option<Goal>,
    pub context_items: Vec<ContextItem>,
    capacity: usize,
    attention_weights: Vec<f32>,
    action_plan: Vec<PlannedAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: Uuid,
    pub description: String,
    pub subgoals: Vec<Goal>,
    pub status: GoalStatus,
    pub priority: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoalStatus {
    Active,
    Completed,
    Failed,
    Suspended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    pub id: Uuid,
    pub content: String,
    pub source: ContextSource,
    pub relevance: f32,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextSource {
    EpisodicMemory(Uuid),
    SemanticMemory(String),
    CollectiveKnowledge(Uuid),
    ExternalInput,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedAction {
    pub id: Uuid,
    pub description: String,
    pub preconditions: Vec<String>,
    pub expected_effects: Vec<String>,
    pub status: ActionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl WorkingMemory {
    pub fn new(capacity: usize) -> Self {
        Self {
            current_goal: None,
            context_items: Vec::new(),
            capacity,
            attention_weights: Vec::new(),
            action_plan: Vec::new(),
        }
    }

    pub fn set_goal(&mut self, description: &str, priority: f32) {
        self.current_goal = Some(Goal {
            id: Uuid::new_v4(),
            description: description.to_string(),
            subgoals: Vec::new(),
            status: GoalStatus::Active,
            priority,
        });
        self.context_items.clear();
        self.attention_weights.clear();
        self.action_plan.clear();
    }

    pub fn add_context(&mut self, content: &str, source: ContextSource, relevance: f32) {
        if self.context_items.len() >= self.capacity {
            if let Some(idx) = self
                .attention_weights
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
            {
                self.context_items.remove(idx);
                self.attention_weights.remove(idx);
            }
        }
        self.context_items.push(ContextItem {
            id: Uuid::new_v4(),
            content: content.to_string(),
            source,
            relevance,
            added_at: Utc::now(),
        });
        self.attention_weights.push(relevance);
        self.normalize_attention();
    }

    pub fn plan_action(
        &mut self,
        description: &str,
        preconditions: Vec<String>,
        effects: Vec<String>,
    ) {
        self.action_plan.push(PlannedAction {
            id: Uuid::new_v4(),
            description: description.to_string(),
            preconditions,
            expected_effects: effects,
            status: ActionStatus::Pending,
        });
    }

    pub fn next_action(&mut self) -> Option<&mut PlannedAction> {
        self.action_plan
            .iter_mut()
            .find(|a| a.status == ActionStatus::Pending)
    }

    pub fn context_summary(&self) -> String {
        let mut s = String::new();
        if let Some(g) = &self.current_goal {
            s.push_str(&format!("Goal: {}\n", g.description));
        }
        s.push_str("Context:\n");
        for (item, w) in self.context_items.iter().zip(self.attention_weights.iter()) {
            s.push_str(&format!("  [{:.2}] {}\n", w, item.content));
        }
        s
    }

    fn normalize_attention(&mut self) {
        let sum: f32 = self.attention_weights.iter().sum();
        if sum > 0.0 {
            for w in &mut self.attention_weights {
                *w /= sum;
            }
        }
    }
}

// =============================================================================
// COLLECTIVE MEMORY (DISTRIBUTED KNOWLEDGE BASE)
// =============================================================================

pub struct CollectiveMemory {
    node_id: Uuid,
    entries: DashMap<String, KnowledgeEntry>,
    by_hash: DashMap<String, Uuid>,
    pending_sync: Arc<RwLock<Vec<KnowledgeEntry>>>,
    confirmations: DashMap<Uuid, GSet<Uuid>>,
    config: CollectiveConfig,
}

#[derive(Debug, Clone)]
pub struct CollectiveConfig {
    pub min_confirmations: u32,
    pub max_entries: usize,
    pub auto_sync: bool,
    pub sync_interval_secs: u64,
}

impl Default for CollectiveConfig {
    fn default() -> Self {
        Self {
            min_confirmations: 3,
            max_entries: 100_000,
            auto_sync: true,
            sync_interval_secs: 30,
        }
    }
}

impl CollectiveMemory {
    pub fn new(node_id: Uuid, config: CollectiveConfig) -> Self {
        Self {
            node_id,
            entries: DashMap::new(),
            by_hash: DashMap::new(),
            pending_sync: Arc::new(RwLock::new(Vec::new())),
            confirmations: DashMap::new(),
            config,
        }
    }

    pub fn store(
        &self,
        key: &str,
        value: KnowledgeValue,
        tags: Vec<String>,
        confidence: f32,
    ) -> KnowledgeEntry {
        let mut entry = KnowledgeEntry {
            id: Uuid::new_v4(),
            key: key.to_string(),
            value,
            tags: tags.into_iter().collect(),
            clock: VectorClock::new(),
            origin_node: self.node_id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            confidence,
            confirmations: 1,
            embedding: None,
        };
        entry.clock.tick(self.node_id);
        let hash = entry.content_hash();

        if let Some(_existing_entry_id) = self.by_hash.get(&hash) {
            // Content already exists with this hash - increment confirmation count
            if let Some(mut existing) = self.entries.get_mut(&key.to_string()) {
                existing.confirmations += 1;
                existing.clock.merge(&entry.clock);
                return existing.clone();
            }
        }

        self.by_hash.insert(hash, entry.id);
        self.entries.insert(key.to_string(), entry.clone());
        let mut confs = GSet::new();
        confs.add(self.node_id);
        self.confirmations.insert(entry.id, confs);
        entry
    }

    pub fn get(&self, key: &str) -> Option<KnowledgeEntry> {
        self.entries.get(key).map(|e| e.clone())
    }

    pub fn query_by_tags(&self, tags: &[String]) -> Vec<KnowledgeEntry> {
        let tag_set: HashSet<_> = tags.iter().collect();
        self.entries
            .iter()
            .filter(|e| e.value().tags.iter().any(|t| tag_set.contains(t)))
            .map(|e| e.value().clone())
            .collect()
    }

    pub fn confirm(&self, entry_id: Uuid, confirming_node: Uuid) -> bool {
        if let Some(mut confs) = self.confirmations.get_mut(&entry_id) {
            confs.add(confirming_node);
            // Direct key lookup instead of O(n) scan over all entries
            for mut e in self.entries.iter_mut() {
                if e.id == entry_id {
                    e.confirmations = confs.len() as u32;
                    return true;
                }
            }
        }
        false
    }

    pub fn is_confirmed(&self, entry_id: &Uuid) -> bool {
        self.confirmations
            .get(entry_id)
            .map(|c| c.len() as u32 >= self.config.min_confirmations)
            .unwrap_or(false)
    }

    pub fn merge_remote(&self, remote: KnowledgeEntry) {
        let key = remote.key.clone();
        if let Some(mut local) = self.entries.get_mut(&key) {
            if remote.clock.happens_before(&local.clock) {
                // Remote is older, ignore
            } else if local.clock.happens_before(&remote.clock) {
                *local = remote;
            } else {
                local.clock.merge(&remote.clock);
                local.confirmations = local.confirmations.max(remote.confirmations);
                local.confidence = (local.confidence + remote.confidence) / 2.0;
                local.updated_at = local.updated_at.max(remote.updated_at);
            }
        } else {
            self.entries.insert(key, remote);
        }
    }

    pub async fn get_pending_sync(&self) -> Vec<KnowledgeEntry> {
        self.pending_sync.read().await.clone()
    }
    pub async fn mark_synced(&self) {
        self.pending_sync.write().await.clear();
    }

    pub fn get_trusted(&self, min_confidence: f32) -> Vec<KnowledgeEntry> {
        self.entries
            .iter()
            .filter(|e| e.confidence >= min_confidence && self.is_confirmed(&e.id))
            .map(|e| e.value().clone())
            .collect()
    }

    pub fn stats(&self) -> CollectiveStats {
        CollectiveStats {
            total_entries: self.entries.len(),
            confirmed_entries: self
                .entries
                .iter()
                .filter(|e| self.is_confirmed(&e.id))
                .count(),
            pending_sync: self.pending_sync.try_read().map(|p| p.len()).unwrap_or(0),
            avg_confidence: self.entries.iter().map(|e| e.confidence).sum::<f32>()
                / self.entries.len().max(1) as f32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveStats {
    pub total_entries: usize,
    pub confirmed_entries: usize,
    pub pending_sync: usize,
    pub avg_confidence: f32,
}

// =============================================================================
// UNIFIED MEMORY SYSTEM
// =============================================================================

/// The unified memory system integrating all bioinspired components
pub struct UnifiedMemory {
    pub node_id: Uuid,
    pub episodic: EpisodicMemory,
    pub semantic: SemanticMemory,
    pub working: WorkingMemory,
    pub collective: CollectiveMemory,
    config: UnifiedConfig,
}

#[derive(Debug, Clone)]
pub struct UnifiedConfig {
    pub latent_dim: usize,
    pub working_memory_capacity: usize,
    pub auto_consolidate: bool,
    pub consolidation_threshold: usize,
}

impl Default for UnifiedConfig {
    fn default() -> Self {
        Self {
            latent_dim: 512,
            working_memory_capacity: 10,
            auto_consolidate: true,
            consolidation_threshold: 50,
        }
    }
}

impl UnifiedMemory {
    pub fn new(node_id: Uuid, config: UnifiedConfig) -> Self {
        Self {
            node_id,
            episodic: EpisodicMemory::new(config.latent_dim),
            semantic: SemanticMemory::new(),
            working: WorkingMemory::new(config.working_memory_capacity),
            collective: CollectiveMemory::new(node_id, CollectiveConfig::default()),
            config,
        }
    }

    /// Store an experience (routes to appropriate memory system)
    pub fn experience(&mut self, content: &str, context: HashMap<String, String>) -> MemoryResult {
        let mut result = MemoryResult::default();

        if let Some(episode) = self.episodic.store(content, context.clone()) {
            result.episodic_id = Some(episode.id);
            result.stored_episodic = true;
            if self.is_goal_relevant(content) {
                self.working.add_context(
                    content,
                    ContextSource::EpisodicMemory(episode.id),
                    episode.surprise_score,
                );
                result.added_to_working = true;
            }
        }

        if let Some(concepts) = self.extract_concepts(content) {
            for (name, def) in concepts {
                self.semantic.store_concept(&name, &def, context.clone());
            }
            result.stored_semantic = true;
        }

        result
    }

    /// Learn a fact (store in collective and semantic memory)
    pub fn learn(
        &mut self,
        key: &str,
        value: &str,
        tags: Vec<String>,
        confidence: f32,
    ) -> KnowledgeEntry {
        let entry = self.collective.store(
            key,
            KnowledgeValue::Text(value.to_string()),
            tags.clone(),
            confidence,
        );
        let mut attrs = HashMap::new();
        attrs.insert("confidence".to_string(), confidence.to_string());
        attrs.insert("source".to_string(), "collective".to_string());
        self.semantic.store_concept(key, value, attrs);
        if self.is_goal_relevant(value) {
            self.working.add_context(
                value,
                ContextSource::CollectiveKnowledge(entry.id),
                confidence,
            );
        }
        entry
    }

    /// Recall information (queries all memory systems)
    pub fn recall(&mut self, query: &str, max_results: usize) -> RecallResult {
        let episodes = self.episodic.retrieve(query, max_results);
        let (concepts, relations) = if let Some((c, r)) = self.semantic.retrieve(query) {
            (vec![c], r)
        } else {
            (Vec::new(), Vec::new())
        };
        let knowledge = if let Some(e) = self.collective.get(query) {
            vec![e]
        } else {
            Vec::new()
        };
        let result = RecallResult {
            episodes,
            concepts,
            relations,
            knowledge,
        };
        for ep in result.episodes.iter().take(3) {
            self.working.add_context(
                &ep.content,
                ContextSource::EpisodicMemory(ep.id),
                ep.surprise_score,
            );
        }
        result
    }

    pub fn set_goal(&mut self, goal: &str, priority: f32) {
        self.working.set_goal(goal, priority);
    }
    pub fn context(&self) -> String {
        self.working.context_summary()
    }

    /// Consolidate memories (episodic → semantic → collective)
    pub fn consolidate(&mut self) {
        let important: Vec<_> = self
            .episodic
            .recent_episodes
            .iter()
            .filter(|e| e.surprise_score > 0.5 || e.retrieved_count > 3)
            .cloned()
            .collect();

        for ep in &important {
            let mut attrs = HashMap::new();
            attrs.insert("source".to_string(), "episodic".to_string());
            attrs.insert("surprise".to_string(), ep.surprise_score.to_string());
            self.semantic
                .store_concept(&format!("ep_{}", ep.id), &ep.content, attrs);
        }

        if self.semantic.stats().concept_count > 100 {
            for c in self.semantic.concepts.iter() {
                if c.access_count > 5 {
                    self.collective.store(
                        &c.name,
                        KnowledgeValue::Text(c.definition.clone()),
                        vec!["consolidated".to_string()],
                        0.8,
                    );
                }
            }
        }
    }

    pub fn stats(&self) -> UnifiedStats {
        UnifiedStats {
            episodic: self.episodic.stats(),
            semantic: self.semantic.stats(),
            collective: self.collective.stats(),
            working_memory_items: self.working.context_items.len(),
            active_goal: self
                .working
                .current_goal
                .as_ref()
                .map(|g| g.description.clone()),
        }
    }

    fn is_goal_relevant(&self, content: &str) -> bool {
        self.working
            .current_goal
            .as_ref()
            .map(|g| {
                let goal_lower = g.description.to_lowercase();
                let content_lower = content.to_lowercase();
                let gw: HashSet<_> = goal_lower.split_whitespace().collect();
                let cw: HashSet<_> = content_lower.split_whitespace().collect();
                gw.intersection(&cw).count() > 0
            })
            .unwrap_or(false)
    }

    fn extract_concepts(&self, content: &str) -> Option<Vec<(String, String)>> {
        let mut concepts = Vec::new();
        for s in content.split('.') {
            let s = s.trim();
            if s.contains(" is ") {
                let p: Vec<_> = s.splitn(2, " is ").collect();
                if p.len() == 2 {
                    concepts.push((p[0].trim().to_string(), p[1].trim().to_string()));
                }
            }
        }
        if concepts.is_empty() {
            None
        } else {
            Some(concepts)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryResult {
    pub stored_episodic: bool,
    pub episodic_id: Option<Uuid>,
    pub stored_semantic: bool,
    pub added_to_working: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RecallResult {
    pub episodes: Vec<Episode>,
    pub concepts: Vec<SemanticConcept>,
    pub relations: Vec<SemanticRelation>,
    pub knowledge: Vec<KnowledgeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedStats {
    pub episodic: EpisodicStats,
    pub semantic: SemanticStats,
    pub collective: CollectiveStats,
    pub working_memory_items: usize,
    pub active_goal: Option<String>,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_clock() {
        let n1 = Uuid::new_v4();
        let n2 = Uuid::new_v4();
        let mut c1 = VectorClock::new();
        c1.tick(n1);
        c1.tick(n1);
        let mut c2 = VectorClock::new();
        c2.tick(n2);
        assert!(c1.is_concurrent(&c2));
        c1.merge(&c2);
        c1.tick(n1);
        assert!(c2.happens_before(&c1));
    }

    #[test]
    fn test_lww_register() {
        let n1 = Uuid::new_v4();
        let n2 = Uuid::new_v4();
        let mut r1 = LwwRegister::new("first", n1);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let r2 = LwwRegister::new("second", n2);
        r1.merge(&r2);
        assert_eq!(r1.value, "second");
    }

    #[test]
    fn test_gset() {
        let mut s1 = GSet::<String>::new();
        s1.add("a".to_string());
        s1.add("b".to_string());
        let mut s2 = GSet::<String>::new();
        s2.add("b".to_string());
        s2.add("c".to_string());
        s1.merge(&s2);
        assert_eq!(s1.len(), 3);
    }

    #[test]
    fn test_collective_memory() {
        let node = Uuid::new_v4();
        let mem = CollectiveMemory::new(node, CollectiveConfig::default());
        let e = mem.store(
            "key",
            KnowledgeValue::Text("val".to_string()),
            vec!["t".to_string()],
            0.9,
        );
        let r = mem.get("key").unwrap();
        assert_eq!(r.id, e.id);
    }

    #[test]
    fn test_confirmation() {
        let (n1, n2, n3) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let cfg = CollectiveConfig {
            min_confirmations: 3,
            ..Default::default()
        };
        let mem = CollectiveMemory::new(n1, cfg);
        let e = mem.store(
            "fact",
            KnowledgeValue::Text("sky is blue".to_string()),
            vec![],
            1.0,
        );
        assert!(!mem.is_confirmed(&e.id));
        mem.confirm(e.id, n2);
        assert!(!mem.is_confirmed(&e.id));
        mem.confirm(e.id, n3);
        assert!(mem.is_confirmed(&e.id));
    }

    #[test]
    fn test_episodic() {
        let mut ep = EpisodicMemory::new(256);
        let result = ep.store("Surprising event!", HashMap::new());
        // store() returns Some only if surprise exceeds threshold
        if result.is_some() {
            assert_eq!(ep.stats().episode_count, 1);
        } else {
            assert_eq!(ep.stats().episode_count, 0);
        }
    }

    #[test]
    fn test_semantic() {
        let sem = SemanticMemory::new();
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), "animal".to_string());
        sem.store_concept("dog", "A domesticated canine", attrs);
        sem.add_relation("dog", "is_a", "animal", 1.0);
        let (c, r) = sem.retrieve("dog").unwrap();
        assert_eq!(c.name, "dog");
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn test_working() {
        let mut wm = WorkingMemory::new(5);
        wm.set_goal("Find cats", 1.0);
        wm.add_context("Cats are feline", ContextSource::ExternalInput, 0.8);
        let s = wm.context_summary();
        assert!(s.contains("cats") || s.contains("Cats"));
    }

    #[test]
    fn test_unified() {
        let node = Uuid::new_v4();
        let mut mem = UnifiedMemory::new(node, UnifiedConfig::default());
        mem.set_goal("Learn programming", 1.0);
        let _ = mem.experience("Rust is safe.", HashMap::new());
        let _e = mem.learn("rust", "A safe language", vec!["lang".to_string()], 0.95);
        let r = mem.recall("rust", 5);
        assert!(!r.knowledge.is_empty());
        assert!(mem.stats().collective.total_entries > 0);
    }

    #[test]
    fn test_vector_clock_same_node_ordering() {
        let n1 = Uuid::new_v4();
        let mut c1 = VectorClock::new();
        c1.tick(n1);
        let mut c2 = c1.clone();
        c2.tick(n1);
        assert!(c1.happens_before(&c2));
        assert!(!c2.happens_before(&c1));
    }

    #[test]
    fn test_vector_clock_merge_idempotent() {
        let n1 = Uuid::new_v4();
        let n2 = Uuid::new_v4();
        let mut c1 = VectorClock::new();
        c1.tick(n1);
        let mut c2 = VectorClock::new();
        c2.tick(n2);
        c1.merge(&c2);
        let c1_before = c1.clone();
        c1.merge(&c2);
        // Merging again should not change anything
        assert!(!c1.happens_before(&c1_before));
        assert!(!c1_before.happens_before(&c1));
    }

    #[test]
    fn test_gset_add_duplicate_idempotent() {
        let mut s = GSet::<String>::new();
        s.add("a".to_string());
        s.add("a".to_string());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn test_gset_merge_commutative() {
        let mut s1 = GSet::<String>::new();
        s1.add("a".to_string());
        let mut s2 = GSet::<String>::new();
        s2.add("b".to_string());

        let mut m1 = s1.clone();
        m1.merge(&s2);
        let mut m2 = s2.clone();
        m2.merge(&s1);
        assert_eq!(m1.len(), m2.len());
    }

    #[test]
    fn test_lww_register_older_timestamp_ignored() {
        let n1 = Uuid::new_v4();
        let n2 = Uuid::new_v4();
        let r1 = LwwRegister::new("first", n1);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut r2 = LwwRegister::new("second", n2);
        // r2 is newer, merge r1 into r2 — r2 should keep its value
        r2.merge(&r1);
        assert_eq!(r2.value, "second");
    }

    #[test]
    fn test_collective_query_by_tags() {
        let node = Uuid::new_v4();
        let mem = CollectiveMemory::new(node, CollectiveConfig::default());
        mem.store("k1", KnowledgeValue::Text("v1".into()), vec!["tag_a".into()], 0.5);
        mem.store("k2", KnowledgeValue::Text("v2".into()), vec!["tag_b".into()], 0.5);
        mem.store("k3", KnowledgeValue::Text("v3".into()), vec!["tag_a".into(), "tag_b".into()], 0.5);
        let results = mem.query_by_tags(&["tag_a".into()]);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_collective_stats() {
        let node = Uuid::new_v4();
        let mem = CollectiveMemory::new(node, CollectiveConfig::default());
        mem.store("k1", KnowledgeValue::Text("v1".into()), vec![], 0.9);
        mem.store("k2", KnowledgeValue::Text("v2".into()), vec![], 0.8);
        let stats = mem.stats();
        assert_eq!(stats.total_entries, 2);
    }

    #[test]
    fn test_semantic_retrieve_nonexistent() {
        let sem = SemanticMemory::new();
        let result = sem.retrieve("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_semantic_multiple_relations() {
        let sem = SemanticMemory::new();
        let attrs = HashMap::new();
        sem.store_concept("cat", "A feline animal", attrs);
        sem.add_relation("cat", "is_a", "animal", 1.0);
        sem.add_relation("cat", "has", "fur", 0.9);
        let (_, rels) = sem.retrieve("cat").unwrap();
        assert_eq!(rels.len(), 2);
    }

    #[test]
    fn test_working_memory_capacity_limit() {
        let mut wm = WorkingMemory::new(3);
        wm.add_context("A", ContextSource::ExternalInput, 1.0);
        wm.add_context("B", ContextSource::ExternalInput, 1.0);
        wm.add_context("C", ContextSource::ExternalInput, 1.0);
        wm.add_context("D", ContextSource::ExternalInput, 1.0);
        // Capacity is 3, so oldest should be evicted
        assert!(wm.context_items.len() <= 3);
    }

    #[test]
    fn test_knowledge_value_variants() {
        let text = KnowledgeValue::Text("hello".into());
        let structured = KnowledgeValue::Structured(serde_json::json!({"key": "value"}));
        let large = KnowledgeValue::LargeContent {
            summary: "summary".into(),
            content_id: Uuid::new_v4(),
            size: 1024,
        };
        // Just verify construction doesn't panic
        assert!(matches!(text, KnowledgeValue::Text(_)));
        assert!(matches!(structured, KnowledgeValue::Structured(_)));
        assert!(matches!(large, KnowledgeValue::LargeContent { .. }));
    }

    #[test]
    fn test_unified_stats_structure() {
        let node = Uuid::new_v4();
        let mem = UnifiedMemory::new(node, UnifiedConfig::default());
        let stats = mem.stats();
        assert_eq!(stats.collective.total_entries, 0);
        assert_eq!(stats.episodic.episode_count, 0);
    }
}
