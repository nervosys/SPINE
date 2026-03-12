// =============================================================================
// VISUALIZER: Swarm Visualization Data Structures
// =============================================================================
//
// Provides serializable snapshots of swarm topology, message flows, and
// resource utilization for external renderers (Grafana, web UIs, etc.).
// This module generates data — rendering is done by consumers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AgentId;

/// A snapshot of the entire swarm at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmSnapshot {
    pub snapshot_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub nodes: Vec<NodeSnapshot>,
    pub edges: Vec<EdgeSnapshot>,
    pub clusters: Vec<ClusterSnapshot>,
    pub message_flows: Vec<MessageFlow>,
    pub resource_heatmap: Vec<ResourceCell>,
}

impl SwarmSnapshot {
    pub fn new() -> Self {
        Self {
            snapshot_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            nodes: Vec::new(),
            edges: Vec::new(),
            clusters: Vec::new(),
            message_flows: Vec::new(),
            resource_heatmap: Vec::new(),
        }
    }

    /// Add a node to the snapshot.
    pub fn add_node(&mut self, node: NodeSnapshot) {
        self.nodes.push(node);
    }

    /// Add an edge between two nodes.
    pub fn add_edge(&mut self, edge: EdgeSnapshot) {
        self.edges.push(edge);
    }

    /// Add a cluster grouping.
    pub fn add_cluster(&mut self, cluster: ClusterSnapshot) {
        self.clusters.push(cluster);
    }

    /// Record a message flow.
    pub fn add_message_flow(&mut self, flow: MessageFlow) {
        self.message_flows.push(flow);
    }

    /// Add a resource heatmap cell.
    pub fn add_resource_cell(&mut self, cell: ResourceCell) {
        self.resource_heatmap.push(cell);
    }

    /// Get total message volume across all flows.
    pub fn total_message_volume(&self) -> u64 {
        self.message_flows.iter().map(|f| f.message_count).sum()
    }

    /// Get average load factor across all nodes.
    pub fn average_load(&self) -> f64 {
        if self.nodes.is_empty() {
            return 0.0;
        }
        let total: f64 = self.nodes.iter().map(|n| n.load_factor).sum();
        total / self.nodes.len() as f64
    }

    /// Find nodes above a given load threshold.
    pub fn overloaded_nodes(&self, threshold: f64) -> Vec<&NodeSnapshot> {
        self.nodes
            .iter()
            .filter(|n| n.load_factor > threshold)
            .collect()
    }

    /// Get isolated nodes (no edges).
    pub fn isolated_nodes(&self) -> Vec<&NodeSnapshot> {
        self.nodes
            .iter()
            .filter(|n| {
                !self
                    .edges
                    .iter()
                    .any(|e| e.source == n.agent_id || e.target == n.agent_id)
            })
            .collect()
    }
}

impl Default for SwarmSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of a single agent node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSnapshot {
    pub agent_id: AgentId,
    pub label: String,
    pub state: NodeState,
    pub load_factor: f64,
    pub memory_bytes: u64,
    pub cpu_percent: f64,
    pub position: Option<(f64, f64)>,
    pub capabilities: Vec<String>,
    pub cluster_id: Option<String>,
}

impl NodeSnapshot {
    pub fn new(agent_id: AgentId, label: impl Into<String>) -> Self {
        Self {
            agent_id,
            label: label.into(),
            state: NodeState::Active,
            load_factor: 0.0,
            memory_bytes: 0,
            cpu_percent: 0.0,
            position: None,
            capabilities: Vec::new(),
            cluster_id: None,
        }
    }
}

/// Visual state of a node.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeState {
    Active,
    Idle,
    Overloaded,
    Degraded,
    Offline,
    Starting,
}

/// An edge between two nodes in the topology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeSnapshot {
    pub source: AgentId,
    pub target: AgentId,
    pub edge_type: EdgeType,
    pub latency_ms: f64,
    pub bandwidth_bps: u64,
    pub message_count: u64,
}

/// Type of connection edge.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeType {
    Direct,
    Mesh,
    Federation,
    Gossip,
}

/// A cluster of related nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSnapshot {
    pub cluster_id: String,
    pub label: String,
    pub members: Vec<AgentId>,
    pub leader: Option<AgentId>,
    pub avg_latency_ms: f64,
    pub total_load: f64,
}

