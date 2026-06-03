//! # spine-gpu: GPU-Accelerated Neural Encoding
//!
//! Cross-platform GPU compute for neural network operations used in SPINE's
//! latent-space encoding, attention mechanisms, and protocol cryptography.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │            GpuAccelerator               │
//! │  ┌─────────┐  ┌──────────┐  ┌────────┐ │
//! │  │ MatMul  │  │ Softmax  │  │  VAE   │ │
//! │  │ Kernel  │  │ Kernel   │  │Forward │ │
//! │  └────┬────┘  └────┬─────┘  └───┬────┘ │
//! │       └─────────┬──┘────────────┘       │
//! │            ┌────┴─────┐                 │
//! │            │ Backend  │                 │
//! │            └────┬─────┘                 │
//! │    ┌────────────┼────────────┐          │
//! │    ▼            ▼            ▼          │
//! │  wgpu       CPU-SIMD      (CUDA)       │
//! │ Vulkan/     Fallback     Optional       │
//! │ Metal/DX12                              │
//! └─────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - `cpu` (default): SIMD-optimized CPU fallback
//! - `wgpu-backend`: Cross-platform GPU via wgpu (Vulkan/Metal/DX12/WebGPU)

use anyhow::Result;
use std::fmt;

// ============================================================================
// Compute Backend Abstraction
// ============================================================================

/// Device type for compute operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Cpu,
    Gpu,
}

impl fmt::Display for DeviceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceType::Cpu => write!(f, "CPU"),
            DeviceType::Gpu => write!(f, "GPU"),
        }
    }
}

/// Information about a compute device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub device_type: DeviceType,
    pub memory_bytes: u64,
    pub max_workgroup_size: u32,
    pub max_buffer_size: u64,
}

/// Trait for GPU-acceleratable tensor operations.
pub trait ComputeBackend: Send + Sync {
    /// Device information.
    fn device_info(&self) -> &DeviceInfo;

    /// Dense matrix-vector multiply: output = weights × input + bias
    fn mat_vec_mul(
        &self,
        weights: &[f32],
        input: &[f32],
        bias: &[f32],
        rows: usize,
        cols: usize,
        output: &mut [f32],
    ) -> Result<()>;

    /// Dense matrix-matrix multiply: C = A × B
    fn mat_mul(
        &self,
        a: &[f32],
        b: &[f32],
        m: usize,
        k: usize,
        n: usize,
        c: &mut [f32],
    ) -> Result<()>;

    /// Softmax in-place over a vector.
    fn softmax(&self, data: &mut [f32]) -> Result<()>;

    /// Element-wise ReLU in-place.
    fn relu(&self, data: &mut [f32]) -> Result<()>;

    /// Element-wise tanh in-place.
    fn tanh_activate(&self, data: &mut [f32]) -> Result<()>;

    /// Element-wise sigmoid in-place.
    fn sigmoid(&self, data: &mut [f32]) -> Result<()>;

    /// Dot product of two vectors.
    fn dot_product(&self, a: &[f32], b: &[f32]) -> Result<f32>;

    /// Cosine similarity between two vectors.
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> Result<f32>;

    /// Layer normalization in-place.
    fn layer_norm(&self, data: &mut [f32], gamma: &[f32], beta: &[f32], eps: f32) -> Result<()>;

    /// Batched attention: Q × K^T / sqrt(d) → softmax → × V
    fn attention(
        &self,
        query: &[f32],
        key: &[f32],
        value: &[f32],
        seq_len: usize,
        head_dim: usize,
        output: &mut [f32],
    ) -> Result<()>;

    /// VAE encoder forward pass: input → μ, logσ²
    fn vae_encode(
        &self,
        input: &[f32],
        encoder_weights: &[EncoderLayer],
        output_mu: &mut [f32],
        output_logvar: &mut [f32],
    ) -> Result<()>;

    /// VAE reparameterize: z = μ + ε·exp(logσ²/2)
    fn vae_reparameterize(
        &self,
        mu: &[f32],
        logvar: &[f32],
        epsilon: &[f32],
        output: &mut [f32],
    ) -> Result<()>;
}

/// Encoder layer weights for VAE forward pass.
#[derive(Debug, Clone)]
pub struct EncoderLayer {
    pub weights: Vec<f32>,
    pub bias: Vec<f32>,
    pub input_dim: usize,
    pub output_dim: usize,
    pub activation: ActivationKind,
}

/// Activation function types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationKind {
    ReLU,
    Tanh,
    Sigmoid,
    None,
}

// ============================================================================
// CPU Backend (SIMD-optimized fallback)
// ============================================================================

/// CPU compute backend with SIMD-friendly implementations.
pub struct CpuBackend {
    info: DeviceInfo,
}

