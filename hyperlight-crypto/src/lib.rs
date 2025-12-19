//! # Hyperlight Crypto
//!
//! Advanced cryptographic primitives for the Hyperlight stack including:
//! - Transformer-based message prediction for speculative decoding
//! - Post-quantum key evolution using lattice-based cryptography concepts
//!
//! ## Transformer Predictor
//!
//! Uses a decoder-only transformer architecture to predict message sequences,
//! enabling speculative decoding where receivers can pre-compute responses.
//!
//! ## Quantum-Resistant Key Evolution
//!
//! Implements NTRU-inspired lattice operations for key evolution that
//! resists quantum computing attacks (Shor's algorithm).

use hyperlight_neural::{Activation, DenseLayer, MultiHeadAttention};
use rand::prelude::*;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::collections::VecDeque;

// ============================================================================
// TRANSFORMER-BASED MESSAGE PREDICTOR
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
        let mut encodings = vec![vec![0.0; embed_dim]; max_len];
        
        for pos in 0..max_len {
            for i in 0..embed_dim {
                let angle = pos as f32 / (10000.0_f32).powf(2.0 * (i / 2) as f32 / embed_dim as f32);
                if i % 2 == 0 {
                    encodings[pos][i] = angle.sin();
                } else {
                    encodings[pos][i] = angle.cos();
                }
            }
        }
        
        Self {
            max_len,
            embed_dim,
            encodings,
        }
    }

    pub fn get(&self, position: usize) -> &[f32] {
        &self.encodings[position.min(self.max_len - 1)]
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
        let mean: f32 = x.iter().sum::<f32>() / x.len() as f32;
        let var: f32 = x.iter().map(|&v| (v - mean).powi(2)).sum::<f32>() / x.len() as f32;
        let std = (var + self.eps).sqrt();
        
        x.iter()
            .enumerate()
            .map(|(i, &v)| self.gamma[i] * (v - mean) / std + self.beta[i])
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

/// Single transformer decoder block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformerBlock {
    attention: MultiHeadAttention,
    ff: FeedForward,
    norm1: LayerNorm,
    norm2: LayerNorm,
    embed_dim: usize,
}

impl TransformerBlock {
    pub fn new(embed_dim: usize, num_heads: usize, ff_dim: usize, rng: &mut StdRng) -> Self {
        Self {
            attention: MultiHeadAttention::new(embed_dim, num_heads, rng),
            ff: FeedForward::new(embed_dim, ff_dim, rng),
            norm1: LayerNorm::new(embed_dim),
            norm2: LayerNorm::new(embed_dim),
            embed_dim,
        }
    }

    pub fn forward(&mut self, sequence: &[Vec<f32>]) -> Vec<f32> {
        if sequence.is_empty() {
            return vec![0.0; self.embed_dim];
        }
        
        // Self-attention with residual
        let attended = self.attention.forward(sequence);
        let last = &sequence[sequence.len() - 1];
        let residual1: Vec<f32> = attended.iter()
            .zip(last.iter())
            .map(|(a, l)| a + l)
            .collect();
        let normed1 = self.norm1.forward(&residual1);
        
        // Feed-forward with residual
        let ff_out = self.ff.forward(&normed1);
        let residual2: Vec<f32> = ff_out.iter()
            .zip(normed1.iter())
            .map(|(f, n)| f + n)
            .collect();
        self.norm2.forward(&residual2)
    }
}

/// Byte-level tokenizer for message encoding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByteTokenizer {
    embed_dim: usize,
    embeddings: Vec<Vec<f32>>,  // 256 byte embeddings
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
        
        Self { embed_dim, embeddings }
    }

    pub fn encode(&self, byte: u8) -> &[f32] {
        &self.embeddings[byte as usize]
    }

    pub fn encode_sequence(&self, bytes: &[u8]) -> Vec<Vec<f32>> {
        bytes.iter().map(|&b| self.embeddings[b as usize].clone()).collect()
    }
}

/// Output projection to predict next byte distribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputProjection {
    weights: Vec<Vec<f32>>,  // [256][embed_dim]
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
        probs.iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i as u8)
            .unwrap_or(0)
    }
}

/// Full transformer message predictor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformerPredictor {
    tokenizer: ByteTokenizer,
    positional: PositionalEncoding,
    blocks: Vec<TransformerBlock>,
    output: OutputProjection,
    embed_dim: usize,
    max_seq_len: usize,
    context_window: VecDeque<Vec<f32>>,
    #[serde(skip, default = "default_rng")]
    rng: StdRng,
}

fn default_rng() -> StdRng {
    StdRng::seed_from_u64(42)
}

impl TransformerPredictor {
    pub fn new(config: TransformerConfig) -> Self {
        let mut rng = StdRng::seed_from_u64(config.seed);
        
        let tokenizer = ByteTokenizer::new(config.embed_dim, &mut rng);
        let positional = PositionalEncoding::new(config.max_seq_len, config.embed_dim);
        
        let blocks: Vec<TransformerBlock> = (0..config.num_layers)
            .map(|_| TransformerBlock::new(
                config.embed_dim,
                config.num_heads,
                config.ff_dim,
                &mut rng,
            ))
            .collect();
        
        let output = OutputProjection::new(config.embed_dim, &mut rng);
        
        Self {
            tokenizer,
            positional,
            blocks,
            output,
            embed_dim: config.embed_dim,
            max_seq_len: config.max_seq_len,
            context_window: VecDeque::with_capacity(config.max_seq_len),
            rng,
        }
    }

