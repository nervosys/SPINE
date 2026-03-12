// =============================================================================
// SPINE Agent Lifecycle Management
// =============================================================================
//
// Persistent agent lifecycle: spawn, suspend, resume, migrate between nodes.
// Agents are first-class runtime entities with durable state.
//
// =============================================================================

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::{AgentId, AgentProfile};

// =============================================================================
// AGENT LIFECYCLE STATES
// =============================================================================

/// The lifecycle state of a managed agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentState {
    /// Agent is being initialized (loading state, connecting to mesh).
    Spawning,
    /// Agent is running and processing tasks.
    Running,
    /// Agent is temporarily suspended (state preserved, resources released).
    Suspended,
    /// Agent is being migrated to another node.
    Migrating,
    /// Agent has been stopped and its state persisted for future resumption.
    Stopped,
    /// Agent has been permanently terminated.
    Terminated,
    /// Agent encountered an unrecoverable error.
    Failed,
}

/// A lifecycle event recorded in the agent's history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub timestamp: DateTime<Utc>,
    pub from_state: AgentState,
    pub to_state: AgentState,
    pub reason: String,
    pub node_id: Option<Uuid>,
}

/// Persistent checkpoint of an agent's full runtime state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCheckpoint {
    pub agent_id: AgentId,
    pub version: u64,
    pub state: AgentState,
    pub profile: AgentProfile,
    pub memory: serde_json::Value,
    pub task_queue: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub node_id: Uuid,
    pub checksum: String,
}

impl AgentCheckpoint {
    /// Create a checkpoint from current agent runtime state.
    pub fn capture(
        agent_id: AgentId,
        version: u64,
        state: AgentState,
        profile: AgentProfile,
        memory: serde_json::Value,
        task_queue: Vec<String>,
        node_id: Uuid,
    ) -> Self {
        use sha2::{Digest, Sha256};
        let data = serde_json::to_string(&memory).unwrap_or_default();
        let hash = Sha256::digest(data.as_bytes());
        Self {
            agent_id,
            version,
            state,
            profile,
            memory,
            task_queue,
            created_at: Utc::now(),
            node_id,
            checksum: format!("{:x}", hash),
        }
    }
}

/// Configuration for agent lifecycle management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleConfig {
    /// Maximum number of managed agents on this node.
    pub max_agents: usize,
    /// How often to auto-checkpoint running agents (seconds).
    pub checkpoint_interval_secs: u64,
    /// Maximum checkpoints to retain per agent.
    pub max_checkpoints: usize,
    /// Auto-suspend idle agents after this duration (seconds). 0 = never.
    pub idle_suspend_secs: u64,
    /// Maximum time for a migration before it's considered failed (seconds).
    pub migration_timeout_secs: u64,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            max_agents: 256,
            checkpoint_interval_secs: 300,
            max_checkpoints: 10,
            idle_suspend_secs: 0,
            migration_timeout_secs: 60,
        }
    }
}

/// A handle to a managed agent within the lifecycle system.
#[derive(Debug, Clone)]
pub struct ManagedAgent {
    pub agent_id: AgentId,
    pub profile: AgentProfile,
    pub state: AgentState,
    pub node_id: Uuid,
    pub spawned_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub checkpoint_version: u64,
    pub history: Vec<LifecycleEvent>,
}

// =============================================================================
// MIGRATION REQUEST
// =============================================================================

/// A request to migrate an agent from one node to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationRequest {
    pub agent_id: AgentId,
    pub source_node: Uuid,
    pub target_node: Uuid,
    pub reason: String,
    pub initiated_at: DateTime<Utc>,
}

/// The result of a migration attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MigrationResult {
    Success {
        agent_id: AgentId,
        target_node: Uuid,
        duration_ms: u64,
    },
    Failed {
        agent_id: AgentId,
        error: String,
    },
}

// =============================================================================
// LIFECYCLE MANAGER
// =============================================================================

/// Manages the full lifecycle of agents on a node.
///
/// Handles spawning, suspending, resuming, migrating, and terminating agents
/// with persistent checkpointing for durability.
pub struct LifecycleManager {
    node_id: Uuid,
    config: LifecycleConfig,
    agents: DashMap<AgentId, ManagedAgent>,
    checkpoints: DashMap<AgentId, Vec<AgentCheckpoint>>,
    pending_migrations: DashMap<AgentId, MigrationRequest>,
}

