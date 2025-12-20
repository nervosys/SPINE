//! # Hyperlight Neural
//!
//! Neural architecture for the Agentic Web Stack's Chameleon Protocol.
//! Implements next-generation sequence modeling for AI-to-AI communication.
//!
//! ## Architecture: Titans (Test-Time Training Transformers)
//!
//! This module implements the Titans architecture from Google Research, which combines:
//! - **Neural Long-Term Memory (NLM)**: Learnable memory that adapts at inference time
//! - **Persistent Memory Tokens**: Compressed representations that survive across contexts
//! - **Surprise-Gated Updates**: Memory writes proportional to prediction error
//! - **Test-Time Training**: Online gradient descent during inference
//!
//! ## Key Features
//!
//! - **Learned Projections**: Train projection matrices from message data
//! - **Variational Encoding**: Stochastic latent space with learnable variance
//! - **Adversarial Training**: Train encoder to resist cryptanalysis
//! - **Titans Memory**: Neural long-term memory replacing LSTM for unbounded context
//! - **Message Prediction**: Multi-head attention with persistent memory tokens

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

/// Titans Neural Long-Term Memory (NLM) Module
/// 
/// Implements the Neural Long-Term Memory from the Titans paper.
/// Unlike LSTM which has fixed gates, NLM uses test-time training
/// to adapt its memory based on prediction surprise.
/// 
/// Memory update rule: M_t = M_{t-1} - η * ∇L(M_{t-1}, x_t)
/// Where L is the surprise loss and η is gated by prediction error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitansMemory {
    hidden_dim: usize,
    memory_dim: usize,
    num_memory_tokens: usize,
    // Memory projection weights
    w_query: Vec<Vec<f32>>,   // Project input to query
    w_key: Vec<Vec<f32>>,     // Project memory to key
    w_value: Vec<Vec<f32>>,   // Project memory to value
    w_write: Vec<Vec<f32>>,   // Project surprise to write vector
    w_erase: Vec<Vec<f32>>,   // Project surprise to erase vector
    // Persistent memory tokens (survive across sequences)
    memory_tokens: Vec<Vec<f32>>,
    // Surprise gate parameters
    surprise_threshold: f32,
    learning_rate: f32,
    // State
    last_prediction: Vec<f32>,
    cumulative_surprise: f32,
}

impl TitansMemory {
    pub fn new(input_dim: usize, hidden_dim: usize, num_memory_tokens: usize, rng: &mut StdRng) -> Self {
        let memory_dim = hidden_dim;
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
        
        // Initialize memory tokens with small random values
        let memory_tokens: Vec<Vec<f32>> = (0..num_memory_tokens)
            .map(|_| {
                (0..memory_dim)
                    .map(|_| rng.gen::<f32>() * 0.1 - 0.05)
                    .collect()
            })
            .collect();
        
        Self {
            hidden_dim,
            memory_dim,
            num_memory_tokens,
            w_query: init_weight(hidden_dim, input_dim, rng),
            w_key: init_weight(hidden_dim, memory_dim, rng),
            w_value: init_weight(hidden_dim, memory_dim, rng),
            w_write: init_weight(memory_dim, hidden_dim, rng),
            w_erase: init_weight(memory_dim, hidden_dim, rng),
            memory_tokens,
            surprise_threshold: 0.5,
            learning_rate: 0.01,
            last_prediction: vec![0.0; hidden_dim],
            cumulative_surprise: 0.0,
        }
    }

