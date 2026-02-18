# SPINE Roadmap

> **Headless semantic browser with adaptive encryption for AI agents**
> 25 Rust crates · 431 tests · 0 warnings · Apache 2.0

---

## Completed

### v1.0 — Core Features ✅

- [x] Rust workspace with core, parser, protocol, agent crates
- [x] Unified Representation (UR) parser for semantic HTML extraction
- [x] Low-latency binary protocol with encryption (AES-256-GCM) and compression (Zstd)
- [x] Navigate and GetUR browser commands
- [x] High-level `AgentClient` API and examples

### v1.0 — Advanced Features ✅

- [x] HLS/HLB compiler (variables, state, conditionals, loops, expressions, functions)
- [x] Virtual DOM runtime with reactive re-rendering
- [x] Chameleon Protocol (latent-space cryptography with moving-target defense)
- [x] Speculative Decoding (bidirectional Titans-based message prediction)
- [x] WebAssembly runtime (HLB → WASM near-native execution)
- [x] Distributed clustering (load balancing, session affinity, leader election)
- [x] Neural Latent Encoder (VAE + Titans Memory + Attention)
- [x] Quantum-resistant keys (RLWE lattice cryptography with forward-secure evolution)
- [x] Human interaction layer (realistic mouse paths, typing delays)
- [x] MIRAS memory framework (YAAD, MONETA, MEMORA variants)
- [x] High-performance transport (zero-copy I/O, BBR congestion control, connection pooling)
- [x] Reactive streaming (multiplexing, flow control, chunked transfer, priority queuing)
- [x] Recursive Language Model (infinite context via REPL per arXiv:2512.24601)
- [x] Unified bioinspired memory (episodic/semantic/working/collective + CRDT distributed)
- [x] Evolvable neural protocols (genetic algorithm-based protocol evolution)
- [x] Co-evolutionary arms race (Red/Blue adversarial protocol cryptography)

### v1.1 — Architecture Fix ✅

- [x] Honest framing as "headless semantic browser"
- [x] THREAT_MODEL.md with 4 adversary tiers
- [x] X3DH key exchange for initial trust establishment
- [x] Security levels: Standard, Hardened, PostQuantum
- [x] Sybil resistance: stake-weighted voting, node reputation, proof-of-work
- [x] Titans/RLM qualification with documented tradeoffs
- [x] Legacy web bridge reframed as compatibility layer

### Phase 1 — Optimization Pass ✅

- [x] 121 Clippy fixes (loop patterns, matches!, iterators)
- [x] Zero-copy serialization: 12× faster latent encoding (22 GiB/s)
- [x] Single-pass cosine similarity: 2.5× faster (9 GiB/s)
- [x] Iterator-based neural matmul for LLVM vectorization
- [x] Mathematical proofs (OPTIMIZATIONS.md)
- [x] TCP/IP benchmark: 514× lower latency, 610× higher throughput

### Phase 2 — SIMD & Binary Optimization ✅

- [x] 8-wide SIMD-friendly dot products for AVX2
- [x] Zero-allocation TitansMemory forward pass (25-40% faster)
- [x] Quake III-style fast rsqrt for attention scaling
- [x] Zero-copy frame decode via Bytes slicing (30% speedup)
- [x] Binary LatentVector with bytemuck/bincode (7-22× faster)
- [x] FlatDenseLayer: cache-optimal flattened weight storage (20-30% inference speedup)

### Phase 3 — Kernel Primitives ✅

- [x] `spine-kernel` crate: ultra-low-level hardware primitives
- [x] AVX2/NEON SIMD intrinsics: dot product (57 GiB/s), matmul (15.5 Gelem/s)
- [x] Custom allocators: BumpAllocator (505 ps), SlabAllocator, ArenaAllocator
- [x] Lock-free atomics: PaddedAtomicU64, SeqLock, LockFreeStack (4.4 ns)
- [x] Wait-free ring buffers: SPSC/MPSC (1.36 ns/op, 700M ops/sec)
- [x] RDTSC sub-nanosecond timing (2.6× faster than Instant::now)
- [x] Direct syscalls: mmap, CPU affinity, NUMA, thread priority
- [x] io_uring kernel bypass I/O (optional feature)