impl CpuBackend {
    pub fn new() -> Self {
        Self {
            info: DeviceInfo {
                name: "CPU (SIMD fallback)".to_string(),
                device_type: DeviceType::Cpu,
                memory_bytes: 0, // unlimited for CPU
                max_workgroup_size: 1,
                max_buffer_size: u64::MAX,
            },
        }
    }
}

impl Default for CpuBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ComputeBackend for CpuBackend {
    fn device_info(&self) -> &DeviceInfo {
        &self.info
    }

    fn mat_vec_mul(
        &self,
        weights: &[f32],
        input: &[f32],
        bias: &[f32],
        rows: usize,
        cols: usize,
        output: &mut [f32],
    ) -> Result<()> {
        assert_eq!(weights.len(), rows * cols);
        assert_eq!(input.len(), cols);
        assert_eq!(bias.len(), rows);
        assert_eq!(output.len(), rows);

        for i in 0..rows {
            let row = &weights[i * cols..(i + 1) * cols];
            // 8-wide accumulator for SIMD-friendly dot product
            let mut acc = [0.0f32; 8];
            let chunks = cols / 8;
            for c in 0..chunks {
                let base = c * 8;
                for k in 0..8 {
                    acc[k] += row[base + k] * input[base + k];
                }
            }
            let mut sum: f32 = acc.iter().sum();
            for j in (chunks * 8)..cols {
                sum += row[j] * input[j];
            }
            output[i] = sum + bias[i];
        }
        Ok(())
    }

    fn mat_mul(
        &self,
        a: &[f32],
        b: &[f32],
        m: usize,
        k: usize,
        n: usize,
        c: &mut [f32],
    ) -> Result<()> {
        assert_eq!(a.len(), m * k);
        assert_eq!(b.len(), k * n);
        assert_eq!(c.len(), m * n);

        // Row-major: C[i][j] = sum_p A[i][p] * B[p][j]
        for i in 0..m {
            for j in 0..n {
                let mut sum = 0.0f32;
                for p in 0..k {
                    sum += a[i * k + p] * b[p * n + j];
                }
                c[i * n + j] = sum;
            }
        }
        Ok(())
    }

    fn softmax(&self, data: &mut [f32]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        let max = data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0f32;
        for x in data.iter_mut() {
            *x = (*x - max).exp();
            sum += *x;
        }
        if sum > 0.0 {
            for x in data.iter_mut() {
                *x /= sum;
            }
        }
        Ok(())
    }

    fn relu(&self, data: &mut [f32]) -> Result<()> {
        for x in data.iter_mut() {
            *x = x.max(0.0);
        }
        Ok(())
    }

    fn tanh_activate(&self, data: &mut [f32]) -> Result<()> {
        for x in data.iter_mut() {
            *x = x.tanh();
        }
        Ok(())
    }

    fn sigmoid(&self, data: &mut [f32]) -> Result<()> {
        for x in data.iter_mut() {
            *x = 1.0 / (1.0 + (-*x).exp());
        }
        Ok(())
    }

    fn dot_product(&self, a: &[f32], b: &[f32]) -> Result<f32> {
        let len = a.len().min(b.len());
        let mut acc = [0.0f32; 8];
        let chunks = len / 8;
        for c in 0..chunks {
            let base = c * 8;
            for k in 0..8 {
                acc[k] += a[base + k] * b[base + k];
            }
        }
        let mut sum: f32 = acc.iter().sum();
        for i in (chunks * 8)..len {
            sum += a[i] * b[i];
        }
        Ok(sum)
    }

    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> Result<f32> {
        let len = a.len().min(b.len());
        let (mut dot, mut na2, mut nb2) = (0.0f32, 0.0f32, 0.0f32);
        for i in 0..len {
            dot += a[i] * b[i];
            na2 += a[i] * a[i];
            nb2 += b[i] * b[i];
        }
        let denom = na2.sqrt() * nb2.sqrt();
        Ok(if denom > 0.0 { dot / denom } else { 0.0 })
    }

    fn layer_norm(&self, data: &mut [f32], gamma: &[f32], beta: &[f32], eps: f32) -> Result<()> {
        let n = data.len() as f32;
        let mean: f32 = data.iter().sum::<f32>() / n;
        let var: f32 = data.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n;
        let std = (var + eps).sqrt();
        for (i, x) in data.iter_mut().enumerate() {
            *x = gamma[i] * (*x - mean) / std + beta[i];
        }
        Ok(())
    }

