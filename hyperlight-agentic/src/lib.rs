//! # Hyperlight Agentic Web Stack
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

use std::collections::{HashMap, VecDeque, BTreeMap};
use std::sync::Arc;
use std::time::Duration;
use std::net::SocketAddr;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use petgraph::graph::{DiGraph, NodeIndex};
use sha2::{Sha256, Digest};
use tokio::sync::{mpsc, RwLock, broadcast, oneshot, Mutex};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use rand::Rng;

use hyperlight_neural::MirasVariant;
use hyperlight_crypto::MirasTitansPredictor;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
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
    Extract { query: SemanticQuery, from: ResourceLocator },
    /// Submit data to a resource
    Submit { data: serde_json::Value, to: ResourceLocator },
    /// Learn something new
    Learn { topic: String, depth: LearningDepth },
    /// Find agents with specific capabilities
    FindAgents { capabilities: Vec<AgentCapability> },
    /// Form a swarm for collective task
    FormSwarm { task: Box<SwarmTask> },
    /// Execute a multi-step plan
    ExecutePlan { plan: Plan },
    /// Monitor a resource for changes
    Monitor { resource: ResourceLocator, interval: Duration },
    /// Custom goal with semantic description
    Custom { description: String, parameters: serde_json::Value },
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
    Semantic { concept: String, constraints: Vec<String> },
    /// Agent-relative path
    AgentPath { agent: AgentId, path: String },
    /// Knowledge graph node
    KnowledgeNode { graph: String, node_id: String },
    /// Latent space coordinates (for neural resources)
    LatentCoord { space: String, coordinates: Vec<f32> },
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
    Execute { command: String, args: serde_json::Value },
    /// Send message to another agent
    Message { to: AgentId, content: Box<AgentMessage> },
    /// Store knowledge
    Store { key: String, value: serde_json::Value },
    /// Retrieve knowledge
    Retrieve { key: String },
    /// Wait for a condition
    Wait { condition: Condition, timeout: Duration },
    /// Branch based on condition
    Branch { condition: Condition, if_true: Box<Action>, if_false: Box<Action> },
    /// Execute multiple actions in parallel
    Parallel(Vec<Action>),
    /// Execute multiple actions in sequence
    Sequence(Vec<Action>),
    /// Delegate to another agent
    Delegate { to: AgentId, task: Box<Goal> },
    /// Learn from current context
    Learn { topic: String },
    /// Custom action
    Custom { name: String, params: serde_json::Value },
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
    ValueEquals { path: String, expected: serde_json::Value },
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
    Custom { predicate: String, args: serde_json::Value },
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
    Response { data: serde_json::Value, confidence: f32 },
    /// Request to perform an action
    ActionRequest(Box<Action>),
    /// Confirmation of action completion
    ActionComplete { success: bool, result: Option<serde_json::Value> },
    /// Invitation to join a swarm
    SwarmInvite(SwarmTask),
    /// Accept/reject swarm invitation
    SwarmResponse { accepted: bool, reason: Option<String> },
    /// Knowledge sharing
    KnowledgeShare { topic: String, knowledge: serde_json::Value },
    /// Trust update
    TrustUpdate { level: TrustLevel, reason: String },
    /// Heartbeat/ping
    Heartbeat,
    /// Error notification
    Error { code: String, message: String },
    /// Custom message type
    Custom { msg_type: String, payload: serde_json::Value },
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
        let mut results: Vec<(String, f32)> = self.embeddings
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
    /// Knowledge graph
    knowledge: Arc<RwLock<KnowledgeGraph>>,
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
    TrustChanged { agent: AgentId, new_level: TrustLevel },
    /// Custom event
    Custom { event_type: String, data: serde_json::Value },
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
            hyperlight_crypto::TitansConfig {
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
        
        Self {
            profile,
            intentions: DashMap::new(),
            known_agents: DashMap::new(),
            swarms: DashMap::new(),
            knowledge: Arc::new(RwLock::new(KnowledgeGraph::new())),
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
                    preconditions: vec![Condition::HasCapability(AgentCapability::SwarmParticipation)],
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
    pub async fn store_knowledge(&self, id: String, label: String, node_type: String, properties: serde_json::Value) {
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
        self.known_agents.iter().map(|r| r.value().clone()).collect()
    }
    
    /// Find agents with specific capability
    pub fn find_agents_with_capability(&self, cap: &AgentCapability) -> Vec<AgentProfile> {
        self.known_agents
            .iter()
            .filter(|r| r.value().capabilities.iter().any(|c| std::mem::discriminant(c) == std::mem::discriminant(cap)))
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
            .filter(|r| r.value().status == IntentionStatus::Active || r.value().status == IntentionStatus::Pending)
            .map(|r| r.value().clone())
            .collect()
    }
    
    /// Update trust level for an agent
    pub fn update_trust(&self, agent_id: AgentId, level: TrustLevel) {
        if let Some(mut profile) = self.known_agents.get_mut(&agent_id) {
            profile.trust_level = level;
            let _ = self.events.send(AgenticEvent::TrustChanged { agent: agent_id, new_level: level });
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
    StateChanged { key: String, old: serde_json::Value, new: serde_json::Value },
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
                        let all_deps_done = plan.dependencies
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
                side_effects.push(SideEffect::ResourceAccessed { locator: locator.clone() });
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
            Action::Execute { command, args } => {
                Ok(serde_json::json!({
                    "command": command,
                    "args": args,
                    "status": "executed"
                }))
            }
            Action::Store { key, value } => {
                side_effects.push(SideEffect::KnowledgeAdded { key: key.clone() });
                self.runtime.store_knowledge(
                    key.clone(),
                    key.clone(),
                    "stored".to_string(),
                    value.clone()
                ).await;
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
            Action::Wait { condition: _, timeout } => {
                tokio::time::sleep(timeout.min(Duration::from_secs(10))).await;
                Ok(serde_json::json!({ "waited": timeout.as_secs() }))
            }
            Action::Parallel(actions) => {
                let futures: Vec<_> = actions.into_iter()
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
            Action::Branch { condition, if_true, if_false } => {
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
                side_effects.push(SideEffect::MessageSent { to, message_id: msg_id });
                Ok(serde_json::json!({ "message_id": msg_id.to_string() }))
            }
            Action::Delegate { to, task } => {
                Ok(serde_json::json!({
                    "delegated_to": to.0.to_string(),
                    "task": format!("{:?}", task)
                }))
            }
            Action::Custom { name, params } => {
                Ok(serde_json::json!({
                    "custom_action": name,
                    "params": params
                }))
            }
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
            Condition::ValueEquals { path: _, expected: _ } => true, // Simplified
            Condition::AgentAvailable(id) => self.runtime.known_agents.contains_key(id),
            Condition::HasCapability(cap) => {
                self.runtime.profile().capabilities.iter()
                    .any(|c| std::mem::discriminant(c) == std::mem::discriminant(cap))
            }
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
            Condition::Custom { predicate: _, args: _ } => true,
        }
    }
}

// =============================================================================
// SWARM COORDINATOR
// =============================================================================

/// Coordinates swarm formation and task distribution
pub struct SwarmCoordinator {
    runtime: Arc<AgenticWebRuntime>,
    active_swarms: DashMap<Uuid, SwarmState>,
    pending_invites: DashMap<Uuid, Vec<AgentId>>,
}

/// Internal state for a swarm
#[derive(Debug, Clone)]
pub struct SwarmState {
    pub swarm: Swarm,
    pub task_assignments: HashMap<AgentId, Vec<Uuid>>,
    pub partial_results: HashMap<AgentId, serde_json::Value>,
    pub started_at: DateTime<Utc>,
    pub phase: SwarmPhase,
}

/// Phase of swarm execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwarmPhase {
    Forming,
    Distributing,
    Executing,
    Aggregating,
    Validating,
    Complete,
}

impl SwarmCoordinator {
    pub fn new(runtime: Arc<AgenticWebRuntime>) -> Self {
        Self {
            runtime,
            active_swarms: DashMap::new(),
            pending_invites: DashMap::new(),
        }
    }
    
    /// Create and start forming a new swarm
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
        
        let state = SwarmState {
            swarm,
            task_assignments: HashMap::new(),
            partial_results: HashMap::new(),
            started_at: Utc::now(),
            phase: SwarmPhase::Forming,
        };
        
        self.active_swarms.insert(swarm_id, state);
        let _ = self.runtime.events.send(AgenticEvent::SwarmFormed(swarm_id));
        
        // Find and invite suitable agents
        let candidates = self.find_candidates(&task);
        self.pending_invites.insert(swarm_id, candidates.clone());
        
        swarm_id
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
            
            // Check if we have enough members to start
            if state.swarm.members.len() >= state.swarm.task.min_members {
                state.swarm.status = SwarmStatus::Active;
                state.phase = SwarmPhase::Distributing;
            }
        }
    }
    
    /// Distribute tasks to swarm members
    pub fn distribute_tasks(&self, swarm_id: Uuid) -> HashMap<AgentId, Vec<serde_json::Value>> {
        let mut assignments = HashMap::new();
        
        if let Some(mut state) = self.active_swarms.get_mut(&swarm_id) {
            let workers: Vec<_> = state.swarm.members
                .iter()
                .filter(|m| matches!(m.role, SwarmRole::Worker | SwarmRole::Coordinator))
                .map(|m| m.agent_id)
                .collect();
            
            if !workers.is_empty() {
                // Simple round-robin distribution
                // In reality, this would be based on the task decomposition
                for (i, worker) in workers.iter().enumerate() {
                    assignments.insert(*worker, vec![serde_json::json!({
                        "subtask_id": i,
                        "swarm_id": swarm_id.to_string(),
                        "instructions": format!("Execute subtask {}", i)
                    })]);
                }
                
                state.phase = SwarmPhase::Executing;
            }
        }
        
        assignments
    }
    
    /// Submit partial result from a member
    pub fn submit_result(&self, swarm_id: Uuid, agent_id: AgentId, result: serde_json::Value) {
        if let Some(mut state) = self.active_swarms.get_mut(&swarm_id) {
            state.partial_results.insert(agent_id, result);
            
            // Update contribution score
            if let Some(member) = state.swarm.members.iter_mut().find(|m| m.agent_id == agent_id) {
                member.contribution_score += 1.0;
            }
            
            // Check if all results are in
            let workers: Vec<_> = state.swarm.members
                .iter()
                .filter(|m| matches!(m.role, SwarmRole::Worker | SwarmRole::Coordinator))
                .map(|m| m.agent_id)
                .collect();
            
            if workers.iter().all(|w| state.partial_results.contains_key(w)) {
                state.phase = SwarmPhase::Aggregating;
            }
        }
    }
    
    /// Aggregate results and complete the swarm task
    pub fn aggregate_results(&self, swarm_id: Uuid) -> Option<serde_json::Value> {
        if let Some(mut state) = self.active_swarms.get_mut(&swarm_id) {
            if state.phase != SwarmPhase::Aggregating {
                return None;
            }
            
            // Combine all partial results
            let combined: Vec<_> = state.partial_results.values().cloned().collect();
            let result = serde_json::json!({
                "swarm_id": swarm_id.to_string(),
                "members": state.swarm.members.len(),
                "partial_results": combined,
                "completed_at": Utc::now().to_rfc3339()
            });
            
            state.phase = SwarmPhase::Complete;
            state.swarm.status = SwarmStatus::Completed;
            
            let _ = self.runtime.events.send(AgenticEvent::SwarmCompleted(swarm_id));
            
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
        self.active_swarms.iter()
            .filter(|s| s.swarm.status == SwarmStatus::Active || s.swarm.status == SwarmStatus::Forming)
            .map(|s| s.swarm.id)
            .collect()
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
            self.by_capability.entry(cap_key).or_default().push(agent_id);
        }
        
        // Index by trust level
        self.by_trust.entry(profile.trust_level).or_default().push(agent_id);
        
        // Store agent
        self.agents.insert(agent_id, RegisteredAgent {
            profile,
            endpoint,
            last_heartbeat: Utc::now(),
            is_online: true,
            metadata: HashMap::new(),
        });
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
        self.agents.iter()
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
    SwarmResponse { swarm_id: Uuid, accepted: bool, role: Option<SwarmRole> },
    /// Query knowledge
    KnowledgeQuery { query: SemanticQuery },
    /// Knowledge response
    KnowledgeResponse { data: serde_json::Value },
    /// Error
    Error { code: u32, message: String },
}

impl AgentServer {
    pub fn new(runtime: Arc<AgenticWebRuntime>, registry: Arc<AgentRegistry>, addr: SocketAddr) -> Self {
        Self {
            runtime,
            registry,
            addr,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
    
    /// Start the server
    pub async fn start(&self) -> anyhow::Result<()> {
        self.running.store(true, std::sync::atomic::Ordering::SeqCst);
        
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
        self.running.store(false, std::sync::atomic::Ordering::SeqCst);
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
                AgentProtocolMessage::MessageAck { message_id: Uuid::new_v4() }
            }
            AgentProtocolMessage::Heartbeat(id) => {
                registry.heartbeat(id);
                AgentProtocolMessage::MessageAck { message_id: Uuid::new_v4() }
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
        self.connections.insert(addr.to_string(), Arc::new(Mutex::new(stream)));
        
        // Register ourselves
        let register_msg = AgentProtocolMessage::Register(self.runtime.profile().clone());
        self.send(addr, register_msg).await?;
        
        Ok(())
    }
    
    /// Send a message to a server
    pub async fn send(&self, addr: &str, msg: AgentProtocolMessage) -> anyhow::Result<AgentProtocolMessage> {
        let conn = self.connections.get(addr)
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
    pub async fn discover(&self, addr: &str, capability: AgentCapability) -> anyhow::Result<Vec<RegisteredAgent>> {
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
    Repeater { child: Box<BehaviorNode>, count: Option<u32> },
    /// Invert child result
    Inverter(Box<BehaviorNode>),
    /// Always succeed
    Succeeder(Box<BehaviorNode>),
    /// Run child until condition is met
    UntilSuccess(Box<BehaviorNode>),
    /// Run children in parallel
    Parallel { children: Vec<BehaviorNode>, success_threshold: usize },
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
            BehaviorNode::Inverter(child) => {
                match Box::pin(self.execute(child)).await {
                    BehaviorResult::Success => BehaviorResult::Failure,
                    BehaviorResult::Failure => BehaviorResult::Success,
                    BehaviorResult::Running => BehaviorResult::Running,
                }
            }
            BehaviorNode::Succeeder(child) => {
                let _ = Box::pin(self.execute(child)).await;
                BehaviorResult::Success
            }
            BehaviorNode::UntilSuccess(child) => {
                loop {
                    match Box::pin(self.execute(child)).await {
                        BehaviorResult::Success => return BehaviorResult::Success,
                        BehaviorResult::Running => return BehaviorResult::Running,
                        BehaviorResult::Failure => continue,
                    }
                }
            }
            BehaviorNode::Parallel { children, success_threshold } => {
                let futures: Vec<_> = children.iter()
                    .map(|c| Box::pin(self.execute(c)))
                    .collect();
                
                let results = futures::future::join_all(futures).await;
                let successes = results.iter().filter(|&&r| r == BehaviorResult::Success).count();
                
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
        self.runtime.update_intention_status(intention_id, IntentionStatus::Active);
        
        // Plan
        let plan = self.runtime.plan(intention_id).await
            .ok_or("Failed to create plan")?;
        
        // Execute
        let results = self.executor.execute_plan(plan).await?;
        
        // Mark complete
        self.runtime.update_intention_status(intention_id, IntentionStatus::Completed);
        
        // Aggregate results
        let final_result: Vec<_> = results.iter()
            .filter_map(|r| r.data.clone())
            .collect();
        
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
    pub fn generate(name: &str) -> Self {
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
        if chunk.len() > 1 { result.push(ALPHABET[(n >> 6) as usize & 63] as char); }
        if chunk.len() > 2 { result.push(ALPHABET[n as usize & 63] as char); }
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
    LatentSpace { 
        encoder: String,
        dimension: usize,
    },
    /// Structured semantic messages
    SemanticJSON {
        schema_version: String,
    },
    /// Compressed binary protocol
    BinaryCompact {
        compression: String,
    },
    /// Natural language with embeddings
    NaturalLanguage {
        language: String,
        embedding_model: String,
    },
    /// Custom protocol with specification
    Custom {
        name: String,
        spec_uri: String,
    },
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
    pub fn initiate(initiator: &str, responder: &str, protocols: Vec<CommunicationProtocol>) -> Self {
        Self {
            initiator: initiator.to_string(),
            responder: responder.to_string(),
            proposed_protocols: protocols,
            selected: None,
            status: NegotiationStatus::Initiated,
        }
    }
    
    /// Respond to a negotiation request
    pub fn respond(&mut self, acceptable: &[CommunicationProtocol]) -> Option<&CommunicationProtocol> {
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
            self.routing.default = agent_id.clone();
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
        let specialists: Vec<_> = self.components.iter()
            .filter(|c| c.active)
            .filter(|c| match &c.role {
                ComponentRole::Specialist { capability: cap } => 
                    std::mem::discriminant(cap) == std::mem::discriminant(capability),
                ComponentRole::Primary { domains: _ } => true,
                _ => false,
            })
            .map(|c| c.agent_id.clone())
            .collect();
        
        if specialists.is_empty() {
            vec![self.routing.default.clone()]
        } else {
            specialists
        }
    }
    
    /// Combine capabilities from all components
    pub fn refresh_capabilities(&mut self, component_caps: &HashMap<AgentId, Vec<AgentCapability>>) {
        self.capabilities.clear();
        for comp in &self.components {
            if let Some(caps) = component_caps.get(&comp.agent_id) {
                for cap in caps {
                    if !self.capabilities.iter().any(|c| 
                        std::mem::discriminant(c) == std::mem::discriminant(cap)
                    ) {
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
    Subscription { credits_per_period: u64, period_days: u32 },
    /// Usage-based
    UsageBased { rate: f64, unit: String },
    /// Negotiable
    Negotiable { min_credits: u64 },
}

/// Price range for indexing
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum PriceRange {
    Free,
    Low,      // < 10 credits
    Medium,   // 10-100 credits
    High,     // 100-1000 credits
    Premium,  // > 1000 credits
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
            self.by_capability.entry(key)
                .or_insert_with(Vec::new)
                .push(id);
        }
        
        // Index by price
        let price_range = self.price_to_range(&listing.pricing);
        self.by_price.entry(price_range)
            .or_insert_with(Vec::new)
            .push(id);
        
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
            PricingModel::Subscription { credits_per_period, period_days: _ } if *credits_per_period < 100 => PriceRange::Medium,
            PricingModel::Subscription { credits_per_period: _, period_days: _ } => PriceRange::High,
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
                PricingModel::Subscription { credits_per_period, period_days: _ } => *credits_per_period <= max_credits,
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
            let rep_a = self.reputation.get(&a.provider).map(|r| r.score).unwrap_or(0.0);
            let rep_b = self.reputation.get(&b.provider).map(|r| r.score).unwrap_or(0.0);
            rep_b.partial_cmp(&rep_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        
        // Limit results
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }
        
        results
    }
    
    /// Initiate a transaction
    pub async fn procure(&self, listing_id: Uuid, consumer: AgentId) -> Result<Uuid, String> {
        let listing = self.listings.get(&listing_id)
            .ok_or("Listing not found")?
            .clone();
        
        if listing.status != ListingStatus::Active {
            return Err("Listing not active".to_string());
        }
        
        let credits = match &listing.pricing {
            PricingModel::Free => 0,
            PricingModel::PerRequest { credits } => *credits,
            PricingModel::Subscription { credits_per_period, period_days: _ } => *credits_per_period,
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
    pub async fn complete_transaction(&self, tx_id: Uuid, success: bool, rating: Option<u8>, review: Option<String>) {
        let provider = {
            let mut txs = self.transactions.write().await;
            if let Some(tx) = txs.iter_mut().find(|t| t.id == tx_id) {
                tx.completed_at = Some(Utc::now());
                tx.rating = rating;
                tx.review = review.clone();
                tx.status = if success {
                    TransactionStatus::Completed
                } else {
                    TransactionStatus::Failed { reason: "Service failed".to_string() }
                };
                Some(tx.provider.clone())
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
        rep.success_rate = (rep.success_rate * prev_completed + if success { 1.0 } else { 0.0 }) / (prev_completed + 1.0);
        
        // Update rating
        if let Some(r) = rating {
            let prev_reviews = rep.reviews as f32;
            rep.reviews += 1;
            rep.avg_rating = (rep.avg_rating * prev_reviews + r as f32) / (prev_reviews + 1.0);
        }
        
        // Calculate overall score
        rep.score = (rep.success_rate * 40.0) + (rep.avg_rating / 5.0 * 40.0) + (rep.completed as f32).min(20.0);
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
        let insert_pos = timeline.binary_search_by(|e| e.timestamp.cmp(&event.timestamp))
            .unwrap_or_else(|pos| pos);
        timeline.insert(insert_pos, event.clone());
        
        // Update causal graph
        let mut graph = self.causal_graph.write().await;
        let node = graph.add_node(id.to_string());
        
        // Link to causes
        for cause_id in &event.causes {
            let cause_str = cause_id.to_string();
            if let Some((cause_idx, _)) = graph.node_indices()
                .find_map(|i| graph.node_weight(i).filter(|w| *w == &cause_str).map(|_| (i, ())))
            {
                graph.add_edge(cause_idx, node, CausalRelation {
                    relation_type: CausalType::DirectCause,
                    strength: 0.8,
                    delay: None,
                    probability: 1.0,
                });
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
    pub async fn query_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<TemporalEvent> {
        let timeline = self.timeline.read().await;
        timeline.iter()
            .filter(|e| e.timestamp >= start && e.timestamp <= end)
            .cloned()
            .collect()
    }
    
    /// Find causal chain between events
    pub async fn find_causal_chain(&self, from: Uuid, to: Uuid) -> Option<Vec<Uuid>> {
        let graph = self.causal_graph.read().await;
        
        let from_str = from.to_string();
        let to_str = to.to_string();
        
        let from_idx = graph.node_indices()
            .find(|i| graph.node_weight(*i).map(|w| w == &from_str).unwrap_or(false))?;
        let to_idx = graph.node_indices()
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
    pub fn validate_schedule(&self, schedule: &[(Uuid, DateTime<Utc>, Option<Duration>)]) -> Vec<String> {
        let mut violations = Vec::new();
        
        for constraint in &self.constraints {
            let involved: Vec<_> = schedule.iter()
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
    pub fn create_pool(&self, name: &str, owner: AgentId, initial_context: serde_json::Value) -> String {
        let pool = ContextPool {
            name: name.to_string(),
            context: initial_context,
            schema: None,
            participants: vec![owner.clone()],
            owner,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            version: 1,
        };
        
        self.pools.insert(name.to_string(), pool);
        name.to_string()
    }
    
    /// Share context to a pool
    pub fn share(&self, pool_name: &str, context: serde_json::Value, agent: &AgentId) -> Result<u64, String> {
        let mut pool = self.pools.get_mut(pool_name)
            .ok_or("Pool not found")?;
        
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
        let pool = self.pools.get(pool_name)
            .ok_or("Pool not found")?;
        
        if !self.has_permission(pool_name, agent, ContextPermission::Read) {
            return Err("No read permission".to_string());
        }
        
        Ok(pool.context.clone())
    }
    
    /// Join a context pool
    pub fn join_pool(&self, pool_name: &str, agent: AgentId) -> Result<(), String> {
        let mut pool = self.pools.get_mut(pool_name)
            .ok_or("Pool not found")?;
        
        if !pool.participants.contains(&agent) {
            pool.participants.push(agent);
        }
        
        Ok(())
    }
    
    /// Add a context policy
    pub fn add_policy(&mut self, policy: ContextPolicy) {
        self.policies.push(policy);
    }
    
    fn has_permission(&self, pool_name: &str, agent: &AgentId, required: ContextPermission) -> bool {
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
            if pool_name.starts_with(&policy.pool_pattern) || policy.pool_pattern == "*" {
                if policy.allowed_agents.contains(agent) {
                    match (&policy.permission, &required) {
                        (ContextPermission::Admin, _) => return true,
                        (ContextPermission::ReadWrite, _) => return true,
                        (ContextPermission::Write, ContextPermission::Write) => return true,
                        (ContextPermission::Read, ContextPermission::Read) => return true,
                        _ => {}
                    }
                }
            }
        }
        
        false
    }
    
    /// List pools an agent has access to
    pub fn list_accessible_pools(&self, agent: &AgentId) -> Vec<String> {
        self.pools.iter()
            .filter(|entry| {
                entry.participants.contains(agent) || 
                self.has_permission(entry.key(), agent, ContextPermission::Read)
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
                schema_version: "1.0".to_string() 
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
    pub fn with_marketplace(mut self, title: &str, description: &str, pricing: PricingModel) -> Self {
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
        Self { major, minor, patch, prerelease: None }
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
        Self { major: self.major, minor: self.minor, patch: self.patch + 1, prerelease: None }
    }

    pub fn bump_minor(&self) -> Self {
        Self { major: self.major, minor: self.minor + 1, patch: 0, prerelease: None }
    }

    pub fn bump_major(&self) -> Self {
        Self { major: self.major + 1, minor: 0, patch: 0, prerelease: None }
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
    UpdateBehavior { old_behavior: String, new_behavior: String },
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
            let next_migration = self.migrations.iter()
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
    pub fn snapshot(&self, agent_id: Uuid, version: AgentVersion, state: serde_json::Value) -> AgentSnapshot {
        let snapshot = AgentSnapshot {
            id: agent_id,
            version,
            timestamp: Utc::now(),
            state: state.clone(),
            knowledge_hash: format!("{:x}", simple_hash(&state.to_string())),
        };
        
        self.rollback_points
            .entry(agent_id)
            .or_insert_with(Vec::new)
            .push(snapshot.clone());
        
        snapshot
    }

    /// Apply migration to agent
    pub async fn apply_migration(&self, agent_id: Uuid, migration: &Migration) -> Result<(), String> {
        for step in &migration.steps {
            self.apply_step(agent_id, step).await?;
        }
        
        self.applied
            .entry(agent_id)
            .or_insert_with(Vec::new)
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
                println!("  [Migration] Transforming knowledge with: {}", transform_fn);
            }
            MigrationStep::UpdateBehavior { old_behavior, new_behavior } => {
                println!("  [Migration] Updating behavior {} -> {}", old_behavior, new_behavior);
            }
            MigrationStep::MigrateState { key, transform } => {
                println!("  [Migration] Migrating state key {} with: {}", key, transform);
            }
            MigrationStep::Custom { script } => {
                println!("  [Migration] Executing custom script: {}", script);
            }
        }
        Ok(())
    }

    /// Rollback to previous version
    pub async fn rollback(&self, agent_id: Uuid) -> Result<AgentSnapshot, String> {
        let mut snapshots = self.rollback_points
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
    Pool { agents: Vec<Uuid>, strategy: LoadBalanceStrategy },
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
    TimeWindow { start: DateTime<Utc>, end: DateTime<Utc> },
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
            .or_insert_with(Vec::new)
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
        
        self.subscriptions
            .entry(agent_id)
            .or_insert_with(Vec::new)
            .push(sub);
        
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
            SemanticPattern::ContentType(ct) => {
                message.content.get("type")
                    .and_then(|v| v.as_str())
                    .map(|t| t == ct)
                    .unwrap_or(false)
            }
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
            RouteDestination::Pool { agents, strategy } => {
                match strategy {
                    LoadBalanceStrategy::Random => {
                        agents.first().copied().into_iter().collect()
                    }
                    LoadBalanceStrategy::RoundRobin => {
                        agents.first().copied().into_iter().collect()
                    }
                    _ => agents.clone(),
                }
            }
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
    pub fn vote(&self, proposal_id: Uuid, voter: Uuid, decision: VoteDecision, weight: f64, reasoning: Option<&str>) -> Result<(), String> {
        let mut proposal = self.proposals
            .get_mut(&proposal_id)
            .ok_or("Proposal not found")?;
        
        if !proposal.participants.contains(&voter) {
            return Err("Not a participant".to_string());
        }
        
        if proposal.status != ConsensusStatus::Proposed && proposal.status != ConsensusStatus::Voting {
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
        
        self.votes
            .entry(proposal_id)
            .or_insert_with(Vec::new)
            .push(vote);
        
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
                summary.accept_count > summary.reject_count && 
                summary.accept_count > (votes.len() / 2)
            }
            QuorumRequirement::SuperMajority => {
                let threshold = (votes.len() as f64 * 0.66).ceil() as usize;
                summary.accept_count >= threshold
            }
            QuorumRequirement::Unanimous => {
                summary.accept_count == proposal.participants.len() && summary.reject_count == 0
            }
            QuorumRequirement::MinVotes(min) => {
                summary.accept_count >= *min
            }
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
        let proposal = self.proposals
            .get(&proposal_id)
            .ok_or("Proposal not found")?;
        
        let votes = self.votes
            .get(&proposal_id)
            .ok_or("No votes found")?;
        
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
    IntentionCreated { goal: String },
    /// Planning phase
    PlanningStarted,
    PlanningCompleted { steps: usize },
    /// Action execution
    ActionStarted { action: String },
    ActionCompleted { result: String },
    ActionFailed { error: String },
    /// Knowledge operations
    KnowledgeQuery { query: String },
    KnowledgeUpdate { node_id: String },
    /// Communication
    MessageSent { to: Uuid },
    MessageReceived { from: Uuid },
    /// State changes
    StateChange { key: String, old: String, new: String },
    /// Custom events
    Custom { name: String },
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
    OnMessage { from: Option<Uuid>, to: Option<Uuid> },
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
    pub fn record(&self, session_id: Uuid, event_type: TraceEventType, data: serde_json::Value) -> Option<Uuid> {
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
    pub fn add_breakpoint(&self, agent_id: Uuid, condition: BreakpointCondition, action: BreakpointAction) -> Uuid {
        let bp = Breakpoint {
            id: Uuid::new_v4(),
            condition,
            action,
            hit_count: 0,
            enabled: true,
        };
        
        let id = bp.id;
        self.breakpoints
            .entry(agent_id)
            .or_insert_with(Vec::new)
            .push(bp);
        
        id
    }

    /// Check breakpoints
    pub fn check_breakpoints(&self, agent_id: Uuid, event: &TraceEventType) -> Option<BreakpointAction> {
        let breakpoints = self.breakpoints.get(&agent_id)?;
        
        for bp in breakpoints.iter() {
            if !bp.enabled {
                continue;
            }
            
            let matches = match (&bp.condition, event) {
                (BreakpointCondition::OnAction(action), TraceEventType::ActionStarted { action: a }) => {
                    action == a
                }
                (BreakpointCondition::OnStateChange { key }, TraceEventType::StateChange { key: k, .. }) => {
                    key == k
                }
                (BreakpointCondition::OnMessage { from, to }, TraceEventType::MessageReceived { from: f }) => {
                    from.map_or(true, |fr| &fr == f) && to.is_none()
                }
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
        self.watchers
            .entry(agent_id)
            .or_insert_with(Vec::new)
            .push(watcher);
        
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
                TraceEventType::MessageSent { .. } | TraceEventType::MessageReceived { .. } => message_count += 1,
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
        rule.conditions.iter().all(|c| self.evaluate_condition(c, context))
    }

    fn evaluate_condition(&self, condition: &PolicyCondition, _context: &EvaluationContext) -> bool {
        match condition {
            PolicyCondition::TimeWindow { start: _, end: _ } => {
                // Would parse and check time
                true
            }
            PolicyCondition::RateLimit { max: _, window_secs: _ } => {
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
        let mut stream = self.events.entry(aggregate_id).or_insert_with(Vec::new);
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
            .map(|e| e.iter().filter(|ev| ev.version >= from_version).cloned().collect())
            .unwrap_or_default()
    }

    /// Create projection
    pub fn create_projection(&self, name: &str, event_types: Vec<String>, initial: serde_json::Value) {
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
                .or_insert_with(Vec::new)
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
    pub fn establish_trust(&self, local: &str, remote: &str, level: TrustLevel, bidirectional: bool) {
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
    pub fn query(&self, predicate: &str, bindings: &[BindingPattern]) -> Vec<(Fact, HashMap<String, FactValue>)> {
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

    fn match_bindings(&self, args: &[FactValue], patterns: &[BindingPattern]) -> Option<HashMap<String, FactValue>> {
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
                        self.facts.insert(inference.result.id.clone(), inference.result.clone());
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

    fn create_inference(&self, rule: &InferenceRule, bindings: &HashMap<String, FactValue>) -> Inference {
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
                    from: bindings.keys().cloned().collect() 
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
            if existing.predicate == fact.predicate && existing.arguments.len() == fact.arguments.len() {
                let all_equal = existing.arguments.iter()
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
                explanation_type: ExplanationType::External { source: source.clone() },
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
    pub fn remember(&self, content: serde_json::Value, context: MemoryContext, importance: f64) -> Uuid {
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
        
        self.associations.insert((source.to_string(), target.to_string()), assoc);
    }

    /// Retrieve memories by similarity
    pub fn recall(&self, query_embedding: &[f32], limit: usize) -> Vec<EpisodicMemory> {
        let mut results: Vec<_> = self.episodes.iter()
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
        let mut results: Vec<_> = self.episodes.iter()
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
        self.working_memory.try_lock().ok().map(|wm| wm.items.iter().cloned().collect())
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
    Achievement { target_state: String },
    Maintenance { invariant: String },
    Optimization { metric: String, direction: OptimizationDirection },
    Query { question: String },
    Procedure { steps: Vec<String> },
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
        self.goals.iter()
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
            
            let total_progress: f64 = goal.subgoals.iter()
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
        
        let children: Vec<GoalTree> = goal.subgoals.iter()
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
            deadline: Utc::now() + chrono::Duration::seconds(rules.timeout_per_round_secs as i64 * rules.max_rounds as i64),
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
        let mut negotiation = self.negotiations
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
        let negotiation = self.negotiations
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
            let accept_count = last_round.responses.iter()
                .filter(|r| r.response_type == ResponseType::Accept)
                .count();
            
            let total = negotiation.participants.len();
            let threshold = (total as f64 * negotiation.rules.voting_threshold) as usize;
            
            if accept_count >= threshold {
                negotiation.status = NegotiationPhase::Concluded { 
                    outcome: NegotiationOutcome::Agreement 
                };
            }
        }
    }

    /// Get negotiation status
    pub fn get_status(&self, negotiation_id: Uuid) -> Option<NegotiationPhase> {
        self.negotiations.get(&negotiation_id).map(|n| n.status.clone())
    }

    /// Calculate pareto-optimal solutions
    pub fn find_pareto_optimal(&self, negotiation_id: Uuid) -> Vec<Proposal> {
        let history = match self.history.get(&negotiation_id) {
            Some(h) => h,
            None => return vec![],
        };
        
        let all_proposals: Vec<Proposal> = history.iter()
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
                    other.utility_claims.get(agent).map_or(false, |other_utility| other_utility >= utility)
                }) && proposal.utility_claims.iter().any(|(agent, utility)| {
                    other.utility_claims.get(agent).map_or(false, |other_utility| other_utility > utility)
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
    pub fn allocate(&self, agent_id: Uuid, resource_id: &str, amount: f64, priority: AllocationPriority) -> Result<Allocation, String> {
        let mut resource = self.resources
            .get_mut(resource_id)
            .ok_or("Resource not found")?;
        
        // Check quota
        if let Some(mut quota) = self.quotas.get_mut(&agent_id) {
            if let Some(limit) = quota.limits.get(resource_id) {
                let used = quota.used.get(resource_id).copied().unwrap_or(0.0);
                if used + amount > *limit {
                    return Err(format!("Quota exceeded: {} + {} > {}", used, amount, limit));
                }
            }
        }
        
        // Check availability
        if resource.available < amount {
            return Err(format!("Insufficient resources: {} < {}", resource.available, amount));
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
        
        self.allocations.insert((agent_id, resource_id.to_string()), allocation.clone());
        
        // Update quota usage
        if let Some(mut quota) = self.quotas.get_mut(&agent_id) {
            *quota.used.entry(resource_id.to_string()).or_insert(0.0) += amount;
        }
        
        Ok(allocation)
    }

    /// Release allocation
    pub fn release(&self, agent_id: Uuid, resource_id: &str) -> Result<f64, String> {
        let allocation = self.allocations
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
        
        self.usage_history
            .entry(agent_id)
            .or_insert_with(Vec::new)
            .push(record);
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
        
        if let ResourceLocator::Semantic { concept, constraints } = loc {
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
    DelayedReward { value: f64, delay_steps: usize, attribution: Vec<(String, f64)> },
    /// Intrinsic motivation (curiosity, novelty)
    Intrinsic { kind: IntrinsicMotivation, intensity: f64 },
    /// Social learning from other agents
    Social { source_agent: Uuid, behavior: String, outcome: f64 },
    /// Counterfactual learning (what-if)
    Counterfactual { action_taken: String, alternative: String, estimated_diff: f64 },
}

/// Intrinsic motivation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntrinsicMotivation {
    Curiosity,          // Novelty-seeking
    Competence,         // Mastery-seeking
    Autonomy,           // Self-determination
    Relatedness,        // Social connection
    Progress,           // Improvement sensing
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
            if let Some(min_idx) = buffer.iter()
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
        let mut rng = rand::thread_rng();
        let samples: Vec<_> = buffer.iter()
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
                p.action_values.insert(entry.key().clone(), entry.value().clone());
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
        let current_value = policy.action_values
            .get(action)
            .map(|v| v.value)
            .unwrap_or(0.0);
        
        // Q-learning update: Q(s,a) = Q(s,a) + α * (r + γ * max(Q(s')) - Q(s,a))
        let max_next = policy.action_values.iter()
            .map(|e| e.value().value)
            .fold(0.0_f64, |a, b| a.max(b));
        
        let td_target = reward + policy.discount_factor * max_next;
        let delta = td_target - current_value;
        let new_value = current_value + policy.learning_rate * delta;
        
        // Update action value with running statistics
        let mut entry = policy.action_values.entry(action.clone()).or_insert(ActionValue {
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
            available_actions.iter()
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
        let total_value: f64 = self.policies.iter()
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
    pub learned_from: Option<Uuid>,  // Source agent
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
    StateEquals { key: String, value: serde_json::Value },
    HasCapability(String),
    ResourceAvailable { resource: String, amount: f64 },
    TimeConstraint { after: Option<DateTime<Utc>>, before: Option<DateTime<Utc>> },
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
    StateChange { key: String, value: serde_json::Value },
    ResourceConsumed { resource: String, amount: f64 },
    ResourceProduced { resource: String, amount: f64 },
    MessageSent { recipient: String },
    KnowledgeGained { topic: String },
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
        
        self.category_index
            .entry(category)
            .or_insert_with(Vec::new)
            .push(id);
        
        id
    }
    
    /// Get skill by ID
    pub fn get(&self, id: &Uuid) -> Option<Skill> {
        self.skills.get(id).map(|s| s.clone())
    }
    
    /// Get skill by name
    pub fn get_by_name(&self, name: &str) -> Option<Skill> {
        self.name_index.get(name)
            .and_then(|id| self.skills.get(&id).map(|s| s.clone()))
    }
    
    /// Find skills by category
    pub fn find_by_category(&self, category: &SkillCategory) -> Vec<Skill> {
        self.category_index.get(category)
            .map(|ids| ids.iter()
                .filter_map(|id| self.skills.get(id).map(|s| s.clone()))
                .collect())
            .unwrap_or_default()
    }
    
    /// Find applicable skills given current state
    pub fn find_applicable(&self, state: &HashMap<String, serde_json::Value>) -> Vec<Skill> {
        self.skills.iter()
            .filter(|entry| self.check_preconditions(&entry.preconditions, state))
            .map(|entry| entry.clone())
            .collect()
    }
    
    fn check_preconditions(&self, preconditions: &[SkillCondition], state: &HashMap<String, serde_json::Value>) -> bool {
        preconditions.iter().all(|cond| {
            match &cond.kind {
                ConditionKind::StateEquals { key, value } => {
                    state.get(key).map(|v| v == value).unwrap_or(false)
                }
                ConditionKind::HasCapability(cap) => {
                    state.get("capabilities")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().any(|c| c.as_str() == Some(cap)))
                        .unwrap_or(false)
                }
                _ => true
            }
        })
    }
    
    /// Transfer skill from one agent to another (with adaptation)
    pub fn transfer_skill(&self, skill_id: &Uuid, target_agent: Uuid) -> Option<Skill> {
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
            skill.success_rate = skill.success_rate * (1.0 - alpha) + 
                                 (if success { 1.0 } else { 0.0 }) * alpha;
        }
    }
    
    /// Get library statistics
    pub fn stats(&self) -> SkillLibraryStats {
        let by_category: HashMap<String, usize> = self.category_index.iter()
            .map(|e| (format!("{:?}", e.key()), e.value().len()))
            .collect();
        
        SkillLibraryStats {
            total_skills: self.skills.len(),
            by_category,
            average_success_rate: self.skills.iter()
                .map(|s| s.success_rate)
                .sum::<f64>() / self.skills.len().max(1) as f64,
            total_usages: self.skills.iter()
                .map(|s| s.usage_count)
                .sum(),
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
        let current = policy.action_values
            .get(action)
            .map(|v| v.value)
            .unwrap_or(0.0);
        
        let new_value = current + policy.learning_rate * (reward - current);
        
        policy.action_values.entry(action.clone()).or_insert(ActionValue {
            action: action.clone(),
            value: 0.0,
            count: 0,
            variance: 0.0,
            last_update: Utc::now(),
        }).value = new_value;
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
                .or_insert_with(Vec::new)
                .push(performance);
            
            // Compute meta-gradient (simplified)
            for (_i, grad) in gradients.iter_mut().enumerate() {
                *grad += (performance - 0.5) * 0.01; // Baseline subtraction
            }
        }
        
        // Update meta-parameters
        for (param, grad) in self.meta_parameters.iter_mut().zip(gradients.iter()) {
            *param += self.config.outer_learning_rate * grad / tasks.len() as f64;
        }
        
        // Update base policy's learning rate based on adaptation success
        let avg_performance: Vec<f64> = self.adaptation_performance.iter()
            .flat_map(|e| {
                let vals: Vec<f64> = e.value().iter().cloned().collect();
                vals
            })
            .collect();
        
        if !avg_performance.is_empty() {
            let avg = avg_performance.iter().sum::<f64>() / avg_performance.len() as f64;
            // Increase exploration if adapting poorly
            if avg < 0.5 {
                self.base_policy.exploration_rate = (self.base_policy.exploration_rate * 1.1).min(0.5);
            }
        }
    }
    
    fn evaluate_policy(&self, policy: &LearningPolicy, experiences: &[Experience]) -> f64 {
        if experiences.is_empty() {
            return 0.5;
        }
        
        let mut total_reward = 0.0;
        for exp in experiences {
            let predicted_value = policy.action_values
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
        let domain_performance: HashMap<String, f64> = self.adaptation_performance.iter()
            .map(|e| {
                let avg = e.value().iter().sum::<f64>() / e.value().len().max(1) as f64;
                (e.key().clone(), avg)
            })
            .collect();
        
        MetaLearningStats {
            tasks_learned: self.task_history.len(),
            domains: domain_performance.keys().cloned().collect(),
            domain_performance,
            meta_parameter_norm: self.meta_parameters.iter().map(|p| p * p).sum::<f64>().sqrt(),
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
    progress: DashMap<Uuid, AgentProgress>,  // Agent -> Progress
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
        self.progress.insert(agent_id, AgentProgress {
            agent_id,
            current_stage: first_stage,
            completed_stages: Vec::new(),
            stage_attempts: HashMap::new(),
            stage_scores: HashMap::new(),
            started_at: Utc::now(),
            last_activity: Utc::now(),
        });
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
                let prereqs_met = stage.prerequisites.iter()
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
        let stage_info = self.stages.get(stage_id).map(|s| (s.success_threshold, s.max_attempts));
        
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
        let completion_rates: HashMap<Uuid, f64> = self.stages.iter()
            .map(|entry| {
                let stage_id = *entry.key();
                let completed = self.progress.iter()
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
            average_progress: self.progress.iter()
                .map(|p| p.completed_stages.len() as f64 / self.stages.len().max(1) as f64)
                .sum::<f64>() / self.progress.len().max(1) as f64,
        }
    }
}

/// Result of curriculum stage attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CurriculumResult {
    StageCompleted { stage_id: Uuid, score: f64, next_stage: Option<Uuid> },
    StageFailed { stage_id: Uuid, attempts: usize, best_score: f64 },
    RetryNeeded { stage_id: Uuid, attempts_remaining: usize, current_score: f64 },
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
    NovelStrategy { problem_type: String, solution_path: Vec<String> },
    /// Self-organizing structure
    SelfOrganization { structure_type: String, participants: usize },
    /// Collective intelligence emergence
    CollectiveIntelligence { task: String, combined_performance: f64 },
    /// Role differentiation
    RoleDifferentiation { roles: Vec<String>, specialization_scores: HashMap<Uuid, f64> },
    /// Communication protocol emergence
    EmergentProtocol { signal_vocabulary: Vec<String>, effectiveness: f64 },
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
            time_windows.entry(window_key).or_insert_with(Vec::new).push(action);
        }
        
        // Find windows with multiple agents doing related actions
        for (_, actions) in &time_windows {
            if actions.len() >= 3 {
                let agents: Vec<Uuid> = actions.iter().map(|a| a.agent_id).collect();
                let unique_agents: std::collections::HashSet<_> = agents.iter().collect();
                
                if unique_agents.len() >= 2 {
                    let action_sequence: Vec<String> = actions.iter()
                        .map(|a| a.action.clone())
                        .collect();
                    
                    return Some(EmergentBehavior {
                        id: Uuid::new_v4(),
                        name: "Spontaneous Coordination".to_string(),
                        pattern: BehaviorPattern::SpontaneousCoordination { action_sequence },
                        involved_agents: unique_agents.into_iter().cloned().collect(),
                        first_observed: Utc::now(),
                        occurrence_count: 1,
                        beneficial: None,
                        description: "Multiple agents synchronized actions without explicit coordination".to_string(),
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
                .or_insert_with(HashMap::new)
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
                pattern: BehaviorPattern::RoleDifferentiation { roles, specialization_scores: specialization_scores.clone() },
                involved_agents: specialization_scores.keys().cloned().collect(),
                first_observed: Utc::now(),
                occurrence_count: 1,
                beneficial: Some(true),
                description: "Agents have naturally differentiated into specialized roles".to_string(),
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
        
        let high_outcome_actions: Vec<_> = recent.iter()
            .filter(|a| a.outcome.map(|o| o > 0.8).unwrap_or(false))
            .collect();
        
        if high_outcome_actions.len() >= 3 {
            let solution_path: Vec<String> = high_outcome_actions.iter()
                .map(|a| a.action.clone())
                .collect();
            
            return Some(EmergentBehavior {
                id: Uuid::new_v4(),
                name: "Novel Strategy".to_string(),
                pattern: BehaviorPattern::NovelStrategy { 
                    problem_type: "general".to_string(), 
                    solution_path 
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
            beneficial: behaviors.iter().filter(|b| b.beneficial == Some(true)).count(),
            harmful: behaviors.iter().filter(|b| b.beneficial == Some(false)).count(),
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
