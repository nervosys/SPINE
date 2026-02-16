// Allow dead code for cluster coordination APIs
#![allow(dead_code)]

pub mod sybil;
pub mod marketplace;
pub mod raft;
pub use sybil::{NodeReputation, StakeWeightedConsensus};
pub use marketplace::*;
pub use raft::{RaftNode, RaftCluster, RaftConfig, RaftCommand, RaftRole, RaftStatus};

// =============================================================================
// SPINE CLUSTER - Distributed Agent Coordination
// =============================================================================
//
// This module provides distributed coordination for SPINE agents:
//
// 1. Service Discovery - Agents can find available SPINE nodes
// 2. Load Balancing - Distribute requests across cluster nodes
// 3. Session Affinity - Route requests to the node holding session state
// 4. Failover - Automatic recovery when nodes fail
// 5. Coordination - Distributed locking and leader election
//
// =============================================================================

use anyhow::{Context, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use uuid::Uuid;

// =============================================================================
// CLUSTER TYPES
// =============================================================================

/// Unique identifier for a cluster node
pub type NodeId = Uuid;

/// Unique identifier for a session
pub type SessionId = String;

/// Node health status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeStatus {
    /// Node is healthy and accepting requests
    Healthy,
    /// Node is experiencing issues but still operational
    Degraded,
    /// Node is not responding
    Unreachable,
    /// Node is gracefully shutting down
    Draining,
}

/// Information about a cluster node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: NodeId,
    pub address: SocketAddr,
    pub status: NodeStatus,
    pub last_heartbeat: u64, // Unix timestamp in millis
    pub sessions_count: usize,
    pub load: f32, // 0.0 - 1.0
    pub capabilities: NodeCapabilities,
}

/// What a node can do
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeCapabilities {
    pub supports_wasm: bool,
    pub supports_chameleon: bool,
    pub supports_speculation: bool,
    pub max_sessions: usize,
    pub region: Option<String>,
    /// List of specific skills/tools available on this node
    pub skills: Vec<String>,
}

/// Session location in the cluster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLocation {
    pub session_id: SessionId,
    pub node_id: NodeId,
    pub created_at: u64,
    pub last_accessed: u64,
}

/// Load balancing strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadBalancingStrategy {
    /// Round-robin across healthy nodes
    RoundRobin,
    /// Route to least loaded node
    LeastConnections,
    /// Route based on consistent hashing
    ConsistentHash,
    /// Prefer nodes in same region
    RegionAware,
    /// Random selection
    Random,
}

// =============================================================================
// CLUSTER MESSAGES
// =============================================================================

/// Messages exchanged between cluster nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClusterMessage {
    /// Heartbeat to indicate node is alive
    Heartbeat {
        node_id: NodeId,
        load: f32,
        sessions_count: usize,
    },
    /// Request to join the cluster
    JoinRequest { node_info: NodeInfo },
    /// Response to join request
    JoinResponse {
        accepted: bool,
        cluster_state: Option<ClusterState>,
    },
    /// Notify cluster of session creation
    SessionCreated {
        session_id: SessionId,
        node_id: NodeId,
    },
    /// Notify cluster of session destruction
    SessionDestroyed { session_id: SessionId },
    /// Request to transfer session to another node
    SessionTransferRequest {
        session_id: SessionId,
        from_node: NodeId,
        to_node: NodeId,
    },
    /// Session transfer data
    SessionTransferData {
        session_id: SessionId,
        data: Vec<u8>,
    },
    /// Leader election message
    ElectionStart { candidate_id: NodeId, term: u64 },
    /// Vote in leader election
    ElectionVote {
        voter_id: NodeId,
        candidate_id: NodeId,
        term: u64,
    },
    /// Announce new leader
    LeaderAnnouncement { leader_id: NodeId, term: u64 },
    /// Update node capabilities/skills
    UpdateCapabilities {
        node_id: NodeId,
        capabilities: NodeCapabilities,
    },
    /// Node is leaving the cluster
    LeaveNotification { node_id: NodeId },
    /// Distributed search request
    SearchRequest {
        query: String,
        request_id: String,
        origin_node: NodeId,
    },
    /// Distributed search response
    SearchResponse {
        request_id: String,
        results: serde_json::Value,
        node_id: NodeId,
    },
    /// Sync knowledge entry across cluster
    SyncKnowledge {
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
        origin_node: NodeId,
    },
    /// Swarm search request
    SwarmSearchRequest {
        query: String,
        depth: usize,
        request_id: String,
        origin_node: NodeId,
    },
    /// Task delegation request
    TaskDelegation {
        task: String,
        target_agent_id: Option<Uuid>,
        origin_node: NodeId,
    },
    /// Propose a new fact for consensus
    ProposeKnowledge {
        proposal_id: Uuid,
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
        origin_node: NodeId,
    },
    /// Vote on a knowledge proposal
    KnowledgeVote {
        proposal_id: Uuid,
        voter_id: NodeId,
        approved: bool,
        confidence: f32,
    },
    /// Commit a knowledge proposal after consensus
    CommitKnowledge {
        proposal_id: Uuid,
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    },
    /// Propose a swarm plan
    ProposeSwarmPlan {
        plan: spine_protocol::SwarmPlan,
        origin_node: NodeId,
    },
    /// Update a task in a swarm plan
    UpdatePlanTask {
        plan_id: Uuid,
        task_id: Uuid,
        status: spine_protocol::TaskStatus,
        result: Option<serde_json::Value>,
        node_id: NodeId,
    },
}

