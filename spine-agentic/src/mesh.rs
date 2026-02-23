//! # Agent Mesh Networking
//!
//! Peer-to-peer encrypted mesh for inter-agent communication.
//! Agents connect directly to each other, route messages through
//! intermediate peers, and discover new peers via gossip.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────┐     ┌──────────┐     ┌──────────┐
//! │  Agent A │────▶│  Agent B │────▶│  Agent C │
//! │ (MeshNode)│◀───│ (MeshNode)│◀───│ (MeshNode)│
//! └──────────┘     └──────────┘     └──────────┘
//!       │               │                │
//!       └───────────────┴────────────────┘
//!              Gossip peer discovery
//! ```
//!
//! ## Features
//!
//! - **Encrypted transport**: Uses spine-protocol's `ProtocolHandler` for wire encryption
//! - **Multi-hop routing**: Messages traverse the mesh via shortest path with TTL
//! - **Gossip discovery**: Peers periodically share their peer tables
//! - **Signed messages**: Every message is signed with the sender's Ed25519 key
//! - **Connection management**: Automatic reconnection, peer health monitoring

use crate::identity::{PublicIdentity, SigningIdentity};
use crate::AgentId;
use anyhow::Result;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

// ──────────────────────────── Configuration ────────────────────────────

/// Configuration for a mesh node.
#[derive(Debug, Clone)]
pub struct MeshConfig {
    /// Address to listen on for incoming peer connections.
    pub listen_addr: SocketAddr,
    /// Maximum number of simultaneous peer connections.
    pub max_peers: usize,
    /// Maximum number of hops a message can traverse.
    pub max_hops: u8,
    /// Interval between gossip rounds (seconds).
    pub gossip_interval_secs: u64,
    /// Time before a peer is considered stale (seconds).
    pub peer_timeout_secs: u64,
    /// Optional symmetric encryption key for the protocol layer.
    pub encryption_key: Option<[u8; 32]>,
    /// Enable Chameleon moving-target encryption.
    pub chameleon: bool,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            max_peers: 64,
            max_hops: 8,
            gossip_interval_secs: 30,
            peer_timeout_secs: 120,
            encryption_key: None,
            chameleon: false,
        }
    }
}

// ──────────────────────────── Peer State ────────────────────────────

/// State of a peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PeerState {
    /// Discovered via gossip but not yet connected.
    Discovered,
    /// TCP connection in progress.
    Connecting,
    /// Fully connected with active protocol handler.
    Connected,
    /// Was connected but link dropped.
    Disconnected,
    /// Administratively banned (misbehavior, Sybil, etc.).
    Banned,
}

/// Information about a known peer in the mesh.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// The peer's agent identity.
    pub agent_id: AgentId,
    /// Network address.
    pub addr: SocketAddr,
    /// Current connection state.
    pub state: PeerState,
    /// The peer's public identity (if known).
    pub identity: Option<PublicIdentity>,
    /// When we last heard from this peer.
    pub last_seen: DateTime<Utc>,
    /// Round-trip latency estimate in milliseconds.
    pub latency_ms: u64,
    /// Messages sent to this peer.
    pub messages_sent: u64,
    /// Messages received from this peer.
    pub messages_received: u64,
    /// Hop count from us to this peer (1 = direct).
    pub hop_count: u8,
}

impl PeerInfo {
    /// Create a new peer info for a directly discovered peer.
    pub fn new(agent_id: AgentId, addr: SocketAddr) -> Self {
        Self {
            agent_id,
            addr,
            state: PeerState::Discovered,
            identity: None,
            last_seen: Utc::now(),
            latency_ms: 0,
            messages_sent: 0,
            messages_received: 0,
            hop_count: 1,
        }
    }

    /// Whether the peer is considered healthy.
    pub fn is_healthy(&self) -> bool {
        matches!(self.state, PeerState::Connected)
    }

    /// Whether the peer is stale (not seen within timeout).
    pub fn is_stale(&self, timeout_secs: u64) -> bool {
        let elapsed = Utc::now()
            .signed_duration_since(self.last_seen)
            .num_seconds();
        elapsed > timeout_secs as i64
    }
}

// ──────────────────────────── Mesh Messages ────────────────────────────

/// A routable message in the mesh network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshEnvelope {
    /// Unique message identifier.
    pub id: Uuid,
    /// Originating agent.
    pub from: AgentId,
    /// Destination agent (or broadcast).
    pub to: MeshTarget,
    /// Time-to-live (decremented at each hop).
    pub ttl: u8,
    /// Agents this message has passed through.
    pub hops: Vec<AgentId>,
    /// The message payload.
    pub payload: MeshPayload,
    /// Signature over (id || from || to || payload) by the originator.
    pub signature: Vec<u8>,
    /// When the message was created.
    pub timestamp: DateTime<Utc>,
}