impl LifecycleManager {
    pub fn new(node_id: Uuid, config: LifecycleConfig) -> Self {
        Self {
            node_id,
            config,
            agents: DashMap::new(),
            checkpoints: DashMap::new(),
            pending_migrations: DashMap::new(),
        }
    }

    /// Spawn a new agent on this node.
    pub fn spawn(&self, profile: AgentProfile) -> Result<AgentId, LifecycleError> {
        if self.agents.len() >= self.config.max_agents {
            return Err(LifecycleError::CapacityExceeded {
                max: self.config.max_agents,
            });
        }

        let agent_id = AgentId::new();
        let now = Utc::now();

        let managed = ManagedAgent {
            agent_id,
            profile: profile.clone(),
            state: AgentState::Running,
            node_id: self.node_id,
            spawned_at: now,
            last_active: now,
            checkpoint_version: 0,
            history: vec![LifecycleEvent {
                timestamp: now,
                from_state: AgentState::Spawning,
                to_state: AgentState::Running,
                reason: "Initial spawn".to_string(),
                node_id: Some(self.node_id),
            }],
        };

        self.agents.insert(agent_id, managed);
        Ok(agent_id)
    }

    /// Spawn an agent from a previously saved checkpoint (resume from persistence).
    pub fn spawn_from_checkpoint(
        &self,
        checkpoint: AgentCheckpoint,
    ) -> Result<AgentId, LifecycleError> {
        if self.agents.len() >= self.config.max_agents {
            return Err(LifecycleError::CapacityExceeded {
                max: self.config.max_agents,
            });
        }

        let agent_id = checkpoint.agent_id;
        let now = Utc::now();

        let managed = ManagedAgent {
            agent_id,
            profile: checkpoint.profile.clone(),
            state: AgentState::Running,
            node_id: self.node_id,
            spawned_at: now,
            last_active: now,
            checkpoint_version: checkpoint.version,
            history: vec![LifecycleEvent {
                timestamp: now,
                from_state: AgentState::Stopped,
                to_state: AgentState::Running,
                reason: format!("Resumed from checkpoint v{}", checkpoint.version),
                node_id: Some(self.node_id),
            }],
        };

        self.agents.insert(agent_id, managed);
        Ok(agent_id)
    }

    /// Suspend a running agent, preserving its state.
    pub fn suspend(&self, agent_id: &AgentId, reason: &str) -> Result<(), LifecycleError> {
        let mut agent = self
            .agents
            .get_mut(agent_id)
            .ok_or(LifecycleError::AgentNotFound(*agent_id))?;

        if agent.state != AgentState::Running {
            return Err(LifecycleError::InvalidTransition {
                from: agent.state,
                to: AgentState::Suspended,
            });
        }

        let event = LifecycleEvent {
            timestamp: Utc::now(),
            from_state: agent.state,
            to_state: AgentState::Suspended,
            reason: reason.to_string(),
            node_id: Some(self.node_id),
        };

        agent.state = AgentState::Suspended;
        agent.history.push(event);
        Ok(())
    }

    /// Resume a suspended agent.
    pub fn resume(&self, agent_id: &AgentId) -> Result<(), LifecycleError> {
        let mut agent = self
            .agents
            .get_mut(agent_id)
            .ok_or(LifecycleError::AgentNotFound(*agent_id))?;

        if agent.state != AgentState::Suspended {
            return Err(LifecycleError::InvalidTransition {
                from: agent.state,
                to: AgentState::Running,
            });
        }

        let event = LifecycleEvent {
            timestamp: Utc::now(),
            from_state: agent.state,
            to_state: AgentState::Running,
            reason: "Resumed".to_string(),
            node_id: Some(self.node_id),
        };

        agent.state = AgentState::Running;
        agent.last_active = Utc::now();
        agent.history.push(event);
        Ok(())
    }

    /// Terminate an agent permanently.
    pub fn terminate(
        &self,
        agent_id: &AgentId,
        reason: &str,
    ) -> Result<ManagedAgent, LifecycleError> {
        let mut agent = self
            .agents
            .get_mut(agent_id)
            .ok_or(LifecycleError::AgentNotFound(*agent_id))?;

        if agent.state == AgentState::Terminated {
            return Err(LifecycleError::InvalidTransition {
                from: agent.state,
                to: AgentState::Terminated,
            });
        }

        let event = LifecycleEvent {
            timestamp: Utc::now(),
            from_state: agent.state,
            to_state: AgentState::Terminated,
            reason: reason.to_string(),
            node_id: Some(self.node_id),
        };

        agent.state = AgentState::Terminated;
        agent.history.push(event);

        drop(agent);
        let (_, terminated) = self
            .agents
            .remove(agent_id)
            .ok_or(LifecycleError::AgentNotFound(*agent_id))?;
        Ok(terminated)
    }