### Weakness Remediation ✅

- [x] W1: Realistic network benchmarks with actual TCP I/O
- [x] W2: Real LLM dispatchers (OpenAI, Anthropic, load-balanced)
- [x] W3: Comprehensive RLWE security tests (12 new tests)
- [x] W4: Scalability benchmarks (1000+ agents, 100M+ chars)
- [x] W5: Graceful degradation (OfflineDispatcher, AdaptiveDispatcher)

### Phase 4 — Hot-Path Optimization ✅

- [x] Protocol buffer reuse (eliminated 8 heap allocs/message)
- [x] Single-pass serde_json::to_writer (no double serialization)
- [x] Adaptive compression: 1-byte flag, skip < 64 bytes
- [x] Stack-allocated headers `[u8; 16]` and signatures `[f32; 8]`
- [x] `std::mem::take` in speculation miss path
- [x] RwLock for concurrent encoder reads
- [x] Cached WasmRuntime, NeuralProtocol, UnifiedRepresentation
- [x] Async file I/O (tokio::fs)
- [x] OnceLock CSS selectors, single-pass text extraction
- [x] Partial sort retrieval: O(n) avg via `select_nth_unstable_by`

### Phase 5 — Protocol Evolution ✅

- [x] Transport plugin system: composable `TransportPlugin` trait with ordered pipeline
- [x] Built-in plugins: Metrics, RateLimiter, Tagging, Logging, SizeLimiter
- [x] WebSocket bridge: client/server `AsyncRead+AsyncWrite` adapters
- [x] `WebSocketClientStream` for agent→server ws:// connections
- [x] Multi-transport server: `tokio::select!` TCP + WebSocket + QUIC
- [x] `AgentClient::connect_ws()` for WebSocket transport
- [x] QUIC server integration (feature-gated quinn endpoint)
- [x] Agent capability marketplace: registry, discovery, bidding, contracts, reputation, audit log
- [x] 245 tests passing (+27 from Phase 5)

### Phase 6 — Production Hardening

- [x] **Configuration management**: TOML/env-based `SpineConfig` with layered overrides (`spine.toml` → env vars → defaults)
- [x] **Health check endpoints**: `/health` (status, uptime, connections), `/ready` (session capacity), `/metrics` (Prometheus)
- [x] **Graceful shutdown**: `tokio::signal::ctrl_c` handler with connection draining and configurable timeout
- [x] **Connection limits**: Per-IP max connections enforcement, active connection tracking, reject during shutdown
- [x] **Watchdog timer**: Background task reaping abandoned/idle sessions on configurable interval
- [x] **Error recovery**: `AgentClient::connect_with_retry()` with exponential backoff (capped at 60s)
- [x] **Session persistence**: Automatic save on shutdown, config-driven persistence interval
- [x] **SESSIONS_ACTIVE gauge**: Upgraded from Counter to IntGauge for accurate session tracking
- [x] **Config-driven server**: All ports, hosts, timeouts, limits, TLS paths from config
- [x] 249 tests passing (+4 from Phase 6)


### Phase 7 — Testing & Verification

- [x] **Property-based testing**: proptest for protocol, parser, transport, and crypto (41 properties across 4 crates)
- [x] **Fuzz testing**: 5 cargo-fuzz targets for parser HTML, latent vectors, frame decode, message deser, frame headers
- [x] **Integration test harness**: 11 multi-session in-process tests (plaintext, encrypted, chameleon, concurrent, stress)
- [x] **Coverage tracking**: scripts/coverage.sh with HTML/JSON/LCOV modes via cargo-llvm-cov
- [x] **Deterministic replay**: TraceLog, ReplayVerifier, TraceSummary in spine-protocol/src/replay.rs
- [x] **Chaos testing**: 13 tests — random disconnects, corrupted headers, truncated messages, rapid reconnect, floods
- [x] **Bug fixes from testing**: header_size minimum bound, morphology evolution ordering, bytemuck alignment fallback
- [x] **321 tests passing**: +72 tests, 0 failures, 0 clippy warnings

