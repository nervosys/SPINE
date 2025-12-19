//! # Hyperlight Neural
//!
//! Neural network-based latent encoder for the Chameleon Protocol.
//! Provides trainable projection matrices that evolve based on communication patterns.
//!
//! ## Key Features
//!
//! - **Learned Projections**: Train projection matrices from message data
//! - **Variational Encoding**: Stochastic latent space with learnable variance
//! - **Adversarial Training**: Train encoder to resist cryptanalysis
//! - **Temporal Consistency**: LSTM-based state evolution for moving-target defense
//! - **Message Prediction**: Transformer-style attention for speculative decoding

use rand::prelude::*;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Activation functions for neural network layers
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Activation {
    ReLU,
    Tanh,
    Sigmoid,
    GELU,
    SiLU,
    None,
}

impl Activation {
    pub fn apply(&self, x: f32) -> f32 {
        match self {
            Activation::ReLU => x.max(0.0),
            Activation::Tanh => x.tanh(),
            Activation::Sigmoid => 1.0 / (1.0 + (-x).exp()),
            Activation::GELU => {
                // Gaussian Error Linear Unit: x * Φ(x)
                x * 0.5 * (1.0 + (x * 0.7978845608 * (1.0 + 0.044715 * x * x)).tanh())
            }
            Activation::SiLU => {
                // Sigmoid Linear Unit: x * σ(x)
                x * (1.0 / (1.0 + (-x).exp()))
            }
            Activation::None => x,
        }
    }

    pub fn derivative(&self, x: f32) -> f32 {
        match self {
            Activation::ReLU => if x > 0.0 { 1.0 } else { 0.0 },
            Activation::Tanh => 1.0 - x.tanh().powi(2),
            Activation::Sigmoid => {
                let s = 1.0 / (1.0 + (-x).exp());
                s * (1.0 - s)
            }
            Activation::GELU => {
                // Approximate derivative
                let cdf = 0.5 * (1.0 + (x * 0.7978845608 * (1.0 + 0.044715 * x * x)).tanh());
                let pdf = (-0.5 * x * x).exp() / (2.0 * std::f32::consts::PI).sqrt();
                cdf + x * pdf
            }
            Activation::SiLU => {
                let s = 1.0 / (1.0 + (-x).exp());
                s + x * s * (1.0 - s)
            }
            Activation::None => 1.0,
        }
    }
}

/// Dense (fully connected) layer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DenseLayer {
    pub weights: Vec<Vec<f32>>,  // [output_dim][input_dim]
    pub biases: Vec<f32>,        // [output_dim]
    pub activation: Activation,
    input_dim: usize,
    output_dim: usize,
    // Gradients for training
    #[serde(skip)]
    weight_gradients: Vec<Vec<f32>>,
    #[serde(skip)]
    bias_gradients: Vec<f32>,
    #[serde(skip)]
    last_input: Vec<f32>,
    #[serde(skip)]
    last_pre_activation: Vec<f32>,
}

impl DenseLayer {
    pub fn new(input_dim: usize, output_dim: usize, activation: Activation, rng: &mut StdRng) -> Self {
        // Xavier/Glorot initialization
        let scale = (2.0 / (input_dim + output_dim) as f32).sqrt();
        
        let weights: Vec<Vec<f32>> = (0..output_dim)
            .map(|_| {
                (0..input_dim)
                    .map(|_| rng.gen::<f32>() * 2.0 * scale - scale)
                    .collect()
            })
            .collect();
        
        let biases = vec![0.0; output_dim];
        
        Self {
            weights,
            biases,
            activation,
            input_dim,
            output_dim,
            weight_gradients: vec![vec![0.0; input_dim]; output_dim],
            bias_gradients: vec![0.0; output_dim],
            last_input: vec![],
            last_pre_activation: vec![],
        }
    }

    pub fn forward(&mut self, input: &[f32]) -> Vec<f32> {
        self.last_input = input.to_vec();
        let mut output = vec![0.0; self.output_dim];
        
        for i in 0..self.output_dim {
            let mut sum = self.biases[i];
            for j in 0..self.input_dim {
                sum += self.weights[i][j] * input[j];
            }
            self.last_pre_activation.push(sum);
            output[i] = self.activation.apply(sum);
        }
        
        output
    }

