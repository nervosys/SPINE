//! # Raft Consensus for SPINE Cluster
//!
//! Implementation of the Raft consensus algorithm for distributed state machine
//! replication across SPINE cluster nodes. Based on the Raft paper by Ongaro & Ousterhout.
//!
//! ## Design
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │              RaftNode                       │
//! │  ┌──────────┐  ┌──────────┐  ┌───────────┐ │
//! │  │ Leader   │  │ Log      │  │ State     │ │
//! │  │ Election │  │ Replica- │  │ Machine   │ │
//! │  │          │  │ tion     │  │           │ │
//! │  └────┬─────┘  └────┬─────┘  └─────┬─────┘ │
//! │       └──────────┬───┘──────────────┘       │
//! │            ┌─────┴──────┐                   │
//! │            │ Transport  │                   │
//! │            │  (mpsc)    │                   │
//! │            └────────────┘                   │
//! └──────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

/// Unique node identifier in the Raft cluster.
pub type RaftNodeId = Uuid;

/// Index into the replicated log.
pub type LogIndex = u64;

/// Raft term number (monotonically increasing).
pub type Term = u64;

// ============================================================================
// Raft State
// ============================================================================

/// Raft node role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RaftRole {
    Follower,
    Candidate,
    Leader,
}

/// Persistent state on all servers (survives restarts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentState {
    /// Latest term server has seen.
    pub current_term: Term,
    /// Candidate that received vote in current term (if any).
    pub voted_for: Option<RaftNodeId>,
    /// Log entries; each entry contains command for state machine.
    pub log: Vec<LogEntry>,
}

impl PersistentState {
    pub fn new() -> Self {
        Self {
            current_term: 0,
            voted_for: None,
            log: Vec::new(),
        }
    }
}

impl Default for PersistentState {
    fn default() -> Self {
        Self::new()
    }
}

/// Volatile state on all servers.
#[derive(Debug, Clone)]
pub struct VolatileState {
    /// Index of highest log entry known to be committed.
    pub commit_index: LogIndex,
    /// Index of highest log entry applied to state machine.
    pub last_applied: LogIndex,
}

/// Volatile state on leaders (reinitialized after election).
#[derive(Debug, Clone)]
pub struct LeaderState {
    /// For each server, index of the next log entry to send.
    pub next_index: HashMap<RaftNodeId, LogIndex>,
    /// For each server, index of highest log entry known to be replicated.
    pub match_index: HashMap<RaftNodeId, LogIndex>,
}

/// A single entry in the replicated log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// The term when entry was received by leader.
    pub term: Term,
    /// Index of this entry in the log (1-based).
    pub index: LogIndex,
    /// The command to apply to the state machine.
    pub command: RaftCommand,
}

/// Commands that can be replicated through Raft.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RaftCommand {
    /// Register a node in the cluster.
    RegisterNode {
        node_id: RaftNodeId,
        address: String,
    },
    /// Remove a node from the cluster.
    RemoveNode { node_id: RaftNodeId },
    /// Update cluster configuration.
    UpdateConfig { key: String, value: String },
    /// Store a key-value pair in the distributed state.
    Put { key: String, value: Vec<u8> },
    /// Delete a key from the distributed state.
    Delete { key: String },
    /// Session assignment to a node.
    AssignSession {
        session_id: String,
        node_id: RaftNodeId,
    },
    /// No-op for leader confirmation.
    Noop,
}

// ============================================================================
// RPC Messages
// ============================================================================

/// Request sent by candidates to gather votes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteRequest {
    /// Candidate's term.
    pub term: Term,
    /// Candidate requesting vote.
    pub candidate_id: RaftNodeId,
    /// Index of candidate's last log entry.
    pub last_log_index: LogIndex,
    /// Term of candidate's last log entry.
    pub last_log_term: Term,
}

/// Response to a vote request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestVoteResponse {
    /// Current term, for candidate to update itself.
    pub term: Term,
    /// True means candidate received vote.
    pub vote_granted: bool,
}

/// Sent by leader to replicate log entries and as heartbeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesRequest {
    /// Leader's term.
    pub term: Term,
    /// Leader's ID so followers can redirect clients.
    pub leader_id: RaftNodeId,
    /// Index of log entry immediately preceding new ones.
    pub prev_log_index: LogIndex,
    /// Term of prev_log_index entry.
    pub prev_log_term: Term,
    /// Log entries to store (empty for heartbeat).
    pub entries: Vec<LogEntry>,
    /// Leader's commit index.
    pub leader_commit: LogIndex,
}

/// Response to an append entries request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppendEntriesResponse {
    /// Current term, for leader to update itself.
    pub term: Term,
    /// True if follower contained entry matching prev_log_index and prev_log_term.
    pub success: bool,
    /// Hint for leader to optimize next_index backtracking.
    pub match_index: LogIndex,
}