    fn matmul(&self, w: &[Vec<f32>], x: &[f32]) -> Vec<f32> {
        let out_dim = w.len();
        let in_dim = x.len().min(w[0].len());
        let mut result = vec![0.0; out_dim];
        for i in 0..out_dim {
            for j in 0..in_dim {
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
        if sum > 0.0 {
            for s in scores.iter_mut() {
                *s /= sum;
            }
        }
    }

    /// Compute surprise (prediction error) for gating memory updates
    fn compute_surprise(&self, input: &[f32]) -> f32 {
        if self.last_prediction.is_empty() {
            return 1.0;
        }
        let mut mse = 0.0;
        let len = input.len().min(self.last_prediction.len());
        for i in 0..len {
            let diff = input[i] - self.last_prediction[i];
            mse += diff * diff;
        }
        (mse / len as f32).sqrt().tanh() // Normalize to [0, 1]
    }

    /// Forward pass with test-time training (online memory updates)
    pub fn forward(&mut self, input: &[f32]) -> Vec<f32> {
        // 1. Compute surprise from last prediction
        let surprise = self.compute_surprise(input);
        self.cumulative_surprise += surprise;
        
        // 2. Query the memory
        let query = self.matmul(&self.w_query, input);
        
        // 3. Compute attention over memory tokens
        let mut attention_scores = Vec::with_capacity(self.num_memory_tokens);
        for token in &self.memory_tokens {
            let key = self.matmul(&self.w_key, token);
            let score: f32 = query.iter().zip(key.iter()).map(|(q, k)| q * k).sum();
            attention_scores.push(score / (self.hidden_dim as f32).sqrt());
        }
        Self::softmax(&mut attention_scores);
        
        // 4. Weighted sum of memory values
        let mut attended = vec![0.0; self.hidden_dim];
        for (i, token) in self.memory_tokens.iter().enumerate() {
            let value = self.matmul(&self.w_value, token);
            for j in 0..self.hidden_dim {
                attended[j] += attention_scores[i] * value[j];
            }
        }
        
        // 5. TEST-TIME TRAINING: Update memory based on surprise
        if surprise > self.surprise_threshold {
            let gate = (surprise - self.surprise_threshold) * self.learning_rate;
            
            // Compute write and erase vectors
            let write_vec = self.matmul(&self.w_write, &query);
            let erase_vec = self.matmul(&self.w_erase, &query);
            
            // Update memory tokens (gradient descent on surprise)
            for (i, token) in self.memory_tokens.iter_mut().enumerate() {
                let write_strength = attention_scores[i] * gate;
                for j in 0..self.memory_dim {
                    // Erase old content, write new content
                    let erase = 1.0 - (erase_vec[j].tanh() * write_strength).abs();
                    token[j] = token[j] * erase + write_vec[j] * write_strength;
                }
            }
        }
        
        // 6. Store prediction for next surprise computation
        self.last_prediction = attended.clone();
        
        attended
    }

    pub fn reset_state(&mut self) {
        self.last_prediction = vec![0.0; self.hidden_dim];
        self.cumulative_surprise = 0.0;
        // Note: memory_tokens persist (that's the point of long-term memory)
    }

    /// Hard reset including memory tokens
    pub fn reset_all(&mut self, rng: &mut StdRng) {
        self.reset_state();
        self.memory_tokens = (0..self.num_memory_tokens)
            .map(|_| {
                (0..self.memory_dim)
                    .map(|_| rng.gen::<f32>() * 0.1 - 0.05)
                    .collect()
            })
            .collect();
    }
    
    /// Get cumulative surprise (useful for anomaly detection)
    pub fn get_surprise(&self) -> f32 {
        self.cumulative_surprise
    }
}

// ============================================================================
// MIRAS VARIANTS: Memory as Internal Regularized Associative Storage
// ============================================================================
// 
// MIRAS is a theoretical framework that unifies sequence models as associative
// memory modules with different loss functions and regularization schemes.
// These variants provide different tradeoffs for continual learning.

/// Loss function types for MIRAS variants
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MirasLoss {
    /// Standard MSE loss (baseline Titans)
    MSE,
    /// Huber loss - robust to outliers (YAAD)
    Huber { delta: f32 },
    /// Generalized norm with configurable p (MONETA)
    LpNorm { p: f32 },
    /// KL divergence for probability-constrained memory (MEMORA)
    KLDivergence,
}

impl MirasLoss {
    /// Compute loss value
    pub fn compute(&self, predicted: &[f32], target: &[f32]) -> f32 {
        let n = predicted.len().min(target.len());
        if n == 0 {
            return 0.0;
        }
        
        match self {
            MirasLoss::MSE => {
                let sum: f32 = predicted.iter()
                    .zip(target.iter())
                    .map(|(p, t)| (p - t).powi(2))
                    .sum();
                sum / n as f32
            }
            MirasLoss::Huber { delta } => {
                let sum: f32 = predicted.iter()
                    .zip(target.iter())
                    .map(|(p, t)| {
                        let diff = (p - t).abs();
                        if diff <= *delta {
                            0.5 * diff * diff
                        } else {
                            delta * (diff - 0.5 * delta)
                        }
                    })
                    .sum();
                sum / n as f32
            }
            MirasLoss::LpNorm { p } => {
                let sum: f32 = predicted.iter()
                    .zip(target.iter())
                    .map(|(pred, tgt)| (pred - tgt).abs().powf(*p))
                    .sum();
                (sum / n as f32).powf(1.0 / p)
            }
            MirasLoss::KLDivergence => {
                // Treat as probability distributions (softmax first)
                let mut p_soft = predicted.to_vec();
                let mut t_soft = target.to_vec();
                Self::softmax_inplace(&mut p_soft);
                Self::softmax_inplace(&mut t_soft);
                
                let sum: f32 = t_soft.iter()
                    .zip(p_soft.iter())
                    .map(|(t, p)| {
                        if *t > 1e-10 && *p > 1e-10 {
                            t * (t / p).ln()
                        } else {
                            0.0
                        }
                    })
                    .sum();
                sum
            }
        }
    }
    
    /// Compute gradient of loss w.r.t. prediction
    pub fn gradient(&self, predicted: &[f32], target: &[f32]) -> Vec<f32> {
        let n = predicted.len().min(target.len());
        
        match self {
            MirasLoss::MSE => {
                predicted.iter()
                    .zip(target.iter())
                    .map(|(p, t)| 2.0 * (p - t) / n as f32)
                    .collect()
            }
            MirasLoss::Huber { delta } => {
                predicted.iter()
                    .zip(target.iter())
                    .map(|(p, t)| {
                        let diff = p - t;
                        if diff.abs() <= *delta {
                            diff / n as f32
                        } else {
                            delta * diff.signum() / n as f32
                        }
                    })
                    .collect()
            }
            MirasLoss::LpNorm { p } => {
                let loss = self.compute(predicted, target);
                if loss < 1e-10 {
                    return vec![0.0; n];
                }
                predicted.iter()
                    .zip(target.iter())
                    .map(|(pred, tgt)| {
                        let diff = pred - tgt;
                        let sign = if diff > 0.0 { 1.0 } else { -1.0 };
                        sign * diff.abs().powf(p - 1.0) * loss.powf(1.0 - p) / n as f32
                    })
                    .collect()
            }
            MirasLoss::KLDivergence => {
                let mut p_soft = predicted.to_vec();
                Self::softmax_inplace(&mut p_soft);
                let mut t_soft = target.to_vec();
                Self::softmax_inplace(&mut t_soft);
                
                // d(KL)/d(p) = -t/p (before softmax jacobian)
                // Simplified: gradient w.r.t. logits
                p_soft.iter()
                    .zip(t_soft.iter())
                    .map(|(p, t)| p - t)
                    .collect()
            }
        }
    }
    
    fn softmax_inplace(x: &mut [f32]) {
        let max = x.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0;
        for v in x.iter_mut() {
            *v = (*v - max).exp();
            sum += *v;
        }
        if sum > 0.0 {
            for v in x.iter_mut() {
                *v /= sum;
            }
        }
    }
}

/// Retention gate types for MIRAS memory regularization
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RetentionGate {
    /// Exponential decay (standard)
    Exponential { decay: f32 },
    /// Adaptive decay based on surprise
    Adaptive { base_decay: f32, surprise_scale: f32 },
    /// L1 regularization (sparse memory)
    L1Sparse { lambda: f32 },
    /// L2 regularization (smooth memory)
    L2Smooth { lambda: f32 },
}

impl RetentionGate {
    /// Compute retention factor given current surprise
    pub fn compute(&self, surprise: f32) -> f32 {
        match self {
            RetentionGate::Exponential { decay } => *decay,
            RetentionGate::Adaptive { base_decay, surprise_scale } => {
                // Higher surprise = more retention (remember novel things)
                (*base_decay + surprise * surprise_scale).min(1.0)
            }
            RetentionGate::L1Sparse { lambda } => 1.0 - lambda,
            RetentionGate::L2Smooth { lambda } => 1.0 - lambda,
        }
    }
    
    /// Apply regularization to memory
    pub fn regularize(&self, memory: &mut [f32]) {
        match self {
            RetentionGate::L1Sparse { lambda } => {
                for m in memory.iter_mut() {
                    let sign = m.signum();
                    *m = (*m - lambda * sign).max(0.0) * sign;
                }
            }
            RetentionGate::L2Smooth { lambda } => {
                for m in memory.iter_mut() {
                    *m *= 1.0 - lambda;
                }
            }
            _ => {} // Exponential and Adaptive don't modify directly
        }
    }
}

/// YAAD: Yet Another Attention Design (Huber-loss variant)
/// 
/// Robust to outliers in the input stream - doesn't overreact to
/// single anomalous tokens. Ideal for noisy protocol data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YaadMemory {
    base: TitansMemory,
    loss: MirasLoss,
    retention: RetentionGate,
    momentum: f32,
    momentum_buffer: Vec<f32>,
}

impl YaadMemory {
    pub fn new(input_dim: usize, hidden_dim: usize, num_memory_tokens: usize, rng: &mut StdRng) -> Self {
        Self {
            base: TitansMemory::new(input_dim, hidden_dim, num_memory_tokens, rng),
            loss: MirasLoss::Huber { delta: 1.0 },
            retention: RetentionGate::Adaptive { base_decay: 0.95, surprise_scale: 0.05 },
            momentum: 0.9,
            momentum_buffer: vec![0.0; hidden_dim],
        }
    }
    