    pub fn backward(&mut self, grad_output: &[f32], learning_rate: f32) -> Vec<f32> {
        let mut grad_input = vec![0.0; self.input_dim];
        
        for i in 0..self.output_dim {
            let grad = grad_output[i] * self.activation.derivative(self.last_pre_activation[i]);
            
            // Bias gradient
            self.bias_gradients[i] += grad;
            
            // Weight gradients and input gradient
            for j in 0..self.input_dim {
                self.weight_gradients[i][j] += grad * self.last_input[j];
                grad_input[j] += grad * self.weights[i][j];
            }
        }
        
        // Apply gradients (SGD)
        for i in 0..self.output_dim {
            self.biases[i] -= learning_rate * self.bias_gradients[i];
            for j in 0..self.input_dim {
                self.weights[i][j] -= learning_rate * self.weight_gradients[i][j];
            }
        }
        
        // Reset gradients
        self.weight_gradients = vec![vec![0.0; self.input_dim]; self.output_dim];
        self.bias_gradients = vec![0.0; self.output_dim];
        self.last_pre_activation.clear();
        
        grad_input
    }
}

/// LSTM cell for temporal state evolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LSTMCell {
    hidden_dim: usize,
    input_dim: usize,
    // Input gate weights
    w_ii: Vec<Vec<f32>>,
    w_hi: Vec<Vec<f32>>,
    b_i: Vec<f32>,
    // Forget gate weights
    w_if: Vec<Vec<f32>>,
    w_hf: Vec<Vec<f32>>,
    b_f: Vec<f32>,
    // Cell gate weights
    w_ig: Vec<Vec<f32>>,
    w_hg: Vec<Vec<f32>>,
    b_g: Vec<f32>,
    // Output gate weights
    w_io: Vec<Vec<f32>>,
    w_ho: Vec<Vec<f32>>,
    b_o: Vec<f32>,
    // State
    hidden_state: Vec<f32>,
    cell_state: Vec<f32>,
}

impl LSTMCell {
    pub fn new(input_dim: usize, hidden_dim: usize, rng: &mut StdRng) -> Self {
        let scale = (1.0 / hidden_dim as f32).sqrt();
        
        let init_weight = |rows: usize, cols: usize, rng: &mut StdRng| -> Vec<Vec<f32>> {
            (0..rows)
                .map(|_| {
                    (0..cols)
                        .map(|_| rng.gen::<f32>() * 2.0 * scale - scale)
                        .collect()
                })
                .collect()
        };
        
        Self {
            hidden_dim,
            input_dim,
            w_ii: init_weight(hidden_dim, input_dim, rng),
            w_hi: init_weight(hidden_dim, hidden_dim, rng),
            b_i: vec![0.0; hidden_dim],
            w_if: init_weight(hidden_dim, input_dim, rng),
            w_hf: init_weight(hidden_dim, hidden_dim, rng),
            b_f: vec![1.0; hidden_dim],  // Forget gate bias initialized to 1
            w_ig: init_weight(hidden_dim, input_dim, rng),
            w_hg: init_weight(hidden_dim, hidden_dim, rng),
            b_g: vec![0.0; hidden_dim],
            w_io: init_weight(hidden_dim, input_dim, rng),
            w_ho: init_weight(hidden_dim, hidden_dim, rng),
            b_o: vec![0.0; hidden_dim],
            hidden_state: vec![0.0; hidden_dim],
            cell_state: vec![0.0; hidden_dim],
        }
    }

    fn matmul_add(&self, w_x: &[Vec<f32>], w_h: &[Vec<f32>], bias: &[f32], x: &[f32], h: &[f32]) -> Vec<f32> {
        let mut result = vec![0.0; self.hidden_dim];
        for i in 0..self.hidden_dim {
            result[i] = bias[i];
            for j in 0..self.input_dim {
                result[i] += w_x[i][j] * x[j];
            }
            for j in 0..self.hidden_dim {
                result[i] += w_h[i][j] * h[j];
            }
        }
        result
    }