    fn attention(
        &self,
        query: &[f32],
        key: &[f32],
        value: &[f32],
        seq_len: usize,
        head_dim: usize,
        output: &mut [f32],
    ) -> Result<()> {
        let scale = (head_dim as f32).sqrt();

        // Compute attention scores: Q × K^T / sqrt(d)
        let mut scores = vec![0.0f32; seq_len * seq_len];
        for i in 0..seq_len {
            for j in 0..seq_len {
                let mut dot = 0.0f32;
                for d in 0..head_dim {
                    dot += query[i * head_dim + d] * key[j * head_dim + d];
                }
                scores[i * seq_len + j] = dot / scale;
            }
        }

        // Softmax per row
        for i in 0..seq_len {
            let row = &mut scores[i * seq_len..(i + 1) * seq_len];
            self.softmax(row)?;
        }

        // Weighted sum: scores × V
        for i in 0..seq_len {
            for d in 0..head_dim {
                let mut sum = 0.0f32;
                for j in 0..seq_len {
                    sum += scores[i * seq_len + j] * value[j * head_dim + d];
                }
                output[i * head_dim + d] = sum;
            }
        }

        Ok(())
    }

    fn vae_encode(
        &self,
        input: &[f32],
        encoder_weights: &[EncoderLayer],
        output_mu: &mut [f32],
        output_logvar: &mut [f32],
    ) -> Result<()> {
        let mut current = input.to_vec();

        // Forward through all but last two layers (mu and logvar heads)
        for layer in &encoder_weights[..encoder_weights.len().saturating_sub(2)] {
            let mut output = vec![0.0f32; layer.output_dim];
            self.mat_vec_mul(
                &layer.weights,
                &current,
                &layer.bias,
                layer.output_dim,
                layer.input_dim,
                &mut output,
            )?;
            match layer.activation {
                ActivationKind::ReLU => self.relu(&mut output)?,
                ActivationKind::Tanh => self.tanh_activate(&mut output)?,
                ActivationKind::Sigmoid => self.sigmoid(&mut output)?,
                ActivationKind::None => {}
            }
            current = output;
        }

        // Mu head
        if encoder_weights.len() >= 2 {
            let mu_layer = &encoder_weights[encoder_weights.len() - 2];
            self.mat_vec_mul(
                &mu_layer.weights,
                &current,
                &mu_layer.bias,
                mu_layer.output_dim,
                mu_layer.input_dim,
                output_mu,
            )?;
        }

        // LogVar head
        if let Some(lv_layer) = encoder_weights.last() {
            self.mat_vec_mul(
                &lv_layer.weights,
                &current,
                &lv_layer.bias,
                lv_layer.output_dim,
                lv_layer.input_dim,
                output_logvar,
            )?;
        }

        Ok(())
    }

    fn vae_reparameterize(
        &self,
        mu: &[f32],
        logvar: &[f32],
        epsilon: &[f32],
        output: &mut [f32],
    ) -> Result<()> {
        for i in 0..mu.len() {
            let std = (logvar[i] * 0.5).exp();
            output[i] = mu[i] + epsilon[i] * std;
        }
        Ok(())
    }
}

// ============================================================================
// wgpu Backend (GPU compute shaders)
// ============================================================================

#[cfg(feature = "wgpu-backend")]
pub mod wgpu_backend {
    use super::*;
    use wgpu::util::DeviceExt;

    /// WGSL compute shader for matrix-vector multiplication.
    const MATVEC_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> weights: array<f32>;
@group(0) @binding(1) var<storage, read> input_vec: array<f32>;
@group(0) @binding(2) var<storage, read> bias: array<f32>;
@group(0) @binding(3) var<storage, read_write> output_vec: array<f32>;

struct Params {
    rows: u32,
    cols: u32,
}
@group(0) @binding(4) var<uniform> params: Params;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.x;
    if row >= params.rows { return; }

    var sum: f32 = 0.0;
    let cols = params.cols;
    let base = row * cols;

    // Vectorized accumulation
    var i: u32 = 0u;
    loop {
        if i >= cols { break; }
        sum += weights[base + i] * input_vec[i];
        i += 1u;
    }

    output_vec[row] = sum + bias[row];
}
"#;

    /// WGSL compute shader for tiled f32 matrix-matrix multiplication.
    /// `c[m,n] = a[m,k] * b[k,n]`, row-major, 16×16 workgroup-local tile.
    const MATMUL_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> mat_a: array<f32>;
@group(0) @binding(1) var<storage, read> mat_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> mat_c: array<f32>;

struct Params {
    m: u32,
    k: u32,
    n: u32,
}
@group(0) @binding(3) var<uniform> params: Params;

const TILE: u32 = 16u;

var<workgroup> tile_a: array<f32, 256>;
var<workgroup> tile_b: array<f32, 256>;

