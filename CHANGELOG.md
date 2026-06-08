# Changelog

All notable changes to SPINE are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.4.0] — 2026-06-08 — Binary wire format (CBOR) — win on encoding

SPINE's message body was UTF-8 JSON behind a binary header. v1.4.0 replaces
it with a compact, self-describing binary wire codec so SPINE is competitive
on raw encoding efficiency — the axis it previously lost to gRPC/protobuf in
the `agentic-eval` web benchmark — without giving up `serde_json::Value`
ergonomics in tool args and schemas.

### Added

- **`spine_protocol::wire` — binary wire codec.** Every message body is now
  framed with an 8-byte `SpineWireHeader` (`"SP"` magic, version, format byte,
  `u32` big-endian payload length) followed by a **CBOR** ([RFC 8949](https://www.rfc-editor.org/rfc/rfc8949))
  payload. CBOR is the only serde-compatible binary format that round-trips
  `serde_json::Value` natively (it is self-describing, so `deserialize_any`
  works — unlike bincode 1.x / postcard, which reject it). The `format` byte
  auto-selects the codec on decode: `0x01` JSON (legacy/debug), `0x02` CBOR,
  `0x03` CBOR+zstd. Payloads ≥ 128 B are additionally zstd-compressed when that
  actually shrinks them (the encoder falls back to plain CBOR otherwise).
- `ciborium = "0.2"` dependency on `spine-protocol`.
- `examples/wire_sizes.rs` — prints JSON vs SPINE-wire sizes, codec, and
  savings per representative frame. Run with
  `cargo run -p spine-protocol --example wire_sizes`.
- `tests/wire_encoding.rs` — per-frame round-trip proofs (value-level equality)
  plus measured size assertions.

### Changed

- `ProtocolHandler` encode/decode paths (`send_message`, `send_message_raw`,
  `encode_message` / `decode_message`, and the speculative-frame payload) now
  serialize the message body through `wire::encode` / `wire::decode` instead of
  `serde_json`. The existing padding, adaptive-compression, Chameleon, and AEAD
  layers are unchanged and compose on top.

### Measured (via `examples/wire_sizes.rs`, header included)

| frame                | JSON   | SPINE  | codec     | saved |
|----------------------|-------:|-------:|-----------|------:|
| EncodedFrame (1 KiB embedding) | 3975 B | 546 B  | cbor+zstd | 86%  |
| CapabilityAd (2 caps + schema) | 806 B  | 322 B  | cbor+zstd | 60%  |
| ToolCall (URL + headers args)  | 323 B  | 255 B  | cbor+zstd | 21%  |
| StreamToken (text)             | 132 B  | 123 B  | cbor      | 7%   |
| Ping (control)                 | 33 B   | 30 B   | cbor      | 9%   |

The win is largest exactly where agent *data* traffic lives — embeddings,
latents, hidden states, and structured capability/schema frames — where CBOR's
native byte/number widths replace JSON's decimal-string-per-byte blowup.
High-entropy text (URLs, UUIDs, prose) can't be compressed below its own
content, so those frames shrink modestly; they still always beat JSON.

### Compatibility

- **Decode is backward-compatible.** `wire::decode` detects a missing `"SP"`
  magic and falls back to parsing the buffer as legacy v1.3.x raw-JSON, so a
  v1.4.0 node reads v1.3.x bodies. The reverse (a v1.3.x node reading v1.4.0
  CBOR) is **not** supported — broader cross-version interop is deliberately a
  later concern.

## [1.3.0] — 2026-06-04 — Close all v1.2.1 residuals

Addresses every remaining item from the v1.2.1 multi-framework audit.
No new feature surface; pure security hardening.

### Fixed (HIGH residuals from v1.2.1)

- **Gateway bearer auth is now secure by default** (CMMC AC.L1-3.1.1,
  MITRE T1190). The binary refuses to start unless the deployer makes
  an explicit choice via env var:
  - `SPINE_GATEWAY_BEARER_TOKEN=<secret>` — auth on (recommended).
  - `SPINE_GATEWAY_ALLOW_UNAUTH=1` — explicit opt-out for local dev
    or behind an authenticating proxy.
  Setting neither, setting both, or setting the token to an
  empty/whitespace string causes the gateway to print the deployer
  message to stderr and exit with code 2. Previous v1.2.1 behavior
  (silently running open) is no longer reachable. New type
  `AuthMode::resolve` carries the contract; legacy `BearerConfig::
  from_env` kept for backwards-compat with `#[allow(dead_code)]`.

### Fixed (MEDIUM residuals from v1.2.1)

- **Private-key memory is now zeroized on Drop** (NIST SP 800-171
  § 3.13.10). Added `zeroize = "1.8"` to `spine-crypto`,
  `spine-protocol`, and `spine-agentic`. The following structs derive
  or manually impl `Zeroize` / `ZeroizeOnDrop`:
  - `spine_crypto::RingElement` — RLWE coefficient vector.
  - `spine_crypto::QuantumKeyPair` — RLWE secret/public ring
    elements; `params` skipped (plaintext metadata).
  - `spine_crypto::MlKemKeyPair` — FIPS 203 decapsulation key bytes;
    `algorithm` skipped (enum tag, not secret).
  - `spine_crypto::QuantumKeyEvolution` — manual `Drop` that scrubs
    the rolling key-hash history; wrapped key structs zero themselves.
  - `spine_protocol::ProtocolMorphology` — manual `Drop` zeroing the
    32-byte session HMAC key.
  - `spine_agentic::Ed25519Keypair` — manual `Drop` scrubbing the
    cached public-key bytes; the inner `ed25519-dalek::SigningKey`
    already derives `ZeroizeOnDrop` upstream.

  Regression guards: 3 compile-time `assert_zeroize_on_drop` checks
  + 3 runtime "fill with secret, zeroize, observe zeros" tests in
  `spine-crypto`.

### Added (DEPLOYER residual from v1.2.1)

- **`fips` cargo feature on `spine-gateway`**. Enabling it
  (`cargo build -p spine-gateway --features fips`) pulls in
  `aws-lc-rs` as the rustls `CryptoProvider` and installs it as the
  process-wide rustls default at startup. The gateway emits an
  `INFO` line noting that FIPS mode is active.

  An end-to-end FIPS 140-3 *validated* module requires the deployer
  to also rebuild `aws-lc-rs` itself with `AWS_LC_FIPS=1` — the
  cargo feature wires SPINE's integration point; the validated
  binary is a deployer toolchain decision documented in
  `SECURITY_AUDIT.md § 2` and `SECURITY.md § FIPS 140-3 build`.

### Changed

- `spine-gateway` deps: added `rustls` (direct), `thiserror`, and the
  `fips` feature pass-through to `rustls/aws_lc_rs`.
- `spine-crypto` / `spine-protocol` / `spine-agentic` deps: added
  `zeroize = { version = "1.8", features = ["derive"] }`.
- `BearerConfig` now has a manual `Debug` impl that prints only the
  token length — never the secret bytes — so debug-log paths can't
  leak the configured bearer.
- `SECURITY.md` gains two new sections: "Gateway authentication
  (secure by default as of v1.3.0)" and "FIPS 140-3 build" and
  "Cryptographic key memory hygiene".

### Verification

- `cargo test --workspace --no-fail-fast` → **1,084 passed / 0
  failed / 5 ignored** (+12 since v1.2.1: 6 auth-resolve tests + 6
  zeroize regression guards).
- `cargo check -p spine-gateway --features fips` → clean compile;
  `aws-lc-rs 1.17.0` cross-builds on Windows.

### Breaking changes

- **`spine-gateway` startup behavior**. Existing deployments that
  relied on the v1.2.x "run open when env var unset" behavior will
  now exit with code 2 on launch. Migration: set
  `SPINE_GATEWAY_ALLOW_UNAUTH=1` to keep the previous open-public
  behavior (and immediately move to setting the bearer token for
  production).

## [1.2.1] — 2026-06-04 — Release hygiene + multi-framework audit

This release is gated by a formal security audit against CVE / RustSec,
NIST FIPS 140-3, MITRE ATT&CK v15, and CMMC 2.0. Full report in
`SECURITY_AUDIT.md`. Every BLOCKER is closed; HIGH and below are
documented and tracked.

### Fixed (BLOCKER)

- **T1565.002 Data Manipulation — Cross-request state leak in
  `/v1/embeddings`** (`spine-gateway` + `spine-protocol`). The default
  `TitansLatentCodec` is now **stateless**: every `encode()` call
  resets the wrapped `NeuralLatentEncoder`'s `message_history` buffer
  and re-seeds the PRNG before encoding, so request A's content can no
  longer influence request B's embedding via the moving-target morph.
  Added `NeuralLatentEncoder::reset_state(seed)` upstream in
  `spine-neural` to make this cheap. Opt-in stateful behavior is
  available via `TitansLatentCodec::stateful(...)`. Regression guards:
  `titans_stateless_codec_does_not_leak_across_calls`,
  `titans_stateful_codec_is_context_aware`.

- **T1499.002 Endpoint DoS — Service Exhaustion Flood**: gateway POST
  bodies were unbounded. A 10 GB POST to `/v1/embeddings` or `/api/parse`
  would deserialize and allocate without limit. Added
  `axum::extract::DefaultBodyLimit::max(8 MiB)` on the router. Per-route
  override remains available for trusted deployments.

- **T1499.004 Endpoint DoS / T1496 Resource Hijacking — CPU bound on
  HLS execution**: the WASM runtime had no fuel/timeout enforcement,
  so a `loop {}` inside an HLS program would hang the executor
  indefinitely. Added `wasmtime::Config::consume_fuel(true)` +
  per-execution fuel budget (default 1 B units ≈ seconds of CPU).
  Regression guards: `test_wasm_fuel_metering_is_active`,
  `test_default_fuel_budget_completes_normal_programs`.

- **T1190 Exploit Public-Facing App — bearer auth contract mismatch**:
  the OpenAPI schema declared a `bearer` security scheme since v1.0.0
  but no middleware enforced it. Added `auth::require_bearer` (in
  `src/spine-gateway/src/auth.rs`) with constant-time secret comparison
  via the `subtle` crate. Activated by setting
  `SPINE_GATEWAY_BEARER_TOKEN`; emits a `WARN` at startup when unset.
  Path allowlist for `/health`, `/ready`, `/swagger-ui`, `/api-docs`.

### Added

- `SECURITY.md` — supported-versions table, private reporting channels,
  threat model, cryptographic choices, known hardening boundaries.
- `SECURITY_AUDIT.md` — full v1.2.1 audit report across CVE / FIPS /
  ATT&CK / CMMC with per-finding severity and applied fixes.
- This `CHANGELOG.md`.
- `.gitignore` patterns for common secret-bearing file shapes
  (`.env*`, `*.pem`, `*.key`, `credentials*.json`,
  `secrets.{yml,yaml}`, `*.sqlite*`, backups).
- `spine-gateway` deps: `subtle = "2.5"` (constant-time comparison).

### Changed

- All four agentic gateway handlers (`chat_completions_stream`,
  `embeddings`, `capabilities`, `codecs`) now carry
  `#[tracing::instrument(skip_all, …)]` so request bodies never reach
  tracing spans. Span fields restricted to `model` and `stream` flags.
- `WasmRuntime::new()` now constructs a runtime with the default
  fuel budget; legacy callers see no API change. Trusted callers that
  need a larger budget can use `WasmRuntime::with_fuel(N)`.
- Gateway startup now logs whether bearer auth is enabled or disabled
  so deployers cannot accidentally run a production gateway with
  authentication off.

### Verification

- `cargo test --workspace --no-fail-fast` → **1,072 passed / 0 failed
  / 5 ignored** (+10 since v1.2.0: 2 WASM fuel guards + 8 auth tests).
- `cargo audit` → 0 vulnerabilities, 10 unmaintained warnings (all
  accepted with rationale in `SECURITY_AUDIT.md § 1`).

### Known residuals tracked for future release

- Bearer auth is opt-in by default. `SECURITY.md` documents the
  deployer obligation; will flip to opt-out in a future major version.
- Private-key memory not zeroized on drop. Targeted for v1.3.0
  (`zeroize` crate on `DecapsulationKey`, HKDF outputs, Ed25519
  secrets).
- FIPS 140-3 validated cryptographic module: algorithms are
  FIPS-approved by spec, but the Rust crates we use are not
  FIPS-validated. Federal deployments must swap `ring` for
  `aws-lc-rs` FIPS build; documented in `SECURITY_AUDIT.md § 2`.

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