    /// Create a checkpoint of the agent's current state.
    pub fn checkpoint(
        &self,
        agent_id: &AgentId,
        memory: serde_json::Value,
        task_queue: Vec<String>,
    ) -> Result<AgentCheckpoint, LifecycleError> {
        let mut agent = self
            .agents
            .get_mut(agent_id)
            .ok_or(LifecycleError::AgentNotFound(*agent_id))?;

        agent.checkpoint_version += 1;
        let cp = AgentCheckpoint::capture(
            *agent_id,
            agent.checkpoint_version,
            agent.state,
            agent.profile.clone(),
            memory,
            task_queue,
            self.node_id,
        );

        let mut cps = self.checkpoints.entry(*agent_id).or_default();
        cps.push(cp.clone());

        // Trim old checkpoints beyond retention limit
        while cps.len() > self.config.max_checkpoints {
            cps.remove(0);
        }

        Ok(cp)
    }

    /// Get the latest checkpoint for an agent.
    pub fn latest_checkpoint(&self, agent_id: &AgentId) -> Option<AgentCheckpoint> {
        self.checkpoints
            .get(agent_id)
            .and_then(|cps| cps.last().cloned())
    }

    /// Begin migration: marks agent as Migrating and produces a checkpoint for transfer.
    pub fn begin_migration(
        &self,
        agent_id: &AgentId,
        target_node: Uuid,
        reason: &str,
        memory: serde_json::Value,
        task_queue: Vec<String>,
    ) -> Result<(MigrationRequest, AgentCheckpoint), LifecycleError> {
        let mut agent = self
            .agents
            .get_mut(agent_id)
            .ok_or(LifecycleError::AgentNotFound(*agent_id))?;

        if agent.state != AgentState::Running && agent.state != AgentState::Suspended {
            return Err(LifecycleError::InvalidTransition {
                from: agent.state,
                to: AgentState::Migrating,
            });
        }

        let event = LifecycleEvent {
            timestamp: Utc::now(),
            from_state: agent.state,
            to_state: AgentState::Migrating,
            reason: reason.to_string(),
            node_id: Some(self.node_id),
        };

        agent.state = AgentState::Migrating;
        agent.checkpoint_version += 1;
        agent.history.push(event);

        let cp = AgentCheckpoint::capture(
            *agent_id,
            agent.checkpoint_version,
            AgentState::Migrating,
            agent.profile.clone(),
            memory,
            task_queue,
            self.node_id,
        );

        let request = MigrationRequest {
            agent_id: *agent_id,
            source_node: self.node_id,
            target_node,
            reason: reason.to_string(),
            initiated_at: Utc::now(),
        };

        drop(agent);
        self.pending_migrations.insert(*agent_id, request.clone());

        Ok((request, cp))
    }

    /// Complete migration on the source node (remove the agent after successful transfer).
    pub fn complete_migration_source(
        &self,
        agent_id: &AgentId,
    ) -> Result<(), LifecycleError> {
        self.pending_migrations.remove(agent_id);
        self.agents.remove(agent_id);
        self.checkpoints.remove(agent_id);
        Ok(())
    }

    /// Accept a migrated agent on the target node.
    pub fn accept_migration(
        &self,
        checkpoint: AgentCheckpoint,
        source_node: Uuid,
    ) -> Result<AgentId, LifecycleError> {
        if self.agents.len() >= self.config.max_agents {
            return Err(LifecycleError::CapacityExceeded {
                max: self.config.max_agents,
            });
        }

        let agent_id = checkpoint.agent_id;
        let now = Utc::now();

        let managed = ManagedAgent {
            agent_id,
            profile: checkpoint.profile.clone(),
            state: AgentState::Running,
            node_id: self.node_id,
            spawned_at: now,
            last_active: now,
            checkpoint_version: checkpoint.version,
            history: vec![LifecycleEvent {
                timestamp: now,
                from_state: AgentState::Migrating,
                to_state: AgentState::Running,
                reason: format!("Migrated from node {}", source_node),
                node_id: Some(self.node_id),
            }],
        };

        self.agents.insert(agent_id, managed);
        Ok(agent_id)
    }