/// A recorded message flow between agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageFlow {
    pub source: AgentId,
    pub target: AgentId,
    pub message_count: u64,
    pub bytes_transferred: u64,
    pub avg_latency_ms: f64,
    pub error_count: u64,
    pub time_window_secs: u64,
}

/// A cell in the resource heatmap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceCell {
    pub agent_id: AgentId,
    pub resource_type: ResourceType,
    pub utilization: f64,
    pub capacity: f64,
}

/// Type of resource being tracked.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResourceType {
    Cpu,
    Memory,
    Network,
    Storage,
    Tasks,
}

/// Time-series data point for trend visualization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSeriesPoint {
    pub timestamp: DateTime<Utc>,
    pub value: f64,
    pub label: String,
}

/// Collects snapshots over time for animation/playback.
pub struct SnapshotRecorder {
    snapshots: Vec<SwarmSnapshot>,
    max_snapshots: usize,
}

impl SnapshotRecorder {
    pub fn new(max_snapshots: usize) -> Self {
        Self {
            snapshots: Vec::new(),
            max_snapshots,
        }
    }

    /// Record a new snapshot.
    pub fn record(&mut self, snapshot: SwarmSnapshot) {
        if self.snapshots.len() >= self.max_snapshots {
            self.snapshots.remove(0);
        }
        self.snapshots.push(snapshot);
    }

    /// Get all recorded snapshots.
    pub fn snapshots(&self) -> &[SwarmSnapshot] {
        &self.snapshots
    }

    /// Get the latest snapshot.
    pub fn latest(&self) -> Option<&SwarmSnapshot> {
        self.snapshots.last()
    }

    /// Get snapshot count.
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    /// Check if recorder is empty.
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    /// Extract time-series for a specific metric across snapshots.
    pub fn time_series_load(&self) -> Vec<TimeSeriesPoint> {
        self.snapshots
            .iter()
            .map(|s| TimeSeriesPoint {
                timestamp: s.timestamp,
                value: s.average_load(),
                label: "avg_load".into(),
            })
            .collect()
    }

    /// Extract message volume time-series.
    pub fn time_series_messages(&self) -> Vec<TimeSeriesPoint> {
        self.snapshots
            .iter()
            .map(|s| TimeSeriesPoint {
                timestamp: s.timestamp,
                value: s.total_message_volume() as f64,
                label: "message_volume".into(),
            })
            .collect()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_basic() {
        let mut snap = SwarmSnapshot::new();

        let a1 = AgentId::new();
        let a2 = AgentId::new();

        snap.add_node(NodeSnapshot::new(a1, "agent-1"));
        snap.add_node(NodeSnapshot::new(a2, "agent-2"));

        snap.add_edge(EdgeSnapshot {
            source: a1,
            target: a2,
            edge_type: EdgeType::Direct,
            latency_ms: 5.0,
            bandwidth_bps: 1_000_000,
            message_count: 42,
        });

        assert_eq!(snap.nodes.len(), 2);
        assert_eq!(snap.edges.len(), 1);
    }

    #[test]
    fn test_average_load() {
        let mut snap = SwarmSnapshot::new();
        let mut n1 = NodeSnapshot::new(AgentId::new(), "n1");
        n1.load_factor = 0.4;
        let mut n2 = NodeSnapshot::new(AgentId::new(), "n2");
        n2.load_factor = 0.8;

        snap.add_node(n1);
        snap.add_node(n2);

        let avg = snap.average_load();
        assert!((avg - 0.6).abs() < 1e-9);
    }

    #[test]
    fn test_overloaded_nodes() {
        let mut snap = SwarmSnapshot::new();
        let mut n1 = NodeSnapshot::new(AgentId::new(), "low");
        n1.load_factor = 0.3;
        let mut n2 = NodeSnapshot::new(AgentId::new(), "high");
        n2.load_factor = 0.95;

        snap.add_node(n1);
        snap.add_node(n2);

        let overloaded = snap.overloaded_nodes(0.8);
        assert_eq!(overloaded.len(), 1);
        assert_eq!(overloaded[0].label, "high");
    }

    #[test]
    fn test_isolated_nodes() {
        let a1 = AgentId::new();
        let a2 = AgentId::new();
        let a3 = AgentId::new();

        let mut snap = SwarmSnapshot::new();
        snap.add_node(NodeSnapshot::new(a1, "connected1"));
        snap.add_node(NodeSnapshot::new(a2, "connected2"));
        snap.add_node(NodeSnapshot::new(a3, "isolated"));

        snap.add_edge(EdgeSnapshot {
            source: a1,
            target: a2,
            edge_type: EdgeType::Mesh,
            latency_ms: 1.0,
            bandwidth_bps: 0,
            message_count: 0,
        });

        let isolated = snap.isolated_nodes();
        assert_eq!(isolated.len(), 1);
        assert_eq!(isolated[0].agent_id, a3);
    }

    #[test]
    fn test_total_message_volume() {
        let mut snap = SwarmSnapshot::new();
        let a1 = AgentId::new();
        let a2 = AgentId::new();
        snap.add_message_flow(MessageFlow {
            source: a1,
            target: a2,
            message_count: 100,
            bytes_transferred: 50000,
            avg_latency_ms: 2.5,
            error_count: 1,
            time_window_secs: 60,
        });
        snap.add_message_flow(MessageFlow {
            source: a2,
            target: a1,
            message_count: 50,
            bytes_transferred: 25000,
            avg_latency_ms: 3.0,
            error_count: 0,
            time_window_secs: 60,
        });

        assert_eq!(snap.total_message_volume(), 150);
    }

    #[test]
    fn test_snapshot_serialization() {
        let mut snap = SwarmSnapshot::new();
        snap.add_node(NodeSnapshot::new(AgentId::new(), "test"));

        let json = serde_json::to_string(&snap).unwrap();
        let restored: SwarmSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.nodes.len(), 1);
    }