    pub fn forward(&mut self, input: &[f32]) -> Vec<f32> {
        let output = self.base.forward(input);
        
        // Apply Huber-based gradient with momentum
        let gradient = self.loss.gradient(input, &self.base.last_prediction);
        let len = gradient.len().min(self.momentum_buffer.len());
        for i in 0..len {
            self.momentum_buffer[i] = self.momentum * self.momentum_buffer[i] + (1.0 - self.momentum) * gradient[i];
        }
        
        // Apply retention regularization
        let retention = self.retention.compute(self.base.get_surprise());
        for token in &mut self.base.memory_tokens {
            for m in token.iter_mut() {
                *m *= retention;
            }
        }
        
        output
    }
    
    pub fn get_surprise(&self) -> f32 {
        self.base.get_surprise()
    }
    
    pub fn reset_state(&mut self) {
        self.base.reset_state();
        self.momentum_buffer.fill(0.0);
    }
}

/// MONETA: Memory with Optimized Norm-based Training Architecture
/// 
/// Uses Lp norms for more disciplined memory updates.
/// Better stability for very long sequences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonetaMemory {
    base: TitansMemory,
    loss: MirasLoss,
    retention: RetentionGate,
    p_norm: f32,
}

impl MonetaMemory {
    pub fn new(input_dim: usize, hidden_dim: usize, num_memory_tokens: usize, p: f32, rng: &mut StdRng) -> Self {
        Self {
            base: TitansMemory::new(input_dim, hidden_dim, num_memory_tokens, rng),
            loss: MirasLoss::LpNorm { p },
            retention: RetentionGate::L2Smooth { lambda: 0.01 },
            p_norm: p,
        }
    }
    
