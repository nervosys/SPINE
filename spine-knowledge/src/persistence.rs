//! # Knowledge Persistence
//!
//! Optional persistent storage integration for SPINE knowledge base.
//! Uses `spine-storage` backends to persist episodic, semantic, and
//! collective memory across restarts.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use spine_knowledge::persistence::PersistentKnowledge;
//! use spine_storage::{StorageConfig, StorageEngine, create_backend};
//!
//! let config = StorageConfig {
//!     engine: StorageEngine::Sqlite,
//!     path: Some("knowledge.db".to_string()),
//! };
//! let backend = create_backend(&config).unwrap();
//! let pk = PersistentKnowledge::new(backend);
//! ```

use crate::{
    CollectiveMemory, EpisodicMemory, Episode, KnowledgeEntry,
    SemanticConcept, SemanticMemory, SemanticRelation,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use spine_storage::StorageBackend;
use std::sync::Arc;
use uuid::Uuid;

/// Namespace constants for storage.
const NS_EPISODES: &str = "episodes";
const NS_KNOWLEDGE: &str = "knowledge";
const NS_CONCEPTS: &str = "concepts";
const NS_RELATIONS: &str = "relations";
const NS_METADATA: &str = "metadata";

/// Persistent knowledge storage adapter.
///
/// Wraps a `StorageBackend` and provides methods to save/load
/// individual memory components.
pub struct PersistentKnowledge {
    backend: Box<dyn StorageBackend>,
}

impl PersistentKnowledge {
    /// Create a new persistent knowledge adapter.
    pub fn new(backend: Box<dyn StorageBackend>) -> Self {
        Self { backend }
    }

    // ========================================================================
    // Episodic Memory Persistence
    // ========================================================================

    /// Save an episode to persistent storage.
    pub fn save_episode(&self, episode: &Episode) -> Result<()> {
        let key = episode.id.to_string();
        let value = serde_json::to_vec(episode)?;
        self.backend.put(NS_EPISODES, key.as_bytes(), &value)
    }

    /// Load an episode by ID.
    pub fn load_episode(&self, id: Uuid) -> Result<Option<Episode>> {
        let key = id.to_string();
        match self.backend.get(NS_EPISODES, key.as_bytes())? {
            Some(data) => Ok(Some(serde_json::from_slice(&data)?)),
            None => Ok(None),
        }
    }

    /// Load all episodes.
    pub fn load_all_episodes(&self) -> Result<Vec<Episode>> {
        let entries = self.backend.scan(NS_EPISODES, None)?;
        let mut episodes = Vec::new();
        for (_, value) in entries {
            if let Ok(episode) = serde_json::from_slice::<Episode>(&value) {
                episodes.push(episode);
            }
        }
        Ok(episodes)
    }

    /// Save all episodes from an `EpisodicMemory`.
    pub fn save_episodic_memory(&self, memory: &EpisodicMemory) -> Result<usize> {
        let episodes = memory.recent_episodes(memory.stats().total_episodes);
        let pairs: Vec<(Vec<u8>, Vec<u8>)> = episodes
            .iter()
            .filter_map(|ep| {
                let key = ep.id.to_string().into_bytes();
                let value = serde_json::to_vec(ep).ok()?;
                Some((key, value))
            })
            .collect();
        let count = pairs.len();
        let refs: Vec<(&[u8], &[u8])> = pairs.iter().map(|(k, v)| (k.as_slice(), v.as_slice())).collect();
        self.backend.batch_put(NS_EPISODES, &refs)?;
        Ok(count)
    }

    // ========================================================================
    // Knowledge Entry Persistence
    // ========================================================================

    /// Save a knowledge entry.
    pub fn save_knowledge_entry(&self, entry: &KnowledgeEntry) -> Result<()> {
        let key = entry.key.clone();
        let value = serde_json::to_vec(entry)?;
        self.backend.put(NS_KNOWLEDGE, key.as_bytes(), &value)
    }

    /// Load a knowledge entry by key.
    pub fn load_knowledge_entry(&self, key: &str) -> Result<Option<KnowledgeEntry>> {
        match self.backend.get(NS_KNOWLEDGE, key.as_bytes())? {
            Some(data) => Ok(Some(serde_json::from_slice(&data)?)),
            None => Ok(None),
        }
    }

    /// Load all knowledge entries.
    pub fn load_all_knowledge_entries(&self) -> Result<Vec<KnowledgeEntry>> {
        let entries = self.backend.scan(NS_KNOWLEDGE, None)?;
        let mut knowledge = Vec::new();
        for (_, value) in entries {
            if let Ok(entry) = serde_json::from_slice::<KnowledgeEntry>(&value) {
                knowledge.push(entry);
            }
        }
        Ok(knowledge)
    }

    /// Delete a knowledge entry.
    pub fn delete_knowledge_entry(&self, key: &str) -> Result<()> {
        self.backend.delete(NS_KNOWLEDGE, key.as_bytes())
    }

    // ========================================================================
    // Semantic Memory Persistence
    // ========================================================================

    /// Save a semantic concept.
    pub fn save_concept(&self, concept: &SemanticConcept) -> Result<()> {
        let key = concept.id.to_string();
        let value = serde_json::to_vec(concept)?;
        self.backend.put(NS_CONCEPTS, key.as_bytes(), &value)
    }

    /// Load all concepts.
    pub fn load_all_concepts(&self) -> Result<Vec<SemanticConcept>> {
        let entries = self.backend.scan(NS_CONCEPTS, None)?;
        let mut concepts = Vec::new();
        for (_, value) in entries {
            if let Ok(concept) = serde_json::from_slice::<SemanticConcept>(&value) {
                concepts.push(concept);
            }
        }
        Ok(concepts)
    }

    /// Save a semantic relation.
    pub fn save_relation(&self, relation: &SemanticRelation) -> Result<()> {
        let key = format!("{}:{}", relation.from, relation.to);
        let value = serde_json::to_vec(relation)?;
        self.backend.put(NS_RELATIONS, key.as_bytes(), &value)
    }

    /// Load all relations.
    pub fn load_all_relations(&self) -> Result<Vec<SemanticRelation>> {
        let entries = self.backend.scan(NS_RELATIONS, None)?;
        let mut relations = Vec::new();
        for (_, value) in entries {
            if let Ok(relation) = serde_json::from_slice::<SemanticRelation>(&value) {
                relations.push(relation);
            }
        }
        Ok(relations)
    }

    // ========================================================================
    // Metadata
    // ========================================================================

    /// Save metadata (e.g., version, node ID).
    pub fn save_metadata(&self, key: &str, value: &str) -> Result<()> {
        self.backend
            .put(NS_METADATA, key.as_bytes(), value.as_bytes())
    }

    /// Load metadata.
    pub fn load_metadata(&self, key: &str) -> Result<Option<String>> {
        match self.backend.get(NS_METADATA, key.as_bytes())? {
            Some(data) => Ok(Some(String::from_utf8(data).unwrap_or_default())),
            None => Ok(None),
        }
    }

    /// Get the total count of persisted items across all namespaces.
    pub fn total_persisted(&self) -> Result<PersistenceStats> {
        Ok(PersistenceStats {
            episodes: self.backend.count(NS_EPISODES).unwrap_or(0),
            knowledge_entries: self.backend.count(NS_KNOWLEDGE).unwrap_or(0),
            concepts: self.backend.count(NS_CONCEPTS).unwrap_or(0),
            relations: self.backend.count(NS_RELATIONS).unwrap_or(0),
        })
    }
}

/// Persistence statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceStats {
    pub episodes: usize,
    pub knowledge_entries: usize,
    pub concepts: usize,
    pub relations: usize,
}

