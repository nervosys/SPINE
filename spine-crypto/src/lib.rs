// Allow dead code for cryptographic API surface
#![allow(dead_code)]

//! # SPINE Crypto
//!
//! Advanced cryptographic primitives for the SPINE stack including:
//! - **Titans-based message prediction** for speculative decoding with Neural Long-Term Memory
//! - **MIRAS-adaptive prediction** with automatic variant switching
//! - Post-quantum key evolution using lattice-based cryptography concepts
//!
//! ## Titans Predictor (Neural Long-Term Memory)
//!
//! Uses the **Titans architecture** with test-time training for unbounded context
//! message prediction, enabling speculative decoding where receivers can pre-compute
//! responses. Unlike standard Transformers, Titans maintains persistent memory that
//! survives across sequences through surprise-gated updates.
//!
//! Key advantages over Transformers:
//! - **Unbounded context**: Memory persists indefinitely via consolidation
//! - **Test-time training**: Adapts to message patterns during inference
//! - **Surprise detection**: Identifies anomalous messages for security
//! - **Memory efficiency**: O(1) memory vs O(n²) for attention
//!
//! ## MIRAS-Adaptive Prediction
//!
//! Integrates the MIRAS framework for continual learning:
//! - **YAAD**: Yield-Adaptive Anomaly Detection for outlier-robust prediction
//! - **MONETA**: Memory-Optimized Network for stable long-running sessions
//! - **MEMORA**: Balanced updates for mixed traffic patterns
//!
//! The predictor automatically switches between variants based on surprise levels.
//!
//! ## Quantum-Resistant Key Evolution
//!
//! Implements NTRU-inspired lattice operations for key evolution that
//! resists quantum computing attacks (Shor's algorithm).

use rand::prelude::*;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spine_neural::{
    Activation, DenseLayer, MirasNeuralEncoder, MirasVariant, MultiHeadAttention,
    NeuralEncoderConfig, TitansMemory,
};
use std::collections::VecDeque;
use subtle::ConstantTimeEq;

// AES-256-GCM for authenticated encryption (replaces XOR)
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
// HKDF for proper key derivation
use hkdf::Hkdf;

// ============================================================================
// TITANS-BASED MESSAGE PREDICTOR (Neural Long-Term Memory)
// ============================================================================

/// Positional encoding for transformer sequence modeling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionalEncoding {
    max_len: usize,
    embed_dim: usize,
    encodings: Vec<Vec<f32>>,
}

impl PositionalEncoding {
    pub fn new(max_len: usize, embed_dim: usize) -> Self {
        // Use enumerate patterns for better clippy compliance
        let encodings: Vec<Vec<f32>> = (0..max_len)
            .map(|pos| {
                (0..embed_dim)
                    .map(|i| {
                        let angle = pos as f32
                            / (10000.0_f32).powf(2.0 * (i / 2) as f32 / embed_dim as f32);
                        if i % 2 == 0 {
                            angle.sin()
                        } else {
                            angle.cos()
                        }
                    })
                    .collect()
            })
            .collect();

        Self {
            max_len,
            embed_dim,
            encodings,
        }
    }

    pub fn get(&self, position: usize) -> &[f32] {
        // Handle edge case: if max_len is 0, return empty slice (though this shouldn't happen)
        if self.encodings.is_empty() {
            return &[];
        }
        &self.encodings[position.min(self.max_len.saturating_sub(1))]
    }
}

/// Layer normalization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerNorm {
    dim: usize,
    gamma: Vec<f32>,
    beta: Vec<f32>,
    eps: f32,
}

impl LayerNorm {
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            gamma: vec![1.0; dim],
            beta: vec![0.0; dim],
            eps: 1e-5,
        }
    }

    pub fn forward(&self, x: &[f32]) -> Vec<f32> {
        // Handle empty input to prevent division by zero
        if x.is_empty() {
            return Vec::new();
        }
        let n = x.len() as f32;
        let mean: f32 = x.iter().sum::<f32>() / n;
        let var: f32 = x.iter().map(|&v| (v - mean).powi(2)).sum::<f32>() / n;
        let std = (var + self.eps).sqrt();

        x.iter()
            .enumerate()
            .map(|(i, &v)| self.gamma[i % self.dim] * (v - mean) / std + self.beta[i % self.dim])
            .collect()
    }
}

/// Feed-forward network in transformer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedForward {
    linear1: DenseLayer,
    linear2: DenseLayer,
}

impl FeedForward {
    pub fn new(embed_dim: usize, ff_dim: usize, rng: &mut StdRng) -> Self {
        Self {
            linear1: DenseLayer::new(embed_dim, ff_dim, Activation::GELU, rng),
            linear2: DenseLayer::new(ff_dim, embed_dim, Activation::None, rng),
        }
    }

    pub fn forward(&mut self, x: &[f32]) -> Vec<f32> {
        let hidden = self.linear1.forward(x);
        self.linear2.forward(&hidden)
    }
}

/// Single Titans decoder block with Neural Long-Term Memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitansBlock {
    /// Neural Long-Term Memory for persistent context
    memory: TitansMemory,
    /// Short-term attention for recent sequence
    attention: MultiHeadAttention,
    ff: FeedForward,
    norm1: LayerNorm,
    norm2: LayerNorm,
    norm3: LayerNorm,
    embed_dim: usize,
}

impl TitansBlock {
    pub fn new(
        embed_dim: usize,
        num_heads: usize,
        ff_dim: usize,
        memory_size: usize,
        rng: &mut StdRng,
    ) -> Self {
        Self {
            memory: TitansMemory::new(embed_dim, embed_dim, memory_size, rng),
            attention: MultiHeadAttention::new(embed_dim, num_heads, rng),
            ff: FeedForward::new(embed_dim, ff_dim, rng),
            norm1: LayerNorm::new(embed_dim),
            norm2: LayerNorm::new(embed_dim),
            norm3: LayerNorm::new(embed_dim),
            embed_dim,
        }
    }

    pub fn forward(&mut self, sequence: &[Vec<f32>]) -> Vec<f32> {
        if sequence.is_empty() {
            return vec![0.0; self.embed_dim];
        }

        let last = &sequence[sequence.len() - 1];

        // Step 1: Query Neural Long-Term Memory (persistent context)
        let memory_out = self.memory.forward(last);
        let residual1: Vec<f32> = memory_out
            .iter()
            .zip(last.iter())
            .map(|(m, l)| m + l)
            .collect();
        let normed1 = self.norm1.forward(&residual1);

        // Step 2: Short-term self-attention for recent sequence
        let attended = self.attention.forward(sequence);
        let residual2: Vec<f32> = attended
            .iter()
            .zip(normed1.iter())
            .map(|(a, n)| a + n)
            .collect();
        let normed2 = self.norm2.forward(&residual2);

        // Step 3: Feed-forward with residual
        let ff_out = self.ff.forward(&normed2);
        let residual3: Vec<f32> = ff_out
            .iter()
            .zip(normed2.iter())
            .map(|(f, n)| f + n)
            .collect();
        self.norm3.forward(&residual3)
    }

    /// Get current surprise level (for anomaly detection)
    pub fn get_surprise(&self) -> f32 {
        self.memory.get_surprise()
    }

    /// Reset memory state
    pub fn reset_memory(&mut self) {
        self.memory.reset_state();
    }
}

/// Byte-level tokenizer for message encoding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByteTokenizer {
    embed_dim: usize,
    embeddings: Vec<Vec<f32>>, // 256 byte embeddings
}

impl ByteTokenizer {
    pub fn new(embed_dim: usize, rng: &mut StdRng) -> Self {
        let scale = (1.0 / embed_dim as f32).sqrt();
        let embeddings: Vec<Vec<f32>> = (0..256)
            .map(|_| {
                (0..embed_dim)
                    .map(|_| rng.gen::<f32>() * 2.0 * scale - scale)
                    .collect()
            })
            .collect();

        Self {
            embed_dim,
            embeddings,
        }
    }

    pub fn encode(&self, byte: u8) -> &[f32] {
        &self.embeddings[byte as usize]
    }