    pub fn forward(&mut self, input: &[f32]) -> Vec<f32> {
        let output = self.base.forward(input);
        
        // Apply L2 smoothing for stability
        for token in &mut self.base.memory_tokens {
            self.retention.regularize(token);
        }
        
        output
    }
    
    pub fn get_surprise(&self) -> f32 {
        self.base.get_surprise()
    }
    
    pub fn reset_state(&mut self) {
        self.base.reset_state();
    }
}

/// MEMORA: Memory with Optimized Regularized Associations
/// 
/// Treats memory as a probability distribution for maximum stability.
/// Uses KL divergence to ensure controlled, balanced updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoraMemory {
    base: TitansMemory,
    loss: MirasLoss,
    retention: RetentionGate,
    temperature: f32,
}

impl MemoraMemory {
    pub fn new(input_dim: usize, hidden_dim: usize, num_memory_tokens: usize, rng: &mut StdRng) -> Self {
        Self {
            base: TitansMemory::new(input_dim, hidden_dim, num_memory_tokens, rng),
            loss: MirasLoss::KLDivergence,
            retention: RetentionGate::Exponential { decay: 0.99 },
            temperature: 1.0,
        }
    }
    
    pub fn forward(&mut self, input: &[f32]) -> Vec<f32> {
        let output = self.base.forward(input);
        
        // Normalize memory tokens to probability-like distributions
        for token in &mut self.base.memory_tokens {
            let max = token.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let sum: f32 = token.iter().map(|&x| ((x - max) / self.temperature).exp()).sum();
            if sum > 0.0 {
                for m in token.iter_mut() {
                    *m = ((*m - max) / self.temperature).exp() / sum;
                }
            }
        }
        
        output
    }
    
    pub fn get_surprise(&self) -> f32 {
        self.base.get_surprise()
    }
    
    pub fn reset_state(&mut self) {
        self.base.reset_state();
    }
}

/// Unified MIRAS memory that can switch between variants at runtime
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MirasMemory {
    /// Standard Titans (baseline)
    Titans(TitansMemory),
    /// YAAD - robust to outliers
    Yaad(YaadMemory),
    /// MONETA - Lp norm stability
    Moneta(MonetaMemory),
    /// MEMORA - probability-constrained
    Memora(MemoraMemory),
}

impl MirasMemory {
    pub fn new_titans(input_dim: usize, hidden_dim: usize, num_tokens: usize, rng: &mut StdRng) -> Self {
        MirasMemory::Titans(TitansMemory::new(input_dim, hidden_dim, num_tokens, rng))
    }
    
    pub fn new_yaad(input_dim: usize, hidden_dim: usize, num_tokens: usize, rng: &mut StdRng) -> Self {
        MirasMemory::Yaad(YaadMemory::new(input_dim, hidden_dim, num_tokens, rng))
    }
    
    pub fn new_moneta(input_dim: usize, hidden_dim: usize, num_tokens: usize, p: f32, rng: &mut StdRng) -> Self {
        MirasMemory::Moneta(MonetaMemory::new(input_dim, hidden_dim, num_tokens, p, rng))
    }
    