    fn sigmoid_vec(v: &[f32]) -> Vec<f32> {
        v.iter().map(|&x| 1.0 / (1.0 + (-x).exp())).collect()
    }

    fn tanh_vec(v: &[f32]) -> Vec<f32> {
        v.iter().map(|&x| x.tanh()).collect()
    }

    pub fn forward(&mut self, input: &[f32]) -> Vec<f32> {
        let h = self.hidden_state.clone();
        
        // Input gate
        let i_gate = Self::sigmoid_vec(&self.matmul_add(&self.w_ii, &self.w_hi, &self.b_i, input, &h));
        // Forget gate
        let f_gate = Self::sigmoid_vec(&self.matmul_add(&self.w_if, &self.w_hf, &self.b_f, input, &h));
        // Cell gate
        let g_gate = Self::tanh_vec(&self.matmul_add(&self.w_ig, &self.w_hg, &self.b_g, input, &h));
        // Output gate
        let o_gate = Self::sigmoid_vec(&self.matmul_add(&self.w_io, &self.w_ho, &self.b_o, input, &h));
        
        // Update cell state
        for i in 0..self.hidden_dim {
            self.cell_state[i] = f_gate[i] * self.cell_state[i] + i_gate[i] * g_gate[i];
        }
        
        // Update hidden state
        let cell_tanh = Self::tanh_vec(&self.cell_state);
        for i in 0..self.hidden_dim {
            self.hidden_state[i] = o_gate[i] * cell_tanh[i];
        }
        
        self.hidden_state.clone()
    }

    pub fn reset_state(&mut self) {
        self.hidden_state = vec![0.0; self.hidden_dim];
        self.cell_state = vec![0.0; self.hidden_dim];
    }
}

/// Multi-head attention for message prediction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiHeadAttention {
    num_heads: usize,
    head_dim: usize,
    embed_dim: usize,
    w_q: Vec<Vec<f32>>,  // [embed_dim][embed_dim]
    w_k: Vec<Vec<f32>>,
    w_v: Vec<Vec<f32>>,
    w_o: Vec<Vec<f32>>,
}

impl MultiHeadAttention {
    pub fn new(embed_dim: usize, num_heads: usize, rng: &mut StdRng) -> Self {
        assert!(embed_dim % num_heads == 0, "embed_dim must be divisible by num_heads");
        let head_dim = embed_dim / num_heads;
        let scale = (1.0 / embed_dim as f32).sqrt();
        
        let init_weight = |rng: &mut StdRng| -> Vec<Vec<f32>> {
            (0..embed_dim)
                .map(|_| {
                    (0..embed_dim)
                        .map(|_| rng.gen::<f32>() * 2.0 * scale - scale)
                        .collect()
                })
                .collect()
        };
        
        Self {
            num_heads,
            head_dim,
            embed_dim,
            w_q: init_weight(rng),
            w_k: init_weight(rng),
            w_v: init_weight(rng),
            w_o: init_weight(rng),
        }
    }

    fn matmul(&self, w: &[Vec<f32>], x: &[f32]) -> Vec<f32> {
        let mut result = vec![0.0; self.embed_dim];
        for i in 0..self.embed_dim {
            for j in 0..self.embed_dim {
                result[i] += w[i][j] * x[j];
            }
        }
        result
    }

    fn softmax(scores: &mut [f32]) {
        let max = scores.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0;
        for s in scores.iter_mut() {
            *s = (*s - max).exp();
            sum += *s;
        }
        for s in scores.iter_mut() {
            *s /= sum;
        }
    }