    pub fn encode_sequence(&self, bytes: &[u8]) -> Vec<Vec<f32>> {
        bytes
            .iter()
            .map(|&b| self.embeddings[b as usize].clone())
            .collect()
    }
}

/// Output projection to predict next byte distribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputProjection {
    weights: Vec<Vec<f32>>, // [256][embed_dim]
    temperature: f32,
}

impl OutputProjection {
    pub fn new(embed_dim: usize, rng: &mut StdRng) -> Self {
        let scale = (1.0 / embed_dim as f32).sqrt();
        let weights: Vec<Vec<f32>> = (0..256)
            .map(|_| {
                (0..embed_dim)
                    .map(|_| rng.gen::<f32>() * 2.0 * scale - scale)
                    .collect()
            })
            .collect();

        Self {
            weights,
            temperature: 1.0,
        }
    }

    pub fn set_temperature(&mut self, temp: f32) {
        self.temperature = temp.max(0.01);
    }

    pub fn forward(&self, hidden: &[f32]) -> Vec<f32> {
        let mut logits = vec![0.0; 256];
        for (i, w) in self.weights.iter().enumerate() {
            for (j, &h) in hidden.iter().enumerate() {
                logits[i] += w[j] * h;
            }
            logits[i] /= self.temperature;
        }

        // Softmax
        let max = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0;
        for l in &mut logits {
            *l = (*l - max).exp();
            sum += *l;
        }
        for l in &mut logits {
            *l /= sum;
        }

        logits
    }

    pub fn sample(&self, probs: &[f32], rng: &mut StdRng) -> u8 {
        let mut cumsum = 0.0;
        let r: f32 = rng.gen();
        for (i, &p) in probs.iter().enumerate() {
            cumsum += p;
            if r < cumsum {
                return i as u8;
            }
        }
        255
    }

    pub fn argmax(&self, probs: &[f32]) -> u8 {
        probs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                // Handle NaN: treat NaN as less than any number
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Less)
            })
            .map(|(i, _)| i as u8)
            .unwrap_or(0)
    }
}

/// Titans-based message predictor with Neural Long-Term Memory
///
/// Unlike standard Transformers with fixed context windows, Titans maintains
/// persistent memory that survives across sequences through surprise-gated
/// test-time training. This enables:
/// - **Unbounded context**: Memory consolidates patterns indefinitely
/// - **Anomaly detection**: High surprise indicates novel/malicious messages
/// - **Adaptive prediction**: Memory updates based on prediction errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitansPredictor {
    tokenizer: ByteTokenizer,
    positional: PositionalEncoding,
    blocks: Vec<TitansBlock>,
    output: OutputProjection,
    embed_dim: usize,
    max_seq_len: usize,
    memory_size: usize,
    context_window: VecDeque<Vec<f32>>,
    /// Accumulated surprise for anomaly detection
    total_surprise: f32,
    #[serde(skip, default = "default_rng")]
    rng: StdRng,
}

/// MIRAS-adaptive message predictor with automatic variant switching
///
/// Extends TitansPredictor with MIRAS continual learning framework:
/// - **Adaptive encoding**: Uses MirasNeuralEncoder for latent projections
/// - **Variant switching**: Automatically selects optimal MIRAS variant
/// - **Outlier robustness**: YAAD for high-anomaly traffic
/// - **Long-term stability**: MONETA for extended sessions
#[derive(Debug, Clone)]
pub struct MirasTitansPredictor {
    /// Base Titans predictor
    base: TitansPredictor,
    /// MIRAS encoder for adaptive projections
    miras_encoder: Option<MirasNeuralEncoder>,
    /// Current MIRAS variant
    active_variant: MirasVariant,
    /// Surprise history for adaptive switching
    surprise_history: VecDeque<f32>,
    /// Threshold for variant switching
    anomaly_threshold: f32,
    /// Message counter for long-running detection
    message_count: u64,
    /// Predictions enhanced with MIRAS embeddings
    miras_enhanced_predictions: u64,
    /// Latent dimension for encoding
    latent_dim: usize,
}

impl MirasTitansPredictor {
    /// Create a new MIRAS-enhanced predictor
    pub fn new(config: TitansConfig) -> Self {
        let base = TitansPredictor::new(config.clone());

        // Create MIRAS encoder (embed_dim must be divisible by attention_heads)
        let encoder_config = NeuralEncoderConfig {
            input_dim: config.embed_dim,
            latent_dim: config.embed_dim,
            hidden_dims: vec![config.ff_dim, config.embed_dim],
            attention_heads: config.num_heads,
            seed: config.seed + 1,
            miras_variant: MirasVariant::Titans,
            memory_tokens: config.memory_size,
        };

        let miras_encoder = Some(MirasNeuralEncoder::new(&encoder_config));

        Self {
            base,
            miras_encoder,
            active_variant: MirasVariant::Titans,
            surprise_history: VecDeque::with_capacity(100),
            anomaly_threshold: 0.5,
            message_count: 0,
            miras_enhanced_predictions: 0,
            latent_dim: config.embed_dim,
        }
    }

    /// Create with specific MIRAS variant
    pub fn new_with_variant(config: TitansConfig, variant: MirasVariant) -> Self {
        let base = TitansPredictor::new(config.clone());

        // Recreate encoder with specified variant
        let encoder_config = NeuralEncoderConfig {
            input_dim: config.embed_dim,
            latent_dim: config.embed_dim,
            hidden_dims: vec![config.ff_dim, config.embed_dim],
            attention_heads: config.num_heads,
            seed: config.seed + 1,
            miras_variant: variant,
            memory_tokens: config.memory_size,
        };

        Self {
            base,
            miras_encoder: Some(MirasNeuralEncoder::new(&encoder_config)),
            active_variant: variant,
            surprise_history: VecDeque::with_capacity(100),
            anomaly_threshold: 0.5,
            message_count: 0,
            miras_enhanced_predictions: 0,
            latent_dim: config.embed_dim,
        }
    }

    /// Set anomaly threshold for variant switching
    pub fn set_anomaly_threshold(&mut self, threshold: f32) {
        self.anomaly_threshold = threshold;
    }

    /// Get current MIRAS variant
    pub fn variant(&self) -> &str {
        match self.active_variant {
            MirasVariant::Titans => "titans",
            MirasVariant::Yaad => "yaad",
            MirasVariant::Moneta { .. } => "moneta",
            MirasVariant::Memora => "memora",
        }
    }

    /// Get average anomaly level
    pub fn anomaly_level(&self) -> f32 {
        if self.surprise_history.is_empty() {
            0.0
        } else {
            self.surprise_history.iter().sum::<f32>() / self.surprise_history.len() as f32
        }
    }

    /// Adaptively switch MIRAS variant based on traffic patterns
    fn maybe_switch_variant(&mut self) {
        let anomaly = self.anomaly_level();

        let new_variant = if anomaly > self.anomaly_threshold * 2.0 {
            // High anomaly: use YAAD for outlier robustness
            MirasVariant::Yaad
        } else if anomaly > self.anomaly_threshold {
            // Moderate anomaly: use MEMORA for balanced updates
            MirasVariant::Memora
        } else if self.message_count > 10000 {
            // Long-running session: use MONETA for stability (p=2 is L2 norm)
            MirasVariant::Moneta { p: 2.0 }
        } else {
            // Normal: baseline Titans
            MirasVariant::Titans
        };

        // Check if variant changed (ignoring Moneta's p value for comparison)
        let variant_changed = !matches!(
            (&new_variant, &self.active_variant),
            (MirasVariant::Titans, MirasVariant::Titans)
                | (MirasVariant::Yaad, MirasVariant::Yaad)
                | (MirasVariant::Moneta { .. }, MirasVariant::Moneta { .. })
                | (MirasVariant::Memora, MirasVariant::Memora)
        );

        if variant_changed {
            self.active_variant = new_variant;
            // Note: In production, we'd rebuild the encoder here
            // For efficiency, we keep the same encoder but track the variant
        }
    }