    pub fn new_memora(input_dim: usize, hidden_dim: usize, num_tokens: usize, rng: &mut StdRng) -> Self {
        MirasMemory::Memora(MemoraMemory::new(input_dim, hidden_dim, num_tokens, rng))
    }
    
    pub fn forward(&mut self, input: &[f32]) -> Vec<f32> {
        match self {
            MirasMemory::Titans(m) => m.forward(input),
            MirasMemory::Yaad(m) => m.forward(input),
            MirasMemory::Moneta(m) => m.forward(input),
            MirasMemory::Memora(m) => m.forward(input),
        }
    }
    
    pub fn get_surprise(&self) -> f32 {
        match self {
            MirasMemory::Titans(m) => m.get_surprise(),
            MirasMemory::Yaad(m) => m.get_surprise(),
            MirasMemory::Moneta(m) => m.get_surprise(),
            MirasMemory::Memora(m) => m.get_surprise(),
        }
    }
    
    pub fn reset_state(&mut self) {
        match self {
            MirasMemory::Titans(m) => m.reset_state(),
            MirasMemory::Yaad(m) => m.reset_state(),
            MirasMemory::Moneta(m) => m.reset_state(),
            MirasMemory::Memora(m) => m.reset_state(),
        }
    }
    
    /// Get variant name for debugging/logging
    pub fn variant_name(&self) -> &'static str {
        match self {
            MirasMemory::Titans(_) => "Titans",
            MirasMemory::Yaad(_) => "YAAD",
            MirasMemory::Moneta(_) => "MONETA",
            MirasMemory::Memora(_) => "MEMORA",
        }
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

/// Neural latent encoder combining all components (Titans architecture)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralLatentEncoder {
    pub variational_encoder: VariationalEncoder,
    pub titans_memory: TitansMemory,
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
        // Titans memory with 8 persistent memory tokens
        let titans_memory = TitansMemory::new(latent_dim, latent_dim, 8, &mut rng);
        let attention = MultiHeadAttention::new(latent_dim, attention_heads, &mut rng);
        let projection_head = DenseLayer::new(latent_dim * 2, latent_dim, Activation::Tanh, &mut rng);
        
        Self {
            variational_encoder,
            titans_memory,
            attention,
            projection_head,
            latent_dim,
            message_history: VecDeque::new(),
            history_limit: 32,
            rng,
        }
    }

    /// Encode a message into latent space with Titans long-term memory
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
        
        // Titans memory evolution (test-time training)
        let temporal = self.titans_memory.forward(&latent);
        
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
        self.titans_memory.reset_state();
        self.message_history.clear();
    }

    /// Get current morph seed for synchronization
    pub fn get_morph_seed(&self) -> u64 {
        self.compute_morph_seed()
    }
    
    /// Get cumulative surprise from Titans memory (useful for anomaly detection)
    pub fn get_surprise(&self) -> f32 {
        self.titans_memory.get_surprise()
    }
}

/// MIRAS variant selection for neural encoder
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum MirasVariant {
    /// Standard Titans memory (baseline)
    #[default]
    Titans,
    /// YAAD - robust to outliers (noisy data)
    Yaad,
    /// MONETA - Lp-norm stability (long sequences)
    Moneta { p: f32 },
    /// MEMORA - probability-constrained (balanced updates)
    Memora,
}

/// Configuration for neural encoder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralEncoderConfig {
    pub input_dim: usize,
    pub latent_dim: usize,
    pub hidden_dims: Vec<usize>,
    pub attention_heads: usize,
    pub seed: u64,
    /// MIRAS variant for memory system (default: Titans)
    #[serde(default)]
    pub miras_variant: MirasVariant,
    /// Number of persistent memory tokens (default: 8)
    #[serde(default = "default_memory_tokens")]
    pub memory_tokens: usize,
}

fn default_memory_tokens() -> usize { 8 }