@compute @workgroup_size(16, 16, 1)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
) {
    let row = gid.y;
    let col = gid.x;
    let lr = lid.y;
    let lc = lid.x;
    var sum: f32 = 0.0;

    let num_tiles = (params.k + TILE - 1u) / TILE;
    for (var t: u32 = 0u; t < num_tiles; t = t + 1u) {
        let a_col = t * TILE + lc;
        if (row < params.m && a_col < params.k) {
            tile_a[lr * TILE + lc] = mat_a[row * params.k + a_col];
        } else {
            tile_a[lr * TILE + lc] = 0.0;
        }
        let b_row = t * TILE + lr;
        if (b_row < params.k && col < params.n) {
            tile_b[lr * TILE + lc] = mat_b[b_row * params.n + col];
        } else {
            tile_b[lr * TILE + lc] = 0.0;
        }
        workgroupBarrier();

        for (var i: u32 = 0u; i < TILE; i = i + 1u) {
            sum += tile_a[lr * TILE + i] * tile_b[i * TILE + lc];
        }
        workgroupBarrier();
    }

    if (row < params.m && col < params.n) {
        mat_c[row * params.n + col] = sum;
    }
}
"#;

    /// WGSL compute shader for softmax.
    const SOFTMAX_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

struct Params {
    length: u32,
}
@group(0) @binding(1) var<uniform> params: Params;

// Phase 1: find max (single workgroup reduction)
@compute @workgroup_size(1)
fn find_max(@builtin(global_invocation_id) gid: vec3<u32>) {
    var max_val: f32 = data[0];
    for (var i: u32 = 1u; i < params.length; i++) {
        max_val = max(max_val, data[i]);
    }
    // Subtract max and exponentiate
    var sum: f32 = 0.0;
    for (var i: u32 = 0u; i < params.length; i++) {
        data[i] = exp(data[i] - max_val);
        sum += data[i];
    }
    // Normalize
    for (var i: u32 = 0u; i < params.length; i++) {
        data[i] /= sum;
    }
}
"#;

    /// WGSL compute shader for dot product via parallel reduction.
    const DOT_PRODUCT_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read> vec_a: array<f32>;
@group(0) @binding(1) var<storage, read> vec_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> result: array<f32>;

struct Params {
    length: u32,
}
@group(0) @binding(3) var<uniform> params: Params;

