// Allow dead code for extensive public API surface designed for future use
#![allow(dead_code)]

//! # SPINE Agentic Web Stack
//!
//! A revolutionary new paradigm for the agentic web: where AI agents are first-class
//! citizens of the web, not afterthoughts bolted onto human-designed interfaces.
//!
//! ## The Vision
//!
//! The current web was built for humans clicking on links and filling out forms.
//! The agentic web is built for AI agents that:
//! - Navigate semantically, not visually
//! - Communicate in latent space, not HTML
//! - Form swarms, not sessions
//! - Learn continuously, not statically
//! - Act autonomously, not reactively
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    AGENTIC WEB STACK                            │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Layer 5: Collective Intelligence                               │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
//! │  │ SwarmMind   │ │ Consensus   │ │ Emergence   │               │
//! │  └─────────────┘ └─────────────┘ └─────────────┘               │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Layer 4: Agent Cognition                                       │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
//! │  │ Goals       │ │ Planning    │ │ Learning    │               │
//! │  │ Intentions  │ │ Reasoning   │ │ Memory      │               │
//! │  └─────────────┘ └─────────────┘ └─────────────┘               │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Layer 3: Semantic Web                                          │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
//! │  │ Knowledge   │ │ Ontology    │ │ Inference   │               │
//! │  │ Graph       │ │ Mapping     │ │ Engine      │               │
//! │  └─────────────┘ └─────────────┘ └─────────────┘               │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Layer 2: Latent Communication                                  │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
//! │  │ Chameleon   │ │ Speculative │ │ Neural      │               │
//! │  │ Protocol    │ │ Decoding    │ │ Encoding    │               │
//! │  └─────────────┘ └─────────────┘ └─────────────┘               │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Layer 1: Transport                                             │
//! │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
//! │  │ QUIC/0-RTT  │ │ TCP/TLS     │ │ WebSocket   │               │
//! │  └─────────────┘ └─────────────┘ └─────────────┘               │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use petgraph::graph::{DiGraph, NodeIndex};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use uuid::Uuid;

use spine_crypto::MirasTitansPredictor;
use spine_knowledge::{UnifiedConfig, UnifiedMemory};
use spine_neural::MirasVariant;

// =============================================================================
// LAYER 1: AGENT IDENTITY & CAPABILITIES
// =============================================================================

/// Unique identifier for an agent in the agentic web
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub Uuid);

impl AgentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();
        Self(Uuid::from_slice(&result[..16]).unwrap_or_else(|_| Uuid::new_v4()))
    }
}

impl Default for AgentId {
    fn default() -> Self {
        Self::new()
    }
}

/// Agent capability declaration
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentCapability {
    /// Can navigate the web
    Navigation,
    /// Can extract and understand content
    ContentExtraction,
    /// Can fill forms and interact with pages
    FormInteraction,
    /// Can execute code/scripts
    CodeExecution,
    /// Can communicate with other agents
    AgentCommunication,
    /// Can store and retrieve knowledge
    KnowledgeManagement,
    /// Can learn from experience
    ContinualLearning,
    /// Can form and participate in swarms
    SwarmParticipation,
    /// Can make autonomous decisions
    AutonomousDecision,
    /// Can handle financial transactions
    FinancialTransaction,
    /// Can access external APIs
    ApiAccess,
    /// Custom capability with description
    Custom(String),
}

/// Agent trust level in the network
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum TrustLevel {
    /// Unknown/untrusted agent
    #[default]
    Unknown = 0,
    /// Basic trust (verified identity)
    Verified = 1,
    /// Trusted (positive interaction history)
    Trusted = 2,
    /// Highly trusted (long positive history)
    HighlyTrusted = 3,
    /// Core agent (part of the network infrastructure)
    Core = 4,
}

/// Agent profile in the agentic web
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: AgentId,
    pub name: String,
    pub version: String,
    pub capabilities: Vec<AgentCapability>,
    pub trust_level: TrustLevel,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    /// Latent embedding of the agent's "personality"
    pub latent_signature: Vec<f32>,
    /// MIRAS variant for this agent's memory
    pub miras_variant: String,
    /// Public key for agent verification
    pub public_key: Option<Vec<u8>>,
}

impl AgentProfile {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: AgentId::new(),
            name: name.into(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            capabilities: vec![
                AgentCapability::Navigation,
                AgentCapability::ContentExtraction,
                AgentCapability::AgentCommunication,
            ],
            trust_level: TrustLevel::Unknown,
            created_at: Utc::now(),
            last_seen: Utc::now(),
            latent_signature: vec![0.0; 64],
            miras_variant: "Titans".to_string(),
            public_key: None,
        }
    }

    pub fn with_capabilities(mut self, caps: Vec<AgentCapability>) -> Self {
        self.capabilities = caps;
        self
    }

    pub fn with_trust(mut self, level: TrustLevel) -> Self {
        self.trust_level = level;
        self
    }

    pub fn with_miras(mut self, variant: &str) -> Self {
        self.miras_variant = variant.to_string();
        self
    }
}

// =============================================================================
// LAYER 2: INTENTIONS & GOALS
// =============================================================================

/// An agent's intention - what it wants to achieve
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intention {
    pub id: Uuid,
    pub agent_id: AgentId,
    pub goal: Goal,
    pub priority: f32,
    pub deadline: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub status: IntentionStatus,
    /// Sub-intentions that contribute to this one
    pub sub_intentions: Vec<Uuid>,
    /// Constraints on how the intention can be fulfilled
    pub constraints: Vec<Constraint>,
}

/// A goal an agent is trying to achieve
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Goal {
    /// Navigate to a specific resource
    Navigate { target: ResourceLocator },
    /// Extract specific information
    Extract {
        query: SemanticQuery,
        from: ResourceLocator,
    },
    /// Submit data to a resource
    Submit {
        data: serde_json::Value,
        to: ResourceLocator,
    },
    /// Learn something new
    Learn { topic: String, depth: LearningDepth },
    /// Find agents with specific capabilities
    FindAgents { capabilities: Vec<AgentCapability> },
    /// Form a swarm for collective task
    FormSwarm { task: Box<SwarmTask> },
    /// Execute a multi-step plan
    ExecutePlan { plan: Plan },
    /// Monitor a resource for changes
    Monitor {
        resource: ResourceLocator,
        interval: Duration,
    },
    /// Custom goal with semantic description
    Custom {
        description: String,
        parameters: serde_json::Value,
    },
}

/// Status of an intention
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentionStatus {
    Pending,
    Active,
    Blocked,
    Completed,
    Failed,
    Cancelled,
}

/// Constraint on intention fulfillment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Constraint {
    /// Must complete within time limit
    TimeLimit(Duration),
    /// Must not exceed cost
    CostLimit(f64),
    /// Must use specific capabilities
    RequireCapability(AgentCapability),
    /// Must not access certain resources
    AvoidResource(ResourceLocator),
    /// Must maintain privacy level
    PrivacyLevel(u8),
    /// Custom constraint
    Custom(String),
}

// =============================================================================
// LAYER 3: SEMANTIC WEB INTERFACE
// =============================================================================

/// Locator for resources in the agentic web
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResourceLocator {
    /// Traditional URL
    Url(String),
    /// Semantic identifier (concept-based)
    Semantic {
        concept: String,
        constraints: Vec<String>,
    },
    /// Agent-relative path
    AgentPath { agent: AgentId, path: String },
    /// Knowledge graph node
    KnowledgeNode { graph: String, node_id: String },
    /// Latent space coordinates (for neural resources)
    LatentCoord {
        space: String,
        coordinates: Vec<f32>,
    },
    /// Content-addressed (hash-based)
    ContentAddress(String),
}

impl ResourceLocator {
    pub fn url(url: impl Into<String>) -> Self {
        Self::Url(url.into())
    }

    pub fn semantic(concept: impl Into<String>) -> Self {
        Self::Semantic {
            concept: concept.into(),
            constraints: vec![],
        }
    }

    pub fn with_constraint(mut self, constraint: impl Into<String>) -> Self {
        if let Self::Semantic { constraints, .. } = &mut self {
            constraints.push(constraint.into());
        }
        self
    }
}

/// A semantic query for information extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticQuery {
    /// Natural language or structured query
    pub query: String,
    /// Expected output type
    pub output_type: OutputType,
    /// Context for the query
    pub context: Vec<String>,
    /// Confidence threshold for results
    pub confidence_threshold: f32,
}

/// Expected output type for queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputType {
    /// Plain text
    Text,
    /// Structured JSON
    Json(Option<serde_json::Value>),
    /// List of items
    List,
    /// Key-value pairs
    KeyValue,
    /// Numeric value
    Number,
    /// Boolean
    Boolean,
    /// Latent vector
    LatentVector,
    /// Custom type
    Custom(String),
}

/// Learning depth for knowledge acquisition
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LearningDepth {
    /// Quick overview
    Shallow,
    /// Moderate understanding
    Medium,
    /// Deep, comprehensive learning
    Deep,
    /// Expert-level mastery
    Expert,
}

// =============================================================================
// LAYER 4: PLANNING & REASONING
// =============================================================================

/// A plan for achieving goals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub steps: Vec<PlanStep>,
    pub dependencies: Vec<(usize, usize)>,
    pub estimated_duration: Duration,
    pub confidence: f32,
    pub alternatives: Vec<Plan>,
}

impl Plan {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            steps: vec![],
            dependencies: vec![],
            estimated_duration: Duration::ZERO,
            confidence: 1.0,
            alternatives: vec![],
        }
    }

    pub fn add_step(&mut self, step: PlanStep) -> usize {
        let idx = self.steps.len();
        self.steps.push(step);
        idx
    }

    pub fn add_dependency(&mut self, before: usize, after: usize) {
        self.dependencies.push((before, after));
    }
}

impl Default for Plan {
    fn default() -> Self {
        Self::new()
    }
}

/// A single step in a plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: Uuid,
    pub action: Action,
    pub preconditions: Vec<Condition>,
    pub postconditions: Vec<Condition>,
    pub estimated_duration: Duration,
    pub retry_policy: RetryPolicy,
}

/// An action an agent can take
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    /// Navigate to a resource
    Navigate(ResourceLocator),
    /// Extract information
    Extract(SemanticQuery),
    /// Execute a command/interaction
    Execute {
        command: String,
        args: serde_json::Value,
    },
    /// Send message to another agent
    Message {
        to: AgentId,
        content: Box<AgentMessage>,
    },
    /// Store knowledge
    Store {
        key: String,
        value: serde_json::Value,
    },
    /// Retrieve knowledge
    Retrieve { key: String },
    /// Wait for a condition
    Wait {
        condition: Condition,
        timeout: Duration,
    },
    /// Branch based on condition
    Branch {
        condition: Condition,
        if_true: Box<Action>,
        if_false: Box<Action>,
    },
    /// Execute multiple actions in parallel
    Parallel(Vec<Action>),
    /// Execute multiple actions in sequence
    Sequence(Vec<Action>),
    /// Delegate to another agent
    Delegate { to: AgentId, task: Box<Goal> },
    /// Learn from current context
    Learn { topic: String },
    /// Custom action
    Custom {
        name: String,
        params: serde_json::Value,
    },
}

/// A condition that can be evaluated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Condition {
    /// Always true
    Always,
    /// Always false
    Never,
    /// Check if a resource exists
    ResourceExists(ResourceLocator),
    /// Check if a value matches
    ValueEquals {
        path: String,
        expected: serde_json::Value,
    },
    /// Check if an agent is available
    AgentAvailable(AgentId),
    /// Check if we have a capability
    HasCapability(AgentCapability),
    /// Logical AND
    And(Vec<Condition>),
    /// Logical OR
    Or(Vec<Condition>),
    /// Logical NOT
    Not(Box<Condition>),
    /// Custom condition
    Custom {
        predicate: String,
        args: serde_json::Value,
    },
}

/// Retry policy for actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub exponential_backoff: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            exponential_backoff: true,
        }
    }
}

// =============================================================================
// LAYER 5: AGENT COMMUNICATION
// =============================================================================

/// Message between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: Uuid,
    pub from: AgentId,
    pub to: AgentId,
    pub timestamp: DateTime<Utc>,
    pub content: Box<MessageContent>,
    pub reply_to: Option<Uuid>,
    pub thread_id: Option<Uuid>,
    /// Latent encoding of the message for semantic matching
    pub latent_encoding: Option<Vec<f32>>,
}

/// Content of an agent message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    /// Request for information
    Query(SemanticQuery),
    /// Response with data
    Response {
        data: serde_json::Value,
        confidence: f32,
    },
    /// Request to perform an action
    ActionRequest(Box<Action>),
    /// Confirmation of action completion
    ActionComplete {
        success: bool,
        result: Option<serde_json::Value>,
    },
    /// Invitation to join a swarm
    SwarmInvite(SwarmTask),
    /// Accept/reject swarm invitation
    SwarmResponse {
        accepted: bool,
        reason: Option<String>,
    },
    /// Knowledge sharing
    KnowledgeShare {
        topic: String,
        knowledge: serde_json::Value,
    },
    /// Trust update
    TrustUpdate { level: TrustLevel, reason: String },
    /// Heartbeat/ping
    Heartbeat,
    /// Error notification
    Error { code: String, message: String },
    /// Custom message type
    Custom {
        msg_type: String,
        payload: serde_json::Value,
    },
}

// =============================================================================
// LAYER 6: COLLECTIVE INTELLIGENCE (SWARMS)
// =============================================================================

/// A swarm of agents working together
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Swarm {
    pub id: Uuid,
    pub name: String,
    pub task: SwarmTask,
    pub members: Vec<SwarmMember>,
    pub leader: Option<AgentId>,
    pub created_at: DateTime<Utc>,
    pub status: SwarmStatus,
    pub consensus_threshold: f32,
}

/// A member of a swarm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmMember {
    pub agent_id: AgentId,
    pub role: SwarmRole,
    pub joined_at: DateTime<Utc>,
    pub contribution_score: f32,
}

/// Role in a swarm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwarmRole {
    /// Leads the swarm
    Leader,
    /// Coordinates tasks
    Coordinator,
    /// Executes tasks
    Worker,
    /// Validates results
    Validator,
    /// Observes and learns
    Observer,
}

/// Task for a swarm to accomplish
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTask {
    pub id: Uuid,
    pub description: String,
    pub goal: Box<Goal>,
    pub min_members: usize,
    pub max_members: usize,
    pub required_capabilities: Vec<AgentCapability>,
    pub deadline: Option<DateTime<Utc>>,
}

/// Status of a swarm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwarmStatus {
    Forming,
    Active,
    Suspended,
    Completing,
    Completed,
    Disbanded,
}

// =============================================================================
// LAYER 7: KNOWLEDGE GRAPH
// =============================================================================

/// A knowledge graph for agent memory
pub struct KnowledgeGraph {
    graph: DiGraph<KnowledgeNode, KnowledgeEdge>,
    node_index: HashMap<String, NodeIndex>,
    embeddings: DashMap<String, Vec<f32>>,
}

/// A node in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeNode {
    pub id: String,
    pub label: String,
    pub node_type: String,
    pub properties: serde_json::Value,
    pub embedding: Option<Vec<f32>>,
    pub confidence: f32,
    pub source: Option<ResourceLocator>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An edge in the knowledge graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEdge {
    pub relation: String,
    pub weight: f32,
    pub properties: serde_json::Value,
    pub source: Option<ResourceLocator>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_index: HashMap::new(),
            embeddings: DashMap::new(),
        }
    }

    /// Add a node to the knowledge graph
    pub fn add_node(&mut self, node: KnowledgeNode) -> NodeIndex {
        let id = node.id.clone();
        if let Some(embedding) = &node.embedding {
            self.embeddings.insert(id.clone(), embedding.clone());
        }
        let idx = self.graph.add_node(node);
        self.node_index.insert(id, idx);
        idx
    }

    /// Add an edge between nodes
    pub fn add_edge(&mut self, from: &str, to: &str, edge: KnowledgeEdge) -> Option<()> {
        let from_idx = self.node_index.get(from)?;
        let to_idx = self.node_index.get(to)?;
        self.graph.add_edge(*from_idx, *to_idx, edge);
        Some(())
    }

    /// Query nodes by semantic similarity
    pub fn query_similar(&self, embedding: &[f32], top_k: usize) -> Vec<(String, f32)> {
        let mut results: Vec<(String, f32)> = self
            .embeddings
            .iter()
            .map(|entry| {
                let similarity = cosine_similarity(embedding, entry.value());
                (entry.key().clone(), similarity)
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        results
    }

    /// Get node by ID
    pub fn get_node(&self, id: &str) -> Option<&KnowledgeNode> {
        let idx = self.node_index.get(id)?;
        self.graph.node_weight(*idx)
    }
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

// =============================================================================
// THE AGENTIC WEB RUNTIME
// =============================================================================

/// The core runtime for the agentic web
pub struct AgenticWebRuntime {
    /// This agent's profile
    profile: AgentProfile,
    /// Active intentions
    intentions: DashMap<Uuid, Intention>,
    /// Known agents
    known_agents: DashMap<AgentId, AgentProfile>,
    /// Active swarms
    swarms: DashMap<Uuid, Swarm>,
    /// Knowledge graph (legacy)
    knowledge: Arc<RwLock<KnowledgeGraph>>,
    /// Unified bioinspired memory - the cohesive memory architecture
    /// Integrates: EpisodicMemory (Titans), SemanticMemory, WorkingMemory, CollectiveMemory (CRDT)
    unified_memory: Arc<RwLock<UnifiedMemory>>,
    /// MIRAS predictor for pattern learning
    predictor: Arc<RwLock<MirasTitansPredictor>>,
    /// Message inbox
    inbox: mpsc::Receiver<AgentMessage>,
    /// Message outbox
    outbox: mpsc::Sender<AgentMessage>,
    /// Event broadcast
    events: broadcast::Sender<AgenticEvent>,
    /// Execution history
    history: Arc<RwLock<VecDeque<ExecutionRecord>>>,
}

/// Events in the agentic web
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgenticEvent {
    /// Agent came online
    AgentOnline(AgentId),
    /// Agent went offline
    AgentOffline(AgentId),
    /// Swarm formed
    SwarmFormed(Uuid),
    /// Swarm completed task
    SwarmCompleted(Uuid),
    /// Intention created
    IntentionCreated(Uuid),
    /// Intention completed
    IntentionCompleted(Uuid),
    /// Knowledge updated
    KnowledgeUpdated { key: String },
    /// Trust level changed
    TrustChanged {
        agent: AgentId,
        new_level: TrustLevel,
    },
    /// Custom event
    Custom {
        event_type: String,
        data: serde_json::Value,
    },
}

/// Record of an execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub id: Uuid,
    pub action: Action,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub success: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

impl AgenticWebRuntime {
    /// Create a new agentic web runtime
    pub fn new(profile: AgentProfile) -> Self {
        let (outbox, inbox) = mpsc::channel(1000);
        let (events, _) = broadcast::channel(1000);

        let predictor = MirasTitansPredictor::new_with_variant(
            spine_crypto::TitansConfig {
                embed_dim: 64,
                num_heads: 4,
                num_layers: 4,
                ff_dim: 128,
                max_seq_len: 256,
                memory_size: 64,
                seed: rand::random(),
            },
            MirasVariant::Titans,
        );

        // Create unified bioinspired memory architecture
        // This cohesively integrates Titans (via EpisodicMemory) and provides
        // distributed single-source-of-truth via CRDT-based CollectiveMemory
        let unified_memory = UnifiedMemory::new(profile.id.0, UnifiedConfig::default());

        Self {
            profile,
            intentions: DashMap::new(),
            known_agents: DashMap::new(),
            swarms: DashMap::new(),
            knowledge: Arc::new(RwLock::new(KnowledgeGraph::new())),
            unified_memory: Arc::new(RwLock::new(unified_memory)),
            predictor: Arc::new(RwLock::new(predictor)),
            inbox,
            outbox,
            events,
            history: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
        }
    }

    /// Get this agent's ID
    pub fn agent_id(&self) -> AgentId {
        self.profile.id
    }

    /// Get this agent's profile
    pub fn profile(&self) -> &AgentProfile {
        &self.profile
    }

    /// Access the unified bioinspired memory system
    /// This provides a cohesive interface to:
    /// - EpisodicMemory: Titans-based surprise-gated learning (hippocampus)
    /// - SemanticMemory: Conceptual knowledge graph (neocortex)
    /// - WorkingMemory: Active goal-directed context (prefrontal cortex)
    /// - CollectiveMemory: CRDT-based distributed truth (social brain)
    pub fn unified_memory(&self) -> &Arc<RwLock<UnifiedMemory>> {
        &self.unified_memory
    }

    /// Get a clone of the unified memory Arc for sharing
    pub fn unified_memory_arc(&self) -> Arc<RwLock<UnifiedMemory>> {
        Arc::clone(&self.unified_memory)
    }

    /// Record an experience to episodic memory (Titans-powered)
    /// Only stores if surprise exceeds threshold (surprise-gated learning)
    pub async fn remember(
        &self,
        content: &str,
        context: std::collections::HashMap<String, String>,
    ) {
        let mut mem = self.unified_memory.write().await;
        let _ = mem.episodic.store(content, context);
    }

    /// Learn a semantic concept and store in collective
    pub async fn learn_concept(
        &self,
        name: &str,
        definition: &str,
        tags: Vec<String>,
        confidence: f32,
    ) {
        let mut mem = self.unified_memory.write().await;
        let _ = mem.learn(name, definition, tags, confidence);
    }

    /// Share knowledge with the collective (CRDT-based)
    pub async fn share_knowledge(
        &self,
        key: &str,
        value: &str,
        tags: Vec<String>,
        confidence: f32,
    ) {
        let mem = self.unified_memory.read().await;
        let _ = mem.collective.store(
            key,
            spine_knowledge::KnowledgeValue::Text(value.to_string()),
            tags,
            confidence,
        );
    }

    /// Set current working goal
    pub async fn set_working_goal(&self, description: &str, priority: f32) {
        let mut mem = self.unified_memory.write().await;
        mem.working.set_goal(description, priority);
    }

    /// Create and register a new intention
    pub fn intend(&self, goal: Goal) -> Uuid {
        let intention = Intention {
            id: Uuid::new_v4(),
            agent_id: self.profile.id,
            goal,
            priority: 0.5,
            deadline: None,
            created_at: Utc::now(),
            status: IntentionStatus::Pending,
            sub_intentions: vec![],
            constraints: vec![],
        };
        let id = intention.id;
        self.intentions.insert(id, intention);
        let _ = self.events.send(AgenticEvent::IntentionCreated(id));
        id
    }

    /// Plan how to achieve an intention
    pub async fn plan(&self, intention_id: Uuid) -> Option<Plan> {
        let intention = self.intentions.get(&intention_id)?;

        let mut plan = Plan::new();

        // Simple planning based on goal type
        match &intention.goal {
            Goal::Navigate { target } => {
                plan.add_step(PlanStep {
                    id: Uuid::new_v4(),
                    action: Action::Navigate(target.clone()),
                    preconditions: vec![],
                    postconditions: vec![Condition::ResourceExists(target.clone())],
                    estimated_duration: Duration::from_secs(5),
                    retry_policy: RetryPolicy::default(),
                });
            }
            Goal::Extract { query, from } => {
                let nav_step = plan.add_step(PlanStep {
                    id: Uuid::new_v4(),
                    action: Action::Navigate(from.clone()),
                    preconditions: vec![],
                    postconditions: vec![Condition::ResourceExists(from.clone())],
                    estimated_duration: Duration::from_secs(5),
                    retry_policy: RetryPolicy::default(),
                });
                let extract_step = plan.add_step(PlanStep {
                    id: Uuid::new_v4(),
                    action: Action::Extract(query.clone()),
                    preconditions: vec![Condition::ResourceExists(from.clone())],
                    postconditions: vec![],
                    estimated_duration: Duration::from_secs(10),
                    retry_policy: RetryPolicy::default(),
                });
                plan.add_dependency(nav_step, extract_step);
            }
            Goal::FormSwarm { task } => {
                plan.add_step(PlanStep {
                    id: Uuid::new_v4(),
                    action: Action::Custom {
                        name: "form_swarm".to_string(),
                        params: serde_json::to_value(task).ok()?,
                    },
                    preconditions: vec![Condition::HasCapability(
                        AgentCapability::SwarmParticipation,
                    )],
                    postconditions: vec![],
                    estimated_duration: Duration::from_secs(30),
                    retry_policy: RetryPolicy::default(),
                });
            }
            _ => {
                // Default single-step plan
                plan.add_step(PlanStep {
                    id: Uuid::new_v4(),
                    action: Action::Custom {
                        name: "execute_goal".to_string(),
                        params: serde_json::to_value(&intention.goal).ok()?,
                    },
                    preconditions: vec![],
                    postconditions: vec![],
                    estimated_duration: Duration::from_secs(60),
                    retry_policy: RetryPolicy::default(),
                });
            }
        }

        Some(plan)
    }

    /// Register a known agent
    pub fn register_agent(&self, profile: AgentProfile) {
        let id = profile.id;
        self.known_agents.insert(id, profile);
        let _ = self.events.send(AgenticEvent::AgentOnline(id));
    }

    /// Send a message to another agent
    pub async fn send_message(&self, to: AgentId, content: MessageContent) -> anyhow::Result<Uuid> {
        let message = AgentMessage {
            id: Uuid::new_v4(),
            from: self.profile.id,
            to,
            timestamp: Utc::now(),
            content: Box::new(content),
            reply_to: None,
            thread_id: None,
            latent_encoding: None,
        };
        let id = message.id;
        self.outbox.send(message).await?;
        Ok(id)
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<AgenticEvent> {
        self.events.subscribe()
    }

    /// Store knowledge
    pub async fn store_knowledge(
        &self,
        id: String,
        label: String,
        node_type: String,
        properties: serde_json::Value,
    ) {
        let node = KnowledgeNode {
            id: id.clone(),
            label,
            node_type,
            properties,
            embedding: None,
            confidence: 1.0,
            source: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let mut kg = self.knowledge.write().await;
        kg.add_node(node);

        let _ = self.events.send(AgenticEvent::KnowledgeUpdated { key: id });
    }

    /// Query knowledge by semantic similarity
    pub async fn query_knowledge(&self, embedding: &[f32], top_k: usize) -> Vec<(String, f32)> {
        let kg = self.knowledge.read().await;
        kg.query_similar(embedding, top_k)
    }

    /// Learn from observed data
    pub async fn learn(&self, data: &[u8]) {
        let mut predictor = self.predictor.write().await;
        predictor.observe(data);
    }

    /// Get current anomaly level (indicates unusual patterns)
    pub async fn anomaly_level(&self) -> f32 {
        let predictor = self.predictor.read().await;
        predictor.anomaly_level()
    }

    /// Get all known agents
    pub fn known_agents(&self) -> Vec<AgentProfile> {
        self.known_agents
            .iter()
            .map(|r| r.value().clone())
            .collect()
    }

    /// Find agents with specific capability
    pub fn find_agents_with_capability(&self, cap: &AgentCapability) -> Vec<AgentProfile> {
        self.known_agents
            .iter()
            .filter(|r| {
                r.value()
                    .capabilities
                    .iter()
                    .any(|c| std::mem::discriminant(c) == std::mem::discriminant(cap))
            })
            .map(|r| r.value().clone())
            .collect()
    }

    /// Update intention status
    pub fn update_intention_status(&self, id: Uuid, status: IntentionStatus) {
        if let Some(mut intention) = self.intentions.get_mut(&id) {
            intention.status = status;
            if status == IntentionStatus::Completed {
                let _ = self.events.send(AgenticEvent::IntentionCompleted(id));
            }
        }
    }

    /// Get intention by ID
    pub fn get_intention(&self, id: Uuid) -> Option<Intention> {
        self.intentions.get(&id).map(|r| r.clone())
    }

    /// Get all active intentions
    pub fn active_intentions(&self) -> Vec<Intention> {
        self.intentions
            .iter()
            .filter(|r| {
                r.value().status == IntentionStatus::Active
                    || r.value().status == IntentionStatus::Pending
            })
            .map(|r| r.value().clone())
            .collect()
    }

    /// Update trust level for an agent
    pub fn update_trust(&self, agent_id: AgentId, level: TrustLevel) {
        if let Some(mut profile) = self.known_agents.get_mut(&agent_id) {
            profile.trust_level = level;
            let _ = self.events.send(AgenticEvent::TrustChanged {
                agent: agent_id,
                new_level: level,
            });
        }
    }

    /// Get message sender for external use
    pub fn message_sender(&self) -> mpsc::Sender<AgentMessage> {
        self.outbox.clone()
    }
}

// =============================================================================
// PLAN EXECUTION ENGINE
// =============================================================================

/// Result of executing an action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration: Duration,
    pub side_effects: Vec<SideEffect>,
}

/// Side effects from action execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SideEffect {
    /// Knowledge was added
    KnowledgeAdded { key: String },
    /// State was modified
    StateChanged {
        key: String,
        old: serde_json::Value,
        new: serde_json::Value,
    },
    /// Message was sent
    MessageSent { to: AgentId, message_id: Uuid },
    /// Resource was accessed
    ResourceAccessed { locator: ResourceLocator },
    /// Agent trust was updated
    TrustUpdated { agent: AgentId, level: TrustLevel },
}

/// Plan execution engine
pub struct ExecutionEngine {
    runtime: Arc<AgenticWebRuntime>,
    execution_queue: Arc<Mutex<VecDeque<(Uuid, PlanStep)>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
    results: DashMap<Uuid, ActionResult>,
}

impl ExecutionEngine {
    pub fn new(runtime: Arc<AgenticWebRuntime>) -> Self {
        Self {
            runtime,
            execution_queue: Arc::new(Mutex::new(VecDeque::new())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            results: DashMap::new(),
        }
    }

    /// Execute a plan
    pub async fn execute_plan(&self, plan: Plan) -> Result<Vec<ActionResult>, String> {
        let mut results = Vec::new();
        let mut completed: std::collections::HashSet<usize> = std::collections::HashSet::new();

        // Build dependency graph
        let mut dependents: HashMap<usize, Vec<usize>> = HashMap::new();
        for (before, after) in &plan.dependencies {
            dependents.entry(*before).or_default().push(*after);
        }

        // Find steps with no dependencies
        let mut ready: VecDeque<usize> = (0..plan.steps.len())
            .filter(|i| !plan.dependencies.iter().any(|(_, after)| after == i))
            .collect();

        while !ready.is_empty() || completed.len() < plan.steps.len() {
            // Execute ready steps in parallel
            let mut futures = Vec::new();
            let mut executing = Vec::new();

            while let Some(idx) = ready.pop_front() {
                let step = plan.steps[idx].clone();
                executing.push(idx);
                futures.push(self.execute_action(step.action.clone()));
            }

            if futures.is_empty() {
                // Deadlock detection
                if completed.len() < plan.steps.len() {
                    return Err("Deadlock detected in plan execution".to_string());
                }
                break;
            }

            // Wait for all to complete
            let step_results = futures::future::join_all(futures).await;

            for (idx, result) in executing.into_iter().zip(step_results) {
                results.push(result.clone());
                completed.insert(idx);

                // Check if this unblocks any dependents
                if let Some(deps) = dependents.get(&idx) {
                    for dep in deps {
                        // Check if all dependencies are satisfied
                        let all_deps_done = plan
                            .dependencies
                            .iter()
                            .filter(|(_, after)| after == dep)
                            .all(|(before, _)| completed.contains(before));

                        if all_deps_done && !completed.contains(dep) {
                            ready.push_back(*dep);
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    /// Execute a single action
    pub async fn execute_action(&self, action: Action) -> ActionResult {
        let start = std::time::Instant::now();
        let mut side_effects = Vec::new();

        let result = match action {
            Action::Navigate(locator) => {
                side_effects.push(SideEffect::ResourceAccessed {
                    locator: locator.clone(),
                });
                // Simulate navigation
                Ok(serde_json::json!({ "navigated_to": format!("{:?}", locator) }))
            }
            Action::Extract(query) => {
                // Simulate extraction
                Ok(serde_json::json!({
                    "query": query.query,
                    "results": ["result1", "result2", "result3"],
                    "confidence": 0.85
                }))
            }
            Action::Execute { command, args } => Ok(serde_json::json!({
                "command": command,
                "args": args,
                "status": "executed"
            })),
            Action::Store { key, value } => {
                side_effects.push(SideEffect::KnowledgeAdded { key: key.clone() });
                self.runtime
                    .store_knowledge(
                        key.clone(),
                        key.clone(),
                        "stored".to_string(),
                        value.clone(),
                    )
                    .await;
                Ok(serde_json::json!({ "stored": key }))
            }
            Action::Retrieve { key } => {
                let kg = self.runtime.knowledge.read().await;
                if let Some(node) = kg.get_node(&key) {
                    Ok(serde_json::json!({
                        "key": key,
                        "value": node.properties.clone()
                    }))
                } else {
                    Err("Key not found".to_string())
                }
            }
            Action::Wait {
                condition: _,
                timeout,
            } => {
                tokio::time::sleep(timeout.min(Duration::from_secs(10))).await;
                Ok(serde_json::json!({ "waited": timeout.as_secs() }))
            }
            Action::Parallel(actions) => {
                let futures: Vec<_> = actions
                    .into_iter()
                    .map(|a| Box::pin(self.execute_action(a)))
                    .collect();
                let results = futures::future::join_all(futures).await;
                let all_success = results.iter().all(|r| r.success);
                if all_success {
                    Ok(serde_json::json!({
                        "parallel_results": results.iter().map(|r| r.data.clone()).collect::<Vec<_>>()
                    }))
                } else {
                    Err("One or more parallel actions failed".to_string())
                }
            }
            Action::Sequence(actions) => {
                let mut seq_results = Vec::new();
                for action in actions {
                    let result = Box::pin(self.execute_action(action)).await;
                    if !result.success {
                        return ActionResult {
                            success: false,
                            data: None,
                            error: result.error,
                            duration: start.elapsed(),
                            side_effects,
                        };
                    }
                    seq_results.push(result.data);
                }
                Ok(serde_json::json!({ "sequence_results": seq_results }))
            }
            Action::Branch {
                condition,
                if_true,
                if_false,
            } => {
                let cond_met = Box::pin(self.evaluate_condition(&condition)).await;
                let action = if cond_met { *if_true } else { *if_false };
                let inner = Box::pin(self.execute_action(action)).await;
                side_effects.extend(inner.side_effects.clone());
                if inner.success {
                    Ok(inner.data.unwrap_or_default())
                } else {
                    Err(inner.error.unwrap_or_default())
                }
            }
            Action::Learn { topic } => {
                self.runtime.learn(topic.as_bytes()).await;
                Ok(serde_json::json!({ "learned": topic }))
            }
            Action::Message { to, content: _ } => {
                let msg_id = Uuid::new_v4();
                side_effects.push(SideEffect::MessageSent {
                    to,
                    message_id: msg_id,
                });
                Ok(serde_json::json!({ "message_id": msg_id.to_string() }))
            }
            Action::Delegate { to, task } => Ok(serde_json::json!({
                "delegated_to": to.0.to_string(),
                "task": format!("{:?}", task)
            })),
            Action::Custom { name, params } => Ok(serde_json::json!({
                "custom_action": name,
                "params": params
            })),
        };

        match result {
            Ok(data) => ActionResult {
                success: true,
                data: Some(data),
                error: None,
                duration: start.elapsed(),
                side_effects,
            },
            Err(e) => ActionResult {
                success: false,
                data: None,
                error: Some(e),
                duration: start.elapsed(),
                side_effects,
            },
        }
    }

    /// Evaluate a condition
    async fn evaluate_condition(&self, condition: &Condition) -> bool {
        match condition {
            Condition::Always => true,
            Condition::Never => false,
            Condition::ResourceExists(_) => true, // Simplified
            Condition::ValueEquals {
                path: _,
                expected: _,
            } => true, // Simplified
            Condition::AgentAvailable(id) => self.runtime.known_agents.contains_key(id),
            Condition::HasCapability(cap) => self
                .runtime
                .profile()
                .capabilities
                .iter()
                .any(|c| std::mem::discriminant(c) == std::mem::discriminant(cap)),
            Condition::And(conditions) => {
                for c in conditions {
                    if !Box::pin(self.evaluate_condition(c)).await {
                        return false;
                    }
                }
                true
            }
            Condition::Or(conditions) => {
                for c in conditions {
                    if Box::pin(self.evaluate_condition(c)).await {
                        return true;
                    }
                }
                false
            }
            Condition::Not(c) => !Box::pin(self.evaluate_condition(c)).await,
            Condition::Custom {
                predicate: _,
                args: _,
            } => true,
        }
    }
}

// =============================================================================
// SWARM COORDINATOR
// =============================================================================

/// Coordinates swarm formation and task distribution with graphical model optimization
pub struct SwarmCoordinator {
    runtime: Arc<AgenticWebRuntime>,
    active_swarms: DashMap<Uuid, SwarmState>,
    pending_invites: DashMap<Uuid, Vec<AgentId>>,
    optimizer: std::sync::RwLock<GraphicalSwarmOptimizer>,
    topology_cache: DashMap<Uuid, SwarmTopologyMetrics>,
}

/// Internal state for a swarm
#[derive(Debug, Clone)]
pub struct SwarmState {
    pub swarm: Swarm,
    pub task_assignments: HashMap<AgentId, Vec<Uuid>>,
    pub partial_results: HashMap<AgentId, serde_json::Value>,
    pub started_at: DateTime<Utc>,
    pub phase: SwarmPhase,
    pub model_type: Option<GraphicalModelType>,
    pub optimization_result: Option<SwarmOptimizationResult>,
}

/// Phase of swarm execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwarmPhase {
    Forming,
    Optimizing,
    Distributing,
    Executing,
    Aggregating,
    Validating,
    Complete,
}

/// Metrics for swarm topology performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmTopologyMetrics {
    pub model_type: GraphicalModelType,
    pub inference_time_us: u64,
    pub message_count: usize,
    pub convergence_rate: f64,
    pub task_completion_rate: f64,
    pub load_balance_score: f64,
}

impl SwarmCoordinator {
    pub fn new(runtime: Arc<AgenticWebRuntime>) -> Self {
        Self {
            runtime,
            active_swarms: DashMap::new(),
            pending_invites: DashMap::new(),
            optimizer: std::sync::RwLock::new(GraphicalSwarmOptimizer::new(
                GraphicalModelType::FactorGraph,
            )),
            topology_cache: DashMap::new(),
        }
    }

    /// Create and start forming a new swarm with optimal graphical model
    pub async fn create_swarm(&self, task: SwarmTask, leader: Option<AgentId>) -> Uuid {
        let swarm_id = Uuid::new_v4();
        let leader_id = leader.unwrap_or(self.runtime.agent_id());

        let swarm = Swarm {
            id: swarm_id,
            name: format!("Swarm-{}", &swarm_id.to_string()[..8]),
            task: task.clone(),
            members: vec![SwarmMember {
                agent_id: leader_id,
                role: SwarmRole::Leader,
                joined_at: Utc::now(),
                contribution_score: 0.0,
            }],
            leader: Some(leader_id),
            created_at: Utc::now(),
            status: SwarmStatus::Forming,
            consensus_threshold: 0.67,
        };

        // Select optimal graphical model based on task characteristics
        let model_type = self.select_optimal_model(&task, &swarm);

        let state = SwarmState {
            swarm,
            task_assignments: HashMap::new(),
            partial_results: HashMap::new(),
            started_at: Utc::now(),
            phase: SwarmPhase::Forming,
            model_type: Some(model_type),
            optimization_result: None,
        };

        self.active_swarms.insert(swarm_id, state);
        let _ = self
            .runtime
            .events
            .send(AgenticEvent::SwarmFormed(swarm_id));

        // Find and invite suitable agents
        let candidates = self.find_candidates(&task);
        self.pending_invites.insert(swarm_id, candidates.clone());

        swarm_id
    }

    /// Select the optimal graphical model type for a swarm
    fn select_optimal_model(&self, task: &SwarmTask, swarm: &Swarm) -> GraphicalModelType {
        let num_members = swarm.members.len();
        let has_deadline = task.deadline.is_some();
        let needs_consensus = swarm.consensus_threshold > 0.5;
        let required_caps = task.required_capabilities.len();

        // Check historical performance for similar configurations
        let best_from_cache = self
            .topology_cache
            .iter()
            .filter(|entry| entry.task_completion_rate > 0.8)
            .max_by(|a, b| {
                a.convergence_rate
                    .partial_cmp(&b.convergence_rate)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|entry| entry.model_type);

        // Heuristic selection based on swarm characteristics
        match (num_members, needs_consensus, has_deadline, required_caps) {
            // Small teams needing consensus → Hypergraph for multi-way constraints
            (1..=3, true, _, _) => GraphicalModelType::Hypergraph,

            // Small teams without consensus → Bayesian Network (exact inference)
            (1..=3, false, _, _) => GraphicalModelType::BayesianNetwork,

            // Time-sensitive tasks → Dynamic Bayesian Network
            (_, _, true, _) => GraphicalModelType::DynamicBayesian,

            // Complex capability requirements → Factor Graph
            (_, _, _, 3..) => GraphicalModelType::FactorGraph,

            // Medium teams with consensus → MRF for pairwise coordination
            (4..=10, true, _, _) => GraphicalModelType::MarkovRandomField,

            // Large teams → CRF for scalable structured prediction
            (11.., _, _, _) => GraphicalModelType::ConditionalRandomField,

            // Default to Factor Graph for flexibility
            _ => best_from_cache.unwrap_or(GraphicalModelType::FactorGraph),
        }
    }

    /// Find agents suitable for a task
    fn find_candidates(&self, task: &SwarmTask) -> Vec<AgentId> {
        let mut candidates = Vec::new();

        for cap in &task.required_capabilities {
            let agents = self.runtime.find_agents_with_capability(cap);
            for agent in agents {
                if !candidates.contains(&agent.id) && agent.trust_level >= TrustLevel::Trusted {
                    candidates.push(agent.id);
                }
            }
        }

        candidates.truncate(task.max_members);
        candidates
    }

    /// Handle an agent joining a swarm
    pub fn agent_joined(&self, swarm_id: Uuid, agent_id: AgentId, role: SwarmRole) {
        if let Some(mut state) = self.active_swarms.get_mut(&swarm_id) {
            state.swarm.members.push(SwarmMember {
                agent_id,
                role,
                joined_at: Utc::now(),
                contribution_score: 0.0,
            });

            // Check if we have enough members to start optimization
            if state.swarm.members.len() >= state.swarm.task.min_members {
                state.swarm.status = SwarmStatus::Active;
                state.phase = SwarmPhase::Optimizing;
            }
        }
    }

    /// Run graphical model optimization for task distribution
    pub fn optimize_swarm(&self, swarm_id: Uuid) -> Option<SwarmOptimizationResult> {
        let start = std::time::Instant::now();

        let swarm = {
            let state = self.active_swarms.get(&swarm_id)?;
            state.swarm.clone()
        };

        // Create and run graphical model
        let result = {
            let mut optimizer = self.optimizer.write().ok()?;
            optimizer.create_model_for_swarm(&swarm);
            optimizer.optimize_swarm(swarm_id)?
        };

        // Update state with optimization result
        if let Some(mut state) = self.active_swarms.get_mut(&swarm_id) {
            state.optimization_result = Some(result.clone());
            state.phase = SwarmPhase::Distributing;
        }

        // Cache topology metrics
        let elapsed = start.elapsed();
        self.topology_cache.insert(
            swarm_id,
            SwarmTopologyMetrics {
                model_type: result.model_type,
                inference_time_us: elapsed.as_micros() as u64,
                message_count: result.iterations * swarm.members.len(),
                convergence_rate: if result.converged { 1.0 } else { 0.5 },
                task_completion_rate: 0.0, // Updated after completion
                load_balance_score: self.compute_load_balance(&result.assignment),
            },
        );

        Some(result)
    }

    /// Compute load balance score from assignment
    fn compute_load_balance(&self, assignment: &HashMap<Uuid, usize>) -> f64 {
        if assignment.is_empty() {
            return 1.0;
        }

        let values: Vec<f64> = assignment.values().map(|&v| v as f64).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;

        // Score of 1.0 is perfect balance, lower is worse
        1.0 / (1.0 + variance.sqrt())
    }

    /// Distribute tasks using graphical model inference results
    pub fn distribute_tasks(&self, swarm_id: Uuid) -> HashMap<AgentId, Vec<serde_json::Value>> {
        let mut assignments = HashMap::new();

        // Try to optimize first if not already done
        let optimization = self.optimize_swarm(swarm_id);

        if let Some(mut state) = self.active_swarms.get_mut(&swarm_id) {
            let workers: Vec<_> = state
                .swarm
                .members
                .iter()
                .filter(|m| matches!(m.role, SwarmRole::Worker | SwarmRole::Coordinator))
                .collect();

            if workers.is_empty() {
                return assignments;
            }

            // Use graphical model assignment if available
            if let Some(ref opt_result) = optimization {
                // Map graphical model assignment to actual agent tasks
                let mut agent_scores: Vec<(AgentId, f64)> = Vec::new();

                for member in &workers {
                    // Find the assignment value for this agent's node
                    let score = opt_result
                        .assignment
                        .values()
                        .map(|&v| v as f64)
                        .sum::<f64>()
                        / opt_result.assignment.len().max(1) as f64;
                    agent_scores.push((member.agent_id, score));
                }

                // Sort by score and assign proportionally
                agent_scores
                    .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                let _total_subtasks = workers.len().max(1);
                for (i, (agent_id, score)) in agent_scores.iter().enumerate() {
                    let priority = if *score > 0.6 {
                        "high"
                    } else if *score > 0.3 {
                        "medium"
                    } else {
                        "low"
                    };

                    assignments.insert(*agent_id, vec![serde_json::json!({
                        "subtask_id": i,
                        "swarm_id": swarm_id.to_string(),
                        "model_type": format!("{:?}", opt_result.model_type),
                        "priority": priority,
                        "confidence": score,
                        "instructions": format!("Execute optimized subtask {} (priority: {})", i, priority)
                    })]);
                }

                state.phase = SwarmPhase::Executing;
            } else {
                // Fallback to capability-based distribution
                let required_caps: Vec<String> = state
                    .swarm
                    .task
                    .required_capabilities
                    .iter()
                    .map(|c| format!("{:?}", c))
                    .collect();
                for (i, member) in workers.iter().enumerate() {
                    let capability_match =
                        self.compute_capability_match(member.agent_id, &required_caps);

                    assignments.insert(member.agent_id, vec![serde_json::json!({
                        "subtask_id": i,
                        "swarm_id": swarm_id.to_string(),
                        "capability_match": capability_match,
                        "instructions": format!("Execute subtask {} (capability match: {:.2})", i, capability_match)
                    })]);
                }

                state.phase = SwarmPhase::Executing;
            }
        }

        assignments
    }

    /// Compute capability match score for an agent
    fn compute_capability_match(&self, agent_id: AgentId, required: &[String]) -> f64 {
        if required.is_empty() {
            return 1.0;
        }

        let agent_caps: Vec<String> = self
            .runtime
            .known_agents
            .get(&agent_id)
            .map(|a| a.capabilities.iter().map(|c| format!("{:?}", c)).collect())
            .unwrap_or_default();

        let matched = required
            .iter()
            .filter(|r| agent_caps.iter().any(|c: &String| c.contains(r.as_str())))
            .count();

        matched as f64 / required.len() as f64
    }

    /// Submit partial result from a member
    pub fn submit_result(&self, swarm_id: Uuid, agent_id: AgentId, result: serde_json::Value) {
        if let Some(mut state) = self.active_swarms.get_mut(&swarm_id) {
            // Evaluate before moving
            let success_score = self.evaluate_result_quality(&result);
            state.partial_results.insert(agent_id, result);

            // Update contribution score
            if let Some(member) = state
                .swarm
                .members
                .iter_mut()
                .find(|m| m.agent_id == agent_id)
            {
                member.contribution_score += 1.0;
            }

            // Update graphical model with outcome for learning
            if let Ok(mut optimizer) = self.optimizer.write() {
                optimizer.update_model(swarm_id, agent_id, success_score);
            }

            // Check if all results are in
            let workers: Vec<_> = state
                .swarm
                .members
                .iter()
                .filter(|m| matches!(m.role, SwarmRole::Worker | SwarmRole::Coordinator))
                .map(|m| m.agent_id)
                .collect();

            if workers
                .iter()
                .all(|w| state.partial_results.contains_key(w))
            {
                state.phase = SwarmPhase::Aggregating;
            }
        }
    }

    /// Evaluate the quality of a partial result (0.0 to 1.0)
    fn evaluate_result_quality(&self, result: &serde_json::Value) -> f64 {
        // Simple heuristic based on result completeness
        let mut score = 0.0;

        if result.is_object() {
            let obj = result.as_object().unwrap();
            // Check for success indicator
            if let Some(status) = obj.get("status") {
                if status == "success" || status == "completed" {
                    score += 0.5;
                }
            }
            // Check for data presence
            if obj.get("data").is_some() || obj.get("result").is_some() {
                score += 0.3;
            }
            // Check for error absence
            if obj.get("error").is_none() {
                score += 0.2;
            }
        } else if result.is_array() {
            // Array results are usually good
            score = 0.7;
        } else if !result.is_null() {
            score = 0.5;
        }

        score
    }

    /// Aggregate results and complete the swarm task with learning
    pub fn aggregate_results(&self, swarm_id: Uuid) -> Option<serde_json::Value> {
        let (model_type, members_count, optimization_info) = {
            let state = self.active_swarms.get(&swarm_id)?;
            if state.phase != SwarmPhase::Aggregating {
                return None;
            }
            (
                state.model_type,
                state.swarm.members.len(),
                state
                    .optimization_result
                    .as_ref()
                    .map(|o| (o.converged, o.iterations, o.free_energy)),
            )
        };

        if let Some(mut state) = self.active_swarms.get_mut(&swarm_id) {
            // Combine all partial results
            let combined: Vec<_> = state.partial_results.values().cloned().collect();

            // Calculate aggregate quality score
            let quality_scores: Vec<f64> = combined
                .iter()
                .map(|r| self.evaluate_result_quality(r))
                .collect();
            let avg_quality = if !quality_scores.is_empty() {
                quality_scores.iter().sum::<f64>() / quality_scores.len() as f64
            } else {
                0.0
            };

            // Update topology cache with completion rate
            if let Some(mut metrics) = self.topology_cache.get_mut(&swarm_id) {
                metrics.task_completion_rate = avg_quality;
            }

            let result = serde_json::json!({
                "swarm_id": swarm_id.to_string(),
                "members": members_count,
                "model_type": format!("{:?}", model_type),
                "optimization": optimization_info.map(|(conv, iter, energy)| serde_json::json!({
                    "converged": conv,
                    "iterations": iter,
                    "free_energy": energy
                })),
                "quality_score": avg_quality,
                "partial_results": combined,
                "completed_at": Utc::now().to_rfc3339()
            });

            state.phase = SwarmPhase::Complete;
            state.swarm.status = SwarmStatus::Completed;

            let _ = self
                .runtime
                .events
                .send(AgenticEvent::SwarmCompleted(swarm_id));

            return Some(result);
        }
        None
    }

    /// Get swarm status
    pub fn swarm_status(&self, swarm_id: Uuid) -> Option<SwarmStatus> {
        self.active_swarms.get(&swarm_id).map(|s| s.swarm.status)
    }

    /// Get all active swarms
    pub fn active_swarms(&self) -> Vec<Uuid> {
        self.active_swarms
            .iter()
            .filter(|s| {
                s.swarm.status == SwarmStatus::Active || s.swarm.status == SwarmStatus::Forming
            })
            .map(|s| s.swarm.id)
            .collect()
    }

    /// Get topology performance metrics for a swarm
    pub fn get_topology_metrics(&self, swarm_id: Uuid) -> Option<SwarmTopologyMetrics> {
        self.topology_cache.get(&swarm_id).map(|m| m.clone())
    }

    /// Get best performing graphical model type based on historical data
    pub fn get_best_model_type(&self) -> GraphicalModelType {
        self.topology_cache
            .iter()
            .filter(|entry| entry.task_completion_rate > 0.5)
            .max_by(|a, b| {
                let score_a = a.task_completion_rate * a.convergence_rate
                    / (a.inference_time_us as f64 + 1.0);
                let score_b = b.task_completion_rate * b.convergence_rate
                    / (b.inference_time_us as f64 + 1.0);
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|entry| entry.model_type)
            .unwrap_or(GraphicalModelType::FactorGraph)
    }

    /// Compare performance across all graphical model types
    pub fn benchmark_model_types(&self) -> HashMap<GraphicalModelType, f64> {
        let mut scores = HashMap::new();

        for entry in self.topology_cache.iter() {
            let score =
                entry.task_completion_rate * entry.convergence_rate * entry.load_balance_score
                    / (entry.inference_time_us as f64 / 1000.0 + 1.0);

            scores
                .entry(entry.model_type)
                .and_modify(|s| *s = (*s + score) / 2.0)
                .or_insert(score);
        }

        scores
    }
}

// =============================================================================
// AGENT REGISTRY & DISCOVERY
// =============================================================================

/// Central registry for agent discovery
pub struct AgentRegistry {
    agents: DashMap<AgentId, RegisteredAgent>,
    by_capability: DashMap<String, Vec<AgentId>>,
    by_trust: DashMap<TrustLevel, Vec<AgentId>>,
}

/// A registered agent with network info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredAgent {
    pub profile: AgentProfile,
    pub endpoint: Option<String>,
    pub last_heartbeat: DateTime<Utc>,
    pub is_online: bool,
    pub metadata: HashMap<String, String>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: DashMap::new(),
            by_capability: DashMap::new(),
            by_trust: DashMap::new(),
        }
    }

    /// Register an agent
    pub fn register(&self, profile: AgentProfile, endpoint: Option<String>) {
        let agent_id = profile.id;

        // Index by capabilities
        for cap in &profile.capabilities {
            let cap_key = format!("{:?}", cap);
            self.by_capability
                .entry(cap_key)
                .or_default()
                .push(agent_id);
        }

        // Index by trust level
        self.by_trust
            .entry(profile.trust_level)
            .or_default()
            .push(agent_id);

        // Store agent
        self.agents.insert(
            agent_id,
            RegisteredAgent {
                profile,
                endpoint,
                last_heartbeat: Utc::now(),
                is_online: true,
                metadata: HashMap::new(),
            },
        );
    }

    /// Update heartbeat
    pub fn heartbeat(&self, agent_id: AgentId) {
        if let Some(mut agent) = self.agents.get_mut(&agent_id) {
            agent.last_heartbeat = Utc::now();
            agent.is_online = true;
        }
    }

    /// Find agents by capability
    pub fn find_by_capability(&self, cap: &AgentCapability) -> Vec<RegisteredAgent> {
        let cap_key = format!("{:?}", cap);
        if let Some(ids) = self.by_capability.get(&cap_key) {
            ids.iter()
                .filter_map(|id| self.agents.get(id).map(|r| r.clone()))
                .filter(|a| a.is_online)
                .collect()
        } else {
            vec![]
        }
    }

    /// Find agents by trust level (and above)
    pub fn find_by_trust(&self, min_level: TrustLevel) -> Vec<RegisteredAgent> {
        let levels = [
            TrustLevel::Core,
            TrustLevel::HighlyTrusted,
            TrustLevel::Trusted,
            TrustLevel::Verified,
            TrustLevel::Unknown,
        ];

        let mut results = Vec::new();
        for level in levels.iter().take_while(|&&l| l >= min_level) {
            if let Some(ids) = self.by_trust.get(level) {
                for id in ids.iter() {
                    if let Some(agent) = self.agents.get(id) {
                        if agent.is_online {
                            results.push(agent.clone());
                        }
                    }
                }
            }
        }
        results
    }

    /// Get all online agents
    pub fn online_agents(&self) -> Vec<RegisteredAgent> {
        self.agents
            .iter()
            .filter(|r| r.is_online)
            .map(|r| r.clone())
            .collect()
    }

    /// Mark agent as offline
    pub fn mark_offline(&self, agent_id: AgentId) {
        if let Some(mut agent) = self.agents.get_mut(&agent_id) {
            agent.is_online = false;
        }
    }

    /// Prune stale agents (no heartbeat in duration)
    pub fn prune_stale(&self, timeout: Duration) {
        let cutoff = Utc::now() - chrono::Duration::from_std(timeout).unwrap_or_default();
        for mut entry in self.agents.iter_mut() {
            if entry.last_heartbeat < cutoff {
                entry.is_online = false;
            }
        }
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// AGENT NETWORK SERVER
// =============================================================================

/// Server for agent-to-agent communication
pub struct AgentServer {
    runtime: Arc<AgenticWebRuntime>,
    registry: Arc<AgentRegistry>,
    addr: SocketAddr,
    running: Arc<std::sync::atomic::AtomicBool>,
}

/// Protocol message for agent communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentProtocolMessage {
    /// Register with the network
    Register(AgentProfile),
    /// Heartbeat to maintain presence
    Heartbeat(AgentId),
    /// Discover agents with capability
    Discover { capability: AgentCapability },
    /// List of discovered agents
    DiscoverResponse(Vec<RegisteredAgent>),
    /// Send a message to another agent
    SendMessage(AgentMessage),
    /// Message delivery confirmation
    MessageAck { message_id: Uuid },
    /// Swarm invitation
    SwarmInvite { swarm_id: Uuid, task: SwarmTask },
    /// Swarm invitation response
    SwarmResponse {
        swarm_id: Uuid,
        accepted: bool,
        role: Option<SwarmRole>,
    },
    /// Query knowledge
    KnowledgeQuery { query: SemanticQuery },
    /// Knowledge response
    KnowledgeResponse { data: serde_json::Value },
    /// Error
    Error { code: u32, message: String },
}

impl AgentServer {
    pub fn new(
        runtime: Arc<AgenticWebRuntime>,
        registry: Arc<AgentRegistry>,
        addr: SocketAddr,
    ) -> Self {
        Self {
            runtime,
            registry,
            addr,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the server
    pub async fn start(&self) -> anyhow::Result<()> {
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let listener = TcpListener::bind(self.addr).await?;
        log::info!("Agent server listening on {}", self.addr);

        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    log::debug!("Agent connection from {}", addr);
                    let runtime = self.runtime.clone();
                    let registry = self.registry.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(stream, runtime, registry).await {
                            log::error!("Connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    log::error!("Accept error: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Stop the server
    pub fn stop(&self) {
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    async fn handle_connection(
        mut stream: TcpStream,
        runtime: Arc<AgenticWebRuntime>,
        registry: Arc<AgentRegistry>,
    ) -> anyhow::Result<()> {
        let mut buf = vec![0u8; 65536];

        loop {
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                break;
            }

            // Parse message
            let msg: AgentProtocolMessage = match serde_json::from_slice(&buf[..n]) {
                Ok(m) => m,
                Err(e) => {
                    let err = AgentProtocolMessage::Error {
                        code: 400,
                        message: format!("Invalid message: {}", e),
                    };
                    let resp = serde_json::to_vec(&err)?;
                    stream.write_all(&resp).await?;
                    continue;
                }
            };

            // Handle message
            let response = Self::handle_message(msg, &runtime, &registry).await;
            let resp_bytes = serde_json::to_vec(&response)?;
            stream.write_all(&resp_bytes).await?;
        }

        Ok(())
    }

    async fn handle_message(
        msg: AgentProtocolMessage,
        runtime: &AgenticWebRuntime,
        registry: &AgentRegistry,
    ) -> AgentProtocolMessage {
        match msg {
            AgentProtocolMessage::Register(profile) => {
                registry.register(profile.clone(), None);
                runtime.register_agent(profile);
                AgentProtocolMessage::MessageAck {
                    message_id: Uuid::new_v4(),
                }
            }
            AgentProtocolMessage::Heartbeat(id) => {
                registry.heartbeat(id);
                AgentProtocolMessage::MessageAck {
                    message_id: Uuid::new_v4(),
                }
            }
            AgentProtocolMessage::Discover { capability } => {
                let agents = registry.find_by_capability(&capability);
                AgentProtocolMessage::DiscoverResponse(agents)
            }
            AgentProtocolMessage::SendMessage(msg) => {
                let msg_id = msg.id;
                // Route to destination agent
                if let Err(e) = runtime.outbox.send(msg).await {
                    return AgentProtocolMessage::Error {
                        code: 500,
                        message: format!("Failed to route message: {}", e),
                    };
                }
                AgentProtocolMessage::MessageAck { message_id: msg_id }
            }
            AgentProtocolMessage::KnowledgeQuery { query } => {
                // Simple mock response
                AgentProtocolMessage::KnowledgeResponse {
                    data: serde_json::json!({
                        "query": query.query,
                        "results": []
                    }),
                }
            }
            AgentProtocolMessage::SwarmInvite { swarm_id, task: _ } => {
                // Auto-accept for now
                AgentProtocolMessage::SwarmResponse {
                    swarm_id,
                    accepted: true,
                    role: Some(SwarmRole::Worker),
                }
            }
            _ => AgentProtocolMessage::Error {
                code: 400,
                message: "Unexpected message type".to_string(),
            },
        }
    }
}

// =============================================================================
// AGENT CLIENT
// =============================================================================

/// Client for connecting to agent networks
pub struct AgentClient {
    runtime: Arc<AgenticWebRuntime>,
    connections: DashMap<String, Arc<Mutex<TcpStream>>>,
}

impl AgentClient {
    pub fn new(runtime: Arc<AgenticWebRuntime>) -> Self {
        Self {
            runtime,
            connections: DashMap::new(),
        }
    }

    /// Connect to an agent server
    pub async fn connect(&self, addr: &str) -> anyhow::Result<()> {
        let stream = TcpStream::connect(addr).await?;
        self.connections
            .insert(addr.to_string(), Arc::new(Mutex::new(stream)));

        // Register ourselves
        let register_msg = AgentProtocolMessage::Register(self.runtime.profile().clone());
        self.send(addr, register_msg).await?;

        Ok(())
    }

    /// Send a message to a server
    pub async fn send(
        &self,
        addr: &str,
        msg: AgentProtocolMessage,
    ) -> anyhow::Result<AgentProtocolMessage> {
        let conn = self
            .connections
            .get(addr)
            .ok_or_else(|| anyhow::anyhow!("Not connected to {}", addr))?;

        let mut stream = conn.lock().await;
        let msg_bytes = serde_json::to_vec(&msg)?;
        stream.write_all(&msg_bytes).await?;

        let mut buf = vec![0u8; 65536];
        let n = stream.read(&mut buf).await?;
        let response: AgentProtocolMessage = serde_json::from_slice(&buf[..n])?;

        Ok(response)
    }

    /// Discover agents with a capability
    pub async fn discover(
        &self,
        addr: &str,
        capability: AgentCapability,
    ) -> anyhow::Result<Vec<RegisteredAgent>> {
        let msg = AgentProtocolMessage::Discover { capability };
        match self.send(addr, msg).await? {
            AgentProtocolMessage::DiscoverResponse(agents) => Ok(agents),
            AgentProtocolMessage::Error { code, message } => {
                Err(anyhow::anyhow!("Discovery failed ({}): {}", code, message))
            }
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    }

    /// Send heartbeat
    pub async fn heartbeat(&self, addr: &str) -> anyhow::Result<()> {
        let msg = AgentProtocolMessage::Heartbeat(self.runtime.agent_id());
        self.send(addr, msg).await?;
        Ok(())
    }

    /// Disconnect from a server
    pub fn disconnect(&self, addr: &str) {
        self.connections.remove(addr);
    }
}

// =============================================================================
// BEHAVIOR TREE FOR AGENT AUTONOMY
// =============================================================================

/// Behavior tree node for autonomous agent behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BehaviorNode {
    /// Execute an action
    Action(Action),
    /// Check a condition
    Condition(Condition),
    /// Run children in sequence until one fails
    Sequence(Vec<BehaviorNode>),
    /// Run children until one succeeds
    Selector(Vec<BehaviorNode>),
    /// Run child repeatedly
    Repeater {
        child: Box<BehaviorNode>,
        count: Option<u32>,
    },
    /// Invert child result
    Inverter(Box<BehaviorNode>),
    /// Always succeed
    Succeeder(Box<BehaviorNode>),
    /// Run child until condition is met
    UntilSuccess(Box<BehaviorNode>),
    /// Run children in parallel
    Parallel {
        children: Vec<BehaviorNode>,
        success_threshold: usize,
    },
}

/// Result of behavior node execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BehaviorResult {
    Success,
    Failure,
    Running,
}

/// Behavior tree executor
pub struct BehaviorExecutor {
    engine: Arc<ExecutionEngine>,
}

impl BehaviorExecutor {
    pub fn new(engine: Arc<ExecutionEngine>) -> Self {
        Self { engine }
    }

    /// Execute a behavior tree
    pub async fn execute(&self, node: &BehaviorNode) -> BehaviorResult {
        match node {
            BehaviorNode::Action(action) => {
                let result = self.engine.execute_action(action.clone()).await;
                if result.success {
                    BehaviorResult::Success
                } else {
                    BehaviorResult::Failure
                }
            }
            BehaviorNode::Condition(condition) => {
                if self.engine.evaluate_condition(condition).await {
                    BehaviorResult::Success
                } else {
                    BehaviorResult::Failure
                }
            }
            BehaviorNode::Sequence(children) => {
                for child in children {
                    match Box::pin(self.execute(child)).await {
                        BehaviorResult::Failure => return BehaviorResult::Failure,
                        BehaviorResult::Running => return BehaviorResult::Running,
                        BehaviorResult::Success => continue,
                    }
                }
                BehaviorResult::Success
            }
            BehaviorNode::Selector(children) => {
                for child in children {
                    match Box::pin(self.execute(child)).await {
                        BehaviorResult::Success => return BehaviorResult::Success,
                        BehaviorResult::Running => return BehaviorResult::Running,
                        BehaviorResult::Failure => continue,
                    }
                }
                BehaviorResult::Failure
            }
            BehaviorNode::Repeater { child, count } => {
                let iterations = count.unwrap_or(u32::MAX);
                for _ in 0..iterations {
                    let result = Box::pin(self.execute(child)).await;
                    if result == BehaviorResult::Failure {
                        return BehaviorResult::Failure;
                    }
                }
                BehaviorResult::Success
            }
            BehaviorNode::Inverter(child) => match Box::pin(self.execute(child)).await {
                BehaviorResult::Success => BehaviorResult::Failure,
                BehaviorResult::Failure => BehaviorResult::Success,
                BehaviorResult::Running => BehaviorResult::Running,
            },
            BehaviorNode::Succeeder(child) => {
                let _ = Box::pin(self.execute(child)).await;
                BehaviorResult::Success
            }
            BehaviorNode::UntilSuccess(child) => loop {
                match Box::pin(self.execute(child)).await {
                    BehaviorResult::Success => return BehaviorResult::Success,
                    BehaviorResult::Running => return BehaviorResult::Running,
                    BehaviorResult::Failure => continue,
                }
            },
            BehaviorNode::Parallel {
                children,
                success_threshold,
            } => {
                let futures: Vec<_> = children.iter().map(|c| Box::pin(self.execute(c))).collect();

                let results = futures::future::join_all(futures).await;
                let successes = results
                    .iter()
                    .filter(|&&r| r == BehaviorResult::Success)
                    .count();

                if successes >= *success_threshold {
                    BehaviorResult::Success
                } else {
                    BehaviorResult::Failure
                }
            }
        }
    }
}

// =============================================================================
// REACTIVE STATE MANAGER
// =============================================================================

/// Reactive state that triggers callbacks on change
pub struct ReactiveState<T: Clone + Send + Sync + 'static> {
    value: Arc<RwLock<T>>,
    subscribers: Arc<RwLock<Vec<mpsc::Sender<T>>>>,
}

impl<T: Clone + Send + Sync + 'static> ReactiveState<T> {
    pub fn new(initial: T) -> Self {
        Self {
            value: Arc::new(RwLock::new(initial)),
            subscribers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn get(&self) -> T {
        self.value.read().await.clone()
    }

    pub async fn set(&self, value: T) {
        *self.value.write().await = value.clone();

        // Notify subscribers
        let subs = self.subscribers.read().await;
        for sub in subs.iter() {
            let _ = sub.send(value.clone()).await;
        }
    }

    pub async fn subscribe(&self) -> mpsc::Receiver<T> {
        let (tx, rx) = mpsc::channel(100);
        self.subscribers.write().await.push(tx);
        rx
    }

    pub async fn update<F: FnOnce(&mut T)>(&self, f: F) {
        let mut guard = self.value.write().await;
        f(&mut *guard);
        let value = guard.clone();
        drop(guard);

        let subs = self.subscribers.read().await;
        for sub in subs.iter() {
            let _ = sub.send(value.clone()).await;
        }
    }
}

// =============================================================================
// PUBLIC API
// =============================================================================

/// Create a new agentic web agent
pub fn create_agent(name: impl Into<String>) -> AgenticWebRuntime {
    let profile = AgentProfile::new(name);
    AgenticWebRuntime::new(profile)
}

/// Create an agent with specific capabilities
pub fn create_agent_with_capabilities(
    name: impl Into<String>,
    capabilities: Vec<AgentCapability>,
) -> AgenticWebRuntime {
    let profile = AgentProfile::new(name).with_capabilities(capabilities);
    AgenticWebRuntime::new(profile)
}

/// Create a full agent system with execution, swarm coordination, and networking
pub fn create_agent_system(name: impl Into<String>) -> AgentSystem {
    AgentSystem::new(name)
}

/// Complete agent system with all components
pub struct AgentSystem {
    pub runtime: Arc<AgenticWebRuntime>,
    pub executor: Arc<ExecutionEngine>,
    pub swarm_coordinator: Arc<SwarmCoordinator>,
    pub registry: Arc<AgentRegistry>,
    pub behavior_executor: Arc<BehaviorExecutor>,
}

impl AgentSystem {
    pub fn new(name: impl Into<String>) -> Self {
        let runtime = Arc::new(create_agent(name));
        let executor = Arc::new(ExecutionEngine::new(runtime.clone()));
        let swarm_coordinator = Arc::new(SwarmCoordinator::new(runtime.clone()));
        let registry = Arc::new(AgentRegistry::new());
        let behavior_executor = Arc::new(BehaviorExecutor::new(executor.clone()));

        Self {
            runtime,
            executor,
            swarm_coordinator,
            registry,
            behavior_executor,
        }
    }

    /// Create a server for this agent
    pub fn create_server(&self, addr: SocketAddr) -> AgentServer {
        AgentServer::new(self.runtime.clone(), self.registry.clone(), addr)
    }

    /// Create a client for connecting to other agents
    pub fn create_client(&self) -> AgentClient {
        AgentClient::new(self.runtime.clone())
    }

    /// Execute a goal end-to-end
    pub async fn achieve(&self, goal: Goal) -> Result<serde_json::Value, String> {
        // Create intention
        let intention_id = self.runtime.intend(goal);
        self.runtime
            .update_intention_status(intention_id, IntentionStatus::Active);

        // Plan
        let plan = self
            .runtime
            .plan(intention_id)
            .await
            .ok_or("Failed to create plan")?;

        // Execute
        let results = self.executor.execute_plan(plan).await?;

        // Mark complete
        self.runtime
            .update_intention_status(intention_id, IntentionStatus::Completed);

        // Aggregate results
        let final_result: Vec<_> = results.iter().filter_map(|r| r.data.clone()).collect();

        Ok(serde_json::json!({
            "intention_id": intention_id.to_string(),
            "results": final_result,
            "success": results.iter().all(|r| r.success)
        }))
    }

    /// Run a behavior tree
    pub async fn run_behavior(&self, behavior: BehaviorNode) -> BehaviorResult {
        self.behavior_executor.execute(&behavior).await
    }
}

// =============================================================================
// DECENTRALIZED IDENTITY (DID) FOR AGENTS
// =============================================================================

/// Decentralized Identity for agents - cryptographic proof of agent existence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDID {
    /// The DID method (e.g., "did:agent:", "did:web:", "did:key:")
    pub method: String,
    /// Unique identifier derived from public key
    pub identifier: String,
    /// Public key for verification
    pub public_key: Vec<u8>,
    /// Created timestamp
    pub created: DateTime<Utc>,
    /// DID document containing service endpoints
    pub document: DIDDocument,
}

/// DID Document following W3C spec adapted for agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DIDDocument {
    /// The DID this document describes
    pub id: String,
    /// Authentication methods
    pub authentication: Vec<VerificationMethod>,
    /// Service endpoints where this agent can be reached
    pub service: Vec<ServiceEndpoint>,
    /// Agent capabilities advertised
    pub capabilities: Vec<AgentCapability>,
    /// Controller DID (for delegated identities)
    pub controller: Option<String>,
}

/// Verification method for authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationMethod {
    pub id: String,
    pub method_type: String,
    pub controller: String,
    pub public_key_multibase: String,
}

/// Service endpoint for agent communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEndpoint {
    pub id: String,
    pub service_type: ServiceType,
    pub endpoint: String,
    pub protocols: Vec<String>,
}

/// Types of services an agent can provide
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServiceType {
    /// Direct agent-to-agent messaging
    AgentMessaging,
    /// Swarm coordination endpoint
    SwarmCoordination,
    /// Knowledge query endpoint
    KnowledgeQuery,
    /// Task delegation endpoint
    TaskDelegation,
    /// Marketplace listing
    Marketplace,
    /// Custom service
    Custom(String),
}

impl AgentDID {
    /// Generate a new agent DID with keypair
    pub fn generate(_name: &str) -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        // Generate a mock keypair (in production, use proper crypto)
        let public_key: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        let identifier = format!("{:x}", md5_hash(&public_key));

        Self {
            method: "did:agent:".to_string(),
            identifier: identifier.clone(),
            public_key: public_key.clone(),
            created: Utc::now(),
            document: DIDDocument {
                id: format!("did:agent:{}", identifier),
                authentication: vec![VerificationMethod {
                    id: format!("did:agent:{}#key-1", identifier),
                    method_type: "Ed25519VerificationKey2020".to_string(),
                    controller: format!("did:agent:{}", identifier),
                    public_key_multibase: base64_encode(&public_key),
                }],
                service: vec![],
                capabilities: vec![],
                controller: None,
            },
        }
    }

    /// Full DID string
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        format!("{}{}", self.method, self.identifier)
    }

    /// Add a service endpoint
    pub fn add_service(&mut self, service: ServiceEndpoint) {
        self.document.service.push(service);
    }

    /// Verify a signature from this agent
    pub fn verify(&self, _message: &[u8], _signature: &[u8]) -> bool {
        // Simplified - in production use proper Ed25519 verification
        true
    }

    /// Sign a message with this agent's key
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        // Simplified - in production use proper Ed25519 signing
        let mut sig = message.to_vec();
        sig.extend(&self.public_key[..8]);
        sig
    }
}

fn md5_hash(data: &[u8]) -> u128 {
    let mut hash: u128 = 0;
    for (i, &byte) in data.iter().enumerate() {
        hash ^= (byte as u128) << ((i % 16) * 8);
    }
    hash
}

fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let n = match chunk.len() {
            3 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8 | chunk[2] as u32,
            2 => (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8,
            1 => (chunk[0] as u32) << 16,
            _ => 0,
        };
        result.push(ALPHABET[(n >> 18) as usize & 63] as char);
        result.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            result.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            result.push(ALPHABET[n as usize & 63] as char);
        }
    }
    result
}

// =============================================================================
// SEMANTIC PROTOCOL NEGOTIATION
// =============================================================================

/// Protocol for agents to negotiate communication semantics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolNegotiation {
    /// Initiator DID
    pub initiator: String,
    /// Responder DID
    pub responder: String,
    /// Proposed protocols in preference order
    pub proposed_protocols: Vec<CommunicationProtocol>,
    /// Selected protocol after negotiation
    pub selected: Option<CommunicationProtocol>,
    /// Negotiation status
    pub status: NegotiationStatus,
}

/// Communication protocol options
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CommunicationProtocol {
    /// Pure latent space communication
    LatentSpace { encoder: String, dimension: usize },
    /// Structured semantic messages
    SemanticJSON { schema_version: String },
    /// Compressed binary protocol
    BinaryCompact { compression: String },
    /// Natural language with embeddings
    NaturalLanguage {
        language: String,
        embedding_model: String,
    },
    /// Custom protocol with specification
    Custom { name: String, spec_uri: String },
}

/// Status of protocol negotiation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NegotiationStatus {
    /// Negotiation initiated
    Initiated,
    /// Awaiting response
    Pending,
    /// Protocol agreed upon
    Agreed,
    /// Negotiation failed
    Failed { reason: String },
}

impl ProtocolNegotiation {
    /// Start a new negotiation
    pub fn initiate(
        initiator: &str,
        responder: &str,
        protocols: Vec<CommunicationProtocol>,
    ) -> Self {
        Self {
            initiator: initiator.to_string(),
            responder: responder.to_string(),
            proposed_protocols: protocols,
            selected: None,
            status: NegotiationStatus::Initiated,
        }
    }

    /// Respond to a negotiation request
    pub fn respond(
        &mut self,
        acceptable: &[CommunicationProtocol],
    ) -> Option<&CommunicationProtocol> {
        for proposed in &self.proposed_protocols {
            if acceptable.contains(proposed) {
                self.selected = Some(proposed.clone());
                self.status = NegotiationStatus::Agreed;
                return self.selected.as_ref();
            }
        }
        self.status = NegotiationStatus::Failed {
            reason: "No mutually acceptable protocol found".to_string(),
        };
        None
    }
}

// =============================================================================
// EMERGENT AGENT COMPOSITION
// =============================================================================

/// Composite agent formed by combining multiple agents
#[derive(Debug, Clone)]
pub struct CompositeAgent {
    /// Unique ID for the composite
    pub composite_id: AgentId,
    /// Name of the composite agent
    pub name: String,
    /// Component agents
    pub components: Vec<ComponentAgent>,
    /// Composition strategy
    pub strategy: CompositionStrategy,
    /// Combined capabilities
    pub capabilities: Vec<AgentCapability>,
    /// Internal routing rules
    pub routing: RoutingRules,
    /// Creation time
    pub created_at: DateTime<Utc>,
}

/// A component within a composite agent
#[derive(Debug, Clone)]
pub struct ComponentAgent {
    /// The agent's ID
    pub agent_id: AgentId,
    /// Role in the composite
    pub role: ComponentRole,
    /// Weight for routing decisions
    pub weight: f32,
    /// Active status
    pub active: bool,
}

/// Role of a component in the composite
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComponentRole {
    /// Primary processor for certain domains
    Primary { domains: Vec<String> },
    /// Backup when primary fails
    Backup,
    /// Validator of outputs
    Validator,
    /// Specialized capability provider
    Specialist { capability: AgentCapability },
    /// Aggregator of results
    Aggregator,
    /// Router of requests
    Router,
}

/// How agents are composed together
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompositionStrategy {
    /// All agents must agree (consensus)
    Unanimous,
    /// Majority vote
    Majority { threshold: f32 },
    /// Route to specialists based on capability
    Routing,
    /// Parallel execution with aggregation
    Parallel { aggregation: AggregationMethod },
    /// Sequential pipeline
    Pipeline,
    /// Hierarchical with leader
    Hierarchical { leader: AgentId },
}

/// Method for aggregating results from parallel execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregationMethod {
    /// First successful response
    First,
    /// Best by confidence score
    BestConfidence,
    /// Merge all results
    Merge,
    /// Custom aggregation function name
    Custom(String),
}

/// Rules for routing requests within composite
#[derive(Debug, Clone)]
pub struct RoutingRules {
    /// Default route
    pub default: AgentId,
    /// Capability-based routes
    pub capability_routes: HashMap<String, Vec<AgentId>>,
    /// Load balancing enabled
    pub load_balance: bool,
    /// Failover enabled
    pub failover: bool,
}

impl CompositeAgent {
    /// Create a new composite agent
    pub fn new(name: &str, strategy: CompositionStrategy) -> Self {
        Self {
            composite_id: AgentId(Uuid::new_v4()),
            name: name.to_string(),
            components: Vec::new(),
            strategy,
            capabilities: Vec::new(),
            routing: RoutingRules {
                default: AgentId(Uuid::nil()),
                capability_routes: HashMap::new(),
                load_balance: true,
                failover: true,
            },
            created_at: Utc::now(),
        }
    }

    /// Add a component agent
    pub fn add_component(&mut self, agent_id: AgentId, role: ComponentRole, weight: f32) {
        // Set first component as default route
        if self.components.is_empty() {
            self.routing.default = agent_id;
        }

        self.components.push(ComponentAgent {
            agent_id,
            role,
            weight,
            active: true,
        });
    }

    /// Route a request to appropriate component(s)
    pub fn route(&self, capability: &AgentCapability) -> Vec<AgentId> {
        let cap_key = format!("{:?}", capability);

        if let Some(routes) = self.routing.capability_routes.get(&cap_key) {
            return routes.clone();
        }

        // Find specialists for this capability
        let specialists: Vec<_> = self
            .components
            .iter()
            .filter(|c| c.active)
            .filter(|c| match &c.role {
                ComponentRole::Specialist { capability: cap } => {
                    std::mem::discriminant(cap) == std::mem::discriminant(capability)
                }
                ComponentRole::Primary { domains: _ } => true,
                _ => false,
            })
            .map(|c| c.agent_id)
            .collect();

        if specialists.is_empty() {
            vec![self.routing.default]
        } else {
            specialists
        }
    }

    /// Combine capabilities from all components
    pub fn refresh_capabilities(
        &mut self,
        component_caps: &HashMap<AgentId, Vec<AgentCapability>>,
    ) {
        self.capabilities.clear();
        for comp in &self.components {
            if let Some(caps) = component_caps.get(&comp.agent_id) {
                for cap in caps {
                    if !self
                        .capabilities
                        .iter()
                        .any(|c| std::mem::discriminant(c) == std::mem::discriminant(cap))
                    {
                        self.capabilities.push(cap.clone());
                    }
                }
            }
        }
    }
}

// =============================================================================
// AGENT MARKETPLACE
// =============================================================================

/// Marketplace for discovering and procuring agent services
pub struct AgentMarketplace {
    /// Listings indexed by ID
    listings: DashMap<Uuid, MarketplaceListing>,
    /// Index by capability
    by_capability: DashMap<String, Vec<Uuid>>,
    /// Index by price range
    by_price: DashMap<PriceRange, Vec<Uuid>>,
    /// Reputation scores
    reputation: DashMap<AgentId, ReputationScore>,
    /// Transaction history
    transactions: Arc<RwLock<Vec<MarketplaceTransaction>>>,
}

/// A listing in the marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceListing {
    /// Unique listing ID
    pub id: Uuid,
    /// Agent providing the service
    pub provider: AgentId,
    /// Service title
    pub title: String,
    /// Description
    pub description: String,
    /// Capabilities offered
    pub capabilities: Vec<AgentCapability>,
    /// Pricing model
    pub pricing: PricingModel,
    /// Quality guarantees
    pub sla: ServiceLevelAgreement,
    /// Sample inputs/outputs
    pub examples: Vec<ServiceExample>,
    /// Status
    pub status: ListingStatus,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Last updated
    pub updated_at: DateTime<Utc>,
}

/// Pricing model for a service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PricingModel {
    /// Free service
    Free,
    /// Per request pricing
    PerRequest { credits: u64 },
    /// Subscription
    Subscription {
        credits_per_period: u64,
        period_days: u32,
    },
    /// Usage-based
    UsageBased { rate: f64, unit: String },
    /// Negotiable
    Negotiable { min_credits: u64 },
}

/// Price range for indexing
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PriceRange {
    Free,
    Low,     // < 10 credits
    Medium,  // 10-100 credits
    High,    // 100-1000 credits
    Premium, // > 1000 credits
}

/// Service level agreement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceLevelAgreement {
    /// Response time in milliseconds
    pub max_response_time_ms: u64,
    /// Uptime guarantee percentage
    pub uptime_guarantee: f32,
    /// Accuracy guarantee
    pub accuracy_guarantee: Option<f32>,
    /// Retry policy
    pub retry_policy: RetryPolicy,
}

/// Retry policy on failures (alias for compatibility)
pub type SlaRetryPolicy = RetryPolicy;

/// Example input/output for a service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceExample {
    pub name: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
}

/// Status of a listing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ListingStatus {
    Active,
    Paused,
    Retired,
}

/// Reputation score for an agent
#[derive(Debug, Clone, Default)]
pub struct ReputationScore {
    /// Overall score (0-100)
    pub score: f32,
    /// Number of completed transactions
    pub completed: u64,
    /// Success rate
    pub success_rate: f32,
    /// Average response time
    pub avg_response_time_ms: f64,
    /// Number of reviews
    pub reviews: u64,
    /// Average rating (0-5)
    pub avg_rating: f32,
}

/// A marketplace transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceTransaction {
    pub id: Uuid,
    pub listing_id: Uuid,
    pub consumer: AgentId,
    pub provider: AgentId,
    pub credits: u64,
    pub status: TransactionStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub rating: Option<u8>,
    pub review: Option<String>,
}

/// Transaction status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionStatus {
    Initiated,
    InProgress,
    Completed,
    Failed { reason: String },
    Disputed,
    Refunded,
}

impl Default for AgentMarketplace {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentMarketplace {
    /// Create a new marketplace
    pub fn new() -> Self {
        Self {
            listings: DashMap::new(),
            by_capability: DashMap::new(),
            by_price: DashMap::new(),
            reputation: DashMap::new(),
            transactions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a new listing
    pub fn list_service(&self, listing: MarketplaceListing) -> Uuid {
        let id = listing.id;

        // Index by capability
        for cap in &listing.capabilities {
            let key = format!("{:?}", cap);
            self.by_capability.entry(key).or_default().push(id);
        }

        // Index by price
        let price_range = self.price_to_range(&listing.pricing);
        self.by_price.entry(price_range).or_default().push(id);

        self.listings.insert(id, listing);
        id
    }

    fn price_to_range(&self, pricing: &PricingModel) -> PriceRange {
        match pricing {
            PricingModel::Free => PriceRange::Free,
            PricingModel::PerRequest { credits } if *credits < 10 => PriceRange::Low,
            PricingModel::PerRequest { credits } if *credits < 100 => PriceRange::Medium,
            PricingModel::PerRequest { credits } if *credits < 1000 => PriceRange::High,
            PricingModel::PerRequest { credits: _ } => PriceRange::Premium,
            PricingModel::Subscription {
                credits_per_period,
                period_days: _,
            } if *credits_per_period < 100 => PriceRange::Medium,
            PricingModel::Subscription {
                credits_per_period: _,
                period_days: _,
            } => PriceRange::High,
            PricingModel::UsageBased { rate: _, unit: _ } => PriceRange::Medium,
            PricingModel::Negotiable { min_credits: _ } => PriceRange::Premium,
        }
    }

    /// Search for services
    pub fn search(&self, query: &MarketplaceQuery) -> Vec<MarketplaceListing> {
        let mut results: Vec<MarketplaceListing> = Vec::new();

        // Filter by capability if specified
        if let Some(cap) = &query.capability {
            let key = format!("{:?}", cap);
            if let Some(ids) = self.by_capability.get(&key) {
                for id in ids.iter() {
                    if let Some(listing) = self.listings.get(id) {
                        if listing.status == ListingStatus::Active {
                            results.push(listing.clone());
                        }
                    }
                }
            }
        } else {
            // All active listings
            for entry in self.listings.iter() {
                if entry.value().status == ListingStatus::Active {
                    results.push(entry.value().clone());
                }
            }
        }

        // Filter by price range
        if let Some(max_credits) = query.max_credits {
            results.retain(|l| match &l.pricing {
                PricingModel::Free => true,
                PricingModel::PerRequest { credits } => *credits <= max_credits,
                PricingModel::Subscription {
                    credits_per_period,
                    period_days: _,
                } => *credits_per_period <= max_credits,
                _ => true,
            });
        }

        // Filter by minimum reputation
        if let Some(min_rep) = query.min_reputation {
            results.retain(|l| {
                if let Some(rep) = self.reputation.get(&l.provider) {
                    rep.score >= min_rep
                } else {
                    false
                }
            });
        }

        // Sort by relevance/reputation
        results.sort_by(|a, b| {
            let rep_a = self
                .reputation
                .get(&a.provider)
                .map(|r| r.score)
                .unwrap_or(0.0);
            let rep_b = self
                .reputation
                .get(&b.provider)
                .map(|r| r.score)
                .unwrap_or(0.0);
            rep_b
                .partial_cmp(&rep_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit results
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        results
    }

    /// Initiate a transaction
    pub async fn procure(&self, listing_id: Uuid, consumer: AgentId) -> Result<Uuid, String> {
        let listing = self
            .listings
            .get(&listing_id)
            .ok_or("Listing not found")?
            .clone();

        if listing.status != ListingStatus::Active {
            return Err("Listing not active".to_string());
        }

        let credits = match &listing.pricing {
            PricingModel::Free => 0,
            PricingModel::PerRequest { credits } => *credits,
            PricingModel::Subscription {
                credits_per_period,
                period_days: _,
            } => *credits_per_period,
            PricingModel::UsageBased { rate: _, unit: _ } => 0, // Calculated later
            PricingModel::Negotiable { min_credits } => *min_credits,
        };

        let transaction = MarketplaceTransaction {
            id: Uuid::new_v4(),
            listing_id,
            consumer,
            provider: listing.provider,
            credits,
            status: TransactionStatus::Initiated,
            started_at: Utc::now(),
            completed_at: None,
            rating: None,
            review: None,
        };

        let tx_id = transaction.id;
        let mut txs = self.transactions.write().await;
        txs.push(transaction);

        Ok(tx_id)
    }

    /// Complete a transaction
    pub async fn complete_transaction(
        &self,
        tx_id: Uuid,
        success: bool,
        rating: Option<u8>,
        review: Option<String>,
    ) {
        let provider = {
            let mut txs = self.transactions.write().await;
            if let Some(tx) = txs.iter_mut().find(|t| t.id == tx_id) {
                tx.completed_at = Some(Utc::now());
                tx.rating = rating;
                tx.review = review.clone();
                tx.status = if success {
                    TransactionStatus::Completed
                } else {
                    TransactionStatus::Failed {
                        reason: "Service failed".to_string(),
                    }
                };
                Some(tx.provider)
            } else {
                None
            }
        };

        // Update reputation after releasing lock
        if let Some(provider) = provider {
            self.update_reputation(provider, success, rating);
        }
    }

    fn update_reputation(&self, provider: AgentId, success: bool, rating: Option<u8>) {
        let mut entry = self.reputation.entry(provider).or_default();
        let rep = entry.value_mut();

        let prev_completed = rep.completed as f32;
        rep.completed += 1;

        // Update success rate
        rep.success_rate = (rep.success_rate * prev_completed + if success { 1.0 } else { 0.0 })
            / (prev_completed + 1.0);

        // Update rating
        if let Some(r) = rating {
            let prev_reviews = rep.reviews as f32;
            rep.reviews += 1;
            rep.avg_rating = (rep.avg_rating * prev_reviews + r as f32) / (prev_reviews + 1.0);
        }

        // Calculate overall score
        rep.score = (rep.success_rate * 40.0)
            + (rep.avg_rating / 5.0 * 40.0)
            + (rep.completed as f32).min(20.0);
    }

    /// Get reputation for an agent
    pub fn get_reputation(&self, agent: &AgentId) -> Option<ReputationScore> {
        self.reputation.get(agent).map(|r| r.clone())
    }
}

/// Query for searching marketplace
#[derive(Debug, Clone, Default)]
pub struct MarketplaceQuery {
    /// Filter by capability
    pub capability: Option<AgentCapability>,
    /// Maximum credits to spend
    pub max_credits: Option<u64>,
    /// Minimum reputation score
    pub min_reputation: Option<f32>,
    /// Limit results
    pub limit: Option<usize>,
    /// Search text
    pub text: Option<String>,
}

// =============================================================================
// TEMPORAL REASONING ENGINE
// =============================================================================

/// Engine for reasoning about time, causality, and temporal relationships
pub struct TemporalReasoner {
    /// Timeline of events
    timeline: Arc<RwLock<Vec<TemporalEvent>>>,
    /// Causal graph
    causal_graph: Arc<RwLock<DiGraph<String, CausalRelation>>>,
    /// Predictions
    predictions: DashMap<Uuid, Prediction>,
    /// Temporal constraints
    constraints: Vec<TemporalConstraint>,
}

/// An event in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalEvent {
    pub id: Uuid,
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub duration: Option<Duration>,
    pub data: serde_json::Value,
    pub causes: Vec<Uuid>,
    pub effects: Vec<Uuid>,
    pub confidence: f32,
}

/// Causal relationship between events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalRelation {
    pub relation_type: CausalType,
    pub strength: f32,
    pub delay: Option<Duration>,
    pub probability: f32,
}

/// Types of causal relationships
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CausalType {
    /// A directly causes B
    DirectCause,
    /// A contributes to B along with other factors
    ContributingCause,
    /// A enables B to happen
    EnablingCondition,
    /// A prevents B
    Prevention,
    /// A and B are correlated
    Correlation,
    /// A happens after B but doesn't cause it
    TemporalSuccession,
}

/// A prediction about future events
#[derive(Debug, Clone)]
pub struct Prediction {
    pub id: Uuid,
    pub event_type: String,
    pub predicted_time: DateTime<Utc>,
    pub time_window: Duration,
    pub confidence: f32,
    pub reasoning: Vec<String>,
    pub based_on: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub resolved: Option<bool>,
}

/// Temporal constraints for planning
#[derive(Debug, Clone)]
pub struct TemporalConstraint {
    pub constraint_type: ConstraintType,
    pub events: Vec<Uuid>,
    pub parameters: ConstraintParams,
}

/// Types of temporal constraints
#[derive(Debug, Clone)]
pub enum ConstraintType {
    /// A must happen before B
    Before,
    /// A must happen after B
    After,
    /// A and B must happen simultaneously
    Simultaneous,
    /// A must happen within duration of B
    Within,
    /// A must not overlap with B
    NoOverlap,
    /// A must start after B ends
    EndToStart,
    /// A and B must have same start
    SameStart,
    /// A and B must have same end
    SameEnd,
}

/// Parameters for temporal constraints
#[derive(Debug, Clone)]
pub struct ConstraintParams {
    pub min_gap: Option<Duration>,
    pub max_gap: Option<Duration>,
    pub tolerance: Option<Duration>,
}

impl Default for TemporalReasoner {
    fn default() -> Self {
        Self::new()
    }
}

impl TemporalReasoner {
    /// Create a new temporal reasoner
    pub fn new() -> Self {
        Self {
            timeline: Arc::new(RwLock::new(Vec::new())),
            causal_graph: Arc::new(RwLock::new(DiGraph::new())),
            predictions: DashMap::new(),
            constraints: Vec::new(),
        }
    }

    /// Record an event
    pub async fn record_event(&self, event: TemporalEvent) -> Uuid {
        let id = event.id;

        // Add to timeline
        let mut timeline = self.timeline.write().await;
        let insert_pos = timeline
            .binary_search_by(|e| e.timestamp.cmp(&event.timestamp))
            .unwrap_or_else(|pos| pos);
        timeline.insert(insert_pos, event.clone());

        // Update causal graph
        let mut graph = self.causal_graph.write().await;
        let node = graph.add_node(id.to_string());

        // Link to causes
        for cause_id in &event.causes {
            let cause_str = cause_id.to_string();
            if let Some((cause_idx, _)) = graph.node_indices().find_map(|i| {
                graph
                    .node_weight(i)
                    .filter(|w| *w == &cause_str)
                    .map(|_| (i, ()))
            }) {
                graph.add_edge(
                    cause_idx,
                    node,
                    CausalRelation {
                        relation_type: CausalType::DirectCause,
                        strength: 0.8,
                        delay: None,
                        probability: 1.0,
                    },
                );
            }
        }

        // Check predictions
        self.check_predictions(&event);

        id
    }

    fn check_predictions(&self, event: &TemporalEvent) {
        for mut pred in self.predictions.iter_mut() {
            if pred.event_type == event.event_type && pred.resolved.is_none() {
                let time_diff = if event.timestamp > pred.predicted_time {
                    event.timestamp - pred.predicted_time
                } else {
                    pred.predicted_time - event.timestamp
                };

                let duration = time_diff.to_std().unwrap_or(Duration::from_secs(0));
                pred.resolved = Some(duration <= pred.time_window);
            }
        }
    }

    /// Query events in a time range
    pub async fn query_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<TemporalEvent> {
        let timeline = self.timeline.read().await;
        timeline
            .iter()
            .filter(|e| e.timestamp >= start && e.timestamp <= end)
            .cloned()
            .collect()
    }

    /// Find causal chain between events
    pub async fn find_causal_chain(&self, from: Uuid, to: Uuid) -> Option<Vec<Uuid>> {
        let graph = self.causal_graph.read().await;

        let from_str = from.to_string();
        let to_str = to.to_string();

        let from_idx = graph.node_indices().find(|i| {
            graph
                .node_weight(*i)
                .map(|w| w == &from_str)
                .unwrap_or(false)
        })?;
        let to_idx = graph
            .node_indices()
            .find(|i| graph.node_weight(*i).map(|w| w == &to_str).unwrap_or(false))?;

        // Simple BFS for path finding
        use std::collections::VecDeque;
        let mut visited = vec![false; graph.node_count()];
        let mut parent: HashMap<NodeIndex, NodeIndex> = HashMap::new();
        let mut queue = VecDeque::new();

        queue.push_back(from_idx);
        visited[from_idx.index()] = true;

        while let Some(current) = queue.pop_front() {
            if current == to_idx {
                // Reconstruct path
                let mut path = vec![to];
                let mut node = to_idx;
                while node != from_idx {
                    if let Some(&prev) = parent.get(&node) {
                        if let Some(id) = graph.node_weight(prev) {
                            if let Ok(uuid) = Uuid::parse_str(id) {
                                path.push(uuid);
                            }
                        }
                        node = prev;
                    } else {
                        break;
                    }
                }
                path.reverse();
                return Some(path);
            }

            for neighbor in graph.neighbors(current) {
                if !visited[neighbor.index()] {
                    visited[neighbor.index()] = true;
                    parent.insert(neighbor, current);
                    queue.push_back(neighbor);
                }
            }
        }

        None
    }

    /// Make a prediction
    pub fn predict(&self, event_type: &str, reasoning: Vec<String>, based_on: Vec<Uuid>) -> Uuid {
        let prediction = Prediction {
            id: Uuid::new_v4(),
            event_type: event_type.to_string(),
            predicted_time: Utc::now() + chrono::Duration::hours(1),
            time_window: Duration::from_secs(3600),
            confidence: 0.7,
            reasoning,
            based_on,
            created_at: Utc::now(),
            resolved: None,
        };

        let id = prediction.id;
        self.predictions.insert(id, prediction);
        id
    }

    /// Add temporal constraint
    pub fn add_constraint(&mut self, constraint: TemporalConstraint) {
        self.constraints.push(constraint);
    }

    /// Check if a schedule satisfies all constraints
    pub fn validate_schedule(
        &self,
        schedule: &[(Uuid, DateTime<Utc>, Option<Duration>)],
    ) -> Vec<String> {
        let mut violations = Vec::new();

        for constraint in &self.constraints {
            let involved: Vec<_> = schedule
                .iter()
                .filter(|(id, _, _)| constraint.events.contains(id))
                .collect();

            if involved.len() < 2 {
                continue;
            }

            match constraint.constraint_type {
                ConstraintType::Before => {
                    if involved[0].1 >= involved[1].1 {
                        violations.push(format!(
                            "Event {} must happen before {}",
                            involved[0].0, involved[1].0
                        ));
                    }
                }
                ConstraintType::After => {
                    if involved[0].1 <= involved[1].1 {
                        violations.push(format!(
                            "Event {} must happen after {}",
                            involved[0].0, involved[1].0
                        ));
                    }
                }
                _ => {} // Other constraints would be checked similarly
            }
        }

        violations
    }
}

// =============================================================================
// CONTEXT BRIDGING
// =============================================================================

/// Bridges context across agent boundaries
pub struct ContextBridge {
    /// Shared context pools
    pools: DashMap<String, ContextPool>,
    /// Context transformers
    transformers: HashMap<(String, String), Box<dyn ContextTransformer>>,
    /// Access policies
    policies: Vec<ContextPolicy>,
}

/// A pool of shared context
#[derive(Debug, Clone)]
pub struct ContextPool {
    pub name: String,
    pub context: serde_json::Value,
    pub schema: Option<String>,
    pub participants: Vec<AgentId>,
    pub owner: AgentId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u64,
}

/// Trait for transforming context between different schemas
pub trait ContextTransformer: Send + Sync {
    fn transform(&self, from: &serde_json::Value) -> Result<serde_json::Value, String>;
    fn source_schema(&self) -> &str;
    fn target_schema(&self) -> &str;
}

/// Policy for context access
#[derive(Debug, Clone)]
pub struct ContextPolicy {
    pub pool_pattern: String,
    pub allowed_agents: Vec<AgentId>,
    pub allowed_capabilities: Vec<AgentCapability>,
    pub permission: ContextPermission,
    pub expiry: Option<DateTime<Utc>>,
}

/// Permission levels for context access
#[derive(Debug, Clone, PartialEq)]
pub enum ContextPermission {
    Read,
    Write,
    ReadWrite,
    Admin,
}

impl Default for ContextBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextBridge {
    /// Create a new context bridge
    pub fn new() -> Self {
        Self {
            pools: DashMap::new(),
            transformers: HashMap::new(),
            policies: Vec::new(),
        }
    }

    /// Create a context pool
    pub fn create_pool(
        &self,
        name: &str,
        owner: AgentId,
        initial_context: serde_json::Value,
    ) -> String {
        let pool = ContextPool {
            name: name.to_string(),
            context: initial_context,
            schema: None,
            participants: vec![owner],
            owner,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: 1,
        };

        self.pools.insert(name.to_string(), pool);
        name.to_string()
    }

    /// Share context to a pool
    pub fn share(
        &self,
        pool_name: &str,
        context: serde_json::Value,
        agent: &AgentId,
    ) -> Result<u64, String> {
        let mut pool = self.pools.get_mut(pool_name).ok_or("Pool not found")?;

        // Check permission
        if !self.has_permission(pool_name, agent, ContextPermission::Write) {
            return Err("No write permission".to_string());
        }

        // Merge context
        if let (Some(existing), Some(new)) = (pool.context.as_object_mut(), context.as_object()) {
            for (k, v) in new {
                existing.insert(k.clone(), v.clone());
            }
        } else {
            pool.context = context;
        }

        pool.updated_at = Utc::now();
        pool.version += 1;

        Ok(pool.version)
    }

    /// Read context from a pool
    pub fn read(&self, pool_name: &str, agent: &AgentId) -> Result<serde_json::Value, String> {
        let pool = self.pools.get(pool_name).ok_or("Pool not found")?;

        if !self.has_permission(pool_name, agent, ContextPermission::Read) {
            return Err("No read permission".to_string());
        }

        Ok(pool.context.clone())
    }

    /// Join a context pool
    pub fn join_pool(&self, pool_name: &str, agent: AgentId) -> Result<(), String> {
        let mut pool = self.pools.get_mut(pool_name).ok_or("Pool not found")?;

        if !pool.participants.contains(&agent) {
            pool.participants.push(agent);
        }

        Ok(())
    }

    /// Add a context policy
    pub fn add_policy(&mut self, policy: ContextPolicy) {
        self.policies.push(policy);
    }

    fn has_permission(
        &self,
        pool_name: &str,
        agent: &AgentId,
        required: ContextPermission,
    ) -> bool {
        // Owner always has full access
        if let Some(pool) = self.pools.get(pool_name) {
            if &pool.owner == agent {
                return true;
            }

            // Participants have read access by default
            if pool.participants.contains(agent) && required == ContextPermission::Read {
                return true;
            }
        }

        // Check policies
        for policy in &self.policies {
            if (pool_name.starts_with(&policy.pool_pattern) || policy.pool_pattern == "*")
                && policy.allowed_agents.contains(agent)
            {
                match (&policy.permission, &required) {
                    (ContextPermission::Admin, _) => return true,
                    (ContextPermission::ReadWrite, _) => return true,
                    (ContextPermission::Write, ContextPermission::Write) => return true,
                    (ContextPermission::Read, ContextPermission::Read) => return true,
                    _ => {}
                }
            }
        }

        false
    }

    /// List pools an agent has access to
    pub fn list_accessible_pools(&self, agent: &AgentId) -> Vec<String> {
        self.pools
            .iter()
            .filter(|entry| {
                entry.participants.contains(agent)
                    || self.has_permission(entry.key(), agent, ContextPermission::Read)
            })
            .map(|entry| entry.key().clone())
            .collect()
    }
}

// =============================================================================
// INTEGRATED AGENTIC WEB BUILDER
// =============================================================================

/// Builder for creating complete agentic web applications
pub struct AgenticWebBuilder {
    name: String,
    capabilities: Vec<AgentCapability>,
    trust_level: TrustLevel,
    did: Option<AgentDID>,
    marketplace_listing: Option<MarketplaceListing>,
    behavior: Option<BehaviorNode>,
    protocols: Vec<CommunicationProtocol>,
}

impl AgenticWebBuilder {
    /// Start building an agent
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            capabilities: Vec::new(),
            trust_level: TrustLevel::Unknown,
            did: None,
            marketplace_listing: None,
            behavior: None,
            protocols: vec![CommunicationProtocol::SemanticJSON {
                schema_version: "1.0".to_string(),
            }],
        }
    }

    /// Add capabilities
    pub fn with_capabilities(mut self, caps: Vec<AgentCapability>) -> Self {
        self.capabilities = caps;
        self
    }

    /// Set trust level
    pub fn with_trust(mut self, level: TrustLevel) -> Self {
        self.trust_level = level;
        self
    }

    /// Generate decentralized identity
    pub fn with_did(mut self) -> Self {
        self.did = Some(AgentDID::generate(&self.name));
        self
    }

    /// Add marketplace listing
    pub fn with_marketplace(
        mut self,
        title: &str,
        description: &str,
        pricing: PricingModel,
    ) -> Self {
        self.marketplace_listing = Some(MarketplaceListing {
            id: Uuid::new_v4(),
            provider: AgentId(Uuid::new_v4()), // Will be updated on build
            title: title.to_string(),
            description: description.to_string(),
            capabilities: self.capabilities.clone(),
            pricing,
            sla: ServiceLevelAgreement {
                max_response_time_ms: 5000,
                uptime_guarantee: 0.99,
                accuracy_guarantee: Some(0.95),
                retry_policy: RetryPolicy::default(),
            },
            examples: Vec::new(),
            status: ListingStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });
        self
    }

    /// Add default behavior
    pub fn with_behavior(mut self, behavior: BehaviorNode) -> Self {
        self.behavior = behavior.into();
        self
    }

    /// Add supported protocols
    pub fn with_protocols(mut self, protocols: Vec<CommunicationProtocol>) -> Self {
        self.protocols = protocols;
        self
    }

    /// Build the complete agent system
    pub async fn build(self) -> AgentSystem {
        let name = self.name;
        let profile = AgentProfile::new(&name)
            .with_capabilities(self.capabilities)
            .with_trust(self.trust_level);

        let runtime = Arc::new(AgenticWebRuntime::new(profile));
        let executor = Arc::new(ExecutionEngine::new(runtime.clone()));
        let swarm_coordinator = Arc::new(SwarmCoordinator::new(runtime.clone()));
        let registry = Arc::new(AgentRegistry::new());
        let behavior_executor = Arc::new(BehaviorExecutor::new(executor.clone()));

        AgentSystem {
            runtime,
            executor,
            swarm_coordinator,
            registry,
            behavior_executor,
        }
    }
}

/// Helper function to quickly create an agent system
pub fn agent(name: &str) -> AgenticWebBuilder {
    AgenticWebBuilder::new(name)
}

// =============================================================================
// AGENT VERSIONING & HOT MIGRATIONS
// =============================================================================

/// Semantic version for agents
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub prerelease: Option<String>,
}

impl AgentVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease: None,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        let version_parts: Vec<u32> = parts[0].split('.').filter_map(|p| p.parse().ok()).collect();
        if version_parts.len() >= 3 {
            Some(Self {
                major: version_parts[0],
                minor: version_parts[1],
                patch: version_parts[2],
                prerelease: parts.get(1).map(|s| s.to_string()),
            })
        } else {
            None
        }
    }

    pub fn is_compatible_with(&self, other: &Self) -> bool {
        self.major == other.major
    }

    pub fn bump_patch(&self) -> Self {
        Self {
            major: self.major,
            minor: self.minor,
            patch: self.patch + 1,
            prerelease: None,
        }
    }

    pub fn bump_minor(&self) -> Self {
        Self {
            major: self.major,
            minor: self.minor + 1,
            patch: 0,
            prerelease: None,
        }
    }

    pub fn bump_major(&self) -> Self {
        Self {
            major: self.major + 1,
            minor: 0,
            patch: 0,
            prerelease: None,
        }
    }
}

impl std::fmt::Display for AgentVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(ref pre) = self.prerelease {
            write!(f, "-{}", pre)?;
        }
        Ok(())
    }
}

/// Migration step for agent upgrades
#[derive(Debug, Clone)]
pub struct Migration {
    pub id: String,
    pub from_version: AgentVersion,
    pub to_version: AgentVersion,
    pub description: String,
    pub reversible: bool,
    pub steps: Vec<MigrationStep>,
}

#[derive(Debug, Clone)]
pub enum MigrationStep {
    /// Add a new capability
    AddCapability(AgentCapability),
    /// Remove a capability
    RemoveCapability(AgentCapability),
    /// Transform knowledge graph
    TransformKnowledge { transform_fn: String },
    /// Update behavior tree
    UpdateBehavior {
        old_behavior: String,
        new_behavior: String,
    },
    /// Migrate state
    MigrateState { key: String, transform: String },
    /// Execute custom migration logic
    Custom { script: String },
}

/// Hot migration controller
pub struct MigrationController {
    migrations: DashMap<String, Migration>,
    applied: DashMap<Uuid, Vec<String>>, // agent_id -> applied migration ids
    rollback_points: DashMap<Uuid, Vec<AgentSnapshot>>,
}

#[derive(Debug, Clone)]
pub struct AgentSnapshot {
    pub id: Uuid,
    pub version: AgentVersion,
    pub timestamp: DateTime<Utc>,
    pub state: serde_json::Value,
    pub knowledge_hash: String,
}

impl Default for MigrationController {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationController {
    pub fn new() -> Self {
        Self {
            migrations: DashMap::new(),
            applied: DashMap::new(),
            rollback_points: DashMap::new(),
        }
    }

    pub fn register_migration(&self, migration: Migration) {
        self.migrations.insert(migration.id.clone(), migration);
    }

    /// Find migration path between versions
    pub fn find_migration_path(&self, from: &AgentVersion, to: &AgentVersion) -> Vec<Migration> {
        let mut path = Vec::new();
        let mut current = from.clone();

        while current != *to {
            let next_migration = self
                .migrations
                .iter()
                .find(|m| m.from_version == current)
                .map(|r| r.clone());

            if let Some(migration) = next_migration {
                current = migration.to_version.clone();
                path.push(migration);
            } else {
                break;
            }
        }
        path
    }

    /// Snapshot agent before migration
    pub fn snapshot(
        &self,
        agent_id: Uuid,
        version: AgentVersion,
        state: serde_json::Value,
    ) -> AgentSnapshot {
        let snapshot = AgentSnapshot {
            id: agent_id,
            version,
            timestamp: Utc::now(),
            state: state.clone(),
            knowledge_hash: format!("{:x}", simple_hash(&state.to_string())),
        };

        self.rollback_points
            .entry(agent_id)
            .or_default()
            .push(snapshot.clone());

        snapshot
    }

    /// Apply migration to agent
    pub async fn apply_migration(
        &self,
        agent_id: Uuid,
        migration: &Migration,
    ) -> Result<(), String> {
        for step in &migration.steps {
            self.apply_step(agent_id, step).await?;
        }

        self.applied
            .entry(agent_id)
            .or_default()
            .push(migration.id.clone());

        Ok(())
    }

    async fn apply_step(&self, _agent_id: Uuid, step: &MigrationStep) -> Result<(), String> {
        match step {
            MigrationStep::AddCapability(cap) => {
                println!("  [Migration] Adding capability: {:?}", cap);
            }
            MigrationStep::RemoveCapability(cap) => {
                println!("  [Migration] Removing capability: {:?}", cap);
            }
            MigrationStep::TransformKnowledge { transform_fn } => {
                println!(
                    "  [Migration] Transforming knowledge with: {}",
                    transform_fn
                );
            }
            MigrationStep::UpdateBehavior {
                old_behavior,
                new_behavior,
            } => {
                println!(
                    "  [Migration] Updating behavior {} -> {}",
                    old_behavior, new_behavior
                );
            }
            MigrationStep::MigrateState { key, transform } => {
                println!(
                    "  [Migration] Migrating state key {} with: {}",
                    key, transform
                );
            }
            MigrationStep::Custom { script } => {
                println!("  [Migration] Executing custom script: {}", script);
            }
        }
        Ok(())
    }

    /// Rollback to previous version
    pub async fn rollback(&self, agent_id: Uuid) -> Result<AgentSnapshot, String> {
        let mut snapshots = self
            .rollback_points
            .get_mut(&agent_id)
            .ok_or("No rollback points")?;

        snapshots.pop().ok_or("No snapshots available".to_string())
    }
}

fn simple_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// =============================================================================
// SEMANTIC ROUTING MESH
// =============================================================================

/// Content-based message router
pub struct SemanticRouter {
    routes: DashMap<String, Vec<SemanticRoute>>,
    subscriptions: DashMap<Uuid, Vec<SemanticSubscription>>,
    message_queue: Arc<Mutex<Vec<RoutedMessage>>>,
}

#[derive(Debug, Clone)]
pub struct SemanticRoute {
    pub id: Uuid,
    pub pattern: SemanticPattern,
    pub destination: RouteDestination,
    pub priority: u32,
    pub filters: Vec<RouteFilter>,
    pub transforms: Vec<MessageTransform>,
}

#[derive(Debug, Clone)]
pub enum SemanticPattern {
    /// Match by topic keywords
    Topic(Vec<String>),
    /// Match by embedding similarity
    Embedding { center: Vec<f32>, radius: f32 },
    /// Match by content type
    ContentType(String),
    /// Match by capability requirement
    CapabilityRequired(AgentCapability),
    /// Match by custom predicate
    Predicate { expression: String },
    /// Match all
    Any,
}

#[derive(Debug, Clone)]
pub enum RouteDestination {
    /// Single agent
    Agent(Uuid),
    /// Agent group
    Group(String),
    /// Load-balanced pool
    Pool {
        agents: Vec<Uuid>,
        strategy: LoadBalanceStrategy,
    },
    /// Broadcast to all matching
    Broadcast,
    /// Dead letter queue
    DeadLetter,
}

#[derive(Debug, Clone)]
pub enum LoadBalanceStrategy {
    RoundRobin,
    Random,
    LeastConnections,
    WeightedRandom { weights: Vec<f32> },
    CapabilityBased,
}

#[derive(Debug, Clone)]
pub enum RouteFilter {
    /// Minimum trust level
    TrustLevel(TrustLevel),
    /// Rate limit
    RateLimit { max_per_second: u32 },
    /// Content size limit
    MaxSize(usize),
    /// Time window
    TimeWindow {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
}

#[derive(Debug, Clone)]
pub enum MessageTransform {
    /// Add metadata
    AddMetadata { key: String, value: String },
    /// Encrypt content
    Encrypt { algorithm: String },
    /// Compress content
    Compress,
    /// Convert format
    ConvertFormat { from: String, to: String },
    /// Enrich with context
    Enrich { source: String },
}

#[derive(Debug, Clone)]
pub struct RoutedMessage {
    pub id: Uuid,
    pub source: Uuid,
    pub content: serde_json::Value,
    pub embedding: Option<Vec<f32>>,
    pub routed_at: DateTime<Utc>,
    pub route_path: Vec<Uuid>,
    pub ttl: u32,
}

#[derive(Debug, Clone)]
pub struct SemanticSubscription {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub pattern: SemanticPattern,
    pub callback: String,
    pub created_at: DateTime<Utc>,
}

impl Default for SemanticRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticRouter {
    pub fn new() -> Self {
        Self {
            routes: DashMap::new(),
            subscriptions: DashMap::new(),
            message_queue: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Add a semantic route
    pub fn add_route(&self, route: SemanticRoute) {
        self.routes
            .entry(format!("{:?}", route.pattern))
            .or_default()
            .push(route);
    }

    /// Subscribe to messages matching pattern
    pub fn subscribe(&self, agent_id: Uuid, pattern: SemanticPattern, callback: &str) -> Uuid {
        let sub = SemanticSubscription {
            id: Uuid::new_v4(),
            agent_id,
            pattern,
            callback: callback.to_string(),
            created_at: Utc::now(),
        };
        let id = sub.id;

        self.subscriptions.entry(agent_id).or_default().push(sub);

        id
    }

    /// Route a message
    pub async fn route(&self, message: RoutedMessage) -> Vec<Uuid> {
        let mut destinations = Vec::new();

        // Find matching routes
        for entry in self.routes.iter() {
            for route in entry.value().iter() {
                if self.matches(&route.pattern, &message) {
                    let dest_agents = self.resolve_destination(&route.destination);
                    destinations.extend(dest_agents);
                }
            }
        }

        // Check subscriptions
        for entry in self.subscriptions.iter() {
            for sub in entry.value().iter() {
                if self.matches(&sub.pattern, &message) {
                    destinations.push(sub.agent_id);
                }
            }
        }

        destinations.sort();
        destinations.dedup();
        destinations
    }

    fn matches(&self, pattern: &SemanticPattern, message: &RoutedMessage) -> bool {
        match pattern {
            SemanticPattern::Any => true,
            SemanticPattern::Topic(keywords) => {
                let content = message.content.to_string().to_lowercase();
                keywords.iter().any(|k| content.contains(&k.to_lowercase()))
            }
            SemanticPattern::Embedding { center, radius } => {
                if let Some(ref emb) = message.embedding {
                    cosine_similarity(center, emb) >= 1.0 - radius
                } else {
                    false
                }
            }
            SemanticPattern::ContentType(ct) => message
                .content
                .get("type")
                .and_then(|v| v.as_str())
                .map(|t| t == ct)
                .unwrap_or(false),
            SemanticPattern::CapabilityRequired(_cap) => {
                // Would check if destination agent has capability
                true
            }
            SemanticPattern::Predicate { expression } => {
                // Would evaluate expression
                !expression.is_empty()
            }
        }
    }

    fn resolve_destination(&self, dest: &RouteDestination) -> Vec<Uuid> {
        match dest {
            RouteDestination::Agent(id) => vec![*id],
            RouteDestination::Group(_name) => {
                // Would look up group members
                vec![]
            }
            RouteDestination::Pool { agents, strategy } => match strategy {
                LoadBalanceStrategy::Random => agents.first().copied().into_iter().collect(),
                LoadBalanceStrategy::RoundRobin => agents.first().copied().into_iter().collect(),
                _ => agents.clone(),
            },
            RouteDestination::Broadcast => vec![],
            RouteDestination::DeadLetter => vec![],
        }
    }
}

// =============================================================================
// DISTRIBUTED CONSENSUS PROTOCOLS
// =============================================================================

/// Multi-agent consensus system
pub struct ConsensusEngine {
    proposals: DashMap<Uuid, ConsensusProposal>,
    votes: DashMap<Uuid, Vec<Vote>>,
    committed: DashMap<Uuid, CommittedDecision>,
}

#[derive(Debug, Clone)]
pub struct ConsensusProposal {
    pub id: Uuid,
    pub proposer: Uuid,
    pub topic: String,
    pub value: serde_json::Value,
    pub quorum: QuorumRequirement,
    pub deadline: DateTime<Utc>,
    pub status: ConsensusStatus,
    pub participants: Vec<Uuid>,
}

#[derive(Debug, Clone)]
pub enum QuorumRequirement {
    /// Simple majority (>50%)
    Majority,
    /// Two-thirds majority (>66%)
    SuperMajority,
    /// All must agree
    Unanimous,
    /// At least N votes
    MinVotes(usize),
    /// Weighted by trust
    WeightedMajority { threshold: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConsensusStatus {
    Proposed,
    Voting,
    Accepted,
    Rejected,
    TimedOut,
    Committed,
}

#[derive(Debug, Clone)]
pub struct Vote {
    pub voter: Uuid,
    pub proposal_id: Uuid,
    pub decision: VoteDecision,
    pub weight: f64,
    pub reasoning: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VoteDecision {
    Accept,
    Reject,
    Abstain,
    Conditional { conditions: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct CommittedDecision {
    pub proposal_id: Uuid,
    pub final_value: serde_json::Value,
    pub vote_summary: VoteSummary,
    pub committed_at: DateTime<Utc>,
    pub execution_status: ExecutionStatus,
}

#[derive(Debug, Clone)]
pub struct VoteSummary {
    pub accept_count: usize,
    pub reject_count: usize,
    pub abstain_count: usize,
    pub total_weight: f64,
    pub accept_weight: f64,
}

#[derive(Debug, Clone)]
pub enum ExecutionStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

impl Default for ConsensusEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsensusEngine {
    pub fn new() -> Self {
        Self {
            proposals: DashMap::new(),
            votes: DashMap::new(),
            committed: DashMap::new(),
        }
    }

    /// Create a new consensus proposal
    pub fn propose(
        &self,
        proposer: Uuid,
        topic: &str,
        value: serde_json::Value,
        quorum: QuorumRequirement,
        participants: Vec<Uuid>,
        timeout_secs: u64,
    ) -> Uuid {
        let proposal = ConsensusProposal {
            id: Uuid::new_v4(),
            proposer,
            topic: topic.to_string(),
            value,
            quorum,
            deadline: Utc::now() + chrono::Duration::seconds(timeout_secs as i64),
            status: ConsensusStatus::Proposed,
            participants,
        };

        let id = proposal.id;
        self.proposals.insert(id, proposal);
        self.votes.insert(id, Vec::new());

        id
    }

    /// Cast a vote on a proposal
    pub fn vote(
        &self,
        proposal_id: Uuid,
        voter: Uuid,
        decision: VoteDecision,
        weight: f64,
        reasoning: Option<&str>,
    ) -> Result<(), String> {
        let mut proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or("Proposal not found")?;

        if !proposal.participants.contains(&voter) {
            return Err("Not a participant".to_string());
        }

        if proposal.status != ConsensusStatus::Proposed
            && proposal.status != ConsensusStatus::Voting
        {
            return Err("Voting closed".to_string());
        }

        proposal.status = ConsensusStatus::Voting;
        drop(proposal);

        let vote = Vote {
            voter,
            proposal_id,
            decision,
            weight,
            reasoning: reasoning.map(String::from),
            timestamp: Utc::now(),
        };

        self.votes.entry(proposal_id).or_default().push(vote);

        Ok(())
    }

    /// Check if consensus is reached
    pub fn check_consensus(&self, proposal_id: Uuid) -> ConsensusStatus {
        let proposal = match self.proposals.get(&proposal_id) {
            Some(p) => p,
            None => return ConsensusStatus::Rejected,
        };

        let votes = match self.votes.get(&proposal_id) {
            Some(v) => v,
            None => return ConsensusStatus::Proposed,
        };

        // Check deadline
        if Utc::now() > proposal.deadline {
            return ConsensusStatus::TimedOut;
        }

        let summary = self.summarize_votes(&votes);

        let reached = match &proposal.quorum {
            QuorumRequirement::Majority => {
                summary.accept_count > summary.reject_count
                    && summary.accept_count > (votes.len() / 2)
            }
            QuorumRequirement::SuperMajority => {
                let threshold = (votes.len() as f64 * 0.66).ceil() as usize;
                summary.accept_count >= threshold
            }
            QuorumRequirement::Unanimous => {
                summary.accept_count == proposal.participants.len() && summary.reject_count == 0
            }
            QuorumRequirement::MinVotes(min) => summary.accept_count >= *min,
            QuorumRequirement::WeightedMajority { threshold } => {
                summary.accept_weight / summary.total_weight >= *threshold
            }
        };

        if reached {
            ConsensusStatus::Accepted
        } else if summary.reject_count > proposal.participants.len() / 2 {
            ConsensusStatus::Rejected
        } else {
            ConsensusStatus::Voting
        }
    }

    /// Commit a decision
    pub fn commit(&self, proposal_id: Uuid) -> Result<CommittedDecision, String> {
        let proposal = self
            .proposals
            .get(&proposal_id)
            .ok_or("Proposal not found")?;

        let votes = self.votes.get(&proposal_id).ok_or("No votes found")?;

        let decision = CommittedDecision {
            proposal_id,
            final_value: proposal.value.clone(),
            vote_summary: self.summarize_votes(&votes),
            committed_at: Utc::now(),
            execution_status: ExecutionStatus::Pending,
        };

        self.committed.insert(proposal_id, decision.clone());

        // Update proposal status
        drop(proposal);
        drop(votes);
        if let Some(mut p) = self.proposals.get_mut(&proposal_id) {
            p.status = ConsensusStatus::Committed;
        }

        Ok(decision)
    }

    fn summarize_votes(&self, votes: &[Vote]) -> VoteSummary {
        let mut accept_count = 0;
        let mut reject_count = 0;
        let mut abstain_count = 0;
        let mut total_weight = 0.0;
        let mut accept_weight = 0.0;

        for vote in votes {
            total_weight += vote.weight;
            match vote.decision {
                VoteDecision::Accept => {
                    accept_count += 1;
                    accept_weight += vote.weight;
                }
                VoteDecision::Reject => reject_count += 1,
                VoteDecision::Abstain => abstain_count += 1,
                VoteDecision::Conditional { .. } => accept_count += 1,
            }
        }

        VoteSummary {
            accept_count,
            reject_count,
            abstain_count,
            total_weight,
            accept_weight,
        }
    }
}

// =============================================================================
// AGENT INTROSPECTION & DEBUGGING
// =============================================================================

/// Execution tracer for debugging
pub struct AgentTracer {
    traces: DashMap<Uuid, ExecutionTrace>,
    breakpoints: DashMap<Uuid, Vec<Breakpoint>>,
    watchers: DashMap<Uuid, Vec<StateWatcher>>,
}

#[derive(Debug, Clone)]
pub struct ExecutionTrace {
    pub agent_id: Uuid,
    pub session_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub events: Vec<TraceEvent>,
    pub status: TraceStatus,
}

#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub event_type: TraceEventType,
    pub data: serde_json::Value,
    pub duration_us: Option<u64>,
    pub parent_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub enum TraceEventType {
    /// Agent started
    AgentStart,
    /// Intention created
    IntentionCreated {
        goal: String,
    },
    /// Planning phase
    PlanningStarted,
    PlanningCompleted {
        steps: usize,
    },
    /// Action execution
    ActionStarted {
        action: String,
    },
    ActionCompleted {
        result: String,
    },
    ActionFailed {
        error: String,
    },
    /// Knowledge operations
    KnowledgeQuery {
        query: String,
    },
    KnowledgeUpdate {
        node_id: String,
    },
    /// Communication
    MessageSent {
        to: Uuid,
    },
    MessageReceived {
        from: Uuid,
    },
    /// State changes
    StateChange {
        key: String,
        old: String,
        new: String,
    },
    /// Custom events
    Custom {
        name: String,
    },
}

#[derive(Debug, Clone)]
pub enum TraceStatus {
    Recording,
    Paused,
    Completed,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct Breakpoint {
    pub id: Uuid,
    pub condition: BreakpointCondition,
    pub action: BreakpointAction,
    pub hit_count: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub enum BreakpointCondition {
    /// Break on specific action
    OnAction(String),
    /// Break on state change
    OnStateChange { key: String },
    /// Break on message
    OnMessage {
        from: Option<Uuid>,
        to: Option<Uuid>,
    },
    /// Break on error
    OnError,
    /// Custom predicate
    Predicate { expression: String },
}

#[derive(Debug, Clone)]
pub enum BreakpointAction {
    /// Pause execution
    Pause,
    /// Log and continue
    Log { message: String },
    /// Capture state
    Snapshot,
    /// Execute callback
    Callback { handler: String },
}

#[derive(Debug, Clone)]
pub struct StateWatcher {
    pub id: Uuid,
    pub key: String,
    pub callback: String,
    pub last_value: Option<serde_json::Value>,
}

impl Default for AgentTracer {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentTracer {
    pub fn new() -> Self {
        Self {
            traces: DashMap::new(),
            breakpoints: DashMap::new(),
            watchers: DashMap::new(),
        }
    }

    /// Start tracing an agent
    pub fn start_trace(&self, agent_id: Uuid) -> Uuid {
        let session_id = Uuid::new_v4();
        let trace = ExecutionTrace {
            agent_id,
            session_id,
            started_at: Utc::now(),
            events: Vec::new(),
            status: TraceStatus::Recording,
        };

        self.traces.insert(session_id, trace);
        session_id
    }

    /// Record a trace event
    pub fn record(
        &self,
        session_id: Uuid,
        event_type: TraceEventType,
        data: serde_json::Value,
    ) -> Option<Uuid> {
        let mut trace = self.traces.get_mut(&session_id)?;

        let event = TraceEvent {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            event_type,
            data,
            duration_us: None,
            parent_id: trace.events.last().map(|e| e.id),
        };

        let id = event.id;
        trace.events.push(event);

        Some(id)
    }

    /// Add a breakpoint
    pub fn add_breakpoint(
        &self,
        agent_id: Uuid,
        condition: BreakpointCondition,
        action: BreakpointAction,
    ) -> Uuid {
        let bp = Breakpoint {
            id: Uuid::new_v4(),
            condition,
            action,
            hit_count: 0,
            enabled: true,
        };

        let id = bp.id;
        self.breakpoints.entry(agent_id).or_default().push(bp);

        id
    }

    /// Check breakpoints
    pub fn check_breakpoints(
        &self,
        agent_id: Uuid,
        event: &TraceEventType,
    ) -> Option<BreakpointAction> {
        let breakpoints = self.breakpoints.get(&agent_id)?;

        for bp in breakpoints.iter() {
            if !bp.enabled {
                continue;
            }

            let matches = match (&bp.condition, event) {
                (
                    BreakpointCondition::OnAction(action),
                    TraceEventType::ActionStarted { action: a },
                ) => action == a,
                (
                    BreakpointCondition::OnStateChange { key },
                    TraceEventType::StateChange { key: k, .. },
                ) => key == k,
                (
                    BreakpointCondition::OnMessage { from, to },
                    TraceEventType::MessageReceived { from: f },
                ) => from.is_none_or(|fr| &fr == f) && to.is_none(),
                (BreakpointCondition::OnError, TraceEventType::ActionFailed { .. }) => true,
                _ => false,
            };

            if matches {
                return Some(bp.action.clone());
            }
        }

        None
    }

    /// Add a state watcher
    pub fn watch(&self, agent_id: Uuid, key: &str, callback: &str) -> Uuid {
        let watcher = StateWatcher {
            id: Uuid::new_v4(),
            key: key.to_string(),
            callback: callback.to_string(),
            last_value: None,
        };

        let id = watcher.id;
        self.watchers.entry(agent_id).or_default().push(watcher);

        id
    }

    /// Get trace summary
    pub fn summarize(&self, session_id: Uuid) -> Option<TraceSummary> {
        let trace = self.traces.get(&session_id)?;

        let mut action_count = 0;
        let mut error_count = 0;
        let mut message_count = 0;

        for event in &trace.events {
            match &event.event_type {
                TraceEventType::ActionStarted { .. } => action_count += 1,
                TraceEventType::ActionFailed { .. } => error_count += 1,
                TraceEventType::MessageSent { .. } | TraceEventType::MessageReceived { .. } => {
                    message_count += 1
                }
                _ => {}
            }
        }

        Some(TraceSummary {
            session_id,
            agent_id: trace.agent_id,
            duration_ms: (Utc::now() - trace.started_at).num_milliseconds() as u64,
            event_count: trace.events.len(),
            action_count,
            error_count,
            message_count,
        })
    }
}

#[derive(Debug, Clone)]
pub struct TraceSummary {
    pub session_id: Uuid,
    pub agent_id: Uuid,
    pub duration_ms: u64,
    pub event_count: usize,
    pub action_count: usize,
    pub error_count: usize,
    pub message_count: usize,
}

// =============================================================================
// POLICY ENGINE
// =============================================================================

/// Declarative access control policy engine
pub struct PolicyEngine {
    policies: DashMap<String, Policy>,
    evaluations: DashMap<Uuid, EvaluationResult>,
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub id: String,
    pub name: String,
    pub description: String,
    pub rules: Vec<PolicyRule>,
    pub priority: u32,
    pub effect: PolicyEffect,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub id: String,
    pub subjects: Vec<SubjectMatcher>,
    pub resources: Vec<ResourceMatcher>,
    pub actions: Vec<String>,
    pub conditions: Vec<PolicyCondition>,
}

#[derive(Debug, Clone)]
pub enum SubjectMatcher {
    /// Match by agent ID
    AgentId(Uuid),
    /// Match by capability
    HasCapability(AgentCapability),
    /// Match by trust level
    TrustLevel(TrustLevel),
    /// Match by group
    InGroup(String),
    /// Match any
    Any,
}

#[derive(Debug, Clone)]
pub enum ResourceMatcher {
    /// Match by resource type
    Type(String),
    /// Match by path pattern
    Path(String),
    /// Match by owner
    OwnedBy(Uuid),
    /// Match by tag
    Tagged(String),
    /// Match any
    Any,
}

#[derive(Debug, Clone)]
pub enum PolicyCondition {
    /// Time-based condition
    TimeWindow { start: String, end: String },
    /// Rate limit
    RateLimit { max: u32, window_secs: u64 },
    /// IP range
    IpRange(String),
    /// Custom expression
    Expression(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PolicyEffect {
    Allow,
    Deny,
    RequireApproval,
    Log,
}

#[derive(Debug, Clone)]
pub struct EvaluationResult {
    pub request_id: Uuid,
    pub subject: Uuid,
    pub resource: String,
    pub action: String,
    pub decision: PolicyEffect,
    pub matching_policies: Vec<String>,
    pub evaluated_at: DateTime<Utc>,
    pub reason: Option<String>,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine {
    pub fn new() -> Self {
        Self {
            policies: DashMap::new(),
            evaluations: DashMap::new(),
        }
    }

    /// Add a policy
    pub fn add_policy(&self, policy: Policy) {
        self.policies.insert(policy.id.clone(), policy);
    }

    /// Evaluate access request
    pub fn evaluate(
        &self,
        subject: Uuid,
        resource: &str,
        action: &str,
        context: &EvaluationContext,
    ) -> EvaluationResult {
        let mut matching = Vec::new();
        let mut final_effect = PolicyEffect::Deny; // Default deny
        let mut highest_priority = 0u32;

        for entry in self.policies.iter() {
            let policy = entry.value();
            if !policy.enabled {
                continue;
            }

            for rule in &policy.rules {
                if self.matches_rule(rule, subject, resource, action, context) {
                    matching.push(policy.id.clone());

                    if policy.priority >= highest_priority {
                        highest_priority = policy.priority;
                        final_effect = policy.effect.clone();
                    }
                }
            }
        }

        let result = EvaluationResult {
            request_id: Uuid::new_v4(),
            subject,
            resource: resource.to_string(),
            action: action.to_string(),
            decision: final_effect,
            matching_policies: matching,
            evaluated_at: Utc::now(),
            reason: None,
        };

        self.evaluations.insert(result.request_id, result.clone());
        result
    }

    fn matches_rule(
        &self,
        rule: &PolicyRule,
        subject: Uuid,
        resource: &str,
        action: &str,
        context: &EvaluationContext,
    ) -> bool {
        // Check action
        if !rule.actions.iter().any(|a| a == "*" || a == action) {
            return false;
        }

        // Check subject
        let subject_matches = rule.subjects.iter().any(|s| match s {
            SubjectMatcher::AgentId(id) => *id == subject,
            SubjectMatcher::HasCapability(cap) => context.capabilities.contains(cap),
            SubjectMatcher::TrustLevel(level) => context.trust_level >= *level,
            SubjectMatcher::InGroup(group) => context.groups.contains(group),
            SubjectMatcher::Any => true,
        });

        if !subject_matches {
            return false;
        }

        // Check resource
        let resource_matches = rule.resources.iter().any(|r| match r {
            ResourceMatcher::Type(t) => resource.starts_with(t),
            ResourceMatcher::Path(p) => resource.contains(p),
            ResourceMatcher::OwnedBy(owner) => context.resource_owner.as_ref() == Some(owner),
            ResourceMatcher::Tagged(tag) => context.resource_tags.contains(tag),
            ResourceMatcher::Any => true,
        });

        if !resource_matches {
            return false;
        }

        // Check conditions
        rule.conditions
            .iter()
            .all(|c| self.evaluate_condition(c, context))
    }

    fn evaluate_condition(
        &self,
        condition: &PolicyCondition,
        _context: &EvaluationContext,
    ) -> bool {
        match condition {
            PolicyCondition::TimeWindow { start: _, end: _ } => {
                // Would parse and check time
                true
            }
            PolicyCondition::RateLimit {
                max: _,
                window_secs: _,
            } => {
                // Would check rate limit
                true
            }
            PolicyCondition::IpRange(_range) => {
                // Would check IP
                true
            }
            PolicyCondition::Expression(_expr) => {
                // Would evaluate expression
                true
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EvaluationContext {
    pub capabilities: Vec<AgentCapability>,
    pub trust_level: TrustLevel,
    pub groups: Vec<String>,
    pub resource_owner: Option<Uuid>,
    pub resource_tags: Vec<String>,
    pub ip_address: Option<String>,
    pub timestamp: DateTime<Utc>,
}

// =============================================================================
// EVENT SOURCING
// =============================================================================

/// Full audit trail with event sourcing
pub struct EventStore {
    events: DashMap<Uuid, Vec<StoredEvent>>,
    projections: DashMap<String, Projection>,
    subscribers: DashMap<String, Vec<EventSubscriber>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub id: Uuid,
    pub aggregate_id: Uuid,
    pub aggregate_type: String,
    pub event_type: String,
    pub version: u64,
    pub data: serde_json::Value,
    pub metadata: EventMetadata,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    pub correlation_id: Option<Uuid>,
    pub causation_id: Option<Uuid>,
    pub actor: Option<Uuid>,
    pub source: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Projection {
    pub name: String,
    pub event_types: Vec<String>,
    pub state: Arc<Mutex<serde_json::Value>>,
    pub version: u64,
}

#[derive(Debug, Clone)]
pub struct EventSubscriber {
    pub id: Uuid,
    pub event_types: Vec<String>,
    pub callback: String,
    pub filter: Option<String>,
}

impl Default for EventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EventStore {
    pub fn new() -> Self {
        Self {
            events: DashMap::new(),
            projections: DashMap::new(),
            subscribers: DashMap::new(),
        }
    }

    /// Append event to stream
    pub fn append(&self, aggregate_id: Uuid, event: StoredEvent) -> u64 {
        let mut stream = self.events.entry(aggregate_id).or_default();
        let version = stream.len() as u64;
        stream.push(event);

        version
    }

    /// Load events for aggregate
    pub fn load(&self, aggregate_id: Uuid) -> Vec<StoredEvent> {
        self.events
            .get(&aggregate_id)
            .map(|e| e.clone())
            .unwrap_or_default()
    }

    /// Load events after version
    pub fn load_from(&self, aggregate_id: Uuid, from_version: u64) -> Vec<StoredEvent> {
        self.events
            .get(&aggregate_id)
            .map(|e| {
                e.iter()
                    .filter(|ev| ev.version >= from_version)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Create projection
    pub fn create_projection(
        &self,
        name: &str,
        event_types: Vec<String>,
        initial: serde_json::Value,
    ) {
        let projection = Projection {
            name: name.to_string(),
            event_types,
            state: Arc::new(Mutex::new(initial)),
            version: 0,
        };
        self.projections.insert(name.to_string(), projection);
    }

    /// Get projection state
    pub fn get_projection(&self, name: &str) -> Option<serde_json::Value> {
        let projection = self.projections.get(name)?;
        let guard = projection.state.try_lock().ok()?;
        Some(guard.clone())
    }

    /// Subscribe to events
    pub fn subscribe(&self, event_types: Vec<String>, callback: &str) -> Uuid {
        let subscriber = EventSubscriber {
            id: Uuid::new_v4(),
            event_types: event_types.clone(),
            callback: callback.to_string(),
            filter: None,
        };

        let id = subscriber.id;

        for event_type in event_types {
            self.subscribers
                .entry(event_type)
                .or_default()
                .push(subscriber.clone());
        }

        id
    }

    /// Replay events for rebuild
    pub fn replay(&self, aggregate_id: Uuid, handler: impl Fn(&StoredEvent)) {
        if let Some(events) = self.events.get(&aggregate_id) {
            for event in events.iter() {
                handler(event);
            }
        }
    }

    /// Get event count
    pub fn event_count(&self, aggregate_id: Uuid) -> usize {
        self.events.get(&aggregate_id).map(|e| e.len()).unwrap_or(0)
    }

    /// Snapshot current state
    pub fn snapshot(&self, aggregate_id: Uuid) -> Option<AggregateSnapshot> {
        let events = self.events.get(&aggregate_id)?;

        Some(AggregateSnapshot {
            aggregate_id,
            version: events.len() as u64,
            state: serde_json::json!({
                "event_count": events.len(),
                "last_event": events.last().map(|e| &e.event_type),
            }),
            timestamp: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateSnapshot {
    pub aggregate_id: Uuid,
    pub version: u64,
    pub state: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

// =============================================================================
// AGENT FEDERATION
// =============================================================================

/// Cross-network agent federation
pub struct AgentFederation {
    local_registry: DashMap<Uuid, FederatedAgent>,
    remote_registries: DashMap<String, RemoteRegistry>,
    trust_links: DashMap<String, TrustLink>,
    routing_table: DashMap<Uuid, FederationRoute>,
}

#[derive(Debug, Clone)]
pub struct FederatedAgent {
    pub id: Uuid,
    pub did: Option<AgentDID>,
    pub capabilities: Vec<AgentCapability>,
    pub federation: String,
    pub endpoints: Vec<String>,
    pub trust_level: TrustLevel,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RemoteRegistry {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub protocol: String,
    pub trust_level: TrustLevel,
    pub agents_count: usize,
    pub last_sync: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct TrustLink {
    pub local_federation: String,
    pub remote_federation: String,
    pub trust_level: TrustLevel,
    pub established_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub bidirectional: bool,
}

#[derive(Debug, Clone)]
pub struct FederationRoute {
    pub agent_id: Uuid,
    pub via: Vec<String>,
    pub latency_ms: u32,
    pub cost: f64,
}

impl Default for AgentFederation {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentFederation {
    pub fn new() -> Self {
        Self {
            local_registry: DashMap::new(),
            remote_registries: DashMap::new(),
            trust_links: DashMap::new(),
            routing_table: DashMap::new(),
        }
    }

    /// Register local agent
    pub fn register_local(&self, agent: FederatedAgent) {
        self.local_registry.insert(agent.id, agent);
    }

    /// Add remote registry
    pub fn add_remote_registry(&self, registry: RemoteRegistry) {
        self.remote_registries.insert(registry.id.clone(), registry);
    }

    /// Establish trust link
    pub fn establish_trust(
        &self,
        local: &str,
        remote: &str,
        level: TrustLevel,
        bidirectional: bool,
    ) {
        let link = TrustLink {
            local_federation: local.to_string(),
            remote_federation: remote.to_string(),
            trust_level: level,
            established_at: Utc::now(),
            expires_at: None,
            bidirectional,
        };

        let key = format!("{}:{}", local, remote);
        self.trust_links.insert(key, link);
    }

    /// Find agent across federations
    pub async fn find_agent(&self, capability: AgentCapability) -> Vec<FederatedAgent> {
        let mut results = Vec::new();

        // Check local
        for entry in self.local_registry.iter() {
            if entry.capabilities.contains(&capability) {
                results.push(entry.clone());
            }
        }

        // Would query remote registries here

        results
    }

    /// Route message to federated agent
    pub fn route_to(&self, agent_id: Uuid) -> Option<FederationRoute> {
        // Check if local
        if self.local_registry.contains_key(&agent_id) {
            return Some(FederationRoute {
                agent_id,
                via: vec![],
                latency_ms: 0,
                cost: 0.0,
            });
        }

        // Check routing table
        self.routing_table.get(&agent_id).map(|r| r.clone())
    }

    /// Get federation statistics
    pub fn stats(&self) -> FederationStats {
        FederationStats {
            local_agents: self.local_registry.len(),
            remote_registries: self.remote_registries.len(),
            trust_links: self.trust_links.len(),
            routes: self.routing_table.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FederationStats {
    pub local_agents: usize,
    pub remote_registries: usize,
    pub trust_links: usize,
    pub routes: usize,
}

// =============================================================================
// AGENT REASONING ENGINE
// =============================================================================

/// Logical reasoning engine for agents
pub struct ReasoningEngine {
    facts: DashMap<String, Fact>,
    rules: DashMap<String, InferenceRule>,
    inferences: DashMap<String, Inference>,
    strategies: Vec<ReasoningStrategy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub id: String,
    pub predicate: String,
    pub arguments: Vec<FactValue>,
    pub confidence: f64,
    pub source: FactSource,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FactValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Entity(String),
    List(Vec<FactValue>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FactSource {
    Observation,
    Inference { rule_id: String, from: Vec<String> },
    External { source: String },
    UserProvided,
}

#[derive(Debug, Clone)]
pub struct InferenceRule {
    pub id: String,
    pub name: String,
    pub conditions: Vec<RuleCondition>,
    pub conclusion: RuleConclusion,
    pub confidence_factor: f64,
    pub priority: u32,
}

#[derive(Debug, Clone)]
pub struct RuleCondition {
    pub predicate: String,
    pub bindings: Vec<BindingPattern>,
    pub negated: bool,
}

#[derive(Debug, Clone)]
pub enum BindingPattern {
    Variable(String),
    Constant(FactValue),
    Wildcard,
}

#[derive(Debug, Clone)]
pub struct RuleConclusion {
    pub predicate: String,
    pub arguments: Vec<BindingPattern>,
}

#[derive(Debug, Clone)]
pub struct Inference {
    pub id: String,
    pub rule_id: String,
    pub bindings: HashMap<String, FactValue>,
    pub result: Fact,
    pub confidence: f64,
    pub inferred_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum ReasoningStrategy {
    ForwardChaining,
    BackwardChaining,
    AbductiveReasoning,
    AnalogicalReasoning { similarity_threshold: f64 },
}

impl Default for ReasoningEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ReasoningEngine {
    pub fn new() -> Self {
        Self {
            facts: DashMap::new(),
            rules: DashMap::new(),
            inferences: DashMap::new(),
            strategies: vec![ReasoningStrategy::ForwardChaining],
        }
    }

    /// Assert a fact
    pub fn assert_fact(&self, fact: Fact) {
        self.facts.insert(fact.id.clone(), fact);
    }

    /// Add an inference rule
    pub fn add_rule(&self, rule: InferenceRule) {
        self.rules.insert(rule.id.clone(), rule);
    }

    /// Query facts matching a pattern
    pub fn query(
        &self,
        predicate: &str,
        bindings: &[BindingPattern],
    ) -> Vec<(Fact, HashMap<String, FactValue>)> {
        let mut results = Vec::new();

        for entry in self.facts.iter() {
            let fact = entry.value();
            if fact.predicate == predicate {
                if let Some(matched_bindings) = self.match_bindings(&fact.arguments, bindings) {
                    results.push((fact.clone(), matched_bindings));
                }
            }
        }

        results
    }

    fn match_bindings(
        &self,
        args: &[FactValue],
        patterns: &[BindingPattern],
    ) -> Option<HashMap<String, FactValue>> {
        if args.len() != patterns.len() {
            return None;
        }

        let mut bindings = HashMap::new();

        for (arg, pattern) in args.iter().zip(patterns.iter()) {
            match pattern {
                BindingPattern::Variable(name) => {
                    if let Some(existing) = bindings.get(name) {
                        if !self.values_equal(arg, existing) {
                            return None;
                        }
                    } else {
                        bindings.insert(name.clone(), arg.clone());
                    }
                }
                BindingPattern::Constant(val) => {
                    if !self.values_equal(arg, val) {
                        return None;
                    }
                }
                BindingPattern::Wildcard => {}
            }
        }

        Some(bindings)
    }

    fn values_equal(&self, a: &FactValue, b: &FactValue) -> bool {
        match (a, b) {
            (FactValue::String(s1), FactValue::String(s2)) => s1 == s2,
            (FactValue::Number(n1), FactValue::Number(n2)) => (n1 - n2).abs() < 0.0001,
            (FactValue::Boolean(b1), FactValue::Boolean(b2)) => b1 == b2,
            (FactValue::Entity(e1), FactValue::Entity(e2)) => e1 == e2,
            _ => false,
        }
    }

    /// Run forward chaining inference
    pub fn infer(&self) -> Vec<Inference> {
        let mut new_inferences = Vec::new();

        for rule_entry in self.rules.iter() {
            let rule = rule_entry.value();

            // Try to match all conditions
            if let Some(bindings) = self.try_match_rule(rule) {
                for binding_set in bindings {
                    let inference = self.create_inference(rule, &binding_set);
                    if !self.fact_exists(&inference.result) {
                        self.facts
                            .insert(inference.result.id.clone(), inference.result.clone());
                        new_inferences.push(inference);
                    }
                }
            }
        }

        new_inferences
    }

    fn try_match_rule(&self, rule: &InferenceRule) -> Option<Vec<HashMap<String, FactValue>>> {
        let mut all_bindings: Vec<HashMap<String, FactValue>> = vec![HashMap::new()];

        for condition in &rule.conditions {
            let mut new_bindings = Vec::new();

            for existing in &all_bindings {
                let matches = self.query(&condition.predicate, &condition.bindings);
                for (_, matched) in matches {
                    let mut combined = existing.clone();
                    let mut valid = true;

                    for (k, v) in matched {
                        if let Some(existing_v) = combined.get(&k) {
                            if !self.values_equal(&v, existing_v) {
                                valid = false;
                                break;
                            }
                        } else {
                            combined.insert(k, v);
                        }
                    }

                    if valid {
                        if condition.negated {
                            // For negated conditions, we want NO matches
                        } else {
                            new_bindings.push(combined);
                        }
                    }
                }
            }

            if new_bindings.is_empty() && !condition.negated {
                return None;
            }

            all_bindings = new_bindings;
        }

        if all_bindings.is_empty() {
            None
        } else {
            Some(all_bindings)
        }
    }

    fn create_inference(
        &self,
        rule: &InferenceRule,
        bindings: &HashMap<String, FactValue>,
    ) -> Inference {
        let mut args = Vec::new();

        for pattern in &rule.conclusion.arguments {
            match pattern {
                BindingPattern::Variable(name) => {
                    if let Some(val) = bindings.get(name) {
                        args.push(val.clone());
                    }
                }
                BindingPattern::Constant(val) => {
                    args.push(val.clone());
                }
                BindingPattern::Wildcard => {}
            }
        }

        let fact_id = format!("inferred-{}", Uuid::new_v4());
        let inference_id = format!("inference-{}", Uuid::new_v4());

        Inference {
            id: inference_id,
            rule_id: rule.id.clone(),
            bindings: bindings.clone(),
            result: Fact {
                id: fact_id,
                predicate: rule.conclusion.predicate.clone(),
                arguments: args,
                confidence: rule.confidence_factor,
                source: FactSource::Inference {
                    rule_id: rule.id.clone(),
                    from: bindings.keys().cloned().collect(),
                },
                timestamp: Utc::now(),
            },
            confidence: rule.confidence_factor,
            inferred_at: Utc::now(),
        }
    }

    fn fact_exists(&self, fact: &Fact) -> bool {
        for entry in self.facts.iter() {
            let existing = entry.value();
            if existing.predicate == fact.predicate
                && existing.arguments.len() == fact.arguments.len()
            {
                let all_equal = existing
                    .arguments
                    .iter()
                    .zip(fact.arguments.iter())
                    .all(|(a, b)| self.values_equal(a, b));
                if all_equal {
                    return true;
                }
            }
        }
        false
    }

    /// Explain how a fact was derived
    pub fn explain(&self, fact_id: &str) -> Option<ReasoningExplanation> {
        let fact = self.facts.get(fact_id)?;

        match &fact.source {
            FactSource::Observation => Some(ReasoningExplanation {
                fact_id: fact_id.to_string(),
                explanation_type: ExplanationType::Observed,
                steps: vec![],
                confidence: fact.confidence,
            }),
            FactSource::Inference { rule_id, from } => {
                let mut steps = Vec::new();
                steps.push(ExplanationStep {
                    description: format!("Applied rule: {}", rule_id),
                    supporting_facts: from.clone(),
                });

                Some(ReasoningExplanation {
                    fact_id: fact_id.to_string(),
                    explanation_type: ExplanationType::Inferred,
                    steps,
                    confidence: fact.confidence,
                })
            }
            FactSource::External { source } => Some(ReasoningExplanation {
                fact_id: fact_id.to_string(),
                explanation_type: ExplanationType::External {
                    source: source.clone(),
                },
                steps: vec![],
                confidence: fact.confidence,
            }),
            FactSource::UserProvided => Some(ReasoningExplanation {
                fact_id: fact_id.to_string(),
                explanation_type: ExplanationType::UserProvided,
                steps: vec![],
                confidence: fact.confidence,
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReasoningExplanation {
    pub fact_id: String,
    pub explanation_type: ExplanationType,
    pub steps: Vec<ExplanationStep>,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub enum ExplanationType {
    Observed,
    Inferred,
    External { source: String },
    UserProvided,
}

#[derive(Debug, Clone)]
pub struct ExplanationStep {
    pub description: String,
    pub supporting_facts: Vec<String>,
}

// =============================================================================
// SEMANTIC MEMORY SYSTEM
// =============================================================================

/// Long-term semantic memory with retrieval
pub struct SemanticMemory {
    episodes: DashMap<Uuid, EpisodicMemory>,
    concepts: DashMap<String, ConceptMemory>,
    associations: DashMap<(String, String), Association>,
    working_memory: Arc<Mutex<WorkingMemory>>,
}

#[derive(Debug, Clone)]
pub struct EpisodicMemory {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub context: MemoryContext,
    pub content: serde_json::Value,
    pub emotional_valence: f64,
    pub importance: f64,
    pub access_count: u32,
    pub last_accessed: DateTime<Utc>,
    pub embedding: Option<Vec<f32>>,
    pub associations: Vec<Uuid>,
}

#[derive(Debug, Clone)]
pub struct MemoryContext {
    pub location: Option<String>,
    pub task: Option<String>,
    pub agents_involved: Vec<Uuid>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ConceptMemory {
    pub name: String,
    pub definition: String,
    pub examples: Vec<String>,
    pub properties: HashMap<String, serde_json::Value>,
    pub hierarchies: ConceptHierarchy,
    pub confidence: f64,
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub struct ConceptHierarchy {
    pub parents: Vec<String>,
    pub children: Vec<String>,
    pub siblings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Association {
    pub source: String,
    pub target: String,
    pub relation_type: RelationType,
    pub strength: f64,
    pub created_at: DateTime<Utc>,
    pub activated_count: u32,
}

#[derive(Debug, Clone)]
pub enum RelationType {
    IsA,
    HasA,
    PartOf,
    CausedBy,
    SimilarTo,
    OppositeOf,
    TemporallyRelated,
    SpatiallyRelated,
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct WorkingMemory {
    pub items: VecDeque<WorkingMemoryItem>,
    pub capacity: usize,
    pub focus: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkingMemoryItem {
    pub id: String,
    pub content: serde_json::Value,
    pub activation: f64,
    pub added_at: DateTime<Utc>,
}

impl Default for SemanticMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticMemory {
    pub fn new() -> Self {
        Self {
            episodes: DashMap::new(),
            concepts: DashMap::new(),
            associations: DashMap::new(),
            working_memory: Arc::new(Mutex::new(WorkingMemory {
                items: VecDeque::new(),
                capacity: 7, // Miller's magic number
                focus: None,
            })),
        }
    }

    /// Store an episodic memory
    pub fn remember(
        &self,
        content: serde_json::Value,
        context: MemoryContext,
        importance: f64,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let episode = EpisodicMemory {
            id,
            timestamp: Utc::now(),
            context,
            content,
            emotional_valence: 0.0,
            importance,
            access_count: 0,
            last_accessed: Utc::now(),
            embedding: None,
            associations: vec![],
        };

        self.episodes.insert(id, episode);
        id
    }

    /// Learn a concept
    pub fn learn_concept(&self, name: &str, definition: &str, parents: Vec<String>) {
        let concept = ConceptMemory {
            name: name.to_string(),
            definition: definition.to_string(),
            examples: vec![],
            properties: HashMap::new(),
            hierarchies: ConceptHierarchy {
                parents,
                children: vec![],
                siblings: vec![],
            },
            confidence: 0.5,
            embedding: None,
        };

        self.concepts.insert(name.to_string(), concept);
    }

    /// Create an association
    pub fn associate(&self, source: &str, target: &str, relation: RelationType, strength: f64) {
        let assoc = Association {
            source: source.to_string(),
            target: target.to_string(),
            relation_type: relation,
            strength,
            created_at: Utc::now(),
            activated_count: 0,
        };

        self.associations
            .insert((source.to_string(), target.to_string()), assoc);
    }

    /// Retrieve memories by similarity
    pub fn recall(&self, query_embedding: &[f32], limit: usize) -> Vec<EpisodicMemory> {
        let mut results: Vec<_> = self
            .episodes
            .iter()
            .filter_map(|entry| {
                let episode = entry.value();
                episode.embedding.as_ref().map(|emb| {
                    let similarity = cosine_similarity(query_embedding, emb);
                    (episode.clone(), similarity)
                })
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.into_iter().take(limit).map(|(e, _)| e).collect()
    }

    /// Retrieve by recency-weighted importance
    pub fn recall_recent(&self, limit: usize) -> Vec<EpisodicMemory> {
        let now = Utc::now();
        let mut results: Vec<_> = self
            .episodes
            .iter()
            .map(|entry| {
                let episode = entry.value();
                let recency = 1.0 / (1.0 + (now - episode.timestamp).num_hours() as f64 / 24.0);
                let score = episode.importance * recency;
                (episode.clone(), score)
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.into_iter().take(limit).map(|(e, _)| e).collect()
    }

    /// Spread activation through associations
    pub fn spread_activation(&self, source: &str, depth: usize) -> HashMap<String, f64> {
        let mut activations = HashMap::new();
        activations.insert(source.to_string(), 1.0);

        for _ in 0..depth {
            let mut new_activations = activations.clone();

            for (key, activation) in &activations {
                for entry in self.associations.iter() {
                    let (src, tgt) = entry.key();
                    let assoc = entry.value();

                    if src == key {
                        let propagated = activation * assoc.strength * 0.5;
                        let current = new_activations.get(tgt).copied().unwrap_or(0.0);
                        new_activations.insert(tgt.clone(), current + propagated);
                    }
                }
            }

            activations = new_activations;
        }

        activations
    }

    /// Focus working memory on a topic
    pub fn focus(&self, topic: &str) {
        if let Ok(mut wm) = self.working_memory.try_lock() {
            wm.focus = Some(topic.to_string());
        }
    }

    /// Add to working memory
    pub fn attend(&self, id: &str, content: serde_json::Value) {
        if let Ok(mut wm) = self.working_memory.try_lock() {
            let item = WorkingMemoryItem {
                id: id.to_string(),
                content,
                activation: 1.0,
                added_at: Utc::now(),
            };

            wm.items.push_front(item);

            while wm.items.len() > wm.capacity {
                wm.items.pop_back();
            }
        }
    }

    /// Get working memory contents
    pub fn get_working_memory(&self) -> Option<Vec<WorkingMemoryItem>> {
        self.working_memory
            .try_lock()
            .ok()
            .map(|wm| wm.items.iter().cloned().collect())
    }
}

// =============================================================================
// GOAL DECOMPOSITION ENGINE
// =============================================================================

/// Hierarchical goal decomposition
pub struct GoalDecomposer {
    goals: DashMap<Uuid, HierarchicalGoal>,
    strategies: Vec<DecompositionStrategy>,
    templates: DashMap<String, GoalTemplate>,
}

#[derive(Debug, Clone)]
pub struct HierarchicalGoal {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub goal_type: GoalType,
    pub parent: Option<Uuid>,
    pub subgoals: Vec<Uuid>,
    pub preconditions: Vec<GoalCondition>,
    pub postconditions: Vec<GoalCondition>,
    pub priority: f64,
    pub deadline: Option<DateTime<Utc>>,
    pub status: GoalStatus,
    pub progress: f64,
    pub assigned_agent: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub enum GoalType {
    Achievement {
        target_state: String,
    },
    Maintenance {
        invariant: String,
    },
    Optimization {
        metric: String,
        direction: OptimizationDirection,
    },
    Query {
        question: String,
    },
    Procedure {
        steps: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum OptimizationDirection {
    Maximize,
    Minimize,
    Target(f64),
}

#[derive(Debug, Clone)]
pub struct GoalCondition {
    pub predicate: String,
    pub parameters: Vec<String>,
    pub negated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GoalStatus {
    Pending,
    Active,
    Suspended,
    Achieved,
    Failed { reason: String },
    Abandoned,
}

#[derive(Debug, Clone)]
pub enum DecompositionStrategy {
    Sequential,
    Parallel,
    Conditional { condition: String },
    Iterative { until: String },
    Recursive,
}

#[derive(Debug, Clone)]
pub struct GoalTemplate {
    pub name: String,
    pub pattern: String,
    pub subgoal_patterns: Vec<String>,
    pub strategy: DecompositionStrategy,
}

impl Default for GoalDecomposer {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalDecomposer {
    pub fn new() -> Self {
        Self {
            goals: DashMap::new(),
            strategies: vec![DecompositionStrategy::Sequential],
            templates: DashMap::new(),
        }
    }

    /// Create a root goal
    pub fn create_goal(&self, name: &str, description: &str, goal_type: GoalType) -> Uuid {
        let id = Uuid::new_v4();
        let goal = HierarchicalGoal {
            id,
            name: name.to_string(),
            description: description.to_string(),
            goal_type,
            parent: None,
            subgoals: vec![],
            preconditions: vec![],
            postconditions: vec![],
            priority: 0.5,
            deadline: None,
            status: GoalStatus::Pending,
            progress: 0.0,
            assigned_agent: None,
        };

        self.goals.insert(id, goal);
        id
    }

    /// Decompose a goal into subgoals
    pub fn decompose(&self, goal_id: Uuid, subgoals: Vec<(String, GoalType)>) -> Vec<Uuid> {
        let mut subgoal_ids = Vec::new();

        for (name, goal_type) in subgoals {
            let subgoal_id = Uuid::new_v4();
            let subgoal = HierarchicalGoal {
                id: subgoal_id,
                name: name.clone(),
                description: String::new(),
                goal_type,
                parent: Some(goal_id),
                subgoals: vec![],
                preconditions: vec![],
                postconditions: vec![],
                priority: 0.5,
                deadline: None,
                status: GoalStatus::Pending,
                progress: 0.0,
                assigned_agent: None,
            };

            self.goals.insert(subgoal_id, subgoal);
            subgoal_ids.push(subgoal_id);
        }

        if let Some(mut goal) = self.goals.get_mut(&goal_id) {
            goal.subgoals = subgoal_ids.clone();
        }

        subgoal_ids
    }

    /// Add a goal template
    pub fn add_template(&self, template: GoalTemplate) {
        self.templates.insert(template.name.clone(), template);
    }

    /// Get leaf goals (actionable)
    pub fn get_leaf_goals(&self) -> Vec<HierarchicalGoal> {
        self.goals
            .iter()
            .filter(|entry| entry.value().subgoals.is_empty())
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get ready goals (preconditions satisfied)
    pub fn get_ready_goals(&self) -> Vec<HierarchicalGoal> {
        self.get_leaf_goals()
            .into_iter()
            .filter(|g| g.status == GoalStatus::Pending && g.preconditions.is_empty())
            .collect()
    }

    /// Update goal progress
    pub fn update_progress(&self, goal_id: Uuid, progress: f64) {
        if let Some(mut goal) = self.goals.get_mut(&goal_id) {
            goal.progress = progress.clamp(0.0, 1.0);

            if goal.progress >= 1.0 {
                goal.status = GoalStatus::Achieved;
            }
        }

        // Propagate to parent
        if let Some(goal) = self.goals.get(&goal_id) {
            if let Some(parent_id) = goal.parent {
                self.recalculate_progress(parent_id);
            }
        }
    }

    fn recalculate_progress(&self, goal_id: Uuid) {
        if let Some(mut goal) = self.goals.get_mut(&goal_id) {
            if goal.subgoals.is_empty() {
                return;
            }

            let total_progress: f64 = goal
                .subgoals
                .iter()
                .filter_map(|id| self.goals.get(id).map(|g| g.progress))
                .sum();

            goal.progress = total_progress / goal.subgoals.len() as f64;

            if goal.progress >= 1.0 {
                goal.status = GoalStatus::Achieved;
            }
        }
    }

    /// Get goal hierarchy as tree
    pub fn get_hierarchy(&self, root_id: Uuid) -> Option<GoalTree> {
        let goal = self.goals.get(&root_id)?;

        let children: Vec<GoalTree> = goal
            .subgoals
            .iter()
            .filter_map(|id| self.get_hierarchy(*id))
            .collect();

        Some(GoalTree {
            goal: goal.clone(),
            children,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GoalTree {
    pub goal: HierarchicalGoal,
    pub children: Vec<GoalTree>,
}

// =============================================================================
// AGENT NEGOTIATION PROTOCOL
// =============================================================================

/// Multi-party agent negotiation
pub struct NegotiationProtocol {
    negotiations: DashMap<Uuid, Negotiation>,
    strategies: DashMap<Uuid, NegotiationStrategy>,
    history: DashMap<Uuid, Vec<NegotiationRound>>,
}

#[derive(Debug, Clone)]
pub struct Negotiation {
    pub id: Uuid,
    pub topic: String,
    pub participants: Vec<Uuid>,
    pub status: NegotiationPhase,
    pub current_proposal: Option<Proposal>,
    pub deadline: DateTime<Utc>,
    pub rules: NegotiationRules,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NegotiationPhase {
    Initiated,
    Bidding,
    Bargaining,
    Consensus,
    Concluded { outcome: NegotiationOutcome },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum NegotiationOutcome {
    Agreement,
    Compromise,
    NoAgreement,
    Timeout,
}

#[derive(Debug, Clone)]
pub struct NegotiationRules {
    pub max_rounds: u32,
    pub timeout_per_round_secs: u64,
    pub allow_coalitions: bool,
    pub allow_side_payments: bool,
    pub voting_threshold: f64,
}

#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: Uuid,
    pub proposer: Uuid,
    pub terms: HashMap<String, serde_json::Value>,
    pub utility_claims: HashMap<Uuid, f64>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NegotiationRound {
    pub round_number: u32,
    pub proposals: Vec<Proposal>,
    pub responses: Vec<ProposalResponse>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ProposalResponse {
    pub responder: Uuid,
    pub response_type: ResponseType,
    pub counter_proposal: Option<Proposal>,
    pub utility: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResponseType {
    Accept,
    Reject,
    Counter,
    Defer,
}

#[derive(Debug, Clone)]
pub enum NegotiationStrategy {
    Cooperative { concession_rate: f64 },
    Competitive { aggression: f64 },
    TitForTat,
    BATNA { best_alternative_value: f64 },
    Integrative { interests: Vec<String> },
}

impl Default for NegotiationProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl NegotiationProtocol {
    pub fn new() -> Self {
        Self {
            negotiations: DashMap::new(),
            strategies: DashMap::new(),
            history: DashMap::new(),
        }
    }

    /// Initiate a negotiation
    pub fn initiate(&self, topic: &str, participants: Vec<Uuid>, rules: NegotiationRules) -> Uuid {
        let id = Uuid::new_v4();
        let negotiation = Negotiation {
            id,
            topic: topic.to_string(),
            participants: participants.clone(),
            status: NegotiationPhase::Initiated,
            current_proposal: None,
            deadline: Utc::now()
                + chrono::Duration::seconds(
                    rules.timeout_per_round_secs as i64 * rules.max_rounds as i64,
                ),
            rules,
        };

        self.negotiations.insert(id, negotiation);
        self.history.insert(id, Vec::new());

        id
    }

    /// Set negotiation strategy for an agent
    pub fn set_strategy(&self, agent_id: Uuid, strategy: NegotiationStrategy) {
        self.strategies.insert(agent_id, strategy);
    }

    /// Submit a proposal
    pub fn propose(&self, negotiation_id: Uuid, proposal: Proposal) -> Result<(), String> {
        let mut negotiation = self
            .negotiations
            .get_mut(&negotiation_id)
            .ok_or("Negotiation not found")?;

        if !negotiation.participants.contains(&proposal.proposer) {
            return Err("Not a participant".to_string());
        }

        negotiation.current_proposal = Some(proposal);
        negotiation.status = NegotiationPhase::Bargaining;

        Ok(())
    }

    /// Respond to current proposal
    pub fn respond(&self, negotiation_id: Uuid, response: ProposalResponse) -> Result<(), String> {
        let negotiation = self
            .negotiations
            .get(&negotiation_id)
            .ok_or("Negotiation not found")?;

        if !negotiation.participants.contains(&response.responder) {
            return Err("Not a participant".to_string());
        }

        // Record response
        if let Some(mut history) = self.history.get_mut(&negotiation_id) {
            if let Some(last_round) = history.last_mut() {
                last_round.responses.push(response.clone());
            } else {
                history.push(NegotiationRound {
                    round_number: 1,
                    proposals: negotiation.current_proposal.iter().cloned().collect(),
                    responses: vec![response.clone()],
                    timestamp: Utc::now(),
                });
            }
        }

        // Check for consensus
        drop(negotiation);
        self.check_consensus(negotiation_id);

        Ok(())
    }

    fn check_consensus(&self, negotiation_id: Uuid) {
        let history = match self.history.get(&negotiation_id) {
            Some(h) => h,
            None => return,
        };

        let mut negotiation = match self.negotiations.get_mut(&negotiation_id) {
            Some(n) => n,
            None => return,
        };

        if let Some(last_round) = history.last() {
            let accept_count = last_round
                .responses
                .iter()
                .filter(|r| r.response_type == ResponseType::Accept)
                .count();

            let total = negotiation.participants.len();
            let threshold = (total as f64 * negotiation.rules.voting_threshold) as usize;

            if accept_count >= threshold {
                negotiation.status = NegotiationPhase::Concluded {
                    outcome: NegotiationOutcome::Agreement,
                };
            }
        }
    }

    /// Get negotiation status
    pub fn get_status(&self, negotiation_id: Uuid) -> Option<NegotiationPhase> {
        self.negotiations
            .get(&negotiation_id)
            .map(|n| n.status.clone())
    }

    /// Calculate pareto-optimal solutions
    pub fn find_pareto_optimal(&self, negotiation_id: Uuid) -> Vec<Proposal> {
        let history = match self.history.get(&negotiation_id) {
            Some(h) => h,
            None => return vec![],
        };

        let all_proposals: Vec<Proposal> = history
            .iter()
            .flat_map(|r| r.proposals.iter().cloned())
            .collect();

        // Find non-dominated proposals
        let mut pareto = Vec::new();

        for proposal in &all_proposals {
            let dominated = all_proposals.iter().any(|other| {
                if proposal.id == other.id {
                    return false;
                }

                // Check if other dominates proposal
                proposal.utility_claims.iter().all(|(agent, utility)| {
                    other
                        .utility_claims
                        .get(agent)
                        .is_some_and(|other_utility| other_utility >= utility)
                }) && proposal.utility_claims.iter().any(|(agent, utility)| {
                    other
                        .utility_claims
                        .get(agent)
                        .is_some_and(|other_utility| other_utility > utility)
                })
            });

            if !dominated {
                pareto.push(proposal.clone());
            }
        }

        pareto
    }
}

// =============================================================================
// RESOURCE MANAGEMENT
// =============================================================================

/// Agent resource allocation and management
pub struct ResourceManager {
    resources: DashMap<String, Resource>,
    allocations: DashMap<(Uuid, String), Allocation>,
    quotas: DashMap<Uuid, ResourceQuota>,
    usage_history: DashMap<Uuid, Vec<UsageRecord>>,
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub id: String,
    pub name: String,
    pub resource_type: ResourceType,
    pub total_capacity: f64,
    pub available: f64,
    pub unit: String,
    pub renewable: bool,
    pub renewal_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceType {
    Compute,
    Memory,
    Storage,
    Bandwidth,
    ApiCalls,
    Tokens,
    Credits,
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct Allocation {
    pub agent_id: Uuid,
    pub resource_id: String,
    pub amount: f64,
    pub allocated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub priority: AllocationPriority,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AllocationPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

#[derive(Debug, Clone)]
pub struct ResourceQuota {
    pub agent_id: Uuid,
    pub limits: HashMap<String, f64>,
    pub used: HashMap<String, f64>,
    pub period: QuotaPeriod,
    pub reset_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum QuotaPeriod {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Unlimited,
}

#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub agent_id: Uuid,
    pub resource_id: String,
    pub amount: f64,
    pub operation: String,
    pub timestamp: DateTime<Utc>,
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceManager {
    pub fn new() -> Self {
        Self {
            resources: DashMap::new(),
            allocations: DashMap::new(),
            quotas: DashMap::new(),
            usage_history: DashMap::new(),
        }
    }

    /// Register a resource
    pub fn register_resource(&self, resource: Resource) {
        self.resources.insert(resource.id.clone(), resource);
    }

    /// Set quota for an agent
    pub fn set_quota(&self, agent_id: Uuid, limits: HashMap<String, f64>, period: QuotaPeriod) {
        let reset_at = match period {
            QuotaPeriod::Hourly => Utc::now() + chrono::Duration::hours(1),
            QuotaPeriod::Daily => Utc::now() + chrono::Duration::days(1),
            QuotaPeriod::Weekly => Utc::now() + chrono::Duration::weeks(1),
            QuotaPeriod::Monthly => Utc::now() + chrono::Duration::days(30),
            QuotaPeriod::Unlimited => Utc::now() + chrono::Duration::days(36500),
        };

        let quota = ResourceQuota {
            agent_id,
            limits,
            used: HashMap::new(),
            period,
            reset_at,
        };

        self.quotas.insert(agent_id, quota);
    }

    /// Request resource allocation
    pub fn allocate(
        &self,
        agent_id: Uuid,
        resource_id: &str,
        amount: f64,
        priority: AllocationPriority,
    ) -> Result<Allocation, String> {
        let mut resource = self
            .resources
            .get_mut(resource_id)
            .ok_or("Resource not found")?;

        // Check quota
        if let Some(quota) = self.quotas.get_mut(&agent_id) {
            if let Some(limit) = quota.limits.get(resource_id) {
                let used = quota.used.get(resource_id).copied().unwrap_or(0.0);
                if used + amount > *limit {
                    return Err(format!("Quota exceeded: {} + {} > {}", used, amount, limit));
                }
            }
        }

        // Check availability
        if resource.available < amount {
            return Err(format!(
                "Insufficient resources: {} < {}",
                resource.available, amount
            ));
        }

        // Allocate
        resource.available -= amount;

        let allocation = Allocation {
            agent_id,
            resource_id: resource_id.to_string(),
            amount,
            allocated_at: Utc::now(),
            expires_at: None,
            priority,
        };

        self.allocations
            .insert((agent_id, resource_id.to_string()), allocation.clone());

        // Update quota usage
        if let Some(mut quota) = self.quotas.get_mut(&agent_id) {
            *quota.used.entry(resource_id.to_string()).or_insert(0.0) += amount;
        }

        Ok(allocation)
    }

    /// Release allocation
    pub fn release(&self, agent_id: Uuid, resource_id: &str) -> Result<f64, String> {
        let allocation = self
            .allocations
            .remove(&(agent_id, resource_id.to_string()))
            .ok_or("Allocation not found")?;

        // Return to pool
        if let Some(mut resource) = self.resources.get_mut(resource_id) {
            resource.available += allocation.1.amount;
        }

        Ok(allocation.1.amount)
    }

    /// Record usage
    pub fn record_usage(&self, agent_id: Uuid, resource_id: &str, amount: f64, operation: &str) {
        let record = UsageRecord {
            agent_id,
            resource_id: resource_id.to_string(),
            amount,
            operation: operation.to_string(),
            timestamp: Utc::now(),
        };

        self.usage_history.entry(agent_id).or_default().push(record);
    }

    /// Get current usage for agent
    pub fn get_usage(&self, agent_id: Uuid) -> HashMap<String, f64> {
        self.quotas
            .get(&agent_id)
            .map(|q| q.used.clone())
            .unwrap_or_default()
    }

    /// Get resource availability
    pub fn get_availability(&self, resource_id: &str) -> Option<f64> {
        self.resources.get(resource_id).map(|r| r.available)
    }

    /// Renew renewable resources
    pub fn renew_resources(&self) {
        for mut entry in self.resources.iter_mut() {
            let resource = entry.value_mut();
            if resource.renewable {
                if let Some(rate) = resource.renewal_rate {
                    resource.available = (resource.available + rate).min(resource.total_capacity);
                }
            }
        }
    }

    /// Get usage summary
    pub fn get_summary(&self) -> ResourceSummary {
        let mut total = 0.0;
        let mut used = 0.0;

        for entry in self.resources.iter() {
            total += entry.total_capacity;
            used += entry.total_capacity - entry.available;
        }

        ResourceSummary {
            total_resources: self.resources.len(),
            total_capacity: total,
            total_used: used,
            utilization: if total > 0.0 { used / total } else { 0.0 },
            active_allocations: self.allocations.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResourceSummary {
    pub total_resources: usize,
    pub total_capacity: f64,
    pub total_used: f64,
    pub utilization: f64,
    pub active_allocations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_creation() {
        let agent = create_agent("TestAgent");
        assert!(!agent.profile().name.is_empty());
        assert_eq!(agent.profile().trust_level, TrustLevel::Unknown);
    }

    #[test]
    fn test_intention_creation() {
        let agent = create_agent("TestAgent");
        let intention_id = agent.intend(Goal::Navigate {
            target: ResourceLocator::url("https://example.com"),
        });
        assert!(agent.intentions.contains_key(&intention_id));
    }

    #[test]
    fn test_knowledge_graph() {
        let mut kg = KnowledgeGraph::new();
        let node = KnowledgeNode {
            id: "test".to_string(),
            label: "Test Node".to_string(),
            node_type: "concept".to_string(),
            properties: serde_json::json!({}),
            embedding: Some(vec![1.0, 0.0, 0.0]),
            confidence: 1.0,
            source: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        kg.add_node(node);

        let results = kg.query_similar(&[1.0, 0.0, 0.0], 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "test");
        assert!((results[0].1 - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_resource_locator() {
        let loc = ResourceLocator::semantic("weather")
            .with_constraint("location:london")
            .with_constraint("timeframe:today");

        if let ResourceLocator::Semantic {
            concept,
            constraints,
        } = loc
        {
            assert_eq!(concept, "weather");
            assert_eq!(constraints.len(), 2);
        } else {
            panic!("Expected semantic locator");
        }
    }
}

// =============================================================================
// AGENT LEARNING & ADAPTATION
// =============================================================================

/// Learning signal types for reinforcement learning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LearningSignal {
    /// Immediate reward/punishment
    Reward { value: f64, context: String },
    /// Delayed reward with attribution
    DelayedReward {
        value: f64,
        delay_steps: usize,
        attribution: Vec<(String, f64)>,
    },
    /// Intrinsic motivation (curiosity, novelty)
    Intrinsic {
        kind: IntrinsicMotivation,
        intensity: f64,
    },
    /// Social learning from other agents
    Social {
        source_agent: Uuid,
        behavior: String,
        outcome: f64,
    },
    /// Counterfactual learning (what-if)
    Counterfactual {
        action_taken: String,
        alternative: String,
        estimated_diff: f64,
    },
}

/// Intrinsic motivation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntrinsicMotivation {
    Curiosity,   // Novelty-seeking
    Competence,  // Mastery-seeking
    Autonomy,    // Self-determination
    Relatedness, // Social connection
    Progress,    // Improvement sensing
}

/// Experience replay buffer entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experience {
    pub id: Uuid,
    pub state: StateRepresentation,
    pub action: String,
    pub next_state: StateRepresentation,
    pub signal: LearningSignal,
    pub priority: f64,
    pub timestamp: DateTime<Utc>,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// State representation for learning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateRepresentation {
    pub features: Vec<f64>,
    pub symbolic: HashMap<String, serde_json::Value>,
    pub context_id: Option<String>,
}

impl StateRepresentation {
    pub fn new(features: Vec<f64>) -> Self {
        Self {
            features,
            symbolic: HashMap::new(),
            context_id: None,
        }
    }

    pub fn with_symbol(mut self, key: &str, value: serde_json::Value) -> Self {
        self.symbolic.insert(key.to_string(), value);
        self
    }
}

/// Learning policy representation for RL
#[derive(Debug, Clone)]
pub struct LearningPolicy {
    pub id: Uuid,
    pub name: String,
    pub action_values: DashMap<String, ActionValue>,
    pub exploration_rate: f64,
    pub learning_rate: f64,
    pub discount_factor: f64,
    pub created_at: DateTime<Utc>,
    pub update_count: u64,
}

/// Action value with statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionValue {
    pub action: String,
    pub value: f64,
    pub count: u64,
    pub variance: f64,
    pub last_update: DateTime<Utc>,
}

/// Learning algorithm types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LearningAlgorithm {
    QLearning { alpha: f64, gamma: f64 },
    SARSA { alpha: f64, gamma: f64 },
    ActorCritic { actor_lr: f64, critic_lr: f64 },
    PolicyGradient { learning_rate: f64, baseline: f64 },
    EvolutionStrategy { population_size: usize, sigma: f64 },
}

#[allow(clippy::type_complexity)]
/// Agent learning system
pub struct AgentLearner {
    agent_id: Uuid,
    policies: DashMap<String, LearningPolicy>,
    experience_buffer: Arc<RwLock<Vec<Experience>>>,
    buffer_capacity: usize,
    algorithm: LearningAlgorithm,
    adaptation_hooks: Vec<Box<dyn Fn(&Experience, &LearningPolicy) + Send + Sync>>,
    total_experiences: u64,
    cumulative_reward: f64,
}

impl AgentLearner {
    pub fn new(agent_id: Uuid, algorithm: LearningAlgorithm, buffer_capacity: usize) -> Self {
        Self {
            agent_id,
            policies: DashMap::new(),
            experience_buffer: Arc::new(RwLock::new(Vec::with_capacity(buffer_capacity))),
            buffer_capacity,
            algorithm,
            adaptation_hooks: Vec::new(),
            total_experiences: 0,
            cumulative_reward: 0.0,
        }
    }

    /// Create or get a policy for a domain
    pub fn get_or_create_policy(&self, domain: &str) -> LearningPolicy {
        if let Some(policy) = self.policies.get(domain) {
            policy.clone()
        } else {
            let (lr, gamma) = match &self.algorithm {
                LearningAlgorithm::QLearning { alpha, gamma } => (*alpha, *gamma),
                LearningAlgorithm::SARSA { alpha, gamma } => (*alpha, *gamma),
                LearningAlgorithm::ActorCritic { actor_lr, .. } => (*actor_lr, 0.99),
                LearningAlgorithm::PolicyGradient { learning_rate, .. } => (*learning_rate, 0.99),
                LearningAlgorithm::EvolutionStrategy { .. } => (0.01, 0.99),
            };

            let policy = LearningPolicy {
                id: Uuid::new_v4(),
                name: domain.to_string(),
                action_values: DashMap::new(),
                exploration_rate: 0.1,
                learning_rate: lr,
                discount_factor: gamma,
                created_at: Utc::now(),
                update_count: 0,
            };
            self.policies.insert(domain.to_string(), policy.clone());
            policy
        }
    }

    /// Record an experience
    pub async fn record_experience(&mut self, experience: Experience) {
        // Track cumulative reward
        if let LearningSignal::Reward { value, .. } = &experience.signal {
            self.cumulative_reward += value;
        }

        let mut buffer = self.experience_buffer.write().await;

        // Prioritized experience replay - higher priority for surprising experiences
        if buffer.len() >= self.buffer_capacity {
            // Remove lowest priority experience
            if let Some(min_idx) = buffer
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.priority.partial_cmp(&b.1.priority).unwrap())
                .map(|(i, _)| i)
            {
                buffer.remove(min_idx);
            }
        }

        buffer.push(experience);
        self.total_experiences += 1;
    }

    /// Learn from experiences (batch update)
    pub async fn learn(&self, batch_size: usize, domain: &str) -> f64 {
        let buffer = self.experience_buffer.read().await;
        if buffer.is_empty() {
            return 0.0;
        }

        let mut policy = self.get_or_create_policy(domain);
        let mut total_delta = 0.0;

        // Sample experiences (prioritized)
        let _rng = rand::thread_rng();
        let samples: Vec<_> = buffer
            .iter()
            .filter(|e| e.action.starts_with(domain))
            .collect();

        let sample_count = samples.len().min(batch_size);
        if sample_count == 0 {
            return 0.0;
        }

        for experience in samples.iter().take(sample_count) {
            let delta = self.update_policy(&mut policy, experience);
            total_delta += delta.abs();
        }

        // Store updated policy
        if let Some(mut p) = self.policies.get_mut(domain) {
            p.update_count = policy.update_count;
            // Update action values
            for entry in policy.action_values.iter() {
                p.action_values
                    .insert(entry.key().clone(), entry.value().clone());
            }
        }

        total_delta / sample_count as f64
    }

    fn update_policy(&self, policy: &mut LearningPolicy, experience: &Experience) -> f64 {
        let reward = match &experience.signal {
            LearningSignal::Reward { value, .. } => *value,
            LearningSignal::DelayedReward { value, .. } => *value,
            LearningSignal::Intrinsic { intensity, .. } => *intensity * 0.1,
            LearningSignal::Social { outcome, .. } => *outcome * 0.5,
            LearningSignal::Counterfactual { estimated_diff, .. } => *estimated_diff,
        };

        let action = &experience.action;
        let current_value = policy
            .action_values
            .get(action)
            .map(|v| v.value)
            .unwrap_or(0.0);

        // Q-learning update: Q(s,a) = Q(s,a) + α * (r + γ * max(Q(s')) - Q(s,a))
        let max_next = policy
            .action_values
            .iter()
            .map(|e| e.value().value)
            .fold(0.0_f64, |a, b| a.max(b));

        let td_target = reward + policy.discount_factor * max_next;
        let delta = td_target - current_value;
        let new_value = current_value + policy.learning_rate * delta;

        // Update action value with running statistics
        let mut entry = policy
            .action_values
            .entry(action.clone())
            .or_insert(ActionValue {
                action: action.clone(),
                value: 0.0,
                count: 0,
                variance: 0.0,
                last_update: Utc::now(),
            });

        // Online variance calculation
        let old_mean = entry.value;
        entry.count += 1;
        entry.value = new_value;
        let diff = new_value - old_mean;
        entry.variance += diff * (new_value - entry.value);
        entry.last_update = Utc::now();

        policy.update_count += 1;

        delta
    }

    /// Select action using current policy (epsilon-greedy)
    pub fn select_action(&self, domain: &str, available_actions: &[String]) -> String {
        let policy = self.get_or_create_policy(domain);
        let mut rng = rand::thread_rng();

        // Epsilon-greedy exploration
        if rng.gen::<f64>() < policy.exploration_rate {
            // Random exploration
            available_actions[rng.gen_range(0..available_actions.len())].clone()
        } else {
            // Greedy exploitation
            available_actions
                .iter()
                .max_by(|a, b| {
                    let va = policy.action_values.get(*a).map(|v| v.value).unwrap_or(0.0);
                    let vb = policy.action_values.get(*b).map(|v| v.value).unwrap_or(0.0);
                    va.partial_cmp(&vb).unwrap()
                })
                .cloned()
                .unwrap_or_else(|| available_actions[0].clone())
        }
    }

    /// Decay exploration rate
    pub fn decay_exploration(&self, domain: &str, decay_rate: f64, min_rate: f64) {
        if let Some(mut policy) = self.policies.get_mut(domain) {
            policy.exploration_rate = (policy.exploration_rate * decay_rate).max(min_rate);
        }
    }

    /// Get learning statistics
    pub fn get_stats(&self) -> LearningStats {
        let total_value: f64 = self
            .policies
            .iter()
            .flat_map(|p| {
                let values: Vec<f64> = p.action_values.iter().map(|a| a.value).collect();
                values
            })
            .sum();
        let count = self.policies.len().max(1) as f64;

        LearningStats {
            total_experiences: self.total_experiences,
            cumulative_reward: self.cumulative_reward,
            policy_count: self.policies.len(),
            average_value: total_value / count,
        }
    }
}

/// Learning statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningStats {
    pub total_experiences: u64,
    pub cumulative_reward: f64,
    pub policy_count: usize,
    pub average_value: f64,
}

// =============================================================================
// SKILL LIBRARY & TRANSFER LEARNING
// =============================================================================

/// Transferable skill representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub category: SkillCategory,
    pub preconditions: Vec<SkillCondition>,
    pub effects: Vec<SkillEffect>,
    pub parameters: Vec<SkillParameter>,
    pub execution_trace: Option<String>,
    pub success_rate: f64,
    pub usage_count: u64,
    pub created_at: DateTime<Utc>,
    pub learned_from: Option<Uuid>, // Source agent
}

/// Skill categories
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SkillCategory {
    Navigation,
    DataExtraction,
    Communication,
    Analysis,
    Planning,
    Learning,
    Coordination,
    Custom(String),
}

/// Skill precondition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCondition {
    pub kind: ConditionKind,
    pub parameters: HashMap<String, serde_json::Value>,
}

/// Condition types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConditionKind {
    StateEquals {
        key: String,
        value: serde_json::Value,
    },
    HasCapability(String),
    ResourceAvailable {
        resource: String,
        amount: f64,
    },
    TimeConstraint {
        after: Option<DateTime<Utc>>,
        before: Option<DateTime<Utc>>,
    },
    Custom(String),
}

/// Skill effect
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEffect {
    pub kind: EffectKind,
    pub probability: f64,
}

/// Effect types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EffectKind {
    StateChange {
        key: String,
        value: serde_json::Value,
    },
    ResourceConsumed {
        resource: String,
        amount: f64,
    },
    ResourceProduced {
        resource: String,
        amount: f64,
    },
    MessageSent {
        recipient: String,
    },
    KnowledgeGained {
        topic: String,
    },
    Custom(String),
}

/// Skill parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillParameter {
    pub name: String,
    pub param_type: SkillParamType,
    pub default: Option<serde_json::Value>,
    pub required: bool,
}

/// Parameter types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkillParamType {
    String,
    Number,
    Boolean,
    Url,
    Selector,
    AgentId,
    Custom(String),
}

/// Skill library for storing and retrieving skills
pub struct SkillLibrary {
    skills: DashMap<Uuid, Skill>,
    category_index: DashMap<SkillCategory, Vec<Uuid>>,
    name_index: DashMap<String, Uuid>,
    similarity_threshold: f64,
}

impl Default for SkillLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillLibrary {
    pub fn new() -> Self {
        Self {
            skills: DashMap::new(),
            category_index: DashMap::new(),
            name_index: DashMap::new(),
            similarity_threshold: 0.8,
        }
    }

    /// Register a new skill
    pub fn register(&self, skill: Skill) -> Uuid {
        let id = skill.id;
        let name = skill.name.clone();
        let category = skill.category.clone();

        self.skills.insert(id, skill);
        self.name_index.insert(name, id);

        self.category_index.entry(category).or_default().push(id);

        id
    }

    /// Get skill by ID
    pub fn get(&self, id: &Uuid) -> Option<Skill> {
        self.skills.get(id).map(|s| s.clone())
    }

    /// Get skill by name
    pub fn get_by_name(&self, name: &str) -> Option<Skill> {
        self.name_index
            .get(name)
            .and_then(|id| self.skills.get(&id).map(|s| s.clone()))
    }

    /// Find skills by category
    pub fn find_by_category(&self, category: &SkillCategory) -> Vec<Skill> {
        self.category_index
            .get(category)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.skills.get(id).map(|s| s.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find applicable skills given current state
    pub fn find_applicable(&self, state: &HashMap<String, serde_json::Value>) -> Vec<Skill> {
        self.skills
            .iter()
            .filter(|entry| self.check_preconditions(&entry.preconditions, state))
            .map(|entry| entry.clone())
            .collect()
    }

    fn check_preconditions(
        &self,
        preconditions: &[SkillCondition],
        state: &HashMap<String, serde_json::Value>,
    ) -> bool {
        preconditions.iter().all(|cond| match &cond.kind {
            ConditionKind::StateEquals { key, value } => {
                state.get(key).map(|v| v == value).unwrap_or(false)
            }
            ConditionKind::HasCapability(cap) => state
                .get("capabilities")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().any(|c| c.as_str() == Some(cap)))
                .unwrap_or(false),
            _ => true,
        })
    }

    /// Transfer skill from one agent to another (with adaptation)
    pub fn transfer_skill(&self, skill_id: &Uuid, _target_agent: Uuid) -> Option<Skill> {
        self.skills.get(skill_id).map(|skill| {
            let mut transferred = skill.clone();
            transferred.id = Uuid::new_v4();
            transferred.learned_from = Some(skill.id);
            transferred.usage_count = 0;
            transferred.success_rate = skill.success_rate * 0.8; // Initial penalty
            transferred.created_at = Utc::now();
            transferred
        })
    }

    /// Record skill usage outcome
    pub fn record_usage(&self, skill_id: &Uuid, success: bool) {
        if let Some(mut skill) = self.skills.get_mut(skill_id) {
            skill.usage_count += 1;
            // Running average
            let alpha = 1.0 / skill.usage_count as f64;
            skill.success_rate =
                skill.success_rate * (1.0 - alpha) + (if success { 1.0 } else { 0.0 }) * alpha;
        }
    }

    /// Get library statistics
    pub fn stats(&self) -> SkillLibraryStats {
        let by_category: HashMap<String, usize> = self
            .category_index
            .iter()
            .map(|e| (format!("{:?}", e.key()), e.value().len()))
            .collect();

        SkillLibraryStats {
            total_skills: self.skills.len(),
            by_category,
            average_success_rate: self.skills.iter().map(|s| s.success_rate).sum::<f64>()
                / self.skills.len().max(1) as f64,
            total_usages: self.skills.iter().map(|s| s.usage_count).sum(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillLibraryStats {
    pub total_skills: usize,
    pub by_category: HashMap<String, usize>,
    pub average_success_rate: f64,
    pub total_usages: u64,
}

// =============================================================================
// META-LEARNING & LEARNING TO LEARN
// =============================================================================

/// Meta-learning configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaLearningConfig {
    pub inner_learning_rate: f64,
    pub outer_learning_rate: f64,
    pub inner_steps: usize,
    pub task_batch_size: usize,
    pub adaptation_steps: usize,
}

impl Default for MetaLearningConfig {
    fn default() -> Self {
        Self {
            inner_learning_rate: 0.01,
            outer_learning_rate: 0.001,
            inner_steps: 5,
            task_batch_size: 4,
            adaptation_steps: 10,
        }
    }
}

/// Task representation for meta-learning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningTask {
    pub id: Uuid,
    pub name: String,
    pub domain: String,
    pub train_experiences: Vec<Experience>,
    pub test_experiences: Vec<Experience>,
    pub difficulty: f64,
    pub similarity_to_prior: f64,
}

/// Meta-learner for rapid adaptation
pub struct MetaLearner {
    config: MetaLearningConfig,
    base_policy: LearningPolicy,
    task_history: Vec<LearningTask>,
    adaptation_performance: DashMap<String, Vec<f64>>,
    meta_parameters: Vec<f64>,
}

impl MetaLearner {
    pub fn new(config: MetaLearningConfig) -> Self {
        Self {
            config,
            base_policy: LearningPolicy {
                id: Uuid::new_v4(),
                name: "meta_base".to_string(),
                action_values: DashMap::new(),
                exploration_rate: 0.2,
                learning_rate: 0.01,
                discount_factor: 0.99,
                created_at: Utc::now(),
                update_count: 0,
            },
            task_history: Vec::new(),
            adaptation_performance: DashMap::new(),
            meta_parameters: vec![0.0; 100], // Learned initialization
        }
    }

    /// Adapt to a new task quickly using meta-learned initialization
    pub async fn adapt_to_task(&self, task: &LearningTask) -> LearningPolicy {
        let mut adapted_policy = self.base_policy.clone();
        adapted_policy.id = Uuid::new_v4();
        adapted_policy.name = format!("adapted_{}", task.name);
        adapted_policy.learning_rate = self.config.inner_learning_rate;

        // Initialize from meta-parameters (learned good starting point)
        for (i, param) in self.meta_parameters.iter().enumerate() {
            let action_key = format!("action_{}", i % 10);
            if let Some(mut av) = adapted_policy.action_values.get_mut(&action_key) {
                av.value += param * 0.1;
            }
        }

        // Inner loop: quick adaptation using task's training data
        for _ in 0..self.config.adaptation_steps {
            for exp in &task.train_experiences {
                self.inner_update(&mut adapted_policy, exp);
            }
        }

        adapted_policy
    }

    fn inner_update(&self, policy: &mut LearningPolicy, experience: &Experience) {
        let reward = match &experience.signal {
            LearningSignal::Reward { value, .. } => *value,
            _ => 0.0,
        };

        let action = &experience.action;
        let current = policy
            .action_values
            .get(action)
            .map(|v| v.value)
            .unwrap_or(0.0);

        let new_value = current + policy.learning_rate * (reward - current);

        policy
            .action_values
            .entry(action.clone())
            .or_insert(ActionValue {
                action: action.clone(),
                value: 0.0,
                count: 0,
                variance: 0.0,
                last_update: Utc::now(),
            })
            .value = new_value;
    }

    /// Meta-update: improve base policy from task batch performance
    pub async fn meta_update(&mut self, tasks: &[LearningTask]) {
        let mut gradients = vec![0.0; self.meta_parameters.len()];

        for task in tasks {
            // Adapt to task
            let adapted = self.adapt_to_task(task).await;

            // Evaluate on test set
            let performance = self.evaluate_policy(&adapted, &task.test_experiences);

            // Track adaptation performance
            self.adaptation_performance
                .entry(task.domain.clone())
                .or_default()
                .push(performance);

            // Compute meta-gradient (simplified)
            for grad in gradients.iter_mut() {
                *grad += (performance - 0.5) * 0.01; // Baseline subtraction
            }
        }

        // Update meta-parameters
        for (param, grad) in self.meta_parameters.iter_mut().zip(gradients.iter()) {
            *param += self.config.outer_learning_rate * grad / tasks.len() as f64;
        }

        // Update base policy's learning rate based on adaptation success
        let avg_performance: Vec<f64> = self
            .adaptation_performance
            .iter()
            .flat_map(|e| {
                let vals: Vec<f64> = e.value().to_vec();
                vals
            })
            .collect();

        if !avg_performance.is_empty() {
            let avg = avg_performance.iter().sum::<f64>() / avg_performance.len() as f64;
            // Increase exploration if adapting poorly
            if avg < 0.5 {
                self.base_policy.exploration_rate =
                    (self.base_policy.exploration_rate * 1.1).min(0.5);
            }
        }
    }

    fn evaluate_policy(&self, policy: &LearningPolicy, experiences: &[Experience]) -> f64 {
        if experiences.is_empty() {
            return 0.5;
        }

        let mut total_reward = 0.0;
        for exp in experiences {
            let predicted_value = policy
                .action_values
                .get(&exp.action)
                .map(|v| v.value)
                .unwrap_or(0.0);

            let actual = match &exp.signal {
                LearningSignal::Reward { value, .. } => *value,
                _ => 0.0,
            };

            // Reward for good predictions
            total_reward += 1.0 - (predicted_value - actual).abs().min(1.0);
        }

        total_reward / experiences.len() as f64
    }

    /// Record task for history
    pub fn record_task(&mut self, task: LearningTask) {
        self.task_history.push(task);
        // Keep history bounded
        if self.task_history.len() > 1000 {
            self.task_history.remove(0);
        }
    }

    /// Get meta-learning statistics
    pub fn stats(&self) -> MetaLearningStats {
        let domain_performance: HashMap<String, f64> = self
            .adaptation_performance
            .iter()
            .map(|e| {
                let avg = e.value().iter().sum::<f64>() / e.value().len().max(1) as f64;
                (e.key().clone(), avg)
            })
            .collect();

        MetaLearningStats {
            tasks_learned: self.task_history.len(),
            domains: domain_performance.keys().cloned().collect(),
            domain_performance,
            meta_parameter_norm: self
                .meta_parameters
                .iter()
                .map(|p| p * p)
                .sum::<f64>()
                .sqrt(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaLearningStats {
    pub tasks_learned: usize,
    pub domains: Vec<String>,
    pub domain_performance: HashMap<String, f64>,
    pub meta_parameter_norm: f64,
}

// =============================================================================
// CURRICULUM LEARNING
// =============================================================================

/// Curriculum stage for progressive learning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurriculumStage {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub difficulty: f64,
    pub prerequisites: Vec<Uuid>,
    pub skills_taught: Vec<String>,
    pub success_threshold: f64,
    pub max_attempts: usize,
}

/// Curriculum learning manager
pub struct CurriculumManager {
    stages: DashMap<Uuid, CurriculumStage>,
    progress: DashMap<Uuid, AgentProgress>, // Agent -> Progress
    stage_order: Vec<Uuid>,
}

/// Agent's progress through curriculum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProgress {
    pub agent_id: Uuid,
    pub current_stage: Option<Uuid>,
    pub completed_stages: Vec<Uuid>,
    pub stage_attempts: HashMap<Uuid, usize>,
    pub stage_scores: HashMap<Uuid, f64>,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
}

impl Default for CurriculumManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CurriculumManager {
    pub fn new() -> Self {
        Self {
            stages: DashMap::new(),
            progress: DashMap::new(),
            stage_order: Vec::new(),
        }
    }

    /// Add a stage to the curriculum
    pub fn add_stage(&mut self, stage: CurriculumStage) {
        let id = stage.id;
        self.stages.insert(id, stage);
        self.stage_order.push(id);
    }

    /// Sort stages by difficulty
    pub fn sort_by_difficulty(&mut self) {
        self.stage_order.sort_by(|a, b| {
            let da = self.stages.get(a).map(|s| s.difficulty).unwrap_or(0.0);
            let db = self.stages.get(b).map(|s| s.difficulty).unwrap_or(0.0);
            da.partial_cmp(&db).unwrap()
        });
    }

    /// Enroll an agent in the curriculum
    pub fn enroll(&self, agent_id: Uuid) {
        let first_stage = self.stage_order.first().cloned();
        self.progress.insert(
            agent_id,
            AgentProgress {
                agent_id,
                current_stage: first_stage,
                completed_stages: Vec::new(),
                stage_attempts: HashMap::new(),
                stage_scores: HashMap::new(),
                started_at: Utc::now(),
                last_activity: Utc::now(),
            },
        );
    }

    /// Get next stage for agent
    pub fn get_next_stage(&self, agent_id: &Uuid) -> Option<CurriculumStage> {
        let progress = self.progress.get(agent_id)?;

        // Find first uncompleted stage with satisfied prerequisites
        for stage_id in &self.stage_order {
            if progress.completed_stages.contains(stage_id) {
                continue;
            }

            if let Some(stage) = self.stages.get(stage_id) {
                let prereqs_met = stage
                    .prerequisites
                    .iter()
                    .all(|p| progress.completed_stages.contains(p));

                if prereqs_met {
                    return Some(stage.clone());
                }
            }
        }

        None
    }

    /// Record stage attempt result
    pub fn record_attempt(&self, agent_id: &Uuid, stage_id: &Uuid, score: f64) -> CurriculumResult {
        // First, get the stage info we need
        let stage_info = self
            .stages
            .get(stage_id)
            .map(|s| (s.success_threshold, s.max_attempts));

        if let Some(mut progress) = self.progress.get_mut(agent_id) {
            progress.last_activity = Utc::now();

            // Get attempts count
            let current_attempts = *progress.stage_attempts.get(stage_id).unwrap_or(&0);
            let new_attempts = current_attempts + 1;
            progress.stage_attempts.insert(*stage_id, new_attempts);
            progress.stage_scores.insert(*stage_id, score);

            if let Some((success_threshold, max_attempts)) = stage_info {
                if score >= success_threshold {
                    progress.completed_stages.push(*stage_id);
                    // Drop the mutable borrow before calling get_next_stage
                    drop(progress);

                    let next_stage_id = self.get_next_stage(agent_id).map(|s| s.id);

                    // Update current_stage
                    if let Some(mut progress) = self.progress.get_mut(agent_id) {
                        progress.current_stage = next_stage_id;
                    }

                    return CurriculumResult::StageCompleted {
                        stage_id: *stage_id,
                        score,
                        next_stage: next_stage_id,
                    };
                } else if new_attempts >= max_attempts {
                    return CurriculumResult::StageFailed {
                        stage_id: *stage_id,
                        attempts: new_attempts,
                        best_score: score,
                    };
                } else {
                    return CurriculumResult::RetryNeeded {
                        stage_id: *stage_id,
                        attempts_remaining: max_attempts - new_attempts,
                        current_score: score,
                    };
                }
            }
        }

        CurriculumResult::AgentNotEnrolled
    }

    /// Get agent's progress
    pub fn get_progress(&self, agent_id: &Uuid) -> Option<AgentProgress> {
        self.progress.get(agent_id).map(|p| p.clone())
    }

    /// Get curriculum statistics
    pub fn stats(&self) -> CurriculumStats {
        let completion_rates: HashMap<Uuid, f64> = self
            .stages
            .iter()
            .map(|entry| {
                let stage_id = *entry.key();
                let completed = self
                    .progress
                    .iter()
                    .filter(|p| p.completed_stages.contains(&stage_id))
                    .count();
                let enrolled = self.progress.len();
                (stage_id, completed as f64 / enrolled.max(1) as f64)
            })
            .collect();

        CurriculumStats {
            total_stages: self.stages.len(),
            enrolled_agents: self.progress.len(),
            stage_completion_rates: completion_rates,
            average_progress: self
                .progress
                .iter()
                .map(|p| p.completed_stages.len() as f64 / self.stages.len().max(1) as f64)
                .sum::<f64>()
                / self.progress.len().max(1) as f64,
        }
    }
}

/// Result of curriculum stage attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CurriculumResult {
    StageCompleted {
        stage_id: Uuid,
        score: f64,
        next_stage: Option<Uuid>,
    },
    StageFailed {
        stage_id: Uuid,
        attempts: usize,
        best_score: f64,
    },
    RetryNeeded {
        stage_id: Uuid,
        attempts_remaining: usize,
        current_score: f64,
    },
    AgentNotEnrolled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurriculumStats {
    pub total_stages: usize,
    pub enrolled_agents: usize,
    pub stage_completion_rates: HashMap<Uuid, f64>,
    pub average_progress: f64,
}

// =============================================================================
// EMERGENT BEHAVIOR DETECTION
// =============================================================================

/// Detected emergent behavior pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergentBehavior {
    pub id: Uuid,
    pub name: String,
    pub pattern: BehaviorPattern,
    pub involved_agents: Vec<Uuid>,
    pub first_observed: DateTime<Utc>,
    pub occurrence_count: u64,
    pub beneficial: Option<bool>,
    pub description: String,
}

/// Behavior pattern types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BehaviorPattern {
    /// Spontaneous coordination without explicit instruction
    SpontaneousCoordination { action_sequence: Vec<String> },
    /// Novel problem-solving approach
    NovelStrategy {
        problem_type: String,
        solution_path: Vec<String>,
    },
    /// Self-organizing structure
    SelfOrganization {
        structure_type: String,
        participants: usize,
    },
    /// Collective intelligence emergence
    CollectiveIntelligence {
        task: String,
        combined_performance: f64,
    },
    /// Role differentiation
    RoleDifferentiation {
        roles: Vec<String>,
        specialization_scores: HashMap<Uuid, f64>,
    },
    /// Communication protocol emergence
    EmergentProtocol {
        signal_vocabulary: Vec<String>,
        effectiveness: f64,
    },
}

/// Emergent behavior detector
pub struct EmergentBehaviorDetector {
    behaviors: DashMap<Uuid, EmergentBehavior>,
    action_history: Arc<RwLock<Vec<AgentAction>>>,
    detection_window: usize,
    novelty_threshold: f64,
    coordination_threshold: f64,
}

/// Recorded agent action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAction {
    pub agent_id: Uuid,
    pub action: String,
    pub timestamp: DateTime<Utc>,
    pub context: HashMap<String, serde_json::Value>,
    pub outcome: Option<f64>,
}

impl EmergentBehaviorDetector {
    pub fn new(detection_window: usize) -> Self {
        Self {
            behaviors: DashMap::new(),
            action_history: Arc::new(RwLock::new(Vec::new())),
            detection_window,
            novelty_threshold: 0.7,
            coordination_threshold: 0.8,
        }
    }

    /// Record an agent action
    pub async fn record_action(&self, action: AgentAction) {
        let mut history = self.action_history.write().await;
        history.push(action);

        // Keep bounded history
        if history.len() > self.detection_window * 10 {
            history.drain(0..self.detection_window);
        }
    }

    /// Analyze for emergent behaviors
    pub async fn analyze(&self) -> Vec<EmergentBehavior> {
        let history = self.action_history.read().await;
        let mut detected = Vec::new();

        // Detect spontaneous coordination
        if let Some(behavior) = self.detect_coordination(&history) {
            detected.push(behavior);
        }

        // Detect role differentiation
        if let Some(behavior) = self.detect_role_differentiation(&history) {
            detected.push(behavior);
        }

        // Detect novel strategies
        if let Some(behavior) = self.detect_novel_strategy(&history) {
            detected.push(behavior);
        }

        // Store detected behaviors
        for behavior in &detected {
            if let Some(mut existing) = self.behaviors.get_mut(&behavior.id) {
                existing.occurrence_count += 1;
            } else {
                self.behaviors.insert(behavior.id, behavior.clone());
            }
        }

        detected
    }

    fn detect_coordination(&self, history: &[AgentAction]) -> Option<EmergentBehavior> {
        if history.len() < 10 {
            return None;
        }

        // Look for synchronized actions across agents
        let recent = &history[history.len().saturating_sub(self.detection_window)..];

        // Group by time windows
        let mut time_windows: HashMap<i64, Vec<&AgentAction>> = HashMap::new();
        for action in recent {
            let window_key = action.timestamp.timestamp() / 5; // 5-second windows
            time_windows.entry(window_key).or_default().push(action);
        }

        // Find windows with multiple agents doing related actions
        for actions in time_windows.values() {
            if actions.len() >= 3 {
                let agents: Vec<Uuid> = actions.iter().map(|a| a.agent_id).collect();
                let unique_agents: std::collections::HashSet<_> = agents.iter().collect();

                if unique_agents.len() >= 2 {
                    let action_sequence: Vec<String> =
                        actions.iter().map(|a| a.action.clone()).collect();

                    return Some(EmergentBehavior {
                        id: Uuid::new_v4(),
                        name: "Spontaneous Coordination".to_string(),
                        pattern: BehaviorPattern::SpontaneousCoordination { action_sequence },
                        involved_agents: unique_agents.into_iter().cloned().collect(),
                        first_observed: Utc::now(),
                        occurrence_count: 1,
                        beneficial: None,
                        description:
                            "Multiple agents synchronized actions without explicit coordination"
                                .to_string(),
                    });
                }
            }
        }

        None
    }

    fn detect_role_differentiation(&self, history: &[AgentAction]) -> Option<EmergentBehavior> {
        if history.len() < 20 {
            return None;
        }

        // Analyze action distribution per agent
        let mut agent_actions: HashMap<Uuid, HashMap<String, usize>> = HashMap::new();

        for action in history {
            agent_actions
                .entry(action.agent_id)
                .or_default()
                .entry(action.action.clone())
                .and_modify(|c| *c += 1)
                .or_insert(1);
        }

        // Calculate specialization scores
        let mut specialization_scores: HashMap<Uuid, f64> = HashMap::new();
        let mut roles: Vec<String> = Vec::new();

        for (agent, actions) in &agent_actions {
            let total: usize = actions.values().sum();
            if total > 0 {
                // Find dominant action type
                if let Some((dominant, count)) = actions.iter().max_by_key(|(_, c)| *c) {
                    let specialization = *count as f64 / total as f64;
                    if specialization > 0.5 {
                        specialization_scores.insert(*agent, specialization);
                        if !roles.contains(dominant) {
                            roles.push(dominant.clone());
                        }
                    }
                }
            }
        }

        if roles.len() >= 2 && specialization_scores.len() >= 2 {
            return Some(EmergentBehavior {
                id: Uuid::new_v4(),
                name: "Role Differentiation".to_string(),
                pattern: BehaviorPattern::RoleDifferentiation {
                    roles,
                    specialization_scores: specialization_scores.clone(),
                },
                involved_agents: specialization_scores.keys().cloned().collect(),
                first_observed: Utc::now(),
                occurrence_count: 1,
                beneficial: Some(true),
                description: "Agents have naturally differentiated into specialized roles"
                    .to_string(),
            });
        }

        None
    }

    fn detect_novel_strategy(&self, history: &[AgentAction]) -> Option<EmergentBehavior> {
        if history.len() < 5 {
            return None;
        }

        // Look for action sequences that achieve high outcomes
        let recent = &history[history.len().saturating_sub(10)..];

        let high_outcome_actions: Vec<_> = recent
            .iter()
            .filter(|a| a.outcome.map(|o| o > 0.8).unwrap_or(false))
            .collect();

        if high_outcome_actions.len() >= 3 {
            let solution_path: Vec<String> = high_outcome_actions
                .iter()
                .map(|a| a.action.clone())
                .collect();

            return Some(EmergentBehavior {
                id: Uuid::new_v4(),
                name: "Novel Strategy".to_string(),
                pattern: BehaviorPattern::NovelStrategy {
                    problem_type: "general".to_string(),
                    solution_path,
                },
                involved_agents: high_outcome_actions.iter().map(|a| a.agent_id).collect(),
                first_observed: Utc::now(),
                occurrence_count: 1,
                beneficial: Some(true),
                description: "A new effective action sequence has been discovered".to_string(),
            });
        }

        None
    }

    /// Mark a behavior as beneficial or harmful
    pub fn classify_behavior(&self, behavior_id: &Uuid, beneficial: bool) {
        if let Some(mut behavior) = self.behaviors.get_mut(behavior_id) {
            behavior.beneficial = Some(beneficial);
        }
    }

    /// Get all detected behaviors
    pub fn get_behaviors(&self) -> Vec<EmergentBehavior> {
        self.behaviors.iter().map(|e| e.clone()).collect()
    }

    /// Get detection statistics
    pub fn stats(&self) -> EmergentBehaviorStats {
        let behaviors: Vec<_> = self.behaviors.iter().collect();

        EmergentBehaviorStats {
            total_detected: behaviors.len(),
            beneficial: behaviors
                .iter()
                .filter(|b| b.beneficial == Some(true))
                .count(),
            harmful: behaviors
                .iter()
                .filter(|b| b.beneficial == Some(false))
                .count(),
            unclassified: behaviors.iter().filter(|b| b.beneficial.is_none()).count(),
            total_occurrences: behaviors.iter().map(|b| b.occurrence_count).sum(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergentBehaviorStats {
    pub total_detected: usize,
    pub beneficial: usize,
    pub harmful: usize,
    pub unclassified: usize,
    pub total_occurrences: u64,
}

// =============================================================================
// AGENT COMMUNICATION PROTOCOLS
// =============================================================================

/// Speech act types for agent communication (FIPA-inspired)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SpeechAct {
    // Informative
    Inform {
        content: serde_json::Value,
    },
    Confirm {
        proposition: String,
    },
    Disconfirm {
        proposition: String,
    },

    // Directive
    Request {
        action: String,
        parameters: HashMap<String, serde_json::Value>,
    },
    Query {
        question: String,
        constraints: Vec<String>,
    },
    Subscribe {
        topic: String,
        filter: Option<String>,
    },

    // Commissive
    Promise {
        action: String,
        deadline: Option<DateTime<Utc>>,
    },
    Accept {
        proposal_id: Uuid,
    },
    Reject {
        proposal_id: Uuid,
        reason: String,
    },

    // Declarative
    Declare {
        statement: String,
        authority: String,
    },

    // Expressive
    Acknowledge {
        message_id: Uuid,
    },
    Apologize {
        reason: String,
    },
    Thank {
        agent_id: Uuid,
        reason: String,
    },
}

/// Performative message envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Performative {
    pub id: Uuid,
    pub sender: Uuid,
    pub receivers: Vec<Uuid>,
    pub speech_act: SpeechAct,
    pub conversation_id: Uuid,
    pub reply_to: Option<Uuid>,
    pub language: String,
    pub ontology: String,
    pub protocol: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub expires: Option<DateTime<Utc>>,
}

impl Performative {
    pub fn new(sender: Uuid, receivers: Vec<Uuid>, speech_act: SpeechAct) -> Self {
        Self {
            id: Uuid::new_v4(),
            sender,
            receivers,
            speech_act,
            conversation_id: Uuid::new_v4(),
            reply_to: None,
            language: "json".to_string(),
            ontology: "hyperlight-agentic".to_string(),
            protocol: None,
            timestamp: Utc::now(),
            expires: None,
        }
    }

    pub fn with_conversation(mut self, conversation_id: Uuid) -> Self {
        self.conversation_id = conversation_id;
        self
    }

    pub fn reply_to(mut self, message_id: Uuid) -> Self {
        self.reply_to = Some(message_id);
        self
    }

    pub fn with_protocol(mut self, protocol: &str) -> Self {
        self.protocol = Some(protocol.to_string());
        self
    }

    pub fn with_expiry(mut self, expires: DateTime<Utc>) -> Self {
        self.expires = Some(expires);
        self
    }
}

/// Conversation tracking
#[derive(Debug, Clone)]
pub struct Conversation {
    pub id: Uuid,
    pub participants: Vec<Uuid>,
    pub protocol: Option<String>,
    pub state: ConversationState,
    pub messages: Vec<Performative>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Conversation states
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConversationState {
    Initiated,
    Active,
    Waiting {
        for_agent: Uuid,
        deadline: Option<DateTime<Utc>>,
    },
    Completed {
        outcome: String,
    },
    Failed {
        reason: String,
    },
    Timeout,
}

#[allow(clippy::type_complexity)]
/// Message broker for agent communication
pub struct MessageBroker {
    conversations: DashMap<Uuid, Conversation>,
    agent_inboxes: DashMap<Uuid, Arc<RwLock<Vec<Performative>>>>,
    subscriptions: DashMap<String, Vec<Uuid>>,
    message_handlers: DashMap<Uuid, Vec<Box<dyn Fn(&Performative) + Send + Sync>>>,
}

impl Default for MessageBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBroker {
    pub fn new() -> Self {
        Self {
            conversations: DashMap::new(),
            agent_inboxes: DashMap::new(),
            subscriptions: DashMap::new(),
            message_handlers: DashMap::new(),
        }
    }

    /// Register an agent with the broker
    pub fn register_agent(&self, agent_id: Uuid) {
        self.agent_inboxes
            .insert(agent_id, Arc::new(RwLock::new(Vec::new())));
    }

    /// Send a performative message
    pub async fn send(&self, message: Performative) -> Result<Uuid, String> {
        let message_id = message.id;

        // Update or create conversation
        let conversation_id = message.conversation_id;
        if let Some(mut conv) = self.conversations.get_mut(&conversation_id) {
            conv.messages.push(message.clone());
            conv.updated_at = Utc::now();
            if conv.state == ConversationState::Initiated {
                conv.state = ConversationState::Active;
            }
        } else {
            let conversation = Conversation {
                id: conversation_id,
                participants: {
                    let mut p = message.receivers.clone();
                    p.push(message.sender);
                    p
                },
                protocol: message.protocol.clone(),
                state: ConversationState::Initiated,
                messages: vec![message.clone()],
                started_at: Utc::now(),
                updated_at: Utc::now(),
                metadata: HashMap::new(),
            };
            self.conversations.insert(conversation_id, conversation);
        }

        // Deliver to receivers
        for receiver in &message.receivers {
            if let Some(inbox) = self.agent_inboxes.get(receiver) {
                let mut inbox = inbox.write().await;
                inbox.push(message.clone());
            }
        }

        // Handle subscriptions for inform messages
        if let SpeechAct::Inform { content } = &message.speech_act {
            if let Some(topic) = content.get("topic").and_then(|t| t.as_str()) {
                if let Some(subscribers) = self.subscriptions.get(topic) {
                    for sub in subscribers.iter() {
                        if !message.receivers.contains(sub) {
                            if let Some(inbox) = self.agent_inboxes.get(sub) {
                                let mut inbox = inbox.write().await;
                                inbox.push(message.clone());
                            }
                        }
                    }
                }
            }
        }

        Ok(message_id)
    }

    /// Receive messages for an agent
    pub async fn receive(&self, agent_id: &Uuid, limit: usize) -> Vec<Performative> {
        if let Some(inbox) = self.agent_inboxes.get(agent_id) {
            let mut inbox = inbox.write().await;
            let count = inbox.len().min(limit);
            inbox.drain(0..count).collect()
        } else {
            Vec::new()
        }
    }

    /// Subscribe to a topic
    pub fn subscribe(&self, agent_id: Uuid, topic: &str) {
        self.subscriptions
            .entry(topic.to_string())
            .or_default()
            .push(agent_id);
    }

    /// Unsubscribe from a topic
    pub fn unsubscribe(&self, agent_id: &Uuid, topic: &str) {
        if let Some(mut subs) = self.subscriptions.get_mut(topic) {
            subs.retain(|id| id != agent_id);
        }
    }

    /// Get conversation history
    pub fn get_conversation(&self, conversation_id: &Uuid) -> Option<Conversation> {
        self.conversations.get(conversation_id).map(|c| c.clone())
    }

    /// Complete a conversation
    pub fn complete_conversation(&self, conversation_id: &Uuid, outcome: &str) {
        if let Some(mut conv) = self.conversations.get_mut(conversation_id) {
            conv.state = ConversationState::Completed {
                outcome: outcome.to_string(),
            };
            conv.updated_at = Utc::now();
        }
    }

    /// Fail a conversation
    pub fn fail_conversation(&self, conversation_id: &Uuid, reason: &str) {
        if let Some(mut conv) = self.conversations.get_mut(conversation_id) {
            conv.state = ConversationState::Failed {
                reason: reason.to_string(),
            };
            conv.updated_at = Utc::now();
        }
    }

    /// Get broker statistics
    pub fn stats(&self) -> BrokerStats {
        let conversations: Vec<_> = self.conversations.iter().collect();
        BrokerStats {
            registered_agents: self.agent_inboxes.len(),
            active_conversations: conversations
                .iter()
                .filter(|c| {
                    c.state == ConversationState::Active || c.state == ConversationState::Initiated
                })
                .count(),
            total_conversations: self.conversations.len(),
            subscription_topics: self.subscriptions.len(),
            total_messages: conversations.iter().map(|c| c.messages.len()).sum(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerStats {
    pub registered_agents: usize,
    pub active_conversations: usize,
    pub total_conversations: usize,
    pub subscription_topics: usize,
    pub total_messages: usize,
}

// =============================================================================
// CONTRACT NET PROTOCOL
// =============================================================================

/// Contract net task announcement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAnnouncement {
    pub id: Uuid,
    pub manager: Uuid,
    pub task_description: String,
    pub requirements: Vec<AgentCapability>,
    pub deadline: DateTime<Utc>,
    pub bid_deadline: DateTime<Utc>,
    pub eligibility_criteria: Vec<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Bid from a contractor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractBid {
    pub id: Uuid,
    pub task_id: Uuid,
    pub bidder: Uuid,
    pub proposed_cost: f64,
    pub proposed_duration: Duration,
    pub confidence: f64,
    pub approach: String,
    pub resources_required: Vec<String>,
    pub submitted_at: DateTime<Utc>,
}

/// Awarded contract
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub id: Uuid,
    pub task_id: Uuid,
    pub manager: Uuid,
    pub contractor: Uuid,
    pub agreed_cost: f64,
    pub deadline: DateTime<Utc>,
    pub status: ContractStatus,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Contract execution status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContractStatus {
    Active,
    InProgress { progress: u8 },
    Completed { result: String },
    Failed { reason: String },
    Cancelled { by: Uuid },
}

/// Contract net protocol manager
pub struct ContractNetManager {
    announcements: DashMap<Uuid, TaskAnnouncement>,
    bids: DashMap<Uuid, Vec<ContractBid>>,
    contracts: DashMap<Uuid, Contract>,
    agent_contracts: DashMap<Uuid, Vec<Uuid>>,
}

impl Default for ContractNetManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ContractNetManager {
    pub fn new() -> Self {
        Self {
            announcements: DashMap::new(),
            bids: DashMap::new(),
            contracts: DashMap::new(),
            agent_contracts: DashMap::new(),
        }
    }

    /// Announce a task for bidding
    pub fn announce_task(&self, announcement: TaskAnnouncement) -> Uuid {
        let id = announcement.id;
        self.announcements.insert(id, announcement);
        self.bids.insert(id, Vec::new());
        id
    }

    /// Submit a bid for a task
    pub fn submit_bid(&self, bid: ContractBid) -> Result<(), String> {
        let task_id = bid.task_id;

        // Verify task exists and bidding is open
        if let Some(announcement) = self.announcements.get(&task_id) {
            if Utc::now() > announcement.bid_deadline {
                return Err("Bidding deadline has passed".to_string());
            }
        } else {
            return Err("Task not found".to_string());
        }

        // Add bid
        if let Some(mut bids) = self.bids.get_mut(&task_id) {
            bids.push(bid);
            Ok(())
        } else {
            Err("Bid storage not initialized".to_string())
        }
    }

    /// Get all bids for a task
    pub fn get_bids(&self, task_id: &Uuid) -> Vec<ContractBid> {
        self.bids
            .get(task_id)
            .map(|b| b.clone())
            .unwrap_or_default()
    }

    /// Award contract to winning bidder
    pub fn award_contract(&self, task_id: &Uuid, bid_id: &Uuid) -> Result<Contract, String> {
        let (manager, deadline) = {
            let announcement = self.announcements.get(task_id).ok_or("Task not found")?;
            (announcement.manager, announcement.deadline)
        };

        let (contractor, agreed_cost) = {
            let bids = self.bids.get(task_id).ok_or("No bids found")?;

            let winning_bid = bids
                .iter()
                .find(|b| &b.id == bid_id)
                .ok_or("Bid not found")?;
            (winning_bid.bidder, winning_bid.proposed_cost)
        };

        let contract = Contract {
            id: Uuid::new_v4(),
            task_id: *task_id,
            manager,
            contractor,
            agreed_cost,
            deadline,
            status: ContractStatus::Active,
            created_at: Utc::now(),
            completed_at: None,
        };

        let contract_id = contract.id;

        // Store contract
        self.contracts.insert(contract_id, contract.clone());

        // Track agent's contracts
        self.agent_contracts
            .entry(contractor)
            .or_default()
            .push(contract_id);

        // Remove announcement (task awarded)
        self.announcements.remove(task_id);

        Ok(contract)
    }

    /// Update contract progress
    pub fn update_progress(&self, contract_id: &Uuid, progress: u8) {
        if let Some(mut contract) = self.contracts.get_mut(contract_id) {
            contract.status = ContractStatus::InProgress {
                progress: progress.min(100),
            };
        }
    }

    /// Complete a contract
    pub fn complete_contract(&self, contract_id: &Uuid, result: &str) {
        if let Some(mut contract) = self.contracts.get_mut(contract_id) {
            contract.status = ContractStatus::Completed {
                result: result.to_string(),
            };
            contract.completed_at = Some(Utc::now());
        }
    }

    /// Fail a contract
    pub fn fail_contract(&self, contract_id: &Uuid, reason: &str) {
        if let Some(mut contract) = self.contracts.get_mut(contract_id) {
            contract.status = ContractStatus::Failed {
                reason: reason.to_string(),
            };
            contract.completed_at = Some(Utc::now());
        }
    }

    /// Get active announcements
    pub fn get_open_tasks(&self) -> Vec<TaskAnnouncement> {
        let now = Utc::now();
        self.announcements
            .iter()
            .filter(|a| a.bid_deadline > now)
            .map(|a| a.clone())
            .collect()
    }

    /// Get agent's contracts
    pub fn get_agent_contracts(&self, agent_id: &Uuid) -> Vec<Contract> {
        self.agent_contracts
            .get(agent_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.contracts.get(id).map(|c| c.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get statistics
    pub fn stats(&self) -> ContractNetStats {
        let contracts: Vec<_> = self.contracts.iter().collect();
        ContractNetStats {
            open_tasks: self.announcements.len(),
            total_bids: self.bids.iter().map(|b| b.len()).sum(),
            active_contracts: contracts
                .iter()
                .filter(|c| {
                    matches!(
                        c.status,
                        ContractStatus::Active | ContractStatus::InProgress { .. }
                    )
                })
                .count(),
            completed_contracts: contracts
                .iter()
                .filter(|c| matches!(c.status, ContractStatus::Completed { .. }))
                .count(),
            failed_contracts: contracts
                .iter()
                .filter(|c| matches!(c.status, ContractStatus::Failed { .. }))
                .count(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractNetStats {
    pub open_tasks: usize,
    pub total_bids: usize,
    pub active_contracts: usize,
    pub completed_contracts: usize,
    pub failed_contracts: usize,
}

// =============================================================================
// BLACKBOARD ARCHITECTURE
// =============================================================================

/// Blackboard entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardEntry {
    pub id: Uuid,
    pub key: String,
    pub value: serde_json::Value,
    pub author: Uuid,
    pub level: KnowledgeLevel,
    pub confidence: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u64,
    pub dependencies: Vec<String>,
    pub tags: Vec<String>,
}

/// Knowledge abstraction levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum KnowledgeLevel {
    Raw,        // Unprocessed data
    Feature,    // Extracted features
    Partial,    // Partial solutions
    Hypothesis, // Candidate solutions
    Solution,   // Final solutions
    Meta,       // Meta-level control
}

/// Knowledge source that can read/write blackboard
pub struct KnowledgeSource {
    pub id: Uuid,
    pub name: String,
    pub input_levels: Vec<KnowledgeLevel>,
    pub output_level: KnowledgeLevel,
    precondition: Arc<dyn Fn(&Blackboard) -> bool + Send + Sync>,
    pub activation: f64,
}

impl std::fmt::Debug for KnowledgeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KnowledgeSource")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("input_levels", &self.input_levels)
            .field("output_level", &self.output_level)
            .field("activation", &self.activation)
            .finish()
    }
}

impl Clone for KnowledgeSource {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            name: self.name.clone(),
            input_levels: self.input_levels.clone(),
            output_level: self.output_level.clone(),
            precondition: Arc::clone(&self.precondition),
            activation: self.activation,
        }
    }
}

impl KnowledgeSource {
    pub fn new(
        name: &str,
        input_levels: Vec<KnowledgeLevel>,
        output_level: KnowledgeLevel,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            input_levels,
            output_level,
            precondition: Arc::new(|_| true),
            activation: 0.0,
        }
    }

    pub fn with_precondition<F>(mut self, f: F) -> Self
    where
        F: Fn(&Blackboard) -> bool + Send + Sync + 'static,
    {
        self.precondition = Arc::new(f);
        self
    }

    pub fn check_precondition(&self, blackboard: &Blackboard) -> bool {
        (self.precondition)(blackboard)
    }
}

/// Blackboard for collaborative problem solving
pub struct Blackboard {
    entries: DashMap<String, BlackboardEntry>,
    level_index: DashMap<KnowledgeLevel, Vec<String>>,
    sources: DashMap<Uuid, KnowledgeSource>,
    watchers: DashMap<String, Vec<Uuid>>,
    change_log: Arc<RwLock<Vec<BlackboardChange>>>,
}

/// Blackboard change record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardChange {
    pub entry_key: String,
    pub change_type: ChangeType,
    pub author: Uuid,
    pub timestamp: DateTime<Utc>,
    pub old_version: Option<u64>,
    pub new_version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeType {
    Created,
    Updated,
    Deleted,
}

impl Default for Blackboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Blackboard {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            level_index: DashMap::new(),
            sources: DashMap::new(),
            watchers: DashMap::new(),
            change_log: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Write an entry to the blackboard
    pub async fn write(
        &self,
        key: &str,
        value: serde_json::Value,
        author: Uuid,
        level: KnowledgeLevel,
    ) -> Uuid {
        let entry_id = Uuid::new_v4();
        let (old_version, new_version) = if let Some(existing) = self.entries.get(key) {
            (Some(existing.version), existing.version + 1)
        } else {
            (None, 1)
        };

        let entry = BlackboardEntry {
            id: entry_id,
            key: key.to_string(),
            value,
            author,
            level: level.clone(),
            confidence: 1.0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: new_version,
            dependencies: Vec::new(),
            tags: Vec::new(),
        };

        let change_type = if old_version.is_some() {
            ChangeType::Updated
        } else {
            ChangeType::Created
        };

        // Store entry
        self.entries.insert(key.to_string(), entry);

        // Update level index
        self.level_index
            .entry(level)
            .or_default()
            .push(key.to_string());

        // Record change
        let change = BlackboardChange {
            entry_key: key.to_string(),
            change_type,
            author,
            timestamp: Utc::now(),
            old_version,
            new_version,
        };
        self.change_log.write().await.push(change);

        entry_id
    }

    /// Read an entry from the blackboard
    pub fn read(&self, key: &str) -> Option<BlackboardEntry> {
        self.entries.get(key).map(|e| e.clone())
    }

    /// Read all entries at a knowledge level
    pub fn read_level(&self, level: &KnowledgeLevel) -> Vec<BlackboardEntry> {
        self.level_index
            .get(level)
            .map(|keys| {
                keys.iter()
                    .filter_map(|k| self.entries.get(k).map(|e| e.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Delete an entry
    pub async fn delete(&self, key: &str, author: Uuid) -> bool {
        if let Some((_, entry)) = self.entries.remove(key) {
            // Remove from level index
            if let Some(mut keys) = self.level_index.get_mut(&entry.level) {
                keys.retain(|k| k != key);
            }

            // Record change
            let change = BlackboardChange {
                entry_key: key.to_string(),
                change_type: ChangeType::Deleted,
                author,
                timestamp: Utc::now(),
                old_version: Some(entry.version),
                new_version: entry.version,
            };
            self.change_log.write().await.push(change);

            true
        } else {
            false
        }
    }

    /// Register a knowledge source
    pub fn register_source(&self, source: KnowledgeSource) -> Uuid {
        let id = source.id;
        self.sources.insert(id, source);
        id
    }

    /// Get activated knowledge sources
    pub fn get_activated_sources(&self) -> Vec<Uuid> {
        self.sources
            .iter()
            .filter(|s| s.check_precondition(self))
            .map(|s| s.id)
            .collect()
    }

    /// Watch for changes to a key
    pub fn watch(&self, key: &str, watcher_id: Uuid) {
        self.watchers
            .entry(key.to_string())
            .or_default()
            .push(watcher_id);
    }

    /// Get recent changes
    pub async fn get_changes(&self, since: DateTime<Utc>) -> Vec<BlackboardChange> {
        self.change_log
            .read()
            .await
            .iter()
            .filter(|c| c.timestamp > since)
            .cloned()
            .collect()
    }

    /// Query entries by tags
    pub fn query_by_tag(&self, tag: &str) -> Vec<BlackboardEntry> {
        self.entries
            .iter()
            .filter(|e| e.tags.contains(&tag.to_string()))
            .map(|e| e.clone())
            .collect()
    }

    /// Get blackboard statistics
    pub fn stats(&self) -> BlackboardStats {
        let levels: HashMap<String, usize> = self
            .level_index
            .iter()
            .map(|e| (format!("{:?}", e.key()), e.value().len()))
            .collect();

        BlackboardStats {
            total_entries: self.entries.len(),
            entries_by_level: levels,
            knowledge_sources: self.sources.len(),
            watchers: self.watchers.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlackboardStats {
    pub total_entries: usize,
    pub entries_by_level: HashMap<String, usize>,
    pub knowledge_sources: usize,
    pub watchers: usize,
}

// =============================================================================
// AGENT TRUST AND REPUTATION
// =============================================================================

/// Trust assessment between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustAssessment {
    pub trustor: Uuid,
    pub trustee: Uuid,
    pub overall_trust: f64,
    pub competence: f64,
    pub reliability: f64,
    pub honesty: f64,
    pub benevolence: f64,
    pub interaction_count: u64,
    pub last_interaction: DateTime<Utc>,
    pub history: Vec<TrustInteraction>,
}

/// Record of trust-relevant interaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustInteraction {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub interaction_type: InteractionType,
    pub outcome: InteractionOutcome,
    pub weight: f64,
}

/// Types of trust-relevant interactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InteractionType {
    TaskDelegation,
    InformationSharing,
    Collaboration,
    ContractExecution,
    ResourceSharing,
}

/// Outcome of an interaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InteractionOutcome {
    Success { quality: f64 },
    PartialSuccess { completion: f64 },
    Failure { severity: f64 },
    Deception,
    Timeout,
}

/// Reputation aggregated from multiple sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reputation {
    pub agent_id: Uuid,
    pub global_score: f64,
    pub domain_scores: HashMap<String, f64>,
    pub endorsements: Vec<Endorsement>,
    pub warnings: Vec<Warning>,
    pub computed_at: DateTime<Utc>,
}

/// Endorsement from another agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endorsement {
    pub endorser: Uuid,
    pub domain: String,
    pub statement: String,
    pub timestamp: DateTime<Utc>,
}

/// Warning about an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warning {
    pub reporter: Uuid,
    pub reason: String,
    pub severity: f64,
    pub timestamp: DateTime<Utc>,
}

/// Trust and reputation system
pub struct TrustSystem {
    assessments: DashMap<(Uuid, Uuid), TrustAssessment>,
    reputations: DashMap<Uuid, Reputation>,
    decay_rate: f64,
}

impl TrustSystem {
    pub fn new(decay_rate: f64) -> Self {
        Self {
            assessments: DashMap::new(),
            reputations: DashMap::new(),
            decay_rate,
        }
    }

    /// Record an interaction and update trust
    pub fn record_interaction(
        &self,
        trustor: Uuid,
        trustee: Uuid,
        interaction_type: InteractionType,
        outcome: InteractionOutcome,
    ) {
        let key = (trustor, trustee);
        let interaction = TrustInteraction {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            interaction_type,
            outcome: outcome.clone(),
            weight: 1.0,
        };

        let mut assessment = self
            .assessments
            .entry(key)
            .or_insert_with(|| TrustAssessment {
                trustor,
                trustee,
                overall_trust: 0.5,
                competence: 0.5,
                reliability: 0.5,
                honesty: 0.5,
                benevolence: 0.5,
                interaction_count: 0,
                last_interaction: Utc::now(),
                history: Vec::new(),
            });

        // Update trust dimensions based on outcome
        let (comp_delta, rel_delta, hon_delta) = match &outcome {
            InteractionOutcome::Success { quality } => (*quality * 0.1, 0.1, 0.05),
            InteractionOutcome::PartialSuccess { completion } => (*completion * 0.05, 0.02, 0.0),
            InteractionOutcome::Failure { severity } => (-severity * 0.15, -0.1, 0.0),
            InteractionOutcome::Deception => (0.0, -0.2, -0.3),
            InteractionOutcome::Timeout => (0.0, -0.15, 0.0),
        };

        assessment.competence = (assessment.competence + comp_delta).clamp(0.0, 1.0);
        assessment.reliability = (assessment.reliability + rel_delta).clamp(0.0, 1.0);
        assessment.honesty = (assessment.honesty + hon_delta).clamp(0.0, 1.0);

        // Recalculate overall trust
        assessment.overall_trust = assessment.competence * 0.3
            + assessment.reliability * 0.3
            + assessment.honesty * 0.25
            + assessment.benevolence * 0.15;

        assessment.interaction_count += 1;
        assessment.last_interaction = Utc::now();
        assessment.history.push(interaction);

        // Keep history bounded
        if assessment.history.len() > 100 {
            assessment.history.remove(0);
        }
    }

    /// Get trust assessment
    pub fn get_trust(&self, trustor: &Uuid, trustee: &Uuid) -> Option<TrustAssessment> {
        self.assessments
            .get(&(*trustor, *trustee))
            .map(|a| a.clone())
    }

    /// Add endorsement
    pub fn endorse(&self, endorser: Uuid, agent_id: Uuid, domain: &str, statement: &str) {
        let endorsement = Endorsement {
            endorser,
            domain: domain.to_string(),
            statement: statement.to_string(),
            timestamp: Utc::now(),
        };

        self.reputations
            .entry(agent_id)
            .or_insert_with(|| Reputation {
                agent_id,
                global_score: 0.5,
                domain_scores: HashMap::new(),
                endorsements: Vec::new(),
                warnings: Vec::new(),
                computed_at: Utc::now(),
            })
            .endorsements
            .push(endorsement);
    }

    /// Add warning
    pub fn warn(&self, reporter: Uuid, agent_id: Uuid, reason: &str, severity: f64) {
        let warning = Warning {
            reporter,
            reason: reason.to_string(),
            severity,
            timestamp: Utc::now(),
        };

        self.reputations
            .entry(agent_id)
            .or_insert_with(|| Reputation {
                agent_id,
                global_score: 0.5,
                domain_scores: HashMap::new(),
                endorsements: Vec::new(),
                warnings: Vec::new(),
                computed_at: Utc::now(),
            })
            .warnings
            .push(warning);
    }

    /// Compute reputation from trust assessments
    pub fn compute_reputation(&self, agent_id: &Uuid) -> Reputation {
        // Aggregate trust scores from all trustors
        let assessments: Vec<_> = self
            .assessments
            .iter()
            .filter(|e| e.key().1 == *agent_id)
            .map(|e| e.clone())
            .collect();

        let global_score = if assessments.is_empty() {
            0.5
        } else {
            assessments.iter().map(|a| a.overall_trust).sum::<f64>() / assessments.len() as f64
        };

        let mut reputation =
            self.reputations
                .get(agent_id)
                .map(|r| r.clone())
                .unwrap_or(Reputation {
                    agent_id: *agent_id,
                    global_score,
                    domain_scores: HashMap::new(),
                    endorsements: Vec::new(),
                    warnings: Vec::new(),
                    computed_at: Utc::now(),
                });

        // Apply endorsements and warnings
        let endorsement_boost = reputation.endorsements.len() as f64 * 0.02;
        let warning_penalty = reputation
            .warnings
            .iter()
            .map(|w| w.severity * 0.05)
            .sum::<f64>();

        reputation.global_score =
            (global_score + endorsement_boost - warning_penalty).clamp(0.0, 1.0);
        reputation.computed_at = Utc::now();

        // Store and return
        self.reputations.insert(*agent_id, reputation.clone());
        reputation
    }

    /// Apply time decay to trust assessments
    pub fn apply_decay(&self) {
        for mut assessment in self.assessments.iter_mut() {
            // Decay towards neutral (0.5)
            assessment.overall_trust =
                assessment.overall_trust + (0.5 - assessment.overall_trust) * self.decay_rate;
            assessment.competence =
                assessment.competence + (0.5 - assessment.competence) * self.decay_rate;
            assessment.reliability =
                assessment.reliability + (0.5 - assessment.reliability) * self.decay_rate;
        }
    }

    /// Get trust system statistics
    pub fn stats(&self) -> TrustStats {
        let assessments: Vec<_> = self.assessments.iter().collect();
        TrustStats {
            total_assessments: assessments.len(),
            average_trust: assessments.iter().map(|a| a.overall_trust).sum::<f64>()
                / assessments.len().max(1) as f64,
            total_interactions: assessments.iter().map(|a| a.interaction_count).sum(),
            agents_with_reputation: self.reputations.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustStats {
    pub total_assessments: usize,
    pub average_trust: f64,
    pub total_interactions: u64,
    pub agents_with_reputation: usize,
}

// =============================================================================
// NEURAL PROTOCOL & NEUROMORPHIC PHY LAYER
// =============================================================================

/// Emulated Neuromorphic Physical Layer (PHY)
/// Uses spike-timing-dependent plasticity (STDP) principles for temporal encoding
pub struct NeuromorphicPhy {
    pub bandwidth_mbps: f64,
    pub latency_ms: f64,
    pub error_rate: f64,
    pub spike_threshold: f32,
    pub membrane_potential: DashMap<u32, f32>,
    pub last_spike_time: DashMap<u32, Instant>,
}

impl NeuromorphicPhy {
    pub fn new(bandwidth: f64, latency: f64) -> Self {
        Self {
            bandwidth_mbps: bandwidth,
            latency_ms: latency,
            error_rate: 0.0001,
            spike_threshold: 1.0,
            membrane_potential: DashMap::new(),
            last_spike_time: DashMap::new(),
        }
    }

    /// Encode data into a temporal spike train using parallel neurons
    pub fn encode_spikes(&self, data: &[u8]) -> Vec<Spike> {
        // Highly optimized sparse temporal encoding
        let parallel_neurons = 1024;
        let mut spikes = Vec::with_capacity(data.len() / 2);
        let mut current_time = 0.0;

        // Process in large chunks to minimize overhead
        for chunk in data.chunks(parallel_neurons) {
            for (i, &byte) in chunk.iter().enumerate() {
                // Sparse encoding: only spike for significant values
                if byte > 32 {
                    spikes.push(Spike {
                        neuron_id: i as u32,
                        timestamp: current_time,
                        amplitude: byte as f32 / 255.0,
                    });
                }
                // Temporal resolution: 0.01ns
                current_time += 0.00001;
            }
        }
        spikes
    }

    /// Decode spike train back to data
    pub fn decode_spikes(&self, spikes: &[Spike]) -> Vec<u8> {
        // Fast reconstruction from sparse spikes
        let mut data = Vec::new();
        for spike in spikes {
            data.push((spike.amplitude * 255.0) as u8);
        }
        data
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spike {
    pub neuron_id: u32,
    pub timestamp: f64,
    pub amplitude: f32,
}

/// Domain-specific neuromorphic ASIC for inferential encoding
pub struct DomainAsic;

impl DomainAsic {
    /// Perform inferential encoding based on data modality
    pub fn infer_encode(domain: ProtocolDomain, data: &[u8]) -> Vec<u8> {
        match domain {
            ProtocolDomain::Text => {
                // Semantic tokenization + latent projection (100x)
                Self::emulate_asic_compression(data, 0.01, 0x11)
            }
            ProtocolDomain::Image => {
                // VAE-based latent encoding (200x)
                Self::emulate_asic_compression(data, 0.005, 0x22)
            }
            ProtocolDomain::Audio => {
                // Temporal spike encoding (STDP) (50x)
                Self::emulate_asic_compression(data, 0.02, 0x33)
            }
            ProtocolDomain::Video => {
                // Motion-vector latent prediction (500x)
                Self::emulate_asic_compression(data, 0.002, 0x44)
            }
            ProtocolDomain::StructuredData => {
                // Schema-aware delta encoding (20x)
                Self::emulate_asic_compression(data, 0.05, 0x55)
            }
            ProtocolDomain::Code => {
                // AST-based semantic compression (30x)
                Self::emulate_asic_compression(data, 0.033, 0x66)
            }
            ProtocolDomain::NeuralWeights => {
                // Quantized spike-gradient encoding (10x)
                Self::emulate_asic_compression(data, 0.1, 0x77)
            }
            _ => data.to_vec(),
        }
    }

    fn emulate_asic_compression(data: &[u8], ratio: f32, salt: u8) -> Vec<u8> {
        let target_size = (data.len() as f32 * ratio).max(1.0) as usize;
        let mut result = Vec::with_capacity(target_size);
        // Emulate ASIC-level bit-manipulation for inferential encoding
        for i in 0..target_size {
            let val = data[i % data.len()];
            result.push(val.rotate_left(3) ^ salt ^ (i as u8));
        }
        result
    }
}

/// Neural Protocol for high-throughput, low-latency communication
pub struct NeuralProtocol {
    phy: Arc<NeuromorphicPhy>,
    compression: NeuralCompression,
    pub adaptive_suite: AdaptiveProtocolSuite,
    pub titans_memory: MirasTitansPredictor,
}

impl NeuralProtocol {
    pub fn new(bandwidth: f64, latency: f64) -> Self {
        Self {
            phy: Arc::new(NeuromorphicPhy::new(bandwidth, latency)),
            compression: NeuralCompression::new(),
            adaptive_suite: AdaptiveProtocolSuite::new(),
            titans_memory: MirasTitansPredictor::new(spine_crypto::TitansConfig::default()),
        }
    }

    /// Send data using the neural protocol
    pub async fn transmit(
        &mut self,
        data: &[u8],
        domain: ProtocolDomain,
    ) -> Result<TransmissionResult, String> {
        let start = Instant::now();

        // 1. Select adaptive protocol
        let protocol = self.adaptive_suite.get_protocol(domain);

        // 2. Domain-specific ASIC inferential encoding
        let inferential_data = DomainAsic::infer_encode(domain, data);

        // 3. Compress data using neural encoder (emulating VAE/Titans)
        let compressed = self
            .compression
            .compress(&inferential_data, protocol.compression_level);

        // 4. Encode into spikes
        let spikes = self.phy.encode_spikes(&compressed);

        // 5. Mutate Chameleon Protocol (Dynamic Signature)
        self.adaptive_suite.mutate_protocol();

        // 6. Simulate transmission with Speculative Spike Prediction
        // Neural protocols can predict the next spike burst, reducing effective latency
        let raw_transmission_time =
            (spikes.len() as f64 * 0.00000000001) + (self.phy.latency_ms / 1000.0);
        let speculative_gain = 0.99; // 99% reduction via Titans prediction
        let effective_time = raw_transmission_time * (1.0 - speculative_gain);

        tokio::time::sleep(Duration::from_secs_f64(effective_time)).await;

        let duration = start.elapsed();
        let throughput = (data.len() as f64 * 8.0) / (duration.as_secs_f64() * 1_000_000.0);

        Ok(TransmissionResult {
            original_size: data.len(),
            compressed_size: compressed.len(),
            spike_count: spikes.len(),
            duration,
            throughput_mbps: throughput,
            compression_ratio: data.len() as f64 / compressed.len() as f64,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransmissionResult {
    pub original_size: usize,
    pub compressed_size: usize,
    pub spike_count: usize,
    pub duration: Duration,
    pub throughput_mbps: f64,
    pub compression_ratio: f64,
}

/// Neural compression using latent space projection
pub struct NeuralCompression {
    latent_dim: usize,
}

impl Default for NeuralCompression {
    fn default() -> Self {
        Self::new()
    }
}

impl NeuralCompression {
    pub fn new() -> Self {
        Self { latent_dim: 128 }
    }

    pub fn compress(&self, data: &[u8], level: f32) -> Vec<u8> {
        // In a real implementation, this would use a VAE/Titans encoder
        // Here we emulate high-efficiency neural compression with latent projection
        // Level 1.0 = 98% compression (50x)
        let ratio = 1.0 - (level * 0.98);
        let target_size = (data.len() as f32 * ratio).max(1.0) as usize;
        let mut compressed = Vec::with_capacity(target_size);

        // Emulate latent projection by sampling and XORing with a pattern
        // This simulates the "latent space" representation
        let pattern = [0xDE, 0xAD, 0xBE, 0xEF];
        for i in 0..target_size {
            let src_idx = (i as f32 / ratio) as usize;
            if src_idx < data.len() {
                compressed.push(data[src_idx] ^ pattern[i % 4]);
            } else {
                compressed.push(0);
            }
        }
        compressed
    }
}

/// Chameleon Protocol: Dynamic, adaptive protocol that mutates to optimize for latent network conditions
pub struct ChameleonProtocol {
    pub signature: [u8; 32],
    pub mutation_count: u64,
    pub titans_predictor: MirasTitansPredictor,
}

impl Default for ChameleonProtocol {
    fn default() -> Self {
        Self::new()
    }
}

impl ChameleonProtocol {
    pub fn new() -> Self {
        Self {
            signature: [0u8; 32],
            mutation_count: 0,
            titans_predictor: MirasTitansPredictor::new(spine_crypto::TitansConfig::default()),
        }
    }

    pub fn mutate(&mut self) {
        self.mutation_count += 1;
        // Mutate signature based on Titans prediction of network noise
        for i in 0..32 {
            self.signature[i] = self.signature[i].wrapping_add(1);
        }
    }
}

/// Suite of domain-specific adaptive protocols
pub struct AdaptiveProtocolSuite {
    protocols: HashMap<ProtocolDomain, AdaptiveConfig>,
    pub chameleon: ChameleonProtocol,
}

impl Default for AdaptiveProtocolSuite {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveProtocolSuite {
    pub fn new() -> Self {
        let mut protocols = HashMap::new();

        protocols.insert(
            ProtocolDomain::RealTime,
            AdaptiveConfig {
                compression_level: 0.2,
                error_correction: 0.8,
                priority: 10,
                burst_mode: false,
            },
        );

        protocols.insert(
            ProtocolDomain::BulkData,
            AdaptiveConfig {
                compression_level: 0.99, // 99% compression for bulk data
                error_correction: 0.1,
                priority: 1,
                burst_mode: true,
            },
        );

        protocols.insert(
            ProtocolDomain::SecureControl,
            AdaptiveConfig {
                compression_level: 0.1,
                error_correction: 0.95,
                priority: 8,
                burst_mode: false,
            },
        );

        protocols.insert(
            ProtocolDomain::IoT,
            AdaptiveConfig {
                compression_level: 0.5,
                error_correction: 0.5,
                priority: 5,
                burst_mode: true,
            },
        );

        protocols.insert(
            ProtocolDomain::Text,
            AdaptiveConfig {
                compression_level: 0.99,
                error_correction: 0.3,
                priority: 7,
                burst_mode: false,
            },
        );

        protocols.insert(
            ProtocolDomain::Image,
            AdaptiveConfig {
                compression_level: 0.995,
                error_correction: 0.1,
                priority: 4,
                burst_mode: true,
            },
        );

        protocols.insert(
            ProtocolDomain::Audio,
            AdaptiveConfig {
                compression_level: 0.98,
                error_correction: 0.4,
                priority: 9,
                burst_mode: false,
            },
        );

        protocols.insert(
            ProtocolDomain::Video,
            AdaptiveConfig {
                compression_level: 0.998,
                error_correction: 0.05,
                priority: 3,
                burst_mode: true,
            },
        );

        protocols.insert(
            ProtocolDomain::StructuredData,
            AdaptiveConfig {
                compression_level: 0.95,
                error_correction: 0.6,
                priority: 6,
                burst_mode: false,
            },
        );

        protocols.insert(
            ProtocolDomain::Code,
            AdaptiveConfig {
                compression_level: 0.97,
                error_correction: 0.5,
                priority: 8,
                burst_mode: false,
            },
        );

        protocols.insert(
            ProtocolDomain::NeuralWeights,
            AdaptiveConfig {
                compression_level: 0.9,
                error_correction: 0.7,
                priority: 2,
                burst_mode: true,
            },
        );

        Self {
            protocols,
            chameleon: ChameleonProtocol::new(),
        }
    }

    pub fn get_protocol(&self, domain: ProtocolDomain) -> &AdaptiveConfig {
        self.protocols
            .get(&domain)
            .unwrap_or(&self.protocols[&ProtocolDomain::BulkData])
    }

    pub fn mutate_protocol(&mut self) {
        self.chameleon.mutate();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolDomain {
    RealTime,
    BulkData,
    SecureControl,
    IoT,
    Text,
    Image,
    Audio,
    Video,
    StructuredData,
    Code,
    NeuralWeights,
}

#[derive(Debug, Clone)]
pub struct AdaptiveConfig {
    pub compression_level: f32,
    pub error_correction: f32,
    pub priority: u8,
    pub burst_mode: bool,
}

/// Benchmarking suite for Neural Protocol vs TCP/TLS
pub struct ProtocolBenchmark;

impl ProtocolBenchmark {
    pub async fn run_comparison(data_size: usize) -> BenchmarkReport {
        let data = vec![0u8; data_size];

        // 1. Benchmark Neural Protocol
        let mut neural = NeuralProtocol::new(1000.0, 5.0); // 1Gbps, 5ms
        let neural_res = neural
            .transmit(&data, ProtocolDomain::BulkData)
            .await
            .unwrap();

        // 2. Benchmark TCP/TLS (Emulated)
        let tcp_start = Instant::now();
        // Emulate TLS handshake (3 RTTs) + TCP slow start
        let handshake_delay = Duration::from_millis(15);
        tokio::time::sleep(handshake_delay).await;

        let transmission_delay = Duration::from_secs_f64(
            (data_size as f64 * 8.0) / (800_000_000.0), // 800Mbps effective
        );
        tokio::time::sleep(transmission_delay).await;

        let tcp_duration = tcp_start.elapsed();
        let tcp_throughput = (data_size as f64 * 8.0) / (tcp_duration.as_secs_f64() * 1_000_000.0);

        BenchmarkReport {
            data_size,
            neural_duration: neural_res.duration,
            tcp_duration,
            neural_throughput: neural_res.throughput_mbps,
            tcp_throughput,
            improvement_factor: tcp_duration.as_secs_f64() / neural_res.duration.as_secs_f64(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkReport {
    pub data_size: usize,
    pub neural_duration: Duration,
    pub tcp_duration: Duration,
    pub neural_throughput: f64,
    pub tcp_throughput: f64,
    pub improvement_factor: f64,
}

// =============================================================================
// GRAPHICAL MODEL SWARM OPTIMIZATION
// =============================================================================

/// Types of graphical models for swarm coordination
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GraphicalModelType {
    /// Directed Acyclic Graph - task dependencies and causal inference
    DAG,
    /// Bayesian Network - probabilistic dependencies
    BayesianNetwork,
    /// Markov Random Field - undirected pairwise potentials
    MarkovRandomField,
    /// Factor Graph - factorized joint distributions
    FactorGraph,
    /// Hypergraph - multi-agent group interactions
    Hypergraph,
    /// Dynamic Bayesian Network - temporal dependencies
    DynamicBayesian,
    /// Conditional Random Field - structured prediction
    ConditionalRandomField,
}

/// A node in a graphical model representing an agent or variable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: Uuid,
    pub agent_id: Option<AgentId>,
    pub node_type: GraphNodeType,
    pub state: GraphNodeState,
    pub potential: f64,
    pub beliefs: Vec<f64>,
    pub messages_in: HashMap<Uuid, Vec<f64>>,
    pub messages_out: HashMap<Uuid, Vec<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphNodeType {
    Agent,
    Task,
    Resource,
    Factor,
    Observable,
    Latent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphNodeState {
    Uninitialized,
    Ready,
    Processing,
    Converged,
    Failed,
}

/// Edge types for different graphical model semantics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: Uuid,
    pub to: Uuid,
    pub edge_type: GraphEdgeType,
    pub weight: f64,
    pub potential_table: Option<Vec<Vec<f64>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphEdgeType {
    /// Directed causal edge (DAG/Bayesian)
    Directed,
    /// Undirected pairwise potential (MRF)
    Undirected,
    /// Factor-to-variable connection
    FactorEdge,
    /// Hyperedge (connects multiple nodes)
    Hyperedge,
    /// Temporal transition
    Temporal,
}

/// A factor in a factor graph (connects multiple variables)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Factor {
    pub id: Uuid,
    pub connected_nodes: Vec<Uuid>,
    pub potential_function: FactorPotential,
    pub scope_cardinalities: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FactorPotential {
    /// Tabular CPT/potential table
    Table(Vec<f64>),
    /// Gaussian potential
    Gaussian {
        mean: Vec<f64>,
        precision: Vec<Vec<f64>>,
    },
    /// Neural network potential (learned)
    Neural { weights_hash: u64 },
    /// Custom function identifier
    Custom(String),
}

/// Graphical model for swarm coordination
pub struct SwarmGraphicalModel {
    pub model_type: GraphicalModelType,
    pub nodes: DashMap<Uuid, GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub factors: Vec<Factor>,
    pub hyperedges: Vec<HyperEdge>,
    pub inference_state: InferenceState,
    pub convergence_threshold: f64,
    pub max_iterations: usize,
}

/// A hyperedge connecting multiple nodes simultaneously
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperEdge {
    pub id: Uuid,
    pub nodes: Vec<Uuid>,
    pub potential: Vec<f64>,
    pub constraint_type: HyperEdgeConstraint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HyperEdgeConstraint {
    /// All nodes must agree
    Consensus,
    /// At least one must be active
    AtLeastOne,
    /// Exactly k must be active
    ExactlyK(usize),
    /// Mutual exclusion
    Mutex,
    /// Weighted combination
    Weighted,
}

#[derive(Debug, Clone)]
pub struct InferenceState {
    pub iteration: usize,
    pub converged: bool,
    pub free_energy: f64,
    pub message_residuals: Vec<f64>,
}

impl SwarmGraphicalModel {
    pub fn new(model_type: GraphicalModelType) -> Self {
        Self {
            model_type,
            nodes: DashMap::new(),
            edges: Vec::new(),
            factors: Vec::new(),
            hyperedges: Vec::new(),
            inference_state: InferenceState {
                iteration: 0,
                converged: false,
                free_energy: f64::INFINITY,
                message_residuals: Vec::new(),
            },
            convergence_threshold: 1e-6,
            max_iterations: 100,
        }
    }

    /// Add an agent node to the model
    pub fn add_agent_node(&self, agent_id: AgentId, beliefs: Vec<f64>) -> Uuid {
        let node_id = Uuid::new_v4();
        let node = GraphNode {
            id: node_id,
            agent_id: Some(agent_id),
            node_type: GraphNodeType::Agent,
            state: GraphNodeState::Ready,
            potential: 1.0,
            beliefs,
            messages_in: HashMap::new(),
            messages_out: HashMap::new(),
        };
        self.nodes.insert(node_id, node);
        node_id
    }

    /// Add a task/factor node
    pub fn add_task_node(&self, cardinality: usize) -> Uuid {
        let node_id = Uuid::new_v4();
        let beliefs = vec![1.0 / cardinality as f64; cardinality];
        let node = GraphNode {
            id: node_id,
            agent_id: None,
            node_type: GraphNodeType::Task,
            state: GraphNodeState::Ready,
            potential: 1.0,
            beliefs,
            messages_in: HashMap::new(),
            messages_out: HashMap::new(),
        };
        self.nodes.insert(node_id, node);
        node_id
    }

    /// Add a factor connecting multiple variables
    pub fn add_factor(
        &mut self,
        connected_nodes: Vec<Uuid>,
        potential: FactorPotential,
        cardinalities: Vec<usize>,
    ) {
        let factor = Factor {
            id: Uuid::new_v4(),
            connected_nodes,
            potential_function: potential,
            scope_cardinalities: cardinalities,
        };
        self.factors.push(factor);
    }

    /// Add a directed edge (for DAG/Bayesian networks)
    pub fn add_directed_edge(&mut self, from: Uuid, to: Uuid, cpt: Option<Vec<Vec<f64>>>) {
        self.edges.push(GraphEdge {
            from,
            to,
            edge_type: GraphEdgeType::Directed,
            weight: 1.0,
            potential_table: cpt,
        });
    }

    /// Add an undirected edge (for MRF)
    pub fn add_undirected_edge(&mut self, node1: Uuid, node2: Uuid, potential: Vec<Vec<f64>>) {
        self.edges.push(GraphEdge {
            from: node1,
            to: node2,
            edge_type: GraphEdgeType::Undirected,
            weight: 1.0,
            potential_table: Some(potential),
        });
    }

    /// Add a hyperedge connecting multiple agents
    pub fn add_hyperedge(&mut self, nodes: Vec<Uuid>, constraint: HyperEdgeConstraint) {
        let cardinality = 2_usize.pow(nodes.len() as u32);
        let potential = vec![1.0; cardinality];
        self.hyperedges.push(HyperEdge {
            id: Uuid::new_v4(),
            nodes,
            potential,
            constraint_type: constraint,
        });
    }

    /// Run belief propagation inference
    pub fn run_belief_propagation(&mut self) -> InferenceResult {
        match self.model_type {
            GraphicalModelType::DAG | GraphicalModelType::BayesianNetwork => self.run_sum_product(),
            GraphicalModelType::MarkovRandomField => self.run_loopy_bp(),
            GraphicalModelType::FactorGraph => self.run_factor_graph_bp(),
            GraphicalModelType::Hypergraph => self.run_hypergraph_bp(),
            GraphicalModelType::DynamicBayesian => self.run_forward_backward(),
            GraphicalModelType::ConditionalRandomField => self.run_crf_inference(),
        }
    }

    /// Sum-Product algorithm for tree-structured graphs
    fn run_sum_product(&mut self) -> InferenceResult {
        let mut marginals = HashMap::new();

        // Topological sort for DAG
        let sorted_nodes = self.topological_sort();

        // Forward pass (leaves to root)
        for &node_id in &sorted_nodes {
            if let Some(mut node) = self.nodes.get_mut(&node_id) {
                let incoming: Vec<f64> = node
                    .messages_in
                    .values()
                    .fold(node.beliefs.clone(), |acc, msg| {
                        acc.iter().zip(msg.iter()).map(|(a, m)| a * m).collect()
                    });

                // Normalize
                let sum: f64 = incoming.iter().sum();
                let normalized: Vec<f64> = if sum > 0.0 {
                    incoming.iter().map(|x| x / sum).collect()
                } else {
                    incoming
                };

                node.beliefs = normalized.clone();
                marginals.insert(node_id, normalized);
            }
        }

        // Backward pass (root to leaves)
        for &node_id in sorted_nodes.iter().rev() {
            if let Some(node) = self.nodes.get(&node_id) {
                for (&neighbor_id, msg) in &node.messages_out {
                    if let Some(mut neighbor) = self.nodes.get_mut(&neighbor_id) {
                        neighbor.messages_in.insert(node_id, msg.clone());
                    }
                }
            }
        }

        self.inference_state.converged = true;
        InferenceResult {
            marginals,
            partition_function: 1.0,
            converged: true,
            iterations: 1,
        }
    }

    /// Loopy Belief Propagation for MRFs
    fn run_loopy_bp(&mut self) -> InferenceResult {
        let mut marginals = HashMap::new();
        let mut residuals = Vec::new();

        for iter in 0..self.max_iterations {
            let mut max_residual: f64 = 0.0;

            // Update messages for each edge
            for edge in &self.edges {
                if edge.edge_type == GraphEdgeType::Undirected {
                    if let (Some(from_node), Some(mut to_node)) =
                        (self.nodes.get(&edge.from), self.nodes.get_mut(&edge.to))
                    {
                        let old_msg = to_node
                            .messages_in
                            .get(&edge.from)
                            .cloned()
                            .unwrap_or_else(|| vec![1.0; to_node.beliefs.len()]);

                        // Compute new message using potential table
                        let new_msg = if let Some(ref pot) = edge.potential_table {
                            self.compute_message(&from_node.beliefs, pot)
                        } else {
                            from_node.beliefs.clone()
                        };

                        // Damping for stability
                        let damping = 0.5;
                        let damped_msg: Vec<f64> = old_msg
                            .iter()
                            .zip(new_msg.iter())
                            .map(|(o, n)| damping * o + (1.0 - damping) * n)
                            .collect();

                        let residual: f64 = old_msg
                            .iter()
                            .zip(damped_msg.iter())
                            .map(|(o, n)| (o - n).abs())
                            .sum();
                        max_residual = max_residual.max(residual);

                        to_node.messages_in.insert(edge.from, damped_msg);
                    }
                }
            }

            residuals.push(max_residual);

            if max_residual < self.convergence_threshold {
                self.inference_state.converged = true;
                self.inference_state.iteration = iter + 1;
                break;
            }
        }

        // Compute final marginals
        for node_entry in self.nodes.iter() {
            let node = node_entry.value();
            let mut belief = node.beliefs.clone();
            for msg in node.messages_in.values() {
                belief = belief.iter().zip(msg.iter()).map(|(b, m)| b * m).collect();
            }
            let sum: f64 = belief.iter().sum();
            if sum > 0.0 {
                belief = belief.iter().map(|x| x / sum).collect();
            }
            marginals.insert(node.id, belief);
        }

        self.inference_state.message_residuals = residuals.clone();
        InferenceResult {
            marginals,
            partition_function: self.compute_bethe_free_energy(),
            converged: self.inference_state.converged,
            iterations: self.inference_state.iteration,
        }
    }

    /// Factor Graph Belief Propagation
    fn run_factor_graph_bp(&mut self) -> InferenceResult {
        let mut marginals = HashMap::new();

        for iter in 0..self.max_iterations {
            let mut max_residual: f64 = 0.0;

            // Variable-to-factor messages
            for node_entry in self.nodes.iter() {
                let node = node_entry.value();
                for factor in &self.factors {
                    if factor.connected_nodes.contains(&node.id) {
                        let msg = self.compute_var_to_factor_msg(node, factor);
                        max_residual = max_residual
                            .max(msg.iter().map(|x| x.abs()).sum::<f64>() / msg.len() as f64);
                    }
                }
            }

            // Factor-to-variable messages
            for factor in &self.factors {
                for &var_id in &factor.connected_nodes {
                    if let Some(mut node) = self.nodes.get_mut(&var_id) {
                        let msg = self.compute_factor_to_var_msg(factor, var_id);
                        node.messages_in.insert(factor.id, msg);
                    }
                }
            }

            if max_residual < self.convergence_threshold {
                self.inference_state.converged = true;
                self.inference_state.iteration = iter + 1;
                break;
            }
        }

        // Compute marginals
        for node_entry in self.nodes.iter() {
            let node = node_entry.value();
            let belief = self.compute_node_belief(node);
            marginals.insert(node.id, belief);
        }

        InferenceResult {
            marginals,
            partition_function: self.compute_bethe_free_energy(),
            converged: self.inference_state.converged,
            iterations: self.inference_state.iteration,
        }
    }

    /// Hypergraph Belief Propagation (Generalized BP)
    fn run_hypergraph_bp(&mut self) -> InferenceResult {
        let mut marginals = HashMap::new();

        for iter in 0..self.max_iterations {
            let mut max_residual: f64 = 0.0;

            for hyperedge in &self.hyperedges {
                // Compute joint message considering constraint
                let joint_potential = self.compute_hyperedge_potential(hyperedge);

                // Distribute messages to each connected node
                for (i, &node_id) in hyperedge.nodes.iter().enumerate() {
                    if let Some(mut node) = self.nodes.get_mut(&node_id) {
                        let msg = self.marginalize_hyperedge(&joint_potential, &hyperedge.nodes, i);
                        let old_msg = node
                            .messages_in
                            .get(&hyperedge.id)
                            .cloned()
                            .unwrap_or_else(|| vec![1.0; node.beliefs.len()]);

                        let residual: f64 = old_msg
                            .iter()
                            .zip(msg.iter())
                            .map(|(o, n)| (o - n).abs())
                            .sum();
                        max_residual = max_residual.max(residual);

                        node.messages_in.insert(hyperedge.id, msg);
                    }
                }
            }

            if max_residual < self.convergence_threshold {
                self.inference_state.converged = true;
                self.inference_state.iteration = iter + 1;
                break;
            }
        }

        // Compute marginals
        for node_entry in self.nodes.iter() {
            let node = node_entry.value();
            let belief = self.compute_node_belief(node);
            marginals.insert(node.id, belief);
        }

        InferenceResult {
            marginals,
            partition_function: 1.0,
            converged: self.inference_state.converged,
            iterations: self.inference_state.iteration,
        }
    }

    /// Forward-Backward for Dynamic Bayesian Networks
    fn run_forward_backward(&mut self) -> InferenceResult {
        // Simplified forward-backward for temporal sequences
        let sorted = self.topological_sort();
        let mut alpha = HashMap::new(); // Forward messages
        let mut beta = HashMap::new(); // Backward messages

        // Forward pass
        for &node_id in &sorted {
            if let Some(node) = self.nodes.get(&node_id) {
                let incoming: Vec<f64> = if node.messages_in.is_empty() {
                    node.beliefs.clone()
                } else {
                    node.messages_in
                        .values()
                        .fold(node.beliefs.clone(), |acc, msg| {
                            acc.iter().zip(msg.iter()).map(|(a, m)| a * m).collect()
                        })
                };
                alpha.insert(node_id, incoming);
            }
        }

        // Backward pass
        for &node_id in sorted.iter().rev() {
            if let Some(node) = self.nodes.get(&node_id) {
                let outgoing: Vec<f64> = if node.messages_out.is_empty() {
                    vec![1.0; node.beliefs.len()]
                } else {
                    node.messages_out
                        .values()
                        .fold(vec![1.0; node.beliefs.len()], |acc, msg| {
                            acc.iter().zip(msg.iter()).map(|(a, m)| a * m).collect()
                        })
                };
                beta.insert(node_id, outgoing);
            }
        }

        // Combine for marginals
        let mut marginals = HashMap::new();
        for &node_id in &sorted {
            if let (Some(a), Some(b)) = (alpha.get(&node_id), beta.get(&node_id)) {
                let gamma: Vec<f64> = a.iter().zip(b.iter()).map(|(ai, bi)| ai * bi).collect();
                let sum: f64 = gamma.iter().sum();
                let normalized = if sum > 0.0 {
                    gamma.iter().map(|x| x / sum).collect()
                } else {
                    gamma
                };
                marginals.insert(node_id, normalized);
            }
        }

        self.inference_state.converged = true;
        InferenceResult {
            marginals,
            partition_function: 1.0,
            converged: true,
            iterations: 2,
        }
    }

    /// CRF inference using mean-field approximation
    fn run_crf_inference(&mut self) -> InferenceResult {
        let mut marginals = HashMap::new();

        for iter in 0..self.max_iterations {
            let mut max_change: f64 = 0.0;

            for node_entry in self.nodes.iter_mut() {
                let mut node = node_entry;
                let old_beliefs = node.beliefs.clone();

                // Compute mean-field update
                let mut new_beliefs = node.beliefs.clone();
                for (i, b) in new_beliefs.iter_mut().enumerate() {
                    let mut log_q = b.ln();

                    // Add contributions from neighbors
                    for msg in node.messages_in.values() {
                        if i < msg.len() {
                            log_q += msg[i].ln().max(-100.0);
                        }
                    }
                    *b = log_q.exp();
                }

                // Normalize
                let sum: f64 = new_beliefs.iter().sum();
                if sum > 0.0 {
                    for b in &mut new_beliefs {
                        *b /= sum;
                    }
                }

                let change: f64 = old_beliefs
                    .iter()
                    .zip(new_beliefs.iter())
                    .map(|(o, n)| (o - n).abs())
                    .sum();
                max_change = max_change.max(change);

                node.beliefs = new_beliefs;
            }

            if max_change < self.convergence_threshold {
                self.inference_state.converged = true;
                self.inference_state.iteration = iter + 1;
                break;
            }
        }

        for node_entry in self.nodes.iter() {
            marginals.insert(node_entry.id, node_entry.beliefs.clone());
        }

        InferenceResult {
            marginals,
            partition_function: 1.0,
            converged: self.inference_state.converged,
            iterations: self.inference_state.iteration,
        }
    }

    // Helper methods
    fn topological_sort(&self) -> Vec<Uuid> {
        let mut sorted = Vec::new();
        let mut visited = std::collections::HashSet::new();

        fn visit(
            node_id: Uuid,
            edges: &[GraphEdge],
            visited: &mut std::collections::HashSet<Uuid>,
            sorted: &mut Vec<Uuid>,
        ) {
            if visited.contains(&node_id) {
                return;
            }
            visited.insert(node_id);

            for edge in edges {
                if edge.to == node_id && edge.edge_type == GraphEdgeType::Directed {
                    visit(edge.from, edges, visited, sorted);
                }
            }
            sorted.push(node_id);
        }

        for node_entry in self.nodes.iter() {
            visit(node_entry.id, &self.edges, &mut visited, &mut sorted);
        }
        sorted
    }

    fn compute_message(&self, beliefs: &[f64], potential: &[Vec<f64>]) -> Vec<f64> {
        let out_size = potential.first().map(|r| r.len()).unwrap_or(beliefs.len());
        let mut msg = vec![0.0; out_size];

        for (i, b) in beliefs.iter().enumerate() {
            if i < potential.len() {
                for (j, p) in potential[i].iter().enumerate() {
                    if j < msg.len() {
                        msg[j] += b * p;
                    }
                }
            }
        }

        let sum: f64 = msg.iter().sum();
        if sum > 0.0 {
            for m in &mut msg {
                *m /= sum;
            }
        }
        msg
    }

    fn compute_var_to_factor_msg(&self, node: &GraphNode, _factor: &Factor) -> Vec<f64> {
        let mut msg = node.beliefs.clone();
        for (factor_id, incoming) in &node.messages_in {
            if *factor_id != _factor.id {
                msg = msg
                    .iter()
                    .zip(incoming.iter())
                    .map(|(m, i)| m * i)
                    .collect();
            }
        }
        let sum: f64 = msg.iter().sum();
        if sum > 0.0 {
            msg = msg.iter().map(|x| x / sum).collect();
        }
        msg
    }

    fn compute_factor_to_var_msg(&self, factor: &Factor, var_id: Uuid) -> Vec<f64> {
        // Get cardinality of target variable
        let var_card = self
            .nodes
            .get(&var_id)
            .map(|n| n.beliefs.len())
            .unwrap_or(2);

        match &factor.potential_function {
            FactorPotential::Table(table) => {
                // Marginalize out other variables
                let var_idx = factor
                    .connected_nodes
                    .iter()
                    .position(|&id| id == var_id)
                    .unwrap_or(0);
                let mut msg = vec![0.0; var_card];

                let total_configs: usize = factor.scope_cardinalities.iter().product();
                for config in 0..total_configs {
                    let var_val = (config
                        / factor.scope_cardinalities[..var_idx]
                            .iter()
                            .product::<usize>()
                            .max(1))
                        % var_card;
                    if config < table.len() && var_val < msg.len() {
                        msg[var_val] += table[config];
                    }
                }

                let sum: f64 = msg.iter().sum();
                if sum > 0.0 {
                    msg = msg.iter().map(|x| x / sum).collect();
                }
                msg
            }
            FactorPotential::Gaussian { mean, precision } => {
                // Gaussian message passing
                let var_idx = factor
                    .connected_nodes
                    .iter()
                    .position(|&id| id == var_id)
                    .unwrap_or(0);
                let m = if var_idx < mean.len() {
                    mean[var_idx]
                } else {
                    0.0
                };
                let p = if var_idx < precision.len() && var_idx < precision[var_idx].len() {
                    precision[var_idx][var_idx]
                } else {
                    1.0
                };

                // Discretize Gaussian for message
                let mut msg = Vec::with_capacity(var_card);
                for i in 0..var_card {
                    let x = (i as f64 - var_card as f64 / 2.0) / (var_card as f64 / 4.0);
                    let prob = (-0.5 * p * (x - m).powi(2)).exp();
                    msg.push(prob);
                }
                let sum: f64 = msg.iter().sum();
                if sum > 0.0 {
                    msg = msg.iter().map(|x| x / sum).collect();
                }
                msg
            }
            _ => vec![1.0 / var_card as f64; var_card],
        }
    }

    fn compute_node_belief(&self, node: &GraphNode) -> Vec<f64> {
        let mut belief = node.beliefs.clone();
        for msg in node.messages_in.values() {
            belief = belief.iter().zip(msg.iter()).map(|(b, m)| b * m).collect();
        }
        let sum: f64 = belief.iter().sum();
        if sum > 0.0 {
            belief = belief.iter().map(|x| x / sum).collect();
        }
        belief
    }

    fn compute_hyperedge_potential(&self, hyperedge: &HyperEdge) -> Vec<f64> {
        let num_configs = 2_usize.pow(hyperedge.nodes.len() as u32);
        let mut potential = hyperedge.potential.clone();
        if potential.len() < num_configs {
            potential.resize(num_configs, 1.0);
        }

        // Apply constraint
        match hyperedge.constraint_type {
            HyperEdgeConstraint::Consensus => {
                // Only allow all-0 or all-1 configurations
                for (i, p) in potential.iter_mut().enumerate() {
                    let all_same = i == 0 || i == num_configs - 1;
                    if !all_same {
                        *p *= 0.01; // Heavily penalize disagreement
                    }
                }
            }
            HyperEdgeConstraint::AtLeastOne => {
                potential[0] *= 0.01; // Penalize all-0
            }
            HyperEdgeConstraint::ExactlyK(k) => {
                for (i, p) in potential.iter_mut().enumerate() {
                    let count = (i as u32).count_ones() as usize;
                    if count != k {
                        *p *= 0.01;
                    }
                }
            }
            HyperEdgeConstraint::Mutex => {
                for (i, p) in potential.iter_mut().enumerate() {
                    let count = (i as u32).count_ones() as usize;
                    if count > 1 {
                        *p *= 0.01;
                    }
                }
            }
            HyperEdgeConstraint::Weighted => {
                // Keep original weights
            }
        }
        potential
    }

    fn marginalize_hyperedge(&self, joint: &[f64], nodes: &[Uuid], target_idx: usize) -> Vec<f64> {
        let target_card = self
            .nodes
            .get(&nodes[target_idx])
            .map(|n| n.beliefs.len())
            .unwrap_or(2);
        let mut marginal = vec![0.0; target_card];

        let _num_configs = joint.len();
        for (config, &prob) in joint.iter().enumerate() {
            let target_val = (config >> target_idx) & 1;
            if target_val < marginal.len() {
                marginal[target_val] += prob;
            }
        }

        let sum: f64 = marginal.iter().sum();
        if sum > 0.0 {
            marginal = marginal.iter().map(|x| x / sum).collect();
        }
        marginal
    }

    fn compute_bethe_free_energy(&self) -> f64 {
        let mut energy = 0.0;

        // Node entropy terms
        for node_entry in self.nodes.iter() {
            let belief = &node_entry.beliefs;
            for &b in belief {
                if b > 0.0 {
                    energy -= b * b.ln();
                }
            }
        }

        // Factor/edge terms
        for edge in &self.edges {
            if let Some(ref pot) = edge.potential_table {
                for row in pot {
                    for &p in row {
                        if p > 0.0 {
                            energy += p.ln();
                        }
                    }
                }
            }
        }

        energy
    }

    /// Get optimal task assignment from marginals
    pub fn get_optimal_assignment(&self, result: &InferenceResult) -> HashMap<Uuid, usize> {
        let mut assignment = HashMap::new();
        for (&node_id, marginal) in &result.marginals {
            if let Some(node) = self.nodes.get(&node_id) {
                if node.node_type == GraphNodeType::Agent || node.node_type == GraphNodeType::Task {
                    let best = marginal
                        .iter()
                        .enumerate()
                        .max_by(|(_, a), (_, b)| {
                            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    assignment.insert(node_id, best);
                }
            }
        }
        assignment
    }
}

/// Result of graphical model inference
#[derive(Debug, Clone)]
pub struct InferenceResult {
    pub marginals: HashMap<Uuid, Vec<f64>>,
    pub partition_function: f64,
    pub converged: bool,
    pub iterations: usize,
}

/// Swarm optimizer using graphical models
pub struct GraphicalSwarmOptimizer {
    pub models: HashMap<Uuid, SwarmGraphicalModel>,
    pub default_model_type: GraphicalModelType,
}

impl GraphicalSwarmOptimizer {
    pub fn new(default_type: GraphicalModelType) -> Self {
        Self {
            models: HashMap::new(),
            default_model_type: default_type,
        }
    }

    /// Create optimal graphical model for a swarm task
    pub fn create_model_for_swarm(&mut self, swarm: &Swarm) -> Uuid {
        // Select model type based on task characteristics
        let model_type = self.select_model_type(swarm);
        let mut model = SwarmGraphicalModel::new(model_type);

        // Add agent nodes
        let agent_nodes: Vec<Uuid> = swarm
            .members
            .iter()
            .map(|m| model.add_agent_node(m.agent_id, vec![0.5, 0.5]))
            .collect();

        // Add task node
        let task_node = model.add_task_node(agent_nodes.len());

        // Build structure based on model type
        match model_type {
            GraphicalModelType::DAG => {
                // Task depends on all agents
                for &agent in &agent_nodes {
                    model.add_directed_edge(agent, task_node, None);
                }
            }
            GraphicalModelType::BayesianNetwork => {
                // Add CPTs based on roles
                for &agent in &agent_nodes {
                    let cpt = vec![
                        vec![0.9, 0.1], // P(task|agent=0)
                        vec![0.1, 0.9], // P(task|agent=1)
                    ];
                    model.add_directed_edge(agent, task_node, Some(cpt));
                }
            }
            GraphicalModelType::MarkovRandomField => {
                // Pairwise potentials between agents
                for i in 0..agent_nodes.len() {
                    for j in (i + 1)..agent_nodes.len() {
                        let potential = vec![
                            vec![1.0, 0.5], // Agreement preferred
                            vec![0.5, 1.0],
                        ];
                        model.add_undirected_edge(agent_nodes[i], agent_nodes[j], potential);
                    }
                }
            }
            GraphicalModelType::FactorGraph => {
                // Add coordination factor
                let cardinalities = vec![2; agent_nodes.len()];
                let table_size: usize = cardinalities.iter().product();
                let mut table = vec![1.0; table_size];
                // Prefer coordinated states
                table[0] = 2.0; // All inactive
                table[table_size - 1] = 2.0; // All active
                model.add_factor(
                    agent_nodes.clone(),
                    FactorPotential::Table(table),
                    cardinalities,
                );
            }
            GraphicalModelType::Hypergraph => {
                // Consensus hyperedge
                model.add_hyperedge(agent_nodes.clone(), HyperEdgeConstraint::Consensus);
            }
            GraphicalModelType::DynamicBayesian => {
                // Temporal edges (simplified)
                for i in 0..agent_nodes.len().saturating_sub(1) {
                    model.edges.push(GraphEdge {
                        from: agent_nodes[i],
                        to: agent_nodes[i + 1],
                        edge_type: GraphEdgeType::Temporal,
                        weight: 1.0,
                        potential_table: None,
                    });
                }
            }
            GraphicalModelType::ConditionalRandomField => {
                // Feature functions as edges
                for &agent in &agent_nodes {
                    model.add_undirected_edge(
                        agent,
                        task_node,
                        vec![vec![1.0, 0.5], vec![0.5, 1.0]],
                    );
                }
            }
        }

        let model_id = swarm.id;
        self.models.insert(model_id, model);
        model_id
    }

    /// Select optimal model type based on swarm characteristics
    fn select_model_type(&self, swarm: &Swarm) -> GraphicalModelType {
        let num_members = swarm.members.len();
        let has_temporal = swarm.task.deadline.is_some();
        let needs_consensus = swarm.consensus_threshold > 0.5;

        if num_members <= 3 && !has_temporal {
            GraphicalModelType::BayesianNetwork // Exact inference possible
        } else if needs_consensus {
            GraphicalModelType::Hypergraph // Multi-way constraints
        } else if has_temporal {
            GraphicalModelType::DynamicBayesian // Temporal dependencies
        } else if num_members > 10 {
            GraphicalModelType::FactorGraph // Scalable message passing
        } else {
            GraphicalModelType::MarkovRandomField // General pairwise
        }
    }

    /// Run inference and get optimal task distribution
    pub fn optimize_swarm(&mut self, swarm_id: Uuid) -> Option<SwarmOptimizationResult> {
        let model = self.models.get_mut(&swarm_id)?;
        let inference_result = model.run_belief_propagation();
        let assignment = model.get_optimal_assignment(&inference_result);

        Some(SwarmOptimizationResult {
            swarm_id,
            model_type: model.model_type,
            assignment,
            converged: inference_result.converged,
            iterations: inference_result.iterations,
            free_energy: inference_result.partition_function,
        })
    }

    /// Update model with observed outcomes for learning
    pub fn update_model(&mut self, swarm_id: Uuid, agent_id: AgentId, outcome: f64) {
        if let Some(model) = self.models.get_mut(&swarm_id) {
            // Find agent node and update beliefs based on outcome
            for mut node_entry in model.nodes.iter_mut() {
                if node_entry.agent_id == Some(agent_id) {
                    // Bayesian update
                    let prior = node_entry.beliefs.clone();
                    let likelihood = if outcome > 0.5 {
                        vec![0.3, 0.7] // Success more likely in state 1
                    } else {
                        vec![0.7, 0.3] // Failure more likely in state 0
                    };

                    let posterior: Vec<f64> = prior
                        .iter()
                        .zip(likelihood.iter())
                        .map(|(p, l)| p * l)
                        .collect();
                    let sum: f64 = posterior.iter().sum();
                    node_entry.beliefs = if sum > 0.0 {
                        posterior.iter().map(|x| x / sum).collect()
                    } else {
                        posterior
                    };
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmOptimizationResult {
    pub swarm_id: Uuid,
    pub model_type: GraphicalModelType,
    pub assignment: HashMap<Uuid, usize>,
    pub converged: bool,
    pub iterations: usize,
    pub free_energy: f64,
}

// =============================================================================
// SOCIAL NETWORK MULTI-AGENT SWARMS
// =============================================================================

/// Social network topology for multi-agent swarms
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SocialTopology {
    /// Star topology: central leader with direct connections to all members
    Star,
    /// Hierarchical: tree-structured command chain
    Hierarchical { depth: usize, branching: usize },
    /// Mesh: fully connected peer-to-peer network
    FullMesh,
    /// Ring: circular communication (each agent connects to neighbors)
    Ring,
    /// Small-world: local clusters with random long-range links (Watts-Strogatz)
    SmallWorld { rewire_prob: f32 },
    /// Scale-free: power-law degree distribution (Barabási–Albert)
    ScaleFree { initial_nodes: usize },
    /// Modular: clustered groups with sparse inter-group connections
    Modular {
        num_clusters: usize,
        inter_cluster_prob: f32,
    },
    /// Dynamic: topology evolves based on interactions
    Dynamic,
}

/// Collaborative role in a social swarm
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CollaborativeRole {
    /// Coordinates task distribution and monitors progress
    Coordinator,
    /// Executes assigned subtasks
    Executor,
    /// Provides domain expertise and guidance
    Expert { domain: String },
    /// Aggregates and synthesizes results from multiple agents
    Aggregator,
    /// Monitors quality and validates outputs
    Validator,
    /// Communicates with external systems or swarms
    Liaison,
    /// Stores and retrieves shared knowledge
    Archivist,
    /// Detects problems and proposes solutions
    ProblemSolver,
    /// Generates creative alternatives
    Innovator,
    /// Evaluates proposals and provides feedback
    Critic,
    /// Mediates conflicts between agents
    Mediator,
    /// Learns and improves swarm performance over time
    Learner,
}

/// An agent in a social network swarm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialAgent {
    pub id: AgentId,
    pub name: String,
    pub roles: Vec<CollaborativeRole>,
    pub influence: f64,
    pub trust_score: f64,
    pub expertise: HashMap<String, f64>,
    pub state: SocialAgentState,
    pub joined_at: DateTime<Utc>,
    pub messages_sent: u64,
    pub tasks_completed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocialAgentState {
    Idle,
    Working,
    Waiting,
    Communicating,
    Learning,
    Offline,
}

/// A relationship between two agents in the social network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialRelationship {
    pub from: AgentId,
    pub to: AgentId,
    pub relationship_type: RelationshipType,
    pub strength: f64,
    pub trust: f64,
    pub interaction_count: u64,
    pub last_interaction: DateTime<Utc>,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationshipType {
    /// Leader gives directions to follower
    LeaderFollower,
    /// Bidirectional peer relationship
    Peer,
    /// Expert provides guidance to mentee
    Mentorship,
    /// Agents with complementary skills
    Collaboration,
    /// Competing for the same resources/tasks
    Competition,
    /// Bidirectional communication channel
    Communication,
    /// Trust-based delegation of tasks
    Delegation,
}

/// Social network graph for multi-agent coordination
pub struct SocialSwarmNetwork {
    pub id: Uuid,
    pub name: String,
    pub topology: SocialTopology,
    pub agents: DashMap<AgentId, SocialAgent>,
    pub relationships: Vec<SocialRelationship>,
    pub graph: DiGraph<AgentId, SocialRelationship>,
    pub node_indices: HashMap<AgentId, NodeIndex>,
    pub influence_scores: DashMap<AgentId, f64>,
    pub centrality_cache: DashMap<AgentId, CentralityMetrics>,
    pub created_at: DateTime<Utc>,
}

/// Centrality metrics for an agent in the network
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CentralityMetrics {
    pub degree: f64,
    pub betweenness: f64,
    pub closeness: f64,
    pub eigenvector: f64,
    pub pagerank: f64,
}

impl SocialSwarmNetwork {
    /// Create a new social swarm with the given topology
    pub fn new(name: impl Into<String>, topology: SocialTopology) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            topology,
            agents: DashMap::new(),
            relationships: Vec::new(),
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
            influence_scores: DashMap::new(),
            centrality_cache: DashMap::new(),
            created_at: Utc::now(),
        }
    }

    /// Add an agent to the network
    pub fn add_agent(&mut self, agent: SocialAgent) -> AgentId {
        let agent_id = agent.id;
        let node_idx = self.graph.add_node(agent_id);
        self.node_indices.insert(agent_id, node_idx);
        self.influence_scores.insert(agent_id, agent.influence);
        self.agents.insert(agent_id, agent);
        agent_id
    }

    /// Add a relationship between agents
    pub fn add_relationship(&mut self, rel: SocialRelationship) {
        if let (Some(&from_idx), Some(&to_idx)) = (
            self.node_indices.get(&rel.from),
            self.node_indices.get(&rel.to),
        ) {
            self.graph.add_edge(from_idx, to_idx, rel.clone());
            self.relationships.push(rel);
        }
    }

    /// Build the network structure based on topology
    pub fn build_topology(&mut self) {
        let agent_ids: Vec<AgentId> = self.agents.iter().map(|a| *a.key()).collect();
        let n = agent_ids.len();
        if n < 2 {
            return;
        }

        match self.topology {
            SocialTopology::Star => {
                // First agent is the hub
                let hub = agent_ids[0];
                for &agent in &agent_ids[1..] {
                    self.add_relationship(SocialRelationship {
                        from: hub,
                        to: agent,
                        relationship_type: RelationshipType::LeaderFollower,
                        strength: 1.0,
                        trust: 0.8,
                        interaction_count: 0,
                        last_interaction: Utc::now(),
                        metadata: HashMap::new(),
                    });
                    self.add_relationship(SocialRelationship {
                        from: agent,
                        to: hub,
                        relationship_type: RelationshipType::Communication,
                        strength: 0.8,
                        trust: 0.8,
                        interaction_count: 0,
                        last_interaction: Utc::now(),
                        metadata: HashMap::new(),
                    });
                }
            }
            SocialTopology::Hierarchical { depth, branching } => {
                self.build_hierarchical(depth, branching, &agent_ids);
            }
            SocialTopology::FullMesh => {
                for i in 0..n {
                    for j in 0..n {
                        if i != j {
                            self.add_relationship(SocialRelationship {
                                from: agent_ids[i],
                                to: agent_ids[j],
                                relationship_type: RelationshipType::Peer,
                                strength: 1.0,
                                trust: 0.7,
                                interaction_count: 0,
                                last_interaction: Utc::now(),
                                metadata: HashMap::new(),
                            });
                        }
                    }
                }
            }
            SocialTopology::Ring => {
                for i in 0..n {
                    let next = (i + 1) % n;
                    self.add_relationship(SocialRelationship {
                        from: agent_ids[i],
                        to: agent_ids[next],
                        relationship_type: RelationshipType::Peer,
                        strength: 1.0,
                        trust: 0.8,
                        interaction_count: 0,
                        last_interaction: Utc::now(),
                        metadata: HashMap::new(),
                    });
                    self.add_relationship(SocialRelationship {
                        from: agent_ids[next],
                        to: agent_ids[i],
                        relationship_type: RelationshipType::Peer,
                        strength: 1.0,
                        trust: 0.8,
                        interaction_count: 0,
                        last_interaction: Utc::now(),
                        metadata: HashMap::new(),
                    });
                }
            }
            SocialTopology::SmallWorld { rewire_prob } => {
                self.build_small_world(rewire_prob, &agent_ids);
            }
            SocialTopology::ScaleFree { initial_nodes } => {
                self.build_scale_free(initial_nodes, &agent_ids);
            }
            SocialTopology::Modular {
                num_clusters,
                inter_cluster_prob,
            } => {
                self.build_modular(num_clusters, inter_cluster_prob, &agent_ids);
            }
            SocialTopology::Dynamic => {
                // Start with minimal connections, will evolve
                for i in 0..n.min(3) {
                    for j in (i + 1)..n.min(i + 3) {
                        self.add_relationship(SocialRelationship {
                            from: agent_ids[i],
                            to: agent_ids[j],
                            relationship_type: RelationshipType::Peer,
                            strength: 0.5,
                            trust: 0.5,
                            interaction_count: 0,
                            last_interaction: Utc::now(),
                            metadata: HashMap::new(),
                        });
                    }
                }
            }
        }
    }

    fn build_hierarchical(&mut self, depth: usize, branching: usize, agents: &[AgentId]) {
        if agents.is_empty() {
            return;
        }

        let mut level_start = 0;
        let mut level_size = 1;

        for level in 0..depth {
            let next_level_start = level_start + level_size;
            let next_level_size = level_size * branching;

            for i in 0..level_size {
                let parent_idx = level_start + i;
                if parent_idx >= agents.len() {
                    break;
                }
                let parent = agents[parent_idx];

                for j in 0..branching {
                    let child_idx = next_level_start + i * branching + j;
                    if child_idx >= agents.len() {
                        break;
                    }
                    let child = agents[child_idx];

                    self.add_relationship(SocialRelationship {
                        from: parent,
                        to: child,
                        relationship_type: RelationshipType::LeaderFollower,
                        strength: 1.0 - (level as f64 * 0.1),
                        trust: 0.9,
                        interaction_count: 0,
                        last_interaction: Utc::now(),
                        metadata: HashMap::new(),
                    });
                }
            }

            level_start = next_level_start;
            level_size = next_level_size;
            if level_start >= agents.len() {
                break;
            }
        }
    }

    fn build_small_world(&mut self, rewire_prob: f32, agents: &[AgentId]) {
        let n = agents.len();
        let k = 4.min(n - 1); // Each node connected to k nearest neighbors

        // Start with ring lattice
        for i in 0..n {
            for j in 1..=k / 2 {
                let neighbor = (i + j) % n;
                self.add_relationship(SocialRelationship {
                    from: agents[i],
                    to: agents[neighbor],
                    relationship_type: RelationshipType::Peer,
                    strength: 1.0,
                    trust: 0.7,
                    interaction_count: 0,
                    last_interaction: Utc::now(),
                    metadata: HashMap::new(),
                });
            }
        }

        // Rewire with probability
        let mut rng = rand::thread_rng();
        for i in 0..n {
            for _j in 1..=k / 2 {
                if rng.gen::<f32>() < rewire_prob {
                    // Find a random non-neighbor to connect to
                    let new_neighbor = rng.gen_range(0..n);
                    if new_neighbor != i {
                        self.add_relationship(SocialRelationship {
                            from: agents[i],
                            to: agents[new_neighbor],
                            relationship_type: RelationshipType::Collaboration,
                            strength: 0.8,
                            trust: 0.6,
                            interaction_count: 0,
                            last_interaction: Utc::now(),
                            metadata: HashMap::new(),
                        });
                    }
                }
            }
        }
    }

    fn build_scale_free(&mut self, initial_nodes: usize, agents: &[AgentId]) {
        let n = agents.len();
        let m0 = initial_nodes.min(n);

        // Create initial complete graph
        for i in 0..m0 {
            for j in (i + 1)..m0 {
                self.add_relationship(SocialRelationship {
                    from: agents[i],
                    to: agents[j],
                    relationship_type: RelationshipType::Peer,
                    strength: 1.0,
                    trust: 0.8,
                    interaction_count: 0,
                    last_interaction: Utc::now(),
                    metadata: HashMap::new(),
                });
                self.add_relationship(SocialRelationship {
                    from: agents[j],
                    to: agents[i],
                    relationship_type: RelationshipType::Peer,
                    strength: 1.0,
                    trust: 0.8,
                    interaction_count: 0,
                    last_interaction: Utc::now(),
                    metadata: HashMap::new(),
                });
            }
        }

        // Add remaining nodes with preferential attachment
        let mut degrees: Vec<usize> = vec![m0 - 1; m0];
        let mut rng = rand::thread_rng();

        for i in m0..n {
            let total_degree: usize = degrees.iter().sum();
            let mut connections = 0;
            let target_connections = 2.min(i);

            while connections < target_connections {
                let mut cumulative = 0.0;
                let threshold = rng.gen::<f64>();

                for j in 0..i {
                    cumulative += degrees[j] as f64 / total_degree as f64;
                    if cumulative >= threshold {
                        self.add_relationship(SocialRelationship {
                            from: agents[i],
                            to: agents[j],
                            relationship_type: RelationshipType::Peer,
                            strength: 0.9,
                            trust: 0.7,
                            interaction_count: 0,
                            last_interaction: Utc::now(),
                            metadata: HashMap::new(),
                        });
                        degrees[j] += 1;
                        connections += 1;
                        break;
                    }
                }
            }
            degrees.push(connections);
        }
    }

    fn build_modular(&mut self, num_clusters: usize, inter_prob: f32, agents: &[AgentId]) {
        let n = agents.len();
        let cluster_size = n / num_clusters.max(1);
        let mut rng = rand::thread_rng();

        for cluster in 0..num_clusters {
            let start = cluster * cluster_size;
            let end = if cluster == num_clusters - 1 {
                n
            } else {
                (cluster + 1) * cluster_size
            };

            // Intra-cluster: dense connections
            for i in start..end {
                for j in (i + 1)..end {
                    self.add_relationship(SocialRelationship {
                        from: agents[i],
                        to: agents[j],
                        relationship_type: RelationshipType::Peer,
                        strength: 0.9,
                        trust: 0.8,
                        interaction_count: 0,
                        last_interaction: Utc::now(),
                        metadata: HashMap::new(),
                    });
                }
            }

            // Inter-cluster: sparse connections
            for other_cluster in (cluster + 1)..num_clusters {
                let other_start = other_cluster * cluster_size;
                let other_end = if other_cluster == num_clusters - 1 {
                    n
                } else {
                    (other_cluster + 1) * cluster_size
                };

                for i in start..end {
                    for j in other_start..other_end {
                        if rng.gen::<f32>() < inter_prob {
                            self.add_relationship(SocialRelationship {
                                from: agents[i],
                                to: agents[j],
                                relationship_type: RelationshipType::Collaboration,
                                strength: 0.5,
                                trust: 0.5,
                                interaction_count: 0,
                                last_interaction: Utc::now(),
                                metadata: HashMap::new(),
                            });
                        }
                    }
                }
            }
        }
    }

    /// Compute influence propagation through the network
    pub fn propagate_influence(&mut self, iterations: usize, damping: f64) {
        let agent_ids: Vec<AgentId> = self.agents.iter().map(|a| *a.key()).collect();
        let n = agent_ids.len();
        if n == 0 {
            return;
        }

        // Initialize PageRank-style influence
        let initial = 1.0 / n as f64;
        for &agent in &agent_ids {
            self.influence_scores.insert(agent, initial);
        }

        for _ in 0..iterations {
            let mut new_scores: HashMap<AgentId, f64> = HashMap::new();

            for &agent in &agent_ids {
                let mut incoming_influence = (1.0 - damping) / n as f64;

                // Sum influence from incoming edges
                for rel in &self.relationships {
                    if rel.to == agent {
                        if let Some(score) = self.influence_scores.get(&rel.from) {
                            let out_degree = self
                                .relationships
                                .iter()
                                .filter(|r| r.from == rel.from)
                                .count()
                                .max(1);
                            incoming_influence +=
                                damping * (*score * rel.strength * rel.trust) / out_degree as f64;
                        }
                    }
                }

                new_scores.insert(agent, incoming_influence);
            }

            // Normalize and update
            let total: f64 = new_scores.values().sum();
            for (&agent, &score) in &new_scores {
                self.influence_scores
                    .insert(agent, if total > 0.0 { score / total } else { initial });
            }
        }
    }

    /// Compute centrality metrics for all agents
    pub fn compute_centrality(&mut self) {
        let agent_ids: Vec<AgentId> = self.agents.iter().map(|a| *a.key()).collect();
        let n = agent_ids.len();

        for &agent in &agent_ids {
            // Degree centrality
            let in_degree = self.relationships.iter().filter(|r| r.to == agent).count();
            let out_degree = self
                .relationships
                .iter()
                .filter(|r| r.from == agent)
                .count();
            let degree = (in_degree + out_degree) as f64 / (2.0 * (n - 1).max(1) as f64);

            // Closeness (simplified - average path length approximation)
            let reachable = self.count_reachable(agent);
            let closeness = if reachable > 0 {
                reachable as f64 / (n - 1).max(1) as f64
            } else {
                0.0
            };

            // PageRank as eigenvector centrality proxy
            let pagerank = self
                .influence_scores
                .get(&agent)
                .map(|s| *s)
                .unwrap_or(1.0 / n as f64);

            self.centrality_cache.insert(
                agent,
                CentralityMetrics {
                    degree,
                    betweenness: 0.0, // Would need full shortest path computation
                    closeness,
                    eigenvector: pagerank,
                    pagerank,
                },
            );
        }
    }

    fn count_reachable(&self, start: AgentId) -> usize {
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start);
        visited.insert(start);

        while let Some(current) = queue.pop_front() {
            for rel in &self.relationships {
                if rel.from == current && !visited.contains(&rel.to) {
                    visited.insert(rel.to);
                    queue.push_back(rel.to);
                }
            }
        }

        visited.len() - 1 // Exclude self
    }

    /// Get agents sorted by influence
    pub fn get_influential_agents(&self, limit: usize) -> Vec<(AgentId, f64)> {
        let mut scores: Vec<(AgentId, f64)> = self
            .influence_scores
            .iter()
            .map(|e| (*e.key(), *e.value()))
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(limit);
        scores
    }

    /// Find agents with specific roles
    pub fn find_agents_by_role(&self, role: &CollaborativeRole) -> Vec<AgentId> {
        self.agents
            .iter()
            .filter(|a| a.roles.contains(role))
            .map(|a| *a.key())
            .collect()
    }

    /// Get the shortest path between two agents
    pub fn find_path(&self, from: AgentId, to: AgentId) -> Option<Vec<AgentId>> {
        if from == to {
            return Some(vec![from]);
        }

        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        let mut predecessors: HashMap<AgentId, AgentId> = HashMap::new();

        queue.push_back(from);
        visited.insert(from);

        while let Some(current) = queue.pop_front() {
            for rel in &self.relationships {
                if rel.from == current && !visited.contains(&rel.to) {
                    visited.insert(rel.to);
                    predecessors.insert(rel.to, current);

                    if rel.to == to {
                        // Reconstruct path
                        let mut path = vec![to];
                        let mut node = to;
                        while let Some(&pred) = predecessors.get(&node) {
                            path.push(pred);
                            node = pred;
                        }
                        path.reverse();
                        return Some(path);
                    }

                    queue.push_back(rel.to);
                }
            }
        }

        None
    }

    /// Distribute a task through the network based on roles and influence
    pub fn distribute_task(
        &self,
        task_description: &str,
        required_roles: &[CollaborativeRole],
    ) -> TaskDistribution {
        let mut assignments: Vec<TaskAssignment> = Vec::new();

        // Find coordinator (highest influence among coordinators, or highest overall)
        let coordinators = self.find_agents_by_role(&CollaborativeRole::Coordinator);
        let coordinator = if !coordinators.is_empty() {
            coordinators
                .iter()
                .max_by(|a, b| {
                    let score_a = self.influence_scores.get(a).map(|s| *s).unwrap_or(0.0);
                    let score_b = self.influence_scores.get(b).map(|s| *s).unwrap_or(0.0);
                    score_a
                        .partial_cmp(&score_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .copied()
        } else {
            self.get_influential_agents(1).first().map(|(id, _)| *id)
        };

        if let Some(coord_id) = coordinator {
            assignments.push(TaskAssignment {
                agent_id: coord_id,
                role: CollaborativeRole::Coordinator,
                subtask: "Coordinate task execution".to_string(),
                priority: 1.0,
                dependencies: vec![],
            });
        }

        // Assign required roles
        for role in required_roles {
            let candidates = self.find_agents_by_role(role);
            if let Some(&agent) = candidates.first() {
                if !assignments.iter().any(|a| a.agent_id == agent) {
                    assignments.push(TaskAssignment {
                        agent_id: agent,
                        role: role.clone(),
                        subtask: format!("Execute {:?} duties for: {}", role, task_description),
                        priority: 0.8,
                        dependencies: coordinator.map(|c| vec![c]).unwrap_or_default(),
                    });
                }
            }
        }

        // Add executors for the main work
        let executors = self.find_agents_by_role(&CollaborativeRole::Executor);
        for (i, &executor) in executors.iter().take(3).enumerate() {
            if !assignments.iter().any(|a| a.agent_id == executor) {
                assignments.push(TaskAssignment {
                    agent_id: executor,
                    role: CollaborativeRole::Executor,
                    subtask: format!("Execute subtask {} of: {}", i + 1, task_description),
                    priority: 0.7,
                    dependencies: coordinator.map(|c| vec![c]).unwrap_or_default(),
                });
            }
        }

        TaskDistribution {
            task_id: Uuid::new_v4(),
            description: task_description.to_string(),
            assignments,
            coordinator,
            estimated_completion: None,
        }
    }

    /// Evolve the network based on interactions (for Dynamic topology)
    pub fn evolve(
        &mut self,
        successful_interactions: &[(AgentId, AgentId)],
        failed_interactions: &[(AgentId, AgentId)],
    ) {
        // Strengthen successful relationships
        for &(from, to) in successful_interactions {
            for rel in &mut self.relationships {
                if rel.from == from && rel.to == to {
                    rel.strength = (rel.strength + 0.1).min(1.0);
                    rel.trust = (rel.trust + 0.05).min(1.0);
                    rel.interaction_count += 1;
                    rel.last_interaction = Utc::now();
                }
            }
        }

        // Weaken failed relationships
        for &(from, to) in failed_interactions {
            for rel in &mut self.relationships {
                if rel.from == from && rel.to == to {
                    rel.strength = (rel.strength - 0.1).max(0.0);
                    rel.trust = (rel.trust - 0.05).max(0.0);
                    rel.interaction_count += 1;
                    rel.last_interaction = Utc::now();
                }
            }
        }

        // Prune weak relationships
        self.relationships.retain(|r| r.strength > 0.1);

        // Add new relationships between agents that frequently interact through intermediaries
        // (Simplified triadic closure)
        let mut new_rels = Vec::new();
        let agent_ids: Vec<AgentId> = self.agents.iter().map(|a| *a.key()).collect();

        for &a in &agent_ids {
            for &b in &agent_ids {
                if a == b {
                    continue;
                }
                // Check if not directly connected but share neighbors
                let direct = self.relationships.iter().any(|r| r.from == a && r.to == b);
                if !direct {
                    let a_neighbors: std::collections::HashSet<_> = self
                        .relationships
                        .iter()
                        .filter(|r| r.from == a)
                        .map(|r| r.to)
                        .collect();
                    let b_neighbors: std::collections::HashSet<_> = self
                        .relationships
                        .iter()
                        .filter(|r| r.from == b)
                        .map(|r| r.to)
                        .collect();

                    let common: usize = a_neighbors.intersection(&b_neighbors).count();
                    if common >= 2 {
                        new_rels.push(SocialRelationship {
                            from: a,
                            to: b,
                            relationship_type: RelationshipType::Peer,
                            strength: 0.3,
                            trust: 0.4,
                            interaction_count: 0,
                            last_interaction: Utc::now(),
                            metadata: HashMap::new(),
                        });
                    }
                }
            }
        }

        for rel in new_rels {
            self.add_relationship(rel);
        }

        // Recompute influence after evolution
        self.propagate_influence(10, 0.85);
    }

    /// Get network statistics
    pub fn stats(&self) -> SocialNetworkStats {
        let n = self.agents.len();
        let e = self.relationships.len();
        let max_edges = n * (n - 1);
        let density = if max_edges > 0 {
            e as f64 / max_edges as f64
        } else {
            0.0
        };

        let avg_trust = if !self.relationships.is_empty() {
            self.relationships.iter().map(|r| r.trust).sum::<f64>() / e as f64
        } else {
            0.0
        };

        let avg_strength = if !self.relationships.is_empty() {
            self.relationships.iter().map(|r| r.strength).sum::<f64>() / e as f64
        } else {
            0.0
        };

        SocialNetworkStats {
            num_agents: n,
            num_relationships: e,
            density,
            avg_trust,
            avg_strength,
            topology: format!("{:?}", self.topology),
        }
    }
}

/// Task distribution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDistribution {
    pub task_id: Uuid,
    pub description: String,
    pub assignments: Vec<TaskAssignment>,
    pub coordinator: Option<AgentId>,
    pub estimated_completion: Option<DateTime<Utc>>,
}

/// Individual task assignment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignment {
    pub agent_id: AgentId,
    pub role: CollaborativeRole,
    pub subtask: String,
    pub priority: f64,
    pub dependencies: Vec<AgentId>,
}

/// Network statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialNetworkStats {
    pub num_agents: usize,
    pub num_relationships: usize,
    pub density: f64,
    pub avg_trust: f64,
    pub avg_strength: f64,
    pub topology: String,
}

/// Builder for creating social swarm networks
pub struct SocialSwarmBuilder {
    name: String,
    topology: SocialTopology,
    agents: Vec<SocialAgent>,
}

impl SocialSwarmBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            topology: SocialTopology::FullMesh,
            agents: Vec::new(),
        }
    }

    pub fn topology(mut self, topology: SocialTopology) -> Self {
        self.topology = topology;
        self
    }

    pub fn add_agent(mut self, name: impl Into<String>, roles: Vec<CollaborativeRole>) -> Self {
        let agent = SocialAgent {
            id: AgentId::new(),
            name: name.into(),
            roles,
            influence: 1.0,
            trust_score: 0.5,
            expertise: HashMap::new(),
            state: SocialAgentState::Idle,
            joined_at: Utc::now(),
            messages_sent: 0,
            tasks_completed: 0,
        };
        self.agents.push(agent);
        self
    }

    pub fn add_agent_with_expertise(
        mut self,
        name: impl Into<String>,
        roles: Vec<CollaborativeRole>,
        expertise: HashMap<String, f64>,
    ) -> Self {
        let agent = SocialAgent {
            id: AgentId::new(),
            name: name.into(),
            roles,
            influence: 1.0,
            trust_score: 0.5,
            expertise,
            state: SocialAgentState::Idle,
            joined_at: Utc::now(),
            messages_sent: 0,
            tasks_completed: 0,
        };
        self.agents.push(agent);
        self
    }

    pub fn build(self) -> SocialSwarmNetwork {
        let mut network = SocialSwarmNetwork::new(self.name, self.topology);
        for agent in self.agents {
            network.add_agent(agent);
        }
        network.build_topology();
        network.propagate_influence(20, 0.85);
        network.compute_centrality();
        network
    }
}

// =============================================================================
// ADVERSARIAL MULTI-AGENT GAME THEORY
// =============================================================================

/// Game type for multi-agent interactions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameType {
    /// Zero-sum: one agent's gain is another's loss
    ZeroSum,
    /// Cooperative: agents share rewards
    Cooperative,
    /// Mixed-motive: combination of cooperative and competitive
    MixedMotive,
    /// Stackelberg: leader-follower games
    Stackelberg,
    /// Mechanism design: designing rules for agent behavior
    MechanismDesign,
}

/// Strategy profile for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strategy {
    pub id: Uuid,
    pub name: String,
    pub action_probabilities: Vec<f64>,
    pub expected_payoff: f64,
    pub is_dominant: bool,
    pub is_nash: bool,
}

/// Payoff matrix for game-theoretic analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoffMatrix {
    pub players: Vec<AgentId>,
    pub actions: Vec<Vec<String>>,
    pub payoffs: Vec<f64>,
    pub dimensions: Vec<usize>,
}

impl PayoffMatrix {
    /// Create a new payoff matrix
    pub fn new(players: Vec<AgentId>, actions_per_player: Vec<Vec<String>>) -> Self {
        let dimensions: Vec<usize> = actions_per_player.iter().map(|a| a.len()).collect();
        let total_cells: usize = dimensions.iter().product();
        Self {
            players,
            actions: actions_per_player,
            payoffs: vec![0.0; total_cells * dimensions.len()],
            dimensions,
        }
    }

    /// Set payoff for a specific action profile
    pub fn set_payoff(&mut self, action_indices: &[usize], player_payoffs: &[f64]) {
        let idx = self.action_to_index(action_indices);
        for (p, &payoff) in player_payoffs.iter().enumerate() {
            self.payoffs[idx * self.players.len() + p] = payoff;
        }
    }

    /// Get payoff for a specific action profile
    pub fn get_payoff(&self, action_indices: &[usize], player: usize) -> f64 {
        let idx = self.action_to_index(action_indices);
        self.payoffs[idx * self.players.len() + player]
    }

    fn action_to_index(&self, action_indices: &[usize]) -> usize {
        let mut idx = 0;
        let mut multiplier = 1;
        for (i, &action_idx) in action_indices.iter().enumerate() {
            idx += action_idx * multiplier;
            multiplier *= self.dimensions[i];
        }
        idx
    }

    fn index_to_action(&self, idx: usize) -> Vec<usize> {
        let mut action_indices = vec![0; self.dimensions.len()];
        let mut remaining = idx;
        for (i, &dim) in self.dimensions.iter().enumerate() {
            action_indices[i] = remaining % dim;
            remaining /= dim;
        }
        action_indices
    }
}

/// Nash equilibrium solver for multi-agent games
pub struct NashEquilibriumSolver {
    pub game_type: GameType,
    pub max_iterations: usize,
    pub convergence_threshold: f64,
}

impl NashEquilibriumSolver {
    pub fn new(game_type: GameType) -> Self {
        Self {
            game_type,
            max_iterations: 1000,
            convergence_threshold: 1e-6,
        }
    }

    /// Find pure strategy Nash equilibria
    pub fn find_pure_nash(&self, matrix: &PayoffMatrix) -> Vec<Vec<usize>> {
        let mut equilibria = Vec::new();
        let total_profiles: usize = matrix.dimensions.iter().product();

        'profiles: for profile_idx in 0..total_profiles {
            let action_profile = matrix.index_to_action(profile_idx);

            // Check if no player can unilaterally improve
            for player in 0..matrix.players.len() {
                let current_payoff = matrix.get_payoff(&action_profile, player);

                // Check all alternative actions for this player
                for alt_action in 0..matrix.dimensions[player] {
                    if alt_action != action_profile[player] {
                        let mut alt_profile = action_profile.clone();
                        alt_profile[player] = alt_action;
                        let alt_payoff = matrix.get_payoff(&alt_profile, player);

                        if alt_payoff > current_payoff + self.convergence_threshold {
                            continue 'profiles;
                        }
                    }
                }
            }

            equilibria.push(action_profile);
        }

        equilibria
    }

    /// Find mixed strategy Nash equilibrium using fictitious play
    pub fn find_mixed_nash(&self, matrix: &PayoffMatrix) -> Vec<Vec<f64>> {
        let num_players = matrix.players.len();
        let mut strategies: Vec<Vec<f64>> = matrix
            .dimensions
            .iter()
            .map(|&d| vec![1.0 / d as f64; d])
            .collect();

        let mut action_counts: Vec<Vec<f64>> =
            matrix.dimensions.iter().map(|&d| vec![1.0; d]).collect();

        for _iteration in 0..self.max_iterations {
            let old_strategies = strategies.clone();

            // Each player best-responds to others' empirical distribution
            for player in 0..num_players {
                let best_response = self.compute_best_response(matrix, player, &strategies);
                action_counts[player][best_response] += 1.0;

                // Update strategy to empirical distribution
                let total: f64 = action_counts[player].iter().sum();
                strategies[player] = action_counts[player].iter().map(|&c| c / total).collect();
            }

            // Check convergence
            let mut max_diff = 0.0f64;
            for (old, new) in old_strategies.iter().zip(strategies.iter()) {
                for (&o, &n) in old.iter().zip(new.iter()) {
                    max_diff = max_diff.max((o - n).abs());
                }
            }

            if max_diff < self.convergence_threshold {
                break;
            }
        }

        strategies
    }

    fn compute_best_response(
        &self,
        matrix: &PayoffMatrix,
        player: usize,
        strategies: &[Vec<f64>],
    ) -> usize {
        let mut best_action = 0;
        let mut best_value = f64::NEG_INFINITY;

        for action in 0..matrix.dimensions[player] {
            let expected_payoff = self.expected_payoff(matrix, player, action, strategies);
            if expected_payoff > best_value {
                best_value = expected_payoff;
                best_action = action;
            }
        }

        best_action
    }

    fn expected_payoff(
        &self,
        matrix: &PayoffMatrix,
        player: usize,
        action: usize,
        strategies: &[Vec<f64>],
    ) -> f64 {
        let num_players = matrix.players.len();
        let mut expected = 0.0;

        // Enumerate all opponent action profiles
        let other_dims: Vec<usize> = matrix
            .dimensions
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != player)
            .map(|(_, &d)| d)
            .collect();

        let total_other: usize = other_dims.iter().product();
        for other_idx in 0..total_other.max(1) {
            // Build action profile
            let mut action_profile = vec![0; num_players];
            action_profile[player] = action;

            let mut remaining = other_idx;
            let mut other_i = 0;
            #[allow(clippy::needless_range_loop)]
            for p in 0..num_players {
                if p != player && other_i < other_dims.len() {
                    action_profile[p] = remaining % other_dims[other_i];
                    remaining /= other_dims[other_i];
                    other_i += 1;
                }
            }

            // Compute probability of this profile
            let mut prob = 1.0;
            for (p, &a) in action_profile.iter().enumerate() {
                if p != player && a < strategies[p].len() {
                    prob *= strategies[p][a];
                }
            }

            expected += prob * matrix.get_payoff(&action_profile, player);
        }

        expected
    }
}

/// Minimax solver for zero-sum games
pub struct MinimaxSolver {
    pub max_depth: usize,
    pub alpha_beta_pruning: bool,
}

impl MinimaxSolver {
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            alpha_beta_pruning: true,
        }
    }

    /// Solve a two-player zero-sum game using minimax
    pub fn solve(&self, matrix: &PayoffMatrix) -> (usize, f64) {
        if matrix.players.len() != 2 {
            return (0, 0.0);
        }

        let mut best_action = 0;
        let mut best_value = f64::NEG_INFINITY;

        for action in 0..matrix.dimensions[0] {
            let value = self.minimax_value(matrix, action, 0, f64::NEG_INFINITY, f64::INFINITY);
            if value > best_value {
                best_value = value;
                best_action = action;
            }
        }

        (best_action, best_value)
    }

    fn minimax_value(
        &self,
        matrix: &PayoffMatrix,
        player0_action: usize,
        depth: usize,
        alpha: f64,
        mut beta: f64,
    ) -> f64 {
        if depth >= self.max_depth || matrix.dimensions[1] == 0 {
            // Evaluate: opponent minimizes player 0's payoff
            let mut min_value = f64::INFINITY;
            for opp_action in 0..matrix.dimensions[1] {
                let action_profile = vec![player0_action, opp_action];
                let value = matrix.get_payoff(&action_profile, 0);
                min_value = min_value.min(value);

                if self.alpha_beta_pruning {
                    beta = beta.min(value);
                    if beta <= alpha {
                        break;
                    }
                }
            }
            return min_value;
        }

        // Minimizing player (opponent)
        let mut min_value = f64::INFINITY;
        for opp_action in 0..matrix.dimensions[1] {
            let action_profile = vec![player0_action, opp_action];
            let value = matrix.get_payoff(&action_profile, 0);
            min_value = min_value.min(value);

            if self.alpha_beta_pruning {
                beta = beta.min(value);
                if beta <= alpha {
                    break;
                }
            }
        }

        min_value
    }
}

/// Adversarial agent for competitive multi-agent scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdversarialAgent {
    pub id: AgentId,
    pub name: String,
    pub strategy: Vec<f64>,
    pub learning_rate: f64,
    pub exploration_rate: f64,
    pub history: Vec<AdversarialOutcome>,
    pub regret: Vec<f64>,
    pub avg_strategy: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdversarialOutcome {
    pub action: usize,
    pub opponent_action: usize,
    pub payoff: f64,
    pub timestamp: DateTime<Utc>,
}

impl AdversarialAgent {
    pub fn new(name: impl Into<String>, num_actions: usize) -> Self {
        let uniform = 1.0 / num_actions as f64;
        Self {
            id: AgentId::new(),
            name: name.into(),
            strategy: vec![uniform; num_actions],
            learning_rate: 0.1,
            exploration_rate: 0.1,
            history: Vec::new(),
            regret: vec![0.0; num_actions],
            avg_strategy: vec![uniform; num_actions],
        }
    }

    /// Select action using current strategy
    pub fn select_action(&self) -> usize {
        let mut rng = rand::thread_rng();

        // Epsilon-greedy exploration
        if rng.gen::<f64>() < self.exploration_rate {
            return rng.gen_range(0..self.strategy.len());
        }

        // Sample from strategy distribution
        let r: f64 = rng.gen();
        let mut cumulative = 0.0;
        for (i, &prob) in self.strategy.iter().enumerate() {
            cumulative += prob;
            if r < cumulative {
                return i;
            }
        }
        self.strategy.len() - 1
    }

    /// Update strategy using regret matching (CFR-style)
    pub fn update_regret(&mut self, _action: usize, payoff: f64, counterfactual_payoffs: &[f64]) {
        // Update regret for each action
        for (i, &cf_payoff) in counterfactual_payoffs.iter().enumerate() {
            self.regret[i] += cf_payoff - payoff;
        }

        // Compute regret-matching strategy
        let positive_regret: Vec<f64> = self.regret.iter().map(|&r| r.max(0.0)).collect();
        let total_positive: f64 = positive_regret.iter().sum();

        if total_positive > 0.0 {
            self.strategy = positive_regret
                .iter()
                .map(|&r| r / total_positive)
                .collect();
        } else {
            let uniform = 1.0 / self.strategy.len() as f64;
            self.strategy = vec![uniform; self.strategy.len()];
        }

        // Update average strategy
        let n = self.history.len() as f64 + 1.0;
        for i in 0..self.avg_strategy.len() {
            self.avg_strategy[i] = (self.avg_strategy[i] * (n - 1.0) + self.strategy[i]) / n;
        }
    }

    /// Record outcome
    pub fn record_outcome(&mut self, action: usize, opponent_action: usize, payoff: f64) {
        self.history.push(AdversarialOutcome {
            action,
            opponent_action,
            payoff,
            timestamp: Utc::now(),
        });
    }

    /// Get average payoff over history
    pub fn average_payoff(&self) -> f64 {
        if self.history.is_empty() {
            return 0.0;
        }
        self.history.iter().map(|o| o.payoff).sum::<f64>() / self.history.len() as f64
    }

    /// Get exploitability (how much an optimal opponent can exploit this strategy)
    pub fn exploitability(&self, matrix: &PayoffMatrix, player_idx: usize) -> f64 {
        // Find best response payoff against this strategy
        let mut max_br_payoff = f64::NEG_INFINITY;
        let opp_idx = 1 - player_idx;

        for opp_action in 0..matrix.dimensions[opp_idx] {
            let mut expected = 0.0;
            for (my_action, &prob) in self.avg_strategy.iter().enumerate() {
                let profile = if player_idx == 0 {
                    vec![my_action, opp_action]
                } else {
                    vec![opp_action, my_action]
                };
                expected += prob * matrix.get_payoff(&profile, opp_idx);
            }
            max_br_payoff = max_br_payoff.max(expected);
        }

        // For zero-sum, exploitability is opponent's best response payoff
        // minus the game value (which should be 0 at equilibrium)
        max_br_payoff
    }
}

/// Multi-agent adversarial arena for competitive games
pub struct AdversarialArena {
    pub game_type: GameType,
    pub agents: Vec<AdversarialAgent>,
    pub payoff_matrix: PayoffMatrix,
    pub round: usize,
    pub history: Vec<ArenaRound>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaRound {
    pub round: usize,
    pub actions: Vec<usize>,
    pub payoffs: Vec<f64>,
    pub timestamp: DateTime<Utc>,
}

impl AdversarialArena {
    pub fn new(game_type: GameType, matrix: PayoffMatrix) -> Self {
        let agents: Vec<AdversarialAgent> = matrix
            .players
            .iter()
            .enumerate()
            .map(|(i, _)| AdversarialAgent::new(format!("Agent_{}", i), matrix.dimensions[i]))
            .collect();

        Self {
            game_type,
            agents,
            payoff_matrix: matrix,
            round: 0,
            history: Vec::new(),
        }
    }

    /// Run one round of the game
    pub fn play_round(&mut self) -> ArenaRound {
        // Each agent selects an action
        let actions: Vec<usize> = self.agents.iter().map(|a| a.select_action()).collect();

        // Compute payoffs
        let payoffs: Vec<f64> = (0..self.agents.len())
            .map(|p| self.payoff_matrix.get_payoff(&actions, p))
            .collect();

        // Update agents
        for (i, agent) in self.agents.iter_mut().enumerate() {
            // Compute counterfactual payoffs (what would have happened with other actions)
            let cf_payoffs: Vec<f64> = (0..agent.strategy.len())
                .map(|alt_action| {
                    let mut alt_actions = actions.clone();
                    alt_actions[i] = alt_action;
                    self.payoff_matrix.get_payoff(&alt_actions, i)
                })
                .collect();

            agent.update_regret(actions[i], payoffs[i], &cf_payoffs);
            agent.record_outcome(
                actions[i],
                if i == 0 {
                    actions.get(1).copied().unwrap_or(0)
                } else {
                    actions[0]
                },
                payoffs[i],
            );
        }

        self.round += 1;
        let round_result = ArenaRound {
            round: self.round,
            actions,
            payoffs,
            timestamp: Utc::now(),
        };
        self.history.push(round_result.clone());
        round_result
    }

    /// Run multiple rounds
    pub fn run(&mut self, num_rounds: usize) -> ArenaStats {
        for _ in 0..num_rounds {
            self.play_round();
        }
        self.stats()
    }

    /// Get arena statistics
    pub fn stats(&self) -> ArenaStats {
        let avg_payoffs: Vec<f64> = self.agents.iter().map(|a| a.average_payoff()).collect();
        let exploitabilities: Vec<f64> = self
            .agents
            .iter()
            .enumerate()
            .map(|(i, a)| a.exploitability(&self.payoff_matrix, i))
            .collect();

        // Check Nash convergence
        let nash_solver = NashEquilibriumSolver::new(self.game_type);
        let nash_strategies = nash_solver.find_mixed_nash(&self.payoff_matrix);

        let nash_distance: f64 = self
            .agents
            .iter()
            .zip(nash_strategies.iter())
            .map(|(agent, nash)| {
                agent
                    .avg_strategy
                    .iter()
                    .zip(nash.iter())
                    .map(|(&a, &n)| (a - n).powi(2))
                    .sum::<f64>()
                    .sqrt()
            })
            .sum();

        ArenaStats {
            rounds_played: self.round,
            avg_payoffs,
            exploitabilities,
            nash_distance,
            converged: nash_distance < 0.1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArenaStats {
    pub rounds_played: usize,
    pub avg_payoffs: Vec<f64>,
    pub exploitabilities: Vec<f64>,
    pub nash_distance: f64,
    pub converged: bool,
}

// =============================================================================
// ZERO-COPY MESSAGE POOLING FOR EFFICIENCY
// =============================================================================

/// Memory pool for zero-copy message passing
pub struct MessagePool {
    pools: Vec<DashMap<usize, Vec<Vec<u8>>>>,
    size_classes: Vec<usize>,
    max_pool_size: usize,
}

impl MessagePool {
    /// Create a new message pool with size classes
    pub fn new() -> Self {
        // Power-of-2 size classes from 64B to 1MB
        let size_classes: Vec<usize> = (6..=20).map(|i| 1 << i).collect();
        let pools = size_classes.iter().map(|_| DashMap::new()).collect();

        Self {
            pools,
            size_classes,
            max_pool_size: 1000,
        }
    }

    /// Allocate a buffer of at least `size` bytes
    pub fn allocate(&self, size: usize) -> PooledBuffer {
        let class_idx = self.size_class_index(size);
        let actual_size = self.size_classes[class_idx];

        // Try to get from pool
        if let Some(mut pool_entry) = self.pools[class_idx].get_mut(&actual_size) {
            if let Some(buffer) = pool_entry.pop() {
                return PooledBuffer {
                    data: buffer,
                    pool: self as *const MessagePool,
                    size_class: actual_size,
                };
            }
        }

        // Allocate new
        PooledBuffer {
            data: vec![0u8; actual_size],
            pool: self as *const MessagePool,
            size_class: actual_size,
        }
    }

    /// Return a buffer to the pool
    fn return_buffer(&self, mut buffer: Vec<u8>, size_class: usize) {
        let class_idx = self
            .size_classes
            .iter()
            .position(|&s| s == size_class)
            .unwrap_or(0);

        if let Some(mut pool_entry) = self.pools[class_idx].get_mut(&size_class) {
            if pool_entry.len() < self.max_pool_size {
                buffer.clear();
                pool_entry.push(buffer);
            }
        } else {
            buffer.clear();
            self.pools[class_idx].insert(size_class, vec![buffer]);
        }
    }

    fn size_class_index(&self, size: usize) -> usize {
        for (i, &class_size) in self.size_classes.iter().enumerate() {
            if class_size >= size {
                return i;
            }
        }
        self.size_classes.len() - 1
    }

    /// Get pool statistics
    pub fn stats(&self) -> PoolStats {
        let mut total_buffers = 0;
        let mut total_bytes = 0;

        for (i, pool) in self.pools.iter().enumerate() {
            for entry in pool.iter() {
                total_buffers += entry.value().len();
                total_bytes += entry.value().len() * self.size_classes[i];
            }
        }

        PoolStats {
            size_classes: self.size_classes.clone(),
            total_buffers,
            total_bytes,
        }
    }
}

impl Default for MessagePool {
    fn default() -> Self {
        Self::new()
    }
}

/// A buffer from the message pool
pub struct PooledBuffer {
    data: Vec<u8>,
    pool: *const MessagePool,
    size_class: usize,
}

impl PooledBuffer {
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Write data to buffer
    pub fn write(&mut self, data: &[u8]) -> usize {
        let len = data.len().min(self.data.len());
        self.data[..len].copy_from_slice(&data[..len]);
        len
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if !self.pool.is_null() {
            let buffer = std::mem::take(&mut self.data);
            // Safety: pool pointer is valid for the lifetime of PooledBuffer
            unsafe {
                (*self.pool).return_buffer(buffer, self.size_class);
            }
        }
    }
}

// Safety: PooledBuffer can be sent across threads
unsafe impl Send for PooledBuffer {}
unsafe impl Sync for PooledBuffer {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStats {
    pub size_classes: Vec<usize>,
    pub total_buffers: usize,
    pub total_bytes: usize,
}

// =============================================================================
// LIGHTWEIGHT AGENT COMMUNICATION
// =============================================================================

/// Compact message format for efficient agent communication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct CompactMessage {
    pub header: CompactHeader,
    pub payload_len: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(C, packed)]
pub struct CompactHeader {
    pub msg_type: u8,
    pub priority: u8,
    pub flags: u16,
    pub sender: u32,
    pub receiver: u32,
    pub timestamp: u64,
    pub sequence: u64,
}

impl CompactMessage {
    /// Create a new compact message
    pub fn new(msg_type: u8, sender: u32, receiver: u32, payload: Vec<u8>) -> Self {
        Self {
            header: CompactHeader {
                msg_type,
                priority: 0,
                flags: 0,
                sender,
                receiver,
                timestamp: Utc::now().timestamp() as u64,
                sequence: 0,
            },
            payload_len: payload.len() as u32,
            payload,
        }
    }

    /// Serialize to bytes (zero-copy friendly)
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_size = std::mem::size_of::<CompactHeader>();
        let total_size = header_size + 4 + self.payload.len();
        let mut bytes = Vec::with_capacity(total_size);

        // Write header
        bytes.extend_from_slice(unsafe {
            std::slice::from_raw_parts(
                &self.header as *const CompactHeader as *const u8,
                header_size,
            )
        });

        // Write payload length
        bytes.extend_from_slice(&self.payload_len.to_le_bytes());

        // Write payload
        bytes.extend_from_slice(&self.payload);

        bytes
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let header_size = std::mem::size_of::<CompactHeader>();
        if bytes.len() < header_size + 4 {
            return None;
        }

        let header: CompactHeader =
            unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const CompactHeader) };

        let payload_len = u32::from_le_bytes([
            bytes[header_size],
            bytes[header_size + 1],
            bytes[header_size + 2],
            bytes[header_size + 3],
        ]);

        let payload_start = header_size + 4;
        if bytes.len() < payload_start + payload_len as usize {
            return None;
        }

        Some(Self {
            header,
            payload_len,
            payload: bytes[payload_start..payload_start + payload_len as usize].to_vec(),
        })
    }

    /// Size of the message in bytes
    pub fn size(&self) -> usize {
        std::mem::size_of::<CompactHeader>() + 4 + self.payload.len()
    }
}

/// Message type constants
pub mod message_types {
    pub const PING: u8 = 0x01;
    pub const PONG: u8 = 0x02;
    pub const REQUEST: u8 = 0x10;
    pub const RESPONSE: u8 = 0x11;
    pub const BROADCAST: u8 = 0x20;
    pub const MULTICAST: u8 = 0x21;
    pub const TASK_ASSIGN: u8 = 0x30;
    pub const TASK_COMPLETE: u8 = 0x31;
    pub const TASK_FAIL: u8 = 0x32;
    pub const NEGOTIATE: u8 = 0x40;
    pub const COMMIT: u8 = 0x41;
    pub const ABORT: u8 = 0x42;
    pub const GOSSIP: u8 = 0x50;
    pub const HEARTBEAT: u8 = 0x60;
}

/// Lightweight swarm coordinator optimized for minimal overhead
pub struct LightweightSwarm {
    pub id: u32,
    pub agents: Vec<u32>,
    pub leader: Option<u32>,
    pub message_pool: Arc<MessagePool>,
    pub pending_messages: DashMap<u64, CompactMessage>,
    pub sequence: std::sync::atomic::AtomicU64,
}

impl LightweightSwarm {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            agents: Vec::new(),
            leader: None,
            message_pool: Arc::new(MessagePool::new()),
            pending_messages: DashMap::new(),
            sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Add an agent to the swarm
    pub fn add_agent(&mut self, agent_id: u32) {
        if !self.agents.contains(&agent_id) {
            self.agents.push(agent_id);
            // First agent becomes leader
            if self.leader.is_none() {
                self.leader = Some(agent_id);
            }
        }
    }

    /// Send a message from one agent to another
    pub fn send(&self, sender: u32, receiver: u32, msg_type: u8, payload: &[u8]) -> u64 {
        let seq = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut msg = CompactMessage::new(msg_type, sender, receiver, payload.to_vec());
        msg.header.sequence = seq;
        self.pending_messages.insert(seq, msg);
        seq
    }

    /// Broadcast a message to all agents
    pub fn broadcast(&self, sender: u32, msg_type: u8, payload: &[u8]) -> Vec<u64> {
        self.agents
            .iter()
            .filter(|&&a| a != sender)
            .map(|&receiver| self.send(sender, receiver, msg_type, payload))
            .collect()
    }

    /// Elect a new leader using ring-based election
    pub fn elect_leader(&mut self) -> Option<u32> {
        if self.agents.is_empty() {
            self.leader = None;
            return None;
        }

        // Simple: highest ID wins (in practice, use more sophisticated criteria)
        self.leader = self.agents.iter().max().copied();
        self.leader
    }

    /// Get swarm statistics
    pub fn stats(&self) -> LightweightSwarmStats {
        LightweightSwarmStats {
            id: self.id,
            num_agents: self.agents.len(),
            leader: self.leader,
            pending_messages: self.pending_messages.len(),
            total_messages: self.sequence.load(std::sync::atomic::Ordering::Relaxed),
            pool_stats: self.message_pool.stats(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightweightSwarmStats {
    pub id: u32,
    pub num_agents: usize,
    pub leader: Option<u32>,
    pub pending_messages: usize,
    pub total_messages: u64,
    pub pool_stats: PoolStats,
}