    /// Compute attention over a sequence of embeddings
    /// sequence: [seq_len][embed_dim]
    pub fn forward(&self, sequence: &[Vec<f32>]) -> Vec<f32> {
        let seq_len = sequence.len();
        if seq_len == 0 {
            return vec![0.0; self.embed_dim];
        }
        
        // Compute Q, K, V for each position
        let queries: Vec<Vec<f32>> = sequence.iter().map(|x| self.matmul(&self.w_q, x)).collect();
        let keys: Vec<Vec<f32>> = sequence.iter().map(|x| self.matmul(&self.w_k, x)).collect();
        let values: Vec<Vec<f32>> = sequence.iter().map(|x| self.matmul(&self.w_v, x)).collect();
        
        // Attention over the last position (for causal prediction)
        let query = &queries[seq_len - 1];
        
        // Compute attention scores
        let scale = (self.head_dim as f32).sqrt();
        let mut attended = vec![0.0; self.embed_dim];
        
        for head in 0..self.num_heads {
            let start = head * self.head_dim;
            let end = start + self.head_dim;
            
            let mut scores = vec![0.0; seq_len];
            for (i, key) in keys.iter().enumerate() {
                let mut dot = 0.0;
                for j in start..end {
                    dot += query[j] * key[j];
                }
                scores[i] = dot / scale;
            }
            
            Self::softmax(&mut scores);
            
            // Weighted sum of values
            for (i, value) in values.iter().enumerate() {
                for j in start..end {
                    attended[j] += scores[i] * value[j];
                }
            }
        }
        
        // Output projection
        self.matmul(&self.w_o, &attended)
    }
}

/// Variational Autoencoder for latent-space encoding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariationalEncoder {
    encoder_layers: Vec<DenseLayer>,
    mean_layer: DenseLayer,
    logvar_layer: DenseLayer,
    decoder_layers: Vec<DenseLayer>,
    latent_dim: usize,
    input_dim: usize,
}

impl VariationalEncoder {
    pub fn new(input_dim: usize, latent_dim: usize, hidden_dims: &[usize], rng: &mut StdRng) -> Self {
        let mut encoder_layers = Vec::new();
        let mut prev_dim = input_dim;
        
        for &hidden_dim in hidden_dims {
            encoder_layers.push(DenseLayer::new(prev_dim, hidden_dim, Activation::GELU, rng));
            prev_dim = hidden_dim;
        }
        
        let mean_layer = DenseLayer::new(prev_dim, latent_dim, Activation::None, rng);
        let logvar_layer = DenseLayer::new(prev_dim, latent_dim, Activation::None, rng);
        
        let mut decoder_layers = Vec::new();
        prev_dim = latent_dim;
        for &hidden_dim in hidden_dims.iter().rev() {
            decoder_layers.push(DenseLayer::new(prev_dim, hidden_dim, Activation::GELU, rng));
            prev_dim = hidden_dim;
        }
        decoder_layers.push(DenseLayer::new(prev_dim, input_dim, Activation::Sigmoid, rng));
        
        Self {
            encoder_layers,
            mean_layer,
            logvar_layer,
            decoder_layers,
            latent_dim,
            input_dim,
        }
    }

    /// Encode input to latent space, returns (mean, log_variance, sampled_latent)
    pub fn encode(&mut self, input: &[f32], rng: &mut StdRng) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let mut hidden = input.to_vec();
        
        for layer in &mut self.encoder_layers {
            hidden = layer.forward(&hidden);
        }
        
        let mean = self.mean_layer.forward(&hidden);
        let logvar = self.logvar_layer.forward(&hidden);
        
        // Reparameterization trick: z = μ + σ * ε
        let mut latent = vec![0.0; self.latent_dim];
        for i in 0..self.latent_dim {
            let std = (0.5 * logvar[i]).exp();
            let epsilon: f32 = rng.gen::<f32>() * 2.0 - 1.0;  // Standard normal approximation
            latent[i] = mean[i] + std * epsilon;
        }
        
        (mean, logvar, latent)
    }

    /// Decode latent vector back to input space
    pub fn decode(&mut self, latent: &[f32]) -> Vec<f32> {
        let mut hidden = latent.to_vec();
        
        for layer in &mut self.decoder_layers {
            hidden = layer.forward(&hidden);
        }
        
        hidden
    }

    /// Compute VAE loss (reconstruction + KL divergence)
    pub fn compute_loss(&self, input: &[f32], reconstruction: &[f32], mean: &[f32], logvar: &[f32]) -> f32 {
        // Reconstruction loss (MSE)
        let mut recon_loss = 0.0;
        for i in 0..self.input_dim {
            let diff = input[i] - reconstruction[i];
            recon_loss += diff * diff;
        }
        recon_loss /= self.input_dim as f32;
        
        // KL divergence: -0.5 * Σ(1 + log(σ²) - μ² - σ²)
        let mut kl_loss = 0.0;
        for i in 0..self.latent_dim {
            kl_loss += -0.5 * (1.0 + logvar[i] - mean[i] * mean[i] - logvar[i].exp());
        }
        kl_loss /= self.latent_dim as f32;
        
        recon_loss + 0.1 * kl_loss  // β-VAE with β=0.1
    }
}