var<workgroup> shared_data: array<f32, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let global_id = wid.x * 256u + lid.x;
    if global_id < params.length {
        shared_data[lid.x] = vec_a[global_id] * vec_b[global_id];
    } else {
        shared_data[lid.x] = 0.0;
    }
    workgroupBarrier();

    // Tree reduction
    var stride: u32 = 128u;
    loop {
        if stride == 0u { break; }
        if lid.x < stride {
            shared_data[lid.x] += shared_data[lid.x + stride];
        }
        workgroupBarrier();
        stride /= 2u;
    }

    if lid.x == 0u {
        result[wid.x] = shared_data[0];
    }
}
"#;

    /// GPU compute backend using wgpu (Vulkan/Metal/DX12/WebGPU).
    pub struct WgpuBackend {
        info: DeviceInfo,
        device: wgpu::Device,
        queue: wgpu::Queue,
        // Pre-compiled pipelines
        matvec_pipeline: wgpu::ComputePipeline,
        matvec_bind_group_layout: wgpu::BindGroupLayout,
        matmul_pipeline: wgpu::ComputePipeline,
        matmul_bind_group_layout: wgpu::BindGroupLayout,
    }

    impl WgpuBackend {
        /// Create a new wgpu backend, requesting the best available GPU.
        pub fn new() -> Result<Self> {
            let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::all(),
                ..Default::default()
            });

            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: None,
                }))
                .ok_or_else(|| anyhow::anyhow!("No suitable GPU adapter found"))?;

            let adapter_info = adapter.get_info();
            let limits = adapter.limits();

            let (device, queue) = pollster::block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("spine-gpu"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            ))?;

            // Pre-compile matvec pipeline
            let matvec_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("matvec"),
                source: wgpu::ShaderSource::Wgsl(MATVEC_SHADER.into()),
            });

            let matvec_bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("matvec_bgl"),
                    entries: &[
                        bgl_entry(0, true),  // weights
                        bgl_entry(1, true),  // input
                        bgl_entry(2, true),  // bias
                        bgl_entry(3, false), // output
                        wgpu::BindGroupLayoutEntry {
                            binding: 4,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

            let matvec_pipeline_layout =
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("matvec_pl"),
                    bind_group_layouts: &[&matvec_bind_group_layout],
                    push_constant_ranges: &[],
                });

            let matvec_pipeline =
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("matvec_pipeline"),
                    layout: Some(&matvec_pipeline_layout),
                    module: &matvec_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

            // Pre-compile matmul pipeline (tiled f32 GEMM).
            let matmul_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("matmul"),
                source: wgpu::ShaderSource::Wgsl(MATMUL_SHADER.into()),
            });
            let matmul_bind_group_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("matmul_bgl"),
                    entries: &[
                        bgl_entry(0, true),  // a
                        bgl_entry(1, true),  // b
                        bgl_entry(2, false), // c
                        wgpu::BindGroupLayoutEntry {
                            binding: 3,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });
            let matmul_pipeline_layout =
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("matmul_pl"),
                    bind_group_layouts: &[&matmul_bind_group_layout],
                    push_constant_ranges: &[],
                });
            let matmul_pipeline =
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("matmul_pipeline"),
                    layout: Some(&matmul_pipeline_layout),
                    module: &matmul_module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });

            let info = DeviceInfo {
                name: format!("{} ({:?})", adapter_info.name, adapter_info.backend),
                device_type: DeviceType::Gpu,
                memory_bytes: 0, // wgpu doesn't expose VRAM directly
                max_workgroup_size: limits.max_compute_workgroup_size_x,
                max_buffer_size: limits.max_buffer_size as u64,
            };

            Ok(Self {
                info,
                device,
                queue,
                matvec_pipeline,
                matvec_bind_group_layout,
                matmul_pipeline,
                matmul_bind_group_layout,
            })
        }

        /// Execute GPU tiled GEMM: `c[m,n] = a[m,k] * b[k,n]`.
        fn dispatch_matmul(
            &self,
            a: &[f32],
            b: &[f32],
            m: usize,
            k: usize,
            n: usize,
            c: &mut [f32],
        ) -> Result<()> {
            let a_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mat_a"),
                    contents: bytemuck::cast_slice(a),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let b_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mat_b"),
                    contents: bytemuck::cast_slice(b),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let c_size_bytes = (m * n * 4) as u64;
            let c_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mat_c"),
                size: c_size_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let params: [u32; 3] = [m as u32, k as u32, n as u32];
            let params_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("matmul_params"),
                    contents: bytemuck::cast_slice(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("matmul_bg"),
                layout: &self.matmul_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: a_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: b_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: c_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: params_buf.as_entire_binding(),
                    },
                ],
            });

            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.matmul_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                // 16×16 workgroups cover the (n, m) output grid.
                let groups_x = ((n + 15) / 16) as u32;
                let groups_y = ((m + 15) / 16) as u32;
                pass.dispatch_workgroups(groups_x, groups_y, 1);
            }

            let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("matmul_staging"),
                size: c_size_bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(&c_buf, 0, &staging, 0, c_size_bytes);

            self.queue.submit(std::iter::once(encoder.finish()));

            let slice = staging.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).ok();
            });
            self.device.poll(wgpu::Maintain::Wait);
            rx.recv()??;

            let data = slice.get_mapped_range();
            let result: &[f32] = bytemuck::cast_slice(&data);
            c[..m * n].copy_from_slice(&result[..m * n]);
            drop(data);
            staging.unmap();

            Ok(())
        }

        /// Execute a GPU compute pass: upload → dispatch → readback.
        fn dispatch_matvec(
            &self,
            weights: &[f32],
            input: &[f32],
            bias: &[f32],
            rows: usize,
            cols: usize,
            output: &mut [f32],
        ) -> Result<()> {
            let weights_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("weights"),
                    contents: bytemuck::cast_slice(weights),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let input_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("input"),
                    contents: bytemuck::cast_slice(input),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let bias_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("bias"),
                    contents: bytemuck::cast_slice(bias),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let output_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("output"),
                size: (rows * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let params: [u32; 2] = [rows as u32, cols as u32];
            let params_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("params"),
                    contents: bytemuck::cast_slice(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("matvec_bg"),
                layout: &self.matvec_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: weights_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: input_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: bias_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: output_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: params_buf.as_entire_binding(),
                    },
                ],
            });

            let mut encoder = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.matvec_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(((rows + 255) / 256) as u32, 1, 1);
            }

            let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("staging"),
                size: (rows * 4) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(&output_buf, 0, &staging, 0, (rows * 4) as u64);

            self.queue.submit(std::iter::once(encoder.finish()));

            let slice = staging.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).ok();
            });
            self.device.poll(wgpu::Maintain::Wait);
            rx.recv()??;

            let data = slice.get_mapped_range();
            let result: &[f32] = bytemuck::cast_slice(&data);
            output[..rows].copy_from_slice(&result[..rows]);
            drop(data);
            staging.unmap();

            Ok(())
        }
    }

    fn bgl_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
        wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if read_only {
                    wgpu::BufferBindingType::Storage { read_only: true }
                } else {
                    wgpu::BufferBindingType::Storage { read_only: false }
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }
    }

    impl ComputeBackend for WgpuBackend {
        fn device_info(&self) -> &DeviceInfo {
            &self.info
        }

        fn mat_vec_mul(
            &self,
            weights: &[f32],
            input: &[f32],
            bias: &[f32],
            rows: usize,
            cols: usize,
            output: &mut [f32],
        ) -> Result<()> {
            self.dispatch_matvec(weights, input, bias, rows, cols, output)
        }

        fn mat_mul(
            &self,
            a: &[f32],
            b: &[f32],
            m: usize,
            k: usize,
            n: usize,
            c: &mut [f32],
        ) -> Result<()> {
            self.dispatch_matmul(a, b, m, k, n, c)
        }

        fn softmax(&self, data: &mut [f32]) -> Result<()> {
            let cpu = CpuBackend::new();
            cpu.softmax(data)
        }

        fn relu(&self, data: &mut [f32]) -> Result<()> {
            for x in data.iter_mut() {
                *x = x.max(0.0);
            }
            Ok(())
        }

        fn tanh_activate(&self, data: &mut [f32]) -> Result<()> {
            let cpu = CpuBackend::new();
            cpu.tanh_activate(data)
        }

        fn sigmoid(&self, data: &mut [f32]) -> Result<()> {
            let cpu = CpuBackend::new();
            cpu.sigmoid(data)
        }

        fn dot_product(&self, a: &[f32], b: &[f32]) -> Result<f32> {
            let cpu = CpuBackend::new();
            cpu.dot_product(a, b)
        }

        fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> Result<f32> {
            let cpu = CpuBackend::new();
            cpu.cosine_similarity(a, b)
        }

        fn layer_norm(
            &self,
            data: &mut [f32],
            gamma: &[f32],
            beta: &[f32],
            eps: f32,
        ) -> Result<()> {
            let cpu = CpuBackend::new();
            cpu.layer_norm(data, gamma, beta, eps)
        }

        fn attention(
            &self,
            query: &[f32],
            key: &[f32],
            value: &[f32],
            seq_len: usize,
            head_dim: usize,
            output: &mut [f32],
        ) -> Result<()> {
            let cpu = CpuBackend::new();
            cpu.attention(query, key, value, seq_len, head_dim, output)
        }

        fn vae_encode(
            &self,
            input: &[f32],
            encoder_weights: &[EncoderLayer],
            output_mu: &mut [f32],
            output_logvar: &mut [f32],
        ) -> Result<()> {
            let cpu = CpuBackend::new();
            cpu.vae_encode(input, encoder_weights, output_mu, output_logvar)
        }

        fn vae_reparameterize(
            &self,
            mu: &[f32],
            logvar: &[f32],
            epsilon: &[f32],
            output: &mut [f32],
        ) -> Result<()> {
            let cpu = CpuBackend::new();
            cpu.vae_reparameterize(mu, logvar, epsilon, output)
        }
    }
}

