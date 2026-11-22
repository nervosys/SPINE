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
//! - **Encrypted transport**: Runs over a stream the caller secures (TLS or the signed envelope); there is no frame-level encryption
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
use tokio::sync::RwLock;
use uuid::Uuid;

/// Domain-separation prefix for mesh envelope signatures.
const ENVELOPE_SIG_DOMAIN: &[u8] = b"spine-mesh-envelope-v3\0";

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
    /// Ed25519 signature by the originator over the bytes produced by
    /// [`MeshNode::signature_data`]: a domain-separation prefix, then the
    /// deterministic CBOR (`spine-cbor`) of one array holding `id`, `from`,
    /// `to`, the timestamp and `payload`. The bytes are a function of the
    /// values, not of any JSON text, so the JSON wire form may be re-formatted
    /// in transit without invalidating the signature.
    ///
    /// `ttl` and `hops` are deliberately NOT covered: forwarders legitimately
    /// rewrite them at every hop, so the originator cannot sign them. They are
    /// advisory routing metadata and must not be trusted for anything else.
    /// `signature` itself is of course not covered either.
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
    /// Announce a signed name record to the mesh, so peers can cache it and
    /// store it if they are near its key. Boxed because a record with links and
    /// metadata is far larger than any other payload, and an un-boxed variant
    /// would inflate every `MeshEnvelope` in memory.
    NameAnnounce(Box<crate::naming::AnnouncedRecord>),
    /// Ask a peer to resolve a name or capability.
    NameResolveRequest(crate::naming::ResolveRequest),
    /// Answer a resolution request: the record, providers, or closer nodes.
    NameResolveResponse(Box<crate::naming::ResolveResponse>),
    /// Introduce this node to a peer reached by address alone, and ask to be
    /// introduced in return. The first message of bootstrap.
    NameHello(crate::naming::NameHello),
    /// Answer a [`MeshPayload::NameHello`]: who the responder is, and the
    /// keyspace peers it can refer the newcomer to.
    NameHelloAck(Box<crate::naming::NameHelloAck>),
    /// Versioned application RPC over the same authenticated mesh transport.
    Application(Box<ApplicationMessage>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationMessage {
    pub public_key: [u8; 32],
    /// Bound inside the signed payload, unlike mutable routing metadata.
    pub recipient: AgentId,
    pub recipient_key: [u8; 32],
    pub request_id: Uuid,
    pub response: bool,
    pub data: serde_json::Value,
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

// ──────────────────────────── Application replay guard ────────────────────────────

/// Largest allowed difference, in seconds, between an application envelope's
/// signed timestamp and the receiver's clock, in either direction.
pub const APPLICATION_FRESHNESS_SECS: i64 = 120;

/// Most (sender, envelope id) pairs [`ReplayGuard`] remembers at once.
pub const REPLAY_GUARD_CAPACITY: usize = 4096;

/// Most live entries one sender key may hold in a [`ReplayGuard`], so that a
/// single key cannot exhaust the shared table. A fraction of
/// [`REPLAY_GUARD_CAPACITY`]; filling the table takes many distinct keys.
pub const REPLAY_GUARD_PER_SENDER: usize = 256;

/// Why [`ReplayGuard::admit`] refused an envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayRefusal {
    /// The signed timestamp is more than [`APPLICATION_FRESHNESS_SECS`] from
    /// the local clock.
    Stale,
    /// This sender and envelope id were already admitted inside the window.
    Duplicate,
    /// The table is full of entries that can still be replayed, or this
    /// sender already holds its share of it.
    Full,
}

/// Bounded, time-windowed record of recently admitted envelopes.
///
/// An envelope is acceptable only while its signed timestamp is within
/// [`APPLICATION_FRESHNESS_SECS`] of the local clock, so an entry stops
/// mattering once `timestamp + window` has passed and can be dropped safely.
/// At most `capacity` entries are held. When full, expired entries are evicted
/// first; if every entry is still replayable the new envelope is refused
/// (`Full`) rather than forgetting an entry that could be replayed.
///
/// Entries are keyed by the sender's *signing key*, which the caller has
/// already verified against the envelope signature, never by the unauthenticated
/// `from` label (anyone can write any label, so keying on it let one peer mint
/// unlimited senders). Each key may hold at most `per_sender` live entries, so
/// one key cannot fill the table and starve the others; live entries are never
/// evicted to make room, since forgetting one would allow it to be replayed.
#[derive(Debug)]
pub struct ReplayGuard {
    /// (sender key, envelope id) -> last second at which a replay would still
    /// pass the freshness check.
    seen: HashMap<([u8; 32], Uuid), i64>,
    /// Live entries per sender key.
    per_key: HashMap<[u8; 32], usize>,
    capacity: usize,
    per_sender: usize,
}

impl ReplayGuard {
    /// Create a guard remembering at most `capacity` envelopes, of which one
    /// sender may hold at most [`REPLAY_GUARD_PER_SENDER`].
    pub fn new(capacity: usize) -> Self {
        Self::with_per_sender(capacity, REPLAY_GUARD_PER_SENDER)
    }

    /// As [`ReplayGuard::new`], with an explicit per-sender bound.
    pub fn with_per_sender(capacity: usize, per_sender: usize) -> Self {
        Self {
            seen: HashMap::new(),
            per_key: HashMap::new(),
            capacity,
            per_sender,
        }
    }

    fn purge(&mut self, now: i64) {
        self.seen.retain(|_, expires| *expires >= now);
        self.per_key.clear();
        for (sender, _) in self.seen.keys() {
            *self.per_key.entry(*sender).or_insert(0) += 1;
        }
    }

    /// Check freshness and uniqueness of an envelope signed at `timestamp`
    /// (Unix seconds) against `now` (Unix seconds), recording it if admitted.
    ///
    /// `sender` is the signing key the envelope was verified under.
    pub fn admit(
        &mut self,
        sender: [u8; 32],
        id: Uuid,
        timestamp: i64,
        now: i64,
    ) -> Result<(), ReplayRefusal> {
        if timestamp.abs_diff(now) > APPLICATION_FRESHNESS_SECS as u64 {
            return Err(ReplayRefusal::Stale);
        }
        let key = (sender, id);
        if self.seen.contains_key(&key) {
            return Err(ReplayRefusal::Duplicate);
        }
        let held = |guard: &Self| guard.per_key.get(&sender).copied().unwrap_or(0);
        if self.seen.len() >= self.capacity || held(self) >= self.per_sender {
            self.purge(now);
            if self.seen.len() >= self.capacity || held(self) >= self.per_sender {
                return Err(ReplayRefusal::Full);
            }
        }
        self.seen
            .insert(key, timestamp.saturating_add(APPLICATION_FRESHNESS_SECS));
        *self.per_key.entry(sender).or_insert(0) += 1;
        Ok(())
    }

    /// Number of envelopes currently remembered.
    pub fn len(&self) -> usize {
        self.seen.len()
    }

    /// Whether nothing is remembered.
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
    /// Whether the node is running.
    running: Arc<AtomicBool>,
    /// Statistics.
    stats: Arc<MeshStats>,
}

impl MeshNode {
    /// Create a new mesh node with the given identity and configuration.
    pub fn new(identity: SigningIdentity, config: MeshConfig) -> Self {
        Self {
            identity: Arc::new(identity),
            config,
            peers: Arc::new(DashMap::new()),
            routing: Arc::new(RwLock::new(RoutingTable::new())),
            dedup: Arc::new(RwLock::new(MessageDedup::new(10_000))),
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(MeshStats::new()),
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

    /// Register a peer address for direct connection.
    pub fn add_peer(&self, agent_id: AgentId, addr: SocketAddr) {
        if agent_id == self.identity.agent_id {
            return; // Don't add ourselves
        }
        if !self.peers.contains_key(&agent_id) {
            self.peers.insert(agent_id, PeerInfo::new(agent_id, addr));
        }
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
        self.peers
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get a snapshot of the routing table.
    pub async fn routes(&self) -> Vec<(AgentId, RouteEntry)> {
        let table = self.routing.read().await;
        table.iter().map(|(k, v)| (*k, v.clone())).collect()
    }

    /// This node's position in the name keyspace.
    ///
    /// It is the node's Ed25519 public key, so a node identifier is
    /// self-certifying in exactly the way a `did:` name is — the same key both
    /// places the node in the DHT and signs everything it says.
    pub fn name_key(&self) -> spine_name::NameKey {
        spine_name::NameKey::from_bytes(*self.identity.public_key())
    }

    /// Describe this node as a keyspace peer, for announcing to others.
    pub fn as_name_node(&self, endpoints: Vec<String>, now: u64) -> spine_name::NodeInfo {
        spine_name::NodeInfo::new(self.name_key(), endpoints, now)
    }

    /// Build an envelope announcing a signed name record to the mesh.
    pub fn announce_name(
        &self,
        record: spine_name::NameRecord,
    ) -> Result<MeshEnvelope, spine_name::NameError> {
        record.verify()?;
        self.create_envelope(
            MeshTarget::Broadcast,
            MeshPayload::NameAnnounce(Box::new(crate::naming::AnnouncedRecord::new(record))),
        )
        .map_err(|e| spine_name::NameError::Transport(e.to_string()))
    }

    /// Build an envelope asking one specific peer to store a signed record.
    ///
    /// The directed form of [`MeshNode::announce_name`], and the one replication
    /// uses. A broadcast reaches whoever this node happens to be connected to,
    /// which has nothing to do with where the record belongs; a record belongs
    /// with the nodes closest to its key, and those are reached by name.
    pub fn announce_name_to(
        &self,
        peer: AgentId,
        record: spine_name::NameRecord,
    ) -> Result<MeshEnvelope, spine_name::NameError> {
        record.verify()?;
        self.create_envelope(
            MeshTarget::Agent(peer),
            MeshPayload::NameAnnounce(Box::new(crate::naming::AnnouncedRecord::new(record))),
        )
        .map_err(|e| spine_name::NameError::Transport(e.to_string()))
    }

    /// Build an envelope asking a specific peer to resolve a query.
    pub fn name_resolve_request(
        &self,
        peer: AgentId,
        request_id: u64,
        query: crate::naming::ResolveQuery,
    ) -> Result<MeshEnvelope> {
        self.create_envelope(
            MeshTarget::Agent(peer),
            MeshPayload::NameResolveRequest(crate::naming::ResolveRequest { request_id, query }),
        )
    }

    /// Build an envelope carrying a resolution answer back to `peer`.
    pub fn name_resolve_response(
        &self,
        peer: AgentId,
        response: crate::naming::ResolveResponse,
    ) -> Result<MeshEnvelope> {
        self.create_envelope(
            MeshTarget::Agent(peer),
            MeshPayload::NameResolveResponse(Box::new(response)),
        )
    }

    /// Build an envelope introducing this node to a peer known only by address.
    ///
    /// Addressed to [`MeshTarget::Broadcast`] because the whole point is that the
    /// recipient's agent id is not yet known — "whoever is listening at this
    /// address". The naming handler consumes it, so it is never gossiped onward.
    pub fn name_hello(&self, endpoints: Vec<String>) -> Result<MeshEnvelope> {
        self.create_envelope(
            MeshTarget::Broadcast,
            MeshPayload::NameHello(crate::naming::NameHello {
                public_key: *self.identity.public_key(),
                endpoints,
            }),
        )
    }

    /// Build an envelope answering a [`MeshPayload::NameHello`].
    pub fn name_hello_ack(
        &self,
        peer: AgentId,
        endpoints: Vec<String>,
        closer: Vec<crate::naming::KeyspacePeer>,
    ) -> Result<MeshEnvelope> {
        self.create_envelope(
            MeshTarget::Agent(peer),
            MeshPayload::NameHelloAck(Box::new(crate::naming::NameHelloAck {
                public_key: *self.identity.public_key(),
                endpoints,
                closer,
            })),
        )
    }

    /// Verify an envelope against a key carried *in the envelope itself*, rather
    /// than one already trusted for its sender.
    ///
    /// Bootstrap has no key on record for the sender yet. Checking against the carried key still proves something worth
    /// having: the sender holds the private half, so it cannot claim a keyspace
    /// position it is unable to sign for.
    pub fn verify_carried_key(envelope: &MeshEnvelope, public_key: &[u8; 32]) -> bool {
        Self::verify_envelope(public_key, envelope)
    }

    /// Check an envelope's signature against `public_key`. Returns false if the
    /// signed bytes cannot be built (encoding failure) or the signature is bad.
    /// Only the v3 form verifies; there is no fallback to earlier encodings.
    fn verify_envelope(public_key: &[u8], envelope: &MeshEnvelope) -> bool {
        match Self::signature_data(
            &envelope.id,
            &envelope.from,
            &envelope.to,
            &envelope.timestamp,
            &envelope.payload,
        ) {
            Ok(sig_data) => {
                crate::identity::Ed25519Keypair::verify(public_key, &sig_data, &envelope.signature)
            }
            Err(_) => false,
        }
    }

    /// Create a signed mesh envelope for a message.
    ///
    /// Fails if the envelope cannot be encoded for signing (see
    /// [`MeshNode::signature_data`]); nothing is signed in that case.
    pub fn create_envelope(&self, to: MeshTarget, payload: MeshPayload) -> Result<MeshEnvelope> {
        let id = Uuid::new_v4();
        let from = self.identity.agent_id;
        let timestamp = Utc::now();

        // Sign (domain || CBOR [id, from, to, timestamp, payload]).
        let sig_data = Self::signature_data(&id, &from, &to, &timestamp, &payload)?;
        let signature = self.identity.sign(&sig_data).to_vec();

        self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);

        Ok(MeshEnvelope {
            id,
            from,
            to,
            ttl: self.config.max_hops,
            hops: vec![self.identity.agent_id],
            payload,
            signature,
            timestamp,
        })
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
        self.peers
            .retain(|_, peer| !peer.is_stale(timeout) || peer.state == PeerState::Banned);

        // Prune routing table
        let mut routing = self.routing.write().await;
        routing.prune_stale(timeout * 2);

        // Prune dedup
        let mut dedup = self.dedup.write().await;
        dedup.prune(300); // 5 minute window
    }

    /// Compute the bytes an envelope signature covers (signature v3):
    ///
    /// `"spine-mesh-envelope-v3\0"` followed by the deterministic CBOR
    /// (`spine_cbor::to_vec`, RFC 8949 section 4.2, no tags, depth at most 32)
    /// of a single six-element array:
    ///
    /// ```text
    /// [ id (bstr, 16), from (bstr, 16), to, timestamp_secs (int),
    ///   timestamp_nanos (uint), payload ]
    /// ```
    ///
    /// `to` and `payload` are whatever serde produces for `MeshTarget` and
    /// `MeshPayload` under ciborium's non-human-readable serializer. The bytes
    /// need only be a pure function of the values, which they are; the shapes
    /// that result are:
    ///
    /// - structs are maps with text keys; a field skipped by
    ///   `skip_serializing_if` has no key; map keys sort by their encodings;
    /// - enums are externally tagged: a unit variant is its name as text
    ///   (`"Broadcast"`, `"PeerQuery"`), any other variant is a one-entry map
    ///   from the variant name (`{"Ping": 7}`, `{"RouteRequest": {"target": ..}}`);
    /// - `Uuid` (and so `AgentId`, a newtype over it) is a 16-byte bstr,
    ///   because it is not human-readable here (JSON has the hyphenated text);
    /// - `[u8; 32]` (public keys, `content_hash`) is an array of 32 unsigned
    ///   integers, not a bstr (serde treats arrays as tuples);
    /// - `SocketAddr` is `{"V4": [[a, b, c, d], port]}` or `{"V6": [[16 ints],
    ///   port]}`, not the text form;
    /// - `NameKey`, the `NameRecord` signature (`sig_bytes`) and `SpineUri` are
    ///   text (base32 or the canonical URI), as in JSON;
    /// - `Option::None` is `null`, `Some(x)` is `x`; `BTreeMap` is a map;
    /// - `serde_json::Value`: null, bool, text, array and map as themselves;
    ///   numbers are an unsigned or negative integer when stored as `u64` or
    ///   `i64`, otherwise a float in the shortest of binary16, binary32 and
    ///   binary64 that preserves it (`-0.0` is `f9 8000`). An integer outside
    ///   `i64`/`u64` is already an `f64` once parsed, and is encoded as one.
    ///   `1` and `1.0` therefore differ, as they do in JSON text.
    ///
    /// No chrono type occurs inside a payload; the envelope timestamp is split
    /// into integers here instead of being left to chrono's string form.
    ///
    /// Errors if the encoding fails (nesting deeper than 32, or a value serde
    /// cannot express); callers must never sign or accept a signature over a
    /// fallback or partial encoding.
    fn signature_data(
        id: &Uuid,
        from: &AgentId,
        to: &MeshTarget,
        timestamp: &DateTime<Utc>,
        payload: &MeshPayload,
    ) -> Result<Vec<u8>> {
        /// A byte string, written as a CBOR bstr whatever the serializer.
        struct Bstr<'a>(&'a [u8]);
        impl Serialize for Bstr<'_> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bytes(self.0)
            }
        }

        let body = (
            Bstr(id.as_bytes()),
            Bstr(from.0.as_bytes()),
            to,
            timestamp.timestamp(),
            timestamp.timestamp_subsec_nanos(),
            payload,
        );
        let cbor = spine_cbor::to_vec(&body)
            .map_err(|e| anyhow::anyhow!("cannot encode envelope for signing: {e}"))?;

        let mut data = Vec::with_capacity(ENVELOPE_SIG_DOMAIN.len() + cbor.len());
        data.extend_from_slice(ENVELOPE_SIG_DOMAIN);
        data.extend_from_slice(&cbor);
        Ok(data)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_replay_guard_admits_once_and_refuses_stale_envelopes() {
        let mut guard = ReplayGuard::new(8);
        let sender = [1u8; 32];
        let id = Uuid::new_v4();
        let now = 1_000_000;

        assert_eq!(guard.admit(sender, id, now, now), Ok(()));
        assert_eq!(
            guard.admit(sender, id, now, now + 1),
            Err(ReplayRefusal::Duplicate)
        );
        // The same id from a different sender is a different envelope.
        assert_eq!(guard.admit([2u8; 32], id, now, now), Ok(()));

        let window = APPLICATION_FRESHNESS_SECS;
        for skew in [window + 1, -(window + 1)] {
            assert_eq!(
                guard.admit(sender, Uuid::new_v4(), now + skew, now),
                Err(ReplayRefusal::Stale)
            );
        }
        for skew in [window, -window] {
            assert_eq!(guard.admit(sender, Uuid::new_v4(), now + skew, now), Ok(()));
        }
    }

    #[test]
    fn a_full_replay_guard_refuses_rather_than_forgetting_live_entries() {
        let mut guard = ReplayGuard::new(2);
        let sender = [1u8; 32];
        let now = 1_000_000;
        assert_eq!(guard.admit(sender, Uuid::new_v4(), now, now), Ok(()));
        assert_eq!(guard.admit(sender, Uuid::new_v4(), now, now), Ok(()));
        assert_eq!(
            guard.admit(sender, Uuid::new_v4(), now, now),
            Err(ReplayRefusal::Full)
        );
        assert_eq!(guard.len(), 2);

        // Once the remembered envelopes can no longer pass the freshness
        // check they are dropped, and there is room again.
        let later = now + APPLICATION_FRESHNESS_SECS + 1;
        assert_eq!(guard.admit(sender, Uuid::new_v4(), later, later), Ok(()));
        assert_eq!(guard.len(), 1);
        assert!(!guard.is_empty());
    }

    #[test]
    fn one_sender_cannot_fill_the_replay_guard_for_the_others() {
        // The table is large, the per-sender share small: a peer that sprays
        // envelopes (under any `from` label it likes, which is not the key)
        // exhausts only its own share.
        let mut guard = ReplayGuard::with_per_sender(64, 4);
        let spammer = [9u8; 32];
        let victim = [7u8; 32];
        let now = 1_000_000;

        let remembered = Uuid::new_v4();
        assert_eq!(guard.admit(victim, remembered, now, now), Ok(()));

        let mut refused = 0;
        for _ in 0..40 {
            if guard.admit(spammer, Uuid::new_v4(), now, now) == Err(ReplayRefusal::Full) {
                refused += 1;
            }
        }
        assert_eq!(refused, 36, "the spammer holds at most its own share");

        // The victim's entry survived and can still not be replayed, and the
        // victim still has room for new envelopes.
        assert_eq!(
            guard.admit(victim, remembered, now, now),
            Err(ReplayRefusal::Duplicate)
        );
        assert_eq!(guard.admit(victim, Uuid::new_v4(), now, now), Ok(()));
        assert_eq!(guard.len(), 6);
    }

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
        assert_eq!(
            node.get_peer(&peer_id).unwrap().state,
            PeerState::Discovered
        );

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

        let envelope = node
            .create_envelope(MeshTarget::Agent(target), MeshPayload::Ping(12345))
            .unwrap();

        assert_eq!(envelope.from, *node.agent_id());
        assert_eq!(envelope.to, MeshTarget::Agent(target));
        assert!(!envelope.signature.is_empty());
        assert_eq!(envelope.ttl, 4); // max_hops from config
        assert_eq!(envelope.hops.len(), 1);
    }

    #[test]
    fn a_tampered_envelope_fails_verification() {
        let sender = test_identity("sig-sender");
        let key = *sender.public_key();
        let sender_node = MeshNode::new(sender, test_config());
        let receiver_id = AgentId::new();

        let envelope = sender_node
            .create_envelope(MeshTarget::Agent(receiver_id), MeshPayload::Ping(77))
            .unwrap();
        assert!(MeshNode::verify_carried_key(&envelope, &key));

        let mut bad = envelope.clone();
        bad.payload = MeshPayload::Ping(99);
        assert!(!MeshNode::verify_carried_key(&bad, &key));

        let other = test_identity("sig-other-key");
        assert!(!MeshNode::verify_carried_key(&envelope, other.public_key()));
    }

    #[test]
    fn envelope_signature_covers_destination_and_timestamp_but_not_hop_fields() {
        let sender = test_identity("sig-cover");
        let node = MeshNode::new(sender, test_config());
        let key = *node.identity.public_key();
        let envelope = node
            .create_envelope(MeshTarget::Agent(AgentId::new()), MeshPayload::Ping(5))
            .unwrap();

        // Unmodified envelope verifies.
        assert!(MeshNode::verify_carried_key(&envelope, &key));

        // Redirecting it invalidates the signature.
        let mut redirected = envelope.clone();
        redirected.to = MeshTarget::Agent(AgentId::new());
        assert!(!MeshNode::verify_carried_key(&redirected, &key));
        let mut broadcast = envelope.clone();
        broadcast.to = MeshTarget::Broadcast;
        assert!(!MeshNode::verify_carried_key(&broadcast, &key));

        // So does changing the timestamp.
        let mut retimed = envelope.clone();
        retimed.timestamp += chrono::Duration::seconds(1);
        assert!(!MeshNode::verify_carried_key(&retimed, &key));

        // ttl and hops are rewritten by forwarders and are not covered.
        let mut forwarded = envelope.clone();
        forwarded.ttl = forwarded.ttl.wrapping_sub(1);
        forwarded.hops.push(AgentId::new());
        assert!(MeshNode::verify_carried_key(&forwarded, &key));
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

    #[test]
    fn test_mesh_stats_snapshot() {
        let node = MeshNode::new(test_identity("stats"), test_config());

        let snap = node.stats();
        assert_eq!(snap.messages_routed, 0);
        assert_eq!(snap.messages_delivered, 0);
        assert_eq!(snap.messages_dropped, 0);
        assert_eq!(snap.peers_connected, 0);
    }

    // ───────────────── Envelope signature v3 (deterministic CBOR) ─────────────────

    fn name_record() -> spine_name::NameRecord {
        let key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
        let name = spine_name::SpineUri::did(key.verifying_key().to_bytes());
        let mut rec = spine_name::NameRecord::new(name, 3, 1_700_000_000)
            .unwrap()
            .with_ttl(3600)
            .with_endpoint(spine_name::Endpoint::new("tcp", "10.0.0.9:9440"))
            .with_capability("search");
        rec.content_hash = Some([7u8; 32]);
        rec.meta.insert("title".into(), "a record".into());
        rec.links.push(
            spine_name::Link::new(spine_name::Rel::Child, spine_name::SpineUri::did([1u8; 32]))
                .with_title("child")
                .with_capability("index"),
        );
        rec.sign(&key).unwrap();
        rec
    }

    fn keyspace_peer() -> crate::naming::KeyspacePeer {
        crate::naming::KeyspacePeer {
            info: spine_name::NodeInfo::new(
                spine_name::NameKey::of(b"peer"),
                vec!["10.0.0.1:9440".into()],
                5,
            ),
            agent_id: AgentId::new(),
        }
    }

    /// One value of every `MeshPayload` variant.
    fn every_payload() -> Vec<MeshPayload> {
        let rec = name_record();
        vec![
            MeshPayload::AgentMessage(AgentMessageCompact {
                id: Uuid::new_v4(),
                from: AgentId::new(),
                to: AgentId::new(),
                content_type: "text/plain".into(),
                content_data: serde_json::json!({"k": [1, -2, 2.5, null, "s", true]}),
                reply_to: Some(Uuid::new_v4()),
                thread_id: None,
            }),
            MeshPayload::PeerAnnounce(vec![
                PeerAnnouncement {
                    agent_id: AgentId::new(),
                    addr: "10.0.0.1:9000".parse().unwrap(),
                    capabilities: vec!["scraper".into()],
                    hop_count: 1,
                },
                PeerAnnouncement {
                    agent_id: AgentId::new(),
                    addr: "[2001:db8::1]:9001".parse().unwrap(),
                    capabilities: vec![],
                    hop_count: 2,
                },
            ]),
            MeshPayload::PeerQuery,
            MeshPayload::Ping(u64::MAX),
            MeshPayload::Pong(0),
            MeshPayload::RouteRequest {
                target: AgentId::new(),
            },
            MeshPayload::RouteResponse {
                target: AgentId::new(),
                route: vec![AgentId::new(), AgentId::new()],
            },
            MeshPayload::KnowledgeSync {
                topic: "topic".into(),
                entries: vec![
                    serde_json::json!({"b": 1, "a": {"c": [0.5]}}),
                    serde_json::json!(null),
                ],
            },
            MeshPayload::SwarmInvite {
                swarm_id: Uuid::new_v4(),
                task_description: "do it".into(),
                required_capabilities: vec!["x".into(), "y".into()],
            },
            MeshPayload::SwarmResponse {
                swarm_id: Uuid::new_v4(),
                accepted: false,
                reason: Some("busy".into()),
            },
            MeshPayload::NameAnnounce(Box::new(crate::naming::AnnouncedRecord::new(rec.clone()))),
            MeshPayload::NameResolveRequest(crate::naming::ResolveRequest {
                request_id: 11,
                query: crate::naming::ResolveQuery::Capability("search".into()),
            }),
            MeshPayload::NameResolveResponse(Box::new(crate::naming::ResolveResponse {
                request_id: 11,
                record: Some(rec.clone()),
                providers: vec![rec],
                closer: vec![keyspace_peer()],
            })),
            MeshPayload::NameHello(crate::naming::NameHello {
                public_key: [3u8; 32],
                endpoints: vec!["10.0.0.3:9440".into()],
            }),
            MeshPayload::NameHelloAck(Box::new(crate::naming::NameHelloAck {
                public_key: [4u8; 32],
                endpoints: vec!["10.0.0.4:9440".into()],
                closer: vec![keyspace_peer()],
            })),
            MeshPayload::Application(Box::new(ApplicationMessage {
                public_key: [5u8; 32],
                recipient: AgentId::new(),
                recipient_key: [6u8; 32],
                request_id: Uuid::new_v4(),
                response: true,
                data: serde_json::json!({"a": [1, 2.5, null, "x"], "b": {"c": true}}),
            })),
        ]
    }

    /// Exhaustive on purpose: adding a `MeshPayload` variant stops this
    /// compiling until `every_payload` is taught about it.
    fn payload_index(p: &MeshPayload) -> usize {
        match p {
            MeshPayload::AgentMessage(_) => 0,
            MeshPayload::PeerAnnounce(_) => 1,
            MeshPayload::PeerQuery => 2,
            MeshPayload::Ping(_) => 3,
            MeshPayload::Pong(_) => 4,
            MeshPayload::RouteRequest { .. } => 5,
            MeshPayload::RouteResponse { .. } => 6,
            MeshPayload::KnowledgeSync { .. } => 7,
            MeshPayload::SwarmInvite { .. } => 8,
            MeshPayload::SwarmResponse { .. } => 9,
            MeshPayload::NameAnnounce(_) => 10,
            MeshPayload::NameResolveRequest(_) => 11,
            MeshPayload::NameResolveResponse(_) => 12,
            MeshPayload::NameHello(_) => 13,
            MeshPayload::NameHelloAck(_) => 14,
            MeshPayload::Application(_) => 15,
        }
    }

    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn every_payload_variant_is_constructed() {
        let mut seen: Vec<usize> = every_payload().iter().map(payload_index).collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..16).collect::<Vec<_>>());
    }

    #[test]
    fn signer_and_verifier_agree_for_every_payload_variant() {
        let node = MeshNode::new(test_identity("v3-all"), test_config());
        let key = *node.identity.public_key();
        for payload in every_payload() {
            let index = payload_index(&payload);
            let envelope = node
                .create_envelope(MeshTarget::Capability("cap".into()), payload)
                .unwrap();
            assert!(
                MeshNode::verify_carried_key(&envelope, &key),
                "variant {index} does not verify"
            );

            // The receiver holds the struct it parsed from the JSON wire, not
            // the sender's value; it must re-encode to the same bytes.
            let wire = serde_json::to_vec(&envelope).unwrap();
            let received: MeshEnvelope = serde_json::from_slice(&wire).unwrap();
            assert!(
                MeshNode::verify_carried_key(&received, &key),
                "variant {index} does not verify after the JSON wire"
            );
        }
    }

    #[test]
    fn every_signed_field_is_covered() {
        let node = MeshNode::new(test_identity("v3-tamper"), test_config());
        let key = *node.identity.public_key();
        let envelope = node
            .create_envelope(MeshTarget::Agent(AgentId::new()), MeshPayload::Ping(5))
            .unwrap();
        assert!(MeshNode::verify_carried_key(&envelope, &key));

        let mut bad = envelope.clone();
        bad.id = Uuid::new_v4();
        assert!(!MeshNode::verify_carried_key(&bad, &key), "id");

        let mut bad = envelope.clone();
        bad.from = AgentId::new();
        assert!(!MeshNode::verify_carried_key(&bad, &key), "from");

        let mut bad = envelope.clone();
        bad.to = MeshTarget::Capability("x".into());
        assert!(!MeshNode::verify_carried_key(&bad, &key), "to");

        let mut bad = envelope.clone();
        bad.timestamp += chrono::Duration::seconds(1);
        assert!(!MeshNode::verify_carried_key(&bad, &key), "timestamp secs");

        let mut bad = envelope.clone();
        bad.timestamp += chrono::Duration::nanoseconds(1);
        assert!(!MeshNode::verify_carried_key(&bad, &key), "timestamp nanos");

        let mut bad = envelope.clone();
        bad.payload = MeshPayload::Ping(6);
        assert!(!MeshNode::verify_carried_key(&bad, &key), "payload value");

        let mut bad = envelope.clone();
        bad.payload = MeshPayload::Pong(5);
        assert!(!MeshNode::verify_carried_key(&bad, &key), "payload variant");

        let mut bad = envelope.clone();
        bad.signature[0] ^= 1;
        assert!(!MeshNode::verify_carried_key(&bad, &key), "signature");

        // ttl and hops are not signed.
        let mut relayed = envelope.clone();
        relayed.ttl = 1;
        relayed.hops.push(AgentId::new());
        relayed.hops.push(AgentId::new());
        assert!(MeshNode::verify_carried_key(&relayed, &key));

        // A different key does not verify.
        let other = test_identity("somebody-else-entirely");
        assert!(!MeshNode::verify_carried_key(&envelope, other.public_key()));
    }

    #[test]
    fn the_signed_bytes_do_not_depend_on_json_text_formatting() {
        let a: serde_json::Value =
            serde_json::from_str(r#"{"b": 1, "a": [2.50, {"y": null, "x": "s"}]}"#).unwrap();
        let b: serde_json::Value =
            serde_json::from_str("{\"a\":[2.5,{\"x\":\"s\",\"y\":null}],\n\"b\":1}").unwrap();
        let id = Uuid::from_bytes([1; 16]);
        let from = AgentId(Uuid::from_bytes([2; 16]));
        let ts = DateTime::from_timestamp(1_700_000_000, 5).unwrap();
        let bytes = |entries: Vec<serde_json::Value>| {
            MeshNode::signature_data(
                &id,
                &from,
                &MeshTarget::Broadcast,
                &ts,
                &MeshPayload::KnowledgeSync {
                    topic: "t".into(),
                    entries,
                },
            )
            .unwrap()
        };
        assert_eq!(bytes(vec![a]), bytes(vec![b]));
    }

    #[test]
    fn awkward_floats_survive_the_json_wire_and_still_verify() {
        let node = MeshNode::new(test_identity("v3-floats"), test_config());
        let key = *node.identity.public_key();
        let sum = 0.1_f64 + 0.2_f64;
        let mut values = vec![
            serde_json::json!(sum),
            serde_json::json!(1e-7),
            serde_json::json!(5e-324),
            serde_json::json!(f64::MAX),
            serde_json::json!(-0.0),
            serde_json::json!(f64::MIN_POSITIVE),
            serde_json::json!(1.0),
            serde_json::json!(u64::MAX),
            serde_json::json!(i64::MIN),
        ];
        // Integers outside i64/u64 parse as floats, on the sender as well as
        // on the receiver.
        values.push(serde_json::from_str("18446744073709551616").unwrap());
        values.push(serde_json::from_str("-9223372036854775809").unwrap());
        values.push(serde_json::from_str("123456789012345678901234567890").unwrap());
        assert!(values[9].is_f64() && values[10].is_f64() && values[11].is_f64());

        let envelope = node
            .create_envelope(
                MeshTarget::Broadcast,
                MeshPayload::KnowledgeSync {
                    topic: "floats".into(),
                    entries: values,
                },
            )
            .unwrap();
        assert!(MeshNode::verify_carried_key(&envelope, &key));

        let wire = serde_json::to_vec(&envelope).unwrap();
        let received: MeshEnvelope = serde_json::from_slice(&wire).unwrap();
        assert!(MeshNode::verify_carried_key(&received, &key));
        // Sender and receiver hold the same bytes.
        let signed = |e: &MeshEnvelope| {
            MeshNode::signature_data(&e.id, &e.from, &e.to, &e.timestamp, &e.payload).unwrap()
        };
        assert_eq!(signed(&envelope), signed(&received));
    }

    #[test]
    fn an_unencodable_envelope_is_neither_signed_nor_verified() {
        let node = MeshNode::new(test_identity("v3-deep"), test_config());
        let key = *node.identity.public_key();

        // Nesting beyond spine_cbor::MAX_DEPTH cannot be encoded.
        let mut deep = serde_json::Value::Null;
        for _ in 0..40 {
            deep = serde_json::Value::Array(vec![deep]);
        }
        let payload = MeshPayload::KnowledgeSync {
            topic: "deep".into(),
            entries: vec![deep],
        };

        assert!(node
            .create_envelope(MeshTarget::Broadcast, payload.clone())
            .is_err());
        assert_eq!(node.stats().messages_sent, 0);

        let forged = MeshEnvelope {
            id: Uuid::new_v4(),
            from: *node.agent_id(),
            to: MeshTarget::Broadcast,
            ttl: 4,
            hops: vec![],
            payload,
            signature: vec![0u8; 64],
            timestamp: Utc::now(),
        };
        assert!(!MeshNode::verify_carried_key(&forged, &key));
    }

    /// The v2 signed bytes, as the previous release built them from JSON text.
    fn v2_signature_data(e: &MeshEnvelope) -> Vec<u8> {
        let to = serde_json::to_vec(&e.to).unwrap();
        let payload = serde_json::to_vec(&e.payload).unwrap();
        let mut d = b"spine-mesh-envelope-v2\0".to_vec();
        d.extend_from_slice(e.id.as_bytes());
        d.extend_from_slice(e.from.0.as_bytes());
        d.extend_from_slice(&(to.len() as u64).to_be_bytes());
        d.extend_from_slice(&to);
        d.extend_from_slice(&e.timestamp.timestamp().to_be_bytes());
        d.extend_from_slice(&e.timestamp.timestamp_subsec_nanos().to_be_bytes());
        d.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        d.extend_from_slice(&payload);
        d
    }

    #[test]
    fn a_v2_signed_envelope_is_refused() {
        let sender = test_identity("v2-sender");
        let sender_node = MeshNode::new(sender.clone(), test_config());

        let mut envelope = sender_node
            .create_envelope(MeshTarget::Agent(AgentId::new()), MeshPayload::Ping(1))
            .unwrap();
        envelope.signature = sender.sign(&v2_signature_data(&envelope)).to_vec();

        assert!(!MeshNode::verify_carried_key(
            &envelope,
            sender.public_key()
        ));
    }

    /// GOLDEN VECTORS. The expected hex below was derived BY HAND from the
    /// CBOR rules (RFC 8949 section 4.2) with a short standard-library Python
    /// script, and has NOT been confirmed by running Rust: the first CI run
    /// confirms or refutes it. Layout, for the first vector:
    ///
    /// ```text
    /// 7370696e652d6d6573682d656e76656c6f70652d763300  "spine-mesh-envelope-v3\0"
    /// 86                                              array(6)
    /// 50 101112131415161718191a1b1c1d1e1f             id, bstr(16)
    /// 50 202122232425262728292a2b2c2d2e2f             from, bstr(16)
    /// 69 42726f616463617374                           to: text(9) "Broadcast"
    /// 1a 6553f100                                     secs 1700000000
    /// 1a 075bcd15                                     nanos 123456789
    /// 69 5065657251756572 79                          payload: text(9) "PeerQuery"
    /// ```
    ///
    /// The second vector has `to` = `{"Agent": bstr(16)}` and payload
    /// `{"Ping": 1000}` (`a1 64 "Ping" 19 03e8`).
    #[test]
    fn golden_signature_data() {
        let id = Uuid::from_bytes(std::array::from_fn(|i| 0x10 + i as u8));
        let from = AgentId(Uuid::from_bytes(std::array::from_fn(|i| 0x20 + i as u8)));
        let ts = DateTime::from_timestamp(1_700_000_000, 123_456_789).unwrap();

        let broadcast = MeshNode::signature_data(
            &id,
            &from,
            &MeshTarget::Broadcast,
            &ts,
            &MeshPayload::PeerQuery,
        )
        .unwrap();
        assert_eq!(
            hex_of(&broadcast),
            "7370696e652d6d6573682d656e76656c6f70652d7633008650101112131415161718191a1b1c1d1e1f\
             50202122232425262728292a2b2c2d2e2f6942726f6164636173741a6553f1001a075bcd1569506565725175657279"
        );

        let direct = MeshNode::signature_data(
            &id,
            &from,
            &MeshTarget::Agent(from),
            &ts,
            &MeshPayload::Ping(1000),
        )
        .unwrap();
        assert_eq!(
            hex_of(&direct),
            "7370696e652d6d6573682d656e76656c6f70652d7633008650101112131415161718191a1b1c1d1e1f\
             50202122232425262728292a2b2c2d2e2fa1654167656e7450202122232425262728292a2b2c2d2e2f\
             1a6553f1001a075bcd15a16450696e671903e8"
        );

        // A fixed seed signs those bytes and its public key verifies them.
        let identity = SigningIdentity::from_seed("golden", [42u8; 32]);
        let sig = identity.sign(&broadcast);
        assert!(crate::identity::Ed25519Keypair::verify(
            identity.public_key(),
            &broadcast,
            &sig
        ));
        assert!(!crate::identity::Ed25519Keypair::verify(
            identity.public_key(),
            &direct,
            &sig
        ));
    }
}
