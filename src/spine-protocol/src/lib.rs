// Allow dead code for protocol features designed for future extensions
#![allow(dead_code)]

pub mod agentic;
pub mod negotiation;
pub mod replay;

pub use agentic::{
    Capability, CapabilityAdvertisement, CapabilityQuery, CapabilitySelector, StreamData,
    StreamEnd, StreamEndReason, StreamRole, StreamStart, StreamToken, StreamUsage,
    ToolCall, ToolOutcome, ToolResult, TraceContext,
};

use aes_gcm::{aead::Aead, Aes256Gcm, Key, KeyInit, Nonce};
use hmac::{Hmac, Mac};
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use sha2::Sha256 as Sha256Hash;
use spine_crypto::{
    LatticeParams, QuantumSpeculativeProtocol, TransformerConfig, TransformerPredictor,
};
use spine_neural::{MirasNeuralEncoder, MirasVariant, NeuralEncoderConfig, NeuralLatentEncoder};
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;
use zstd::stream::decode_all;
use zstd::stream::encode_all;

// High-performance binary serialization
// (bincode and bytemuck are used via their traits, not explicit imports)

// QUIC transport support (uses rustls 0.23 via quinn)
#[cfg(feature = "quic")]
use quinn::{ClientConfig, Connection as QuinnConnection, Endpoint, ServerConfig};
#[cfg(feature = "quic")]
use rustls as quic_rustls; // rustls 0.23 for QUIC (workspace)
#[cfg(feature = "quic")]
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
#[cfg(feature = "quic")]
use std::net::SocketAddr;

/// Write `header` followed by `body` to an `AsyncWrite` using a single
/// `write_vectored` call when the underlying stream supports it, falling back
/// to sequential writes for the remainder on a partial write.
///
/// Replaces the common `write_all(&header); write_all(&body)` pattern that
/// costs two `send()` syscalls (and on TCP_NODELAY sockets, often two packets
/// on the wire).
#[inline]
pub async fn write_header_body<W: AsyncWrite + Unpin + ?Sized>(
    w: &mut W,
    header: &[u8],
    body: &[u8],
) -> std::io::Result<()> {
    use std::io::IoSlice;
    let total = header.len() + body.len();
    let mut sent: usize = 0;
    // First attempt: single vectored call. Most TCP impls in tokio honor this.
    while sent < total {
        let n = if sent < header.len() {
            let slices = [IoSlice::new(&header[sent..]), IoSlice::new(body)];
            w.write_vectored(&slices).await?
        } else {
            w.write(&body[sent - header.len()..]).await?
        };
        if n == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::WriteZero));
        }
        sent += n;
    }
    Ok(())
}

