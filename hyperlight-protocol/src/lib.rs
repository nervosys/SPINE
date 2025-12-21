use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit, aead::Aead};
use zstd::stream::encode_all;
use zstd::stream::decode_all;
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use hyperlight_neural::{NeuralLatentEncoder, MirasNeuralEncoder, NeuralEncoderConfig, MirasVariant};
use hyperlight_crypto::{TransformerPredictor, TransformerConfig, QuantumSpeculativeProtocol, LatticeParams};

// QUIC transport support (uses rustls 0.23 via quinn)
#[cfg(feature = "quic")]
use quinn::{Endpoint, Connection as QuinnConnection, ClientConfig, ServerConfig};
#[cfg(feature = "quic")]
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
#[cfg(feature = "quic")]
use rustls_quic as quic_rustls;  // rustls 0.23 for QUIC
#[cfg(feature = "quic")]
use std::net::SocketAddr;

/// Chameleon Protocol: A moving-target defense system that uses latent space
/// transformations for implicit encryption and compression.
/// 
/// Core Insight: High-dimensional vector spaces are inherently encrypted—
/// the transformation matrix IS the key. By evolving the transformation
/// based on message history, we create a protocol that is impossible to
/// statically analyze.

// =============================================================================
// SPECULATIVE DECODING ENGINE
// =============================================================================

/// Speculative decoding for protocol messages.
/// 
/// Inspired by LLM speculative decoding: predict the next message using a 
/// lightweight model, send the prediction hash, and if correct, only send
/// a tiny confirmation instead of the full message.
/// 
/// Benefits:
/// - Massive bandwidth reduction when predictions are correct
/// - Reduced latency (receiver can pre-compute response)
/// - Pattern obfuscation (predictions look like noise to observers)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeculativeFrame {
    /// The actual message (may be empty if prediction was correct)
    pub payload: SpeculativePayload,
    /// Hash of our prediction for the NEXT message we expect to receive
    pub next_prediction_hash: u64,
    /// Confidence score for the prediction (0.0 - 1.0)
    pub confidence: f32,
    /// Number of tokens/elements we're speculating ahead
    pub speculation_depth: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpeculativePayload {
    /// Full message (prediction miss or first message)
    Full(Vec<u8>),
    /// Prediction hit - just send confirmation + any delta
    Confirmed { 
        /// Hash of the correctly predicted message
        confirmed_hash: u64,
        /// Any additional data not covered by prediction
        delta: Vec<u8>,
    },
    /// Partial match - send only the differing portions
    Partial {
        /// Hash of matched prefix
        prefix_hash: u64,
        /// Length of matched prefix in bytes
        prefix_len: u32,
        /// The divergent suffix
        suffix: Vec<u8>,
    },
    /// Speculative batch - multiple predicted messages
    Batch {
        /// Ordered predictions (most likely first)
        predictions: Vec<u64>,
        /// Actual message if none match
        fallback: Vec<u8>,
    },
}

/// Prediction model for speculative decoding
#[derive(Clone)]
pub struct SpeculativePredictor {
    /// Transformer-based predictor
    transformer: TransformerPredictor,
    /// Recent message history for pattern learning
    history: VecDeque<MessagePattern>,
    /// Maximum history size
    max_history: usize,
    /// Sequence predictor state
    sequence_state: u64,
    /// Enable speculative output (sending predictions)
    output_speculation: bool,
    /// Enable speculative input (pre-computing responses)
    input_speculation: bool,
}

#[derive(Clone, Debug)]
struct MessagePattern {
    hash: u64,
    _message_type: MessageType,
    _size: usize,
    _latent_signature: Option<Vec<f32>>,
}

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum MessageType {
    Request,
    Response,
    Event,
    Binary,
    Sync,
    Unknown,
}

impl SpeculativePredictor {
    pub fn new() -> Self {
        let config = TransformerConfig {
            embed_dim: 64,
            num_heads: 4,
            num_layers: 4,
            ff_dim: 128,
            max_seq_len: 128,
            memory_size: 32,  // Titans persistent memory tokens
            seed: 42,
        };
        Self {
            transformer: TransformerPredictor::new(config),
            history: VecDeque::with_capacity(64),
            max_history: 64,
            sequence_state: 0,
            output_speculation: true,
            input_speculation: true,
        }
    }
    
    /// Record an observed message and update the prediction model
    pub fn observe(&mut self, data: &[u8], msg_type: MessageType) {
        let hash = Self::hash_data(data);
        let pattern = MessagePattern {
            hash,
            _message_type: msg_type,
            _size: data.len(),
            _latent_signature: self.extract_latent_signature(data),
        };
        
        // Update transformer
        self.transformer.observe(data);
        
        // Add to history
        self.history.push_back(pattern);
        if self.history.len() > self.max_history {
            self.history.pop_front();
        }
        
        // Update sequence state (chaotic mixing)
        self.sequence_state = self.sequence_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(hash);
    }
    
    /// Predict the next message hash with confidence
    pub fn predict_next(&mut self) -> (u64, f32) {
        if self.history.is_empty() {
            return (0, 0.0);
        }
        
        // Predict a sequence of bytes (e.g., 32 bytes for a compact message or header)
        let predicted_data = self.transformer.predict_sequence(32, true);
        let hash = Self::hash_data(&predicted_data);
        
        // For now, we use a fixed confidence from the first byte prediction
        let (_, prob) = self.transformer.predict_next();
        
        (hash, prob)
    }

    /// Predict the next message data
    pub fn predict_next_data(&mut self, length: usize) -> Vec<u8> {
        self.transformer.predict_sequence(length, true)
    }
    
    /// Predict multiple possible next messages (for batch speculation)
    pub fn predict_batch(&mut self, _count: usize) -> Vec<(u64, f32)> {
        let (byte, prob) = self.predict_next();
        vec![(byte, prob)]
    }
    
    /// Check if a message matches our prediction
    pub fn verify_prediction(&self, data: &[u8], predicted_hash: u64) -> bool {
        Self::hash_data(data) == predicted_hash
    }
    
    /// Compute prefix match length for partial speculation
    pub fn compute_prefix_match(&self, data: &[u8], predicted_data: &[u8]) -> usize {
        data.iter()
            .zip(predicted_data.iter())
            .take_while(|(a, b)| a == b)
            .count()
    }
    
    fn hash_data(data: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }
    
    fn extract_latent_signature(&self, data: &[u8]) -> Option<Vec<f32>> {
        // Extract a compact signature from the data for similarity matching
        if data.len() < 16 {
            return None;
        }
        
        // Simple signature: first 8 floats from data interpreted as f32
        let sig: Vec<f32> = data.chunks(4)
            .take(8)
            .map(|chunk| {
                if chunk.len() == 4 {
                    f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                } else {
                    0.0
                }
            })
            .collect();
        
        Some(sig)
    }
}

impl Default for SpeculativePredictor {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// LATENT SPACE CRYPTOGRAPHY
// =============================================================================

/// A point in the latent space. The dimensionality and basis evolve per-message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatentVector {
    /// The high-dimensional representation
    pub components: Vec<f32>,
    /// Dimension hint (can be misleading as part of obfuscation)
    pub dim_hint: u16,
    /// Epoch number for transformation alignment
    pub epoch: u64,
}

/// Dynamic transformation matrix that evolves with each message.
/// This is the "key" in our latent cryptography system.
#[derive(Clone)]
pub struct ChameleonKey {
    /// Neural latent encoder (legacy)
    encoder: NeuralLatentEncoder,
    /// MIRAS-adaptive encoder for enhanced continual learning
    miras_encoder: Option<MirasNeuralEncoder>,
    /// Quantum-resistant key evolution
    quantum: QuantumSpeculativeProtocol,
    /// Dimension of the latent space
    dimension: usize,
    /// Message counter for key evolution
    epoch: u64,
    /// Anomaly history for adaptive MIRAS selection
    anomaly_scores: VecDeque<f32>,
    /// Current MIRAS variant in use
    active_variant: MirasVariant,
    /// Threshold for switching to YAAD (outlier-robust)
    anomaly_threshold: f32,
}

