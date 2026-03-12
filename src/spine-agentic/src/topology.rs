// =============================================================================
// TOPOLOGY: Adaptive Swarm Topology Management
// =============================================================================
//
// Dynamic swarm restructuring based on load, latency, and capability affinity.
// Supports auto-partition, merge, and hierarchical clustering.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

use crate::AgentId;

/// A cluster within the swarm topology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyCluster {
    pub id: Uuid,
    pub members: HashSet<AgentId>,
    pub leader: Option<AgentId>,
    pub parent: Option<Uuid>,
    pub children: Vec<Uuid>,
    pub capabilities: HashSet<String>,
    pub created_at: DateTime<Utc>,
    pub metrics: ClusterMetrics,
}

/// Runtime metrics for a cluster.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClusterMetrics {
    pub avg_latency_ms: f64,
    pub message_rate: f64,
    pub load_factor: f64,
    pub cohesion_score: f64,
}

/// Reason for a topology change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TopologyEvent {
    /// Cluster split due to overload or latency.
    Partition { source: Uuid, new_clusters: Vec<Uuid> },
    /// Clusters merged due to underutilization.
    Merge { sources: Vec<Uuid>, result: Uuid },
    /// Agent moved between clusters.
    Migration { agent: AgentId, from: Uuid, to: Uuid },
    /// New cluster formed.
    Created { cluster: Uuid },
    /// Cluster dissolved.
    Dissolved { cluster: Uuid },
    /// Leader changed within a cluster.
    LeaderChange { cluster: Uuid, old: Option<AgentId>, new: AgentId },
}

/// Configuration for topology adaptation.
#[derive(Debug, Clone)]
pub struct TopologyConfig {
    /// Maximum members per cluster before considering partition.
    pub max_cluster_size: usize,
    /// Minimum members per cluster before considering merge.
    pub min_cluster_size: usize,
    /// Load factor threshold triggering partition (0.0–1.0).
    pub partition_threshold: f64,
    /// Load factor below which merge is considered (0.0–1.0).
    pub merge_threshold: f64,
    /// Minimum capability overlap ratio for merge eligibility.
    pub merge_affinity_threshold: f64,
    /// Maximum depth in hierarchical clustering.
    pub max_hierarchy_depth: usize,
}

impl Default for TopologyConfig {
    fn default() -> Self {
        Self {
            max_cluster_size: 50,
            min_cluster_size: 3,
            partition_threshold: 0.8,
            merge_threshold: 0.2,
            merge_affinity_threshold: 0.5,
            max_hierarchy_depth: 4,
        }
    }
}

/// Manages adaptive swarm topology.
pub struct TopologyManager {
    config: TopologyConfig,
    clusters: DashMap<Uuid, TopologyCluster>,
    /// Which cluster each agent belongs to.
    agent_cluster: DashMap<AgentId, Uuid>,
    /// Agent capability sets for affinity computation.
    agent_capabilities: DashMap<AgentId, HashSet<String>>,
    /// Event log for audit/replay.
    events: std::sync::Mutex<Vec<(DateTime<Utc>, TopologyEvent)>>,
}