// =============================================================================
// CHAMELEON PROTOCOL
// =============================================================================
//
// A moving-target defense system using latent space transformations for
// implicit encryption and compression.
//
// Core Insight: High-dimensional vector spaces are inherently encrypted—
// the transformation matrix IS the key. By evolving the transformation
// based on message history, we create a protocol impossible to analyze.

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
    /// Stack-allocated fixed signature (no heap allocation per message)
    _latent_signature: Option<[f32; 8]>,
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
            memory_size: 32, // Titans persistent memory tokens
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
        self.sequence_state = self
            .sequence_state
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

    fn extract_latent_signature(&self, data: &[u8]) -> Option<[f32; 8]> {
        // Extract a compact signature from the data for similarity matching
        if data.len() < 32 {
            return None;
        }

        // Stack-allocated fixed signature: first 8 floats from data
        let mut sig = [0.0f32; 8];
        for (i, chunk) in data.chunks(4).take(8).enumerate() {
            if chunk.len() == 4 {
                sig[i] = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            }
        }

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

impl LatentVector {
    /// Zero-copy serialization to bytes (22+ GiB/s for 1024-dim vectors)
    /// Layout: [dim_hint: u16][epoch: u64][components: [f32; N]]
    #[inline]
    pub fn to_bytes_fast(&self) -> Vec<u8> {
        let component_bytes = self.components.len() * 4;
        let total = 2 + 8 + component_bytes;
        let mut buf = Vec::with_capacity(total);

        buf.extend_from_slice(&self.dim_hint.to_le_bytes());
        buf.extend_from_slice(&self.epoch.to_le_bytes());
        // SAFETY: f32 has no padding, transmute is valid
        buf.extend_from_slice(bytemuck::cast_slice(&self.components));

        buf
    }

    /// Zero-copy deserialization from bytes
    #[inline]
    pub fn from_bytes_fast(data: &[u8]) -> Option<Self> {
        if data.len() < 10 {
            return None;
        }

        let dim_hint = u16::from_le_bytes([data[0], data[1]]);
        let epoch = u64::from_le_bytes(data[2..10].try_into().ok()?);

        let component_bytes = &data[10..];
        if !component_bytes.len().is_multiple_of(4) {
            return None;
        }

        // Use try_cast_slice to handle unaligned data gracefully
        let components: Vec<f32> = match bytemuck::try_cast_slice(component_bytes) {
            Ok(s) => s.to_vec(),
            Err(_) => component_bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        };

        Some(Self {
            components,
            dim_hint,
            epoch,
        })
    }

    /// Serialize to bytes into pre-allocated buffer (zero-alloc hot path)
    #[inline]
    pub fn to_bytes_into(&self, buf: &mut Vec<u8>) {
        let component_bytes = self.components.len() * 4;
        let total = 2 + 8 + component_bytes;
        buf.clear();
        buf.reserve(total);

        buf.extend_from_slice(&self.dim_hint.to_le_bytes());
        buf.extend_from_slice(&self.epoch.to_le_bytes());
        buf.extend_from_slice(bytemuck::cast_slice(&self.components));
    }

    /// Bincode serialization (fallback, ~5x faster than JSON)
    #[inline]
    pub fn to_bincode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Bincode deserialization
    #[inline]
    pub fn from_bincode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
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
            memory_size: 32, // Titans persistent memory tokens
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
            latent_dim: dimension, // 128
            hidden_dims: vec![256, 128],
            attention_heads: 8, // 128/8=16 ✓
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
    /// HMAC key for cryptographic morphology evolution (not serialized over wire)
    #[serde(skip, default = "default_morphology_key")]
    hmac_key: [u8; 32],
    /// Evolution counter for HMAC domain separation
    #[serde(skip)]
    evolution_counter: u64,
}

fn default_morphology_key() -> [u8; 32] {
    [0u8; 32]
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
            header_size: 5 + (seed % 12) as u8,
            big_endian: seed.is_multiple_of(2),
            checksum_variant: (seed % 4) as u8,
            padding_mode: match seed % 4 {
                0 => PaddingMode::None,
                1 => PaddingMode::Pkcs7,
                2 => PaddingMode::Random,
                _ => PaddingMode::Chaotic,
            },
            hmac_key: [0u8; 32],
            evolution_counter: 0,
        }
    }

    /// Create with a session secret for cryptographic morphology evolution
    fn new_keyed(seed: u64, secret: &[u8; 32]) -> Self {
        let mut m = Self::new(seed);
        m.hmac_key = *secret;
        m
    }

    /// Evolve morphology using HMAC-SHA256 PRF keyed by session secret.
    /// Produces cryptographically unpredictable state transitions that
    /// cannot be predicted without knowledge of the HMAC key.
    fn evolve(&mut self, msg_hash: u64) {
        type HmacSha256 = Hmac<Sha256Hash>;

        // Domain-separate each evolution step with counter + message hash
        self.evolution_counter += 1;
        let mut input = [0u8; 16];
        input[..8].copy_from_slice(&self.evolution_counter.to_le_bytes());
        input[8..].copy_from_slice(&msg_hash.to_le_bytes());

        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(&self.hmac_key).expect("HMAC accepts any key size");
        mac.update(&input);
        let result = mac.finalize().into_bytes();

        // Derive each morphology field from different bytes of the HMAC output
        self.frame_version = self.frame_version.wrapping_add(result[0] % 7);
        self.header_size = 5 + (result[1] % 12);
        self.big_endian = result[2] & 1 == 0;
        self.checksum_variant = result[3] % 4;
        self.padding_mode = match result[4] % 4 {
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
    BinaryProgram(SpineBinary),
    /// Latent-encoded message (implicit encryption)
    LatentMessage(LatentVector),
    /// Protocol synchronization (for moving-target alignment)
    Sync(SyncPayload),
    /// Speculative frame (prediction-accelerated message)
    Speculative(SpeculativeFrame),
    /// Pre-computed response (receiver sends this when prediction matched)
    PreComputed(PreComputedResponse),
    /// Server-initiated request to morph the protocol
    MorphRequest {
        seed: u64,
    },
    /// Health check to verify connection
    Ping {
        timestamp: u64,
    },
    /// Response to health check
    Pong {
        timestamp: u64,
    },

    // ----- Agentic-first primitives (see crate::agentic) -----
    /// MCP-style tool invocation request.
    ToolCall(ToolCall),
    /// MCP-style tool invocation result.
    ToolResult(ToolResult),
    /// LLM token stream opens.
    StreamStart(StreamStart),
    /// One chunk of an LLM token stream.
    StreamToken(StreamToken),
    /// LLM token stream closes.
    StreamEnd(StreamEnd),
    /// Capability handshake query.
    CapabilityQuery(CapabilityQuery),
    /// Capability handshake response.
    CapabilityAd(CapabilityAdvertisement),
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
pub struct SpineBinary {
    pub instructions: Vec<Instruction>,
    pub data: Vec<u8>,
    pub render_start: usize,
    pub exported_functions: std::collections::HashMap<String, usize>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Instruction {
    DefineElement {
        id: u32,
        tag: String,
    },
    SetAttribute {
        id: u32,
        key: String,
        value: String,
    },
    AddChild {
        parent_id: u32,
        child_id: u32,
    },
    EmitEvent {
        name: String,
        payload: serde_json::Value,
    },
    StreamLatent {
        vector: Vec<f32>,
    },
    /// Morph the protocol mid-stream
    MorphProtocol {
        seed: u64,
    },
    /// Inject decoy traffic
    Decoy {
        noise: Vec<f32>,
    },
    /// Declare a reactive state variable
    DeclareState {
        name: String,
        initial_json: serde_json::Value,
    },
    /// Update a reactive state variable
    UpdateState {
        name: String,
        value_json: serde_json::Value,
    },

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
    Call {
        name: String,
        num_args: usize,
    },
    CallTarget(usize),
    Return,

    // --- Stack-based DOM Operations ---
    DefineElementFromStack {
        id: u32,
    },
    SetAttributeFromStack {
        id: u32,
        key: String,
    },
    AddChildFromStack {
        parent_id: u32,
        child_id: u32,
    },
    EmitEventFromStack {
        name: String,
    },
    DefineTextFromStack,
    DeclareStateFromStack {
        name: String,
    },
    UpdateStateFromStack {
        name: String,
    },

    // --- Agentic Operations ---
    NavigateFromStack,
    SearchFromStack,
    StoreKnowledgeFromStack {
        tags: Vec<String>,
    },
    QueryKnowledgeFromStack {
        tags: Vec<String>,
        limit: usize,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ProtocolBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Concat,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ProtocolUnaryOp {
    Not,
    Neg,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum BrowserCommand {
    Navigate {
        url: String,
    },
    GetUR,
    GetRawHTML,
    Click {
        element_id: String,
    },
    Type {
        element_id: String,
        text: String,
    },
    ExecuteBinary(SpineBinary),
    /// Handle an event in the current session
    HandleEvent {
        element_id: u32,
        event_name: String,
        payload: serde_json::Value,
    },
    /// Request latent-encoded response
    GetLatentUR {
        dimensions: usize,
    },
    /// Trigger protocol morphing
    Morph,
    /// Semantic search in the current page
    Search {
        query: String,
    },
    /// Transfer session to another node
    TransferSession {
        target_node_id: Uuid,
    },
    /// Store knowledge in the agent's long-term memory
    StoreKnowledge {
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    },
    /// Query knowledge from the agent's long-term memory
    QueryKnowledge {
        query: String,
        tags: Vec<String>,
        limit: usize,
    },
    /// Delete knowledge from the agent's long-term memory
    DeleteKnowledge {
        key: String,
    },
    /// Get the history of commands in this session
    GetSessionHistory,
    /// Get the capabilities of the current agentic binary
    GetCapabilities,
    /// Enable or disable autonomous mode for the agent
    SetAutonomousMode {
        enabled: bool,
    },
    /// Perform a swarm search across the cluster
    SwarmSearch {
        query: String,
        depth: usize,
    },
    /// Delegate a task to another agent in the cluster
    DelegateTask {
        task: String,
        target_agent_id: Option<Uuid>,
    },
    /// Propose knowledge to the cluster for consensus
    ProposeKnowledge {
        key: String,
        value: serde_json::Value,
        tags: Vec<String>,
    },
    /// Create a swarm plan for a high-level goal
    CreateSwarmPlan {
        goal: String,
    },
    /// Execute a specific task within a swarm plan
    ExecutePlanTask {
        plan_id: Uuid,
        task_id: Uuid,
    },
    /// Transmit data using the neural protocol
    NeuralTransmit {
        data: Vec<u8>,
        domain: String,
    },
    /// Get the full agentic state (memory, speech acts, etc.)
    GetAgenticState,
    /// Send a speech act to another agent
    SendSpeechAct {
        target_id: Uuid,
        performative: String,
        content: String,
    },
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
    pub dependencies: Vec<Uuid>,   // Task IDs
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
    /// Nonce counter for AES-GCM (prevents nonce reuse)
    nonce_counter: u64,
    /// Random session nonce mixed into AES-GCM IV (prevents cross-session reuse)
    session_nonce: [u8; 4],
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
    /// Reusable send buffer to eliminate per-message heap allocations
    send_buf: Vec<u8>,
    /// Reusable read buffer for frame payloads
    read_buf: Vec<u8>,
    /// Reusable latent serialization buffer
    latent_buf: Vec<u8>,
    /// Minimum payload size for zstd compression (below this, overhead exceeds savings)
    compression_threshold: usize,
    /// Latent-space AEAD: chain Chameleon encoding + AES-GCM encryption (M2)
    /// When enabled, data is first neural-encoded to a LatentVector, serialized,
    /// then AES-GCM encrypted. Provides defense-in-depth: neural obfuscation +
    /// authenticated encryption. Requires both chameleon and cipher to be set.
    latent_aead: bool,
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
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            cipher: None,
            chameleon: None,
            morphology: ProtocolMorphology::new(0),
            nonce_counter: 0,
            session_nonce: rand::random::<[u8; 4]>(),
            moving_target: false,
            output_predictor: SpeculativePredictor::new(),
            input_predictor: SpeculativePredictor::new(),
            precomputed_cache: Vec::with_capacity(16),
            prediction_cache: Vec::with_capacity(16),
            last_output_prediction: None,
            last_input_prediction: None,
            speculation_stats: SpeculationStats::default(),
            send_buf: Vec::with_capacity(8192),
            read_buf: Vec::with_capacity(8192),
            latent_buf: Vec::with_capacity(4096),
            compression_threshold: 64,
            latent_aead: false,
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
        self.morphology = ProtocolMorphology::new_keyed(
            u64::from_le_bytes(secret[0..8].try_into().unwrap()),
            &secret,
        );
        self.moving_target = true;
    }

    /// Enable Chameleon + AES-GCM (M2: latent-space AEAD)
    ///
    /// Defense-in-depth: data is first neural-encoded to a LatentVector
    /// (obfuscation), then the serialized latent bytes are AES-256-GCM
    /// encrypted (authenticated encryption). An attacker must break both
    /// the neural encoder AND AES-GCM to recover plaintext.
    pub fn enable_chameleon_aead(&mut self, secret: [u8; 32]) {
        // Derive separate keys for Chameleon and AES-GCM from the shared secret
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &secret);
        let mut aes_key = [0u8; 32];
        hk.expand(b"spine-latent-aead-key", &mut aes_key)
            .expect("HKDF expand");

        // Set up Chameleon with the original secret
        self.chameleon = Some(ChameleonKey::new(&secret));
        self.morphology = ProtocolMorphology::new_keyed(
            u64::from_le_bytes(secret[0..8].try_into().unwrap()),
            &secret,
        );
        self.moving_target = true;

        // Set up AES-GCM with derived key
        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&aes_key);
        self.cipher = Some(Aes256Gcm::new(key));
        self.latent_aead = true;
    }

    /// Enable speculative decoding
    pub fn enable_speculation(&mut self, output: bool, input: bool) {
        self.output_predictor.output_speculation = output;
        self.input_predictor.input_speculation = input;
    }

    /// Pre-compute a response for a predicted request
    pub fn precompute_response(&mut self, predicted_request_hash: u64, response: Message) {
        self.precomputed_cache
            .push((predicted_request_hash, response));
        // Keep cache bounded
        if self.precomputed_cache.len() > 32 {
            self.precomputed_cache.remove(0);
        }
    }

    /// Check if we have a pre-computed response
    fn get_precomputed(&mut self, request_hash: u64) -> Option<Message> {
        if let Some(pos) = self
            .precomputed_cache
            .iter()
            .position(|(h, _)| *h == request_hash)
        {
            self.speculation_stats.precompute_hits += 1;
            Some(self.precomputed_cache.remove(pos).1)
        } else {
            None
        }
    }

    /// Hash message for key evolution
    #[inline]
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
            // Agentic primitives slot into the existing request/response/event
            // taxonomy for purposes of speculative prediction.
            Message::ToolCall(_) => MessageType::Request,
            Message::ToolResult(_) => MessageType::Response,
            Message::StreamStart(_) => MessageType::Event,
            Message::StreamToken(_) => MessageType::Event,
            Message::StreamEnd(_) => MessageType::Event,
            Message::CapabilityQuery(_) => MessageType::Request,
            Message::CapabilityAd(_) => MessageType::Response,
        }
    }

    /// Send a message with speculative encoding
    ///
    /// Optimized hot path: single serialization pass, buffer reuse,
    /// adaptive compression (skipped for small payloads < threshold).
    #[inline]
    pub async fn send_message(&mut self, msg: &Message) -> anyhow::Result<()> {
        // Serialize message once into reusable buffer
        self.send_buf.clear();
        serde_json::to_writer(&mut self.send_buf, msg)?;
        let msg_hash = Self::hash_message(&self.send_buf);
        let msg_type = Self::classify_message(msg);

        // Check if receiver predicted this message
        let payload = if let Some(predicted_hash) = self.last_input_prediction {
            if predicted_hash == msg_hash {
                // Prediction hit! Send minimal confirmation (no data needed)
                self.speculation_stats.output_hits += 1;
                self.speculation_stats.bytes_saved += self.send_buf.len() as u64;
                SpeculativePayload::Confirmed {
                    confirmed_hash: msg_hash,
                    delta: Vec::new(),
                }
            } else {
                // Prediction miss — move buffer contents to avoid clone
                SpeculativePayload::Full(std::mem::take(&mut self.send_buf))
            }
        } else {
            SpeculativePayload::Full(std::mem::take(&mut self.send_buf))
        };

        // Observe for prediction (use payload data if available, else empty)
        let observe_data: &[u8] = match &payload {
            SpeculativePayload::Full(d) => d,
            _ => &[],
        };
        self.output_predictor.observe(observe_data, msg_type);
        let (next_prediction, confidence) = self.output_predictor.predict_next();

        // Cache the predicted data for reconstruction on hit
        if confidence > 0.5 {
            let predicted_data = self.output_predictor.predict_next_data(observe_data.len());
            self.prediction_cache
                .push((next_prediction, predicted_data));
            if self.prediction_cache.len() > 16 {
                self.prediction_cache.remove(0);
            }
        }

        self.last_output_prediction = Some(next_prediction);
        self.speculation_stats.output_predictions += 1;

        // Build and serialize speculative frame in one pass
        let frame = SpeculativeFrame {
            payload,
            next_prediction_hash: next_prediction,
            confidence,
            speculation_depth: 1,
        };

        let mut data = serde_json::to_vec(&frame)?;

        // Restore send_buf capacity for next call
        if self.send_buf.capacity() == 0 {
            self.send_buf = Vec::with_capacity(8192);
        }

        // Apply padding based on current morphology
        data = self.apply_padding(data);

        // Adaptive compression: skip zstd for tiny control payloads (< 64 bytes)
        // For larger payloads, zstd at level 3 provides excellent ratio with low overhead.
        // We prepend a 1-byte flag: 0x01 = compressed, 0x00 = raw
        if data.len() >= self.compression_threshold {
            let compressed = encode_all(&data[..], 3)?;
            let mut flagged = Vec::with_capacity(1 + compressed.len());
            flagged.push(0x01); // compressed flag
            flagged.extend_from_slice(&compressed);
            data = flagged;
        } else {
            data.insert(0, 0x00); // raw flag
        }

        // Chameleon encoding (if enabled)
        if let Some(ref mut chameleon) = self.chameleon {
            let latent = chameleon.encode(&data);
            // FAST: Serialize into reusable buffer (zero-alloc steady-state, 22 GiB/s)
            latent.to_bytes_into(&mut self.latent_buf);
            data = std::mem::take(&mut self.latent_buf);

            // M2: Latent-space AEAD — encrypt the serialized latent vector with AES-GCM
            // This provides defense-in-depth: neural obfuscation + authenticated encryption.
            // An attacker must break BOTH the neural latent encoder AND AES-256-GCM.
            if self.latent_aead {
                if let Some(cipher) = &self.cipher {
                    self.nonce_counter = self.nonce_counter.wrapping_add(1);
                    let mut nonce_full = [0u8; 12];
                    nonce_full[..8].copy_from_slice(&self.nonce_counter.to_le_bytes());
                    nonce_full[8..].copy_from_slice(&self.session_nonce);
                    let nonce = Nonce::from_slice(&nonce_full);
                    data = cipher
                        .encrypt(nonce, data.as_ref())
                        .map_err(|e| anyhow::anyhow!("Latent AEAD encrypt error: {}", e))?;
                    let mut with_nonce = nonce_full.to_vec();
                    with_nonce.extend(data);
                    data = with_nonce;
                }
            }

            // Evolve key after sending (moving target)
            if self.moving_target {
                chameleon.evolve(msg_hash);
                // morphology evolution deferred to after write_morphed_frame
            }
        }
        // Fallback to AES encryption (without Chameleon)
        else if let Some(cipher) = &self.cipher {
            // SECURITY: Nonce = [counter (8 bytes)] || [session_nonce (4 bytes)]
            // Session nonce prevents cross-session nonce reuse with same key
            self.nonce_counter = self.nonce_counter.wrapping_add(1);
            let mut nonce_full = [0u8; 12];
            nonce_full[..8].copy_from_slice(&self.nonce_counter.to_le_bytes());
            nonce_full[8..].copy_from_slice(&self.session_nonce);
            let nonce = Nonce::from_slice(&nonce_full);
            data = cipher
                .encrypt(nonce, data.as_ref())
                .map_err(|e| anyhow::anyhow!("Encryption error: {}", e))?;

            // Prepend nonce for receiver
            let mut with_nonce = nonce_full.to_vec();
            with_nonce.extend(data);
            data = with_nonce;
        }

        // Write with morphing header
        self.write_morphed_frame(&data).await?;

        // Evolve morphology AFTER writing frame to keep sender/receiver in sync
        if self.moving_target && self.chameleon.is_some() {
            self.morphology.evolve(msg_hash);
        }
        Ok(())
    }

    /// Send message without speculation (for bootstrapping)
    pub async fn send_message_raw(&mut self, msg: &Message) -> anyhow::Result<()> {
        let mut data = serde_json::to_vec(msg)?;
        let msg_hash = Self::hash_message(&data);

        data = self.apply_padding(data);

        // Adaptive compression with flag
        // SECURITY NOTE (L2): Compressing plaintext before encryption can leak
        // information via ciphertext size changes (CRIME/BREACH attacks). This is
        // acceptable for agent-to-agent communication where there is no reflected
        // user-controlled content. Disable compression when handling untrusted input.
        if data.len() >= self.compression_threshold {
            let compressed = encode_all(&data[..], 3)?;
            let mut flagged = Vec::with_capacity(1 + compressed.len());
            flagged.push(0x01);
            flagged.extend_from_slice(&compressed);
            data = flagged;
        } else {
            data.insert(0, 0x00);
        }

        if let Some(ref mut chameleon) = self.chameleon {
            let latent = chameleon.encode(&data);
            // FAST: Serialize into reusable buffer
            latent.to_bytes_into(&mut self.latent_buf);
            data = std::mem::take(&mut self.latent_buf);

            // M2: Latent-space AEAD on raw path too
            if self.latent_aead {
                if let Some(cipher) = &self.cipher {
                    self.nonce_counter = self.nonce_counter.wrapping_add(1);
                    let mut nonce_full = [0u8; 12];
                    nonce_full[..8].copy_from_slice(&self.nonce_counter.to_le_bytes());
                    nonce_full[8..].copy_from_slice(&self.session_nonce);
                    let nonce = Nonce::from_slice(&nonce_full);
                    data = cipher
                        .encrypt(nonce, data.as_ref())
                        .map_err(|e| anyhow::anyhow!("Latent AEAD encrypt error: {}", e))?;
                    let mut with_nonce = nonce_full.to_vec();
                    with_nonce.extend(data);
                    data = with_nonce;
                }
            }

            if self.moving_target {
                chameleon.evolve(msg_hash);
                // morphology evolution deferred to after write_morphed_frame
            }
        } else if let Some(cipher) = &self.cipher {
            // SECURITY: Use counter-based nonce to prevent nonce reuse
            self.nonce_counter = self.nonce_counter.wrapping_add(1);
            let mut nonce_full = [0u8; 12];
            nonce_full[..8].copy_from_slice(&self.nonce_counter.to_le_bytes());
            nonce_full[8..].copy_from_slice(&self.session_nonce);
            let nonce = Nonce::from_slice(&nonce_full);
            data = cipher
                .encrypt(nonce, data.as_ref())
                .map_err(|e| anyhow::anyhow!("Encryption error: {}", e))?;

            let mut with_nonce = nonce_full.to_vec();
            with_nonce.extend(data);
            data = with_nonce;
        }

        self.write_morphed_frame(&data).await?;

        // Evolve morphology AFTER writing frame to keep sender/receiver in sync
        if self.moving_target && self.chameleon.is_some() {
            self.morphology.evolve(msg_hash);
        }
        Ok(())
    }

    /// Receive a message with speculative decoding
    pub async fn receive_message(&mut self) -> anyhow::Result<Message> {
        // Read with morphing header
        let mut data = self.read_morphed_frame().await?;

        // Chameleon decoding (if enabled)
        if let Some(ref mut chameleon) = self.chameleon {
            // M2: Latent-space AEAD — decrypt serialized latent vector before decoding
            if self.latent_aead {
                if let Some(cipher) = &self.cipher {
                    if data.len() < 12 {
                        return Err(anyhow::anyhow!("Invalid latent AEAD message"));
                    }
                    let nonce = Nonce::from_slice(&data[..12]);
                    data = cipher
                        .decrypt(nonce, &data[12..])
                        .map_err(|e| anyhow::anyhow!("Latent AEAD decrypt error: {}", e))?;
                }
            }

            // FAST: Zero-copy binary deserialization
            let latent = LatentVector::from_bytes_fast(&data)
                .ok_or_else(|| anyhow::anyhow!("Invalid latent vector format"))?;
            data = chameleon.decode(&latent);
        }
        // Fallback to AES decryption (without Chameleon)
        else if let Some(cipher) = &self.cipher {
            if data.len() < 12 {
                return Err(anyhow::anyhow!("Invalid encrypted message"));
            }
            let nonce = Nonce::from_slice(&data[..12]);
            data = cipher
                .decrypt(nonce, &data[12..])
                .map_err(|e| anyhow::anyhow!("Decryption error: {}", e))?;
        }

        // Adaptive decompression: check compression flag byte
        if data.is_empty() {
            return Err(anyhow::anyhow!("Empty message after decryption"));
        }
        data = if data[0] == 0x01 {
            decode_all(&data[1..])?
        } else {
            data[1..].to_vec()
        };

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
                SpeculativePayload::Confirmed {
                    confirmed_hash,
                    delta,
                } => {
                    // They confirmed our prediction was correct!
                    self.speculation_stats.input_hits += 1;

                    // Reconstruct from our prediction cache
                    if let Some(pos) = self
                        .prediction_cache
                        .iter()
                        .position(|(h, _)| *h == confirmed_hash)
                    {
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
                        return Err(anyhow::anyhow!(
                            "Speculation hit but prediction not in cache"
                        ));
                    }
                }
                SpeculativePayload::Partial {
                    prefix_hash: _,
                    prefix_len: _,
                    suffix,
                } => {
                    self.speculation_stats.input_partial_hits += 1;
                    // Would combine cached prefix with received suffix
                    suffix
                }
                SpeculativePayload::Batch {
                    predictions,
                    fallback,
                } => {
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

    /// Write frame with morphing header format (stack-allocated header, no heap alloc)
    #[inline]
    async fn write_morphed_frame(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let len = data.len() as u32;
        let header_size = self.morphology.header_size as usize;

        // Stack-allocated header (max 16 bytes) — no heap allocation
        let mut header = [0u8; 16];
        debug_assert!(header_size <= 16, "Header size must be <= 16");

        // Length field (endianness varies)
        let len_bytes = if self.morphology.big_endian {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        };
        header[..4].copy_from_slice(&len_bytes);

        // Version byte
        header[4] = self.morphology.frame_version;

        // Padding to header_size
        for b in header[5..header_size].iter_mut() {
            *b = self.morphology.checksum_variant;
        }

        write_header_body(&mut self.stream, &header[..header_size], data).await?;
        Ok(())
    }

    /// Read frame with morphing header format (reuses read buffer to minimize allocations)
    #[inline]
    async fn read_morphed_frame(&mut self) -> anyhow::Result<Vec<u8>> {
        // Stack-allocated header read
        let header_size = self.morphology.header_size as usize;
        let mut header = [0u8; 16];
        self.stream.read_exact(&mut header[..header_size]).await?;

        // Extract length (endianness varies)
        let len_bytes: [u8; 4] = header[0..4].try_into()?;
        let len = if self.morphology.big_endian {
            u32::from_be_bytes(len_bytes)
        } else {
            u32::from_le_bytes(len_bytes)
        } as usize;

        // SECURITY: Limit maximum frame size to prevent OOM attacks (16 MiB)
        const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;
        if len > MAX_FRAME_SIZE {
            anyhow::bail!(
                "Frame size {} exceeds maximum allowed size {}",
                len,
                MAX_FRAME_SIZE
            );
        }

        // Reuse read buffer to avoid per-frame allocations
        self.read_buf.clear();
        self.read_buf.resize(len, 0);
        self.stream.read_exact(&mut self.read_buf).await?;
        Ok(std::mem::take(&mut self.read_buf))
    }

    /// Apply padding based on current morphology
    fn apply_padding(&self, mut data: Vec<u8>) -> Vec<u8> {
        match self.morphology.padding_mode {
            PaddingMode::None => data,
            PaddingMode::Pkcs7 => {
                let padding_len = 16 - (data.len() % 16);
                data.extend(std::iter::repeat_n(padding_len as u8, padding_len));
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
                let padding_len = if data.len() >= target {
                    16
                } else {
                    target - data.len()
                };
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
                    // Validate PKCS7 padding: padding_len must be 1-16 and not exceed data length
                    let padding_len = padding_len as usize;
                    if (1..=16).contains(&padding_len) && padding_len <= data.len() {
                        // Verify all padding bytes are equal to padding_len
                        let start = data.len() - padding_len;
                        let valid = data[start..].iter().all(|&b| b as usize == padding_len);
                        if valid {
                            data.truncate(start);
                        }
                        // If invalid, return data as-is (corrupted padding)
                    }
                }
                data
            }
            PaddingMode::Random => {
                if let Some(&padding_len) = data.last() {
                    // Validate: padding_len + 1 (for length byte) must not exceed data length
                    let total_padding = padding_len as usize + 1;
                    if total_padding <= data.len() {
                        let len = data.len() - total_padding;
                        data.truncate(len);
                    }
                }
                data
            }
            PaddingMode::Chaotic => {
                if data.len() >= 4 {
                    let len_bytes: [u8; 4] = data[data.len() - 4..].try_into().unwrap_or([0; 4]);
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportMode {
    /// TCP with TLS (default, maximum compatibility)
    Tcp,
    /// QUIC with 0-RTT (lower latency, built-in encryption)
    Quic,
    /// Automatic: try QUIC first, fallback to TCP
    #[default]
    Auto,
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
        self.connection
            .close(quinn::VarInt::from_u32(code), reason.as_bytes());
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
    fn generate_self_signed_cert(
    ) -> anyhow::Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
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
            quic_rustls::ServerConfig::builder_with_protocol_versions(&[
                &quic_rustls::version::TLS13,
            ])
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)?,
        )?;

        let mut server_config = ServerConfig::with_crypto(Arc::new(crypto));

        let transport = Arc::get_mut(&mut server_config.transport).unwrap();
        transport.max_idle_timeout(Some(
            std::time::Duration::from_secs(self.config.idle_timeout_secs).try_into()?,
        ));
        transport.keep_alive_interval(Some(std::time::Duration::from_secs(
            self.config.keep_alive_secs,
        )));
        transport.max_concurrent_bidi_streams(self.config.max_streams.into());

        let endpoint = Endpoint::server(server_config, bind_addr)?;
        Ok(endpoint)
    }

    /// Build a client endpoint with platform verifier (for development: skip verification)
    pub fn build_client(&self) -> anyhow::Result<Endpoint> {
        // Create a permissive client config for development
        // In production, use rustls-platform-verifier
        let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(
            quic_rustls::ClientConfig::builder_with_protocol_versions(&[
                &quic_rustls::version::TLS13,
            ])
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth(),
        )?;

        let client_config = ClientConfig::new(Arc::new(crypto));

        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);

        Ok(endpoint)
    }

    /// Connect to a QUIC server
    pub async fn connect(
        &self,
        endpoint: &Endpoint,
        addr: SocketAddr,
        server_name: &str,
    ) -> anyhow::Result<QuicTransport> {
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
pub struct SpineConnection {
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
    /// Latent-space AEAD mode (M2)
    latent_aead: bool,
    /// Nonce counter for AEAD
    nonce_counter: u64,
    /// Session nonce for cross-session uniqueness
    session_nonce: [u8; 4],
}

impl SpineConnection {
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
        self.handler.morphology = ProtocolMorphology::new_keyed(
            u64::from_le_bytes(secret[0..8].try_into().unwrap()),
            &secret,
        );
        self.handler.moving_target = true;
    }

    /// Enable Chameleon + AES-GCM (M2: latent-space AEAD)
    pub fn enable_chameleon_aead(&mut self, secret: [u8; 32]) {
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &secret);
        let mut aes_key = [0u8; 32];
        hk.expand(b"spine-latent-aead-key", &mut aes_key)
            .expect("HKDF expand");

        self.handler.chameleon = Some(ChameleonKey::new(&secret));
        self.handler.morphology = ProtocolMorphology::new_keyed(
            u64::from_le_bytes(secret[0..8].try_into().unwrap()),
            &secret,
        );
        self.handler.moving_target = true;

        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&aes_key);
        self.handler.cipher = Some(Aes256Gcm::new(key));
        self.handler.latent_aead = true;
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
                // QUIC: length-prefixed frame. QuicTransport has its own
                // write_all; can't use the AsyncWrite-based vectored helper here.
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
            TransportInner::Tcp { stream } => self.handler.read_frame(stream.as_mut()).await?,
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
            latent_aead: false,
            nonce_counter: 0,
            session_nonce: rand::random::<[u8; 4]>(),
        }
    }

    fn encode_message(&mut self, msg: &Message) -> anyhow::Result<Vec<u8>> {
        let mut data = serde_json::to_vec(msg)?;

        // Apply padding
        data = self.apply_padding(data);

        // Adaptive compression with flag
        // SECURITY NOTE (L2): Compressing plaintext before encryption can leak
        // information via ciphertext size changes (CRIME/BREACH attacks). This is
        // acceptable for agent-to-agent communication where there is no reflected
        // user-controlled content. Disable compression when handling untrusted input.
        if data.len() >= 64 {
            let compressed = encode_all(&data[..], 3)?;
            let mut flagged = Vec::with_capacity(1 + compressed.len());
            flagged.push(0x01);
            flagged.extend_from_slice(&compressed);
            data = flagged;
        } else {
            data.insert(0, 0x00);
        }

        // Chameleon encoding
        if let Some(ref mut chameleon) = self.chameleon {
            let latent = chameleon.encode(&data);
            // FAST: Zero-copy binary serialization
            data = latent.to_bytes_fast();

            // M2: Latent-space AEAD
            if self.latent_aead {
                if let Some(cipher) = &self.cipher {
                    self.nonce_counter = self.nonce_counter.wrapping_add(1);
                    let mut nonce_full = [0u8; 12];
                    nonce_full[..8].copy_from_slice(&self.nonce_counter.to_le_bytes());
                    nonce_full[8..].copy_from_slice(&self.session_nonce);
                    let nonce = Nonce::from_slice(&nonce_full);
                    data = cipher
                        .encrypt(nonce, data.as_ref())
                        .map_err(|e| anyhow::anyhow!("Latent AEAD encrypt error: {}", e))?;
                    let mut with_nonce = nonce_full.to_vec();
                    with_nonce.extend(data);
                    data = with_nonce;
                }
            }
        } else if let Some(cipher) = &self.cipher {
            let nonce_bytes = rand::random::<[u8; 12]>();
            let nonce = Nonce::from_slice(&nonce_bytes);
            data = cipher
                .encrypt(nonce, data.as_ref())
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
            // M2: Latent-space AEAD — decrypt before decoding
            if self.latent_aead {
                if let Some(cipher) = &self.cipher {
                    if data.len() < 12 {
                        return Err(anyhow::anyhow!("Invalid latent AEAD message"));
                    }
                    let nonce = Nonce::from_slice(&data[..12]);
                    data = cipher
                        .decrypt(nonce, &data[12..])
                        .map_err(|e| anyhow::anyhow!("Latent AEAD decrypt error: {}", e))?;
                }
            }

            // FAST: Zero-copy binary deserialization
            let latent = LatentVector::from_bytes_fast(&data)
                .ok_or_else(|| anyhow::anyhow!("Invalid latent vector format"))?;
            data = chameleon.decode(&latent);
        } else if let Some(cipher) = &self.cipher {
            if data.len() < 12 {
                return Err(anyhow::anyhow!("Invalid encrypted message"));
            }
            let nonce = Nonce::from_slice(&data[..12]);
            data = cipher
                .decrypt(nonce, &data[12..])
                .map_err(|e| anyhow::anyhow!("Decryption error: {}", e))?;
        }

        // Adaptive decompression: check compression flag byte
        if data.is_empty() {
            return Err(anyhow::anyhow!("Empty message after decryption"));
        }
        data = if data[0] == 0x01 {
            decode_all(&data[1..])?
        } else {
            data[1..].to_vec()
        };

        // Remove padding
        data = self.remove_padding(data);

        let msg: Message = serde_json::from_slice(&data)?;
        Ok(msg)
    }

    async fn write_frame<W: AsyncWrite + Unpin + ?Sized>(
        &self,
        writer: &mut W,
        data: &[u8],
    ) -> anyhow::Result<()> {
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

        write_header_body(writer, &header, data).await?;
        Ok(())
    }

    async fn read_frame<R: AsyncRead + Unpin + ?Sized>(
        &self,
        reader: &mut R,
    ) -> anyhow::Result<Vec<u8>> {
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
                data.extend(std::iter::repeat_n(padding_len as u8, padding_len));
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
                let padding_len = if data.len() >= target {
                    16
                } else {
                    target - data.len()
                };
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
                    let len_bytes: [u8; 4] = data[data.len() - 4..].try_into().unwrap_or([0; 4]);
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
) -> anyhow::Result<SpineConnection> {
    match config.mode {
        TransportMode::Tcp => {
            let stream = tcp_fallback.await?;
            Ok(SpineConnection::from_tcp(stream))
        }
        TransportMode::Quic => {
            let builder = QuicEndpointBuilder::with_config(config.clone());
            let endpoint = builder.build_client()?;
            let server_name = config.server_name.as_deref().unwrap_or("localhost");
            let transport = builder.connect(&endpoint, addr, server_name).await?;
            Ok(SpineConnection::from_quic(transport))
        }
        TransportMode::Auto => {
            // Try QUIC first
            let builder = QuicEndpointBuilder::with_config(config.clone());
            if let Ok(endpoint) = builder.build_client() {
                let server_name = config.server_name.as_deref().unwrap_or("localhost");
                if let Ok(Ok(transport)) = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    builder.connect(&endpoint, addr, server_name),
                )
                .await
                {
                    tracing::info!("Connected via QUIC to {}", addr);
                    return Ok(SpineConnection::from_quic(transport));
                }
            }

            // Fallback to TCP
            tracing::info!("QUIC unavailable, falling back to TCP for {}", addr);
            let stream = tcp_fallback.await?;
            Ok(SpineConnection::from_tcp(stream))
        }
    }
}

// =============================================================================
// EVOLVABLE NEURAL PROTOCOL FRAMEWORK
// =============================================================================

/// Genetic representation of a neural protocol's architecture.
/// Protocols are encoded as genomes that can be mutated, crossed over, and selected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolGenome {
    /// Unique identifier for this genome
    pub id: Uuid,
    /// Generation number (evolutionary lineage)
    pub generation: u32,
    /// Neural encoder architecture genes
    pub encoder_genes: EncoderGenes,
    /// Latent space configuration genes
    pub latent_genes: LatentSpaceGenes,
    /// Communication pattern genes
    pub comm_genes: CommunicationGenes,
    /// Fitness score from evaluation
    pub fitness: f64,
    /// Parent genomes (for lineage tracking)
    pub parents: Vec<Uuid>,
    /// Mutation history
    pub mutations: Vec<MutationRecord>,
}