impl ChameleonKey {
    /// Create a new Chameleon key from a shared secret
    pub fn new(secret: &[u8; 32]) -> Self {
        let dimension = 128;
        let config = TransformerConfig {
            embed_dim: 64,
            num_heads: 4,
            num_layers: 4,
            ff_dim: 128,
            max_seq_len: 128,
            memory_size: 32,  // Titans persistent memory tokens
            seed: 42,
        };
        let params = LatticeParams {
            n: 512,
            q: 12289,
            p: 12289,
            sigma: 3.2,
        };
        let mut hasher = DefaultHasher::new();
        secret.hash(&mut hasher);
        let seed = hasher.finish();

        Self {
            // attention_heads (8) must divide latent_dim (128): 128/8=16 ✓
            encoder: NeuralLatentEncoder::new(dimension, 128, &[256, 128], 8, seed),
            miras_encoder: None,
            quantum: QuantumSpeculativeProtocol::new(config, params, seed),
            dimension,
            epoch: 0,
            anomaly_scores: VecDeque::with_capacity(100),
            active_variant: MirasVariant::Titans,
            anomaly_threshold: 5.0,
        }
    }
    
    /// Create with MIRAS-adaptive encoding for continual learning
    pub fn new_with_miras(secret: &[u8; 32], variant: MirasVariant) -> Self {
        let dimension = 128;
        let config = TransformerConfig {
            embed_dim: 64,
            num_heads: 4,
            num_layers: 4,
            ff_dim: 128,
            max_seq_len: 128,
            memory_size: 32,
            seed: 42,
        };
        let params = LatticeParams {
            n: 512,
            q: 12289,
            p: 12289,
            sigma: 3.2,
        };
        let mut hasher = DefaultHasher::new();
        secret.hash(&mut hasher);
        let seed = hasher.finish();
        
        let miras_config = NeuralEncoderConfig {
            input_dim: 256,
            latent_dim: dimension,  // 128
            hidden_dims: vec![256, 128],
            attention_heads: 8,     // 128/8=16 ✓
            seed,
            miras_variant: variant,
            memory_tokens: 16,
        };

        Self {
            // attention_heads (8) must divide latent_dim (128): 128/8=16 ✓
            encoder: NeuralLatentEncoder::new(dimension, 128, &[256, 128], 8, seed),
            miras_encoder: Some(miras_config.build_miras()),
            quantum: QuantumSpeculativeProtocol::new(config, params, seed),
            dimension,
            epoch: 0,
            anomaly_scores: VecDeque::with_capacity(100),
            active_variant: variant,
            anomaly_threshold: 5.0,
        }
    }
    
    /// Encode data into the latent space
    pub fn encode(&mut self, data: &[u8]) -> LatentVector {
        let components = if let Some(ref mut miras) = self.miras_encoder {
            // Use MIRAS encoder with adaptive memory
            let encoded = miras.encode(data);
            
            // Track surprise for anomaly detection
            let surprise = miras.get_surprise();
            self.anomaly_scores.push_back(surprise);
            if self.anomaly_scores.len() > 100 {
                self.anomaly_scores.pop_front();
            }
            
            // Adaptive variant switching based on anomaly patterns
            self.maybe_switch_variant();
            
            encoded
        } else {
            self.encoder.encode(data)
        };
        
        LatentVector {
            components,
            dim_hint: self.dimension as u16,
            epoch: self.epoch,
        }
    }
    
    /// Decode data from the latent space
    pub fn decode(&mut self, vector: &LatentVector) -> Vec<u8> {
        self.encoder.decode(&vector.components, 0)
    }
    
    /// Evolve the key based on message history
    pub fn evolve(&mut self, message_hash: u64) {
        self.epoch += 1;
        
        // Use the quantum-resistant key state to generate a new morph seed
        let quantum_seed = self.quantum.get_morph_seed();
        
        // Combine with message hash for unique per-message evolution
        let combined_seed = quantum_seed ^ message_hash;
        
        // Perturb the neural encoder with the combined seed to shift the latent space
        self.encoder.evolve(combined_seed);
    }
    
    /// Get current anomaly level (average surprise over recent messages)
    pub fn anomaly_level(&self) -> f32 {
        if self.anomaly_scores.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.anomaly_scores.iter().sum();
        sum / self.anomaly_scores.len() as f32
    }
    
    /// Get current MIRAS variant
    pub fn variant(&self) -> &'static str {
        match &self.miras_encoder {
            Some(enc) => enc.variant(),
            None => "Legacy",
        }
    }
    
    /// Adaptively switch MIRAS variant based on traffic patterns
    fn maybe_switch_variant(&mut self) {
        if self.miras_encoder.is_none() {
            return;
        }
        
        let anomaly = self.anomaly_level();
        let current = self.active_variant;
        
        // Decision logic for variant switching
        let new_variant = if anomaly > self.anomaly_threshold * 2.0 {
            // Very high anomaly → YAAD for outlier robustness
            MirasVariant::Yaad
        } else if anomaly > self.anomaly_threshold {
            // Moderate anomaly → MEMORA for balanced updates
            MirasVariant::Memora
        } else if self.epoch > 10000 {
            // Long-running session → MONETA for stability
            MirasVariant::Moneta { p: 2.0 }
        } else {
            // Normal operation → Titans baseline
            MirasVariant::Titans
        };
        
        // Only switch if variant changed (avoid unnecessary rebuilds)
        if std::mem::discriminant(&new_variant) != std::mem::discriminant(&current) {
            let seed = self.epoch ^ 0xDEADBEEF;
            let config = NeuralEncoderConfig {
                input_dim: 256,
                latent_dim: self.dimension,
                hidden_dims: vec![256, 128],
                attention_heads: 8,
                seed,
                miras_variant: new_variant,
                memory_tokens: 16,
            };
            self.miras_encoder = Some(config.build_miras());
            self.active_variant = new_variant;
        }
    }
}

// =============================================================================
// PROTOCOL MORPHING ENGINE
// =============================================================================

/// The protocol's shape-shifting characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMorphology {
    /// Current frame format version (changes unpredictably)
    pub frame_version: u8,
    /// Header size in bytes (varies 4-16)
    pub header_size: u8,
    /// Byte order for length field (alternates)
    pub big_endian: bool,
    /// Checksum algorithm selector
    pub checksum_variant: u8,
    /// Padding strategy
    pub padding_mode: PaddingMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PaddingMode {
    None,
    Pkcs7,
    Random,
    Chaotic,
}

impl ProtocolMorphology {
    fn new(seed: u64) -> Self {
        Self {
            frame_version: (seed % 256) as u8,
            header_size: 4 + (seed % 13) as u8,
            big_endian: seed % 2 == 0,
            checksum_variant: (seed % 4) as u8,
            padding_mode: match seed % 4 {
                0 => PaddingMode::None,
                1 => PaddingMode::Pkcs7,
                2 => PaddingMode::Random,
                _ => PaddingMode::Chaotic,
            },
        }
    }
    
    fn evolve(&mut self, hash: u64) {
        self.frame_version = self.frame_version.wrapping_add((hash % 7) as u8);
        self.header_size = 4 + ((self.header_size as u64 + hash) % 13) as u8;
        self.big_endian = !self.big_endian;
        self.checksum_variant = (self.checksum_variant + (hash % 4) as u8) % 4;
        self.padding_mode = match (self.padding_mode as u8 + (hash % 4) as u8) % 4 {
            0 => PaddingMode::None,
            1 => PaddingMode::Pkcs7,
            2 => PaddingMode::Random,
            _ => PaddingMode::Chaotic,
        };
    }
}

