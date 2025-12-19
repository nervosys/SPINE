use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use aes_gcm::{Aes256Gcm, Key, Nonce, KeyInit, aead::Aead};
use zstd::stream::encode_all;
use zstd::stream::decode_all;
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use hyperlight_neural::NeuralLatentEncoder;
use hyperlight_crypto::{TransformerPredictor, TransformerConfig, QuantumSpeculativeProtocol, LatticeParams};

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
    /// Neural latent encoder
    encoder: NeuralLatentEncoder,
    /// Quantum-resistant key evolution
    quantum: QuantumSpeculativeProtocol,
    /// Dimension of the latent space
    dimension: usize,
    /// Message counter for key evolution
    epoch: u64,
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
            encoder: NeuralLatentEncoder::new(dimension, 1024, &[256, 128], 10, seed),
            quantum: QuantumSpeculativeProtocol::new(config, params, seed),
            dimension,
            epoch: 0,
        }
    }
    
    /// Encode data into the latent space
    pub fn encode(&mut self, data: &[u8]) -> LatentVector {
        let components = self.encoder.encode(data);
        
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
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum BrowserCommand {
    Navigate { url: String },
    GetUR,
    GetRawHTML,
    Click { element_id: String },
    Type { element_id: String, text: String },
    ExecuteBinary(HyperlightBinary),
    /// Request latent-encoded response
    GetLatentUR { dimensions: usize },
    /// Trigger protocol morphing
    Morph,
    /// Semantic search in the current page
    Search { query: String },
    /// Transfer session to another node
    TransferSession { target_node_id: Uuid },
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