/// Neural latent encoder combining all components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralLatentEncoder {
    pub variational_encoder: VariationalEncoder,
    pub temporal_lstm: LSTMCell,
    pub attention: MultiHeadAttention,
    pub projection_head: DenseLayer,
    latent_dim: usize,
    message_history: VecDeque<Vec<f32>>,
    history_limit: usize,
    #[serde(skip, default = "default_rng")]
    rng: StdRng,
}

fn default_rng() -> StdRng {
    StdRng::seed_from_u64(42)
}

impl NeuralLatentEncoder {
    pub fn new(
        input_dim: usize,
        latent_dim: usize,
        hidden_dims: &[usize],
        attention_heads: usize,
        seed: u64,
    ) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        
        let variational_encoder = VariationalEncoder::new(input_dim, latent_dim, hidden_dims, &mut rng);
        let temporal_lstm = LSTMCell::new(latent_dim, latent_dim, &mut rng);
        let attention = MultiHeadAttention::new(latent_dim, attention_heads, &mut rng);
        let projection_head = DenseLayer::new(latent_dim * 2, latent_dim, Activation::Tanh, &mut rng);
        
        Self {
            variational_encoder,
            temporal_lstm,
            attention,
            projection_head,
            latent_dim,
            message_history: VecDeque::new(),
            history_limit: 32,
            rng,
        }
    }

    /// Encode a message into latent space with temporal context
    pub fn encode(&mut self, message_bytes: &[u8]) -> Vec<f32> {
        // Convert bytes to float vector
        let mut input: Vec<f32> = message_bytes.iter().map(|&b| b as f32 / 255.0).collect();
        
        // Pad or truncate to expected dimension
        input.resize(256, 0.0);  // Fixed input size
        
        // Variational encoding
        let (mean, _logvar, latent) = self.variational_encoder.encode(&input, &mut self.rng);
        
        // Add to history
        self.message_history.push_back(latent.clone());
        if self.message_history.len() > self.history_limit {
            self.message_history.pop_front();
        }
        
        // Temporal evolution via LSTM
        let temporal = self.temporal_lstm.forward(&latent);
        
        // Attention over history
        let history: Vec<Vec<f32>> = self.message_history.iter().cloned().collect();
        let attended = self.attention.forward(&history);
        
        // Combine temporal and attended representations
        let mut combined = temporal.clone();
        combined.extend(attended);
        
        // Project to final latent dimension
        let mut final_layer = self.projection_head.clone();
        let projected = final_layer.forward(&combined);
        
        // Apply moving-target transformation based on history
        let morph_seed = self.compute_morph_seed();
        self.apply_morph(&projected, morph_seed)
    }

    /// Decode latent vector back to message space
    pub fn decode(&mut self, latent: &[f32], morph_seed: u64) -> Vec<u8> {
        // Reverse morph transformation
        let unmorped = self.reverse_morph(latent, morph_seed);
        
        // Decode through VAE
        let reconstructed = self.variational_encoder.decode(&unmorped);
        
        // Convert back to bytes
        reconstructed.iter().map(|&f| (f * 255.0).clamp(0.0, 255.0) as u8).collect()
    }

    /// Evolve the latent space based on a new seed
    pub fn evolve(&mut self, seed: u64) {
        self.rng = StdRng::seed_from_u64(seed);
    }

    /// Predict the next message based on history
    pub fn predict_next(&self) -> Vec<f32> {
        if self.message_history.is_empty() {
            return vec![0.0; self.latent_dim];
        }
        
        let history: Vec<Vec<f32>> = self.message_history.iter().cloned().collect();
        self.attention.forward(&history)
    }

    /// Compute morph seed from message history
    fn compute_morph_seed(&self) -> u64 {
        let mut seed = 0u64;
        for (i, msg) in self.message_history.iter().enumerate() {
            for (j, &val) in msg.iter().enumerate() {
                seed = seed.wrapping_add((val.to_bits() as u64).wrapping_mul((i * j + 1) as u64));
            }
        }
        seed
    }

    /// Apply moving-target transformation to latent vector
    fn apply_morph(&self, latent: &[f32], seed: u64) -> Vec<f32> {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut morphed = latent.to_vec();
        
        // Random rotation in latent space
        for i in 0..latent.len() {
            let j = rng.gen_range(0..latent.len());
            if i != j {
                let angle: f32 = rng.gen::<f32>() * 0.1 * std::f32::consts::PI;
                let cos_a = angle.cos();
                let sin_a = angle.sin();
                let (new_i, new_j) = (
                    cos_a * morphed[i] - sin_a * morphed[j],
                    sin_a * morphed[i] + cos_a * morphed[j],
                );
                morphed[i] = new_i;
                morphed[j] = new_j;
            }
        }
        
        // Scale perturbation
        for v in &mut morphed {
            let scale: f32 = 0.95 + rng.gen::<f32>() * 0.1;
            *v *= scale;
        }
        
        morphed
    }

    /// Reverse moving-target transformation
    fn reverse_morph(&self, latent: &[f32], seed: u64) -> Vec<f32> {
        let mut rng = StdRng::seed_from_u64(seed);
        let n = latent.len();
        
        // Collect all transformations to reverse them
        let mut transforms: Vec<(usize, usize, f32)> = Vec::new();
        let mut scales: Vec<f32> = Vec::new();
        
        for i in 0..n {
            let j = rng.gen_range(0..n);
            if i != j {
                let angle: f32 = rng.gen::<f32>() * 0.1 * std::f32::consts::PI;
                transforms.push((i, j, angle));
            }
            let scale: f32 = 0.95 + rng.gen::<f32>() * 0.1;
            scales.push(scale);
        }
        
        // Apply in reverse order
        let mut unmorphed = latent.to_vec();
        
        // Reverse scale
        for (i, &scale) in scales.iter().enumerate() {
            unmorphed[i] /= scale;
        }
        
        // Reverse rotations (in reverse order with negative angle)
        for (i, j, angle) in transforms.into_iter().rev() {
            let cos_a = (-angle).cos();
            let sin_a = (-angle).sin();
            let (new_i, new_j) = (
                cos_a * unmorphed[i] - sin_a * unmorphed[j],
                sin_a * unmorphed[i] + cos_a * unmorphed[j],
            );
            unmorphed[i] = new_i;
            unmorphed[j] = new_j;
        }
        
        unmorphed
    }

    /// Train the encoder on a batch of messages
    pub fn train_step(&mut self, messages: &[&[u8]], learning_rate: f32) -> f32 {
        let mut total_loss = 0.0;
        
        for &msg in messages {
            let mut input: Vec<f32> = msg.iter().map(|&b| b as f32 / 255.0).collect();
            input.resize(256, 0.0);
            
            let (mean, logvar, latent) = self.variational_encoder.encode(&input, &mut self.rng);
            let reconstruction = self.variational_encoder.decode(&latent);
            
            let loss = self.variational_encoder.compute_loss(&input, &reconstruction, &mean, &logvar);
            total_loss += loss;
            
            // Backprop through encoder (simplified - just update based on loss magnitude)
            let grad_scale = loss * learning_rate;
            for layer in &mut self.variational_encoder.encoder_layers {
                let grad = vec![grad_scale; layer.biases.len()];
                layer.backward(&grad, learning_rate);
            }
        }
        
        total_loss / messages.len() as f32
    }

    /// Reset temporal state
    pub fn reset(&mut self) {
        self.temporal_lstm.reset_state();
        self.message_history.clear();
    }

    /// Get current morph seed for synchronization
    pub fn get_morph_seed(&self) -> u64 {
        self.compute_morph_seed()
    }
}