// ============================================================================
// GpuAccelerator — High-level API
// ============================================================================

/// High-level GPU accelerator that automatically selects the best backend.
pub struct GpuAccelerator {
    backend: Box<dyn ComputeBackend>,
}

impl GpuAccelerator {
    /// Create accelerator with the best available backend.
    /// Falls back to CPU if no GPU is available.
    pub fn new() -> Self {
        #[cfg(feature = "wgpu-backend")]
        {
            match wgpu_backend::WgpuBackend::new() {
                Ok(gpu) => {
                    tracing::info!("GPU accelerator initialized: {}", gpu.device_info().name);
                    return Self {
                        backend: Box::new(gpu),
                    };
                }
                Err(e) => {
                    tracing::warn!("GPU not available, falling back to CPU: {}", e);
                }
            }
        }

        tracing::info!("Using CPU compute backend");
        Self {
            backend: Box::new(CpuBackend::new()),
        }
    }

    /// Create with explicit CPU backend.
    pub fn cpu() -> Self {
        Self {
            backend: Box::new(CpuBackend::new()),
        }
    }

    /// Get device information.
    pub fn device_info(&self) -> &DeviceInfo {
        self.backend.device_info()
    }

    /// Is the backend using a GPU?
    pub fn is_gpu(&self) -> bool {
        self.backend.device_info().device_type == DeviceType::Gpu
    }

    /// Access the underlying compute backend.
    pub fn backend(&self) -> &dyn ComputeBackend {
        self.backend.as_ref()
    }

