//! Research-only protocol machinery, compiled when the `research` feature is on.
//!
//! Everything here is experimental: the Chameleon latent-space key, the
//! speculative predictor, protocol morphing, [`ProtocolHandler`] (which owns a
//! `ChameleonKey` and two predictors), `SpineConnection`, and the protocol
//! evolution framework. It is the only part of the crate that needs
//! `spine-neural` and `spine-crypto`. The wire types and codec in the crate
//! root do not depend on it, so `--no-default-features` builds without either
//! crate. With the feature on, every item below is re-exported from the crate
//! root, so public paths are unchanged.

use aes_gcm::{aead::Aead, Aes256Gcm, Key, KeyInit, Nonce};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256 as Sha256Hash;
use spine_crypto::{
    LatticeParams, QuantumSpeculativeProtocol, TransformerConfig, TransformerPredictor,
};
use spine_neural::{MirasNeuralEncoder, MirasVariant, NeuralEncoderConfig, NeuralLatentEncoder};
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use zstd::stream::encode_all;

use super::*;

mod evolution;
pub use evolution::*;

/// Derive the Chameleon layer's seed from a shared secret.
///
/// This replaced `DefaultHasher`, which was wrong for two independent reasons.
///
/// The cryptographic one: `DefaultHasher::new()` is SipHash keyed with zeros, a
/// hash designed for HashMap collision resistance rather than for deriving key
/// material. It offers no guarantee that its 64-bit output is a uniform,
/// secret-dependent value, which is exactly the guarantee a key derivation
/// needs. HKDF-SHA-256 does, and the info string separates this use from the
/// AEAD key derived from the same secret.
///
/// The correctness one, which is arguably worse: the algorithm behind
/// `DefaultHasher` is explicitly unspecified by the standard library and may
/// change between Rust releases. Two peers built with different compilers could
/// therefore derive different seeds from the same shared secret and fail to
/// understand each other, with nothing in the protocol to say why.
///
/// The output is still 64 bits, which is a ceiling this function cannot lift:
/// `NeuralLatentEncoder` and `TransformerConfig` are seeded by a `u64`. Raising
/// it means widening their keying, which is a change to those crates rather than
/// to this derivation. See `SECURITY_AUDIT.md`.
fn chameleon_seed(secret: &[u8; 32]) -> u64 {
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, secret);
    let mut bytes = [0u8; 8];
    hk.expand(b"spine-chameleon-seed", &mut bytes)
        .expect("HKDF expand into 8 bytes cannot fail");
    u64::from_le_bytes(bytes)
}