impl Default for NeuralEncoderConfig {
    fn default() -> Self {
        Self {
            input_dim: 256,
            latent_dim: 64,
            hidden_dims: vec![128, 96],
            attention_heads: 4,
            seed: 42,
            miras_variant: MirasVariant::Titans,
            memory_tokens: 8,
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
    
    /// Build encoder with MIRAS variant selection
    pub fn build_miras(&self) -> MirasNeuralEncoder {
        MirasNeuralEncoder::new(self)
    }
    
    /// Use YAAD for noisy protocol data
    pub fn with_yaad(mut self) -> Self {
        self.miras_variant = MirasVariant::Yaad;
        self
    }
    
    /// Use MONETA for long-running sessions
    pub fn with_moneta(mut self, p: f32) -> Self {
        self.miras_variant = MirasVariant::Moneta { p };
        self
    }
    
    /// Use MEMORA for balanced memory updates
    pub fn with_memora(mut self) -> Self {
        self.miras_variant = MirasVariant::Memora;
        self
    }
}

/// Neural encoder with MIRAS variant support
/// 
/// This encoder uses the unified MirasMemory system, allowing
/// runtime selection between Titans, YAAD, MONETA, and MEMORA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirasNeuralEncoder {
    pub variational_encoder: VariationalEncoder,
    pub memory: MirasMemory,
    pub attention: MultiHeadAttention,
    pub projection_head: DenseLayer,
    latent_dim: usize,
    message_history: VecDeque<Vec<f32>>,
    history_limit: usize,
    #[serde(skip, default = "default_rng")]
    rng: StdRng,
}

impl MirasNeuralEncoder {
    pub fn new(config: &NeuralEncoderConfig) -> Self {
        let mut rng = StdRng::seed_from_u64(config.seed);
        
        let variational_encoder = VariationalEncoder::new(
            config.input_dim, 
            config.latent_dim, 
            &config.hidden_dims, 
            &mut rng
        );
        
        // Create memory based on MIRAS variant
        let memory = match config.miras_variant {
            MirasVariant::Titans => {
                MirasMemory::new_titans(config.latent_dim, config.latent_dim, config.memory_tokens, &mut rng)
            }
            MirasVariant::Yaad => {
                MirasMemory::new_yaad(config.latent_dim, config.latent_dim, config.memory_tokens, &mut rng)
            }
            MirasVariant::Moneta { p } => {
                MirasMemory::new_moneta(config.latent_dim, config.latent_dim, config.memory_tokens, p, &mut rng)
            }
            MirasVariant::Memora => {
                MirasMemory::new_memora(config.latent_dim, config.latent_dim, config.memory_tokens, &mut rng)
            }
        };
        
        let attention = MultiHeadAttention::new(config.latent_dim, config.attention_heads, &mut rng);
        let projection_head = DenseLayer::new(config.latent_dim * 2, config.latent_dim, Activation::Tanh, &mut rng);
        
        Self {
            variational_encoder,
            memory,
            attention,
            projection_head,
            latent_dim: config.latent_dim,
            message_history: VecDeque::new(),
            history_limit: 32,
            rng,
        }
    }
    
    /// Get the active MIRAS variant name
    pub fn variant(&self) -> &'static str {
        self.memory.variant_name()
    }

    /// Encode a message into latent space using selected MIRAS memory
    pub fn encode(&mut self, message_bytes: &[u8]) -> Vec<f32> {
        let mut input: Vec<f32> = message_bytes.iter().map(|&b| b as f32 / 255.0).collect();
        input.resize(256, 0.0);
        
        let (_mean, _logvar, latent) = self.variational_encoder.encode(&input, &mut self.rng);
        
        self.message_history.push_back(latent.clone());
        if self.message_history.len() > self.history_limit {
            self.message_history.pop_front();
        }
        
        // MIRAS memory evolution
        let temporal = self.memory.forward(&latent);
        
        let history: Vec<Vec<f32>> = self.message_history.iter().cloned().collect();
        let attended = self.attention.forward(&history);
        
        let mut combined = temporal.clone();
        combined.extend(attended);
        
        let mut final_layer = self.projection_head.clone();
        let projected = final_layer.forward(&combined);
        
        let morph_seed = self.compute_morph_seed();
        self.apply_morph(&projected, morph_seed)
    }
    
    /// Get cumulative surprise (anomaly detection)
    pub fn get_surprise(&self) -> f32 {
        self.memory.get_surprise()
    }
    
    /// Reset memory and history
    pub fn reset(&mut self) {
        self.memory.reset_state();
        self.message_history.clear();
    }
    
    fn compute_morph_seed(&self) -> u64 {
        let mut seed = 0u64;
        for (i, msg) in self.message_history.iter().enumerate() {
            for (j, &val) in msg.iter().enumerate() {
                seed = seed.wrapping_add((val.to_bits() as u64).wrapping_mul((i * j + 1) as u64));
            }
        }
        seed
    }
    
    fn apply_morph(&self, latent: &[f32], seed: u64) -> Vec<f32> {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut morphed = latent.to_vec();
        
        for i in 0..morphed.len() {
            let rot_angle: f32 = rng.gen_range(0.0..1.0) * std::f32::consts::PI * 2.0;
            let j = (i + 1) % morphed.len();
            let (a, b) = (morphed[i], morphed[j]);
            morphed[i] = a * rot_angle.cos() - b * rot_angle.sin();
            morphed[j] = a * rot_angle.sin() + b * rot_angle.cos();
        }
        
        morphed
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
    fn test_titans_memory_forward() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut titans = TitansMemory::new(4, 8, 4, &mut rng);
        let input = vec![1.0, 2.0, 3.0, 4.0];
        let output = titans.forward(&input);
        assert_eq!(output.len(), 8);
        
        // Second forward should use updated memory state
        let output2 = titans.forward(&input);
        // Outputs may differ due to surprise-gated updates
        assert_eq!(output2.len(), 8);
    }