/// All possible Raft RPC messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RaftMessage {
    RequestVote(RequestVoteRequest),
    RequestVoteResponse(RequestVoteResponse),
    AppendEntries(AppendEntriesRequest),
    AppendEntriesResponse(AppendEntriesResponse),
    /// Client request to submit a command.
    ClientRequest {
        command: RaftCommand,
        /// Channel ID for response routing.
        request_id: Uuid,
    },
}

/// Envelope wrapping a Raft message with source/destination.
#[derive(Debug, Clone)]
pub struct RaftEnvelope {
    pub from: RaftNodeId,
    pub to: RaftNodeId,
    pub message: RaftMessage,
}

// ============================================================================
// State Machine
// ============================================================================

/// Trait for the state machine driven by Raft consensus.
pub trait StateMachine: Send + Sync {
    /// Apply a committed command to the state machine.
    fn apply(&mut self, command: &RaftCommand) -> Result<Vec<u8>, String>;

    /// Take a snapshot of the current state (for log compaction).
    fn snapshot(&self) -> Vec<u8>;

    /// Restore state from a snapshot.
    fn restore(&mut self, snapshot: &[u8]) -> Result<(), String>;
}

/// Default key-value state machine.
#[derive(Debug, Clone, Default)]
pub struct KvStateMachine {
    pub data: HashMap<String, Vec<u8>>,
    pub nodes: HashMap<RaftNodeId, String>,
    pub config: HashMap<String, String>,
    pub sessions: HashMap<String, RaftNodeId>,
}

impl StateMachine for KvStateMachine {
    fn apply(&mut self, command: &RaftCommand) -> Result<Vec<u8>, String> {
        match command {
            RaftCommand::RegisterNode { node_id, address } => {
                self.nodes.insert(*node_id, address.clone());
                Ok(b"ok".to_vec())
            }
            RaftCommand::RemoveNode { node_id } => {
                self.nodes.remove(node_id);
                Ok(b"ok".to_vec())
            }
            RaftCommand::UpdateConfig { key, value } => {
                self.config.insert(key.clone(), value.clone());
                Ok(b"ok".to_vec())
            }
            RaftCommand::Put { key, value } => {
                self.data.insert(key.clone(), value.clone());
                Ok(b"ok".to_vec())
            }
            RaftCommand::Delete { key } => {
                self.data.remove(key);
                Ok(b"ok".to_vec())
            }
            RaftCommand::AssignSession {
                session_id,
                node_id,
            } => {
                self.sessions.insert(session_id.clone(), *node_id);
                Ok(b"ok".to_vec())
            }
            RaftCommand::Noop => Ok(b"ok".to_vec()),
        }
    }

    fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "data": self.data.iter().map(|(k, v)| (k.clone(), base64_encode(v))).collect::<HashMap<_, _>>(),
            "nodes": self.nodes,
            "config": self.config,
            "sessions": self.sessions,
        }))
        .unwrap_or_default()
    }

    fn restore(&mut self, snapshot: &[u8]) -> Result<(), String> {
        let v: serde_json::Value = serde_json::from_slice(snapshot).map_err(|e| e.to_string())?;
        if let Some(nodes) = v.get("nodes") {
            self.nodes = serde_json::from_value(nodes.clone()).unwrap_or_default();
        }
        if let Some(config) = v.get("config") {
            self.config = serde_json::from_value(config.clone()).unwrap_or_default();
        }
        if let Some(sessions) = v.get("sessions") {
            self.sessions = serde_json::from_value(sessions.clone()).unwrap_or_default();
        }
        Ok(())
    }
}