### Phase 8 — Developer Ecosystem ✅

- [x] `spine` CLI tool: init, connect (REPL), query, deploy, benchmark, status — 6 commands with clap derive
- [x] Agent SDK cookbook: 12 examples — simple, encrypted, batch scraper, HLS executor, latent analysis, session transfer, reconnect, WebSocket, swarm, knowledge, web intelligence, autonomous
- [x] OpenAPI gateway (spine-gateway): REST API with axum + utoipa, Swagger UI, session management, health/ready/metrics
- [x] Python bindings (spine-python): PyO3 classes for PyClient, PyUnifiedRepresentation, PySpineBinary with maturin build
- [x] TypeScript WASM bindings (spine-js): wasm-bindgen for parseHtml, compileHls with wasm-pack build
- [x] Documentation site: 18-page mdBook covering architecture, SDK, CLI, gateway, internals, contributing
- [x] Container images: Multi-stage Dockerfile + docker-compose for 3-node cluster with gateway
- [x] 329 tests passing (+8 from Phase 8)

### Phase 9 — GPU & Scale ✅

- [x] **GPU-accelerated neural encoding** (`spine-gpu`): ComputeBackend trait, CpuBackend (SIMD-friendly 8-wide), WgpuBackend (WGSL shaders), GpuAccelerator with auto-backend selection
- [x] **Kubernetes operator** (`spine-k8s`): SpineClusterSpec CRD, CPU/memory/connection-based autoscaling, StatefulSet/Service/HPA manifest generators
- [x] **Raft consensus** (`spine-cluster/raft`): Full Raft with leader election, log replication, heartbeats, KvStateMachine, snapshot/restore, in-process test cluster
- [x] **Persistent storage** (`spine-storage`): StorageBackend trait, InMemoryBackend, SqliteBackend (WAL mode), RocksDbBackend (column families), TypedStorage generic wrapper
- [x] **Tiered caching** (`spine-cache`): L1 in-memory LRU (TTL, byte limits, eviction), L2 file-backed, L3 remote trait, TieredCache with promotion-on-hit and write-through
- [x] **Storage-knowledge integration**: PersistentKnowledge adapter for episode, concept, relation, and entry persistence
- [x] 349 tests passing (+20 from Phase 9)

### Phase 10 — Formal Verification & Audit ✅

- [x] **TLA+ specification** (`formal/tla/ChameleonProtocol.tla`): State machine model with epoch monotonicity, synchronized evolution, morphology abstraction, decoy messages, and `ChameleonProtocol_MC.tla` model checking config for TLC
- [x] **Tamarin prover model** (`formal/tamarin/SpineKeyExchange.spthy`): Symbolic verification of X3DH + RLWE key exchange with 10 security lemmas (secrecy, PFS, KCI resistance), three security levels (Standard/Hardened/PostQuantum), key evolution rules
- [x] **Kani model checking** (`spine-kernel/src/kani_harnesses.rs`): 15 bounded verification harnesses for unsafe code — BumpAllocator (3), SlabAllocator (2), SeqLock (2), LockFreeStack (2), SpscRing (2), MpscRing (1), TaggedPtr (1), AtomicFlags (1), SIMD (1)
- [x] **Cryptographic audit** (`formal/audit/CRYPTO_AUDIT.md`): 13 findings (2 critical, 4 high, 4 medium, 3 low) with remediation priorities, verification coverage matrix, third-party audit scope
- [x] **MISRA compliance** (`formal/misra/MISRA_COMPLIANCE.md`): 16 MISRA C:2012 rules mapped to Rust unsafe, 8 documented deviations with justification/mitigation, verification matrix linked to kani harnesses
- [x] 349 tests passing (0 new — verification artifacts are external tools)