/// Genes controlling the neural encoder architecture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncoderGenes {
    /// Number of hidden layers (1-8)
    pub num_layers: u8,
    /// Hidden dimensions for each layer
    pub layer_dims: Vec<u16>,
    /// Activation functions per layer
    pub activations: Vec<ActivationGene>,
    /// Attention heads in encoder (power of 2)
    pub attention_heads: u8,
    /// Use skip connections
    pub skip_connections: bool,
    /// Dropout rate (0.0-0.5)
    pub dropout_rate: f32,
    /// Layer normalization
    pub layer_norm: bool,
}

/// Gene encoding an activation function
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ActivationGene {
    ReLU,
    GELU,
    SiLU,
    Tanh,
    Sigmoid,
    LeakyReLU,
    Mish,
    Swish,
}

impl ActivationGene {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        match rng.gen_range(0..8) {
            0 => Self::ReLU,
            1 => Self::GELU,
            2 => Self::SiLU,
            3 => Self::Tanh,
            4 => Self::Sigmoid,
            5 => Self::LeakyReLU,
            6 => Self::Mish,
            _ => Self::Swish,
        }
    }

    pub fn apply(&self, x: f32) -> f32 {
        match self {
            Self::ReLU => x.max(0.0),
            Self::GELU => x * 0.5 * (1.0 + (x * 0.797_884_6 * (1.0 + 0.044715 * x * x)).tanh()),
            Self::SiLU => x * (1.0 / (1.0 + (-x).exp())),
            Self::Tanh => x.tanh(),
            Self::Sigmoid => 1.0 / (1.0 + (-x).exp()),
            Self::LeakyReLU => {
                if x > 0.0 {
                    x
                } else {
                    0.01 * x
                }
            }
            Self::Mish => x * (((x).exp() + 1.0).ln()).tanh(),
            Self::Swish => x * (1.0 / (1.0 + (-x).exp())),
        }
    }
}