    /// Get the current state of an agent.
    pub fn get_state(&self, agent_id: &AgentId) -> Option<AgentState> {
        self.agents.get(agent_id).map(|a| a.state)
    }

    /// Get info about a managed agent.
    pub fn get_agent(&self, agent_id: &AgentId) -> Option<ManagedAgent> {
        self.agents.get(agent_id).map(|a| a.clone())
    }

    /// List all managed agents on this node.
    pub fn list_agents(&self) -> Vec<(AgentId, AgentState)> {
        self.agents
            .iter()
            .map(|entry| (*entry.key(), entry.value().state))
            .collect()
    }

    /// Count agents by state.
    pub fn count_by_state(&self) -> HashMap<AgentState, usize> {
        let mut counts = HashMap::new();
        for entry in self.agents.iter() {
            *counts.entry(entry.value().state).or_insert(0) += 1;
        }
        counts
    }

    /// Total number of managed agents.
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    /// Mark an agent as active (update last_active timestamp).
    pub fn touch(&self, agent_id: &AgentId) {
        if let Some(mut agent) = self.agents.get_mut(agent_id) {
            agent.last_active = Utc::now();
        }
    }
}

// =============================================================================
// ERROR TYPES
// =============================================================================

/// Errors that can occur during lifecycle operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LifecycleError {
    AgentNotFound(AgentId),
    InvalidTransition { from: AgentState, to: AgentState },
    CapacityExceeded { max: usize },
    MigrationFailed(String),
    CheckpointFailed(String),
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AgentNotFound(id) => write!(f, "Agent not found: {}", id.0),
            Self::InvalidTransition { from, to } => {
                write!(f, "Invalid state transition: {:?} → {:?}", from, to)
            }
            Self::CapacityExceeded { max } => {
                write!(f, "Node capacity exceeded (max {})", max)
            }
            Self::MigrationFailed(msg) => write!(f, "Migration failed: {}", msg),
            Self::CheckpointFailed(msg) => write!(f, "Checkpoint failed: {}", msg),
        }
    }
}