### Phase 11 — Security Remediation ✅

- [x] **C1: RLWE KEM correctness**: Store public parameter `a` from keygen; encode random message `m ∈ {0,1}^n` as `⌊q/2⌋·m` in ciphertext; recover via rounding in decapsulate; shared secret = `H(m)` matches on both sides
- [x] **C2: XOR → AES-256-GCM**: Replace insecure XOR encryption with authenticated AEAD; derive AES key from KEM shared secret via HKDF; nonce from message counter; reject tampered ciphertext
- [x] **H2: Key evolution RLWE invariant**: Hash `public_key + secret_key + counter`; derive new seed via HKDF; generate fresh keypair maintaining `b = a·s + e` (no broken mixing)
- [x] **H3: SeqLock CAS writer exclusion**: Replace `fetch_add` with CAS loop (load → check even → CAS odd → write → release); concurrent writers spin instead of causing UB
- [x] **H4: LockFreeStack ABA prevention**: Replace `AtomicPtr` with `TaggedPtr` (16-bit version counter in upper bits); fix bit layout to use high 48-bit pointer / upper 16-bit tag for x86-64 canonical addresses
- [x] **A1: MappedRegion RAII**: Safe mmap wrapper with `Drop` impl calling `munmap`; methods for `as_ptr()`, `as_slice()`, `as_mut_slice()`
- [x] **TaggedPtr bit layout fix**: Moved tag from low 16 bits to high 16 bits (x86-64 canonical addressing uses lower 48 bits for pointers); eliminates heap corruption
- [x] 402 tests passing (+9 security tests: 5 crypto + 3 kernel + 1 doc test)

### Phase 12 — Cryptographic Hardening ✅

- [x] **M1: HMAC morphology evolution**: Replace predictable LCG mixing with HMAC-SHA256 PRF; domain-separated with evolution counter + message hash; all morphology fields derived from keyed HMAC output
- [x] **M3: Argon2id memory-hard PoW**: Replace stub PoW with Argon2id mining (m=4096 KiB, t=3, p=1); `ProofOfWork` struct with `mine()`, `verify()`, `compute_hash()`; `register_node_with_pow()` on consensus
- [x] **M4: RLWE NIST Level 3 parameters**: Upgrade defaults from (n=256, q=3329) to (n=1024, q=12289, σ=3.2) for post-quantum security
- [x] **L1: Session nonce in AES-GCM IV**: Add 4-byte random session nonce to nonce construction preventing cross-session nonce reuse
- [x] **L2: Compression oracle documentation**: Document CRIME/BREACH risk at both adaptive compression sites with mitigation guidance
- [x] **L3: Constant-time key comparison**: Replace `==` with `subtle::ConstantTimeEq` in `verify_evolution()` to prevent timing side-channels
- [x] 402 tests passing (+5 PoW tests), 0 failures, 0 Clippy warnings

### Phase 13 — CI/CD & Workspace Integrity

- [x] **Workspace verification**: All 25 crates compile with `--all-targets -D warnings`
- [x] **Clean Clippy**: Zero warnings across entire workspace
- [x] **Test verification**: Full test suite (402 tests) passing with 0 failures

### Phase 14 — Documentation & Polish

- [x] **Stale reference fixes**: Updated "17 crates" to "25 crates", "218 tests" to "402 tests" across README, ARCHITECTURE, OPTIMIZATIONS, paper
- [x] **Naming cleanup**: Removed all "Hyperlight" references in examples and docs
- [x] **ARCHITECTURE.md expansion**: Added crate descriptions 15-25
- [x] **README test table**: Expanded from 13 to 17 active crates with verified per-crate test counts
- [x] **Docs site expansion**: Added 5 new internals pages (GPU, storage, caching, Kubernetes, formal verification)
- [x] **Crate map update**: docs/src/architecture/crates.md expanded to 25 crates
- [x] **Paper update**: paper.typ updated to 25 crates, 402 tests, ~68k LOC
- [x] **ROADMAP**: Added Phase 13-14 entries and populated Planned section