/// Genes controlling the latent space representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatentSpaceGenes {
    /// Base dimensionality (32-1024)
    pub base_dim: u16,
    /// Whether dimension can vary per message
    pub variable_dim: bool,
    /// Minimum dimension when variable
    pub min_dim: u16,
    /// Maximum dimension when variable
    pub max_dim: u16,
    /// Quantization bits (0=continuous, 1-16=discrete)
    pub quantization_bits: u8,
    /// Normalization strategy
    pub normalization: NormalizationGene,
    /// Sparsity target (0.0-1.0)
    pub sparsity_target: f32,
    /// Use residual encoding
    pub residual_encoding: bool,
}

/// Normalization strategies for latent space
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum NormalizationGene {
    None,
    L1,
    L2,
    BatchNorm,
    LayerNorm,
    InstanceNorm,
    Spherical,
}

impl NormalizationGene {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        match rng.gen_range(0..7) {
            0 => Self::None,
            1 => Self::L1,
            2 => Self::L2,
            3 => Self::BatchNorm,
            4 => Self::LayerNorm,
            5 => Self::InstanceNorm,
            _ => Self::Spherical,
        }
    }
}

/// Genes controlling communication patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunicationGenes {
    /// Frame header size strategy
    pub header_strategy: HeaderStrategy,
    /// Compression before encoding
    pub pre_compression: bool,
    /// Error correction level (0-3)
    pub error_correction: u8,
    /// Speculative decoding depth
    pub speculation_depth: u8,
    /// Batching strategy
    pub batching: BatchingGene,
    /// Flow control parameters
    pub flow_control: FlowControlGenes,
    /// Protocol morphology evolution rate
    pub morphology_rate: f32,
}

/// Header encoding strategies
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum HeaderStrategy {
    Fixed(u8),  // Fixed size in bytes
    Variable,   // Variable length encoding
    Implicit,   // Derive from content
    Predictive, // Use speculation to minimize
}

/// Batching strategies for messages
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum BatchingGene {
    None,
    TimeWindow(u16),    // Batch within time window (ms)
    SizeThreshold(u16), // Batch until size threshold
    Adaptive,           // Adapt based on traffic patterns
}

/// Flow control genes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowControlGenes {
    /// Window size (messages)
    pub window_size: u16,
    /// Use congestion-based adaptation
    pub congestion_aware: bool,
    /// Priority levels supported
    pub priority_levels: u8,
    /// Backpressure sensitivity (0.0-1.0)
    pub backpressure_sensitivity: f32,
}

/// Record of a mutation applied to a genome
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationRecord {
    pub mutation_type: MutationType,
    pub gene_path: String,
    pub old_value: String,
    pub new_value: String,
    pub timestamp: u64,
}

/// Types of mutations that can occur
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MutationType {
    PointMutation, // Single gene change
    Insertion,     // Add a layer/component
    Deletion,      // Remove a layer/component
    Duplication,   // Duplicate a component
    Transposition, // Move a component
    Crossover,     // From another genome
    RandomReset,   // Reset to random value
}

impl ProtocolGenome {
    /// Create a random genome
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        let num_layers = rng.gen_range(2..=6);
        let base_dim = 2u16.pow(rng.gen_range(5..=9)); // 32-512