fn base64_encode(data: &[u8]) -> String {
    // Simple hex encoding for snapshot portability
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

// ============================================================================
// Raft Configuration
// ============================================================================

/// Configuration for a Raft node.
#[derive(Debug, Clone)]
pub struct RaftConfig {
    /// Minimum election timeout (ms).
    pub election_timeout_min: u64,
    /// Maximum election timeout (ms).
    pub election_timeout_max: u64,
    /// Heartbeat interval (ms).
    pub heartbeat_interval: u64,
    /// Maximum entries per append request.
    pub max_entries_per_append: usize,
    /// Log compaction threshold (number of entries).
    pub compaction_threshold: usize,
}

impl Default for RaftConfig {
    fn default() -> Self {
        Self {
            election_timeout_min: 150,
            election_timeout_max: 300,
            heartbeat_interval: 50,
            max_entries_per_append: 100,
            compaction_threshold: 10_000,
        }
    }
}

// ============================================================================
// Raft Node
// ============================================================================

/// A single Raft consensus node.
pub struct RaftNode {
    /// This node's unique ID.
    pub id: RaftNodeId,
    /// Current role.
    role: RwLock<RaftRole>,
    /// Persistent state.
    state: RwLock<PersistentState>,
    /// Volatile state.
    volatile: RwLock<VolatileState>,
    /// Leader-only volatile state.
    leader_state: RwLock<Option<LeaderState>>,
    /// Known peers.
    peers: RwLock<Vec<RaftNodeId>>,
    /// State machine.
    state_machine: Mutex<Box<dyn StateMachine>>,
    /// Channel to send messages to peers.
    outbox: mpsc::Sender<RaftEnvelope>,
    /// Configuration.
    config: RaftConfig,
    /// Current leader (if known).
    current_leader: RwLock<Option<RaftNodeId>>,
    /// Election deadline.
    election_deadline: Mutex<Instant>,
    /// Votes received in current election.
    votes_received: RwLock<Vec<RaftNodeId>>,
}

impl RaftNode {
    /// Create a new Raft node.
    pub fn new(
        id: RaftNodeId,
        peers: Vec<RaftNodeId>,
        outbox: mpsc::Sender<RaftEnvelope>,
        config: RaftConfig,
    ) -> Self {
        let election_timeout = Self::random_election_timeout(&config);
        Self {
            id,
            role: RwLock::new(RaftRole::Follower),
            state: RwLock::new(PersistentState::new()),
            volatile: RwLock::new(VolatileState {
                commit_index: 0,
                last_applied: 0,
            }),
            leader_state: RwLock::new(None),
            peers: RwLock::new(peers),
            state_machine: Mutex::new(Box::new(KvStateMachine::default())),
            outbox,
            config,
            current_leader: RwLock::new(None),
            election_deadline: Mutex::new(Instant::now() + election_timeout),
            votes_received: RwLock::new(Vec::new()),
        }
    }

    /// Create a new Raft node with a custom state machine.
    pub fn with_state_machine(
        id: RaftNodeId,
        peers: Vec<RaftNodeId>,
        outbox: mpsc::Sender<RaftEnvelope>,
        config: RaftConfig,
        state_machine: Box<dyn StateMachine>,
    ) -> Self {
        let mut node = Self::new(id, peers, outbox, config);
        node.state_machine = Mutex::new(state_machine);
        node
    }

    /// Get the current role.
    pub async fn role(&self) -> RaftRole {
        *self.role.read().await
    }

    /// Get the current term.
    pub async fn current_term(&self) -> Term {
        self.state.read().await.current_term
    }

    /// Get the current leader.
    pub async fn leader(&self) -> Option<RaftNodeId> {
        *self.current_leader.read().await
    }

    /// Get the commit index.
    pub async fn commit_index(&self) -> LogIndex {
        self.volatile.read().await.commit_index
    }

    /// Get the log length.
    pub async fn log_length(&self) -> usize {
        self.state.read().await.log.len()
    }

    /// Check if election timeout has elapsed.
    pub async fn election_timeout_elapsed(&self) -> bool {
        let deadline = self.election_deadline.lock().await;
        Instant::now() >= *deadline
    }

    /// Reset the election timer.
    async fn reset_election_timer(&self) {
        let timeout = Self::random_election_timeout(&self.config);
        let mut deadline = self.election_deadline.lock().await;
        *deadline = Instant::now() + timeout;
    }

    fn random_election_timeout(config: &RaftConfig) -> Duration {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let ms = rng.gen_range(config.election_timeout_min..=config.election_timeout_max);
        Duration::from_millis(ms)
    }

    // ========================================================================
    // Tick — called periodically to drive the state machine
    // ========================================================================

    /// Main tick function. Call this periodically (e.g., every 10ms).
    pub async fn tick(&self) -> Result<(), String> {
        let role = *self.role.read().await;
        match role {
            RaftRole::Follower | RaftRole::Candidate => {
                if self.election_timeout_elapsed().await {
                    self.start_election().await?;
                }
            }
            RaftRole::Leader => {
                self.send_heartbeats().await?;
            }
        }
        self.apply_committed_entries().await?;
        Ok(())
    }

    // ========================================================================
    // Leader Election
    // ========================================================================

    /// Start a new election.
    async fn start_election(&self) -> Result<(), String> {
        let mut state = self.state.write().await;
        state.current_term += 1;
        state.voted_for = Some(self.id);
        let term = state.current_term;
        let last_log_index = state.log.last().map_or(0, |e| e.index);
        let last_log_term = state.log.last().map_or(0, |e| e.term);
        drop(state);

        *self.role.write().await = RaftRole::Candidate;
        *self.current_leader.write().await = None;
        self.reset_election_timer().await;

        // Vote for self
        let mut votes = self.votes_received.write().await;
        votes.clear();
        votes.push(self.id);
        drop(votes);

        // Send RequestVote to all peers
        let peers = self.peers.read().await.clone();
        for peer in &peers {
            let _ = self
                .outbox
                .send(RaftEnvelope {
                    from: self.id,
                    to: *peer,
                    message: RaftMessage::RequestVote(RequestVoteRequest {
                        term,
                        candidate_id: self.id,
                        last_log_index,
                        last_log_term,
                    }),
                })
                .await;
        }

        // Check if we already won (single-node cluster)
        self.check_election_won().await?;

        Ok(())
    }

    /// Check if we have a majority of votes.
    async fn check_election_won(&self) -> Result<(), String> {
        let votes = self.votes_received.read().await;
        let peers = self.peers.read().await;
        let cluster_size = peers.len() + 1; // +1 for self
        let majority = cluster_size / 2 + 1;

        if votes.len() >= majority && *self.role.read().await == RaftRole::Candidate {
            drop(votes);
            drop(peers);
            self.become_leader().await?;
        }
        Ok(())
    }

    /// Transition to leader role.
    async fn become_leader(&self) -> Result<(), String> {
        *self.role.write().await = RaftRole::Leader;
        *self.current_leader.write().await = Some(self.id);

        let state = self.state.read().await;
        let next_index = state.log.last().map_or(1, |e| e.index + 1);
        drop(state);

        let peers = self.peers.read().await;
        let mut leader_state = LeaderState {
            next_index: HashMap::new(),
            match_index: HashMap::new(),
        };
        for peer in peers.iter() {
            leader_state.next_index.insert(*peer, next_index);
            leader_state.match_index.insert(*peer, 0);
        }
        *self.leader_state.write().await = Some(leader_state);

        // Send initial empty AppendEntries (heartbeat) to assert leadership
        self.send_heartbeats().await?;

        // Commit a no-op to establish commit index for the new term
        self.submit_command(RaftCommand::Noop).await?;

        Ok(())
    }

    // ========================================================================
    // Log Replication
    // ========================================================================

    /// Submit a command for replication (leader only).
    pub async fn submit_command(&self, command: RaftCommand) -> Result<LogIndex, String> {
        if *self.role.read().await != RaftRole::Leader {
            return Err("Not the leader".to_string());
        }

        let mut state = self.state.write().await;
        let index = state.log.last().map_or(1, |e| e.index + 1);
        let term = state.current_term;
        state.log.push(LogEntry {
            term,
            index,
            command,
        });
        drop(state);

        // Replicate to all peers
        self.replicate_to_peers().await?;

        Ok(index)
    }

    /// Send append entries to all peers.
    async fn replicate_to_peers(&self) -> Result<(), String> {
        let peers = self.peers.read().await.clone();
        let leader_state = self.leader_state.read().await;
        let Some(ls) = leader_state.as_ref() else {
            return Ok(());
        };

        let state = self.state.read().await;
        let volatile = self.volatile.read().await;

        for peer in &peers {
            let next_idx = ls.next_index.get(peer).copied().unwrap_or(1);
            let prev_log_index = next_idx.saturating_sub(1);
            let prev_log_term = if prev_log_index > 0 {
                state
                    .log
                    .iter()
                    .find(|e| e.index == prev_log_index)
                    .map_or(0, |e| e.term)
            } else {
                0
            };

            let entries: Vec<LogEntry> = state
                .log
                .iter()
                .filter(|e| e.index >= next_idx)
                .take(self.config.max_entries_per_append)
                .cloned()
                .collect();

            let _ = self
                .outbox
                .send(RaftEnvelope {
                    from: self.id,
                    to: *peer,
                    message: RaftMessage::AppendEntries(AppendEntriesRequest {
                        term: state.current_term,
                        leader_id: self.id,
                        prev_log_index,
                        prev_log_term,
                        entries,
                        leader_commit: volatile.commit_index,
                    }),
                })
                .await;
        }

        Ok(())
    }

    /// Send heartbeats (empty append entries) to all peers.
    async fn send_heartbeats(&self) -> Result<(), String> {
        let peers = self.peers.read().await.clone();
        let state = self.state.read().await;
        let volatile = self.volatile.read().await;

        for peer in &peers {
            let leader_state = self.leader_state.read().await;
            let next_idx = leader_state
                .as_ref()
                .and_then(|ls| ls.next_index.get(peer).copied())
                .unwrap_or(1);
            let prev_log_index = next_idx.saturating_sub(1);
            let prev_log_term = if prev_log_index > 0 {
                state
                    .log
                    .iter()
                    .find(|e| e.index == prev_log_index)
                    .map_or(0, |e| e.term)
            } else {
                0
            };

            let _ = self
                .outbox
                .send(RaftEnvelope {
                    from: self.id,
                    to: *peer,
                    message: RaftMessage::AppendEntries(AppendEntriesRequest {
                        term: state.current_term,
                        leader_id: self.id,
                        prev_log_index,
                        prev_log_term,
                        entries: Vec::new(),
                        leader_commit: volatile.commit_index,
                    }),
                })
                .await;
        }

        Ok(())
    }

    /// Update commit index based on match_index consensus.
    async fn update_commit_index(&self) {
        let state = self.state.read().await;
        let leader_state = self.leader_state.read().await;
        let Some(ls) = leader_state.as_ref() else {
            return;
        };

        let mut volatile = self.volatile.write().await;
        let peers = self.peers.read().await;

        // Find the highest index N such that a majority of match_index[i] >= N
        // and log[N].term == currentTerm
        for entry in state.log.iter().rev() {
            if entry.index <= volatile.commit_index {
                break;
            }
            if entry.term != state.current_term {
                continue;
            }

            let mut replicated = 1; // count self
            for peer in peers.iter() {
                if ls.match_index.get(peer).copied().unwrap_or(0) >= entry.index {
                    replicated += 1;
                }
            }

            let majority = peers.len().div_ceil(2) + 1;
            if replicated >= majority {
                volatile.commit_index = entry.index;
                break;
            }
        }
    }

    /// Apply committed entries to the state machine.
    async fn apply_committed_entries(&self) -> Result<(), String> {
        let volatile = self.volatile.read().await;
        let commit = volatile.commit_index;
        let last_applied = volatile.last_applied;
        drop(volatile);

        if last_applied >= commit {
            return Ok(());
        }

        let state = self.state.read().await;
        let entries_to_apply: Vec<LogEntry> = state
            .log
            .iter()
            .filter(|e| e.index > last_applied && e.index <= commit)
            .cloned()
            .collect();
        drop(state);

        let mut sm = self.state_machine.lock().await;
        for entry in &entries_to_apply {
            let _ = sm.apply(&entry.command);
        }
        drop(sm);

        let mut volatile = self.volatile.write().await;
        volatile.last_applied = commit;

        Ok(())
    }

    // ========================================================================
    // Message Handling
    // ========================================================================

    /// Handle an incoming Raft message.
    pub async fn handle_message(&self, envelope: RaftEnvelope) -> Result<(), String> {
        match envelope.message {
            RaftMessage::RequestVote(req) => self.handle_request_vote(envelope.from, req).await,
            RaftMessage::RequestVoteResponse(resp) => {
                self.handle_request_vote_response(envelope.from, resp).await
            }
            RaftMessage::AppendEntries(req) => self.handle_append_entries(envelope.from, req).await,
            RaftMessage::AppendEntriesResponse(resp) => {
                self.handle_append_entries_response(envelope.from, resp)
                    .await
            }
            RaftMessage::ClientRequest { command, .. } => {
                self.submit_command(command).await?;
                Ok(())
            }
        }
    }

    /// Handle RequestVote RPC.
    async fn handle_request_vote(
        &self,
        from: RaftNodeId,
        req: RequestVoteRequest,
    ) -> Result<(), String> {
        let mut state = self.state.write().await;

        // If term > currentTerm, step down
        if req.term > state.current_term {
            state.current_term = req.term;
            state.voted_for = None;
            drop(state);
            *self.role.write().await = RaftRole::Follower;
            state = self.state.write().await;
        }

        let mut vote_granted = false;

        if req.term >= state.current_term {
            let can_vote = state.voted_for.is_none() || state.voted_for == Some(req.candidate_id);
            let last_log_index = state.log.last().map_or(0, |e| e.index);
            let last_log_term = state.log.last().map_or(0, |e| e.term);

            // Candidate's log must be at least as up-to-date
            let log_ok = req.last_log_term > last_log_term
                || (req.last_log_term == last_log_term && req.last_log_index >= last_log_index);

            if can_vote && log_ok {
                state.voted_for = Some(req.candidate_id);
                vote_granted = true;
                drop(state);
                self.reset_election_timer().await;
            } else {
                drop(state);
            }
        } else {
            drop(state);
        }

        let term = self.state.read().await.current_term;
        let _ = self
            .outbox
            .send(RaftEnvelope {
                from: self.id,
                to: from,
                message: RaftMessage::RequestVoteResponse(RequestVoteResponse {
                    term,
                    vote_granted,
                }),
            })
            .await;

        Ok(())
    }

    /// Handle RequestVote response.
    async fn handle_request_vote_response(
        &self,
        from: RaftNodeId,
        resp: RequestVoteResponse,
    ) -> Result<(), String> {
        if *self.role.read().await != RaftRole::Candidate {
            return Ok(());
        }

        let state = self.state.read().await;
        if resp.term > state.current_term {
            drop(state);
            self.step_down(resp.term).await;
            return Ok(());
        }
        drop(state);

        if resp.vote_granted {
            let mut votes = self.votes_received.write().await;
            if !votes.contains(&from) {
                votes.push(from);
            }
            drop(votes);
            self.check_election_won().await?;
        }

        Ok(())
    }

    /// Handle AppendEntries RPC.
    async fn handle_append_entries(
        &self,
        from: RaftNodeId,
        req: AppendEntriesRequest,
    ) -> Result<(), String> {
        let mut state = self.state.write().await;

        // If term > currentTerm, update
        if req.term > state.current_term {
            state.current_term = req.term;
            state.voted_for = None;
        }

        if req.term < state.current_term {
            let term = state.current_term;
            drop(state);
            let _ = self
                .outbox
                .send(RaftEnvelope {
                    from: self.id,
                    to: from,
                    message: RaftMessage::AppendEntriesResponse(AppendEntriesResponse {
                        term,
                        success: false,
                        match_index: 0,
                    }),
                })
                .await;
            return Ok(());
        }

        // Valid leader — step down if candidate, reset timer
        drop(state);
        *self.role.write().await = RaftRole::Follower;
        *self.current_leader.write().await = Some(req.leader_id);
        self.reset_election_timer().await;

        let mut state = self.state.write().await;

        // Check if log contains an entry at prev_log_index with prev_log_term
        let log_ok = if req.prev_log_index == 0 {
            true
        } else {
            state
                .log
                .iter()
                .any(|e| e.index == req.prev_log_index && e.term == req.prev_log_term)
        };

        if !log_ok {
            let term = state.current_term;
            drop(state);
            let _ = self
                .outbox
                .send(RaftEnvelope {
                    from: self.id,
                    to: from,
                    message: RaftMessage::AppendEntriesResponse(AppendEntriesResponse {
                        term,
                        success: false,
                        match_index: 0,
                    }),
                })
                .await;
            return Ok(());
        }

        // Append new entries (removing conflicting ones)
        for entry in &req.entries {
            if let Some(existing) = state.log.iter().find(|e| e.index == entry.index) {
                if existing.term != entry.term {
                    // Delete existing entry and all following
                    state.log.retain(|e| e.index < entry.index);
                }
            }
            if !state.log.iter().any(|e| e.index == entry.index) {
                state.log.push(entry.clone());
            }
        }

        // Update commit index
        let mut volatile = self.volatile.write().await;
        if req.leader_commit > volatile.commit_index {
            let last_new_index = state.log.last().map_or(0, |e| e.index);
            volatile.commit_index = req.leader_commit.min(last_new_index);
        }

        let match_index = state.log.last().map_or(0, |e| e.index);
        let term = state.current_term;
        drop(state);
        drop(volatile);

        let _ = self
            .outbox
            .send(RaftEnvelope {
                from: self.id,
                to: from,
                message: RaftMessage::AppendEntriesResponse(AppendEntriesResponse {
                    term,
                    success: true,
                    match_index,
                }),
            })
            .await;

        Ok(())
    }

    /// Handle AppendEntries response (leader only).
    async fn handle_append_entries_response(
        &self,
        from: RaftNodeId,
        resp: AppendEntriesResponse,
    ) -> Result<(), String> {
        if *self.role.read().await != RaftRole::Leader {
            return Ok(());
        }

        let state = self.state.read().await;
        if resp.term > state.current_term {
            drop(state);
            self.step_down(resp.term).await;
            return Ok(());
        }
        drop(state);

        let mut leader_state = self.leader_state.write().await;
        if let Some(ls) = leader_state.as_mut() {
            if resp.success {
                ls.match_index.insert(from, resp.match_index);
                ls.next_index.insert(from, resp.match_index + 1);
            } else {
                // Decrement next_index and retry
                let current = ls.next_index.get(&from).copied().unwrap_or(1);
                ls.next_index.insert(from, current.saturating_sub(1).max(1));
            }
        }
        drop(leader_state);

        self.update_commit_index().await;

        Ok(())
    }

    /// Step down to follower for a newer term.
    async fn step_down(&self, new_term: Term) {
        let mut state = self.state.write().await;
        state.current_term = new_term;
        state.voted_for = None;
        drop(state);
        *self.role.write().await = RaftRole::Follower;
        *self.leader_state.write().await = None;
        self.reset_election_timer().await;
    }

    /// Get a snapshot of the Raft node's status.
    pub async fn status(&self) -> RaftStatus {
        let state = self.state.read().await;
        let volatile = self.volatile.read().await;
        let role = *self.role.read().await;
        let leader = *self.current_leader.read().await;
        let peers = self.peers.read().await.len();

        RaftStatus {
            id: self.id,
            role,
            term: state.current_term,
            log_length: state.log.len(),
            commit_index: volatile.commit_index,
            last_applied: volatile.last_applied,
            leader_id: leader,
            peer_count: peers,
        }
    }
}

/// Snapshot of Raft node status for monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftStatus {
    pub id: RaftNodeId,
    pub role: RaftRole,
    pub term: Term,
    pub log_length: usize,
    pub commit_index: LogIndex,
    pub last_applied: LogIndex,
    pub leader_id: Option<RaftNodeId>,
    pub peer_count: usize,
}