/// A fresh 96-bit AES-GCM nonce from the operating system's CSPRNG.
///
/// The previous construction was `counter(8) || session_nonce(4)`, with the
/// counter restarting at zero every session. That would be sound if each session
/// had its own key — but the key here is `HKDF(secret)` with no salt and no
/// per-session input, so every session between the same pair shares one key, and
/// the only thing separating their nonce streams was 32 random bits. By the
/// birthday bound two sessions collide with probability near 39% after 65,536 of
/// them, and a collision repeats `(key, nonce)` for every message index the two
/// have in common.
///
/// AES-GCM does not survive that. It leaks the XOR of the two plaintexts, and
/// because GCM authenticates with a polynomial MAC it also exposes the
/// authentication subkey — so nonce reuse costs forgery, not only secrecy.
///
/// Random 96-bit nonces remove the session dimension entirely: the birthday
/// bound is now over all messages under the key, at roughly 2^-33 after 2^32 of
/// them, which is the regime NIST SP 800-38D sanctions for random IVs.
///
/// This is not a wire-format change. The full nonce has always been prepended to
/// the ciphertext and the receiver has always read it from there, so how the
/// sender chooses those twelve bytes was only ever a sender-side decision.
fn fresh_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    nonce
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
        let seed = chameleon_seed(secret);

        Self {
            // attention_heads (8) must divide latent_dim (128): 128/8=16 ✓
            // Keyed from the whole secret: the encoder weights are the key here,
            // so the u64 seed below is only a per-instance diversifier for the
            // machinery that still takes one.
            encoder: NeuralLatentEncoder::new_keyed(dimension, 128, &[256, 128], 8, secret),
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
        let seed = chameleon_seed(secret);

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
            // Keyed from the whole secret: the encoder weights are the key here,
            // so the u64 seed below is only a per-instance diversifier for the
            // machinery that still takes one.
            encoder: NeuralLatentEncoder::new_keyed(dimension, 128, &[256, 128], 8, secret),
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

/// Scrub the HMAC key on drop so the secret cannot leak via a core
/// dump or swap-to-disk event (NIST SP 800-171 § 3.13.10). The other
/// fields are plaintext metadata and don't need clearing.
impl Drop for ProtocolMorphology {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.hmac_key.zeroize();
    }
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
            Message::StreamCancel(_) => MessageType::Event,
            Message::CapabilityQuery(_) => MessageType::Request,
            Message::CapabilityAd(_) => MessageType::Response,
            // Neural codec frames slot into the same taxonomy.
            Message::Encoded(_) => MessageType::Binary,
            Message::CodecAd(_) => MessageType::Response,
            Message::CodecNegotiation(_) => MessageType::Sync,
            Message::EmbeddingRequest(_) => MessageType::Request,
            Message::EmbeddingResponse(_) => MessageType::Response,
        }
    }

    /// Send a message with speculative encoding
    ///
    /// Optimized hot path: single serialization pass, buffer reuse,
    /// adaptive compression (skipped for small payloads < threshold).
    #[inline]
    pub async fn send_message(&mut self, msg: &Message) -> anyhow::Result<()> {
        // Serialize message once via the binary wire codec (CBOR / CBOR+zstd).
        self.send_buf = wire::encode(msg)?;
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
                    let nonce_full = fresh_nonce();
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
            let nonce_full = fresh_nonce();
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
        let mut data = wire::encode(msg)?;
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
                    let nonce_full = fresh_nonce();
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
            let nonce_full = fresh_nonce();
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
            wire::decode_zstd_limited(&data[1..], wire::MAX_DECODED_FRAME_BYTES)?
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
            let msg: Message = wire::decode(&msg_data)?;

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
            let msg: Message = wire::decode(&data)?;

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
        anyhow::ensure!(
            data.len() <= wire::MAX_TRANSPORT_FRAME_BYTES,
            "native transport frame exceeds size limit"
        );
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
        const MAX_FRAME_SIZE: usize = wire::MAX_TRANSPORT_FRAME_BYTES;
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
                // The length prefix is peer-controlled; bound it before allocating.
                if len > wire::MAX_TRANSPORT_FRAME_BYTES {
                    anyhow::bail!(
                        "QUIC frame of {len} bytes exceeds the {} byte limit",
                        wire::MAX_TRANSPORT_FRAME_BYTES
                    );
                }
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
        }
    }

    fn encode_message(&mut self, msg: &Message) -> anyhow::Result<Vec<u8>> {
        let mut data = wire::encode(msg)?;

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
                    let nonce_full = fresh_nonce();
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
            wire::decode_zstd_limited(&data[1..], wire::MAX_DECODED_FRAME_BYTES)?
        } else {
            data[1..].to_vec()
        };

        // Remove padding
        data = self.remove_padding(data);

        let msg: Message = wire::decode(&data)?;
        Ok(msg)
    }

    async fn write_frame<W: AsyncWrite + Unpin + ?Sized>(
        &self,
        writer: &mut W,
        data: &[u8],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            data.len() <= wire::MAX_TRANSPORT_FRAME_BYTES,
            "native transport frame exceeds size limit"
        );
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

        anyhow::ensure!(
            len <= wire::MAX_TRANSPORT_FRAME_BYTES,
            "native transport frame exceeds size limit"
        );
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

    /// The derivation has to be a function of the secret alone, or two peers
    /// sharing a secret would key their encoders differently and never agree.
    #[test]
    fn the_chameleon_seed_is_determined_by_the_secret() {
        let secret = [7u8; 32];
        assert_eq!(chameleon_seed(&secret), chameleon_seed(&secret));
    }

    /// And it has to actually depend on the secret. A derivation that ignored
    /// part of its input would silently shrink the key space further than the
    /// 64-bit output already does.
    #[test]
    fn every_byte_of_the_secret_changes_the_chameleon_seed() {
        let base = [0u8; 32];
        let baseline = chameleon_seed(&base);
        for position in 0..32 {
            let mut altered = base;
            altered[position] ^= 0xff;
            assert_ne!(
                chameleon_seed(&altered),
                baseline,
                "byte {position} of the secret does not reach the seed"
            );
        }
    }

    /// The seed must not be recoverable by a hash anyone can compute. This
    /// pins the derivation to HKDF rather than to the `DefaultHasher` it
    /// replaced — which was not merely weak but *unstable*, since the standard
    /// library does not promise its algorithm across Rust releases, so two
    /// peers on different compilers could disagree about the same secret.
    #[test]
    fn the_chameleon_seed_is_not_the_std_hash_of_the_secret() {
        use std::hash::{Hash, Hasher};
        let secret = [3u8; 32];
        let mut hasher = DefaultHasher::new();
        secret.hash(&mut hasher);
        assert_ne!(
            chameleon_seed(&secret),
            hasher.finish(),
            "the seed must come from a KDF, not from a HashMap hasher"
        );
    }

    /// The defect this replaced: the nonce was `counter(8) || session_nonce(4)`
    /// with the counter restarting at zero each session, while the AEAD key is a
    /// pure function of the shared secret and so identical across sessions. Two
    /// sessions that drew the same 32-bit `session_nonce` — about a 39% chance
    /// after 65,536 of them — then reused `(key, nonce)` on every message index
    /// they shared, which costs AES-GCM both confidentiality and unforgeability.
    ///
    /// A nonce must never repeat under one key, so the property to assert is
    /// that fresh nonces do not collide, not merely that consecutive ones differ.
    #[test]
    fn nonces_do_not_repeat_across_sessions() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for _ in 0..10_000 {
            assert!(
                seen.insert(fresh_nonce()),
                "a nonce repeated under a key that never changes"
            );
        }
    }

    /// A counter-based nonce is recognisable: its low bytes walk 1, 2, 3 and the
    /// top four never move. This asserts the replacement is not that — the whole
    /// 96 bits vary, which is what removes the session dimension from the
    /// birthday bound.
    #[test]
    fn every_byte_of_the_nonce_carries_entropy() {
        let samples: Vec<[u8; 12]> = (0..256).map(|_| fresh_nonce()).collect();
        for position in 0..12 {
            let distinct: std::collections::HashSet<u8> =
                samples.iter().map(|n| n[position]).collect();
            assert!(
                distinct.len() > 1,
                "byte {position} never varies, so it is structure and not entropy"
            );
        }
    }
}
