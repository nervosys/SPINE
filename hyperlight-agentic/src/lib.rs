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