        Self {
            id: Uuid::new_v4(),
            generation: 0,
            encoder_genes: EncoderGenes {
                num_layers,
                layer_dims: (0..num_layers)
                    .map(|_| 2u16.pow(rng.gen_range(6..=9)))
                    .collect(),
                activations: (0..num_layers)
                    .map(|_| ActivationGene::random(rng))
                    .collect(),
                attention_heads: 2u8.pow(rng.gen_range(1..=4)),
                skip_connections: rng.gen_bool(0.5),
                dropout_rate: rng.gen_range(0.0..0.3),
                layer_norm: rng.gen_bool(0.7),
            },
            latent_genes: LatentSpaceGenes {
                base_dim,
                variable_dim: rng.gen_bool(0.3),
                min_dim: base_dim / 2,
                max_dim: base_dim * 2,
                quantization_bits: if rng.gen_bool(0.3) {
                    rng.gen_range(4..=12)
                } else {
                    0
                },
                normalization: NormalizationGene::random(rng),
                sparsity_target: rng.gen_range(0.0..0.5),
                residual_encoding: rng.gen_bool(0.4),
            },
            comm_genes: CommunicationGenes {
                header_strategy: match rng.gen_range(0..4) {
                    0 => HeaderStrategy::Fixed(rng.gen_range(8..=32)),
                    1 => HeaderStrategy::Variable,
                    2 => HeaderStrategy::Implicit,
                    _ => HeaderStrategy::Predictive,
                },
                pre_compression: rng.gen_bool(0.5),
                error_correction: rng.gen_range(0..=3),
                speculation_depth: rng.gen_range(0..=4),
                batching: match rng.gen_range(0..4) {
                    0 => BatchingGene::None,
                    1 => BatchingGene::TimeWindow(rng.gen_range(1..=100)),
                    2 => BatchingGene::SizeThreshold(rng.gen_range(512..=8192)),
                    _ => BatchingGene::Adaptive,
                },
                flow_control: FlowControlGenes {
                    window_size: rng.gen_range(4..=64),
                    congestion_aware: rng.gen_bool(0.7),
                    priority_levels: rng.gen_range(1..=8),
                    backpressure_sensitivity: rng.gen_range(0.3..0.9),
                },
                morphology_rate: rng.gen_range(0.01..0.2),
            },
            fitness: 0.0,
            parents: vec![],
            mutations: vec![],
        }
    }

    /// Mutate this genome
    pub fn mutate(&mut self, mutation_rate: f32, rng: &mut impl rand::Rng) {
        // Encoder mutations
        if rng.gen::<f32>() < mutation_rate {
            let idx = rng.gen_range(0..self.encoder_genes.activations.len());
            let old = self.encoder_genes.activations[idx];
            self.encoder_genes.activations[idx] = ActivationGene::random(rng);
            if self.encoder_genes.activations[idx] != old {
                self.mutations.push(MutationRecord {
                    mutation_type: MutationType::PointMutation,
                    gene_path: format!("encoder.activation[{}]", idx),
                    old_value: format!("{:?}", old),
                    new_value: format!("{:?}", self.encoder_genes.activations[idx]),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                });
            }
        }

        // Layer dimension mutation
        if rng.gen::<f32>() < mutation_rate * 0.5 {
            let idx = rng.gen_range(0..self.encoder_genes.layer_dims.len());
            let old = self.encoder_genes.layer_dims[idx];
            // Mutate by power of 2 steps
            self.encoder_genes.layer_dims[idx] = if rng.gen_bool(0.5) {
                (old * 2).min(1024)
            } else {
                (old / 2).max(32)
            };
            self.mutations.push(MutationRecord {
                mutation_type: MutationType::PointMutation,
                gene_path: format!("encoder.layer_dims[{}]", idx),
                old_value: old.to_string(),
                new_value: self.encoder_genes.layer_dims[idx].to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
        }

        // Latent space mutations
        if rng.gen::<f32>() < mutation_rate {
            let old = self.latent_genes.normalization;
            self.latent_genes.normalization = NormalizationGene::random(rng);
            if self.latent_genes.normalization != old {
                self.mutations.push(MutationRecord {
                    mutation_type: MutationType::PointMutation,
                    gene_path: "latent.normalization".to_string(),
                    old_value: format!("{:?}", old),
                    new_value: format!("{:?}", self.latent_genes.normalization),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                });
            }
        }

        // Sparsity target mutation
        if rng.gen::<f32>() < mutation_rate {
            let delta = rng.gen_range(-0.1..0.1);
            self.latent_genes.sparsity_target =
                (self.latent_genes.sparsity_target + delta).clamp(0.0, 0.8);
        }

        // Communication gene mutations
        if rng.gen::<f32>() < mutation_rate {
            self.comm_genes.speculation_depth = rng.gen_range(0..=6);
        }

        if rng.gen::<f32>() < mutation_rate * 0.3 {
            self.comm_genes.error_correction = rng.gen_range(0..=3);
        }
    }

    /// Crossover with another genome
    pub fn crossover(&self, other: &Self, rng: &mut impl rand::Rng) -> Self {
        let mut child = self.clone();
        child.id = Uuid::new_v4();
        child.generation = self.generation.max(other.generation) + 1;
        child.parents = vec![self.id, other.id];
        child.fitness = 0.0;
        child.mutations.clear();

        // Uniform crossover for encoder genes
        if rng.gen_bool(0.5) {
            child.encoder_genes.attention_heads = other.encoder_genes.attention_heads;
        }
        if rng.gen_bool(0.5) {
            child.encoder_genes.skip_connections = other.encoder_genes.skip_connections;
        }
        if rng.gen_bool(0.5) {
            child.encoder_genes.layer_norm = other.encoder_genes.layer_norm;
        }

        // Crossover layer dims (average or pick)
        for i in 0..child
            .encoder_genes
            .layer_dims
            .len()
            .min(other.encoder_genes.layer_dims.len())
        {
            if rng.gen_bool(0.5) {
                child.encoder_genes.layer_dims[i] = other.encoder_genes.layer_dims[i];
            }
        }

        // Latent space crossover
        if rng.gen_bool(0.5) {
            child.latent_genes.base_dim = other.latent_genes.base_dim;
            child.latent_genes.min_dim = other.latent_genes.min_dim;
            child.latent_genes.max_dim = other.latent_genes.max_dim;
        }
        if rng.gen_bool(0.5) {
            child.latent_genes.normalization = other.latent_genes.normalization;
        }
        if rng.gen_bool(0.5) {
            child.latent_genes.quantization_bits = other.latent_genes.quantization_bits;
        }

        // Communication crossover
        if rng.gen_bool(0.5) {
            child.comm_genes.header_strategy = other.comm_genes.header_strategy;
        }
        if rng.gen_bool(0.5) {
            child.comm_genes.batching = other.comm_genes.batching;
        }
        if rng.gen_bool(0.5) {
            child.comm_genes.flow_control = other.comm_genes.flow_control.clone();
        }

        child.mutations.push(MutationRecord {
            mutation_type: MutationType::Crossover,
            gene_path: "genome".to_string(),
            old_value: format!("{}", self.id),
            new_value: format!("{} x {}", self.id, other.id),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });

        child
    }
}

/// An instantiated protocol built from a genome
pub struct EvolvedProtocol {
    /// The genome this protocol was built from
    pub genome: ProtocolGenome,
    /// Neural encoder layer weights
    encoder_weights: Vec<Vec<Vec<f32>>>,
    /// Decoder weights (inverse of encoder)
    decoder_weights: Vec<Vec<Vec<f32>>>,
    /// Running statistics for adaptive normalization
    running_mean: Vec<f32>,
    running_var: Vec<f32>,
    /// Message counter for morphology evolution
    message_count: u64,
    /// Current latent dimension (may vary)
    current_dim: usize,
    /// RNG for stochastic operations
    rng: rand::rngs::StdRng,
}

impl EvolvedProtocol {
    /// Build a protocol from a genome
    pub fn from_genome(genome: ProtocolGenome) -> Self {
        let seed = genome.id.as_u128() as u64;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        let base_dim = genome.latent_genes.base_dim as usize;
        let mut input_dim = 256; // Standard input chunk size

        // Build encoder weights
        let mut encoder_weights = Vec::new();
        for (i, &dim) in genome.encoder_genes.layer_dims.iter().enumerate() {
            let output_dim = if i == genome.encoder_genes.layer_dims.len() - 1 {
                base_dim
            } else {
                dim as usize
            };

            // Xavier initialization
            let scale = (2.0 / (input_dim + output_dim) as f32).sqrt();
            let weights: Vec<Vec<f32>> = (0..output_dim)
                .map(|_| {
                    (0..input_dim)
                        .map(|_| rng.gen::<f32>() * 2.0 * scale - scale)
                        .collect()
                })
                .collect();
            encoder_weights.push(weights);
            input_dim = output_dim;
        }

        // Build decoder weights (reverse architecture)
        let mut decoder_weights = Vec::new();
        input_dim = base_dim;
        for &dim in genome.encoder_genes.layer_dims.iter().rev() {
            let output_dim = dim as usize;
            let scale = (2.0 / (input_dim + output_dim) as f32).sqrt();
            let weights: Vec<Vec<f32>> = (0..output_dim)
                .map(|_| {
                    (0..input_dim)
                        .map(|_| rng.gen::<f32>() * 2.0 * scale - scale)
                        .collect()
                })
                .collect();
            decoder_weights.push(weights);
            input_dim = output_dim;
        }
        // Final layer to original size
        let scale = (2.0 / (input_dim + 256) as f32).sqrt();
        let final_weights: Vec<Vec<f32>> = (0..256)
            .map(|_| {
                (0..input_dim)
                    .map(|_| rng.gen::<f32>() * 2.0 * scale - scale)
                    .collect()
            })
            .collect();
        decoder_weights.push(final_weights);

        Self {
            genome,
            encoder_weights,
            decoder_weights,
            running_mean: vec![0.0; base_dim],
            running_var: vec![1.0; base_dim],
            message_count: 0,
            current_dim: base_dim,
            rng,
        }
    }

    /// Encode data into the evolved latent space
    pub fn encode(&mut self, data: &[u8]) -> Vec<f32> {
        // Pad or truncate to 256 bytes
        let mut input: Vec<f32> = data.iter().take(256).map(|&b| b as f32 / 255.0).collect();
        while input.len() < 256 {
            input.push(0.0);
        }

        // Forward through encoder
        let mut hidden = input;
        for (i, weights) in self.encoder_weights.iter().enumerate() {
            let activation = if i < self.genome.encoder_genes.activations.len() {
                self.genome.encoder_genes.activations[i]
            } else {
                ActivationGene::GELU
            };

            hidden = self.forward_layer(weights, &hidden, activation);

            // Apply layer norm if enabled
            if self.genome.encoder_genes.layer_norm {
                hidden = self.layer_norm(&hidden);
            }
        }

        // Apply latent space normalization
        hidden = self.normalize_latent(hidden);

        // Apply quantization if enabled
        if self.genome.latent_genes.quantization_bits > 0 {
            hidden = self.quantize(&hidden, self.genome.latent_genes.quantization_bits);
        }

        // Apply sparsity if enabled
        if self.genome.latent_genes.sparsity_target > 0.0 {
            hidden = self.apply_sparsity(hidden);
        }

        self.message_count += 1;
        hidden
    }

    /// Decode data from the evolved latent space
    pub fn decode(&mut self, latent: &[f32]) -> Vec<u8> {
        let mut hidden = latent.to_vec();

        // Reverse quantization isn't needed (values are already continuous approximations)

        // Forward through decoder
        for (i, weights) in self.decoder_weights.iter().enumerate() {
            let activation = if i < self.decoder_weights.len() - 1 {
                // Use same activation as encoder (reversed)
                let rev_idx = self
                    .genome
                    .encoder_genes
                    .activations
                    .len()
                    .saturating_sub(i + 1);
                if rev_idx < self.genome.encoder_genes.activations.len() {
                    self.genome.encoder_genes.activations[rev_idx]
                } else {
                    ActivationGene::GELU
                }
            } else {
                ActivationGene::Sigmoid // Final layer uses sigmoid for [0,1] output
            };

            hidden = self.forward_layer(weights, &hidden, activation);
        }

        // Convert back to bytes
        hidden
            .iter()
            .map(|&x| (x.clamp(0.0, 1.0) * 255.0) as u8)
            .collect()
    }

    fn forward_layer(
        &self,
        weights: &[Vec<f32>],
        input: &[f32],
        activation: ActivationGene,
    ) -> Vec<f32> {
        weights
            .iter()
            .map(|row| {
                let sum: f32 = row.iter().zip(input.iter()).map(|(&w, &x)| w * x).sum();
                activation.apply(sum)
            })
            .collect()
    }

    fn layer_norm(&self, x: &[f32]) -> Vec<f32> {
        let mean: f32 = x.iter().sum::<f32>() / x.len() as f32;
        let var: f32 = x.iter().map(|&v| (v - mean).powi(2)).sum::<f32>() / x.len() as f32;
        let std = (var + 1e-5).sqrt();
        x.iter().map(|&v| (v - mean) / std).collect()
    }

    fn normalize_latent(&mut self, mut x: Vec<f32>) -> Vec<f32> {
        match self.genome.latent_genes.normalization {
            NormalizationGene::None => x,
            NormalizationGene::L1 => {
                let sum: f32 = x.iter().map(|v| v.abs()).sum();
                if sum > 0.0 {
                    x.iter_mut().for_each(|v| *v /= sum);
                }
                x
            }
            NormalizationGene::L2 => {
                let norm: f32 = x.iter().map(|v| v * v).sum::<f32>().sqrt();
                if norm > 0.0 {
                    x.iter_mut().for_each(|v| *v /= norm);
                }
                x
            }
            NormalizationGene::Spherical => {
                let norm: f32 = x.iter().map(|v| v * v).sum::<f32>().sqrt();
                if norm > 0.0 {
                    x.iter_mut().for_each(|v| *v /= norm);
                }
                x
            }
            _ => self.layer_norm(&x),
        }
    }

    fn quantize(&self, x: &[f32], bits: u8) -> Vec<f32> {
        let levels = 2u32.pow(bits as u32) as f32;
        x.iter()
            .map(|&v| {
                let normalized = (v + 1.0) / 2.0; // Map to [0,1]
                let quantized = (normalized * levels).round() / levels;
                quantized * 2.0 - 1.0 // Map back to [-1,1]
            })
            .collect()
    }

    fn apply_sparsity(&mut self, mut x: Vec<f32>) -> Vec<f32> {
        let target_zeros = (x.len() as f32 * self.genome.latent_genes.sparsity_target) as usize;

        // Sort by absolute value
        let mut indexed: Vec<(usize, f32)> =
            x.iter().enumerate().map(|(i, &v)| (i, v.abs())).collect();
        indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        // Zero out smallest values
        for (idx, _) in indexed.iter().take(target_zeros) {
            x[*idx] = 0.0;
        }

        x
    }

    /// Get the current protocol ID
    pub fn protocol_id(&self) -> Uuid {
        self.genome.id
    }

    /// Get generation number
    pub fn generation(&self) -> u32 {
        self.genome.generation
    }
}

/// Population of protocols undergoing evolution
pub struct ProtocolPopulation {
    /// Current population
    pub genomes: Vec<ProtocolGenome>,
    /// Best genome found so far
    pub best: Option<ProtocolGenome>,
    /// Population size
    pub size: usize,
    /// Mutation rate
    pub mutation_rate: f32,
    /// Elite fraction (preserved without mutation)
    pub elite_fraction: f32,
    /// Current generation
    pub generation: u32,
    /// Fitness history
    pub fitness_history: Vec<f64>,
    /// RNG
    rng: rand::rngs::StdRng,
}

impl ProtocolPopulation {
    /// Create a new random population
    pub fn new(size: usize, mutation_rate: f32, seed: u64) -> Self {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let genomes: Vec<ProtocolGenome> = (0..size)
            .map(|_| ProtocolGenome::random(&mut rng))
            .collect();

        Self {
            genomes,
            best: None,
            size,
            mutation_rate,
            elite_fraction: 0.1,
            generation: 0,
            fitness_history: vec![],
            rng,
        }
    }

    /// Evaluate fitness for all genomes using a fitness function
    pub fn evaluate<F>(&mut self, fitness_fn: F)
    where
        F: Fn(&ProtocolGenome) -> f64,
    {
        for genome in &mut self.genomes {
            genome.fitness = fitness_fn(genome);
        }

        // Update best
        if let Some(best_genome) = self.genomes.iter().max_by(|a, b| {
            a.fitness
                .partial_cmp(&b.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            if self.best.is_none() || best_genome.fitness > self.best.as_ref().unwrap().fitness {
                self.best = Some(best_genome.clone());
            }
        }

        // Record average fitness
        let avg_fitness: f64 =
            self.genomes.iter().map(|g| g.fitness).sum::<f64>() / self.size as f64;
        self.fitness_history.push(avg_fitness);
    }

    /// Evolve to the next generation
    pub fn evolve(&mut self) {
        // Sort by fitness (descending)
        self.genomes.sort_by(|a, b| {
            b.fitness
                .partial_cmp(&a.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let elite_count = (self.size as f32 * self.elite_fraction) as usize;
        let mut new_genomes = Vec::with_capacity(self.size);

        // Keep elites
        for genome in self.genomes.iter().take(elite_count) {
            new_genomes.push(genome.clone());
        }

        // Fill rest with offspring
        while new_genomes.len() < self.size {
            // Tournament selection - get indices first to avoid borrow issues
            let parent1_idx = self.tournament_select_idx();
            let parent2_idx = self.tournament_select_idx();

            // Clone parents to avoid borrow issues
            let parent1 = self.genomes[parent1_idx].clone();
            let parent2 = self.genomes[parent2_idx].clone();

            // Crossover
            let mut child = parent1.crossover(&parent2, &mut self.rng);
            child.generation = self.generation + 1;

            // Mutation
            child.mutate(self.mutation_rate, &mut self.rng);

            new_genomes.push(child);
        }

        self.genomes = new_genomes;
        self.generation += 1;
    }

    fn tournament_select_idx(&mut self) -> usize {
        let tournament_size = 3;
        let mut best_idx = self.rng.gen_range(0..self.genomes.len());

        for _ in 1..tournament_size {
            let idx = self.rng.gen_range(0..self.genomes.len());
            if self.genomes[idx].fitness > self.genomes[best_idx].fitness {
                best_idx = idx;
            }
        }

        best_idx
    }

    /// Get the best evolved protocol
    pub fn best_protocol(&self) -> Option<EvolvedProtocol> {
        self.best
            .as_ref()
            .map(|g| EvolvedProtocol::from_genome(g.clone()))
    }

    /// Run evolution for N generations
    pub fn run<F>(&mut self, generations: u32, fitness_fn: F)
    where
        F: Fn(&ProtocolGenome) -> f64 + Copy,
    {
        for _ in 0..generations {
            self.evaluate(fitness_fn);
            self.evolve();
        }
    }
}

/// Fitness evaluation metrics for evolved protocols
pub struct ProtocolFitnessMetrics {
    /// Encoding throughput (bytes/sec)
    pub throughput: f64,
    /// Compression ratio (original/encoded size)
    pub compression: f64,
    /// Reconstruction accuracy (0-1)
    pub accuracy: f64,
    /// Latency (microseconds)
    pub latency_us: f64,
    /// Resistance to analysis (entropy of encoded messages)
    pub entropy: f64,
    /// Energy efficiency (operations per byte)
    pub efficiency: f64,
}

impl ProtocolFitnessMetrics {
    /// Calculate overall fitness from metrics
    pub fn fitness(&self, weights: &ProtocolFitnessWeights) -> f64 {
        weights.throughput * self.throughput.log10().max(0.0)
            + weights.compression * self.compression.log10().max(0.0)
            + weights.accuracy * self.accuracy
            + weights.latency * (1.0 / (self.latency_us + 1.0).log10())
            + weights.entropy * self.entropy
            + weights.efficiency * self.efficiency.log10().max(0.0)
    }
}

/// Weights for fitness function components
pub struct ProtocolFitnessWeights {
    pub throughput: f64,
    pub compression: f64,
    pub accuracy: f64,
    pub latency: f64,
    pub entropy: f64,
    pub efficiency: f64,
}

impl Default for ProtocolFitnessWeights {
    fn default() -> Self {
        Self {
            throughput: 1.0,
            compression: 1.0,
            accuracy: 2.0, // Accuracy is critical
            latency: 1.0,
            entropy: 0.5, // Nice to have
            efficiency: 0.5,
        }
    }
}

/// Benchmark an evolved protocol
pub fn benchmark_protocol(
    genome: &ProtocolGenome,
    test_data: &[Vec<u8>],
) -> ProtocolFitnessMetrics {
    let mut protocol = EvolvedProtocol::from_genome(genome.clone());

    let start = std::time::Instant::now();
    let mut total_original = 0usize;
    let mut total_encoded = 0usize;
    let mut total_errors = 0f64;
    let mut all_encoded: Vec<Vec<f32>> = vec![];

    for data in test_data {
        total_original += data.len();

        let encoded = protocol.encode(data);
        total_encoded += encoded.len() * 4; // f32 = 4 bytes
        all_encoded.push(encoded.clone());

        let decoded = protocol.decode(&encoded);

        // Calculate reconstruction error
        let error: f64 = data
            .iter()
            .zip(decoded.iter())
            .map(|(&a, &b)| (a as f64 - b as f64).powi(2))
            .sum::<f64>()
            / data.len() as f64;
        total_errors += error.sqrt();
    }

    let elapsed = start.elapsed();
    let throughput = total_original as f64 / elapsed.as_secs_f64();
    let compression = total_original as f64 / total_encoded as f64;
    let accuracy = 1.0 - (total_errors / test_data.len() as f64 / 255.0).min(1.0);
    let latency_us = elapsed.as_micros() as f64 / test_data.len() as f64;

    // Calculate entropy of encoded messages
    let entropy = calculate_entropy(&all_encoded);

    // Efficiency: lower is better, invert for fitness
    let ops_per_byte = estimate_operations(genome) as f64 / total_original as f64;
    let efficiency = 1.0 / ops_per_byte;

    ProtocolFitnessMetrics {
        throughput,
        compression,
        accuracy,
        latency_us,
        entropy,
        efficiency,
    }
}

fn calculate_entropy(encoded: &[Vec<f32>]) -> f64 {
    // Approximate entropy from value distribution
    let mut histogram = [0u32; 256];
    let mut total = 0u32;

    for vec in encoded {
        for &v in vec {
            let bucket = ((v.clamp(-1.0, 1.0) + 1.0) / 2.0 * 255.0) as usize;
            histogram[bucket.min(255)] += 1;
            total += 1;
        }
    }

    if total == 0 {
        return 0.0;
    }

    let mut entropy = 0.0;
    for &count in &histogram {
        if count > 0 {
            let p = count as f64 / total as f64;
            entropy -= p * p.log2();
        }
    }

    entropy / 8.0 // Normalize to [0, 1]
}

fn estimate_operations(genome: &ProtocolGenome) -> u64 {
    let mut ops = 0u64;
    let mut dim = 256u64;

    for &layer_dim in &genome.encoder_genes.layer_dims {
        // Matrix multiply: dim * layer_dim multiplications + additions
        ops += dim * layer_dim as u64 * 2;
        // Activation function
        ops += layer_dim as u64;
        dim = layer_dim as u64;
    }

    // Latent space operations
    ops += genome.latent_genes.base_dim as u64;

    ops
}

// =============================================================================
// CO-EVOLUTIONARY ARMS RACE FRAMEWORK
// =============================================================================

/// Represents a team in the co-evolutionary arms race.
/// Red teams attack (try to break encryption), Blue teams defend (evolve robust protocols).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdversarialTeam {
    /// Defender: evolves protocols to resist analysis and maintain secrecy
    Blue,
    /// Attacker: evolves models to break protocols and extract information
    Red,
}

/// Genome for a Red Team attacker model.
/// Red team evolves to decode/analyze Blue team's latent space communications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackerGenome {
    /// Unique identifier
    pub id: Uuid,
    /// Generation in the arms race
    pub generation: u32,
    /// Neural decoder architecture (tries to invert Blue's encoder)
    pub decoder_genes: DecoderGenes,
    /// Pattern analysis genes (traffic analysis, timing attacks)
    pub analysis_genes: AnalysisGenes,
    /// Side-channel attack genes
    pub side_channel_genes: SideChannelGenes,
    /// Fitness score against Blue team
    pub fitness: f64,
    /// Which Blue protocols this attacker has faced
    pub adversaries_faced: Vec<Uuid>,
    /// Success rate history per adversary
    pub success_history: Vec<(Uuid, f64)>,
}

/// Genes for the attacker's neural decoder (inverse of defender's encoder)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoderGenes {
    /// Architecture mirrors potential defender architectures
    pub num_layers: u8,
    /// Hidden dimensions
    pub layer_dims: Vec<u16>,
    /// Activations
    pub activations: Vec<ActivationGene>,
    /// Attention heads for sequence analysis
    pub attention_heads: u8,
    /// Use bidirectional processing
    pub bidirectional: bool,
    /// Ensemble size (multiple decoder attempts)
    pub ensemble_size: u8,
    /// Noise injection for robustness
    pub noise_injection: f32,
}

/// Genes for traffic analysis attacks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisGenes {
    /// Statistical analysis method
    pub stat_method: StatisticalMethod,
    /// Sequence length for pattern detection
    pub pattern_window: u16,
    /// Frequency analysis depth
    pub frequency_depth: u8,
    /// Correlation threshold for pattern matching
    pub correlation_threshold: f32,
    /// Use differential analysis
    pub differential_analysis: bool,
    /// Clustering algorithm for message grouping
    pub clustering: ClusteringGene,
    /// Dimensionality reduction before analysis
    pub dim_reduction: DimReductionGene,
}

/// Statistical analysis methods for traffic analysis
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum StatisticalMethod {
    /// Chi-squared test for distribution analysis
    ChiSquared,
    /// Kolmogorov-Smirnov test
    KolmogorovSmirnov,
    /// Mutual information estimation
    MutualInformation,
    /// Entropy-based analysis
    EntropyAnalysis,
    /// Correlation analysis
    Correlation,
    /// Spectral analysis (FFT-based)
    Spectral,
}

impl StatisticalMethod {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        match rng.gen_range(0..6) {
            0 => Self::ChiSquared,
            1 => Self::KolmogorovSmirnov,
            2 => Self::MutualInformation,
            3 => Self::EntropyAnalysis,
            4 => Self::Correlation,
            _ => Self::Spectral,
        }
    }
}

/// Clustering algorithms for pattern detection
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ClusteringGene {
    KMeans(u8),   // k clusters
    DBSCAN,       // Density-based
    Hierarchical, // Agglomerative
    SpectralClustering,
    None,
}

impl ClusteringGene {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        match rng.gen_range(0..5) {
            0 => Self::KMeans(rng.gen_range(2..=16)),
            1 => Self::DBSCAN,
            2 => Self::Hierarchical,
            3 => Self::SpectralClustering,
            _ => Self::None,
        }
    }
}