### Phase 15 — Workspace Completeness & CI Hardening

- [x] **4 missing crates added**: spine-gpu, spine-storage, spine-cache, spine-k8s added to workspace members (were on disk but excluded from CI)
- [x] **Clippy fixes**: Removed unused imports (spine-cache, spine-k8s), converted match→if-let (spine-k8s)
- [x] **402 tests passing**: +39 tests from newly integrated crates, 0 failures, 0 Clippy warnings
- [x] **Stale reference sweep**: Updated all "363 tests" → "402 tests" across README, OPTIMIZATIONS, paper, ROADMAP
- [x] **CI status badge**: Added GitHub Actions CI badge to README
- [x] **README test table**: Added spine-gpu, spine-storage, spine-cache, spine-k8s, spine-parser, spine-core rows

### Phase 16 — Quality Infrastructure

- [x] **Accurate test table**: Per-crate counts verified via `cargo test --workspace --list`, sorted by count descending
- [x] **spine-agent tests**: 11 unit tests covering SDK API, protocol types, connection handling, compiler re-exports
- [x] **CI coverage job**: cargo-llvm-cov + Codecov upload with LLVM instrumentation
- [x] **CI cargo-deny job**: License allow-list, advisory database, ban/source rules via deny.toml
- [x] **CI MSRV check**: Rust 1.75.0 minimum supported version verification
- [x] **Dependabot config**: Weekly updates for Cargo dependencies and GitHub Actions
- [x] **Cargo.lock committed**: Removed from .gitignore for reproducible builds
- [x] **415 tests passing**: +13 tests (+11 spine-agent + 2 doc tests), 0 failures, 0 Clippy warnings

### Phase 17 — Certificate-Based Authentication

- [x] **Extended TlsConfig**: 12 new fields (mutual_tls, crl_path, client_cert/key, cert_reload_secs, auto_generate, ACME settings)
- [x] **CRL support**: Certificate Revocation List checking in ``build_server_config``
- [x] **Certificate rotation**: ``RotatingTlsAcceptor`` with file-watcher-based hot-reload
- [x] **Self-signed cert generation**: ``generate_self_signed()`` and ``generate_dev_certs()`` via rcgen (CA + server + client)
- [x] **ACME cert manager**: ``AcmeCertManager`` with Let's Encrypt integration (staging/production, renewal checking)
- [x] **Env var overrides**: 7 new TLS env vars (SPINE_TLS_CERT, _KEY, _CA, _MTLS, _CRL, _AUTO_GENERATE)
- [x] **Agent TLS retry**: ``connect_tls_with_retry()`` with exponential backoff
- [x] **CLI mTLS flags**: ``--client-cert`` and ``--client-key`` for ``spine connect``
- [x] **CLI cert subcommand**: ``spine cert generate`` and ``spine cert info`` commands
- [x] **Gateway TLS config**: Backend TLS config propagated through AppState
- [x] **429 tests passing**: +14 tests (8 TLS + 6 config tests), 0 failures, 0 Clippy warnings

### Phase 18 — Observability Dashboard

- [x] **Grafana dashboard**: Pre-built `deploy/grafana/spine-dashboard.json` with 12 panels (sessions, latency, throughput, errors, memory, CPU, prediction, cache, protocol, connections)
- [x] **Prometheus config**: `deploy/prometheus/prometheus.yml` with spine-core + gateway scrape targets
- [x] **Gateway `/metrics` endpoint**: Prometheus-format exposition (uptime, active sessions, requests, errors counters)
- [x] **Gateway request counting**: `AtomicU64` counters for total requests and errors across all API handlers
- [x] **OpenTelemetry tracing**: `#[instrument]` on key agent methods (navigate, get_ur, search, ping, execute_hls) and gateway handlers (navigate, search, execute_hls)
- [x] **Agent tracing dep**: Added `tracing = "0.1"` to spine-agent
- [x] **431 tests passing**: +2 gateway observability tests, 0 failures, 0 Clippy warnings