/// Target of a mesh message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MeshTarget {
    /// Direct message to a specific agent.
    Agent(AgentId),
    /// Broadcast to all connected peers (with TTL limiting spread).
    Broadcast,
    /// Multicast to agents with a specific capability.
    Capability(String),
}

/// Payload types for mesh messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshPayload {
    /// An inter-agent message (wraps the existing AgentMessage).
    AgentMessage(AgentMessageCompact),
    /// Peer table gossip announcement.
    PeerAnnounce(Vec<PeerAnnouncement>),
    /// Request peer table from a neighbor.
    PeerQuery,
    /// Ping for latency measurement (contains timestamp nonce).
    Ping(u64),
    /// Pong response to ping.
    Pong(u64),
    /// Request a route to a target agent.
    RouteRequest { target: AgentId },
    /// Response with a discovered route.
    RouteResponse {
        target: AgentId,
        route: Vec<AgentId>,
    },
    /// Knowledge synchronization (wraps CollectiveMemory entries).
    KnowledgeSync {
        topic: String,
        entries: Vec<serde_json::Value>,
    },
    /// Swarm task invitation.
    SwarmInvite {
        swarm_id: Uuid,
        task_description: String,
        required_capabilities: Vec<String>,
    },
    /// Swarm task response.
    SwarmResponse {
        swarm_id: Uuid,
        accepted: bool,
        reason: Option<String>,
    },
}

/// Compact agent message for mesh transport (avoids Box<MessageContent> serialization issues).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessageCompact {
    pub id: Uuid,
    pub from: AgentId,
    pub to: AgentId,
    pub content_type: String,
    pub content_data: serde_json::Value,
    pub reply_to: Option<Uuid>,
    pub thread_id: Option<Uuid>,
}

/// Announcement of a peer for gossip propagation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAnnouncement {
    /// The peer's agent id.
    pub agent_id: AgentId,
    /// The peer's network address.
    pub addr: SocketAddr,
    /// Capabilities this peer advertises.
    pub capabilities: Vec<String>,
    /// How many hops away (incremented at each gossip relay).
    pub hop_count: u8,
}

// ──────────────────────────── Routing Table ────────────────────────────

/// A routing table for multi-hop message delivery.
#[derive(Debug, Clone)]
pub struct RoutingTable {
    /// Routes: target AgentId → next-hop AgentId
    routes: HashMap<AgentId, RouteEntry>,
}

/// A single routing table entry.
#[derive(Debug, Clone)]
pub struct RouteEntry {
    /// Next hop toward the target.
    pub next_hop: AgentId,
    /// Total number of hops to reach target.
    pub hop_count: u8,
    /// When this route was last updated.
    pub updated_at: DateTime<Utc>,
    /// Estimated latency in ms.
    pub latency_ms: u64,
}

impl RoutingTable {
    /// Create an empty routing table.
    pub fn new() -> Self {
        Self {
            routes: HashMap::new(),
        }
    }

    /// Update a route if it's better (fewer hops or fresher).
    pub fn update(&mut self, target: AgentId, next_hop: AgentId, hop_count: u8, latency_ms: u64) {
        let entry = RouteEntry {
            next_hop,
            hop_count,
            updated_at: Utc::now(),
            latency_ms,
        };

        match self.routes.get(&target) {
            Some(existing) if existing.hop_count <= hop_count => {
                // Existing route is as good or better — skip
            }
            _ => {
                self.routes.insert(target, entry);
            }
        }
    }

    /// Look up the next hop for a target.
    pub fn next_hop(&self, target: &AgentId) -> Option<&RouteEntry> {
        self.routes.get(target)
    }

    /// Remove stale routes older than the given duration.
    pub fn prune_stale(&mut self, max_age_secs: u64) {
        let now = Utc::now();
        self.routes.retain(|_, entry| {
            now.signed_duration_since(entry.updated_at).num_seconds() < max_age_secs as i64
        });
    }

    /// Get the number of known routes.
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Iterate over all routes.
    pub fn iter(&self) -> impl Iterator<Item = (&AgentId, &RouteEntry)> {
        self.routes.iter()
    }
}

impl Default for RoutingTable {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────── Message Deduplication ────────────────────────────

/// Tracks recently seen message IDs to prevent routing loops.
pub struct MessageDedup {
    seen: VecDeque<(Uuid, DateTime<Utc>)>,
    max_entries: usize,
}

impl MessageDedup {
    /// Create a new dedup tracker.
    pub fn new(max_entries: usize) -> Self {
        Self {
            seen: VecDeque::with_capacity(max_entries),
            max_entries,
        }
    }

    /// Check if a message ID has been seen. If new, record it and return false.
    /// If already seen, return true.
    pub fn check_and_insert(&mut self, id: Uuid) -> bool {
        // Check if already seen
        if self.seen.iter().any(|(seen_id, _)| *seen_id == id) {
            return true; // duplicate
        }

        // Insert and evict old entries if at capacity
        if self.seen.len() >= self.max_entries {
            self.seen.pop_front();
        }
        self.seen.push_back((id, Utc::now()));
        false // new message
    }

