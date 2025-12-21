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

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use petgraph::graph::{DiGraph, NodeIndex};
use sha2::{Sha256, Digest};
use tokio::sync::{mpsc, RwLock, broadcast};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustLevel {
    /// Unknown/untrusted agent
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