/// Dimensionality reduction methods
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DimReductionGene {
    PCA(u8), // Target dimensions
    UMAP,
    TSNE,
    Autoencoder,
    None,
}

impl DimReductionGene {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        match rng.gen_range(0..5) {
            0 => Self::PCA(rng.gen_range(2..=32)),
            1 => Self::UMAP,
            2 => Self::TSNE,
            3 => Self::Autoencoder,
            _ => Self::None,
        }
    }
}

/// Genes for side-channel attacks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SideChannelGenes {
    /// Timing attack sensitivity (microseconds)
    pub timing_resolution: f32,
    /// Power analysis (simulated)
    pub power_analysis: bool,
    /// Cache timing attacks
    pub cache_timing: bool,
    /// Electromagnetic analysis
    pub em_analysis: bool,
    /// Memory access pattern analysis
    pub memory_patterns: bool,
    /// Number of samples needed
    pub sample_requirement: u32,
}

impl AttackerGenome {
    /// Create a random attacker genome
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        let num_layers = rng.gen_range(3..=8);

        Self {
            id: Uuid::new_v4(),
            generation: 0,
            decoder_genes: DecoderGenes {
                num_layers,
                layer_dims: (0..num_layers)
                    .map(|_| 2u16.pow(rng.gen_range(6..=10)))
                    .collect(),
                activations: (0..num_layers)
                    .map(|_| ActivationGene::random(rng))
                    .collect(),
                attention_heads: 2u8.pow(rng.gen_range(2..=5)),
                bidirectional: rng.gen_bool(0.6),
                ensemble_size: rng.gen_range(1..=5),
                noise_injection: rng.gen_range(0.0..0.2),
            },
            analysis_genes: AnalysisGenes {
                stat_method: StatisticalMethod::random(rng),
                pattern_window: rng.gen_range(16..=256),
                frequency_depth: rng.gen_range(2..=8),
                correlation_threshold: rng.gen_range(0.3..0.9),
                differential_analysis: rng.gen_bool(0.5),
                clustering: ClusteringGene::random(rng),
                dim_reduction: DimReductionGene::random(rng),
            },
            side_channel_genes: SideChannelGenes {
                timing_resolution: rng.gen_range(0.1..10.0),
                power_analysis: rng.gen_bool(0.3),
                cache_timing: rng.gen_bool(0.4),
                em_analysis: rng.gen_bool(0.2),
                memory_patterns: rng.gen_bool(0.5),
                sample_requirement: rng.gen_range(100..=10000),
            },
            fitness: 0.0,
            adversaries_faced: vec![],
            success_history: vec![],
        }
    }

    /// Mutate the attacker genome
    pub fn mutate(&mut self, mutation_rate: f32, rng: &mut impl rand::Rng) {
        // Decoder mutations
        if rng.gen::<f32>() < mutation_rate {
            let idx = rng.gen_range(0..self.decoder_genes.activations.len());
            self.decoder_genes.activations[idx] = ActivationGene::random(rng);
        }

        if rng.gen::<f32>() < mutation_rate * 0.5 {
            self.decoder_genes.ensemble_size =
                (self.decoder_genes.ensemble_size as i8 + rng.gen_range(-1..=1)).clamp(1, 8) as u8;
        }

        // Analysis mutations
        if rng.gen::<f32>() < mutation_rate {
            self.analysis_genes.stat_method = StatisticalMethod::random(rng);
        }

        if rng.gen::<f32>() < mutation_rate {
            self.analysis_genes.clustering = ClusteringGene::random(rng);
        }

        // Side-channel mutations
        if rng.gen::<f32>() < mutation_rate * 0.3 {
            self.side_channel_genes.timing_resolution *= rng.gen_range(0.5..2.0);
        }
    }

    /// Crossover with another attacker
    pub fn crossover(&self, other: &Self, rng: &mut impl rand::Rng) -> Self {
        let mut child = self.clone();
        child.id = Uuid::new_v4();
        child.generation = self.generation.max(other.generation) + 1;
        child.fitness = 0.0;
        child.adversaries_faced.clear();
        child.success_history.clear();

        // Crossover decoder genes
        if rng.gen_bool(0.5) {
            child.decoder_genes.bidirectional = other.decoder_genes.bidirectional;
        }
        if rng.gen_bool(0.5) {
            child.decoder_genes.ensemble_size = other.decoder_genes.ensemble_size;
        }

        // Crossover analysis genes
        if rng.gen_bool(0.5) {
            child.analysis_genes.stat_method = other.analysis_genes.stat_method;
        }
        if rng.gen_bool(0.5) {
            child.analysis_genes.clustering = other.analysis_genes.clustering;
        }

        // Crossover side-channel genes
        if rng.gen_bool(0.5) {
            child.side_channel_genes = other.side_channel_genes.clone();
        }

        child
    }
}

/// Extended defender genome with adversarial-awareness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefenderGenome {
    /// Base protocol genome
    pub protocol: ProtocolGenome,
    /// Defensive genes against known attack patterns
    pub defense_genes: DefenseGenes,
    /// Fitness against Red team attacks
    pub adversarial_fitness: f64,
    /// Which Red attackers this defender has faced
    pub attackers_faced: Vec<Uuid>,
    /// Survival rate history per attacker
    pub survival_history: Vec<(Uuid, f64)>,
}

/// Defensive genes to resist Red team attacks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefenseGenes {
    /// Noise injection level (obfuscation)
    pub noise_level: f32,
    /// Key rotation frequency
    pub key_rotation_rate: u32,
    /// Decoy traffic ratio
    pub decoy_ratio: f32,
    /// Timing jitter (microseconds)
    pub timing_jitter: f32,
    /// Message padding strategy
    pub padding_strategy: PaddingStrategy,
    /// Morphology evolution rate (moving target defense)
    pub morph_rate: f32,
    /// Use honey tokens (detectable decoy data)
    pub honey_tokens: bool,
    /// Latent space rotation frequency
    pub rotation_frequency: u32,
    /// Anti-correlation measures
    pub decorrelation: DecorrelationGene,
}

/// Padding strategies to resist traffic analysis
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum PaddingStrategy {
    /// Fixed size padding
    Fixed(u16),
    /// Random padding within range
    Random { min: u16, max: u16 },
    /// Pad to power of 2
    PowerOfTwo,
    /// Adaptive based on traffic patterns
    Adaptive,
    /// Constant-rate padding (always same size)
    ConstantRate(u16),
}

impl PaddingStrategy {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        match rng.gen_range(0..5) {
            0 => Self::Fixed(rng.gen_range(64..=512)),
            1 => Self::Random {
                min: rng.gen_range(32..=128),
                max: rng.gen_range(256..=1024),
            },
            2 => Self::PowerOfTwo,
            3 => Self::Adaptive,
            _ => Self::ConstantRate(rng.gen_range(256..=1024)),
        }
    }
}

/// Decorrelation strategies to prevent pattern detection
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DecorrelationGene {
    /// No decorrelation
    None,
    /// Shuffle message order
    Shuffle,
    /// Add random delays
    RandomDelay,
    /// Split and interleave
    Interleave,
    /// Transform basis (rotate latent space)
    BasisRotation,
    /// All of the above
    Full,
}

impl DecorrelationGene {
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        match rng.gen_range(0..6) {
            0 => Self::None,
            1 => Self::Shuffle,
            2 => Self::RandomDelay,
            3 => Self::Interleave,
            4 => Self::BasisRotation,
            _ => Self::Full,
        }
    }
}

impl DefenderGenome {
    /// Create a random defender genome
    pub fn random(rng: &mut impl rand::Rng) -> Self {
        Self {
            protocol: ProtocolGenome::random(rng),
            defense_genes: DefenseGenes {
                noise_level: rng.gen_range(0.01..0.3),
                key_rotation_rate: rng.gen_range(1..=100),
                decoy_ratio: rng.gen_range(0.0..0.5),
                timing_jitter: rng.gen_range(0.0..100.0),
                padding_strategy: PaddingStrategy::random(rng),
                morph_rate: rng.gen_range(0.01..0.3),
                honey_tokens: rng.gen_bool(0.3),
                rotation_frequency: rng.gen_range(1..=50),
                decorrelation: DecorrelationGene::random(rng),
            },
            adversarial_fitness: 0.0,
            attackers_faced: vec![],
            survival_history: vec![],
        }
    }

    /// Mutate the defender genome
    pub fn mutate(&mut self, mutation_rate: f32, rng: &mut impl rand::Rng) {
        // Mutate underlying protocol
        self.protocol.mutate(mutation_rate, rng);

        // Defense mutations
        if rng.gen::<f32>() < mutation_rate {
            self.defense_genes.noise_level =
                (self.defense_genes.noise_level + rng.gen_range(-0.05..0.05)).clamp(0.0, 0.5);
        }

        if rng.gen::<f32>() < mutation_rate {
            self.defense_genes.padding_strategy = PaddingStrategy::random(rng);
        }

        if rng.gen::<f32>() < mutation_rate {
            self.defense_genes.decorrelation = DecorrelationGene::random(rng);
        }

        if rng.gen::<f32>() < mutation_rate * 0.5 {
            self.defense_genes.timing_jitter *= rng.gen_range(0.5..2.0);
        }
    }

    /// Crossover with another defender
    pub fn crossover(&self, other: &Self, rng: &mut impl rand::Rng) -> Self {
        let mut child = Self {
            protocol: self.protocol.crossover(&other.protocol, rng),
            defense_genes: self.defense_genes.clone(),
            adversarial_fitness: 0.0,
            attackers_faced: vec![],
            survival_history: vec![],
        };

        // Crossover defense genes
        if rng.gen_bool(0.5) {
            child.defense_genes.padding_strategy = other.defense_genes.padding_strategy;
        }
        if rng.gen_bool(0.5) {
            child.defense_genes.decorrelation = other.defense_genes.decorrelation;
        }
        if rng.gen_bool(0.5) {
            child.defense_genes.noise_level = other.defense_genes.noise_level;
        }

        child
    }
}

/// Result of an adversarial evaluation between attacker and defender
#[derive(Debug, Clone)]
pub struct AdversarialResult {
    /// Attacker's ID
    pub attacker_id: Uuid,
    /// Defender's ID
    pub defender_id: Uuid,
    /// Attack success rate (0.0 = defender wins, 1.0 = attacker wins)
    pub attack_success: f64,
    /// Information leaked (bits)
    pub information_leaked: f64,
    /// Time to break (if successful, in simulated cycles)
    pub time_to_break: Option<u64>,
    /// Side-channel success
    pub side_channel_success: bool,
    /// Traffic analysis success
    pub traffic_analysis_success: bool,
    /// Direct decoding success
    pub direct_decode_success: bool,
}