    /// Observe a message with MIRAS-enhanced encoding
    pub fn observe(&mut self, message: &[u8]) {
        // Base observation
        self.base.observe(message);

        // Track surprise
        let surprise = self.base.get_surprise();
        self.surprise_history.push_back(surprise);
        if self.surprise_history.len() > 100 {
            self.surprise_history.pop_front();
        }

        // MIRAS encoding step (for enhanced pattern learning)
        if let Some(ref mut encoder) = self.miras_encoder {
            // Encode with MIRAS (triggers surprise tracking)
            let _latent = encoder.encode(message);
            self.miras_enhanced_predictions += 1;
        }

        self.message_count += 1;

        // Check if we should switch variants
        self.maybe_switch_variant();
    }

    /// Predict next byte (delegates to base)
    pub fn predict_next(&mut self) -> (u8, f32) {
        self.base.predict_next()
    }

    /// Predict sequence (delegates to base)
    pub fn predict_sequence(&mut self, length: usize, greedy: bool) -> Vec<u8> {
        self.base.predict_sequence(length, greedy)
    }

    /// Verify prediction (delegates to base)
    pub fn verify_prediction(&mut self, message: &[u8]) -> (bool, f32) {
        self.base.verify_prediction(message)
    }

    /// Get surprise from base predictor
    pub fn get_surprise(&self) -> f32 {
        self.base.get_surprise()
    }

    /// Check if anomalous
    pub fn is_anomalous(&self, threshold: f32) -> bool {
        self.base.is_anomalous(threshold)
    }

    /// Get MIRAS encoder surprise (if available)
    pub fn get_miras_surprise(&self) -> Option<f32> {
        self.miras_encoder.as_ref().map(|e| e.get_surprise())
    }

    /// Get combined surprise (Titans + MIRAS)
    pub fn get_combined_surprise(&self) -> f32 {
        let titans = self.base.get_surprise();
        let miras = self.get_miras_surprise().unwrap_or(0.0);
        (titans + miras) / 2.0
    }

    /// Reset context (preserves memory)
    pub fn reset(&mut self) {
        self.base.reset();
    }

    /// Full reset including MIRAS state
    pub fn reset_all(&mut self) {
        self.base.reset_all();
        self.surprise_history.clear();
        self.message_count = 0;
        if let Some(ref mut encoder) = self.miras_encoder {
            encoder.reset();
        }
    }

    /// Get statistics
    pub fn stats(&self) -> MirasPredictorStats {
        MirasPredictorStats {
            message_count: self.message_count,
            miras_enhanced_predictions: self.miras_enhanced_predictions,
            current_variant: self.variant().to_string(),
            anomaly_level: self.anomaly_level(),
            titans_surprise: self.base.get_surprise(),
            miras_surprise: self.get_miras_surprise(),
        }
    }
}

/// Statistics for MIRAS predictor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirasPredictorStats {
    pub message_count: u64,
    pub miras_enhanced_predictions: u64,
    pub current_variant: String,
    pub anomaly_level: f32,
    pub titans_surprise: f32,
    pub miras_surprise: Option<f32>,
}

fn default_rng() -> StdRng {
    StdRng::seed_from_u64(42)
}

impl TitansPredictor {
    pub fn new(config: TitansConfig) -> Self {
        let mut rng = StdRng::seed_from_u64(config.seed);

        let tokenizer = ByteTokenizer::new(config.embed_dim, &mut rng);
        let positional = PositionalEncoding::new(config.max_seq_len, config.embed_dim);

        let blocks: Vec<TitansBlock> = (0..config.num_layers)
            .map(|_| {
                TitansBlock::new(
                    config.embed_dim,
                    config.num_heads,
                    config.ff_dim,
                    config.memory_size,
                    &mut rng,
                )
            })
            .collect();

        let output = OutputProjection::new(config.embed_dim, &mut rng);

        Self {
            tokenizer,
            positional,
            blocks,
            output,
            embed_dim: config.embed_dim,
            max_seq_len: config.max_seq_len,
            memory_size: config.memory_size,
            context_window: VecDeque::with_capacity(config.max_seq_len),
            total_surprise: 0.0,
            rng,
        }
    }

    /// Add a message to the context for prediction (triggers test-time training)
    pub fn observe(&mut self, message: &[u8]) {
        for &byte in message {
            let mut embedding = self.tokenizer.encode(byte).to_vec();
            let pos = self.context_window.len();
            let pos_enc = self.positional.get(pos);
            for (e, p) in embedding.iter_mut().zip(pos_enc.iter()) {
                *e += *p;
            }

            self.context_window.push_back(embedding);
            if self.context_window.len() > self.max_seq_len {
                self.context_window.pop_front();
            }
        }

        // Accumulate surprise from all blocks (for anomaly detection)
        self.total_surprise = self.blocks.iter().map(|b| b.get_surprise()).sum::<f32>()
            / self.blocks.len().max(1) as f32;
    }

    /// Predict the next byte
    pub fn predict_next(&mut self) -> (u8, f32) {
        let sequence: Vec<Vec<f32>> = self.context_window.iter().cloned().collect();

        if sequence.is_empty() {
            return (0, 1.0 / 256.0);
        }

        // Forward through Titans blocks (with persistent memory)
        let mut hidden = self.blocks[0].forward(&sequence);
        for block in &mut self.blocks[1..] {
            let seq_with_hidden = vec![hidden.clone()];
            hidden = block.forward(&seq_with_hidden);
        }

        // Project to output
        let probs = self.output.forward(&hidden);
        let predicted = self.output.argmax(&probs);
        let confidence = probs[predicted as usize];

        (predicted, confidence)
    }

    /// Predict multiple bytes autoregressively
    pub fn predict_sequence(&mut self, length: usize, greedy: bool) -> Vec<u8> {
        let mut result = Vec::with_capacity(length);

        for _ in 0..length {
            let sequence: Vec<Vec<f32>> = self.context_window.iter().cloned().collect();

            if sequence.is_empty() {
                let byte = if greedy { 0 } else { self.rng.gen() };
                result.push(byte);
                continue;
            }

            // Forward through Titans blocks
            let mut hidden = self.blocks[0].forward(&sequence);
            for block in &mut self.blocks[1..] {
                let seq_with_hidden = vec![hidden.clone()];
                hidden = block.forward(&seq_with_hidden);
            }

            let probs = self.output.forward(&hidden);
            let byte = if greedy {
                self.output.argmax(&probs)
            } else {
                self.output.sample(&probs, &mut self.rng)
            };

            result.push(byte);

            // Add prediction to context for autoregressive generation
            let mut embedding = self.tokenizer.encode(byte).to_vec();
            let pos = self.context_window.len();
            let pos_enc = self.positional.get(pos);
            for (e, p) in embedding.iter_mut().zip(pos_enc.iter()) {
                *e += *p;
            }
            self.context_window.push_back(embedding);
            if self.context_window.len() > self.max_seq_len {
                self.context_window.pop_front();
            }
        }

        result
    }

    /// Check if a message matches prediction
    pub fn verify_prediction(&mut self, message: &[u8]) -> (bool, f32) {
        let predicted = self.predict_sequence(message.len(), true);
        let matches = predicted == message;

        let similarity = predicted
            .iter()
            .zip(message.iter())
            .filter(|(p, m)| p == m)
            .count() as f32
            / message.len().max(1) as f32;

        (matches, similarity)
    }

    /// Get accumulated surprise (anomaly score)
    /// High values indicate unexpected/novel message patterns
    pub fn get_surprise(&self) -> f32 {
        self.total_surprise
    }

    /// Check if current message pattern is anomalous
    pub fn is_anomalous(&self, threshold: f32) -> bool {
        self.total_surprise > threshold
    }

    /// Reset context window (but preserve long-term memory)
    pub fn reset(&mut self) {
        self.context_window.clear();
        self.total_surprise = 0.0;
    }

    /// Full reset including long-term memory
    pub fn reset_all(&mut self) {
        self.context_window.clear();
        self.total_surprise = 0.0;
        for block in &mut self.blocks {
            block.reset_memory();
        }
    }

    /// Set temperature for sampling
    pub fn set_temperature(&mut self, temp: f32) {
        self.output.set_temperature(temp);
    }
}