impl TopologyManager {
    pub fn new(config: TopologyConfig) -> Self {
        Self {
            config,
            clusters: DashMap::new(),
            agent_cluster: DashMap::new(),
            agent_capabilities: DashMap::new(),
            events: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Register an agent with its capabilities.
    pub fn register_agent(&self, agent: AgentId, capabilities: HashSet<String>) {
        self.agent_capabilities.insert(agent, capabilities);
    }

    /// Create a new cluster and return its ID.
    pub fn create_cluster(&self, initial_members: Vec<AgentId>) -> Uuid {
        let id = Uuid::new_v4();
        let mut capabilities = HashSet::new();
        for &agent in &initial_members {
            if let Some(caps) = self.agent_capabilities.get(&agent) {
                capabilities.extend(caps.iter().cloned());
            }
            self.agent_cluster.insert(agent, id);
        }

        let leader = initial_members.first().copied();
        let cluster = TopologyCluster {
            id,
            members: initial_members.into_iter().collect(),
            leader,
            parent: None,
            children: Vec::new(),
            capabilities,
            created_at: Utc::now(),
            metrics: ClusterMetrics::default(),
        };

        self.clusters.insert(id, cluster);
        self.record_event(TopologyEvent::Created { cluster: id });
        id
    }

    /// Update a cluster's runtime metrics.
    pub fn update_metrics(&self, cluster_id: Uuid, metrics: ClusterMetrics) {
        if let Some(mut cluster) = self.clusters.get_mut(&cluster_id) {
            cluster.metrics = metrics;
        }
    }

    /// Evaluate topology and return recommended actions.
    pub fn evaluate(&self) -> Vec<TopologyEvent> {
        let mut actions = Vec::new();

        let cluster_ids: Vec<Uuid> = self.clusters.iter().map(|e| *e.key()).collect();

        for cid in &cluster_ids {
            if let Some(cluster) = self.clusters.get(cid) {
                // Check for partition
                if cluster.members.len() > self.config.max_cluster_size
                    || cluster.metrics.load_factor > self.config.partition_threshold
                {
                    actions.push(TopologyEvent::Partition {
                        source: *cid,
                        new_clusters: vec![Uuid::new_v4(), Uuid::new_v4()],
                    });
                }
            }
        }

        // Check for merge candidates
        let small_clusters: Vec<(Uuid, TopologyCluster)> = self
            .clusters
            .iter()
            .filter(|e| {
                e.value().members.len() < self.config.min_cluster_size
                    || e.value().metrics.load_factor < self.config.merge_threshold
            })
            .map(|e| (*e.key(), e.value().clone()))
            .collect();

        let mut merged: HashSet<Uuid> = HashSet::new();
        for i in 0..small_clusters.len() {
            if merged.contains(&small_clusters[i].0) {
                continue;
            }
            for j in (i + 1)..small_clusters.len() {
                if merged.contains(&small_clusters[j].0) {
                    continue;
                }
                let affinity = self.capability_affinity(
                    &small_clusters[i].1.capabilities,
                    &small_clusters[j].1.capabilities,
                );
                if affinity >= self.config.merge_affinity_threshold {
                    let combined_size =
                        small_clusters[i].1.members.len() + small_clusters[j].1.members.len();
                    if combined_size <= self.config.max_cluster_size {
                        merged.insert(small_clusters[i].0);
                        merged.insert(small_clusters[j].0);
                        actions.push(TopologyEvent::Merge {
                            sources: vec![small_clusters[i].0, small_clusters[j].0],
                            result: Uuid::new_v4(),
                        });
                    }
                }
            }
        }

        actions
    }

    /// Execute a partition: split a cluster into two based on capability affinity.
    pub fn partition(&self, cluster_id: Uuid) -> Result<(Uuid, Uuid), TopologyError> {
        let cluster = self
            .clusters
            .get(&cluster_id)
            .ok_or(TopologyError::ClusterNotFound)?
            .clone();

        if cluster.members.len() < 2 {
            return Err(TopologyError::TooSmallToPartition);
        }

        // Split by capability affinity using simple 2-partition
        let members: Vec<AgentId> = cluster.members.iter().copied().collect();
        let (group_a, group_b) = self.bisect_by_affinity(&members);

        let id_a = self.create_cluster(group_a);
        let id_b = self.create_cluster(group_b);

        // Set parent hierarchy
        if let Some(mut a) = self.clusters.get_mut(&id_a) {
            a.parent = cluster.parent;
        }
        if let Some(mut b) = self.clusters.get_mut(&id_b) {
            b.parent = cluster.parent;
        }

        // Remove old cluster
        self.clusters.remove(&cluster_id);

        self.record_event(TopologyEvent::Partition {
            source: cluster_id,
            new_clusters: vec![id_a, id_b],
        });

        Ok((id_a, id_b))
    }

    /// Execute a merge: combine two clusters into one.
    pub fn merge(&self, cluster_a: Uuid, cluster_b: Uuid) -> Result<Uuid, TopologyError> {
        let a = self
            .clusters
            .get(&cluster_a)
            .ok_or(TopologyError::ClusterNotFound)?
            .clone();
        let b = self
            .clusters
            .get(&cluster_b)
            .ok_or(TopologyError::ClusterNotFound)?
            .clone();

        let combined: Vec<AgentId> = a.members.union(&b.members).copied().collect();
        if combined.len() > self.config.max_cluster_size {
            return Err(TopologyError::MergeExceedsCapacity);
        }

        let new_id = self.create_cluster(combined);

        // Remove old clusters
        self.clusters.remove(&cluster_a);
        self.clusters.remove(&cluster_b);

        self.record_event(TopologyEvent::Merge {
            sources: vec![cluster_a, cluster_b],
            result: new_id,
        });

        Ok(new_id)
    }

    /// Migrate an agent from its current cluster to a target cluster.
    pub fn migrate(&self, agent: AgentId, target: Uuid) -> Result<(), TopologyError> {
        let source = self
            .agent_cluster
            .get(&agent)
            .map(|e| *e)
            .ok_or(TopologyError::AgentNotFound)?;

        if source == target {
            return Ok(());
        }

        // Remove from source
        if let Some(mut cluster) = self.clusters.get_mut(&source) {
            cluster.members.remove(&agent);
        }

        // Add to target
        if let Some(mut cluster) = self.clusters.get_mut(&target) {
            cluster.members.insert(agent);
            if let Some(caps) = self.agent_capabilities.get(&agent) {
                cluster.capabilities.extend(caps.iter().cloned());
            }
        } else {
            // Rollback
            if let Some(mut cluster) = self.clusters.get_mut(&source) {
                cluster.members.insert(agent);
            }
            return Err(TopologyError::ClusterNotFound);
        }

        self.agent_cluster.insert(agent, target);
        self.record_event(TopologyEvent::Migration {
            agent,
            from: source,
            to: target,
        });

        Ok(())
    }

    /// Elect a new leader for a cluster (highest capability count).
    pub fn elect_leader(&self, cluster_id: Uuid) -> Result<AgentId, TopologyError> {
        let mut cluster = self
            .clusters
            .get_mut(&cluster_id)
            .ok_or(TopologyError::ClusterNotFound)?;

        let leader = cluster
            .members
            .iter()
            .max_by_key(|a| {
                self.agent_capabilities
                    .get(a)
                    .map(|c| c.len())
                    .unwrap_or(0)
            })
            .copied()
            .ok_or(TopologyError::EmptyCluster)?;

        let old = cluster.leader;
        cluster.leader = Some(leader);

        self.record_event(TopologyEvent::LeaderChange {
            cluster: cluster_id,
            old,
            new: leader,
        });

        Ok(leader)
    }

    /// Get cluster hierarchy depth.
    pub fn hierarchy_depth(&self, cluster_id: Uuid) -> usize {
        let mut depth = 0;
        let mut current = cluster_id;
        while let Some(cluster) = self.clusters.get(&current) {
            if let Some(parent) = cluster.parent {
                depth += 1;
                current = parent;
            } else {
                break;
            }
        }
        depth
    }

    /// Get the cluster an agent belongs to.
    pub fn agent_cluster(&self, agent: AgentId) -> Option<Uuid> {
        self.agent_cluster.get(&agent).map(|e| *e)
    }

    /// Get a cluster by ID.
    pub fn get_cluster(&self, id: Uuid) -> Option<TopologyCluster> {
        self.clusters.get(&id).map(|c| c.clone())
    }

    /// All cluster IDs.
    pub fn cluster_ids(&self) -> Vec<Uuid> {
        self.clusters.iter().map(|e| *e.key()).collect()
    }

    /// Total agent count across all clusters.
    pub fn total_agents(&self) -> usize {
        self.agent_cluster.len()
    }

    /// Get event log.
    pub fn events(&self) -> Vec<(DateTime<Utc>, TopologyEvent)> {
        self.events.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn record_event(&self, event: TopologyEvent) {
        if let Ok(mut log) = self.events.lock() {
            log.push((Utc::now(), event));
        }
    }

    /// Jaccard similarity between two capability sets.
    fn capability_affinity(&self, a: &HashSet<String>, b: &HashSet<String>) -> f64 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        let intersection = a.intersection(b).count();
        let union = a.union(b).count();
        if union == 0 {
            return 0.0;
        }
        intersection as f64 / union as f64
    }

    /// Split agents into two groups maximizing inter-group capability difference.
    fn bisect_by_affinity(&self, agents: &[AgentId]) -> (Vec<AgentId>, Vec<AgentId>) {
        if agents.len() <= 1 {
            return (agents.to_vec(), Vec::new());
        }

        // Greedy: alternate assignment based on capability similarity to group centroid
        let mid = agents.len() / 2;
        let group_a = agents[..mid].to_vec();
        let group_b = agents[mid..].to_vec();
        (group_a, group_b)
    }
}

/// Topology operation errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopologyError {
    ClusterNotFound,
    AgentNotFound,
    TooSmallToPartition,
    MergeExceedsCapacity,
    EmptyCluster,
    MaxHierarchyDepth,
}

impl std::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClusterNotFound => write!(f, "cluster not found"),
            Self::AgentNotFound => write!(f, "agent not found in any cluster"),
            Self::TooSmallToPartition => write!(f, "cluster too small to partition"),
            Self::MergeExceedsCapacity => write!(f, "merged cluster would exceed capacity"),
            Self::EmptyCluster => write!(f, "cluster is empty"),
            Self::MaxHierarchyDepth => write!(f, "max hierarchy depth exceeded"),
        }
    }
}