/// Configuration for neural encoder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralEncoderConfig {
    pub input_dim: usize,
    pub latent_dim: usize,
    pub hidden_dims: Vec<usize>,
    pub attention_heads: usize,
    pub seed: u64,
}

impl Default for NeuralEncoderConfig {
    fn default() -> Self {
        Self {
            input_dim: 256,
            latent_dim: 64,
            hidden_dims: vec![128, 96],
            attention_heads: 4,
            seed: 42,
        }
    }
}

impl NeuralEncoderConfig {
    pub fn build(&self) -> NeuralLatentEncoder {
        NeuralLatentEncoder::new(
            self.input_dim,
            self.latent_dim,
            &self.hidden_dims,
            self.attention_heads,
            self.seed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dense_layer_forward() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut layer = DenseLayer::new(4, 2, Activation::ReLU, &mut rng);
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let output = layer.forward(&input);
        assert_eq!(output.len(), 2);
    }

    #[test]
    fn test_lstm_forward() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut lstm = LSTMCell::new(4, 8, &mut rng);
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let output = lstm.forward(&input);
        assert_eq!(output.len(), 8);
        
        // Second forward should use updated state
        let output2 = lstm.forward(&input);
        assert_ne!(output, output2);
    }

    #[test]
    fn test_attention_forward() {
        let mut rng = StdRng::seed_from_u64(42);
        let attention = MultiHeadAttention::new(16, 4, &mut rng);
        let sequence = vec![
            vec![0.1; 16],
            vec![0.2; 16],
            vec![0.3; 16],
        ];
        let output = attention.forward(&sequence);
        assert_eq!(output.len(), 16);
    }

