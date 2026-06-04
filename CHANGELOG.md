# Changelog

All notable changes to SPINE are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.2.1] — 2026-06-04 — Release hygiene

### Fixed
- **Cross-request state leak in `/v1/embeddings`** (
  `spine-gateway` + `spine-protocol`). The default
  `TitansLatentCodec` is now **stateless**: every `encode()` call
  resets the wrapped `NeuralLatentEncoder`'s `message_history` buffer
  and re-seeds the PRNG before encoding, so request A's content can
  no longer influence request B's embedding via the moving-target
  morph. Added `NeuralLatentEncoder::reset_state(seed)` upstream in
  `spine-neural` to make this cheap. Opt-in stateful behavior is
  available via `TitansLatentCodec::stateful(...)`.

  Regression guard: `titans_stateless_codec_does_not_leak_across_calls`
  encodes two distinct queries through the same codec twice and
  asserts the per-query outputs are byte-identical.

### Added
- `SECURITY.md` with supported-versions table, private reporting
  channels, threat model, cryptographic choices, and known hardening
  boundaries.
- This `CHANGELOG.md`.
- `.gitignore` entries for common secret-bearing file shapes (`.env*`,
  `*.pem`, `*.key`, `credentials*.json`, `secrets.{yml,yaml}`,
  `*.sqlite*`) so future commits can't accidentally ship them.

### Changed
- All four agentic gateway handlers (`chat_completions_stream`,
  `embeddings`, `capabilities`, `codecs`) now carry
  `#[tracing::instrument(skip_all, …)]` so request bodies never reach
  tracing spans. Span fields are restricted to `model` and `stream`
  flags.

## [1.2.0] — 2026-06-03 — Neural encoder-decoder protocols

### Added
- **Wire-level latent payloads** (`spine-protocol::agentic_codec`):
  - `EncodedFrame { codec, variant, data, metadata: { modality, shape,
    dtype, original_len, source_hash } }` — every latent is its own
    schema; `declared_size_consistent()` is a one-line check.
  - `Modality` enum: `Text` / `Image` / `Audio` / `Video` /
    `Embedding` / `HiddenState` / `Multimodal` / `Other(String)`.
  - `DType` enum: `F32` / `F16` / `BF16` / `I8` / `U8` / `I16` /
    `I32` / `Q4` / `Q8` with a `bytes_per_element()` table.
  - `CodecDescriptor` + `CodecAdvertisement` + `CodecNegotiation` for
    discovery and handshake.
  - `DecodeHints` (temperature, top_p, top_k, max_tokens,
    stop_sequences, repetition_penalty, presence_penalty,
    frequency_penalty, seed) — rides inline on `StreamStart`.
  - `EmbeddingRequest` / `EmbeddingResponse` (OpenAI shape at the wire
    level; `EmbeddingInput::Encoded` enables cross-codec transcoding).
- **Runtime**: `trait NeuralCodec { id, describe, encode, decode }` +
  `CodecRegistry` (`Arc<RwLock<HashMap>>`) + concrete `TitansLatentCodec`
  that wraps `spine_neural::NeuralLatentEncoder`.
- New `Message` variants: `Encoded`, `CodecAd`, `CodecNegotiation`,
  `EmbeddingRequest`, `EmbeddingResponse`.
- `StreamData::Encoded(EncodedFrame)` variant + `StreamStart.decode_hints`
  + `StreamStart.stream_codec` so token streams carry latents directly.
- Gateway routes: `POST /v1/embeddings`, `GET /v1/agentic/codecs`.

### Tests
- +21 tests (16 in `agentic_codec`, 5 in `agentic_sse`).
- Workspace: 1,060 passing / 0 failing / 5 ignored.

## [1.1.0] — 2026-06-03 — Agentic-first frame family

### Added
- **Tool calling** (MCP-shaped): `ToolCall { id, name, args }` →
  `ToolResult { id, outcome: Ok | Err }` with typed error codes.
- **Token streaming**: `StreamStart` → `StreamToken { seq, data }` →
  `StreamEnd { reason, usage }`. `StreamData::Text | Bytes | ToolCall`.
  `StreamEndReason` mirrors OpenAI/Anthropic finish-reason taxonomy.
- **Capability handshake**: `CapabilityQuery { selector: Exact | Prefix
  | Semantic { embedding, top_k } | All }` →
  `CapabilityAdvertisement { capabilities }` with JSON Schema +
  optional semantic embedding.