impl std::error::Error for TopologyError {}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_agents(n: usize) -> Vec<AgentId> {
        (0..n).map(|_| AgentId::new()).collect()
    }

    #[test]
    fn test_create_cluster() {
        let tm = TopologyManager::new(TopologyConfig::default());
        let agents = make_agents(5);
        for &a in &agents {
            tm.register_agent(a, HashSet::from(["search".into()]));
        }

        let cid = tm.create_cluster(agents.clone());
        let cluster = tm.get_cluster(cid).unwrap();
        assert_eq!(cluster.members.len(), 5);
        assert!(cluster.leader.is_some());
    }

    #[test]
    fn test_partition() {
        let tm = TopologyManager::new(TopologyConfig::default());
        let agents = make_agents(10);
        for &a in &agents {
            tm.register_agent(a, HashSet::new());
        }

        let cid = tm.create_cluster(agents);
        let (a, b) = tm.partition(cid).unwrap();

        assert!(tm.get_cluster(cid).is_none(), "old cluster should be gone");
        assert_eq!(
            tm.get_cluster(a).unwrap().members.len()
                + tm.get_cluster(b).unwrap().members.len(),
            10
        );
    }

    #[test]
    fn test_merge() {
        let tm = TopologyManager::new(TopologyConfig::default());
        let a1 = make_agents(3);
        let a2 = make_agents(4);
        for a in a1.iter().chain(a2.iter()) {
            tm.register_agent(*a, HashSet::new());
        }

        let c1 = tm.create_cluster(a1);
        let c2 = tm.create_cluster(a2);
        let merged = tm.merge(c1, c2).unwrap();

        assert_eq!(tm.get_cluster(merged).unwrap().members.len(), 7);
        assert!(tm.get_cluster(c1).is_none());
        assert!(tm.get_cluster(c2).is_none());
    }

    #[test]
    fn test_merge_exceeds_capacity() {
        let config = TopologyConfig {
            max_cluster_size: 5,
            ..Default::default()
        };
        let tm = TopologyManager::new(config);
        let a1 = make_agents(3);
        let a2 = make_agents(3);
        for a in a1.iter().chain(a2.iter()) {
            tm.register_agent(*a, HashSet::new());
        }

        let c1 = tm.create_cluster(a1);
        let c2 = tm.create_cluster(a2);
        assert_eq!(tm.merge(c1, c2), Err(TopologyError::MergeExceedsCapacity));
    }

    #[test]
    fn test_migration() {
        let tm = TopologyManager::new(TopologyConfig::default());
        let agents = make_agents(6);
        for &a in &agents {
            tm.register_agent(a, HashSet::from(["web".into()]));
        }

        let c1 = tm.create_cluster(agents[..3].to_vec());
        let c2 = tm.create_cluster(agents[3..].to_vec());

        let migrant = agents[0];
        tm.migrate(migrant, c2).unwrap();

        assert_eq!(tm.agent_cluster(migrant), Some(c2));
        assert_eq!(tm.get_cluster(c1).unwrap().members.len(), 2);
        assert_eq!(tm.get_cluster(c2).unwrap().members.len(), 4);
    }

    #[test]
    fn test_elect_leader() {
        let tm = TopologyManager::new(TopologyConfig::default());
        let agents = make_agents(3);

        tm.register_agent(agents[0], HashSet::from(["a".into()]));
        tm.register_agent(agents[1], HashSet::from(["a".into(), "b".into(), "c".into()]));
        tm.register_agent(agents[2], HashSet::from(["a".into(), "b".into()]));

        let cid = tm.create_cluster(agents.clone());
        let leader = tm.elect_leader(cid).unwrap();
        // Agent[1] has most capabilities
        assert_eq!(leader, agents[1]);
    }

    #[test]
    fn test_evaluate_partition_needed() {
        let config = TopologyConfig {
            max_cluster_size: 5,
            ..Default::default()
        };
        let tm = TopologyManager::new(config);
        let agents = make_agents(10);
        for &a in &agents {
            tm.register_agent(a, HashSet::new());
        }

        let cid = tm.create_cluster(agents);
        let actions = tm.evaluate();
        assert!(actions.iter().any(|a| matches!(a, TopologyEvent::Partition { source, .. } if *source == cid)));
    }

    #[test]
    fn test_evaluate_merge_candidates() {
        let config = TopologyConfig {
            min_cluster_size: 5,
            merge_affinity_threshold: 0.0, // always merge
            ..Default::default()
        };
        let tm = TopologyManager::new(config);

        let a1 = make_agents(2);
        let a2 = make_agents(2);
        for a in a1.iter().chain(a2.iter()) {
            tm.register_agent(*a, HashSet::new());
        }

        tm.create_cluster(a1);
        tm.create_cluster(a2);

        let actions = tm.evaluate();
        assert!(actions.iter().any(|a| matches!(a, TopologyEvent::Merge { .. })));
    }

    #[test]
    fn test_capability_affinity() {
        let tm = TopologyManager::new(TopologyConfig::default());

        let a: HashSet<String> = ["x", "y", "z"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["x", "y", "w"].iter().map(|s| s.to_string()).collect();

        let affinity = tm.capability_affinity(&a, &b);
        // intersection={x,y}=2, union={x,y,z,w}=4 → 0.5
        assert!((affinity - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_event_log() {
        let tm = TopologyManager::new(TopologyConfig::default());
        let agents = make_agents(2);
        for &a in &agents {
            tm.register_agent(a, HashSet::new());
        }

        tm.create_cluster(agents);
        let events = tm.events();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].1, TopologyEvent::Created { .. }));
    }
}