    #[test]
    fn test_snapshot_recorder() {
        let mut recorder = SnapshotRecorder::new(3);
        assert!(recorder.is_empty());

        for _ in 0..5 {
            recorder.record(SwarmSnapshot::new());
        }

        assert_eq!(recorder.len(), 3); // capped at max
        assert!(!recorder.is_empty());
        assert!(recorder.latest().is_some());
    }

    #[test]
    fn test_time_series_load() {
        let mut recorder = SnapshotRecorder::new(10);

        for i in 0..3 {
            let mut snap = SwarmSnapshot::new();
            let mut node = NodeSnapshot::new(AgentId::new(), format!("n{i}"));
            node.load_factor = (i as f64 + 1.0) * 0.1;
            snap.add_node(node);
            recorder.record(snap);
        }

        let ts = recorder.time_series_load();
        assert_eq!(ts.len(), 3);
        assert!((ts[0].value - 0.1).abs() < 1e-9);
        assert!((ts[1].value - 0.2).abs() < 1e-9);
    }

    #[test]
    fn test_resource_heatmap() {
        let mut snap = SwarmSnapshot::new();
        snap.add_resource_cell(ResourceCell {
            agent_id: AgentId::new(),
            resource_type: ResourceType::Cpu,
            utilization: 75.0,
            capacity: 100.0,
        });
        snap.add_resource_cell(ResourceCell {
            agent_id: AgentId::new(),
            resource_type: ResourceType::Memory,
            utilization: 4096.0,
            capacity: 8192.0,
        });

        assert_eq!(snap.resource_heatmap.len(), 2);
    }

    #[test]
    fn test_cluster_snapshot() {
        let a1 = AgentId::new();
        let a2 = AgentId::new();

        let mut snap = SwarmSnapshot::new();
        snap.add_cluster(ClusterSnapshot {
            cluster_id: "c1".into(),
            label: "Workers".into(),
            members: vec![a1, a2],
            leader: Some(a1),
            avg_latency_ms: 2.3,
            total_load: 0.7,
        });

        assert_eq!(snap.clusters.len(), 1);
        assert_eq!(snap.clusters[0].members.len(), 2);
    }

    #[test]
    fn test_empty_snapshot_metrics() {
        let snap = SwarmSnapshot::new();
        assert_eq!(snap.average_load(), 0.0);
        assert_eq!(snap.total_message_volume(), 0);
        assert!(snap.overloaded_nodes(0.5).is_empty());
        assert!(snap.isolated_nodes().is_empty());
    }
}