    /// Dense matrix-vector multiply with optional activation.
    pub fn linear_forward(
        &self,
        weights: &[f32],
        input: &[f32],
        bias: &[f32],
        rows: usize,
        cols: usize,
        activation: ActivationKind,
    ) -> Result<Vec<f32>> {
        let mut output = vec![0.0f32; rows];
        self.backend
            .mat_vec_mul(weights, input, bias, rows, cols, &mut output)?;
        match activation {
            ActivationKind::ReLU => self.backend.relu(&mut output)?,
            ActivationKind::Tanh => self.backend.tanh_activate(&mut output)?,
            ActivationKind::Sigmoid => self.backend.sigmoid(&mut output)?,
            ActivationKind::None => {}
        }
        Ok(output)
    }

    /// Full VAE encode: input → latent vector.
    pub fn vae_forward(
        &self,
        input: &[f32],
        encoder_weights: &[EncoderLayer],
        latent_dim: usize,
        epsilon: &[f32],
    ) -> Result<Vec<f32>> {
        let mut mu = vec![0.0f32; latent_dim];
        let mut logvar = vec![0.0f32; latent_dim];
        self.backend
            .vae_encode(input, encoder_weights, &mut mu, &mut logvar)?;

        let mut z = vec![0.0f32; latent_dim];
        self.backend
            .vae_reparameterize(&mu, &logvar, epsilon, &mut z)?;
        Ok(z)
    }

    /// Compute attention output.
    pub fn attention_forward(
        &self,
        query: &[f32],
        key: &[f32],
        value: &[f32],
        seq_len: usize,
        head_dim: usize,
    ) -> Result<Vec<f32>> {
        let mut output = vec![0.0f32; seq_len * head_dim];
        self.backend
            .attention(query, key, value, seq_len, head_dim, &mut output)?;
        Ok(output)
    }
}