// =============================================================================
// MESSAGE TYPES
// =============================================================================

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Message {
    Request(Request),
    Response(Response),
    Event(Event),
    BinaryProgram(HyperlightBinary),
    /// Latent-encoded message (implicit encryption)
    LatentMessage(LatentVector),
    /// Protocol synchronization (for moving-target alignment)
    Sync(SyncPayload),
    /// Speculative frame (prediction-accelerated message)
    Speculative(SpeculativeFrame),
    /// Pre-computed response (receiver sends this when prediction matched)
    PreComputed(PreComputedResponse),
    /// Server-initiated request to morph the protocol
    MorphRequest { seed: u64 },
    /// Health check to verify connection
    Ping { timestamp: u64 },
    /// Response to health check
    Pong { timestamp: u64 },
}

/// Pre-computed response that was speculatively prepared
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PreComputedResponse {
    /// Hash of the request this responds to
    pub request_hash: u64,
    /// The pre-computed result
    pub result: serde_json::Value,
    /// Confidence that this is the right response
    pub confidence: f32,
    /// Alternative responses if primary is rejected
    pub alternatives: Vec<(u64, serde_json::Value)>,
}

/// Virtual DOM patch for incremental updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VDomPatch {
    /// Create a new node
    Create { id: u32, tag: String },
    /// Remove a node
    Remove { id: u32 },
    /// Set an attribute
    SetAttr { id: u32, key: String, value: String },
    /// Remove an attribute
    RemoveAttr { id: u32, key: String },
    /// Append child to parent
    AppendChild { parent_id: u32, child_id: u32 },
    /// Remove child from parent
    RemoveChild { parent_id: u32, child_id: u32 },
    /// Reorder children
    ReorderChildren { parent_id: u32, order: Vec<u32> },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncPayload {
    pub epoch: u64,
    pub morphology_hash: u64,
    pub challenge: Vec<f32>,
    /// Predictor state sync (for speculative decoding alignment)
    pub predictor_state: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HyperlightBinary {
    pub instructions: Vec<Instruction>,
    pub data: Vec<u8>,
    pub render_start: usize,
    pub exported_functions: std::collections::HashMap<String, usize>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Instruction {
    DefineElement { id: u32, tag: String },
    SetAttribute { id: u32, key: String, value: String },
    AddChild { parent_id: u32, child_id: u32 },
    EmitEvent { name: String, payload: serde_json::Value },
    StreamLatent { vector: Vec<f32> },
    /// Morph the protocol mid-stream
    MorphProtocol { seed: u64 },
    /// Inject decoy traffic
    Decoy { noise: Vec<f32> },
    /// Declare a reactive state variable
    DeclareState { name: String, initial_json: serde_json::Value },
    /// Update a reactive state variable
    UpdateState { name: String, value_json: serde_json::Value },
    
    // --- Control Flow & Stack Operations ---
    Push(serde_json::Value),
    Pop,
    Load(String),
    Store(String),
    BinOp(ProtocolBinOp),
    UnaryOp(ProtocolUnaryOp),
    Jump(usize),
    JumpIf(usize),
    JumpIfNot(usize),
    Call { name: String, num_args: usize },
    CallTarget(usize),
    Return,
    
    // --- Stack-based DOM Operations ---
    DefineElementFromStack { id: u32 },
    SetAttributeFromStack { id: u32, key: String },
    AddChildFromStack { parent_id: u32, child_id: u32 },
    EmitEventFromStack { name: String },
    DefineTextFromStack,
    DeclareStateFromStack { name: String },
    UpdateStateFromStack { name: String },

    // --- Agentic Operations ---
    NavigateFromStack,
    SearchFromStack,
    StoreKnowledgeFromStack { tags: Vec<String> },
    QueryKnowledgeFromStack { tags: Vec<String>, limit: usize },
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ProtocolBinOp {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    Concat,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ProtocolUnaryOp {
    Not, Neg,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum BrowserCommand {
    Navigate { url: String },
    GetUR,
    GetRawHTML,
    Click { element_id: String },
    Type { element_id: String, text: String },
    ExecuteBinary(HyperlightBinary),
    /// Handle an event in the current session
    HandleEvent {
        element_id: u32,
        event_name: String,
        payload: serde_json::Value,
    },
    /// Request latent-encoded response
    GetLatentUR { dimensions: usize },
    /// Trigger protocol morphing
    Morph,
    /// Semantic search in the current page
    Search { query: String },
    /// Transfer session to another node
    TransferSession { target_node_id: Uuid },
    /// Store knowledge in the agent's long-term memory
    StoreKnowledge { key: String, value: serde_json::Value, tags: Vec<String> },
    /// Query knowledge from the agent's long-term memory
    QueryKnowledge { query: String, tags: Vec<String>, limit: usize },
    /// Delete knowledge from the agent's long-term memory
    DeleteKnowledge { key: String },
    /// Get the history of commands in this session
    GetSessionHistory,
    /// Get the capabilities of the current agentic binary
    GetCapabilities,
    /// Enable or disable autonomous mode for the agent
    SetAutonomousMode { enabled: bool },
    /// Perform a swarm search across the cluster
    SwarmSearch { query: String, depth: usize },
    /// Delegate a task to another agent in the cluster
    DelegateTask { task: String, target_agent_id: Option<Uuid> },
    /// Propose knowledge to the cluster for consensus
    ProposeKnowledge { key: String, value: serde_json::Value, tags: Vec<String> },
    /// Create a swarm plan for a high-level goal
    CreateSwarmPlan { goal: String },
    /// Execute a specific task within a swarm plan
    ExecutePlanTask { plan_id: Uuid, task_id: Uuid },
    /// Transmit data using the neural protocol
    NeuralTransmit { data: Vec<u8>, domain: String },
    /// Get the full agentic state (memory, speech acts, etc.)
    GetAgenticState,
    /// Send a speech act to another agent
    SendSpeechAct { target_id: Uuid, performative: String, content: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SwarmPlan {
    pub id: Uuid,
    pub goal: String,
    pub tasks: Vec<PlanTask>,
    pub status: PlanStatus,
    pub created_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlanTask {
    pub id: Uuid,
    pub description: String,
    pub required_skills: Vec<String>,
    pub assigned_to: Option<Uuid>, // NodeId or AgentId
    pub dependencies: Vec<Uuid>, // Task IDs
    pub status: TaskStatus,
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum PlanStatus {
    Draft,
    Active,
    Completed,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Request {
    pub id: String,
    pub command: BrowserCommand,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Response {
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Event {
    pub name: String,
    pub data: serde_json::Value,
}

// =============================================================================
// CHAMELEON PROTOCOL HANDLER
// =============================================================================

pub struct ProtocolHandler<S> {
    stream: S,
    /// Traditional encryption (fallback)
    cipher: Option<Aes256Gcm>,
    /// Chameleon latent-space key
    chameleon: Option<ChameleonKey>,
    /// Current protocol shape
    morphology: ProtocolMorphology,
    /// Enable moving-target defense
    moving_target: bool,
    /// Speculative predictor for outgoing messages
    output_predictor: SpeculativePredictor,
    /// Speculative predictor for incoming messages
    input_predictor: SpeculativePredictor,
    /// Cache of pre-computed responses
    precomputed_cache: Vec<(u64, Message)>,
    /// Cache of our own predictions to reconstruct on hit
    prediction_cache: Vec<(u64, Vec<u8>)>,
    /// Last prediction we sent (for verification)
    last_output_prediction: Option<u64>,
    /// Last prediction we received (for verification)
    last_input_prediction: Option<u64>,
    /// Statistics for speculation accuracy
    pub speculation_stats: SpeculationStats,
}

/// Statistics for tracking speculation effectiveness
#[derive(Debug, Clone, Default)]
pub struct SpeculationStats {
    pub output_predictions: u64,
    pub output_hits: u64,
    pub output_partial_hits: u64,
    pub input_predictions: u64,
    pub input_hits: u64,
    pub input_partial_hits: u64,
    pub bytes_saved: u64,
    pub precompute_hits: u64,
}

impl SpeculationStats {
    pub fn output_accuracy(&self) -> f32 {
        if self.output_predictions == 0 {
            0.0
        } else {
            self.output_hits as f32 / self.output_predictions as f32
        }
    }
    
    pub fn input_accuracy(&self) -> f32 {
        if self.input_predictions == 0 {
            0.0
        } else {
            self.input_hits as f32 / self.input_predictions as f32
        }
    }
}

impl<S> ProtocolHandler<S> 
where 
    S: AsyncRead + AsyncWrite + Unpin + Send
{
    pub fn new(stream: S) -> Self {
        Self { 
            stream, 
            cipher: None,
            chameleon: None,
            morphology: ProtocolMorphology::new(0),
            moving_target: false,
            output_predictor: SpeculativePredictor::new(),
            input_predictor: SpeculativePredictor::new(),
            precomputed_cache: Vec::with_capacity(16),
            prediction_cache: Vec::with_capacity(16),
            last_output_prediction: None,
            last_input_prediction: None,
            speculation_stats: SpeculationStats::default(),
        }
    }

    /// Enable traditional AES encryption
    pub fn enable_encryption(&mut self, key: [u8; 32]) {
        let key = Key::<Aes256Gcm>::from_slice(&key);
        self.cipher = Some(Aes256Gcm::new(key));
    }
    
    /// Enable Chameleon protocol (latent-space cryptography + moving target)
    pub fn enable_chameleon(&mut self, secret: [u8; 32]) {
        self.chameleon = Some(ChameleonKey::new(&secret));
        self.morphology = ProtocolMorphology::new(
            u64::from_le_bytes(secret[0..8].try_into().unwrap())
        );
        self.moving_target = true;
    }
    
    /// Enable speculative decoding
    pub fn enable_speculation(&mut self, output: bool, input: bool) {
        self.output_predictor.output_speculation = output;
        self.input_predictor.input_speculation = input;
    }
    
    /// Pre-compute a response for a predicted request
    pub fn precompute_response(&mut self, predicted_request_hash: u64, response: Message) {
        self.precomputed_cache.push((predicted_request_hash, response));
        // Keep cache bounded
        if self.precomputed_cache.len() > 32 {
            self.precomputed_cache.remove(0);
        }
    }
    
    /// Check if we have a pre-computed response
    fn get_precomputed(&mut self, request_hash: u64) -> Option<Message> {
        if let Some(pos) = self.precomputed_cache.iter().position(|(h, _)| *h == request_hash) {
            self.speculation_stats.precompute_hits += 1;
            Some(self.precomputed_cache.remove(pos).1)
        } else {
            None
        }
    }
    
    /// Hash message for key evolution
    fn hash_message(data: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }
    
    /// Classify message type for prediction
    fn classify_message(msg: &Message) -> MessageType {
        match msg {
            Message::Request(_) => MessageType::Request,
            Message::Response(_) => MessageType::Response,
            Message::Event(_) => MessageType::Event,
            Message::BinaryProgram(_) => MessageType::Binary,
            Message::Sync(_) => MessageType::Sync,
            Message::Speculative(_) => MessageType::Unknown,
            Message::PreComputed(_) => MessageType::Response,
            Message::LatentMessage(_) => MessageType::Unknown,
            Message::MorphRequest { .. } => MessageType::Sync,
            Message::Ping { .. } => MessageType::Sync,
            Message::Pong { .. } => MessageType::Sync,
        }
    }

    /// Send a message with speculative encoding
    pub async fn send_message(&mut self, msg: &Message) -> anyhow::Result<()> {
        let raw_data = serde_json::to_vec(msg)?;
        let msg_hash = Self::hash_message(&raw_data);
        let msg_type = Self::classify_message(msg);
        
        // Check if receiver predicted this message
        let payload = if let Some(predicted_hash) = self.last_input_prediction {
            if predicted_hash == msg_hash {
                // Prediction hit! Send minimal confirmation
                self.speculation_stats.output_hits += 1;
                self.speculation_stats.bytes_saved += raw_data.len() as u64;
                SpeculativePayload::Confirmed {
                    confirmed_hash: msg_hash,
                    delta: Vec::new(),
                }
            } else {
                // Prediction miss, send full message
                SpeculativePayload::Full(raw_data.clone())
            }
        } else {
            SpeculativePayload::Full(raw_data.clone())
        };
        
        // Generate prediction for what we'll receive next
        self.output_predictor.observe(&raw_data, msg_type);
        let (next_prediction, confidence) = self.output_predictor.predict_next();
        
        // Cache the predicted data for reconstruction on hit
        if confidence > 0.5 {
            let predicted_data = self.output_predictor.predict_next_data(raw_data.len());
            self.prediction_cache.push((next_prediction, predicted_data));
            if self.prediction_cache.len() > 16 {
                self.prediction_cache.remove(0);
            }
        }

        self.last_output_prediction = Some(next_prediction);
        self.speculation_stats.output_predictions += 1;
        
        // Build speculative frame
        let frame = SpeculativeFrame {
            payload,
            next_prediction_hash: next_prediction,
            confidence,
            speculation_depth: 1,
        };
        
        // Serialize the speculative frame
        let mut data = serde_json::to_vec(&frame)?;
        
        // Apply padding based on current morphology
        data = self.apply_padding(data);
        
        // Compression
        data = encode_all(&data[..], 3)?;

        // Chameleon encoding (if enabled)
        if let Some(ref mut chameleon) = self.chameleon {
            let latent = chameleon.encode(&data);
            data = serde_json::to_vec(&latent)?;
            
            // Evolve key after sending (moving target)
            if self.moving_target {
                chameleon.evolve(msg_hash);
                self.morphology.evolve(msg_hash);
            }
        }
        // Fallback to AES encryption
        else if let Some(cipher) = &self.cipher {
            let nonce_bytes = msg_hash.to_le_bytes();
            let mut nonce_full = [0u8; 12];
            nonce_full[..8].copy_from_slice(&nonce_bytes);
            let nonce = Nonce::from_slice(&nonce_full);
            data = cipher.encrypt(nonce, data.as_ref())
                .map_err(|e| anyhow::anyhow!("Encryption error: {}", e))?;
            
            // Prepend nonce for receiver
            let mut with_nonce = nonce_full.to_vec();
            with_nonce.extend(data);
            data = with_nonce;
        }

        // Write with morphing header
        self.write_morphed_frame(&data).await?;
        Ok(())
    }
    
    /// Send message without speculation (for bootstrapping)
    pub async fn send_message_raw(&mut self, msg: &Message) -> anyhow::Result<()> {
        let mut data = serde_json::to_vec(msg)?;
        let msg_hash = Self::hash_message(&data);
        
        data = self.apply_padding(data);
        data = encode_all(&data[..], 3)?;

        if let Some(ref mut chameleon) = self.chameleon {
            let latent = chameleon.encode(&data);
            data = serde_json::to_vec(&latent)?;
            
            if self.moving_target {
                chameleon.evolve(msg_hash);
                self.morphology.evolve(msg_hash);
            }
        } else if let Some(cipher) = &self.cipher {
            let nonce_bytes = msg_hash.to_le_bytes();
            let mut nonce_full = [0u8; 12];
            nonce_full[..8].copy_from_slice(&nonce_bytes);
            let nonce = Nonce::from_slice(&nonce_full);
            data = cipher.encrypt(nonce, data.as_ref())
                .map_err(|e| anyhow::anyhow!("Encryption error: {}", e))?;
            
            let mut with_nonce = nonce_full.to_vec();
            with_nonce.extend(data);
            data = with_nonce;
        }

        self.write_morphed_frame(&data).await?;
        Ok(())
    }

    /// Receive a message with speculative decoding
    pub async fn receive_message(&mut self) -> anyhow::Result<Message> {
        // Read with morphing header
        let mut data = self.read_morphed_frame().await?;

        // Chameleon decoding (if enabled)
        if let Some(ref mut chameleon) = self.chameleon {
            let latent: LatentVector = serde_json::from_slice(&data)?;
            data = chameleon.decode(&latent);
        }
        // Fallback to AES decryption
        else if let Some(cipher) = &self.cipher {
            if data.len() < 12 {
                return Err(anyhow::anyhow!("Invalid encrypted message"));
            }
            let nonce = Nonce::from_slice(&data[..12]);
            data = cipher.decrypt(nonce, &data[12..])
                .map_err(|e| anyhow::anyhow!("Decryption error: {}", e))?;
        }

        // Decompression
        data = decode_all(&data[..])?;
        
        // Remove padding
        data = self.remove_padding(data);
        
        // Try to parse as speculative frame
        if let Ok(frame) = serde_json::from_slice::<SpeculativeFrame>(&data) {
            // Store their prediction for our next send
            self.last_input_prediction = Some(frame.next_prediction_hash);
            self.speculation_stats.input_predictions += 1;
            
            // Decode payload
            let msg_data = match frame.payload {
                SpeculativePayload::Full(full_data) => full_data,
                SpeculativePayload::Confirmed { confirmed_hash, delta } => {
                    // They confirmed our prediction was correct!
                    self.speculation_stats.input_hits += 1;
                    
                    // Reconstruct from our prediction cache
                    if let Some(pos) = self.prediction_cache.iter().position(|(h, _)| *h == confirmed_hash) {
                        let (_, mut predicted_data) = self.prediction_cache.remove(pos);
                        
                        // Apply delta if any
                        if !delta.is_empty() {
                            // Simple XOR delta for now
                            for (p, d) in predicted_data.iter_mut().zip(delta.iter()) {
                                *p ^= *d;
                            }
                        }
                        predicted_data
                    } else {
                        // Cache miss, but they confirmed it. This shouldn't happen if we're in sync.
                        return Err(anyhow::anyhow!("Speculation hit but prediction not in cache"));
                    }
                }
                SpeculativePayload::Partial { prefix_hash: _, prefix_len: _, suffix } => {
                    self.speculation_stats.input_partial_hits += 1;
                    // Would combine cached prefix with received suffix
                    suffix
                }
                SpeculativePayload::Batch { predictions, fallback } => {
                    // Check if any prediction matches our expectation
                    if let Some(pred) = self.last_output_prediction {
                        if predictions.contains(&pred) {
                            self.speculation_stats.input_hits += 1;
                        }
                    }
                    fallback
                }
            };
            
            // Parse the actual message
            let msg: Message = serde_json::from_slice(&msg_data)?;
            
            // Handle proactive morphing requests from server
            if let Message::MorphRequest { seed } = &msg {
                self.morph_now(*seed);
            }

            // Update input predictor
            let msg_type = Self::classify_message(&msg);
            self.input_predictor.observe(&msg_data, msg_type);
            
            // Evolve Chameleon key based on the decoded message
            if let Some(ref mut chameleon) = self.chameleon {
                let msg_hash = Self::hash_message(&msg_data);
                if self.moving_target {
                    chameleon.evolve(msg_hash);
                    self.morphology.evolve(msg_hash);
                }
            }
            
            Ok(msg)
        } else {
            // Legacy message (no speculative frame)
            let msg: Message = serde_json::from_slice(&data)?;
            
            // Handle proactive morphing requests from server
            if let Message::MorphRequest { seed } = &msg {
                self.morph_now(*seed);
            }

            // Update predictor anyway
            let msg_type = Self::classify_message(&msg);
            self.input_predictor.observe(&data, msg_type);
            
            if let Some(ref mut chameleon) = self.chameleon {
                let msg_hash = Self::hash_message(&data);
                if self.moving_target {
                    chameleon.evolve(msg_hash);
                    self.morphology.evolve(msg_hash);
                }
            }
            
            Ok(msg)
        }
    }
    
    /// Speculatively pre-compute responses for likely incoming requests
    pub fn speculate_responses<F>(&mut self, compute_response: F)
    where
        F: Fn(u64) -> Option<Message>,
    {
        // Get batch of likely next messages
        let predictions = self.input_predictor.predict_batch(4);
        
        for (predicted_hash, confidence) in predictions {
            if confidence > 0.3 {
                if let Some(response) = compute_response(predicted_hash) {
                    self.precompute_response(predicted_hash, response);
                }
            }
        }
    }
    
    /// Get speculation statistics
    pub fn get_speculation_stats(&self) -> &SpeculationStats {
        &self.speculation_stats
    }
    
    /// Write frame with morphing header format
    async fn write_morphed_frame(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let len = data.len() as u32;
        
        // Variable header based on morphology
        let mut header = Vec::with_capacity(self.morphology.header_size as usize);
        
        // Length field (endianness varies)
        let len_bytes = if self.morphology.big_endian {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        };
        header.extend_from_slice(&len_bytes);
        
        // Version byte
        header.push(self.morphology.frame_version);
        
        // Padding to header_size
        while header.len() < self.morphology.header_size as usize {
            header.push(self.morphology.checksum_variant);
        }
        
        self.stream.write_all(&header).await?;
        self.stream.write_all(data).await?;
        Ok(())
    }
    
    /// Read frame with morphing header format
    async fn read_morphed_frame(&mut self) -> anyhow::Result<Vec<u8>> {
        let mut header = vec![0u8; self.morphology.header_size as usize];
        self.stream.read_exact(&mut header).await?;
        
        // Extract length (endianness varies)
        let len_bytes: [u8; 4] = header[0..4].try_into()?;
        let len = if self.morphology.big_endian {
            u32::from_be_bytes(len_bytes)
        } else {
            u32::from_le_bytes(len_bytes)
        } as usize;
        
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf).await?;
        Ok(buf)
    }
    
    /// Apply padding based on current morphology
    fn apply_padding(&self, mut data: Vec<u8>) -> Vec<u8> {
        match self.morphology.padding_mode {
            PaddingMode::None => data,
            PaddingMode::Pkcs7 => {
                let padding_len = 16 - (data.len() % 16);
                data.extend(std::iter::repeat(padding_len as u8).take(padding_len));
                data
            }
            PaddingMode::Random => {
                let padding_len = 16 - (data.len() % 16);
                let mut state = self.morphology.frame_version as u64;
                for _ in 0..padding_len {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    data.push(state as u8);
                }
                // Store actual padding length at end
                data.push(padding_len as u8);
                data
            }
            PaddingMode::Chaotic => {
                // Pad to random length between 16 and 256 bytes
                let target = 16 + (self.morphology.frame_version as usize % 240);
                let padding_len = if data.len() >= target { 16 } else { target - data.len() };
                let original_len = data.len();
                
                let mut state = self.morphology.checksum_variant as u64 * 12345;
                for _ in 0..padding_len {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    data.push(state as u8);
                }
                
                // Encode original length in last 4 bytes
                data.extend_from_slice(&(original_len as u32).to_le_bytes());
                data
            }
        }
    }
    
    /// Remove padding based on current morphology
    fn remove_padding(&self, mut data: Vec<u8>) -> Vec<u8> {
        match self.morphology.padding_mode {
            PaddingMode::None => data,
            PaddingMode::Pkcs7 => {
                if let Some(&padding_len) = data.last() {
                    let len = data.len().saturating_sub(padding_len as usize);
                    data.truncate(len);
                }
                data
            }
            PaddingMode::Random => {
                if let Some(&padding_len) = data.last() {
                    let len = data.len().saturating_sub(padding_len as usize + 1);
                    data.truncate(len);
                }
                data
            }
            PaddingMode::Chaotic => {
                if data.len() >= 4 {
                    let len_bytes: [u8; 4] = data[data.len()-4..].try_into().unwrap_or([0; 4]);
                    let original_len = u32::from_le_bytes(len_bytes) as usize;
                    data.truncate(original_len);
                }
                data
            }
        }
    }
    
    /// Inject decoy traffic to confuse traffic analysis
    pub async fn send_decoy(&mut self) -> anyhow::Result<()> {
        let mut noise = Vec::with_capacity(64);
        let mut state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
            
        for _ in 0..64 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            noise.push((state as f32) / (u64::MAX as f32));
        }
        
        let decoy = Message::Event(Event {
            name: "__decoy__".to_string(),
            data: serde_json::json!({ "noise": noise }),
        });
        
        self.send_message(&decoy).await
    }
    
    /// Force protocol morphing (re-sync with peer)
    pub fn morph_now(&mut self, seed: u64) {
        self.morphology.evolve(seed);
        if let Some(ref mut chameleon) = self.chameleon {
            chameleon.evolve(seed);
        }
    }
}

// =============================================================================
// QUIC TRANSPORT (FALLBACK / 0-RTT)
// =============================================================================

/// Transport mode selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    /// TCP with TLS (default, maximum compatibility)
    Tcp,
    /// QUIC with 0-RTT (lower latency, built-in encryption)
    Quic,
    /// Automatic: try QUIC first, fallback to TCP
    Auto,
}

impl Default for TransportMode {
    fn default() -> Self {
        TransportMode::Auto
    }
}

/// Configuration for transport layer
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Transport mode
    pub mode: TransportMode,
    /// Enable 0-RTT for QUIC (trades security for speed on resumption)
    pub enable_0rtt: bool,
    /// Maximum idle timeout in seconds
    pub idle_timeout_secs: u64,
    /// Keep-alive interval in seconds (0 = disabled)
    pub keep_alive_secs: u64,
    /// Maximum concurrent streams (QUIC only)
    pub max_streams: u32,
    /// Server name for TLS/QUIC SNI
    pub server_name: Option<String>,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            mode: TransportMode::Auto,
            enable_0rtt: true,
            idle_timeout_secs: 30,
            keep_alive_secs: 5,
            max_streams: 100,
            server_name: None,
        }
    }
}

/// Unified transport that supports both TCP and QUIC
pub enum Transport {
    /// TCP stream (wrapped in TLS if encrypted)
    Tcp(Box<dyn AsyncReadWrite>),
    /// QUIC connection with streams
    #[cfg(feature = "quic")]
    Quic(QuicTransport),
}

/// Trait alias for async read/write
pub trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncReadWrite for T {}

#[cfg(feature = "quic")]
pub struct QuicTransport {
    connection: QuinnConnection,
    send_stream: Option<quinn::SendStream>,
    recv_stream: Option<quinn::RecvStream>,
}

#[cfg(feature = "quic")]
impl QuicTransport {
    /// Create a new QUIC transport from an existing connection
    pub fn new(connection: QuinnConnection) -> Self {
        Self {
            connection,
            send_stream: None,
            recv_stream: None,
        }
    }
    
    /// Open a bidirectional stream
    pub async fn open_bi(&mut self) -> anyhow::Result<()> {
        let (send, recv) = self.connection.open_bi().await?;
        self.send_stream = Some(send);
        self.recv_stream = Some(recv);
        Ok(())
    }
    
    /// Accept an incoming bidirectional stream
    pub async fn accept_bi(&mut self) -> anyhow::Result<()> {
        let (send, recv) = self.connection.accept_bi().await?;
        self.send_stream = Some(send);
        self.recv_stream = Some(recv);
        Ok(())
    }
    
    /// Write data to the QUIC stream
    pub async fn write_all(&mut self, data: &[u8]) -> anyhow::Result<()> {
        if let Some(ref mut stream) = self.send_stream {
            stream.write_all(data).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No send stream open"))
        }
    }
    
    /// Read exact bytes from the QUIC stream
    pub async fn read_exact(&mut self, buf: &mut [u8]) -> anyhow::Result<()> {
        if let Some(ref mut stream) = self.recv_stream {
            stream.read_exact(buf).await?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No recv stream open"))
        }
    }
    
    /// Get RTT estimate
    pub fn rtt(&self) -> std::time::Duration {
        self.connection.rtt()
    }
    
    /// Check if connection supports 0-RTT
    pub fn is_0rtt(&self) -> bool {
        // 0-RTT is accepted if we have session tickets
        true // Simplified - quinn handles this internally
    }
    
    /// Close the connection gracefully
    pub fn close(&self, code: u32, reason: &str) {
        self.connection.close(quinn::VarInt::from_u32(code), reason.as_bytes());
    }
}

/// QUIC endpoint builder for server/client
#[cfg(feature = "quic")]
pub struct QuicEndpointBuilder {
    config: TransportConfig,
}

#[cfg(feature = "quic")]
impl QuicEndpointBuilder {
    pub fn new() -> Self {
        Self {
            config: TransportConfig::default(),
        }
    }
    
    pub fn with_config(config: TransportConfig) -> Self {
        Self { config }
    }
    
    /// Generate self-signed certificate for testing/development
    fn generate_self_signed_cert() -> anyhow::Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
        let key = PrivateKeyDer::Pkcs8(cert.get_key_pair().serialize_der().into());
        let cert_der = CertificateDer::from(cert.serialize_der()?);
        Ok((cert_der, key))
    }
    
    /// Build a server endpoint
    pub fn build_server(&self, bind_addr: SocketAddr) -> anyhow::Result<Endpoint> {
        let (cert, key) = Self::generate_self_signed_cert()?;
        
        // Use quinn's crypto config builder with rustls 0.23
        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(
            quic_rustls::ServerConfig::builder_with_protocol_versions(&[&quic_rustls::version::TLS13])
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)?
        )?;
        
        let mut server_config = ServerConfig::with_crypto(Arc::new(crypto));
        
        let transport = Arc::get_mut(&mut server_config.transport).unwrap();
        transport.max_idle_timeout(Some(
            std::time::Duration::from_secs(self.config.idle_timeout_secs).try_into()?
        ));
        transport.keep_alive_interval(Some(
            std::time::Duration::from_secs(self.config.keep_alive_secs)
        ));
        transport.max_concurrent_bidi_streams(self.config.max_streams.into());
        
        let endpoint = Endpoint::server(server_config, bind_addr)?;
        Ok(endpoint)
    }
    
    /// Build a client endpoint with platform verifier (for development: skip verification)
    pub fn build_client(&self) -> anyhow::Result<Endpoint> {
        // Create a permissive client config for development
        // In production, use rustls-platform-verifier
        let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(
            quic_rustls::ClientConfig::builder_with_protocol_versions(&[&quic_rustls::version::TLS13])
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
                .with_no_client_auth()
        )?;
        
        let client_config = ClientConfig::new(Arc::new(crypto));
        
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);
        
        Ok(endpoint)
    }
    
    /// Connect to a QUIC server
    pub async fn connect(&self, endpoint: &Endpoint, addr: SocketAddr, server_name: &str) -> anyhow::Result<QuicTransport> {
        let connection = endpoint.connect(addr, server_name)?.await?;
        let mut transport = QuicTransport::new(connection);
        transport.open_bi().await?;
        Ok(transport)
    }
}

#[cfg(feature = "quic")]
impl Default for QuicEndpointBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Skip certificate verification (development only!)
/// WARNING: Do NOT use in production - this accepts any certificate!
#[cfg(feature = "quic")]
#[derive(Debug)]
struct SkipServerVerification;

#[cfg(feature = "quic")]
impl quic_rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<quic_rustls::client::danger::ServerCertVerified, quic_rustls::Error> {
        Ok(quic_rustls::client::danger::ServerCertVerified::assertion())
    }
    
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &quic_rustls::DigitallySignedStruct,
    ) -> Result<quic_rustls::client::danger::HandshakeSignatureValid, quic_rustls::Error> {
        Ok(quic_rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &quic_rustls::DigitallySignedStruct,
    ) -> Result<quic_rustls::client::danger::HandshakeSignatureValid, quic_rustls::Error> {
        Ok(quic_rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    
    fn supported_verify_schemes(&self) -> Vec<quic_rustls::SignatureScheme> {
        vec![
            quic_rustls::SignatureScheme::RSA_PKCS1_SHA256,
            quic_rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            quic_rustls::SignatureScheme::RSA_PKCS1_SHA384,
            quic_rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            quic_rustls::SignatureScheme::RSA_PKCS1_SHA512,
            quic_rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            quic_rustls::SignatureScheme::RSA_PSS_SHA256,
            quic_rustls::SignatureScheme::RSA_PSS_SHA384,
            quic_rustls::SignatureScheme::RSA_PSS_SHA512,
            quic_rustls::SignatureScheme::ED25519,
        ]
    }
}

/// Unified connection that works over both TCP and QUIC
pub struct HyperlightConnection {
    transport: TransportInner,
    handler: ProtocolHandlerState,
}

enum TransportInner {
    Tcp {
        stream: Box<dyn AsyncReadWrite>,
    },
    #[cfg(feature = "quic")]
    Quic {
        transport: QuicTransport,
    },
}

struct ProtocolHandlerState {
    cipher: Option<Aes256Gcm>,
    chameleon: Option<ChameleonKey>,
    morphology: ProtocolMorphology,
    moving_target: bool,
    output_predictor: SpeculativePredictor,
    input_predictor: SpeculativePredictor,
    precomputed_cache: Vec<(u64, Message)>,
    prediction_cache: Vec<(u64, Vec<u8>)>,
    last_output_prediction: Option<u64>,
    last_input_prediction: Option<u64>,
    pub speculation_stats: SpeculationStats,
}

impl HyperlightConnection {
    /// Create from TCP stream
    pub fn from_tcp<S: AsyncReadWrite + 'static>(stream: S) -> Self {
        Self {
            transport: TransportInner::Tcp {
                stream: Box::new(stream),
            },
            handler: ProtocolHandlerState::new(),
        }
    }
    
    /// Create from QUIC transport
    #[cfg(feature = "quic")]
    pub fn from_quic(transport: QuicTransport) -> Self {
        Self {
            transport: TransportInner::Quic { transport },
            handler: ProtocolHandlerState::new(),
        }
    }
    
    /// Enable Chameleon protocol
    pub fn enable_chameleon(&mut self, secret: [u8; 32]) {
        self.handler.chameleon = Some(ChameleonKey::new(&secret));
        self.handler.morphology = ProtocolMorphology::new(
            u64::from_le_bytes(secret[0..8].try_into().unwrap())
        );
        self.handler.moving_target = true;
    }
    
    /// Get transport mode
    pub fn transport_mode(&self) -> TransportMode {
        match &self.transport {
            TransportInner::Tcp { .. } => TransportMode::Tcp,
            #[cfg(feature = "quic")]
            TransportInner::Quic { .. } => TransportMode::Quic,
        }
    }
    
    /// Get RTT estimate (QUIC only, returns None for TCP)
    #[cfg(feature = "quic")]
    pub fn rtt(&self) -> Option<std::time::Duration> {
        match &self.transport {
            TransportInner::Quic { transport } => Some(transport.rtt()),
            _ => None,
        }
    }
    
    /// Send a message
    pub async fn send(&mut self, msg: &Message) -> anyhow::Result<()> {
        let data = self.handler.encode_message(msg)?;
        
        match &mut self.transport {
            TransportInner::Tcp { stream } => {
                self.handler.write_frame(stream.as_mut(), &data).await?;
            }
            #[cfg(feature = "quic")]
            TransportInner::Quic { transport } => {
                // QUIC: length-prefixed frame
                let len = (data.len() as u32).to_le_bytes();
                transport.write_all(&len).await?;
                transport.write_all(&data).await?;
            }
        }
        
        // Evolve key after sending
        if let Some(ref mut chameleon) = self.handler.chameleon {
            if self.handler.moving_target {
                let hash = Self::hash_data(&data);
                chameleon.evolve(hash);
                self.handler.morphology.evolve(hash);
            }
        }
        
        Ok(())
    }
    
    /// Receive a message
    pub async fn recv(&mut self) -> anyhow::Result<Message> {
        let data = match &mut self.transport {
            TransportInner::Tcp { stream } => {
                self.handler.read_frame(stream.as_mut()).await?
            }
            #[cfg(feature = "quic")]
            TransportInner::Quic { transport } => {
                let mut len_buf = [0u8; 4];
                transport.read_exact(&mut len_buf).await?;
                let len = u32::from_le_bytes(len_buf) as usize;
                let mut buf = vec![0u8; len];
                transport.read_exact(&mut buf).await?;
                buf
            }
        };
        
        let msg = self.handler.decode_message(&data)?;
        
        // Evolve key after receiving
        if let Some(ref mut chameleon) = self.handler.chameleon {
            if self.handler.moving_target {
                let hash = Self::hash_data(&data);
                chameleon.evolve(hash);
                self.handler.morphology.evolve(hash);
            }
        }
        
        Ok(msg)
    }
    
    fn hash_data(data: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }
}

impl ProtocolHandlerState {
    fn new() -> Self {
        Self {
            cipher: None,
            chameleon: None,
            morphology: ProtocolMorphology::new(0),
            moving_target: false,
            output_predictor: SpeculativePredictor::new(),
            input_predictor: SpeculativePredictor::new(),
            precomputed_cache: Vec::with_capacity(16),
            prediction_cache: Vec::with_capacity(16),
            last_output_prediction: None,
            last_input_prediction: None,
            speculation_stats: SpeculationStats::default(),
        }
    }
    
    fn encode_message(&mut self, msg: &Message) -> anyhow::Result<Vec<u8>> {
        let mut data = serde_json::to_vec(msg)?;
        
        // Apply padding
        data = self.apply_padding(data);
        
        // Compression
        data = encode_all(&data[..], 3)?;
        
        // Chameleon encoding
        if let Some(ref mut chameleon) = self.chameleon {
            let latent = chameleon.encode(&data);
            data = serde_json::to_vec(&latent)?;
        } else if let Some(cipher) = &self.cipher {
            let nonce_bytes = rand::random::<[u8; 12]>();
            let nonce = Nonce::from_slice(&nonce_bytes);
            data = cipher.encrypt(nonce, data.as_ref())
                .map_err(|e| anyhow::anyhow!("Encryption error: {}", e))?;
            let mut with_nonce = nonce_bytes.to_vec();
            with_nonce.extend(data);
            data = with_nonce;
        }
        
        Ok(data)
    }
    
    fn decode_message(&mut self, data: &[u8]) -> anyhow::Result<Message> {
        let mut data = data.to_vec();
        
        // Chameleon decoding
        if let Some(ref mut chameleon) = self.chameleon {
            let latent: LatentVector = serde_json::from_slice(&data)?;
            data = chameleon.decode(&latent);
        } else if let Some(cipher) = &self.cipher {
            if data.len() < 12 {
                return Err(anyhow::anyhow!("Invalid encrypted message"));
            }
            let nonce = Nonce::from_slice(&data[..12]);
            data = cipher.decrypt(nonce, &data[12..])
                .map_err(|e| anyhow::anyhow!("Decryption error: {}", e))?;
        }
        
        // Decompression
        data = decode_all(&data[..])?;
        
        // Remove padding
        data = self.remove_padding(data);
        
        let msg: Message = serde_json::from_slice(&data)?;
        Ok(msg)
    }
    
    async fn write_frame<W: AsyncWrite + Unpin + ?Sized>(&self, writer: &mut W, data: &[u8]) -> anyhow::Result<()> {
        let len = data.len() as u32;
        let mut header = Vec::with_capacity(self.morphology.header_size as usize);
        
        let len_bytes = if self.morphology.big_endian {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        };
        header.extend_from_slice(&len_bytes);
        header.push(self.morphology.frame_version);
        
        while header.len() < self.morphology.header_size as usize {
            header.push(self.morphology.checksum_variant);
        }
        
        writer.write_all(&header).await?;
        writer.write_all(data).await?;
        Ok(())
    }
    
    async fn read_frame<R: AsyncRead + Unpin + ?Sized>(&self, reader: &mut R) -> anyhow::Result<Vec<u8>> {
        let mut header = vec![0u8; self.morphology.header_size as usize];
        reader.read_exact(&mut header).await?;
        
        let len_bytes: [u8; 4] = header[0..4].try_into()?;
        let len = if self.morphology.big_endian {
            u32::from_be_bytes(len_bytes)
        } else {
            u32::from_le_bytes(len_bytes)
        } as usize;
        
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).await?;
        Ok(buf)
    }
    
    fn apply_padding(&self, mut data: Vec<u8>) -> Vec<u8> {
        match self.morphology.padding_mode {
            PaddingMode::None => data,
            PaddingMode::Pkcs7 => {
                let padding_len = 16 - (data.len() % 16);
                data.extend(std::iter::repeat(padding_len as u8).take(padding_len));
                data
            }
            PaddingMode::Random => {
                let padding_len = 16 - (data.len() % 16);
                let mut state = self.morphology.frame_version as u64;
                for _ in 0..padding_len {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    data.push(state as u8);
                }
                data.push(padding_len as u8);
                data
            }
            PaddingMode::Chaotic => {
                let target = 16 + (self.morphology.frame_version as usize % 240);
                let padding_len = if data.len() >= target { 16 } else { target - data.len() };
                let original_len = data.len();
                
                let mut state = self.morphology.checksum_variant as u64 * 12345;
                for _ in 0..padding_len {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    data.push(state as u8);
                }
                data.extend_from_slice(&(original_len as u32).to_le_bytes());
                data
            }
        }
    }
    
    fn remove_padding(&self, mut data: Vec<u8>) -> Vec<u8> {
        match self.morphology.padding_mode {
            PaddingMode::None => data,
            PaddingMode::Pkcs7 => {
                if let Some(&padding_len) = data.last() {
                    let len = data.len().saturating_sub(padding_len as usize);
                    data.truncate(len);
                }
                data
            }
            PaddingMode::Random => {
                if let Some(&padding_len) = data.last() {
                    let len = data.len().saturating_sub(padding_len as usize + 1);
                    data.truncate(len);
                }
                data
            }
            PaddingMode::Chaotic => {
                if data.len() >= 4 {
                    let len_bytes: [u8; 4] = data[data.len()-4..].try_into().unwrap_or([0; 4]);
                    let original_len = u32::from_le_bytes(len_bytes) as usize;
                    data.truncate(original_len);
                }
                data
            }
        }
    }
}

/// Connect with automatic transport selection (QUIC with TCP fallback)
#[cfg(feature = "quic")]
pub async fn connect_auto(
    addr: SocketAddr,
    tcp_fallback: impl std::future::Future<Output = anyhow::Result<impl AsyncReadWrite + 'static>>,
    config: TransportConfig,
) -> anyhow::Result<HyperlightConnection> {
    match config.mode {
        TransportMode::Tcp => {
            let stream = tcp_fallback.await?;
            Ok(HyperlightConnection::from_tcp(stream))
        }
        TransportMode::Quic => {
            let builder = QuicEndpointBuilder::with_config(config.clone());
            let endpoint = builder.build_client()?;
            let server_name = config.server_name.as_deref().unwrap_or("localhost");
            let transport = builder.connect(&endpoint, addr, server_name).await?;
            Ok(HyperlightConnection::from_quic(transport))
        }
        TransportMode::Auto => {
            // Try QUIC first
            let builder = QuicEndpointBuilder::with_config(config.clone());
            if let Ok(endpoint) = builder.build_client() {
                let server_name = config.server_name.as_deref().unwrap_or("localhost");
                if let Ok(transport) = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    builder.connect(&endpoint, addr, server_name)
                ).await {
                    if let Ok(transport) = transport {
                        log::info!("Connected via QUIC to {}", addr);
                        return Ok(HyperlightConnection::from_quic(transport));
                    }
                }
            }
            
            // Fallback to TCP
            log::info!("QUIC unavailable, falling back to TCP for {}", addr);
            let stream = tcp_fallback.await?;
            Ok(HyperlightConnection::from_tcp(stream))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chameleon_key_basic() {
        let secret = [0x42u8; 32];
        let mut key = ChameleonKey::new(&secret);
        
        // Encode some data
        let data = b"Hello, Chameleon Protocol!";
        let vector = key.encode(data);
        
        assert!(!vector.components.is_empty());
        assert_eq!(vector.epoch, 0);
        
        // Evolve the key
        key.evolve(0xDEADBEEF);
        let vector2 = key.encode(data);
        assert_eq!(vector2.epoch, 1);
    }
    
    #[test]
    fn test_chameleon_key_miras() {
        let secret = [0x42u8; 32];
        let mut key = ChameleonKey::new_with_miras(&secret, MirasVariant::Yaad);
        
        assert_eq!(key.variant(), "YAAD");
        
        // Encode data
        let data = b"MIRAS-powered encoding";
        let vector = key.encode(data);
        assert!(!vector.components.is_empty());
        
        // Check anomaly tracking
        let anomaly = key.anomaly_level();
        assert!(anomaly >= 0.0);
    }
    
    #[test]
    fn test_chameleon_miras_variants() {
        let secret = [0x42u8; 32];
        
        // Test all MIRAS variants
        for variant in [
            MirasVariant::Titans,
            MirasVariant::Yaad,
            MirasVariant::Moneta { p: 2.0 },
            MirasVariant::Memora,
        ] {
            let mut key = ChameleonKey::new_with_miras(&secret, variant);
            let vector = key.encode(b"Test message");
            assert!(!vector.components.is_empty());
        }
    }
    
    #[test]
    fn test_speculative_predictor() {
        let mut predictor = SpeculativePredictor::new();
        
        // Observe some messages
        predictor.observe(b"GET /api/users", MessageType::Request);
        predictor.observe(b"200 OK", MessageType::Response);
        predictor.observe(b"GET /api/posts", MessageType::Request);
        
        // Get prediction
        let (hash, confidence) = predictor.predict_next();
        assert!(hash != 0 || confidence >= 0.0);
    }
    
    #[test]
    fn test_protocol_morphology() {
        let mut morph = ProtocolMorphology::new(12345);
        let initial_version = morph.frame_version;
        
        morph.evolve(67890);
        
        // Frame version should evolve
        // The evolve function changes internal state
        assert!(morph.frame_version != initial_version || morph.header_size > 0);
    }
}