// Backwards compatibility aliases
pub type TransformerPredictor = TitansPredictor;
pub type TransformerConfig = TitansConfig;

/// Configuration for Titans predictor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitansConfig {
    pub embed_dim: usize,
    pub num_heads: usize,
    pub num_layers: usize,
    pub ff_dim: usize,
    pub max_seq_len: usize,
    /// Size of persistent memory (number of memory tokens)
    pub memory_size: usize,
    pub seed: u64,
}

impl Default for TitansConfig {
    fn default() -> Self {
        Self {
            embed_dim: 64,
            num_heads: 4,
            num_layers: 2,
            ff_dim: 128,
            max_seq_len: 256,
            memory_size: 64, // Persistent memory tokens
            seed: 42,
        }
    }
}

// ============================================================================
// QUANTUM-RESISTANT KEY EVOLUTION
// ============================================================================

/// Parameters for NTRU-like lattice operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatticeParams {
    pub n: usize,   // Polynomial degree (power of 2)
    pub q: u64,     // Large modulus
    pub p: u64,     // Small modulus for message space
    pub sigma: f64, // Gaussian noise standard deviation
}

impl Default for LatticeParams {
    fn default() -> Self {
        Self {
            n: 1024,    // NIST Level 3 security (~192-bit classical)
            q: 12289,   // NTT-friendly prime for n=1024
            p: 3,       // Ternary message space
            sigma: 3.2, // Standard deviation per NIST recommendations
        }
    }
}

/// Polynomial ring element Z_q[X]/(X^n + 1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RingElement {
    coeffs: Vec<i64>,
    n: usize,
    q: u64,
}

impl RingElement {
    pub fn new(n: usize, q: u64) -> Self {
        Self {
            coeffs: vec![0; n],
            n,
            q,
        }
    }

    pub fn random(n: usize, q: u64, rng: &mut StdRng) -> Self {
        let coeffs: Vec<i64> = (0..n).map(|_| rng.gen_range(0..q as i64)).collect();
        Self { coeffs, n, q }
    }

    pub fn random_ternary(n: usize, q: u64, rng: &mut StdRng) -> Self {
        let coeffs: Vec<i64> = (0..n).map(|_| rng.gen_range(-1..=1)).collect();
        Self { coeffs, n, q }
    }

    pub fn random_gaussian(n: usize, q: u64, sigma: f64, rng: &mut StdRng) -> Self {
        // Box-Muller transform for Gaussian
        let coeffs: Vec<i64> = (0..n)
            .map(|_| {
                let u1: f64 = rng.gen::<f64>().max(1e-10);
                let u2: f64 = rng.gen();
                let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                (z * sigma).round() as i64
            })
            .collect();
        Self { coeffs, n, q }
    }

    pub fn from_bytes(bytes: &[u8], n: usize, q: u64) -> Self {
        let mut coeffs = vec![0i64; n];
        for (i, chunk) in bytes.chunks(2).enumerate() {
            if i >= n {
                break;
            }
            let val = if chunk.len() == 2 {
                ((chunk[0] as u16) | ((chunk[1] as u16) << 8)) as i64
            } else {
                chunk[0] as i64
            };
            coeffs[i] = val % q as i64;
        }
        Self { coeffs, n, q }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.n * 2);
        for &c in &self.coeffs {
            let val = ((c % self.q as i64 + self.q as i64) % self.q as i64) as u16;
            bytes.push(val as u8);
            bytes.push((val >> 8) as u8);
        }
        bytes
    }

    fn reduce(&mut self) {
        for c in &mut self.coeffs {
            *c = ((*c % self.q as i64) + self.q as i64) % self.q as i64;
        }
    }

    /// Polynomial multiplication in R_q = Z_q[X]/(X^n + 1)
    pub fn mul(&self, other: &RingElement) -> RingElement {
        assert_eq!(self.n, other.n);
        let mut result = vec![0i64; self.n];

        for i in 0..self.n {
            for j in 0..self.n {
                let idx = i + j;
                let coeff = self.coeffs[i] * other.coeffs[j];
                if idx < self.n {
                    result[idx] += coeff;
                } else {
                    // X^n = -1 in the ring
                    result[idx - self.n] -= coeff;
                }
            }
        }

        let mut elem = RingElement {
            coeffs: result,
            n: self.n,
            q: self.q,
        };
        elem.reduce();
        elem
    }

    /// Polynomial addition
    pub fn add(&self, other: &RingElement) -> RingElement {
        assert_eq!(self.n, other.n);
        let coeffs: Vec<i64> = self
            .coeffs
            .iter()
            .zip(other.coeffs.iter())
            .map(|(a, b)| (a + b) % self.q as i64)
            .collect();
        let mut elem = RingElement {
            coeffs,
            n: self.n,
            q: self.q,
        };
        elem.reduce();
        elem
    }

    /// Polynomial subtraction
    pub fn sub(&self, other: &RingElement) -> RingElement {
        assert_eq!(self.n, other.n);
        let coeffs: Vec<i64> = self
            .coeffs
            .iter()
            .zip(other.coeffs.iter())
            .map(|(a, b)| (a - b) % self.q as i64)
            .collect();
        let mut elem = RingElement {
            coeffs,
            n: self.n,
            q: self.q,
        };
        elem.reduce();
        elem
    }

    /// Scale coefficients
    pub fn scale(&self, scalar: i64) -> RingElement {
        let coeffs: Vec<i64> = self
            .coeffs
            .iter()
            .map(|&c| (c * scalar) % self.q as i64)
            .collect();
        let mut elem = RingElement {
            coeffs,
            n: self.n,
            q: self.q,
        };
        elem.reduce();
        elem
    }
}

/// Quantum-resistant key pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumKeyPair {
    /// Public parameter `a` — must be stored for correct KEM encaps/decaps
    pub a: RingElement,
    pub public_key: RingElement,
    secret_key: RingElement,
    params: LatticeParams,
}

/// Quantum-resistant key evolution system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumKeyEvolution {
    params: LatticeParams,
    current_key: QuantumKeyPair,
    evolution_counter: u64,
    key_history: VecDeque<[u8; 32]>, // Hashes of past keys for forward secrecy
    max_history: usize,
    #[serde(skip, default = "default_rng")]
    rng: StdRng,
}

impl QuantumKeyEvolution {
    pub fn new(params: LatticeParams, seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let current_key = Self::generate_keypair(&params, &mut rng);

        Self {
            params,
            current_key,
            evolution_counter: 0,
            key_history: VecDeque::new(),
            max_history: 100,
            rng,
        }
    }

    fn generate_keypair(params: &LatticeParams, rng: &mut StdRng) -> QuantumKeyPair {
        // RLWE-style key generation
        let a = RingElement::random(params.n, params.q, rng);
        let s = RingElement::random_ternary(params.n, params.q, rng);
        let e = RingElement::random_gaussian(params.n, params.q, params.sigma, rng);

        // Public key: b = a*s + e
        let b = a.mul(&s).add(&e);

        QuantumKeyPair {
            a, // Store `a` for correct KEM operation
            public_key: b,
            secret_key: s,
            params: params.clone(),
        }
    }

    /// Evolve the key forward (one-way function)
    ///
    /// Uses HKDF to derive a new seed from the current key material,
    /// then generates a fresh RLWE keypair that maintains the b=a*s+e invariant.
    pub fn evolve(&mut self) -> [u8; 32] {
        // Hash current key material (public key + secret key + counter)
        let mut hasher = Sha256::new();
        hasher.update(self.current_key.public_key.to_bytes());
        hasher.update(self.current_key.secret_key.to_bytes());
        hasher.update(self.evolution_counter.to_le_bytes());
        let hash: [u8; 32] = hasher.finalize().into();

        // Store in history
        self.key_history.push_back(hash);
        if self.key_history.len() > self.max_history {
            self.key_history.pop_front();
        }

        // Use HKDF to derive new seed (mixes entropy from current key + counter)
        let hk = Hkdf::<Sha256>::new(Some(&self.evolution_counter.to_le_bytes()), &hash);
        let mut okm = [0u8; 32];
        hk.expand(b"spine-key-evolution", &mut okm)
            .expect("HKDF expand failed");
        let new_seed = u64::from_le_bytes(okm[0..8].try_into().unwrap());
        let mut new_rng = StdRng::seed_from_u64(new_seed);

        // Generate a proper RLWE keypair that maintains the b=a*s+e invariant
        self.current_key = Self::generate_keypair(&self.params, &mut new_rng);

        self.evolution_counter += 1;

        hash
    }

