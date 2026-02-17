# GPU Compute

The `spine-gpu` crate provides cross-platform GPU-accelerated tensor operations for SPINE's neural encoding pipeline.

## Architecture

```text
┌─────────────────────────────────────────┐
│            GpuAccelerator               │
│  ┌─────────┐  ┌──────────┐  ┌────────┐ │
│  │ MatMul  │  │ Softmax  │  │  VAE   │ │
│  │ Kernel  │  │ Kernel   │  │Forward │ │
│  └────┬────┘  └────┬─────┘  └───┬────┘ │
│       └─────────┬──┘────────────┘       │
│            ┌────┴─────┐                 │
│            │ Backend  │                 │
│            └────┬─────┘                 │
│    ┌────────────┼────────────┐          │
│    ▼            ▼            ▼          │
│  wgpu       CPU-SIMD      (CUDA)       │
│ Vulkan/     Fallback     Optional       │
│ Metal/DX12                              │
└─────────────────────────────────────────┘
```

## Backends

### CpuBackend (default)

SIMD-optimized fallback using 8-wide unrolled dot products for AVX2/NEON. Includes fast inverse square root (Quake III-style) for attention scaling.

### WgpuBackend (feature: `wgpu-backend`)

Cross-platform GPU compute via WGSL shaders. Supports Vulkan, Metal, DX12, and WebGPU. Uses `wgpu` for buffer management and compute pipeline dispatch.

## ComputeBackend Trait

All backends implement:
- `mat_vec_mul()` — Dense matrix-vector multiply with bias
- `softmax()` — Numerically stable softmax
- `layer_norm()` — Layer normalization with affine transform
- `vae_encode()` / `vae_decode()` — Variational autoencoder forward passes

## Usage

```rust
use spine_gpu::GpuAccelerator;

let accel = GpuAccelerator::new().await?;
println!("Using: {}", accel.device_info().name);
accel.mat_vec_mul(&weights, &input, &bias, rows, cols, &mut output)?;
```