/// Events emitted by the cluster node to the application
#[derive(Debug, Clone)]
pub enum ClusterEvent {
    /// Request to transfer a session to this node
    SessionTransferRequested {
        session_id: SessionId,
        from_node: NodeId,
    },
    /// Session data received from another node
    SessionDataReceived {
        session_id: SessionId,
        data: Vec<u8>,
    },
    /// Leader changed
    LeaderChanged { new_leader: Option<NodeId> },
    /// Node capabilities updated
    CapabilitiesUpdated {
        node_id: NodeId,
        capabilities: NodeCapabilities,
    },
    /// Distributed search request received
    SearchRequested {
        query: String,
        request_id: String,
        origin_node: NodeId,
    },
    /// Distributed search result received
    SearchResultReceived {
        request_id: String,
        results: serde_json::Value,
        node_id: NodeId,
    },
    /// Knowledge entry synced from another node
    KnowledgeSynced {
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
        origin_node: NodeId,
    },
    /// Swarm search request (active web search)
    SwarmSearchRequested {
        query: String,
        depth: usize,
        request_id: String,
        origin_node: NodeId,
    },
    /// Task delegated to another agent
    TaskDelegated {
        task: String,
        target_agent_id: Option<Uuid>,
        origin_node: NodeId,
    },
    /// Knowledge proposal received
    KnowledgeProposed {
        proposal_id: Uuid,
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
        origin_node: NodeId,
    },
    /// Vote received for a knowledge proposal
    KnowledgeVoteReceived {
        proposal_id: Uuid,
        voter_id: NodeId,
        approved: bool,
        confidence: f32,
    },
    /// Knowledge proposal committed
    KnowledgeCommitted {
        proposal_id: Uuid,
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    },
    /// Swarm plan proposed
    SwarmPlanProposed {
        plan: spine_protocol::SwarmPlan,
        origin_node: NodeId,
    },
    /// Plan task updated
    PlanTaskUpdated {
        plan_id: Uuid,
        task_id: Uuid,
        status: spine_protocol::TaskStatus,
        result: Option<serde_json::Value>,
        node_id: NodeId,
    },
}

/// Snapshot of cluster state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClusterState {
    pub nodes: HashMap<NodeId, NodeInfo>,
    pub sessions: HashMap<SessionId, SessionLocation>,
    pub leader_id: Option<NodeId>,
    pub term: u64,
}

// =============================================================================
// CLUSTER NODE
// =============================================================================