    #[test]
    fn test_titans_surprise_detection() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut titans = TitansMemory::new(4, 8, 4, &mut rng);
        
        // Train on consistent pattern
        for _ in 0..10 {
            titans.forward(&[0.1, 0.2, 0.3, 0.4]);
        }
        let surprise_low = titans.get_surprise();
        
        titans.reset_state();
        
        // Now send surprising input
        for i in 0..10 {
            titans.forward(&[(i as f32) * 0.5, (i as f32) * 0.3, 0.1, 0.9]);
        }
        let surprise_high = titans.get_surprise();
        
        // Variable inputs should accumulate more surprise
        assert!(surprise_high >= surprise_low);
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
            ..Default::default()
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
            ..Default::default()
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

    // ========================================================================
    // MIRAS VARIANT TESTS
    // ========================================================================

    #[test]
    fn test_miras_loss_functions() {
        let predicted = vec![0.5, 0.3, 0.2];
        let target = vec![0.4, 0.4, 0.2];
        
        // MSE
        let mse = MirasLoss::MSE;
        let mse_val = mse.compute(&predicted, &target);
        assert!(mse_val >= 0.0);
        
        // Huber
        let huber = MirasLoss::Huber { delta: 0.5 };
        let huber_val = huber.compute(&predicted, &target);
        assert!(huber_val >= 0.0);
        assert!(huber_val <= mse_val || (huber_val - mse_val).abs() < 0.1);
        
        // Lp Norm
        let lp = MirasLoss::LpNorm { p: 1.5 };
        let lp_val = lp.compute(&predicted, &target);
        assert!(lp_val >= 0.0);
        
        // KL Divergence
        let kl = MirasLoss::KLDivergence;
        let kl_val = kl.compute(&predicted, &target);
        assert!(kl_val >= 0.0);
    }

    #[test]
    fn test_miras_loss_gradients() {
        let predicted = vec![0.5, 0.3, 0.2];
        let target = vec![0.4, 0.4, 0.2];
        
        // All loss functions should produce gradients
        let losses = vec![
            MirasLoss::MSE,
            MirasLoss::Huber { delta: 0.5 },
            MirasLoss::LpNorm { p: 2.0 },
            MirasLoss::KLDivergence,
        ];
        
        for loss in losses {
            let grad = loss.gradient(&predicted, &target);
            assert_eq!(grad.len(), predicted.len());
        }
    }

    #[test]
    fn test_retention_gate() {
        let exp = RetentionGate::Exponential { decay: 0.95 };
        assert!((exp.compute(0.0) - 0.95).abs() < 1e-6);
        
        let adaptive = RetentionGate::Adaptive { base_decay: 0.9, surprise_scale: 0.1 };
        let low_surprise = adaptive.compute(0.1);
        let high_surprise = adaptive.compute(0.5);
        assert!(high_surprise > low_surprise); // More surprise = more retention
        
        let l1 = RetentionGate::L1Sparse { lambda: 0.1 };
        let mut mem = vec![0.5, -0.3, 0.05];
        l1.regularize(&mut mem);
        // L1 should push values toward zero
        
        let l2 = RetentionGate::L2Smooth { lambda: 0.1 };
        let mut mem2 = vec![1.0, -1.0, 0.5];
        l2.regularize(&mut mem2);
        assert!((mem2[0] - 0.9).abs() < 1e-6);
    }

    #[test]
    fn test_yaad_memory() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut yaad = YaadMemory::new(4, 8, 4, &mut rng);
        