// ============================================================================
// In-Process Raft Cluster (for testing)
// ============================================================================

/// In-process Raft cluster for testing and single-machine deployments.
pub struct RaftCluster {
    nodes: Vec<Arc<RaftNode>>,
    router_tx: mpsc::Sender<RaftEnvelope>,
    router_handle: Option<tokio::task::JoinHandle<()>>,
}

impl RaftCluster {
    /// Create a new in-process Raft cluster with n nodes.
    pub async fn new(node_count: usize) -> Self {
        let ids: Vec<RaftNodeId> = (0..node_count).map(|_| Uuid::new_v4()).collect();
        let (router_tx, mut router_rx) = mpsc::channel::<RaftEnvelope>(10_000);
        let config = RaftConfig::default();

        let mut nodes = Vec::new();
        let node_map: Arc<RwLock<HashMap<RaftNodeId, Arc<RaftNode>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        for (i, id) in ids.iter().enumerate() {
            let peers: Vec<RaftNodeId> = ids
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, id)| *id)
                .collect();
            let node = Arc::new(RaftNode::new(*id, peers, router_tx.clone(), config.clone()));
            nodes.push(node.clone());
            node_map.write().await.insert(*id, node);
        }

        // Router task: forward messages to the correct node
        let map_clone = node_map.clone();
        let router_handle = tokio::spawn(async move {
            while let Some(envelope) = router_rx.recv().await {
                let map = map_clone.read().await;
                if let Some(node) = map.get(&envelope.to) {
                    let node = node.clone();
                    tokio::spawn(async move {
                        let _ = node.handle_message(envelope).await;
                    });
                }
            }
        });

        Self {
            nodes,
            router_tx,
            router_handle: Some(router_handle),
        }
    }

    /// Get all nodes.
    pub fn nodes(&self) -> &[Arc<RaftNode>] {
        &self.nodes
    }

    /// Find the current leader (if any).
    pub async fn leader(&self) -> Option<Arc<RaftNode>> {
        for node in &self.nodes {
            if node.role().await == RaftRole::Leader {
                return Some(node.clone());
            }
        }
        None
    }

    /// Tick all nodes (call in a loop for simulation).
    pub async fn tick_all(&self) {
        for node in &self.nodes {
            let _ = node.tick().await;
        }
    }

    /// Wait for a leader to be elected.
    pub async fn wait_for_leader(&self, timeout: Duration) -> Option<Arc<RaftNode>> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            self.tick_all().await;
            if let Some(leader) = self.leader().await {
                return Some(leader);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        None
    }
}