impl std::error::Error for LifecycleError {}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{AgentCapability, TrustLevel};

    fn test_profile() -> AgentProfile {
        AgentProfile {
            id: crate::AgentId::new(),
            name: "test-agent".to_string(),
            version: "1.0".to_string(),
            capabilities: vec![AgentCapability::Navigation],
            trust_level: TrustLevel::Trusted,
            created_at: chrono::Utc::now(),
            last_seen: chrono::Utc::now(),
            latent_signature: vec![0.0; 64],
            miras_variant: "Titans".to_string(),
            public_key: None,
            ontology: None,
        }
    }

    #[test]
    fn test_spawn_agent() {
        let mgr = LifecycleManager::new(Uuid::new_v4(), LifecycleConfig::default());
        let id = mgr.spawn(test_profile()).unwrap();
        assert_eq!(mgr.get_state(&id), Some(AgentState::Running));
        assert_eq!(mgr.agent_count(), 1);
    }

    #[test]
    fn test_suspend_and_resume() {
        let mgr = LifecycleManager::new(Uuid::new_v4(), LifecycleConfig::default());
        let id = mgr.spawn(test_profile()).unwrap();

        mgr.suspend(&id, "idle").unwrap();
        assert_eq!(mgr.get_state(&id), Some(AgentState::Suspended));

        mgr.resume(&id).unwrap();
        assert_eq!(mgr.get_state(&id), Some(AgentState::Running));
    }

    #[test]
    fn test_suspend_already_suspended_fails() {
        let mgr = LifecycleManager::new(Uuid::new_v4(), LifecycleConfig::default());
        let id = mgr.spawn(test_profile()).unwrap();
        mgr.suspend(&id, "idle").unwrap();
        let result = mgr.suspend(&id, "again");
        assert!(result.is_err());
    }

    #[test]
    fn test_terminate() {
        let mgr = LifecycleManager::new(Uuid::new_v4(), LifecycleConfig::default());
        let id = mgr.spawn(test_profile()).unwrap();
        let terminated = mgr.terminate(&id, "shutdown").unwrap();
        assert_eq!(terminated.state, AgentState::Terminated);
        assert_eq!(mgr.agent_count(), 0);
    }

    #[test]
    fn test_checkpoint_and_restore() {
        let mgr = LifecycleManager::new(Uuid::new_v4(), LifecycleConfig::default());
        let id = mgr.spawn(test_profile()).unwrap();

        let memory = serde_json::json!({"knowledge": ["fact1", "fact2"]});
        let cp = mgr.checkpoint(&id, memory.clone(), vec!["task1".into()]).unwrap();
        assert_eq!(cp.version, 1);
        assert!(!cp.checksum.is_empty());

        let latest = mgr.latest_checkpoint(&id).unwrap();
        assert_eq!(latest.version, 1);
        assert_eq!(latest.memory, memory);
    }

    #[test]
    fn test_spawn_from_checkpoint() {
        let node_id = Uuid::new_v4();
        let mgr = LifecycleManager::new(node_id, LifecycleConfig::default());
        let id = mgr.spawn(test_profile()).unwrap();

        let cp = mgr
            .checkpoint(&id, serde_json::json!({}), vec![])
            .unwrap();
        mgr.terminate(&id, "test").unwrap();
        assert_eq!(mgr.agent_count(), 0);

        let restored_id = mgr.spawn_from_checkpoint(cp).unwrap();
        assert_eq!(restored_id, id);
        assert_eq!(mgr.get_state(&restored_id), Some(AgentState::Running));
    }

    #[test]
    fn test_migration_flow() {
        let source_node = Uuid::new_v4();
        let target_node = Uuid::new_v4();
        let source = LifecycleManager::new(source_node, LifecycleConfig::default());
        let target = LifecycleManager::new(target_node, LifecycleConfig::default());

        let id = source.spawn(test_profile()).unwrap();

        // Begin migration on source
        let (_req, cp) = source
            .begin_migration(&id, target_node, "load balance", serde_json::json!({}), vec![])
            .unwrap();
        assert_eq!(source.get_state(&id), Some(AgentState::Migrating));

        // Accept on target
        let migrated_id = target.accept_migration(cp, source_node).unwrap();
        assert_eq!(migrated_id, id);
        assert_eq!(target.get_state(&migrated_id), Some(AgentState::Running));

        // Cleanup source
        source.complete_migration_source(&id).unwrap();
        assert_eq!(source.agent_count(), 0);
    }

    #[test]
    fn test_capacity_limit() {
        let config = LifecycleConfig {
            max_agents: 2,
            ..Default::default()
        };
        let mgr = LifecycleManager::new(Uuid::new_v4(), config);

        mgr.spawn(test_profile()).unwrap();
        mgr.spawn(test_profile()).unwrap();
        let result = mgr.spawn(test_profile());
        assert!(matches!(result, Err(LifecycleError::CapacityExceeded { .. })));
    }

    #[test]
    fn test_list_and_count_by_state() {
        let mgr = LifecycleManager::new(Uuid::new_v4(), LifecycleConfig::default());
        let id1 = mgr.spawn(test_profile()).unwrap();
        let _id2 = mgr.spawn(test_profile()).unwrap();
        mgr.suspend(&id1, "test").unwrap();

        let counts = mgr.count_by_state();
        assert_eq!(counts.get(&AgentState::Running), Some(&1));
        assert_eq!(counts.get(&AgentState::Suspended), Some(&1));
        assert_eq!(mgr.list_agents().len(), 2);
    }

    #[test]
    fn test_checkpoint_retention() {
        let config = LifecycleConfig {
            max_checkpoints: 3,
            ..Default::default()
        };
        let mgr = LifecycleManager::new(Uuid::new_v4(), config);
        let id = mgr.spawn(test_profile()).unwrap();

        for i in 0..5 {
            mgr.checkpoint(&id, serde_json::json!({"i": i}), vec![]).unwrap();
        }

        let cps = mgr.checkpoints.get(&id).unwrap();
        assert_eq!(cps.len(), 3);
        assert_eq!(cps.last().unwrap().version, 5);
    }

    #[test]
    fn test_lifecycle_history() {
        let mgr = LifecycleManager::new(Uuid::new_v4(), LifecycleConfig::default());
        let id = mgr.spawn(test_profile()).unwrap();
        mgr.suspend(&id, "idle").unwrap();
        mgr.resume(&id).unwrap();

        let agent = mgr.get_agent(&id).unwrap();
        assert_eq!(agent.history.len(), 3); // spawn, suspend, resume
        assert_eq!(agent.history[0].to_state, AgentState::Running);
        assert_eq!(agent.history[1].to_state, AgentState::Suspended);
        assert_eq!(agent.history[2].to_state, AgentState::Running);
    }
}