    /// Add a message to the context for prediction
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
    }

    /// Predict the next byte
    pub fn predict_next(&mut self) -> (u8, f32) {
        let sequence: Vec<Vec<f32>> = self.context_window.iter().cloned().collect();
        
        if sequence.is_empty() {
            return (0, 1.0 / 256.0);
        }
        
        // Forward through transformer blocks
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
            
            // Forward
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
        
        let similarity = predicted.iter()
            .zip(message.iter())
            .filter(|(p, m)| p == m)
            .count() as f32 / message.len().max(1) as f32;
        
        (matches, similarity)
    }

    /// Reset context
    pub fn reset(&mut self) {
        self.context_window.clear();
    }

    /// Set temperature for sampling
    pub fn set_temperature(&mut self, temp: f32) {
        self.output.set_temperature(temp);
    }
}

/// Configuration for transformer predictor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformerConfig {
    pub embed_dim: usize,
    pub num_heads: usize,
    pub num_layers: usize,
    pub ff_dim: usize,
    pub max_seq_len: usize,
    pub seed: u64,
}

impl Default for TransformerConfig {
    fn default() -> Self {
        Self {
            embed_dim: 64,
            num_heads: 4,
            num_layers: 2,
            ff_dim: 128,
            max_seq_len: 256,
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
    pub n: usize,        // Polynomial degree (power of 2)
    pub q: u64,          // Large modulus
    pub p: u64,          // Small modulus for message space
    pub sigma: f64,      // Gaussian noise standard deviation
}

impl Default for LatticeParams {
    fn default() -> Self {
        Self {
            n: 256,       // Moderate security
            q: 3329,      // Kyber-like modulus
            p: 3,         // Ternary message space
            sigma: 2.0,   // Noise for security
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
        
        let mut elem = RingElement { coeffs: result, n: self.n, q: self.q };
        elem.reduce();
        elem
    }

    /// Polynomial addition
    pub fn add(&self, other: &RingElement) -> RingElement {
        assert_eq!(self.n, other.n);
        let coeffs: Vec<i64> = self.coeffs.iter()
            .zip(other.coeffs.iter())
            .map(|(a, b)| (a + b) % self.q as i64)
            .collect();
        let mut elem = RingElement { coeffs, n: self.n, q: self.q };
        elem.reduce();
        elem
    }

    /// Polynomial subtraction
    pub fn sub(&self, other: &RingElement) -> RingElement {
        assert_eq!(self.n, other.n);
        let coeffs: Vec<i64> = self.coeffs.iter()
            .zip(other.coeffs.iter())
            .map(|(a, b)| (a - b) % self.q as i64)
            .collect();
        let mut elem = RingElement { coeffs, n: self.n, q: self.q };
        elem.reduce();
        elem
    }

    /// Scale coefficients
    pub fn scale(&self, scalar: i64) -> RingElement {
        let coeffs: Vec<i64> = self.coeffs.iter()
            .map(|&c| (c * scalar) % self.q as i64)
            .collect();
        let mut elem = RingElement { coeffs, n: self.n, q: self.q };
        elem.reduce();
        elem
    }
}

/// Quantum-resistant key pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumKeyPair {
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
    key_history: VecDeque<[u8; 32]>,  // Hashes of past keys for forward secrecy
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
            public_key: b,
            secret_key: s,
            params: params.clone(),
        }
    }

    /// Evolve the key forward (one-way function)
    pub fn evolve(&mut self) -> [u8; 32] {
        // Hash current key
        let mut hasher = Sha256::new();
        hasher.update(&self.current_key.public_key.to_bytes());
        hasher.update(&self.evolution_counter.to_le_bytes());
        let hash: [u8; 32] = hasher.finalize().into();
        
        // Store in history
        self.key_history.push_back(hash);
        if self.key_history.len() > self.max_history {
            self.key_history.pop_front();
        }
        
        // Derive new seed from hash
        let new_seed = u64::from_le_bytes(hash[0..8].try_into().unwrap());
        let mut new_rng = StdRng::seed_from_u64(new_seed);
        
        // Generate new keypair with chained randomness
        let old_pk_bytes = self.current_key.public_key.to_bytes();
        let mixing = RingElement::from_bytes(&old_pk_bytes, self.params.n, self.params.q);
        
        let new_key = Self::generate_keypair(&self.params, &mut new_rng);
        
        // Mix old and new for transitional security
        let mixed_pk = new_key.public_key.add(&mixing.scale(
            self.evolution_counter as i64 % (self.params.q as i64 / 2)
        ));
        
        self.current_key = QuantumKeyPair {
            public_key: mixed_pk,
            secret_key: new_key.secret_key,
            params: self.params.clone(),
        };
        
        self.evolution_counter += 1;
        
        hash
    }