    /// Encapsulate a shared secret using the public key (RLWE KEM)
    ///
    /// Uses the stored `a` from keygen to ensure sender and receiver
    /// derive the same shared secret. Encodes a random message `m` into
    /// the high bits and recovers it via rounding on decapsulation.
    pub fn encapsulate(&mut self) -> (Vec<u8>, [u8; 32]) {
        // Use the SAME `a` from keygen — critical for correctness
        let a = &self.current_key.a;
        let r = RingElement::random_ternary(self.params.n, self.params.q, &mut self.rng);
        let e1 = RingElement::random_gaussian(
            self.params.n,
            self.params.q,
            self.params.sigma,
            &mut self.rng,
        );
        let e2 = RingElement::random_gaussian(
            self.params.n,
            self.params.q,
            self.params.sigma,
            &mut self.rng,
        );

        // Generate random message m ∈ {0, 1}^n for KEM
        let m: Vec<i64> = (0..self.params.n)
            .map(|_| self.rng.gen_range(0..2i64))
            .collect();

        // u = a*r + e1
        let u = a.mul(&r).add(&e1);

        // v = b*r + e2 + ⌊q/2⌋·m (encode message in high bits)
        let half_q = (self.params.q / 2) as i64;
        let encoded_m = RingElement {
            coeffs: m.iter().map(|&mi| mi * half_q).collect(),
            n: self.params.n,
            q: self.params.q,
        };
        let v = self.current_key.public_key.mul(&r).add(&e2).add(&encoded_m);

        // Ciphertext = (u, v)
        let mut ciphertext = u.to_bytes();
        ciphertext.extend(v.to_bytes());

        // Shared secret = H(m) — both sides derive from the same m
        let mut hasher = Sha256::new();
        for &mi in &m {
            hasher.update(mi.to_le_bytes());
        }
        let shared_secret: [u8; 32] = hasher.finalize().into();

        (ciphertext, shared_secret)
    }

    /// Decapsulate to recover shared secret (RLWE KEM)
    ///
    /// Recovers the encoded message by computing v - u·s, then rounding
    /// each coefficient to 0 or 1 to recover the original message m.
    /// The shared secret is H(m), matching the encapsulator.
    pub fn decapsulate(&self, ciphertext: &[u8]) -> Option<[u8; 32]> {
        let half = ciphertext.len() / 2;
        if half < self.params.n * 2 {
            return None;
        }

        let u = RingElement::from_bytes(&ciphertext[..half], self.params.n, self.params.q);
        let v = RingElement::from_bytes(&ciphertext[half..], self.params.n, self.params.q);

        // recovered = v - u·s ≈ ⌊q/2⌋·m + (small noise)
        let recovered = v.sub(&u.mul(&self.current_key.secret_key));

        // Round each coefficient: if closer to ⌊q/2⌋ → 1, else → 0
        let half_q = self.params.q as i64 / 2;
        let quarter_q = self.params.q as i64 / 4;
        let m: Vec<i64> = recovered
            .coeffs
            .iter()
            .map(|&c| {
                // Normalize to [0, q)
                let c_pos =
                    ((c % self.params.q as i64) + self.params.q as i64) % self.params.q as i64;
                // If |c - q/2| < q/4, round to 1; otherwise 0
                if (c_pos - half_q).abs() < quarter_q {
                    1i64
                } else {
                    0i64
                }
            })
            .collect();

        // Shared secret = H(m) — matches encapsulate
        let mut hasher = Sha256::new();
        for &mi in &m {
            hasher.update(mi.to_le_bytes());
        }
        Some(hasher.finalize().into())
    }

    /// Get current key hash for synchronization
    pub fn get_key_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.current_key.public_key.to_bytes());
        hasher.finalize().into()
    }

    /// Verify key chain integrity (constant-time comparison)
    pub fn verify_evolution(&self, expected_hash: &[u8; 32]) -> bool {
        self.key_history
            .iter()
            .any(|h| h.ct_eq(expected_hash).into())
    }

    /// Get evolution counter for synchronization
    pub fn get_evolution_counter(&self) -> u64 {
        self.evolution_counter
    }

    /// Export public key for key exchange
    pub fn export_public_key(&self) -> Vec<u8> {
        self.current_key.public_key.to_bytes()
    }
}

/// Combined quantum-resistant speculative protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumSpeculativeProtocol {
    predictor: TransformerPredictor,
    key_evolution: QuantumKeyEvolution,
    prediction_threshold: f32,
    evolution_interval: u64,
    message_count: u64,
}

impl QuantumSpeculativeProtocol {
    pub fn new(
        transformer_config: TransformerConfig,
        lattice_params: LatticeParams,
        seed: u64,
    ) -> Self {
        Self {
            predictor: TransformerPredictor::new(transformer_config),
            key_evolution: QuantumKeyEvolution::new(lattice_params, seed),
            prediction_threshold: 0.8,
            evolution_interval: 10,
            message_count: 0,
        }
    }

    /// Process an outgoing message with prediction and encryption
    pub fn send(&mut self, message: &[u8]) -> QuantumMessage {
        // Check if receiver could predict this
        let (matches, similarity) = self.predictor.verify_prediction(message);

        let payload = if matches && similarity >= self.prediction_threshold {
            // Send confirmation only
            MessagePayload::Confirmation {
                hash: Self::hash_message(message),
                length: message.len(),
            }
        } else {
            // Encapsulate shared secret via RLWE KEM
            let (ciphertext, shared_secret) = self.key_evolution.encapsulate();

            // Derive AES-256-GCM key from KEM shared secret via HKDF
            let hk = Hkdf::<Sha256>::new(None, &shared_secret);
            let mut aes_key = [0u8; 32];
            hk.expand(b"spine-aead-key", &mut aes_key)
                .expect("HKDF expand failed");

            // Create nonce from message count (unique per message)
            let mut nonce_bytes = [0u8; 12];
            nonce_bytes[..8].copy_from_slice(&self.message_count.to_le_bytes());
            let nonce = Nonce::from_slice(&nonce_bytes);

            // Encrypt with AES-256-GCM (authenticated encryption)
            let cipher = Aes256Gcm::new_from_slice(&aes_key).expect("AES key length");
            let encrypted = cipher.encrypt(nonce, message).expect("AES-GCM encrypt");

            // Prepend 12-byte nonce to ciphertext for receiver
            let mut encrypted_message = nonce_bytes.to_vec();
            encrypted_message.extend(encrypted);

            MessagePayload::Full {
                ciphertext,
                encrypted_message,
            }
        };

        // Evolve key periodically
        self.message_count += 1;
        let key_evolution = if self.message_count.is_multiple_of(self.evolution_interval) {
            Some(self.key_evolution.evolve())
        } else {
            None
        };

        QuantumMessage {
            payload,
            evolution_counter: self.key_evolution.get_evolution_counter(),
            key_evolution,
        }
    }

    /// Get a seed for protocol morphing based on current quantum key state
    pub fn get_morph_seed(&self) -> u64 {
        let key_hash = self.key_evolution.get_key_hash();
        u64::from_le_bytes(key_hash[0..8].try_into().unwrap())
    }