    /// Prune entries older than the given age.
    pub fn prune(&mut self, max_age_secs: u64) {
        let cutoff = Utc::now() - chrono::Duration::seconds(max_age_secs as i64);
        while let Some((_, ts)) = self.seen.front() {
            if *ts < cutoff {
                self.seen.pop_front();
            } else {
                break;
            }
        }
    }

    /// Number of tracked message IDs.
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// Whether the tracker is empty.
    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

// ──────────────────────────── Mesh Statistics ────────────────────────────

/// Real-time statistics for a mesh node.
#[derive(Debug)]
pub struct MeshStats {
    /// Total messages routed (forwarded through this node).
    pub messages_routed: AtomicU64,
    /// Total messages delivered locally.
    pub messages_delivered: AtomicU64,
    /// Total messages dropped (TTL expired, no route, etc.).
    pub messages_dropped: AtomicU64,
    /// Total messages sent by this node.
    pub messages_sent: AtomicU64,
    /// Currently connected peers.
    pub peers_connected: AtomicU64,
    /// Total bytes sent across all peers.
    pub bytes_sent: AtomicU64,
    /// Total bytes received across all peers.
    pub bytes_received: AtomicU64,
    /// Total gossip rounds completed.
    pub gossip_rounds: AtomicU64,
}

impl MeshStats {
    /// Create a new stats tracker.
    pub fn new() -> Self {
        Self {
            messages_routed: AtomicU64::new(0),
            messages_delivered: AtomicU64::new(0),
            messages_dropped: AtomicU64::new(0),
            messages_sent: AtomicU64::new(0),
            peers_connected: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            gossip_rounds: AtomicU64::new(0),
        }
    }

    /// Get a snapshot of the current stats.
    pub fn snapshot(&self) -> MeshStatsSnapshot {
        MeshStatsSnapshot {
            messages_routed: self.messages_routed.load(Ordering::Relaxed),
            messages_delivered: self.messages_delivered.load(Ordering::Relaxed),
            messages_dropped: self.messages_dropped.load(Ordering::Relaxed),
            messages_sent: self.messages_sent.load(Ordering::Relaxed),
            peers_connected: self.peers_connected.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
            gossip_rounds: self.gossip_rounds.load(Ordering::Relaxed),
        }
    }
}

impl Default for MeshStats {
    fn default() -> Self {
        Self::new()
    }
}

/// A point-in-time snapshot of mesh statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshStatsSnapshot {
    pub messages_routed: u64,
    pub messages_delivered: u64,
    pub messages_dropped: u64,
    pub messages_sent: u64,
    pub peers_connected: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub gossip_rounds: u64,
}

// ──────────────────────────── Mesh Node ────────────────────────────

/// A peer-to-peer mesh networking node.
///
/// Each agent runs a `MeshNode` that:
/// - Listens for incoming peer connections
/// - Maintains connections to known peers
/// - Routes messages through the mesh (multi-hop)
/// - Discovers new peers via gossip protocol
/// - Signs all outgoing messages for non-repudiation
pub struct MeshNode {
    /// This node's signing identity.
    identity: Arc<SigningIdentity>,
    /// Configuration.
    config: MeshConfig,
    /// Known peers indexed by AgentId.
    peers: Arc<DashMap<AgentId, PeerInfo>>,
    /// Routing table for multi-hop delivery.
    routing: Arc<RwLock<RoutingTable>>,
    /// Message deduplication tracker.
    dedup: Arc<RwLock<MessageDedup>>,
    /// Channel for locally-delivered messages.
    inbox_tx: mpsc::Sender<MeshEnvelope>,
    /// Receive end for delivered messages.
    inbox_rx: Option<mpsc::Receiver<MeshEnvelope>>,
    /// Whether the node is running.
    running: Arc<AtomicBool>,
    /// Statistics.
    stats: Arc<MeshStats>,
    /// Trusted public keys (AgentId → public key).
    trusted_keys: Arc<DashMap<AgentId, Vec<u8>>>,
}

impl MeshNode {
    /// Create a new mesh node with the given identity and configuration.
    pub fn new(identity: SigningIdentity, config: MeshConfig) -> Self {
        let (inbox_tx, inbox_rx) = mpsc::channel(1024);
        Self {
            identity: Arc::new(identity),
            config,
            peers: Arc::new(DashMap::new()),
            routing: Arc::new(RwLock::new(RoutingTable::new())),
            dedup: Arc::new(RwLock::new(MessageDedup::new(10_000))),
            inbox_tx,
            inbox_rx: Some(inbox_rx),
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(MeshStats::new()),
            trusted_keys: Arc::new(DashMap::new()),
        }
    }

    /// Get this node's agent ID.
    pub fn agent_id(&self) -> &AgentId {
        &self.identity.agent_id
    }