impl Default for GpuAccelerator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_mat_vec_mul() {
        let backend = CpuBackend::new();
        // 2×3 matrix × 3-vec + bias
        let weights = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let input = vec![1.0, 1.0, 1.0];
        let bias = vec![0.5, 1.0];
        let mut output = vec![0.0; 2];
        backend
            .mat_vec_mul(&weights, &input, &bias, 2, 3, &mut output)
            .unwrap();
        assert!((output[0] - 6.5).abs() < 1e-5); // 1+2+3+0.5
        assert!((output[1] - 16.0).abs() < 1e-5); // 4+5+6+1.0
    }

    #[test]
    fn test_cpu_mat_mul() {
        let backend = CpuBackend::new();
        // 2×2 × 2×2
        let a = vec![1.0, 2.0, 3.0, 4.0];
        let b = vec![5.0, 6.0, 7.0, 8.0];
        let mut c = vec![0.0; 4];
        backend.mat_mul(&a, &b, 2, 2, 2, &mut c).unwrap();
        assert!((c[0] - 19.0).abs() < 1e-5);
        assert!((c[1] - 22.0).abs() < 1e-5);
        assert!((c[2] - 43.0).abs() < 1e-5);
        assert!((c[3] - 50.0).abs() < 1e-5);
    }

    #[test]
    fn test_cpu_softmax() {
        let backend = CpuBackend::new();
        let mut data = vec![1.0, 2.0, 3.0];
        backend.softmax(&mut data).unwrap();
        let sum: f32 = data.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);
        assert!(data[2] > data[1] && data[1] > data[0]);
    }

    #[test]
    fn test_cpu_activations() {
        let backend = CpuBackend::new();

        let mut relu_data = vec![-1.0, 0.0, 1.0, 2.0];
        backend.relu(&mut relu_data).unwrap();
        assert_eq!(relu_data, vec![0.0, 0.0, 1.0, 2.0]);

        let mut sigmoid_data = vec![0.0];
        backend.sigmoid(&mut sigmoid_data).unwrap();
        assert!((sigmoid_data[0] - 0.5).abs() < 1e-5);

        let mut tanh_data = vec![0.0];
        backend.tanh_activate(&mut tanh_data).unwrap();
        assert!((tanh_data[0]).abs() < 1e-5);
    }

    #[test]
    fn test_cpu_dot_product() {
        let backend = CpuBackend::new();
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        let dot = backend.dot_product(&a, &b).unwrap();
        assert!((dot - 32.0).abs() < 1e-5);
    }

    #[test]
    fn test_cpu_cosine_similarity() {
        let backend = CpuBackend::new();
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 0.0];
        let sim = backend.cosine_similarity(&a, &b).unwrap();
        assert!((sim - 1.0).abs() < 1e-5);

        let c = vec![0.0, 1.0];
        let sim2 = backend.cosine_similarity(&a, &c).unwrap();
        assert!((sim2).abs() < 1e-5); // orthogonal
    }

    #[test]
    fn test_cpu_attention() {
        let backend = CpuBackend::new();
        let seq_len = 2;
        let head_dim = 4;
        let q = vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let k = q.clone();
        let v = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let mut out = vec![0.0; 8];
        backend
            .attention(&q, &k, &v, seq_len, head_dim, &mut out)
            .unwrap();
        // Output should be a weighted combination of V rows
        assert!(out.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn test_cpu_layer_norm() {
        let backend = CpuBackend::new();
        let mut data = vec![1.0, 2.0, 3.0, 4.0];
        let gamma = vec![1.0; 4];
        let beta = vec![0.0; 4];
        backend.layer_norm(&mut data, &gamma, &beta, 1e-5).unwrap();
        let mean: f32 = data.iter().sum::<f32>() / 4.0;
        assert!(mean.abs() < 1e-4); // mean should be ~0
    }

    #[test]
    fn test_cpu_vae_reparameterize() {
        let backend = CpuBackend::new();
        let mu = vec![1.0, 2.0, 3.0];
        let logvar = vec![0.0, 0.0, 0.0]; // std = 1.0
        let epsilon = vec![0.0, 0.0, 0.0]; // no noise
        let mut output = vec![0.0; 3];
        backend
            .vae_reparameterize(&mu, &logvar, &epsilon, &mut output)
            .unwrap();
        // With epsilon=0, output = mu
        assert!((output[0] - 1.0).abs() < 1e-5);
        assert!((output[1] - 2.0).abs() < 1e-5);
        assert!((output[2] - 3.0).abs() < 1e-5);
    }

    #[test]
    fn test_gpu_accelerator_cpu_fallback() {
        let acc = GpuAccelerator::cpu();
        assert_eq!(acc.device_info().device_type, DeviceType::Cpu);
        assert!(!acc.is_gpu());

        let output = acc
            .linear_forward(
                &[1.0, 2.0, 3.0, 4.0],
                &[1.0, 1.0],
                &[0.0, 0.0],
                2,
                2,
                ActivationKind::ReLU,
            )
            .unwrap();
        assert!((output[0] - 3.0).abs() < 1e-5);
        assert!((output[1] - 7.0).abs() < 1e-5);
    }

    #[test]
    fn test_gpu_accelerator_auto_select() {
        let acc = GpuAccelerator::new();
        // Should work regardless of GPU availability
        let dot = acc.backend().dot_product(&[1.0, 2.0], &[3.0, 4.0]).unwrap();
        assert!((dot - 11.0).abs() < 1e-5);
    }

    /// Cross-check the GPU GEMM shader against the CPU reference. Skips
    /// silently when no wgpu adapter is available (CI runners without a
    /// GPU stack).
    #[cfg(feature = "wgpu-backend")]
    #[test]
    fn test_gpu_mat_mul_matches_cpu() {
        let gpu = match wgpu_backend::WgpuBackend::new() {
            Ok(g) => g,
            Err(_) => return, // No adapter — not an environment failure.
        };

        // 4×3 * 3×5 = 4×5 with non-trivial values.
        let a: Vec<f32> = (0..12).map(|i| (i as f32) * 0.5 - 1.0).collect();
        let b: Vec<f32> = (0..15).map(|i| (i as f32) * 0.25 + 0.5).collect();
        let mut c_gpu = vec![0.0f32; 20];
        let mut c_cpu = vec![0.0f32; 20];

        gpu.mat_mul(&a, &b, 4, 3, 5, &mut c_gpu).unwrap();
        CpuBackend::new()
            .mat_mul(&a, &b, 4, 3, 5, &mut c_cpu)
            .unwrap();

        for (g, h) in c_gpu.iter().zip(c_cpu.iter()) {
            assert!(
                (g - h).abs() < 1e-3,
                "GPU/CPU GEMM mismatch: {g} vs {h}"
            );
        }
    }

    #[test]
    fn test_vae_encode_forward() {
        let acc = GpuAccelerator::cpu();
        let layers = vec![
            EncoderLayer {
                weights: vec![1.0, 0.0, 0.0, 1.0],
                bias: vec![0.0, 0.0],
                input_dim: 2,
                output_dim: 2,
                activation: ActivationKind::ReLU,
            },
            // mu head
            EncoderLayer {
                weights: vec![1.0, 0.0, 0.0, 1.0],
                bias: vec![0.0, 0.0],
                input_dim: 2,
                output_dim: 2,
                activation: ActivationKind::None,
            },
            // logvar head
            EncoderLayer {
                weights: vec![0.0, 0.0, 0.0, 0.0],
                bias: vec![0.0, 0.0],
                input_dim: 2,
                output_dim: 2,
                activation: ActivationKind::None,
            },
        ];
        let epsilon = vec![0.0, 0.0];
        let z = acc.vae_forward(&[1.0, 2.0], &layers, 2, &epsilon).unwrap();
        assert_eq!(z.len(), 2);
        assert!(z.iter().all(|x| x.is_finite()));
    }
}