/// A node in the SPINE CLUSTER
pub struct ClusterNode {
    /// This node's ID
    pub id: NodeId,
    /// This node's address
    pub address: SocketAddr,
    /// Known cluster nodes
    nodes: Arc<DashMap<NodeId, NodeInfo>>,
    /// Session locations
    sessions: Arc<DashMap<SessionId, SessionLocation>>,
    /// Current cluster leader
    leader: Arc<RwLock<Option<NodeId>>>,
    /// Current election term
    term: Arc<RwLock<u64>>,
    /// Load balancing strategy
    lb_strategy: LoadBalancingStrategy,
    /// Round-robin index
    rr_index: Arc<std::sync::atomic::AtomicUsize>,
    /// Shutdown signal
    shutdown_tx: Option<broadcast::Sender<()>>,
    /// Node capabilities
    capabilities: NodeCapabilities,
    /// Event sender to application
    event_tx: mpsc::UnboundedSender<ClusterEvent>,
    /// Event receiver for application
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<ClusterEvent>>>,
    /// Consensus threshold (percentage of nodes required to agree)
    consensus_threshold: f32,
}

impl ClusterNode {
    /// Create a new cluster node
    pub fn new(address: SocketAddr, capabilities: NodeCapabilities) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            id: Uuid::new_v4(),
            address,
            nodes: Arc::new(DashMap::new()),
            sessions: Arc::new(DashMap::new()),
            leader: Arc::new(RwLock::new(None)),
            term: Arc::new(RwLock::new(0)),
            lb_strategy: LoadBalancingStrategy::LeastConnections,
            rr_index: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            shutdown_tx: None,
            capabilities,
            event_tx: tx,
            event_rx: Arc::new(Mutex::new(rx)),
            consensus_threshold: 0.67, // Default to 2/3 majority
        }
    }

    /// Get the event receiver
    pub fn get_event_receiver(&self) -> Arc<Mutex<mpsc::UnboundedReceiver<ClusterEvent>>> {
        self.event_rx.clone()
    }

    /// Get own capabilities
    pub fn get_capabilities(&self) -> NodeCapabilities {
        self.capabilities.clone()
    }

    /// Get the consensus threshold
    pub fn get_consensus_threshold(&self) -> f32 {
        self.consensus_threshold
    }

    /// Start the cluster node
    pub async fn start(&mut self) -> Result<()> {
        let (shutdown_tx, _) = broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        // Register self
        let self_info = NodeInfo {
            id: self.id,
            address: self.address,
            status: NodeStatus::Healthy,
            last_heartbeat: current_time_millis(),
            sessions_count: 0,
            load: 0.0,
            capabilities: self.capabilities.clone(),
        };
        self.nodes.insert(self.id, self_info);

        // Start heartbeat task
        let nodes = self.nodes.clone();
        let id = self.id;
        let mut shutdown_rx = shutdown_tx.subscribe();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Update own heartbeat
                        if let Some(mut node) = nodes.get_mut(&id) {
                            node.last_heartbeat = current_time_millis();
                        }

                        // Check for dead nodes
                        let now = current_time_millis();
                        let dead_threshold = 15000; // 15 seconds

                        for mut entry in nodes.iter_mut() {
                            if entry.id != id && now - entry.last_heartbeat > dead_threshold {
                                entry.status = NodeStatus::Unreachable;
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });

        // Start message listener
        let listener = tokio::net::TcpListener::bind(self.address).await?;
        let nodes_clone = self.nodes.clone();
        let event_tx = self.event_tx.clone();
        let id_clone = self.id;

        tokio::spawn(async move {
            loop {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let nodes = nodes_clone.clone();
                    let event_tx = event_tx.clone();
                    let id = id_clone;

                    tokio::spawn(async move {
                        use tokio::io::AsyncReadExt;
                        let mut buf = Vec::new();
                        if socket.read_to_end(&mut buf).await.is_ok() {
                            if let Ok(msg) = serde_json::from_slice::<ClusterMessage>(&buf) {
                                match msg {
                                    ClusterMessage::Heartbeat {
                                        node_id,
                                        load,
                                        sessions_count,
                                    } => {
                                        if let Some(mut node) = nodes.get_mut(&node_id) {
                                            node.last_heartbeat = current_time_millis();
                                            node.load = load;
                                            node.sessions_count = sessions_count;
                                        }
                                    }
                                    ClusterMessage::SessionTransferRequest {
                                        session_id,
                                        from_node,
                                        to_node,
                                    } => {
                                        if to_node == id {
                                            let _ = event_tx.send(
                                                ClusterEvent::SessionTransferRequested {
                                                    session_id,
                                                    from_node,
                                                },
                                            );
                                        }
                                    }
                                    ClusterMessage::SessionTransferData { session_id, data } => {
                                        let _ = event_tx.send(ClusterEvent::SessionDataReceived {
                                            session_id,
                                            data,
                                        });
                                    }
                                    ClusterMessage::SearchRequest {
                                        query,
                                        request_id,
                                        origin_node,
                                    } => {
                                        if origin_node != id {
                                            let _ = event_tx.send(ClusterEvent::SearchRequested {
                                                query,
                                                request_id,
                                                origin_node,
                                            });
                                        }
                                    }
                                    ClusterMessage::SearchResponse {
                                        request_id,
                                        results,
                                        node_id,
                                    } => {
                                        let _ = event_tx.send(ClusterEvent::SearchResultReceived {
                                            request_id,
                                            results,
                                            node_id,
                                        });
                                    }
                                    ClusterMessage::UpdateCapabilities {
                                        node_id,
                                        capabilities,
                                    } => {
                                        if let Some(mut node) = nodes.get_mut(&node_id) {
                                            node.capabilities = capabilities.clone();
                                        }
                                        let _ = event_tx.send(ClusterEvent::CapabilitiesUpdated {
                                            node_id,
                                            capabilities,
                                        });
                                    }
                                    ClusterMessage::SyncKnowledge {
                                        key,
                                        value,
                                        tags,
                                        origin_node,
                                    } => {
                                        if origin_node != id {
                                            let _ = event_tx.send(ClusterEvent::KnowledgeSynced {
                                                key,
                                                value,
                                                tags,
                                                origin_node,
                                            });
                                        }
                                    }
                                    ClusterMessage::SwarmSearchRequest {
                                        query,
                                        depth,
                                        request_id,
                                        origin_node,
                                    } => {
                                        if origin_node != id {
                                            let _ =
                                                event_tx.send(ClusterEvent::SwarmSearchRequested {
                                                    query,
                                                    depth,
                                                    request_id,
                                                    origin_node,
                                                });
                                        }
                                    }
                                    ClusterMessage::TaskDelegation {
                                        task,
                                        target_agent_id,
                                        origin_node,
                                    } => {
                                        if origin_node != id {
                                            let _ = event_tx.send(ClusterEvent::TaskDelegated {
                                                task,
                                                target_agent_id,
                                                origin_node,
                                            });
                                        }
                                    }
                                    ClusterMessage::ProposeKnowledge {
                                        proposal_id,
                                        key,
                                        value,
                                        tags,
                                        origin_node,
                                    } => {
                                        if origin_node != id {
                                            let _ =
                                                event_tx.send(ClusterEvent::KnowledgeProposed {
                                                    proposal_id,
                                                    key,
                                                    value,
                                                    tags,
                                                    origin_node,
                                                });
                                        }
                                    }
                                    ClusterMessage::KnowledgeVote {
                                        proposal_id,
                                        voter_id,
                                        approved,
                                        confidence,
                                    } => {
                                        let _ =
                                            event_tx.send(ClusterEvent::KnowledgeVoteReceived {
                                                proposal_id,
                                                voter_id,
                                                approved,
                                                confidence,
                                            });
                                    }
                                    ClusterMessage::CommitKnowledge {
                                        proposal_id,
                                        key,
                                        value,
                                        tags,
                                    } => {
                                        let _ = event_tx.send(ClusterEvent::KnowledgeCommitted {
                                            proposal_id,
                                            key,
                                            value,
                                            tags,
                                        });
                                    }
                                    ClusterMessage::ProposeSwarmPlan { plan, origin_node } => {
                                        if origin_node != id {
                                            let _ =
                                                event_tx.send(ClusterEvent::SwarmPlanProposed {
                                                    plan,
                                                    origin_node,
                                                });
                                        }
                                    }
                                    ClusterMessage::UpdatePlanTask {
                                        plan_id,
                                        task_id,
                                        status,
                                        result,
                                        node_id,
                                    } => {
                                        let _ = event_tx.send(ClusterEvent::PlanTaskUpdated {
                                            plan_id,
                                            task_id,
                                            status,
                                            result,
                                            node_id,
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                    });
                }
            }
        });

        log::info!("Cluster node {} started on {}", self.id, self.address);
        Ok(())
    }

    /// Broadcast a message to all healthy nodes
    pub async fn broadcast_message(&self, msg: ClusterMessage) -> Result<()> {
        let data = serde_json::to_vec(&msg)?;
        let nodes: Vec<_> = self
            .nodes
            .iter()
            .filter(|n| n.id != self.id && n.status == NodeStatus::Healthy)
            .map(|n| n.address)
            .collect();

        for addr in nodes {
            let data = data.clone();
            tokio::spawn(async move {
                if let Ok(mut stream) = tokio::net::TcpStream::connect(addr).await {
                    use tokio::io::AsyncWriteExt;
                    let _ = stream.write_all(&data).await;
                }
            });
        }
        Ok(())
    }

    /// Propose knowledge for consensus
    pub async fn propose_knowledge(
        &self,
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    ) -> Result<Uuid> {
        let proposal_id = Uuid::new_v4();
        let msg = ClusterMessage::ProposeKnowledge {
            proposal_id,
            key,
            value,
            tags,
            origin_node: self.id,
        };
        self.broadcast_message(msg).await?;
        Ok(proposal_id)
    }

    /// Vote on a knowledge proposal
    pub async fn vote_on_knowledge(
        &self,
        proposal_id: Uuid,
        approved: bool,
        confidence: f32,
    ) -> Result<()> {
        let msg = ClusterMessage::KnowledgeVote {
            proposal_id,
            voter_id: self.id,
            approved,
            confidence,
        };
        // Send vote to the leader (or broadcast if no leader)
        self.broadcast_message(msg).await
    }

    /// Commit knowledge after consensus
    pub async fn commit_knowledge(
        &self,
        proposal_id: Uuid,
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    ) -> Result<()> {
        let msg = ClusterMessage::CommitKnowledge {
            proposal_id,
            key,
            value,
            tags,
        };
        self.broadcast_message(msg).await
    }

    /// Propose a swarm plan to the cluster
    pub async fn propose_swarm_plan(&self, plan: spine_protocol::SwarmPlan) -> Result<()> {
        let msg = ClusterMessage::ProposeSwarmPlan {
            plan,
            origin_node: self.id,
        };
        self.broadcast_message(msg).await
    }

    /// Update a task in a swarm plan
    pub async fn update_plan_task(
        &self,
        plan_id: Uuid,
        task_id: Uuid,
        status: spine_protocol::TaskStatus,
        result: Option<serde_json::Value>,
    ) -> Result<()> {
        let msg = ClusterMessage::UpdatePlanTask {
            plan_id,
            task_id,
            status,
            result,
            node_id: self.id,
        };
        self.broadcast_message(msg).await
    }

    /// Broadcast a search query to the cluster
    pub async fn broadcast_search(&self, query: String, request_id: String) -> Result<()> {
        let msg = ClusterMessage::SearchRequest {
            query,
            request_id,
            origin_node: self.id,
        };
        self.broadcast_message(msg).await
    }

    /// Broadcast knowledge entry to all nodes
    pub async fn broadcast_knowledge(
        &self,
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    ) -> Result<()> {
        let msg = ClusterMessage::SyncKnowledge {
            key,
            value,
            tags,
            origin_node: self.id,
        };
        self.broadcast_message(msg).await
    }

    /// Broadcast updated capabilities (skills) to the cluster
    pub async fn broadcast_capabilities(&self, capabilities: NodeCapabilities) -> Result<()> {
        let msg = ClusterMessage::UpdateCapabilities {
            node_id: self.id,
            capabilities,
        };
        self.broadcast_message(msg).await
    }

    /// Broadcast a swarm search request
    pub async fn broadcast_swarm_search(
        &self,
        query: String,
        depth: usize,
        request_id: String,
    ) -> Result<()> {
        let msg = ClusterMessage::SwarmSearchRequest {
            query,
            depth,
            request_id,
            origin_node: self.id,
        };
        self.broadcast_message(msg).await
    }

    /// Delegate a task to the cluster
    pub async fn delegate_task(&self, task: String, target_agent_id: Option<Uuid>) -> Result<()> {
        let msg = ClusterMessage::TaskDelegation {
            task,
            target_agent_id,
            origin_node: self.id,
        };
        self.broadcast_message(msg).await
    }

    /// Send search results back to the origin node
    pub async fn send_search_results(
        &self,
        target_node_id: NodeId,
        request_id: String,
        results: serde_json::Value,
    ) -> Result<()> {
        if let Some(node) = self.nodes.get(&target_node_id) {
            let msg = ClusterMessage::SearchResponse {
                request_id,
                results,
                node_id: self.id,
            };
            let data = serde_json::to_vec(&msg)?;
            let addr = node.address;
            tokio::spawn(async move {
                if let Ok(mut stream) = tokio::net::TcpStream::connect(addr).await {
                    use tokio::io::AsyncWriteExt;
                    let _ = stream.write_all(&data).await;
                }
            });
            Ok(())
        } else {
            Err(anyhow::anyhow!("Target node not found"))
        }
    }

    /// Join an existing cluster
    pub async fn join_cluster(&self, seed_addr: SocketAddr) -> Result<()> {
        log::info!("Joining cluster via seed node {}", seed_addr);

        let stream = TcpStream::connect(seed_addr)
            .await
            .context("Failed to connect to seed node")?;

        // Send join request
        let join_msg = ClusterMessage::JoinRequest {
            node_info: NodeInfo {
                id: self.id,
                address: self.address,
                status: NodeStatus::Healthy,
                last_heartbeat: current_time_millis(),
                sessions_count: 0,
                load: 0.0,
                capabilities: self.capabilities.clone(),
            },
        };

        // In a real implementation, we'd serialize and send the message
        // For now, just log
        log::info!("Sent join request: {:?}", join_msg);
        drop(stream);

        Ok(())
    }

    /// Get the best node for a new session
    pub fn select_node(&self) -> Option<NodeId> {
        let healthy_nodes: Vec<_> = self
            .nodes
            .iter()
            .filter(|n| n.status == NodeStatus::Healthy)
            .map(|n| (n.id, n.load, n.sessions_count))
            .collect();

        if healthy_nodes.is_empty() {
            return None;
        }

        match self.lb_strategy {
            LoadBalancingStrategy::RoundRobin => {
                let idx = self
                    .rr_index
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some(healthy_nodes[idx % healthy_nodes.len()].0)
            }
            LoadBalancingStrategy::LeastConnections => healthy_nodes
                .iter()
                .min_by_key(|(_, _, count)| *count)
                .map(|(id, _, _)| *id),
            LoadBalancingStrategy::Random => {
                use std::collections::hash_map::RandomState;
                use std::hash::{BuildHasher, Hasher};
                let idx = RandomState::new().build_hasher().finish() as usize;
                Some(healthy_nodes[idx % healthy_nodes.len()].0)
            }
            _ => {
                // Default to round-robin
                let idx = self
                    .rr_index
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some(healthy_nodes[idx % healthy_nodes.len()].0)
            }
        }
    }

    /// Get node for an existing session (session affinity)
    pub fn get_session_node(&self, session_id: &SessionId) -> Option<NodeId> {
        self.sessions.get(session_id).map(|s| s.node_id)
    }

    /// Register a new session
    pub fn register_session(&self, session_id: SessionId, node_id: NodeId) {
        let now = current_time_millis();
        self.sessions.insert(
            session_id.clone(),
            SessionLocation {
                session_id,
                node_id,
                created_at: now,
                last_accessed: now,
            },
        );

        // Update node's session count
        if let Some(mut node) = self.nodes.get_mut(&node_id) {
            node.sessions_count += 1;
        }
    }

    /// Register a session on this node
    pub fn register_local_session(&self, session_id: SessionId) {
        self.register_session(session_id, self.id);
    }

    /// Remove a session
    pub fn remove_session(&self, session_id: &SessionId) {
        if let Some((_, loc)) = self.sessions.remove(session_id) {
            if let Some(mut node) = self.nodes.get_mut(&loc.node_id) {
                node.sessions_count = node.sessions_count.saturating_sub(1);
            }
        }
    }

    /// Get all healthy nodes
    pub fn get_healthy_nodes(&self) -> Vec<NodeInfo> {
        self.nodes
            .iter()
            .filter(|n| n.status == NodeStatus::Healthy)
            .map(|n| n.value().clone())
            .collect()
    }

    /// Get cluster statistics
    pub fn get_stats(&self) -> ClusterStats {
        let nodes: Vec<_> = self.nodes.iter().map(|n| n.value().clone()).collect();

        ClusterStats {
            total_nodes: nodes.len(),
            healthy_nodes: nodes
                .iter()
                .filter(|n| n.status == NodeStatus::Healthy)
                .count(),
            total_sessions: self.sessions.len(),
            avg_load: nodes.iter().map(|n| n.load).sum::<f32>() / nodes.len().max(1) as f32,
            leader_id: *self.leader.blocking_read(),
        }
    }

    /// Initiate a session transfer to another node
    pub async fn initiate_session_transfer(
        &self,
        session_id: SessionId,
        to_node: NodeId,
    ) -> Result<()> {
        let msg = ClusterMessage::SessionTransferRequest {
            session_id,
            from_node: self.id,
            to_node,
        };
        self.broadcast_message(msg).await
    }

    /// Send session data to another node
    pub async fn send_session_data(&self, session_id: SessionId, data: Vec<u8>) -> Result<()> {
        let msg = ClusterMessage::SessionTransferData { session_id, data };
        self.broadcast_message(msg).await
    }

    /// Initiate leader election
    pub async fn start_election(&self) -> Result<bool> {
        let mut term = self.term.write().await;
        *term += 1;
        let current_term = *term;
        drop(term);

        log::info!("Starting election for term {}", current_term);

        // In a simplified implementation, the node with lowest ID wins
        let mut lowest_id = self.id;
        for entry in self.nodes.iter() {
            if entry.status == NodeStatus::Healthy && entry.id < lowest_id {
                lowest_id = entry.id;
            }
        }

        let is_leader = lowest_id == self.id;

        if is_leader {
            let mut leader = self.leader.write().await;
            *leader = Some(self.id);
            log::info!(
                "This node ({}) elected as leader for term {}",
                self.id,
                current_term
            );
        } else {
            let mut leader = self.leader.write().await;
            *leader = Some(lowest_id);
            log::info!(
                "Node {} elected as leader for term {}",
                lowest_id,
                current_term
            );
        }

        Ok(is_leader)
    }

    /// Gracefully leave the cluster
    pub async fn leave(&self) -> Result<()> {
        log::info!("Node {} leaving cluster", self.id);

        // Update own status
        if let Some(mut node) = self.nodes.get_mut(&self.id) {
            node.status = NodeStatus::Draining;
        }

        // Notify other nodes (in real impl, would send LeaveNotification)

        // Signal shutdown
        if let Some(tx) = &self.shutdown_tx {
            let _ = tx.send(());
        }

        Ok(())
    }
}

/// Cluster statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStats {
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub total_sessions: usize,
    pub avg_load: f32,
    pub leader_id: Option<NodeId>,
}

// =============================================================================
// CLUSTER CLIENT
// =============================================================================

/// Client for connecting to a SPINE CLUSTER
pub struct ClusterClient {
    /// Known cluster nodes
    nodes: Vec<SocketAddr>,
    /// Session to node mapping (cache)
    session_cache: DashMap<SessionId, SocketAddr>,
    /// Load balancing strategy
    lb_strategy: LoadBalancingStrategy,
    /// Current index for round-robin
    rr_index: std::sync::atomic::AtomicUsize,
}

impl ClusterClient {
    /// Create a new cluster client
    pub fn new(seed_nodes: Vec<SocketAddr>) -> Self {
        Self {
            nodes: seed_nodes,
            session_cache: DashMap::new(),
            lb_strategy: LoadBalancingStrategy::RoundRobin,
            rr_index: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Get connection address for a session
    pub fn get_connection(&self, session_id: Option<&SessionId>) -> Option<SocketAddr> {
        // Check session cache first
        if let Some(sid) = session_id {
            if let Some(addr) = self.session_cache.get(sid) {
                return Some(*addr);
            }
        }

        // Otherwise, load balance
        if self.nodes.is_empty() {
            return None;
        }

        match self.lb_strategy {
            LoadBalancingStrategy::RoundRobin => {
                let idx = self
                    .rr_index
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some(self.nodes[idx % self.nodes.len()])
            }
            LoadBalancingStrategy::Random => {
                use std::collections::hash_map::RandomState;
                use std::hash::{BuildHasher, Hasher};
                let idx = RandomState::new().build_hasher().finish() as usize;
                Some(self.nodes[idx % self.nodes.len()])
            }
            _ => Some(self.nodes[0]),
        }
    }

    /// Associate a session with a node
    pub fn bind_session(&self, session_id: SessionId, addr: SocketAddr) {
        self.session_cache.insert(session_id, addr);
    }

    /// Remove session binding
    pub fn unbind_session(&self, session_id: &SessionId) {
        self.session_cache.remove(session_id);
    }

    /// Refresh cluster node list
    pub async fn refresh_nodes(&mut self) -> Result<()> {
        // In a real implementation, this would query the cluster for current nodes
        Ok(())
    }
}

// =============================================================================
// HELPERS
// =============================================================================

fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_node_creation() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let node = ClusterNode::new(addr, NodeCapabilities::default());

        assert_eq!(node.address, addr);
        assert!(node.nodes.is_empty() || node.nodes.len() == 0);
    }

    #[test]
    fn test_session_registration() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let node = ClusterNode::new(addr, NodeCapabilities::default());

        let session_id = "session-123".to_string();
        let node_id = node.id;

        // Register self first
        node.nodes.insert(
            node_id,
            NodeInfo {
                id: node_id,
                address: addr,
                status: NodeStatus::Healthy,
                last_heartbeat: current_time_millis(),
                sessions_count: 0,
                load: 0.0,
                capabilities: NodeCapabilities::default(),
            },
        );

        node.register_session(session_id.clone(), node_id);

        assert!(node.sessions.contains_key(&session_id));
        assert_eq!(node.get_session_node(&session_id), Some(node_id));
    }

    #[test]
    fn test_load_balancing() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let node = ClusterNode::new(addr, NodeCapabilities::default());

        // Add multiple nodes
        for i in 0..3 {
            let id = Uuid::new_v4();
            node.nodes.insert(
                id,
                NodeInfo {
                    id,
                    address: format!("127.0.0.1:808{}", i).parse().unwrap(),
                    status: NodeStatus::Healthy,
                    last_heartbeat: current_time_millis(),
                    sessions_count: i,
                    load: i as f32 * 0.1,
                    capabilities: NodeCapabilities::default(),
                },
            );
        }

        // Should select a node
        let selected = node.select_node();
        assert!(selected.is_some());
    }

    #[test]
    fn test_cluster_client() {
        let nodes = vec![
            "127.0.0.1:8080".parse().unwrap(),
            "127.0.0.1:8081".parse().unwrap(),
        ];
        let client = ClusterClient::new(nodes);

        // Should get a connection
        let conn = client.get_connection(None);
        assert!(conn.is_some());

        // Session binding
        let session_id = "test-session".to_string();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        client.bind_session(session_id.clone(), addr);

        let conn = client.get_connection(Some(&session_id));
        assert_eq!(conn, Some(addr));
    }
}