impl Drop for RaftCluster {
    fn drop(&mut self) {
        if let Some(handle) = self.router_handle.take() {
            handle.abort();
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persistent_state_default() {
        let state = PersistentState::new();
        assert_eq!(state.current_term, 0);
        assert!(state.voted_for.is_none());
        assert!(state.log.is_empty());
    }

    #[test]
    fn test_kv_state_machine() {
        let mut sm = KvStateMachine::default();

        let result = sm.apply(&RaftCommand::Put {
            key: "hello".to_string(),
            value: b"world".to_vec(),
        });
        assert!(result.is_ok());
        assert_eq!(sm.data.get("hello").unwrap(), b"world");

        let result = sm.apply(&RaftCommand::Delete {
            key: "hello".to_string(),
        });
        assert!(result.is_ok());
        assert!(!sm.data.contains_key("hello"));
    }

    #[test]
    fn test_kv_state_machine_nodes() {
        let mut sm = KvStateMachine::default();
        let id = Uuid::new_v4();

        sm.apply(&RaftCommand::RegisterNode {
            node_id: id,
            address: "127.0.0.1:8080".to_string(),
        })
        .unwrap();
        assert_eq!(sm.nodes.get(&id).unwrap(), "127.0.0.1:8080");

        sm.apply(&RaftCommand::RemoveNode { node_id: id }).unwrap();
        assert!(!sm.nodes.contains_key(&id));
    }

    #[test]
    fn test_kv_state_machine_snapshot() {
        let mut sm = KvStateMachine::default();
        sm.apply(&RaftCommand::Put {
            key: "k1".to_string(),
            value: b"v1".to_vec(),
        })
        .unwrap();
        sm.apply(&RaftCommand::UpdateConfig {
            key: "max_nodes".to_string(),
            value: "10".to_string(),
        })
        .unwrap();

        let snap = sm.snapshot();
        assert!(!snap.is_empty());

        let mut sm2 = KvStateMachine::default();
        sm2.restore(&snap).unwrap();
        assert_eq!(sm2.config.get("max_nodes").unwrap(), "10");
    }

    #[test]
    fn test_raft_config_default() {
        let config = RaftConfig::default();
        assert!(config.election_timeout_min < config.election_timeout_max);
        assert!(config.heartbeat_interval < config.election_timeout_min);
    }

    #[tokio::test]
    async fn test_raft_node_initial_state() {
        let (tx, _rx) = mpsc::channel(100);
        let id = Uuid::new_v4();
        let node = RaftNode::new(id, vec![], tx, RaftConfig::default());

        assert_eq!(node.role().await, RaftRole::Follower);
        assert_eq!(node.current_term().await, 0);
        assert!(node.leader().await.is_none());
        assert_eq!(node.log_length().await, 0);
    }

    #[tokio::test]
    async fn test_raft_single_node_election() {
        let (tx, _rx) = mpsc::channel(100);
        let id = Uuid::new_v4();
        let node = RaftNode::new(
            id,
            vec![],
            tx,
            RaftConfig {
                election_timeout_min: 1,
                election_timeout_max: 2,
                ..Default::default()
            },
        );

        // Wait for election timeout
        tokio::time::sleep(Duration::from_millis(10)).await;
        node.tick().await.unwrap();

        // Single node should elect itself immediately
        assert_eq!(node.role().await, RaftRole::Leader);
        assert_eq!(node.leader().await, Some(id));
        assert_eq!(node.current_term().await, 1);
    }

    #[tokio::test]
    async fn test_raft_submit_command() {
        let (tx, _rx) = mpsc::channel(100);
        let id = Uuid::new_v4();
        let node = RaftNode::new(
            id,
            vec![],
            tx,
            RaftConfig {
                election_timeout_min: 1,
                election_timeout_max: 2,
                ..Default::default()
            },
        );

        tokio::time::sleep(Duration::from_millis(10)).await;
        node.tick().await.unwrap();
        assert_eq!(node.role().await, RaftRole::Leader);

        let _index = node
            .submit_command(RaftCommand::Put {
                key: "test".to_string(),
                value: b"value".to_vec(),
            })
            .await
            .unwrap();

        // Log should have noop + put
        assert!(node.log_length().await >= 2);
    }

    #[tokio::test]
    async fn test_raft_cluster_election() {
        let cluster = RaftCluster::new(3).await;
        let leader = cluster.wait_for_leader(Duration::from_secs(5)).await;
        assert!(leader.is_some(), "Should elect a leader within 5s");

        let leader = leader.unwrap();
        assert_eq!(leader.role().await, RaftRole::Leader);

        // Verify only one leader
        let mut leader_count = 0;
        for node in cluster.nodes() {
            if node.role().await == RaftRole::Leader {
                leader_count += 1;
            }
        }
        assert_eq!(leader_count, 1);
    }

    #[tokio::test]
    async fn test_raft_log_replication() {
        let cluster = RaftCluster::new(3).await;
        let leader = cluster
            .wait_for_leader(Duration::from_secs(5))
            .await
            .unwrap();

        // Submit a command
        leader
            .submit_command(RaftCommand::Put {
                key: "replicated".to_string(),
                value: b"data".to_vec(),
            })
            .await
            .unwrap();

        // Tick a few times for replication
        for _ in 0..50 {
            cluster.tick_all().await;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // All nodes should have the log entry
        for node in cluster.nodes() {
            assert!(
                node.log_length().await >= 1,
                "Node {} should have log entries",
                node.id
            );
        }
    }

    #[tokio::test]
    async fn test_raft_status() {
        let (tx, _rx) = mpsc::channel(100);
        let id = Uuid::new_v4();
        let node = RaftNode::new(id, vec![Uuid::new_v4()], tx, RaftConfig::default());

        let status = node.status().await;
        assert_eq!(status.id, id);
        assert_eq!(status.role, RaftRole::Follower);
        assert_eq!(status.term, 0);
        assert_eq!(status.log_length, 0);
        assert_eq!(status.peer_count, 1);
    }
}