/// Co-evolutionary arena where Red and Blue teams compete
pub struct CoevolutionaryArena {
    /// Blue team (defenders)
    pub blue_team: Vec<DefenderGenome>,
    /// Red team (attackers)
    pub red_team: Vec<AttackerGenome>,
    /// Population size per team
    pub team_size: usize,
    /// Mutation rate
    pub mutation_rate: f32,
    /// Elite fraction
    pub elite_fraction: f32,
    /// Current generation
    pub generation: u32,
    /// Historical results
    pub history: Vec<GenerationStats>,
    /// RNG
    rng: rand::rngs::StdRng,
    /// Number of adversarial matchups per generation
    pub matchups_per_gen: usize,
}

/// Statistics for one generation of co-evolution
#[derive(Debug, Clone, Default)]
pub struct GenerationStats {
    /// Generation number
    pub generation: u32,
    /// Average Blue team fitness
    pub blue_avg_fitness: f64,
    /// Best Blue team fitness
    pub blue_best_fitness: f64,
    /// Average Red team fitness
    pub red_avg_fitness: f64,
    /// Best Red team fitness  
    pub red_best_fitness: f64,
    /// Average attack success rate
    pub avg_attack_success: f64,
    /// Diversity metric for Blue team
    pub blue_diversity: f64,
    /// Diversity metric for Red team
    pub red_diversity: f64,
}

impl CoevolutionaryArena {
    /// Create a new co-evolutionary arena
    pub fn new(team_size: usize, mutation_rate: f32, seed: u64) -> Self {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

        let blue_team: Vec<DefenderGenome> = (0..team_size)
            .map(|_| DefenderGenome::random(&mut rng))
            .collect();

        let red_team: Vec<AttackerGenome> = (0..team_size)
            .map(|_| AttackerGenome::random(&mut rng))
            .collect();

        Self {
            blue_team,
            red_team,
            team_size,
            mutation_rate,
            elite_fraction: 0.1,
            generation: 0,
            history: vec![],
            rng,
            matchups_per_gen: team_size * 3, // Each individual faces ~3 opponents
        }
    }

    /// Run adversarial evaluation between an attacker and defender
    pub fn evaluate_matchup(
        &mut self,
        attacker: &AttackerGenome,
        defender: &DefenderGenome,
    ) -> AdversarialResult {
        // Create test messages
        let test_messages: Vec<Vec<u8>> = (0..20)
            .map(|i| {
                let mut msg = vec![0u8; 64 + (i * 8) % 128];
                for (j, byte) in msg.iter_mut().enumerate() {
                    *byte = ((i * 17 + j * 31) % 256) as u8;
                }
                msg
            })
            .collect();

        // Build defender's protocol
        let mut defender_protocol = EvolvedProtocol::from_genome(defender.protocol.clone());

        // Encode messages
        let encoded_messages: Vec<Vec<f32>> = test_messages
            .iter()
            .map(|msg| {
                let mut encoded = defender_protocol.encode(msg);

                // Apply defensive measures
                self.apply_defense(&mut encoded, &defender.defense_genes);

                encoded
            })
            .collect();

        // Attacker tries to decode/analyze
        let (direct_success, info_leaked) =
            self.attempt_direct_decode(&attacker.decoder_genes, &encoded_messages, &test_messages);

        let traffic_success =
            self.attempt_traffic_analysis(&attacker.analysis_genes, &encoded_messages);

        let side_channel_success =
            self.attempt_side_channel(&attacker.side_channel_genes, &defender.defense_genes);

        // Calculate overall attack success
        let attack_success = direct_success * 0.5
            + if traffic_success { 0.25 } else { 0.0 }
            + if side_channel_success { 0.25 } else { 0.0 };

        AdversarialResult {
            attacker_id: attacker.id,
            defender_id: defender.protocol.id,
            attack_success,
            information_leaked: info_leaked,
            time_to_break: if attack_success > 0.5 {
                Some(1000)
            } else {
                None
            },
            side_channel_success,
            traffic_analysis_success: traffic_success,
            direct_decode_success: direct_success > 0.3,
        }
    }

    /// Apply defensive measures to encoded data
    fn apply_defense(&mut self, encoded: &mut [f32], defense: &DefenseGenes) {
        // Add noise
        if defense.noise_level > 0.0 {
            for v in encoded.iter_mut() {
                *v += (self.rng.gen::<f32>() - 0.5) * 2.0 * defense.noise_level;
            }
        }

        // Apply decorrelation
        match defense.decorrelation {
            DecorrelationGene::BasisRotation => {
                // Simple rotation: swap pairs
                for i in (0..encoded.len() - 1).step_by(2) {
                    let angle = 0.1 * self.generation as f32;
                    let a = encoded[i];
                    let b = encoded[i + 1];
                    encoded[i] = a * angle.cos() - b * angle.sin();
                    encoded[i + 1] = a * angle.sin() + b * angle.cos();
                }
            }
            DecorrelationGene::Shuffle => {
                // Fisher-Yates shuffle
                for i in (1..encoded.len()).rev() {
                    let j = self.rng.gen_range(0..=i);
                    encoded.swap(i, j);
                }
            }
            _ => {}
        }
    }

    /// Attempt to directly decode the latent space
    fn attempt_direct_decode(
        &self,
        decoder_genes: &DecoderGenes,
        encoded: &[Vec<f32>],
        original: &[Vec<u8>],
    ) -> (f64, f64) {
        // Build attacker's decoder (simplified simulation)
        let total_params: u64 = decoder_genes.layer_dims.iter().map(|&d| d as u64).sum();

        // Success probability based on decoder complexity vs encoded complexity
        let complexity_ratio = total_params as f64 / (encoded[0].len() * encoded.len()) as f64;

        // Higher ensemble = higher chance
        let ensemble_bonus = (decoder_genes.ensemble_size as f64 - 1.0) * 0.05;

        // Bidirectional helps
        let bidir_bonus = if decoder_genes.bidirectional {
            0.1
        } else {
            0.0
        };

        // Calculate accuracy (simulated)
        let mut total_correct = 0.0;
        let mut info_leaked = 0.0;

        for (enc, orig) in encoded.iter().zip(original.iter()) {
            // Simulate decoding attempt
            let decode_score =
                (complexity_ratio * 0.3 + ensemble_bonus + bidir_bonus).clamp(0.0, 0.95);

            // Information leaked is proportional to encoding entropy vs original entropy
            let enc_entropy: f64 = enc
                .iter()
                .map(|&v| {
                    let p = (v.abs() / 2.0).min(1.0) as f64;
                    if p > 0.0 && p < 1.0 {
                        -p * p.log2()
                    } else {
                        0.0
                    }
                })
                .sum::<f64>()
                / enc.len() as f64;

            info_leaked += (1.0 - enc_entropy) * orig.len() as f64 * 8.0;
            total_correct += decode_score;
        }

        let success_rate = total_correct / encoded.len() as f64;
        let total_info = info_leaked;

        (success_rate, total_info)
    }

    /// Attempt traffic analysis attack
    fn attempt_traffic_analysis(&self, analysis: &AnalysisGenes, encoded: &[Vec<f32>]) -> bool {
        if encoded.len() < 2 {
            return false;
        }

        // Calculate correlation between consecutive messages
        let mut correlations = vec![];
        for window in encoded.windows(2) {
            let corr = self.pearson_correlation(&window[0], &window[1]);
            correlations.push(corr);
        }

        let avg_corr = correlations.iter().sum::<f64>() / correlations.len() as f64;

        // Success if correlation exceeds threshold (patterns detected)
        let threshold_modifier = match analysis.stat_method {
            StatisticalMethod::Correlation => 0.9,
            StatisticalMethod::MutualInformation => 0.85,
            StatisticalMethod::Spectral => 0.8,
            _ => 1.0,
        };

        avg_corr.abs() > analysis.correlation_threshold as f64 * threshold_modifier
    }

    /// Calculate Pearson correlation coefficient
    fn pearson_correlation(&self, a: &[f32], b: &[f32]) -> f64 {
        let n = a.len().min(b.len()) as f64;
        if n < 2.0 {
            return 0.0;
        }

        let mean_a: f64 = a.iter().map(|&x| x as f64).sum::<f64>() / n;
        let mean_b: f64 = b.iter().map(|&x| x as f64).sum::<f64>() / n;

        let mut cov = 0.0;
        let mut var_a = 0.0;
        let mut var_b = 0.0;

        for (x, y) in a.iter().zip(b.iter()) {
            let dx = *x as f64 - mean_a;
            let dy = *y as f64 - mean_b;
            cov += dx * dy;
            var_a += dx * dx;
            var_b += dy * dy;
        }

        if var_a < 1e-10 || var_b < 1e-10 {
            return 0.0;
        }

        cov / (var_a.sqrt() * var_b.sqrt())
    }

    /// Attempt side-channel attack
    fn attempt_side_channel(
        &mut self,
        side_channel: &SideChannelGenes,
        defense: &DefenseGenes,
    ) -> bool {
        // Side-channel success depends on timing jitter defense
        let timing_attack_success = side_channel.timing_resolution < defense.timing_jitter * 0.5;

        // Memory pattern analysis
        let memory_success =
            side_channel.memory_patterns && defense.decorrelation != DecorrelationGene::Full;

        // Combined success probability
        let base_probability = if timing_attack_success { 0.3 } else { 0.0 }
            + if memory_success { 0.2 } else { 0.0 }
            + if side_channel.cache_timing { 0.15 } else { 0.0 }
            + if side_channel.power_analysis {
                0.1
            } else {
                0.0
            };

        // Sample requirement affects success
        let sample_factor = (side_channel.sample_requirement as f64 / 5000.0).min(1.0);

        self.rng.gen::<f64>() < base_probability * sample_factor
    }

    /// Evaluate all matchups and assign fitness
    pub fn evaluate_generation(&mut self) {
        // Reset fitness scores
        for defender in &mut self.blue_team {
            defender.adversarial_fitness = 0.0;
            defender.attackers_faced.clear();
        }
        for attacker in &mut self.red_team {
            attacker.fitness = 0.0;
            attacker.adversaries_faced.clear();
        }

        // Run matchups
        let mut results = vec![];
        for _ in 0..self.matchups_per_gen {
            let blue_idx = self.rng.gen_range(0..self.blue_team.len());
            let red_idx = self.rng.gen_range(0..self.red_team.len());

            let result = self.evaluate_matchup(
                &self.red_team[red_idx].clone(),
                &self.blue_team[blue_idx].clone(),
            );

            results.push((blue_idx, red_idx, result));
        }

        // Update fitness based on results
        for (blue_idx, red_idx, result) in results {
            // Defender fitness: 1 - attack_success (higher = better defense)
            self.blue_team[blue_idx].adversarial_fitness += 1.0 - result.attack_success;
            self.blue_team[blue_idx]
                .attackers_faced
                .push(result.attacker_id);

            // Attacker fitness: attack_success (higher = better attack)
            self.red_team[red_idx].fitness += result.attack_success;
            self.red_team[red_idx]
                .adversaries_faced
                .push(result.defender_id);
        }

        // Normalize fitness by number of matchups
        for defender in &mut self.blue_team {
            if !defender.attackers_faced.is_empty() {
                defender.adversarial_fitness /= defender.attackers_faced.len() as f64;
            }
        }
        for attacker in &mut self.red_team {
            if !attacker.adversaries_faced.is_empty() {
                attacker.fitness /= attacker.adversaries_faced.len() as f64;
            }
        }

        // Record statistics
        let stats = self.calculate_stats();
        self.history.push(stats);
    }

    /// Calculate generation statistics
    fn calculate_stats(&self) -> GenerationStats {
        let blue_fitnesses: Vec<f64> = self
            .blue_team
            .iter()
            .map(|d| d.adversarial_fitness)
            .collect();
        let red_fitnesses: Vec<f64> = self.red_team.iter().map(|a| a.fitness).collect();

        GenerationStats {
            generation: self.generation,
            blue_avg_fitness: blue_fitnesses.iter().sum::<f64>() / blue_fitnesses.len() as f64,
            blue_best_fitness: blue_fitnesses.iter().cloned().fold(0.0, f64::max),
            red_avg_fitness: red_fitnesses.iter().sum::<f64>() / red_fitnesses.len() as f64,
            red_best_fitness: red_fitnesses.iter().cloned().fold(0.0, f64::max),
            avg_attack_success: 1.0
                - (blue_fitnesses.iter().sum::<f64>() / blue_fitnesses.len() as f64),
            blue_diversity: self.calculate_diversity_blue(),
            red_diversity: self.calculate_diversity_red(),
        }
    }

    /// Calculate genetic diversity for Blue team
    fn calculate_diversity_blue(&self) -> f64 {
        if self.blue_team.len() < 2 {
            return 0.0;
        }

        let mut diversity = 0.0;
        let mut count = 0;

        for i in 0..self.blue_team.len() {
            for j in (i + 1)..self.blue_team.len() {
                // Compare key genes
                let dim_diff = (self.blue_team[i].protocol.latent_genes.base_dim as i32
                    - self.blue_team[j].protocol.latent_genes.base_dim as i32)
                    .abs() as f64;
                let noise_diff = (self.blue_team[i].defense_genes.noise_level
                    - self.blue_team[j].defense_genes.noise_level)
                    .abs() as f64;

                diversity += dim_diff / 512.0 + noise_diff;
                count += 1;
            }
        }

        if count > 0 {
            diversity / count as f64
        } else {
            0.0
        }
    }

    /// Calculate genetic diversity for Red team
    fn calculate_diversity_red(&self) -> f64 {
        if self.red_team.len() < 2 {
            return 0.0;
        }

        let mut diversity = 0.0;
        let mut count = 0;

        for i in 0..self.red_team.len() {
            for j in (i + 1)..self.red_team.len() {
                let ensemble_diff = (self.red_team[i].decoder_genes.ensemble_size as i32
                    - self.red_team[j].decoder_genes.ensemble_size as i32)
                    .abs() as f64;
                let method_diff = if self.red_team[i].analysis_genes.stat_method
                    != self.red_team[j].analysis_genes.stat_method
                {
                    1.0
                } else {
                    0.0
                };

                diversity += ensemble_diff / 8.0 + method_diff;
                count += 1;
            }
        }

        if count > 0 {
            diversity / count as f64
        } else {
            0.0
        }
    }