impl PersistenceStats {
    pub fn total(&self) -> usize {
        self.episodes + self.knowledge_entries + self.concepts + self.relations
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use spine_storage::InMemoryBackend;
    use crate::KnowledgeValue;

    #[test]
    fn test_save_load_knowledge_entry() {
        let backend = Box::new(InMemoryBackend::new());
        let pk = PersistentKnowledge::new(backend);

        let entry = KnowledgeEntry::new_text(
            "test_key".to_string(),
            "test_value".to_string(),
            Uuid::new_v4(),
        );

        pk.save_knowledge_entry(&entry).unwrap();
        let loaded = pk.load_knowledge_entry("test_key").unwrap().unwrap();
        assert_eq!(loaded.key, "test_key");
    }

    #[test]
    fn test_load_all_knowledge_entries() {
        let backend = Box::new(InMemoryBackend::new());
        let pk = PersistentKnowledge::new(backend);

        for i in 0..5 {
            let entry = KnowledgeEntry::new_text(
                format!("key_{}", i),
                format!("value_{}", i),
                Uuid::new_v4(),
            );
            pk.save_knowledge_entry(&entry).unwrap();
        }

        let all = pk.load_all_knowledge_entries().unwrap();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn test_delete_knowledge_entry() {
        let backend = Box::new(InMemoryBackend::new());
        let pk = PersistentKnowledge::new(backend);

        let entry = KnowledgeEntry::new_text(
            "delete_me".to_string(),
            "value".to_string(),
            Uuid::new_v4(),
        );

        pk.save_knowledge_entry(&entry).unwrap();
        assert!(pk.load_knowledge_entry("delete_me").unwrap().is_some());

        pk.delete_knowledge_entry("delete_me").unwrap();
        assert!(pk.load_knowledge_entry("delete_me").unwrap().is_none());
    }

    #[test]
    fn test_metadata() {
        let backend = Box::new(InMemoryBackend::new());
        let pk = PersistentKnowledge::new(backend);

        pk.save_metadata("version", "1.0").unwrap();
        assert_eq!(pk.load_metadata("version").unwrap(), Some("1.0".to_string()));
        assert!(pk.load_metadata("missing").unwrap().is_none());
    }

    #[test]
    fn test_persistence_stats() {
        let backend = Box::new(InMemoryBackend::new());
        let pk = PersistentKnowledge::new(backend);

        let entry = KnowledgeEntry::new_text(
            "k1".to_string(),
            "v1".to_string(),
            Uuid::new_v4(),
        );
        pk.save_knowledge_entry(&entry).unwrap();

        let stats = pk.total_persisted().unwrap();
        assert_eq!(stats.knowledge_entries, 1);
        assert_eq!(stats.total(), 1);
    }
}