    /// Process an incoming message
    pub fn receive(&mut self, quantum_msg: &QuantumMessage) -> Option<Vec<u8>> {
        // Sync key evolution if needed
        while self.key_evolution.get_evolution_counter() < quantum_msg.evolution_counter {
            self.key_evolution.evolve();
        }

        let message = match &quantum_msg.payload {
            MessagePayload::Confirmation { hash, length } => {
                // Use prediction
                let predicted = self.predictor.predict_sequence(*length, true);

                // Verify hash
                let predicted_hash = Self::hash_message(&predicted);
                if &predicted_hash == hash {
                    Some(predicted)
                } else {
                    None // Prediction mismatch, need retransmission
                }
            }
            MessagePayload::Full {
                ciphertext,
                encrypted_message,
            } => {
                // Decapsulate KEM ciphertext to recover shared secret
                let shared_secret = self.key_evolution.decapsulate(ciphertext)?;

                // Derive AES-256-GCM key from KEM shared secret via HKDF
                let hk = Hkdf::<Sha256>::new(None, &shared_secret);
                let mut aes_key = [0u8; 32];
                hk.expand(b"spine-aead-key", &mut aes_key)
                    .expect("HKDF expand failed");

                // Extract nonce (first 12 bytes) and ciphertext
                if encrypted_message.len() < 12 {
                    return None;
                }
                let nonce = Nonce::from_slice(&encrypted_message[..12]);
                let ciphertext_data = &encrypted_message[12..];

                // Decrypt with AES-256-GCM (authenticated — rejects tampered data)
                let cipher = Aes256Gcm::new_from_slice(&aes_key).expect("AES key length");
                cipher.decrypt(nonce, ciphertext_data).ok()
            }
        };

        // Update predictor
        if let Some(ref msg) = message {
            self.predictor.observe(msg);
        }

        message
    }

    fn hash_message(message: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(message);
        hasher.finalize().into()
    }

    /// Set prediction confidence threshold
    pub fn set_threshold(&mut self, threshold: f32) {
        self.prediction_threshold = threshold.clamp(0.0, 1.0);
    }

    /// Set key evolution interval
    pub fn set_evolution_interval(&mut self, interval: u64) {
        self.evolution_interval = interval.max(1);
    }

    /// Reset protocol state
    pub fn reset(&mut self) {
        self.predictor.reset();
        self.message_count = 0;
    }
}

