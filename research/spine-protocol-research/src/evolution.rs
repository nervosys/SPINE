//! Protocol evolution: genomes, populations and the co-evolutionary arena.

use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