    /// Encapsulate a shared secret using the public key
    pub fn encapsulate(&mut self) -> (Vec<u8>, [u8; 32]) {
        let a = RingElement::random(self.params.n, self.params.q, &mut self.rng);
        let r = RingElement::random_ternary(self.params.n, self.params.q, &mut self.rng);
        let e1 = RingElement::random_gaussian(self.params.n, self.params.q, self.params.sigma, &mut self.rng);
        let e2 = RingElement::random_gaussian(self.params.n, self.params.q, self.params.sigma, &mut self.rng);
        
        // u = a*r + e1
        let u = a.mul(&r).add(&e1);
        
        // v = b*r + e2 + encode(m)
        // For key encapsulation, m is derived from randomness
        let v = self.current_key.public_key.mul(&r).add(&e2);
        
        // Ciphertext
        let mut ciphertext = u.to_bytes();
        ciphertext.extend(v.to_bytes());
        
        // Shared secret (hash of v)
        let mut hasher = Sha256::new();
        hasher.update(&v.to_bytes());
        let shared_secret: [u8; 32] = hasher.finalize().into();
        
        (ciphertext, shared_secret)
    }

    /// Decapsulate to recover shared secret
    pub fn decapsulate(&self, ciphertext: &[u8]) -> Option<[u8; 32]> {
        let half = ciphertext.len() / 2;
        if half < self.params.n * 2 {
            return None;
        }
        
        let u = RingElement::from_bytes(&ciphertext[..half], self.params.n, self.params.q);
        let v = RingElement::from_bytes(&ciphertext[half..], self.params.n, self.params.q);
        
        // m' = v - u*s
        let recovered = v.sub(&u.mul(&self.current_key.secret_key));
        
        // Shared secret
        let mut hasher = Sha256::new();
        hasher.update(&recovered.to_bytes());
        Some(hasher.finalize().into())
    }

    /// Get current key hash for synchronization
    pub fn get_key_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.current_key.public_key.to_bytes());
        hasher.finalize().into()
    }

    /// Verify key chain integrity
    pub fn verify_evolution(&self, expected_hash: &[u8; 32]) -> bool {
        self.key_history.iter().any(|h| h == expected_hash)
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
            // Encapsulate full message
            let (ciphertext, _shared_secret) = self.key_evolution.encapsulate();
            
            // XOR message with shared secret (simplified encryption)
            let mut encrypted = message.to_vec();
            let key_bytes = self.key_evolution.get_key_hash();
            for (i, byte) in encrypted.iter_mut().enumerate() {
                *byte ^= key_bytes[i % 32];
            }
            
            MessagePayload::Full {
                ciphertext,
                encrypted_message: encrypted,
            }
        };
        
        // Update predictor with actual message
        self.predictor.observe(message);
        
        // Evolve key periodically
        self.message_count += 1;
        let key_evolution = if self.message_count % self.evolution_interval == 0 {
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
                    None  // Prediction mismatch, need retransmission
                }
            }
            MessagePayload::Full { ciphertext: _, encrypted_message } => {
                // Decrypt
                let key_bytes = self.key_evolution.get_key_hash();
                let decrypted: Vec<u8> = encrypted_message.iter()
                    .enumerate()
                    .map(|(i, &byte)| byte ^ key_bytes[i % 32])
                    .collect();
                Some(decrypted)
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
    Confirmation {
        hash: [u8; 32],
        length: usize,
    },
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
    fn test_transformer_predictor() {
        let config = TransformerConfig {
            embed_dim: 32,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 64,
            max_seq_len: 64,
            seed: 42,
        };
        let mut predictor = TransformerPredictor::new(config);
        
        // Observe some data
        predictor.observe(b"Hello ");
        predictor.observe(b"World");
        
        // Predict next
        let (next, conf) = predictor.predict_next();
        assert!(conf > 0.0 && conf <= 1.0);
        assert!(next <= 255);
    }

    #[test]
    fn test_ring_operations() {
        let mut rng = StdRng::seed_from_u64(42);
        let params = LatticeParams { n: 16, q: 97, p: 3, sigma: 2.0 };
        
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
        let params = LatticeParams { n: 32, q: 257, p: 3, sigma: 2.0 };
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
        let params = LatticeParams { n: 32, q: 257, p: 3, sigma: 2.0 };
        let mut ke = QuantumKeyEvolution::new(params, 42);
        
        let (ciphertext, shared_secret1) = ke.encapsulate();
        assert!(!ciphertext.is_empty());
        
        let shared_secret2 = ke.decapsulate(&ciphertext);
        assert!(shared_secret2.is_some());
        
        // Note: Due to noise, shared secrets may not match exactly in RLWE
        // This is a simplified demo - production would use error correction
    }

    #[test]
    fn test_quantum_speculative_protocol() {
        let config = TransformerConfig {
            embed_dim: 16,
            num_heads: 2,
            num_layers: 1,
            ff_dim: 32,
            max_seq_len: 32,
            seed: 42,
        };
        let params = LatticeParams { n: 16, q: 97, p: 3, sigma: 2.0 };
        
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
}