    /// Get this node's public identity.
    pub fn public_identity(&self) -> PublicIdentity {
        self.identity.public_identity()
    }

    /// Get the current mesh statistics.
    pub fn stats(&self) -> MeshStatsSnapshot {
        self.stats.snapshot()
    }

    /// Get the number of known peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Get the number of connected peers.
    pub fn connected_peer_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|entry| entry.value().state == PeerState::Connected)
            .count()
    }

    /// Take the inbox receiver (can only be called once).
    pub fn take_inbox(&mut self) -> Option<mpsc::Receiver<MeshEnvelope>> {
        self.inbox_rx.take()
    }

    /// Register a peer address for direct connection.
    pub fn add_peer(&self, agent_id: AgentId, addr: SocketAddr) {
        if agent_id == self.identity.agent_id {
            return; // Don't add ourselves
        }
        if !self.peers.contains_key(&agent_id) {
            self.peers.insert(agent_id, PeerInfo::new(agent_id, addr));
        }
    }

    /// Trust a peer's public key (for signature verification).
    pub fn trust_key(&self, agent_id: AgentId, public_key: Vec<u8>) {
        self.trusted_keys.insert(agent_id, public_key);
    }

    /// Mark a peer as connected.
    pub fn mark_connected(&self, agent_id: &AgentId) {
        if let Some(mut peer) = self.peers.get_mut(agent_id) {
            peer.state = PeerState::Connected;
            peer.last_seen = Utc::now();
            self.stats.peers_connected.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Mark a peer as disconnected.
    pub fn mark_disconnected(&self, agent_id: &AgentId) {
        if let Some(mut peer) = self.peers.get_mut(agent_id) {
            peer.state = PeerState::Disconnected;
            self.stats.peers_connected.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Ban a misbehaving peer.
    pub fn ban_peer(&self, agent_id: &AgentId) {
        if let Some(mut peer) = self.peers.get_mut(agent_id) {
            peer.state = PeerState::Banned;
        }
    }

    /// Get information about a specific peer.
    pub fn get_peer(&self, agent_id: &AgentId) -> Option<PeerInfo> {
        self.peers.get(agent_id).map(|entry| entry.value().clone())
    }

    /// List all known peers.
    pub fn list_peers(&self) -> Vec<PeerInfo> {
        self.peers.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Get a snapshot of the routing table.
    pub async fn routes(&self) -> Vec<(AgentId, RouteEntry)> {
        let table = self.routing.read().await;
        table.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    /// Create a signed mesh envelope for a message.
    pub fn create_envelope(&self, to: MeshTarget, payload: MeshPayload) -> MeshEnvelope {
        let id = Uuid::new_v4();
        let from = self.identity.agent_id;

        // Compute signature over (id || from || payload)
        let sig_data = Self::signature_data(&id, &from, &payload);
        let signature = self.identity.sign(&sig_data).to_vec();

        self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);

        MeshEnvelope {
            id,
            from,
            to,
            ttl: self.config.max_hops,
            hops: vec![self.identity.agent_id],
            payload,
            signature,
            timestamp: Utc::now(),
        }
    }

    /// Process an incoming envelope: verify, route, or deliver.
    pub async fn process_envelope(&self, envelope: MeshEnvelope) -> Result<MeshAction> {
        // 1. Check TTL
        if envelope.ttl == 0 {
            self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
            return Ok(MeshAction::Drop("TTL expired".into()));
        }

        // 2. Dedup check
        {
            let mut dedup = self.dedup.write().await;
            if dedup.check_and_insert(envelope.id) {
                return Ok(MeshAction::Drop("duplicate message".into()));
            }
        }

        // 3. Verify signature (if we know the sender's key)
        if let Some(key) = self.trusted_keys.get(&envelope.from) {
            let sig_data =
                Self::signature_data(&envelope.id, &envelope.from, &envelope.payload);
            if !crate::identity::Ed25519Keypair::verify(&key, &sig_data, &envelope.signature) {
                self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
                return Ok(MeshAction::Drop("invalid signature".into()));
            }
        }

        // 4. Update routing table from hops
        if !envelope.hops.is_empty() {
            let mut routing = self.routing.write().await;
            // The first hop tells us how to reach the sender
            let next_hop = envelope.hops.last().cloned().unwrap_or(envelope.from);
            routing.update(
                envelope.from,
                next_hop,
                envelope.hops.len() as u8,
                0,
            );
        }

        // 5. Determine if this is for us
        let is_for_us = match &envelope.to {
            MeshTarget::Agent(target) => *target == self.identity.agent_id,
            MeshTarget::Broadcast => true, // broadcasts are always delivered locally
            MeshTarget::Capability(_) => true, // deliver locally, let the agent decide
        };

        if is_for_us {
            self.stats
                .messages_delivered
                .fetch_add(1, Ordering::Relaxed);

            // Also forward broadcasts (but not direct messages)
            if matches!(envelope.to, MeshTarget::Broadcast) {
                return Ok(MeshAction::DeliverAndForward(envelope));
            }

            // Deliver to local inbox
            let _ = self.inbox_tx.send(envelope.clone()).await;
            return Ok(MeshAction::Deliver(envelope));
        }

        // 6. Route to next hop
        self.stats.messages_routed.fetch_add(1, Ordering::Relaxed);
        Ok(MeshAction::Forward(envelope))
    }

    /// Prepare an envelope for forwarding (decrement TTL, add our ID to hops).
    pub fn prepare_forward(&self, mut envelope: MeshEnvelope) -> Option<MeshEnvelope> {
        if envelope.ttl <= 1 {
            self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        envelope.ttl -= 1;
        envelope.hops.push(self.identity.agent_id);
        Some(envelope)
    }

    /// Look up the best next hop for a target agent.
    pub async fn resolve_next_hop(&self, target: &AgentId) -> Option<AgentId> {
        // Direct peer?
        if let Some(peer) = self.peers.get(target) {
            if peer.is_healthy() {
                return Some(*target);
            }
        }

        // Routing table?
        let routing = self.routing.read().await;
        routing.next_hop(target).map(|entry| entry.next_hop)
    }

    /// Generate gossip announcements for all known healthy peers.
    pub fn generate_gossip(&self) -> Vec<PeerAnnouncement> {
        let mut announcements = Vec::new();

        // Announce ourselves
        announcements.push(PeerAnnouncement {
            agent_id: self.identity.agent_id,
            addr: self.config.listen_addr,
            capabilities: vec!["mesh-v1".to_string()],
            hop_count: 0,
        });

        // Announce connected peers
        for entry in self.peers.iter() {
            let peer = entry.value();
            if peer.is_healthy() {
                announcements.push(PeerAnnouncement {
                    agent_id: peer.agent_id,
                    addr: peer.addr,
                    capabilities: vec![],
                    hop_count: peer.hop_count,
                });
            }
        }

        self.stats.gossip_rounds.fetch_add(1, Ordering::Relaxed);
        announcements
    }

    /// Process gossip announcements from a peer.
    pub fn process_gossip(&self, announcements: &[PeerAnnouncement]) {
        for ann in announcements {
            // Skip ourselves
            if ann.agent_id == self.identity.agent_id {
                continue;
            }
            // Skip banned peers
            if let Some(peer) = self.peers.get(&ann.agent_id) {
                if peer.state == PeerState::Banned {
                    continue;
                }
            }
            // Add or update
            if !self.peers.contains_key(&ann.agent_id) && self.peers.len() < self.config.max_peers {
                let mut info = PeerInfo::new(ann.agent_id, ann.addr);
                info.hop_count = ann.hop_count + 1;
                self.peers.insert(ann.agent_id, info);
            }
        }
    }

    /// Prune stale peers and routes.
    pub async fn prune_stale(&self) {
        let timeout = self.config.peer_timeout_secs;

        // Remove stale peers (keep banned ones)
        self.peers.retain(|_, peer| {
            !peer.is_stale(timeout) || peer.state == PeerState::Banned
        });

        // Prune routing table
        let mut routing = self.routing.write().await;
        routing.prune_stale(timeout * 2);

        // Prune dedup
        let mut dedup = self.dedup.write().await;
        dedup.prune(300); // 5 minute window
    }

    /// Compute the data to sign for a message.
    fn signature_data(id: &Uuid, from: &AgentId, payload: &MeshPayload) -> Vec<u8> {
        let mut data = Vec::with_capacity(256);
        data.extend_from_slice(id.as_bytes());
        data.extend_from_slice(from.0.as_bytes());
        if let Ok(payload_bytes) = serde_json::to_vec(payload) {
            data.extend_from_slice(&payload_bytes);
        }
        data
    }

    /// Whether the node is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Set the running state.
    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Relaxed);
    }
}

/// Action to take after processing a mesh envelope.
#[derive(Debug)]
pub enum MeshAction {
    /// Deliver the message to the local agent.
    Deliver(MeshEnvelope),
    /// Forward the message to the next hop.
    Forward(MeshEnvelope),
    /// Deliver locally AND forward to other peers (for broadcasts).
    DeliverAndForward(MeshEnvelope),
    /// Drop the message with a reason.
    Drop(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_identity(name: &str) -> SigningIdentity {
        SigningIdentity::from_seed(name, [name.len() as u8; 32])
    }

    fn test_config() -> MeshConfig {
        MeshConfig {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            max_peers: 16,
            max_hops: 4,
            gossip_interval_secs: 5,
            peer_timeout_secs: 30,
            encryption_key: None,
            chameleon: false,
        }
    }

    #[test]
    fn test_mesh_node_creation() {
        let id = test_identity("node-a");
        let agent_id = id.agent_id;
        let node = MeshNode::new(id, test_config());

        assert_eq!(*node.agent_id(), agent_id);
        assert_eq!(node.peer_count(), 0);
        assert_eq!(node.connected_peer_count(), 0);
        assert!(!node.is_running());
    }

    #[test]
    fn test_add_and_list_peers() {
        let node = MeshNode::new(test_identity("node-a"), test_config());
        let peer_id = AgentId::new();
        let addr: SocketAddr = "192.168.1.10:9000".parse().unwrap();

        node.add_peer(peer_id, addr);

        assert_eq!(node.peer_count(), 1);
        let peer = node.get_peer(&peer_id).unwrap();
        assert_eq!(peer.addr, addr);
        assert_eq!(peer.state, PeerState::Discovered);
    }

    #[test]
    fn test_no_self_peer() {
        let id = test_identity("self-test");
        let agent_id = id.agent_id;
        let node = MeshNode::new(id, test_config());

        node.add_peer(agent_id, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(node.peer_count(), 0); // Should not add self
    }

    #[test]
    fn test_peer_state_transitions() {
        let node = MeshNode::new(test_identity("state-test"), test_config());
        let peer_id = AgentId::new();

        node.add_peer(peer_id, "10.0.0.1:9000".parse().unwrap());
        assert_eq!(node.get_peer(&peer_id).unwrap().state, PeerState::Discovered);

        node.mark_connected(&peer_id);
        assert_eq!(node.get_peer(&peer_id).unwrap().state, PeerState::Connected);
        assert_eq!(node.connected_peer_count(), 1);

        node.mark_disconnected(&peer_id);
        assert_eq!(
            node.get_peer(&peer_id).unwrap().state,
            PeerState::Disconnected
        );
        assert_eq!(node.connected_peer_count(), 0);

        node.ban_peer(&peer_id);
        assert_eq!(node.get_peer(&peer_id).unwrap().state, PeerState::Banned);
    }

    #[test]
    fn test_create_signed_envelope() {
        let id = test_identity("signer");
        let node = MeshNode::new(id, test_config());
        let target = AgentId::new();

        let envelope = node.create_envelope(
            MeshTarget::Agent(target),
            MeshPayload::Ping(12345),
        );

        assert_eq!(envelope.from, *node.agent_id());
        assert_eq!(envelope.to, MeshTarget::Agent(target));
        assert!(!envelope.signature.is_empty());
        assert_eq!(envelope.ttl, 4); // max_hops from config
        assert_eq!(envelope.hops.len(), 1);
    }

    #[tokio::test]
    async fn test_process_envelope_deliver() {
        let id = test_identity("receiver");
        let receiver_id = id.agent_id;
        let node = MeshNode::new(id, test_config());

        // Create envelope addressed to this node
        let sender_id = test_identity("sender");
        let sender_node = MeshNode::new(sender_id, test_config());
        let envelope = sender_node.create_envelope(
            MeshTarget::Agent(receiver_id),
            MeshPayload::Ping(999),
        );

        let action = node.process_envelope(envelope).await.unwrap();
        assert!(matches!(action, MeshAction::Deliver(_)));

        let stats = node.stats();
        assert_eq!(stats.messages_delivered, 1);
    }

    #[tokio::test]
    async fn test_process_envelope_forward() {
        let node = MeshNode::new(test_identity("router"), test_config());
        let target = AgentId::new(); // Not us

        let sender_node = MeshNode::new(test_identity("sender"), test_config());
        let envelope =
            sender_node.create_envelope(MeshTarget::Agent(target), MeshPayload::Ping(111));

        let action = node.process_envelope(envelope).await.unwrap();
        assert!(matches!(action, MeshAction::Forward(_)));

        assert_eq!(node.stats().messages_routed, 1);
    }

    #[tokio::test]
    async fn test_process_envelope_ttl_expired() {
        let node = MeshNode::new(test_identity("ttl-test"), test_config());

        let envelope = MeshEnvelope {
            id: Uuid::new_v4(),
            from: AgentId::new(),
            to: MeshTarget::Agent(AgentId::new()),
            ttl: 0, // expired
            hops: vec![],
            payload: MeshPayload::Ping(0),
            signature: vec![0u8; 64],
            timestamp: Utc::now(),
        };

        let action = node.process_envelope(envelope).await.unwrap();
        assert!(matches!(action, MeshAction::Drop(_)));
        assert_eq!(node.stats().messages_dropped, 1);
    }

    #[tokio::test]
    async fn test_process_envelope_dedup() {
        let node = MeshNode::new(test_identity("dedup-test"), test_config());

        let envelope = MeshEnvelope {
            id: Uuid::new_v4(),
            from: AgentId::new(),
            to: MeshTarget::Agent(AgentId::new()),
            ttl: 4,
            hops: vec![],
            payload: MeshPayload::Ping(42),
            signature: vec![0u8; 64],
            timestamp: Utc::now(),
        };

        // First time: forward
        let action = node.process_envelope(envelope.clone()).await.unwrap();
        assert!(matches!(action, MeshAction::Forward(_)));

        // Second time: drop (duplicate)
        let action = node.process_envelope(envelope).await.unwrap();
        assert!(matches!(action, MeshAction::Drop(_)));
    }

    #[tokio::test]
    async fn test_signature_verification() {
        let sender = test_identity("sig-sender");
        let sender_node = MeshNode::new(sender.clone(), test_config());

        let receiver = test_identity("sig-receiver");
        let receiver_id = receiver.agent_id;
        let receiver_node = MeshNode::new(receiver, test_config());

        // Trust the sender's key
        receiver_node.trust_key(sender.agent_id, sender.public_key().to_vec());

        // Valid envelope
        let envelope = sender_node.create_envelope(
            MeshTarget::Agent(receiver_id),
            MeshPayload::Ping(77),
        );
        let action = receiver_node.process_envelope(envelope).await.unwrap();
        assert!(matches!(action, MeshAction::Deliver(_)));

        // Tampered envelope — change the payload after signing
        let mut bad_envelope = sender_node.create_envelope(
            MeshTarget::Agent(receiver_id),
            MeshPayload::Ping(77),
        );
        bad_envelope.payload = MeshPayload::Ping(99); // tamper
        let action = receiver_node.process_envelope(bad_envelope).await.unwrap();
        assert!(matches!(action, MeshAction::Drop(_)));
    }

    #[test]
    fn test_prepare_forward() {
        let node = MeshNode::new(test_identity("forwarder"), test_config());

        let envelope = MeshEnvelope {
            id: Uuid::new_v4(),
            from: AgentId::new(),
            to: MeshTarget::Agent(AgentId::new()),
            ttl: 3,
            hops: vec![AgentId::new()],
            payload: MeshPayload::Ping(0),
            signature: vec![],
            timestamp: Utc::now(),
        };

        let forwarded = node.prepare_forward(envelope).unwrap();
        assert_eq!(forwarded.ttl, 2);
        assert_eq!(forwarded.hops.len(), 2);
        assert_eq!(*forwarded.hops.last().unwrap(), *node.agent_id());
    }

    #[test]
    fn test_prepare_forward_ttl_one_drops() {
        let node = MeshNode::new(test_identity("drop-test"), test_config());

        let envelope = MeshEnvelope {
            id: Uuid::new_v4(),
            from: AgentId::new(),
            to: MeshTarget::Agent(AgentId::new()),
            ttl: 1,
            hops: vec![],
            payload: MeshPayload::Ping(0),
            signature: vec![],
            timestamp: Utc::now(),
        };

        assert!(node.prepare_forward(envelope).is_none());
    }

    #[test]
    fn test_gossip_generation() {
        let node = MeshNode::new(test_identity("gossiper"), test_config());
        let peer1 = AgentId::new();
        let peer2 = AgentId::new();

        node.add_peer(peer1, "10.0.0.1:9000".parse().unwrap());
        node.add_peer(peer2, "10.0.0.2:9000".parse().unwrap());
        node.mark_connected(&peer1);

        let gossip = node.generate_gossip();
        // Should include ourselves + 1 connected peer (not the discovered-only one)
        assert_eq!(gossip.len(), 2); // us + peer1
        assert_eq!(gossip[0].agent_id, *node.agent_id());
        assert_eq!(gossip[0].hop_count, 0);
    }

    #[test]
    fn test_gossip_processing() {
        let node = MeshNode::new(test_identity("receiver"), test_config());

        let announcements = vec![
            PeerAnnouncement {
                agent_id: AgentId::new(),
                addr: "10.0.0.1:9000".parse().unwrap(),
                capabilities: vec!["scraper".into()],
                hop_count: 1,
            },
            PeerAnnouncement {
                agent_id: AgentId::new(),
                addr: "10.0.0.2:9000".parse().unwrap(),
                capabilities: vec!["analyzer".into()],
                hop_count: 2,
            },
        ];

        node.process_gossip(&announcements);
        assert_eq!(node.peer_count(), 2);
    }

    #[test]
    fn test_gossip_ignores_self() {
        let id = test_identity("self-gossip");
        let agent_id = id.agent_id;
        let node = MeshNode::new(id, test_config());

        let announcements = vec![PeerAnnouncement {
            agent_id,
            addr: "10.0.0.1:9000".parse().unwrap(),
            capabilities: vec![],
            hop_count: 0,
        }];

        node.process_gossip(&announcements);
        assert_eq!(node.peer_count(), 0);
    }

    #[test]
    fn test_gossip_respects_max_peers() {
        let mut config = test_config();
        config.max_peers = 2;
        let node = MeshNode::new(test_identity("limited"), config);

        let announcements: Vec<PeerAnnouncement> = (0..5)
            .map(|i| PeerAnnouncement {
                agent_id: AgentId::new(),
                addr: format!("10.0.0.{}:9000", i).parse().unwrap(),
                capabilities: vec![],
                hop_count: 1,
            })
            .collect();

        node.process_gossip(&announcements);
        assert_eq!(node.peer_count(), 2); // capped at max_peers
    }

    #[test]
    fn test_gossip_ignores_banned() {
        let node = MeshNode::new(test_identity("ban-gossip"), test_config());
        let banned_id = AgentId::new();

        // Add and ban a peer
        node.add_peer(banned_id, "10.0.0.1:9000".parse().unwrap());
        node.ban_peer(&banned_id);

        // Try to re-add via gossip
        let announcements = vec![PeerAnnouncement {
            agent_id: banned_id,
            addr: "10.0.0.99:9000".parse().unwrap(),
            capabilities: vec![],
            hop_count: 1,
        }];
        node.process_gossip(&announcements);

        // Should still be banned
        assert_eq!(node.get_peer(&banned_id).unwrap().state, PeerState::Banned);
    }

    #[test]
    fn test_routing_table() {
        let mut table = RoutingTable::new();
        let target = AgentId::new();
        let hop1 = AgentId::new();
        let hop2 = AgentId::new();

        // Add a 3-hop route
        table.update(target, hop1, 3, 100);
        assert_eq!(table.len(), 1);
        assert_eq!(table.next_hop(&target).unwrap().next_hop, hop1);

        // Better route (2 hops) should replace
        table.update(target, hop2, 2, 50);
        assert_eq!(table.next_hop(&target).unwrap().next_hop, hop2);
        assert_eq!(table.next_hop(&target).unwrap().hop_count, 2);

        // Worse route should not replace
        table.update(target, hop1, 5, 200);
        assert_eq!(table.next_hop(&target).unwrap().next_hop, hop2); // unchanged
    }

    #[test]
    fn test_message_dedup() {
        let mut dedup = MessageDedup::new(3);

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let id4 = Uuid::new_v4();

        assert!(!dedup.check_and_insert(id1));
        assert!(!dedup.check_and_insert(id2));
        assert!(!dedup.check_and_insert(id3));
        assert_eq!(dedup.len(), 3);

        // Duplicate
        assert!(dedup.check_and_insert(id1));

        // Fourth new ID evicts oldest
        assert!(!dedup.check_and_insert(id4));
        assert_eq!(dedup.len(), 3);
    }

    #[tokio::test]
    async fn test_resolve_next_hop_direct() {
        let node = MeshNode::new(test_identity("router"), test_config());
        let peer_id = AgentId::new();

        node.add_peer(peer_id, "10.0.0.1:9000".parse().unwrap());
        node.mark_connected(&peer_id);

        let hop = node.resolve_next_hop(&peer_id).await;
        assert_eq!(hop.unwrap(), peer_id);
    }

    #[tokio::test]
    async fn test_resolve_next_hop_routed() {
        let node = MeshNode::new(test_identity("router"), test_config());
        let target = AgentId::new();
        let relay = AgentId::new();

        {
            let mut routing = node.routing.write().await;
            routing.update(target, relay, 2, 50);
        }

        let hop = node.resolve_next_hop(&target).await;
        assert_eq!(hop.unwrap(), relay);
    }

    #[tokio::test]
    async fn test_broadcast_deliver_and_forward() {
        let node = MeshNode::new(test_identity("broadcast"), test_config());

        let sender = test_identity("broadcaster");
        let sender_node = MeshNode::new(sender, test_config());
        let envelope =
            sender_node.create_envelope(MeshTarget::Broadcast, MeshPayload::Ping(0));

        let action = node.process_envelope(envelope).await.unwrap();
        assert!(matches!(action, MeshAction::DeliverAndForward(_)));
    }

    #[test]
    fn test_mesh_stats_snapshot() {
        let node = MeshNode::new(test_identity("stats"), test_config());

        let snap = node.stats();
        assert_eq!(snap.messages_routed, 0);
        assert_eq!(snap.messages_delivered, 0);
        assert_eq!(snap.messages_dropped, 0);
        assert_eq!(snap.peers_connected, 0);
    }

    #[tokio::test]
    async fn test_routing_from_hops() {
        let id = test_identity("learner");
        let learner_id = id.agent_id;
        let node = MeshNode::new(id, test_config());

        let sender_id = AgentId::new();
        let relay_id = AgentId::new();

        // Envelope that passed through relay_id to get to us
        let envelope = MeshEnvelope {
            id: Uuid::new_v4(),
            from: sender_id,
            to: MeshTarget::Agent(learner_id),
            ttl: 2,
            hops: vec![sender_id, relay_id],
            payload: MeshPayload::Ping(0),
            signature: vec![0u8; 64],
            timestamp: Utc::now(),
        };

        node.process_envelope(envelope).await.unwrap();

        // Node should have learned a route to sender via relay
        let routes = node.routes().await;
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].0, sender_id);
        assert_eq!(routes[0].1.next_hop, relay_id);
    }
}