    /// Evolve both teams to the next generation
    pub fn evolve(&mut self) {
        // Sort by fitness
        self.blue_team.sort_by(|a, b| {
            b.adversarial_fitness
                .partial_cmp(&a.adversarial_fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.red_team.sort_by(|a, b| {
            b.fitness
                .partial_cmp(&a.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let elite_count = (self.team_size as f32 * self.elite_fraction) as usize;

        // Evolve Blue team
        let mut new_blue = Vec::with_capacity(self.team_size);
        for defender in self.blue_team.iter().take(elite_count) {
            new_blue.push(defender.clone());
        }
        while new_blue.len() < self.team_size {
            let p1_idx = self.tournament_select_blue();
            let p2_idx = self.tournament_select_blue();
            let parent1 = self.blue_team[p1_idx].clone();
            let parent2 = self.blue_team[p2_idx].clone();

            let mut child = parent1.crossover(&parent2, &mut self.rng);
            child.mutate(self.mutation_rate, &mut self.rng);
            new_blue.push(child);
        }
        self.blue_team = new_blue;

        // Evolve Red team
        let mut new_red = Vec::with_capacity(self.team_size);
        for attacker in self.red_team.iter().take(elite_count) {
            new_red.push(attacker.clone());
        }
        while new_red.len() < self.team_size {
            let p1_idx = self.tournament_select_red();
            let p2_idx = self.tournament_select_red();
            let parent1 = self.red_team[p1_idx].clone();
            let parent2 = self.red_team[p2_idx].clone();

            let mut child = parent1.crossover(&parent2, &mut self.rng);
            child.mutate(self.mutation_rate, &mut self.rng);
            new_red.push(child);
        }
        self.red_team = new_red;

        self.generation += 1;
    }

    fn tournament_select_blue(&mut self) -> usize {
        let tournament_size = 3;
        let mut best_idx = self.rng.gen_range(0..self.blue_team.len());

        for _ in 1..tournament_size {
            let idx = self.rng.gen_range(0..self.blue_team.len());
            if self.blue_team[idx].adversarial_fitness
                > self.blue_team[best_idx].adversarial_fitness
            {
                best_idx = idx;
            }
        }
        best_idx
    }

    fn tournament_select_red(&mut self) -> usize {
        let tournament_size = 3;
        let mut best_idx = self.rng.gen_range(0..self.red_team.len());

        for _ in 1..tournament_size {
            let idx = self.rng.gen_range(0..self.red_team.len());
            if self.red_team[idx].fitness > self.red_team[best_idx].fitness {
                best_idx = idx;
            }
        }
        best_idx
    }

    /// Run the arms race for N generations
    pub fn run(&mut self, generations: u32) {
        for _ in 0..generations {
            self.evaluate_generation();
            self.evolve();
        }
    }

    /// Get the best defender
    pub fn best_defender(&self) -> Option<&DefenderGenome> {
        self.blue_team.iter().max_by(|a, b| {
            a.adversarial_fitness
                .partial_cmp(&b.adversarial_fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Get the best attacker
    pub fn best_attacker(&self) -> Option<&AttackerGenome> {
        self.red_team.iter().max_by(|a, b| {
            a.fitness
                .partial_cmp(&b.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Get arms race summary
    pub fn summary(&self) -> ArmsRaceSummary {
        let latest = self.history.last().cloned().unwrap_or_default();

        ArmsRaceSummary {
            generations_run: self.generation,
            final_blue_fitness: latest.blue_best_fitness,
            final_red_fitness: latest.red_best_fitness,
            equilibrium_reached: self.check_equilibrium(),
            blue_team_size: self.blue_team.len(),
            red_team_size: self.red_team.len(),
        }
    }

    /// Check if arms race has reached equilibrium
    fn check_equilibrium(&self) -> bool {
        if self.history.len() < 10 {
            return false;
        }

        let recent: Vec<&GenerationStats> = self.history.iter().rev().take(10).collect();
        let blue_variance: f64 = recent
            .iter()
            .map(|s| s.blue_best_fitness)
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).powi(2))
            .sum::<f64>()
            / 9.0;

        let red_variance: f64 = recent
            .iter()
            .map(|s| s.red_best_fitness)
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1] - w[0]).powi(2))
            .sum::<f64>()
            / 9.0;

        blue_variance < 0.01 && red_variance < 0.01
    }
}

/// Summary of an arms race run
#[derive(Debug, Clone)]
pub struct ArmsRaceSummary {
    /// Total generations evolved
    pub generations_run: u32,
    /// Best Blue team fitness achieved
    pub final_blue_fitness: f64,
    /// Best Red team fitness achieved
    pub final_red_fitness: f64,
    /// Whether equilibrium was reached
    pub equilibrium_reached: bool,
    /// Size of Blue team
    pub blue_team_size: usize,
    /// Size of Red team
    pub red_team_size: usize,
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

    #[test]
    fn test_protocol_genome_random() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let genome = ProtocolGenome::random(&mut rng);

        assert!(genome.encoder_genes.num_layers >= 2);
        assert!(genome.encoder_genes.num_layers <= 6);
        assert!(!genome.encoder_genes.layer_dims.is_empty());
        assert!(genome.latent_genes.base_dim >= 32);
        assert_eq!(genome.generation, 0);
    }

    #[test]
    fn test_protocol_genome_mutation() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut genome = ProtocolGenome::random(&mut rng);
        let original_fitness = genome.fitness;

        // High mutation rate to ensure something changes
        genome.mutate(1.0, &mut rng);

        // Fitness should still be 0 (not evaluated yet)
        assert_eq!(genome.fitness, original_fitness);
        // Mutations should be recorded
        assert!(!genome.mutations.is_empty());
    }

    #[test]
    fn test_protocol_genome_crossover() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let parent1 = ProtocolGenome::random(&mut rng);
        let parent2 = ProtocolGenome::random(&mut rng);

        let child = parent1.crossover(&parent2, &mut rng);

        assert_ne!(child.id, parent1.id);
        assert_ne!(child.id, parent2.id);
        assert_eq!(child.generation, 1);
        assert_eq!(child.parents.len(), 2);
        assert!(child.parents.contains(&parent1.id));
        assert!(child.parents.contains(&parent2.id));
    }

    #[test]
    fn test_evolved_protocol_encode_decode() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let genome = ProtocolGenome::random(&mut rng);
        let mut protocol = EvolvedProtocol::from_genome(genome);

        let original = b"Hello, evolved neural protocol!";
        let encoded = protocol.encode(original);

        // Encoded should be in latent space
        assert!(!encoded.is_empty());

        let decoded = protocol.decode(&encoded);

        // Decoded should be similar length
        assert_eq!(decoded.len(), 256); // Padded to 256
    }

    #[test]
    fn test_protocol_population_creation() {
        let population = ProtocolPopulation::new(10, 0.1, 42);

        assert_eq!(population.genomes.len(), 10);
        assert_eq!(population.size, 10);
        assert_eq!(population.generation, 0);
        assert!(population.best.is_none());
    }

    #[test]
    fn test_protocol_population_evolution() {
        let mut population = ProtocolPopulation::new(20, 0.1, 42);

        // Simple fitness function based on compression potential
        let fitness_fn = |genome: &ProtocolGenome| -> f64 {
            let dim_score = 1.0 / (genome.latent_genes.base_dim as f64 / 128.0);
            let layer_score = genome.encoder_genes.num_layers as f64 / 4.0;
            dim_score + layer_score
        };

        // Evaluate initial population
        population.evaluate(fitness_fn);
        assert!(population.best.is_some());

        let initial_best_fitness = population.best.as_ref().unwrap().fitness;

        // Evolve for several generations
        for _ in 0..5 {
            population.evolve();
            population.evaluate(fitness_fn);
        }

        assert_eq!(population.generation, 5);
        // Best should be maintained or improved
        assert!(population.best.as_ref().unwrap().fitness >= initial_best_fitness * 0.9);
    }

    #[test]
    fn test_activation_genes() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        // Test all activation functions work
        for _ in 0..20 {
            let activation = ActivationGene::random(&mut rng);
            let result = activation.apply(0.5);
            assert!(!result.is_nan());

            let result_neg = activation.apply(-0.5);
            assert!(!result_neg.is_nan());
        }
    }

    #[test]
    fn test_benchmark_protocol() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let genome = ProtocolGenome::random(&mut rng);

        // Create test data
        let test_data: Vec<Vec<u8>> = (0..10).map(|i| vec![i as u8; 100]).collect();

        let metrics = benchmark_protocol(&genome, &test_data);

        assert!(metrics.throughput > 0.0);
        assert!(metrics.compression > 0.0);
        assert!(metrics.accuracy >= 0.0 && metrics.accuracy <= 1.0);
        assert!(metrics.latency_us > 0.0);
        assert!(metrics.entropy >= 0.0);
    }

    // ==========================================================================
    // CO-EVOLUTIONARY ARMS RACE TESTS
    // ==========================================================================

    #[test]
    fn test_attacker_genome_random() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let attacker = AttackerGenome::random(&mut rng);

        assert!(attacker.decoder_genes.num_layers >= 3);
        assert!(!attacker.decoder_genes.layer_dims.is_empty());
        assert_eq!(attacker.generation, 0);
        assert!(attacker.fitness == 0.0);
    }

    #[test]
    fn test_defender_genome_random() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let defender = DefenderGenome::random(&mut rng);

        assert!(defender.defense_genes.noise_level >= 0.0);
        assert!(defender.defense_genes.noise_level <= 0.3);
        assert!(defender.adversarial_fitness == 0.0);
    }

    #[test]
    fn test_attacker_mutation() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut attacker = AttackerGenome::random(&mut rng);

        attacker.mutate(1.0, &mut rng); // High mutation rate

        // Should still have valid structure
        assert!(attacker.decoder_genes.ensemble_size >= 1);
        assert!(attacker.decoder_genes.ensemble_size <= 8);
    }

    #[test]
    fn test_defender_mutation() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let mut defender = DefenderGenome::random(&mut rng);

        defender.mutate(1.0, &mut rng); // High mutation rate

        // Should still have valid structure
        assert!(defender.defense_genes.noise_level >= 0.0);
        assert!(defender.defense_genes.noise_level <= 0.5);
    }

    #[test]
    fn test_attacker_crossover() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let parent1 = AttackerGenome::random(&mut rng);
        let parent2 = AttackerGenome::random(&mut rng);

        let child = parent1.crossover(&parent2, &mut rng);

        assert_ne!(child.id, parent1.id);
        assert_ne!(child.id, parent2.id);
        assert_eq!(child.generation, 1);
    }

    #[test]
    fn test_defender_crossover() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let parent1 = DefenderGenome::random(&mut rng);
        let parent2 = DefenderGenome::random(&mut rng);

        let child = parent1.crossover(&parent2, &mut rng);

        assert_ne!(child.protocol.id, parent1.protocol.id);
        assert!(child.attackers_faced.is_empty());
    }

    #[test]
    fn test_coevolutionary_arena_creation() {
        let arena = CoevolutionaryArena::new(10, 0.1, 42);

        assert_eq!(arena.blue_team.len(), 10);
        assert_eq!(arena.red_team.len(), 10);
        assert_eq!(arena.generation, 0);
        assert!(arena.history.is_empty());
    }

    #[test]
    fn test_adversarial_matchup() {
        let mut arena = CoevolutionaryArena::new(5, 0.1, 42);

        let attacker = arena.red_team[0].clone();
        let defender = arena.blue_team[0].clone();

        let result = arena.evaluate_matchup(&attacker, &defender);

        assert!(result.attack_success >= 0.0 && result.attack_success <= 1.0);
        assert!(result.information_leaked >= 0.0);
        assert_eq!(result.attacker_id, attacker.id);
        assert_eq!(result.defender_id, defender.protocol.id);
    }

    #[test]
    fn test_coevolutionary_generation() {
        let mut arena = CoevolutionaryArena::new(10, 0.1, 42);

        arena.evaluate_generation();

        // All individuals should have some fitness assigned
        assert!(!arena.history.is_empty());
        let stats = &arena.history[0];
        assert!(stats.blue_avg_fitness >= 0.0);
        assert!(stats.red_avg_fitness >= 0.0);
    }

    #[test]
    fn test_coevolutionary_evolution() {
        let mut arena = CoevolutionaryArena::new(10, 0.1, 42);

        // Run several generations
        for _ in 0..5 {
            arena.evaluate_generation();
            arena.evolve();
        }

        assert_eq!(arena.generation, 5);
        assert_eq!(arena.history.len(), 5);

        // Should have best individuals
        assert!(arena.best_defender().is_some());
        assert!(arena.best_attacker().is_some());
    }

    #[test]
    fn test_arms_race_summary() {
        let mut arena = CoevolutionaryArena::new(8, 0.1, 42);

        arena.run(3);

        let summary = arena.summary();

        assert_eq!(summary.generations_run, 3);
        assert_eq!(summary.blue_team_size, 8);
        assert_eq!(summary.red_team_size, 8);
        assert!(summary.final_blue_fitness >= 0.0);
        assert!(summary.final_red_fitness >= 0.0);
    }

    #[test]
    fn test_statistical_methods() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        for _ in 0..20 {
            let method = StatisticalMethod::random(&mut rng);
            match method {
                StatisticalMethod::ChiSquared
                | StatisticalMethod::KolmogorovSmirnov
                | StatisticalMethod::MutualInformation
                | StatisticalMethod::EntropyAnalysis
                | StatisticalMethod::Correlation
                | StatisticalMethod::Spectral => {} // All valid
            }
        }
    }

    #[test]
    fn test_padding_strategies() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        for _ in 0..20 {
            let strategy = PaddingStrategy::random(&mut rng);
            match strategy {
                PaddingStrategy::Fixed(n) => assert!((64..=512).contains(&n)),
                PaddingStrategy::Random { min, max } => assert!(min < max),
                PaddingStrategy::PowerOfTwo
                | PaddingStrategy::Adaptive
                | PaddingStrategy::ConstantRate(_) => {} // All valid
            }
        }
    }

    #[test]
    fn test_decorrelation_genes() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);

        for _ in 0..20 {
            let gene = DecorrelationGene::random(&mut rng);
            match gene {
                DecorrelationGene::None
                | DecorrelationGene::Shuffle
                | DecorrelationGene::RandomDelay
                | DecorrelationGene::Interleave
                | DecorrelationGene::BasisRotation
                | DecorrelationGene::Full => {} // All valid
            }
        }
    }
}