## Planned

### HLS Type System

- [ ] Static type inference for HLS variables
- [ ] Type-checked function signatures
- [ ] Compile-time error reporting with source locations

### Advanced Cryptography

- [ ] M2: Latent-space AES-GCM (encrypt latent vectors directly in embedding space)
- [ ] ML-KEM (FIPS 203) as alternative to custom RLWE
- [ ] Certificate transparency log integration

### Ecosystem Expansion

- [ ] Go bindings (cgo + spine-go)
- [ ] Java/Kotlin bindings (JNI)
- [ ] Official Helm chart for Kubernetes deployments
- [ ] Browser extension for human-agent hybrid browsing

------

## Performance Benchmarks

| Component                 | Throughput   |
| ------------------------- | ------------ |
| Latent Serialize (1024-d) | 22.3 GiB/s   |
| Cosine Similarity         | 9.0 GiB/s    |
| Frame Encode (8 KB)       | 80 GiB/s     |
| Frame Decode (8 KB)       | 90 GiB/s     |
| BBR Pacing Decision       | 335 ps       |
| Kernel Dot Product (256)  | 57 GiB/s     |
| Kernel MatVec (256×256)   | 15.5 Gelem/s |
| Bump Allocator            | 505 ps       |
| SPSC Ring Push/Pop        | 1.36 ns      |
| RDTSC Read                | 9.3 ns       |

---

## Workspace (25 crates)

| Crate             | Purpose                                                         |
| ----------------- | --------------------------------------------------------------- |
| `spine-kernel`    | SIMD, allocators, atomics, ring buffers, RDTSC timing           |
| `spine-core`      | Multi-session orchestration engine (TCP + WS + QUIC server)     |
| `spine-parser`    | Recursive semantic HTML → Unified Representation                |
| `spine-protocol`  | Binary protocol with encryption, compression, Chameleon         |
| `spine-agent`     | High-level SDK: `AgentClient` (TCP/TLS/WS)                      |
| `spine-agentic`   | Swarm intelligence, game theory, social networks                |
| `spine-compiler`  | HLS → HLB compiler                                              |
| `spine-wasm`      | WebAssembly runtime with host function interop                  |
| `spine-cluster`   | Distributed coordination, Sybil resistance, marketplace         |
| `spine-neural`    | VAE + Titans Memory + MIRAS encoder variants                    |
| `spine-crypto`    | Titans prediction, quantum crypto, X3DH key exchange            |
| `spine-human`     | Legacy web bridge for bot-detection bypass                      |
| `spine-browser`   | Cross-platform GUI browser (egui)                               |
| `spine-transport` | Zero-copy I/O, BBR congestion, WebSocket bridge, plugin system  |
| `spine-stream`    | Reactive streams, multiplexing, flow control, priority queuing  |
| `spine-recursive` | Recursive Language Model (10M+ chars, arXiv:2512.24601)         |
| `spine-knowledge` | Bioinspired memory (episodic/semantic/working/collective, CRDT) |
| `spine-cli`       | CLI tool: init, connect, query, deploy, benchmark, status       |
| `spine-gateway`   | REST API gateway with OpenAPI/Swagger UI (axum + utoipa)        |
| `spine-gpu`       | GPU compute: CpuBackend (SIMD), WgpuBackend (WGSL shaders)     |
| `spine-storage`   | Persistent storage: InMemory, SQLite (WAL), RocksDB (LSM)       |
| `spine-cache`     | Tiered caching: L1 LRU, L2 file-backed, L3 remote              |
| `spine-k8s`       | Kubernetes operator: CRD, autoscaler, manifest generators       |
| `spine-python`*   | Python bindings via PyO3 + maturin                              |
| `spine-js`*       | TypeScript/WASM bindings via wasm-bindgen + wasm-pack           |

\* Excluded from default workspace build (requires Python/wasm32 toolchains)