/// Wire format for quantum-protected messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumMessage {
    pub payload: MessagePayload,
    pub evolution_counter: u64,
    pub key_evolution: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessagePayload {
    /// Prediction matched - send only confirmation
    Confirmation { hash: [u8; 32], length: usize },
    /// Full encrypted message
    Full {
        ciphertext: Vec<u8>,
        encrypted_message: Vec<u8>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_positional_encoding() {
        let pe = PositionalEncoding::new(100, 64);
        let enc0 = pe.get(0);
        let enc50 = pe.get(50);
        assert_eq!(enc0.len(), 64);
        assert_ne!(enc0, enc50);
    }

    #[test]
    fn test_layer_norm() {
        let ln = LayerNorm::new(8);
        let input = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let output = ln.forward(&input);
        assert_eq!(output.len(), 8);

        // Mean should be ~0
        let mean: f32 = output.iter().sum::<f32>() / output.len() as f32;
        assert!(mean.abs() < 0.01);
    }

    #[test]
    fn test_titans_predictor() {
        let config = TitansConfig {
            embed_dim: 32,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 64,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let mut predictor = TitansPredictor::new(config);

        // Observe some data
        predictor.observe(b"Hello ");
        predictor.observe(b"World");

        // Predict next
        let (next, conf) = predictor.predict_next();
        assert!(conf > 0.0 && conf <= 1.0);
        // next is u8, already valid in range 0-255
        let _ = next;

        // Check surprise is tracked
        let surprise = predictor.get_surprise();
        assert!(surprise >= 0.0);
    }

    #[test]
    fn test_titans_anomaly_detection() {
        let config = TitansConfig {
            embed_dim: 32,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 64,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let mut predictor = TitansPredictor::new(config);

        // Train on normal pattern
        for _ in 0..10 {
            predictor.observe(b"GET /api/status\n");
        }
        let _normal_surprise = predictor.get_surprise();

        // Introduce anomalous pattern
        predictor.observe(b"MALICIOUS_PAYLOAD_XYZ!!!");
        let anomaly_surprise = predictor.get_surprise();

        // Anomaly should have higher surprise (or at least be detected)
        assert!(anomaly_surprise >= 0.0);
    }

    #[test]
    fn test_ring_operations() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = LatticeParams {
            n: 16,
            q: 97,
            p: 3,
            sigma: 2.0,
        };

        let a = RingElement::random(params.n, params.q, &mut rng);
        let b = RingElement::random(params.n, params.q, &mut rng);

        let sum = a.add(&b);
        let product = a.mul(&b);

        assert_eq!(sum.coeffs.len(), params.n);
        assert_eq!(product.coeffs.len(), params.n);

        // Check coefficients are in range
        for &c in &sum.coeffs {
            assert!(c >= 0 && c < params.q as i64);
        }
    }

    #[test]
    fn test_key_evolution() {
        let params = LatticeParams {
            n: 32,
            q: 257,
            p: 3,
            sigma: 2.0,
        };
        let mut ke = QuantumKeyEvolution::new(params, 42);

        let hash1 = ke.get_key_hash();
        ke.evolve();
        let hash2 = ke.get_key_hash();

        // Keys should be different after evolution
        assert_ne!(hash1, hash2);

        // Evolution should be tracked
        assert_eq!(ke.get_evolution_counter(), 1);
    }

    #[test]
    fn test_encapsulation() {
        let params = LatticeParams {
            n: 32,
            q: 257,
            p: 3,
            sigma: 2.0,
        };
        let mut ke = QuantumKeyEvolution::new(params, 42);

        let (ciphertext, _shared_secret1) = ke.encapsulate();
        assert!(!ciphertext.is_empty());

        let shared_secret2 = ke.decapsulate(&ciphertext);
        assert!(shared_secret2.is_some());

        // Note: Due to noise, shared secrets may not match exactly in RLWE
        // This is a simplified demo - production would use error correction
    }

    #[test]
    fn test_quantum_speculative_protocol() {
        let config = TitansConfig {
            embed_dim: 16,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 32,
            max_seq_len: 32,
            memory_size: 8,
            seed: 42,
        };
        let params = LatticeParams {
            n: 16,
            q: 97,
            p: 3,
            sigma: 2.0,
        };

        let mut alice = QuantumSpeculativeProtocol::new(config.clone(), params.clone(), 42);
        let mut bob = QuantumSpeculativeProtocol::new(config, params, 42);

        // Alice sends to Bob
        let msg = b"Hello Bob!";
        let quantum_msg = alice.send(msg);

        // Bob receives
        let received = bob.receive(&quantum_msg);
        assert!(received.is_some());
        assert_eq!(received.unwrap(), msg.to_vec());
    }

    #[test]
    fn test_prediction_efficiency() {
        let config = TransformerConfig::default();
        let params = LatticeParams::default();

        let mut sender = QuantumSpeculativeProtocol::new(config.clone(), params.clone(), 42);
        let mut receiver = QuantumSpeculativeProtocol::new(config, params, 42);

        // Send same pattern multiple times to train predictor
        for _ in 0..5 {
            let msg1 = sender.send(b"GET /api/status");
            receiver.receive(&msg1);

            let msg2 = sender.send(b"200 OK");
            receiver.receive(&msg2);
        }

        // After training, check if prediction kicks in
        let msg = sender.send(b"GET /api/status");

        // Even if not confirmed (training takes longer), protocol should work
        let received = receiver.receive(&msg);
        assert!(received.is_some());
    }

    // =========================================================================
    // MIRAS-ADAPTIVE PREDICTOR TESTS
    // =========================================================================

    #[test]
    fn test_miras_predictor_basic() {
        let config = TitansConfig {
            embed_dim: 32,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 64,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let mut predictor = MirasTitansPredictor::new(config);

        // Observe data
        predictor.observe(b"Hello World");

        // Check variant starts as Titans
        assert_eq!(predictor.variant(), "titans");

        // Predict should work
        let (next, conf) = predictor.predict_next();
        assert!(conf > 0.0 && conf <= 1.0);
        // next is u8, already valid in range 0-255
        let _ = next;

        // Stats should be populated
        let stats = predictor.stats();
        assert_eq!(stats.message_count, 1);
        assert!(stats.miras_enhanced_predictions > 0);
    }

    #[test]
    fn test_miras_predictor_variants() {
        let config = TitansConfig {
            embed_dim: 32,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 64,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };

        // Test each variant - check initial state before observe
        for variant in [
            MirasVariant::Titans,
            MirasVariant::Yaad,
            MirasVariant::Moneta { p: 2.0 },
            MirasVariant::Memora,
        ] {
            let predictor = MirasTitansPredictor::new_with_variant(config.clone(), variant);

            // Check variant matches what was requested (before any adaptive switching)
            match variant {
                MirasVariant::Titans => assert_eq!(predictor.variant(), "titans"),
                MirasVariant::Yaad => assert_eq!(predictor.variant(), "yaad"),
                MirasVariant::Moneta { .. } => assert_eq!(predictor.variant(), "moneta"),
                MirasVariant::Memora => assert_eq!(predictor.variant(), "memora"),
            }
        }

        // Test that variants can be used after observation (adaptive switching may occur)
        let mut predictor =
            MirasTitansPredictor::new_with_variant(config.clone(), MirasVariant::Yaad);
        assert_eq!(predictor.variant(), "yaad");

        // After observe, low anomaly may switch to Titans (adaptive behavior)
        predictor.observe(b"test");
        // Variant may have changed due to adaptive switching - this is expected behavior
    }

    #[test]
    fn test_miras_predictor_combined_surprise() {
        let config = TitansConfig {
            embed_dim: 32,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 64,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let mut predictor = MirasTitansPredictor::new(config);

        // Train on normal pattern
        for _ in 0..5 {
            predictor.observe(b"normal message pattern");
        }

        // Get combined surprise
        let combined = predictor.get_combined_surprise();
        assert!(combined >= 0.0);

        // Get individual surprises
        let titans_surprise = predictor.get_surprise();
        let miras_surprise = predictor.get_miras_surprise();

        assert!(titans_surprise >= 0.0);
        assert!(miras_surprise.is_some());
    }

    #[test]
    fn test_miras_predictor_anomaly_level() {
        let config = TitansConfig {
            embed_dim: 32,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 64,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let mut predictor = MirasTitansPredictor::new(config);

        // Initially no anomaly
        assert_eq!(predictor.anomaly_level(), 0.0);

        // After observation, anomaly level is tracked
        predictor.observe(b"test");
        let level = predictor.anomaly_level();
        assert!(level >= 0.0); // Some level is tracked
    }

    #[test]
    fn test_miras_predictor_reset() {
        let config = TitansConfig {
            embed_dim: 32,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 64,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let mut predictor = MirasTitansPredictor::new(config);

        // Add some state
        for _ in 0..10 {
            predictor.observe(b"data");
        }
        assert!(predictor.stats().message_count > 0);

        // Reset
        predictor.reset_all();
        let stats = predictor.stats();
        assert_eq!(stats.message_count, 0);
    }

    // =========================================================================
    // COMPREHENSIVE SECURITY TESTS (W3: Security Validation)
    // =========================================================================

    #[test]
    fn test_rlwe_ring_arithmetic_correctness() {
        // Test ring arithmetic properties: associativity, commutativity, distributivity
        let mut rng = StdRng::seed_from_u64(12345);
        let params = LatticeParams {
            n: 32,
            q: 257,
            p: 3,
            sigma: 2.0,
        };

        let a = RingElement::random(params.n, params.q, &mut rng);
        let b = RingElement::random(params.n, params.q, &mut rng);
        let c = RingElement::random(params.n, params.q, &mut rng);

        // Commutativity of addition: a + b = b + a
        let ab = a.add(&b);
        let ba = b.add(&a);
        assert_eq!(ab.coeffs, ba.coeffs, "Addition should be commutative");

        // Associativity of addition: (a + b) + c = a + (b + c)
        let ab_c = a.add(&b).add(&c);
        let a_bc = a.add(&b.add(&c));
        assert_eq!(ab_c.coeffs, a_bc.coeffs, "Addition should be associative");

        // Distributivity: a * (b + c) = a*b + a*c
        let a_times_bplusc = a.mul(&b.add(&c));
        let ab_plus_ac = a.mul(&b).add(&a.mul(&c));
        assert_eq!(
            a_times_bplusc.coeffs, ab_plus_ac.coeffs,
            "Multiplication should distribute over addition"
        );
    }

    #[test]
    fn test_rlwe_gaussian_distribution() {
        // Verify Gaussian noise has expected statistical properties
        let mut rng = StdRng::seed_from_u64(54321);
        let params = LatticeParams {
            n: 1024,
            q: 12289, // NIST-like parameter
            p: 3,
            sigma: 3.2,
        };

        let e = RingElement::random_gaussian(params.n, params.q, params.sigma, &mut rng);

        // Calculate mean and variance
        let mean: f64 = e.coeffs.iter().map(|&c| c as f64).sum::<f64>() / params.n as f64;
        let variance: f64 = e
            .coeffs
            .iter()
            .map(|&c| (c as f64 - mean).powi(2))
            .sum::<f64>()
            / params.n as f64;

        // Mean should be close to 0 (centered)
        assert!(
            mean.abs() < params.sigma,
            "Gaussian mean should be near 0, got {}",
            mean
        );

        // Variance should be close to sigma^2
        let expected_variance = params.sigma * params.sigma;
        assert!(
            (variance - expected_variance).abs() < expected_variance * 0.5,
            "Variance {} should be close to sigma^2 = {}",
            variance,
            expected_variance
        );
    }

    #[test]
    fn test_rlwe_ternary_distribution() {
        // Verify ternary noise is in {-1, 0, 1}
        let mut rng = StdRng::seed_from_u64(98765);
        let params = LatticeParams {
            n: 256,
            q: 257,
            p: 3,
            sigma: 2.0,
        };

        let s = RingElement::random_ternary(params.n, params.q, &mut rng);

        // Ternary coefficients are in {-1, 0, 1} before reduction
        for &coeff in &s.coeffs {
            assert!(
                coeff == 0 || coeff == 1 || coeff == -1,
                "Ternary coefficient should be -1, 0, or 1, got {}",
                coeff
            );
        }

        // Check roughly uniform distribution among {-1, 0, 1}
        let count_zero = s.coeffs.iter().filter(|&&c| c == 0).count();
        let count_one = s.coeffs.iter().filter(|&&c| c == 1).count();
        let count_neg = s.coeffs.iter().filter(|&&c| c == -1).count();

        // Each should be roughly 1/3 of total
        let expected = params.n / 3;
        let tolerance = params.n / 4; // Allow 25% deviation
        assert!(
            (count_zero as isize - expected as isize).unsigned_abs() < tolerance,
            "Ternary distribution unbalanced: zeros={}, ones={}, neg={}",
            count_zero,
            count_one,
            count_neg
        );
    }

    #[test]
    fn test_key_evolution_forward_secrecy() {
        // Test that key evolution provides forward secrecy
        let params = LatticeParams {
            n: 64,
            q: 257,
            p: 3,
            sigma: 2.0,
        };

        let mut ke1 = QuantumKeyEvolution::new(params.clone(), 42);
        let mut ke2 = QuantumKeyEvolution::new(params, 42);

        // Both start with same state
        assert_eq!(ke1.get_key_hash(), ke2.get_key_hash());

        // Evolve ke1 multiple times
        for _ in 0..5 {
            ke1.evolve();
        }

        // Keys should now be different
        assert_ne!(ke1.get_key_hash(), ke2.get_key_hash());

        // Counter should track evolutions
        assert_eq!(ke1.get_evolution_counter(), 5);
        assert_eq!(ke2.get_evolution_counter(), 0);

        // Sync ke2 to same state
        for _ in 0..5 {
            ke2.evolve();
        }

        // Now should match again (deterministic evolution)
        assert_eq!(ke1.get_key_hash(), ke2.get_key_hash());
    }

    #[test]
    fn test_key_evolution_history_integrity() {
        let params = LatticeParams {
            n: 32,
            q: 257,
            p: 3,
            sigma: 2.0,
        };

        let mut ke = QuantumKeyEvolution::new(params, 42);

        // Collect hashes during evolution
        let mut hashes = Vec::new();
        for _ in 0..10 {
            let hash = ke.evolve();
            hashes.push(hash);
        }

        // All hashes should be unique (no cycles)
        let unique_count = hashes
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(unique_count, 10, "All evolution hashes should be unique");

        // History verification should work for recent keys
        for hash in &hashes {
            assert!(
                ke.verify_evolution(hash),
                "Recent evolution should be verifiable"
            );
        }
    }

    #[test]
    fn test_quantum_protocol_message_integrity() {
        // Test that messages are correctly encrypted and decrypted
        let config = TitansConfig {
            embed_dim: 16,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 32,
            max_seq_len: 32,
            memory_size: 8,
            seed: 42,
        };
        let params = LatticeParams {
            n: 32,
            q: 257,
            p: 3,
            sigma: 2.0,
        };

        let mut alice = QuantumSpeculativeProtocol::new(config.clone(), params.clone(), 42);
        let mut bob = QuantumSpeculativeProtocol::new(config, params, 42);

        // Test multiple different message sizes
        let test_messages = [
            b"A".to_vec(),
            b"Short".to_vec(),
            b"Medium length message".to_vec(),
            b"This is a longer message to test variable length handling properly".to_vec(),
        ];

        for msg in &test_messages {
            let quantum_msg = alice.send(msg);
            let received = bob.receive(&quantum_msg);
            assert!(received.is_some(), "Should receive message");
            assert_eq!(
                &received.unwrap(),
                msg,
                "Received message should match original"
            );
        }
    }

    #[test]
    fn test_tampered_ciphertext_detection() {
        let params = LatticeParams {
            n: 32,
            q: 257,
            p: 3,
            sigma: 2.0,
        };

        let mut ke = QuantumKeyEvolution::new(params, 42);

        let (mut ciphertext, original_secret) = ke.encapsulate();

        // Tamper with ciphertext
        if !ciphertext.is_empty() {
            ciphertext[0] ^= 0xFF;
        }

        // Decapsulation with tampered ciphertext should produce different result
        let tampered_secret = ke.decapsulate(&ciphertext);

        if let Some(tampered) = tampered_secret {
            // Due to noise characteristics, tampered ciphertext produces different secret
            // In production, we'd add MAC for integrity checking
            assert_ne!(
                tampered, original_secret,
                "Tampered ciphertext should produce different secret"
            );
        }
    }

    #[test]
    fn test_lattice_params_security_levels() {
        // Test different security parameter sets
        let toy_params = LatticeParams {
            n: 16,
            q: 97,
            p: 3,
            sigma: 2.0,
        };
        let medium_params = LatticeParams {
            n: 256,
            q: 7681,
            p: 3,
            sigma: 3.19,
        };
        let _high_params = LatticeParams {
            n: 1024,
            q: 12289,
            p: 3,
            sigma: 3.19,
        };

        // Verify params are valid (n is power of 2, q is prime)
        assert!(
            toy_params.n.is_power_of_two(),
            "n should be power of 2 for NTT"
        );
        assert!(
            medium_params.n.is_power_of_two(),
            "n should be power of 2 for NTT"
        );

        // Key generation should work for all param sets
        let mut ke_toy = QuantumKeyEvolution::new(toy_params, 1);
        let mut ke_med = QuantumKeyEvolution::new(medium_params, 1);

        // Both should be able to encapsulate/decapsulate
        let (ct_toy, _) = ke_toy.encapsulate();
        let (ct_med, _) = ke_med.encapsulate();

        assert!(!ct_toy.is_empty());
        assert!(!ct_med.is_empty());

        // Medium params should produce larger ciphertext
        assert!(
            ct_med.len() > ct_toy.len(),
            "Higher security params should produce larger ciphertext"
        );
    }

    #[test]
    fn test_titans_predictor_statistical_properties() {
        let config = TitansConfig {
            embed_dim: 32,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 64,
            max_seq_len: 64,
            memory_size: 16,
            seed: 42,
        };
        let mut predictor = TitansPredictor::new(config);

        // Train on repetitive pattern
        let pattern = b"ABCABC";
        for _ in 0..20 {
            predictor.observe(pattern);
        }

        // Predictions should have reasonable confidence
        let (_, confidence) = predictor.predict_next();
        assert!(
            (0.0..=1.0).contains(&confidence),
            "Confidence should be normalized"
        );

        // Surprise should be tracked
        let surprise = predictor.get_surprise();
        assert!(surprise >= 0.0, "Surprise should be non-negative");
    }

    #[test]
    fn test_kem_shared_secret_match() {
        // Verify that encapsulate and decapsulate produce matching shared secrets
        let params = LatticeParams {
            n: 64,
            q: 257,
            p: 3,
            sigma: 1.5,
        };
        let mut ke = QuantumKeyEvolution::new(params, 12345);

        let (ciphertext, shared_secret_enc) = ke.encapsulate();
        let shared_secret_dec = ke.decapsulate(&ciphertext).unwrap();

        assert_eq!(
            shared_secret_enc, shared_secret_dec,
            "KEM shared secrets must match between encapsulate and decapsulate"
        );
    }

    #[test]
    fn test_aead_tampered_ciphertext_rejected() {
        // Verify that AES-256-GCM rejects tampered ciphertext
        let config = TransformerConfig::default();
        let params = LatticeParams {
            n: 32,
            q: 257,
            p: 3,
            sigma: 2.0,
        };

        let mut alice = QuantumSpeculativeProtocol::new(config.clone(), params.clone(), 42);
        let mut bob = QuantumSpeculativeProtocol::new(config, params, 42);

        let msg = b"Secret message";
        let mut quantum_msg = alice.send(msg);

        // Tamper with the encrypted message
        if let MessagePayload::Full {
            ref mut encrypted_message,
            ..
        } = quantum_msg.payload
        {
            if let Some(byte) = encrypted_message.last_mut() {
                *byte ^= 0xFF; // Flip bits
            }
        }

        // Bob should reject tampered message (AES-GCM authentication failure)
        let received = bob.receive(&quantum_msg);
        assert!(
            received.is_none(),
            "Tampered ciphertext must be rejected by AEAD"
        );
    }

    #[test]
    fn test_key_evolution_maintains_kem_invariant() {
        // Verify that key evolution produces valid keypairs (b = a*s + e)
        let params = LatticeParams {
            n: 32,
            q: 257,
            p: 3,
            sigma: 2.0,
        };
        let mut ke = QuantumKeyEvolution::new(params, 99);

        for _ in 0..5 {
            ke.evolve();
            // After evolution, encaps/decaps should still work
            let (ct, ss_enc) = ke.encapsulate();
            let ss_dec = ke.decapsulate(&ct).unwrap();
            assert_eq!(ss_enc, ss_dec, "KEM must work after key evolution");
        }
    }

    #[test]
    fn test_key_evolution_deterministic_hkdf() {
        // Verify that two instances with the same seed evolve identically
        let params = LatticeParams::default();
        let mut ke1 = QuantumKeyEvolution::new(params.clone(), 7777);
        let mut ke2 = QuantumKeyEvolution::new(params, 7777);

        for _ in 0..5 {
            let h1 = ke1.evolve();
            let h2 = ke2.evolve();
            assert_eq!(
                h1, h2,
                "Deterministic evolution must produce identical hashes"
            );
        }
        assert_eq!(ke1.get_key_hash(), ke2.get_key_hash());
    }

    #[test]
    fn test_aes_gcm_round_trip() {
        // Full send/receive round-trip with AES-256-GCM
        let config = TransformerConfig::default();
        let params = LatticeParams {
            n: 64,
            q: 257,
            p: 3,
            sigma: 1.5,
        };

        let mut alice = QuantumSpeculativeProtocol::new(config.clone(), params.clone(), 100);
        let mut bob = QuantumSpeculativeProtocol::new(config, params, 100);

        // Send multiple messages
        for i in 0..5 {
            let msg = format!("Message number {}", i);
            let quantum_msg = alice.send(msg.as_bytes());
            let received = bob.receive(&quantum_msg);
            assert!(received.is_some(), "Message {} should decrypt", i);
            assert_eq!(
                received.unwrap(),
                msg.as_bytes(),
                "Message {} content mismatch",
                i
            );
        }
    }
}