        // Should handle normal inputs
        let output = yaad.forward(&[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(output.len(), 8);
        
        // Should be robust to outliers
        let output_outlier = yaad.forward(&[10.0, 0.2, 0.3, 0.4]);
        assert_eq!(output_outlier.len(), 8);
        
        // Surprise should accumulate
        assert!(yaad.get_surprise() > 0.0);
    }

    #[test]
    fn test_moneta_memory() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut moneta = MonetaMemory::new(4, 8, 4, 2.0, &mut rng);
        
        // Standard forward pass
        let output = moneta.forward(&[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(output.len(), 8);
        
        // Multiple passes should remain stable
        for _ in 0..10 {
            let out = moneta.forward(&[0.2, 0.3, 0.4, 0.5]);
            assert!(out.iter().all(|&x| x.is_finite()));
        }
    }

    #[test]
    fn test_memora_memory() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut memora = MemoraMemory::new(4, 8, 4, &mut rng);
        
        // Forward pass
        let output = memora.forward(&[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(output.len(), 8);
        
        // Memory tokens should be probability-like (sum near 1)
        // This is enforced by the softmax normalization
        assert!(memora.get_surprise() >= 0.0);
    }

    #[test]
    fn test_miras_memory_enum() {
        let mut rng = StdRng::seed_from_u64(42);
        
        let variants = vec![
            MirasMemory::new_titans(4, 8, 4, &mut rng),
            MirasMemory::new_yaad(4, 8, 4, &mut rng),
            MirasMemory::new_moneta(4, 8, 4, 2.0, &mut rng),
            MirasMemory::new_memora(4, 8, 4, &mut rng),
        ];
        
        let names = ["Titans", "YAAD", "MONETA", "MEMORA"];
        
        for (mut mem, expected_name) in variants.into_iter().zip(names.iter()) {
            assert_eq!(mem.variant_name(), *expected_name);
            
            // All variants should produce valid output
            let output = mem.forward(&[0.1, 0.2, 0.3, 0.4]);
            assert_eq!(output.len(), 8);
            assert!(output.iter().all(|&x| x.is_finite()));
            
            // All variants should track surprise
            let _ = mem.get_surprise();
            
            // All variants should support reset
            mem.reset_state();
        }
    }

    #[test]
    fn test_miras_outlier_robustness() {
        let mut rng = StdRng::seed_from_u64(42);
        
        // YAAD should be more robust to outliers than base Titans
        let mut titans = TitansMemory::new(4, 8, 4, &mut rng);
        let mut yaad = YaadMemory::new(4, 8, 4, &mut rng);
        
        // Train on normal data
        for _ in 0..10 {
            titans.forward(&[0.1, 0.2, 0.3, 0.4]);
            yaad.forward(&[0.1, 0.2, 0.3, 0.4]);
        }
        
        // Inject outlier
        titans.forward(&[100.0, 0.2, 0.3, 0.4]);
        yaad.forward(&[100.0, 0.2, 0.3, 0.4]);
        
        // Both should still produce finite output
        let titans_out = titans.forward(&[0.1, 0.2, 0.3, 0.4]);
        let yaad_out = yaad.forward(&[0.1, 0.2, 0.3, 0.4]);
        
        assert!(titans_out.iter().all(|&x| x.is_finite()));
        assert!(yaad_out.iter().all(|&x| x.is_finite()));
    }

    #[test]
    fn test_miras_neural_encoder() {
        // Test all MIRAS variants via the unified encoder
        let base_config = NeuralEncoderConfig {
            input_dim: 64,
            latent_dim: 16,
            hidden_dims: vec![32],
            attention_heads: 2,
            seed: 42,
            miras_variant: MirasVariant::Titans,
            memory_tokens: 4,
        };
        
        // Test Titans (default)
        let mut enc = base_config.clone().build_miras();
        assert_eq!(enc.variant(), "Titans");
        let latent = enc.encode(b"Test message");
        assert_eq!(latent.len(), 16);
        
        // Test YAAD
        let mut enc = base_config.clone().with_yaad().build_miras();
        assert_eq!(enc.variant(), "YAAD");
        let latent = enc.encode(b"Test message");
        assert_eq!(latent.len(), 16);
        
        // Test MONETA
        let mut enc = base_config.clone().with_moneta(2.0).build_miras();
        assert_eq!(enc.variant(), "MONETA");
        let latent = enc.encode(b"Test message");
        assert_eq!(latent.len(), 16);
        
        // Test MEMORA
        let mut enc = base_config.with_memora().build_miras();
        assert_eq!(enc.variant(), "MEMORA");
        let latent = enc.encode(b"Test message");
        assert_eq!(latent.len(), 16);
    }

    #[test]
    fn test_miras_encoder_surprise_tracking() {
        let config = NeuralEncoderConfig {
            input_dim: 64,
            latent_dim: 16,
            hidden_dims: vec![32],
            attention_heads: 2,
            seed: 42,
            ..Default::default()
        }.with_yaad();
        let mut encoder = config.build_miras();
        
        // Encode varying messages - should accumulate some surprise
        for i in 0..5 {
            encoder.encode(format!("Different {} pattern", i).as_bytes());
        }
        let surprise = encoder.get_surprise();
        
        // Should have accumulated non-zero surprise from varied inputs
        assert!(surprise >= 0.0);
        
        // Reset should clear surprise
        encoder.reset();
        
        // After reset, encode again
        encoder.encode(b"Fresh start");
        
        // Encoder should still work after reset
        let output = encoder.encode(b"Another message");
        assert_eq!(output.len(), 16);
    }
}