- **W3C trace context**: `TraceContext { trace_id, span_id, flags,
  state }` with `to_traceparent` / `from_traceparent` matching the
  W3C header format. Attached inline on tool calls, results, and
  stream starts.
- Gateway routes: `POST /v1/chat/completions` (OpenAI-compatible SSE),
  `GET /v1/agentic/capabilities`.

### Tests
- +15 tests (11 in `agentic`, 4 in `agentic_sse`).
- Workspace: 1,039 passing / 0 failing / 5 ignored.

## [1.0.1] — 2026-06-03 — Hardening release

### Fixed
- **Clippy clean** across the workspace (13 warnings → 0).
- **`cargo audit` 29 vulns → 0**. Wasmtime 27 → 36 (clears 15 CVEs in
  the wasmtime line); prometheus 0.13 → 0.14 (drops protobuf 2.x →
  3.x); transitive `rustls-webpki` / `quinn-proto` / `bytes` / `time`
  pulled to patched lines.
- **GPU matmul shader**: `WgpuBackend::mat_mul` was a CPU-fallback
  TODO; replaced with a real 16×16 tiled WGSL GEMM kernel. Verified
  on Windows via DX12 / Vulkan with `test_gpu_mat_mul_matches_cpu`.

### Added
- **CT log loader**: `CtPolicy::add_logs_from_json_v3()` parses
  Google's v3 schema and ingests usable / qualified / pending entries
  only. The previous placeholder 27-byte SPKIs are removed —
  `CtPolicy::default()` now ships with no trusted logs preloaded.
- **RDMA scaffold**: `IbVerbsRdma` / `GpuDirectRdma` replaced
  `PhantomData` stubs with the full QP state machine (RESET → INIT →
  RTR → RTS), `QpInfo` OOB tuple, and a `linux_verbs` FFI shim whose
  call sites name the exact rdma-core C functions a Linux operator
  with `rdma-sys` linked would invoke. Cross-platform compile
  preserved; `gpu-direct` now implies `rdma`.

### Tests
- Workspace: 1,024 passing / 0 failing / 5 ignored.

## [1.0.0] — 2026-05-21 — Initial public release

First tagged release at `nervosys/SPINE`. Core components:

- `spine-protocol` — Chameleon protocol (moving-target defense),
  speculative decoding with Titans predictor, AES-256-GCM AEAD, zstd
  compression, binary program execution.
- `spine-transport` — TCP / QUIC / WebSocket / shared-memory / RDMA
  trait (`LocalShmRdma` loopback impl), zero-copy frame I/O via
  vectored `writev`, batched server writes, pipelined client.
- `spine-neural` — Titans Neural Long-Term Memory with MIRAS
  variants (Tau, Kappa, Phi), VAE encoder, multi-head attention,
  surprise-gated writes.
- `spine-crypto` — quantum-resistant lattice primitives (research
  grade), X3DH key exchange, Titans-based speculative decoding.
- `spine-agentic` — agent identity (Ed25519 + DID), capability
  ontology (100+ hierarchical terms), DAG workflow engine,
  marketplace, replay debugger.
- `spine-gateway` — OpenAPI REST (axum + utoipa) with Swagger UI.
- `spine-gpu` — wgpu compute backend (mat-vec multiply, softmax, dot
  product) with SIMD-8 CPU fallback.
- 28 crates total; 1,016 tests passing.

### Honest performance posture (per 2026-05 audit)

The pre-1.0 README carried "514× lower latency / 610× higher
throughput" claims that originated in rigged comparison benches
(`tcp_comparison.rs` did pure in-memory encode vs real-loopback TCP;
`traditional_comparison.rs` substituted hand-rolled fakes for JSON,
AES-GCM, and Redis pub/sub). Those claims were **retracted across the
README, ROADMAP, and copilot-instructions**. The benchmarks were left
in tree with explicit "NOT A FAIR BENCHMARK" headers so the rigging is
inspectable.

Measured numbers from `network_realistic.rs`, `spine_vs_www.rs`,
`spine_vs_http2.rs`, `agentic_ai_workload.rs`, `llm_tok_per_sec.rs`,
and `llm_shm_ipc.rs` are recorded in `BENCHMARK_REPORT.md`. Headline
defensible wins: 35.9× faster than HTTP/2 on N=64 concurrent
multiplexed streams (1.42 M req/s), 20× faster than HTTP/2+JSON on
128-batch embedding transmission, 728 M tok/s peak via socket-tuned
async client, 1.33 G tok/s via shared-memory IPC. All on Windows 11
loopback (127.0.0.1); no real-network numbers.