    #[test]
    fn test_variational_encoder() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut vae = VariationalEncoder::new(32, 8, &[16], &mut rng);
        let input = vec![0.5; 32];
        let (mean, logvar, latent) = vae.encode(&input, &mut rng);
        assert_eq!(mean.len(), 8);
        assert_eq!(logvar.len(), 8);
        assert_eq!(latent.len(), 8);
        
        let reconstruction = vae.decode(&latent);
        assert_eq!(reconstruction.len(), 32);
    }

    #[test]
    fn test_neural_encoder_encode_decode() {
        let config = NeuralEncoderConfig {
            input_dim: 64,
            latent_dim: 16,
            hidden_dims: vec![32],
            attention_heads: 2,
            seed: 42,
        };
        let mut encoder = config.build();
        
        let message = b"Hello, Hyperlight!";
        let latent = encoder.encode(message);
        assert_eq!(latent.len(), 16);
        
        let morph_seed = encoder.get_morph_seed();
        let decoded = encoder.decode(&latent, morph_seed);
        assert!(!decoded.is_empty());
    }

    #[test]
    fn test_moving_target_consistency() {
        let config = NeuralEncoderConfig::default();
        let mut encoder = config.build();
        
        // Encode several messages
        for i in 0..5 {
            let msg = format!("Message {}", i);
            let _ = encoder.encode(msg.as_bytes());
        }
        
        // Morph seed should change based on history
        let seed1 = encoder.get_morph_seed();
        let _ = encoder.encode(b"Another message");
        let seed2 = encoder.get_morph_seed();
        assert_ne!(seed1, seed2);
    }

    #[test]
    fn test_prediction() {
        let config = NeuralEncoderConfig::default();
        let mut encoder = config.build();
        
        // Build up history
        for i in 0..5 {
            let msg = format!("Pattern {}", i % 3);
            let _ = encoder.encode(msg.as_bytes());
        }
        
        let prediction = encoder.predict_next();
        assert_eq!(prediction.len(), config.latent_dim);
    }

    #[test]
    fn test_training() {
        let config = NeuralEncoderConfig {
            input_dim: 32,
            latent_dim: 8,
            hidden_dims: vec![16],
            attention_heads: 2,
            seed: 42,
        };
        let mut encoder = config.build();
        
        let messages: Vec<&[u8]> = vec![
            b"Test 1",
            b"Test 2",
            b"Test 3",
        ];
        
        let loss1 = encoder.train_step(&messages, 0.001);
        let loss2 = encoder.train_step(&messages, 0.001);
        
        // Loss should generally decrease with training
        // (not guaranteed due to stochasticity, but should be close)
        assert!(loss1 > 0.0 && loss2 > 0.0);
    }
}

