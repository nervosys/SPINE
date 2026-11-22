# SPINE 🧠🦴

[![License](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.89%2B-orange.svg)](https://www.rust-lang.org/)
[![CI](https://github.com/nervosys/SPINE/actions/workflows/ci.yml/badge.svg)](https://github.com/nervosys/SPINE/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/nervosys/SPINE/branch/master/graph/badge.svg)](https://codecov.io/gh/nervosys/SPINE)
[![Version](https://img.shields.io/badge/version-2.0.1-blue.svg)](CHANGELOG.md)

**SPINE** (Synaptic Pathways INterconnecting Entities) is a Rust web stack for software agents. It provides semantic HTML parsing, native messages and tools, signed name discovery, resource transfer, authenticated durable read-only mesh tasks, and HTTP, MCP, and gRPC bridges. General agent orchestration and specialized hardware paths include experimental components.

For the current local implementation, use the [agent-web quickstart](docs/AGENT_WEB_QUICKSTART.md), [native installation and operations](docs/NATIVE_OPERATIONS.md), [completion status](docs/COMPLETION_STATUS.md), and [completion checklist](COMPLETION_PLAN.md). The source version is 2.0.2; the release badge refers to the historical 2.0.1 publication. Benchmark figures and test counts below are historical measurements, not results from the current validation run, and they predate the native wire becoming spine-wire v1 on 2026-11-21 (specification: [`docs/spec/00-wire.md`](docs/spec/00-wire.md)). The transport benchmarks drive `spine-transport`'s own framing, not the daemon's native wire.

> *"A digital nervous system for AI agents — semantic-first parsing, agentic-first framing, swarm-first coordination, all with the option to fall back to plain HTTP whenever a human or legacy client needs to talk to it."*

## Results at a Glance

Two independent yardsticks: a **transport benchmark** (SPINE vs the real `h2` HTTP/2 crate over TCP loopback) and an **agentic-fitness benchmark** ([`agentic-eval`](https://github.com/nervosys/AetherShell), which ranks seven web stacks on five agent-native axes).

**Transport — SPINE vs real HTTP/2** (`spine_vs_http2.rs`, `agentic_ai_workload.rs`, `llm_tok_per_sec.rs`; ranges are representative loopback medians, re-measured 2026-06-08):

| Workload                                    | SPINE advantage vs HTTP/2 |
| ------------------------------------------- | ------------------------- |
| Single stream, latency                      | **1.6–2.4× faster**       |
| Single stream, throughput                   | **1.8–2.3× higher**       |
| N=64 concurrent streams (pipelined)         | **~32× higher** (≈1.3M req/s on one conn) |
| Embedding batches, 1536-dim, vs HTTP/2+JSON | **~6–25× faster** (RAG / fleet broadcast) |
| LLM tokens/sec, ≥16K-token batches          | **9–15× over HTTP/2 binary**; SPINE sustains **300–550M tok/s** |
| LLM tokens/sec, vs OpenAI JSON-SSE          | JSON-SSE caps near **~10M tok/s** and collapses on large batches |

These are *comparative* results on TCP loopback: the direction and order of magnitude reproduce run-to-run, but absolute peaks are bandwidth- and scheduler-bound and vary by machine (e.g. token throughput peaked at 548M tok/s on the 2026-06-08 re-run vs 728M in the original audit). Full per-size tables, methodology, and the retracted legacy claims are in [`BENCHMARK_REPORT.md`](BENCHMARK_REPORT.md) and the [Performance](#performance) section below.

**Agentic fitness — `agentic-eval` web-stacks ranking** (composite = unweighted mean of five axes):

| Stack                  | Fitness | Streaming | Tools | Encoding | Interop | Security |
| ---------------------- | ------- | --------- | ----- | -------- | ------- | -------- |
| **SPINE**              | **0.90** (1st of 7) | 0.98 | 0.95 | 0.95 | 0.67 | 0.95 |
| gRPC                   | 0.83    | —         | —     | —        | —       | —        |
| OpenAI API (baseline)  | 0.69    | —         | —     | —        | —       | —        |

SPINE leads the composite, edging gRPC by +0.07 and the OpenAI API by **+0.21**. It is strongest on the agent-native axes it was designed for (token streaming, capability handshakes, inline W3C trace context) and at protobuf-parity on encoding (CBOR + byte-string tensor payloads — a 1536-dim embedding frame is **68% smaller than JSON** by default, 89% with opt-in compression; measured on the earlier body encoding before spine-wire v1, not re-measured; `wire_sizes.rs` now measures v1 bodies, so re-run it for current sizes). **Interop (0.67) is its weakest axis** — three deployable bridges (MCP stdio server, OpenAI-compatible gateway, reflection-enabled gRPC `AgentService`) map the agentic *surface*, not SPINE's native binary frames, and SPINE has ~zero native install base. Full methodology and every caveat: [`BENCHMARK_REPORT.md`](BENCHMARK_REPORT.md) and the [Performance](#performance) section below.

## Why "SPINE"?

The name SPINE reflects the bioinspired principles at its core:

- **Synaptic Communication**: Neural-inspired message passing with Titans architecture
- **Adaptive Signal Routing**: Like biological spinal cords routing signals between brain and body
- **Distributed Processing**: Swarm intelligence mimicking neural population coding
- **Moving-Target Defense (research)**: the Chameleon key, inspired by biological camouflage, in `research/spine-protocol-research`; not on the native wire
- **Memory Consolidation**: MIRAS variants implementing continual learning like hippocampal replay

## Why a Headless Semantic Browser?

The traditional web stack (HTTP/HTML/CSS/JS) serves humans well, but AI agents need different capabilities. SPINE provides an efficient semantic layer on top of the existing web:

| Traditional Browsing                   | SPINE Semantic Browsing                  |
| -------------------------------------- | ---------------------------------------- |
| Documents for human reading            | Programs for AI execution                |
| Rendering-first (DOM → Layout → Paint) | Semantics-first (UR extraction)          |
| Stateless request/response             | Persistent neural memory                 |
| Static protocols (fingerprintable)     | Moving-target research (Chameleon, off the wire) |
| Single-agent browsing                  | Multi-agent swarm coordination           |
| No neural nets                         | Titans architecture (test-time training) |

## Core Principles

- **Agentic-First Framing**: Tool calls, token streams, capability handshakes, and W3C trace context are first-class [`Message`](src/spine-protocol/src/agentic.rs) variants — not JSON glued on top of HTTP after the fact. See *Agentic-First Primitives* below.
- **Semantic Extraction**: Directly parses web content into structured representations without rendering pipelines.
- **Binary Execution**: Treats websites as executable programs with instruction-based semantics.
- **Adaptive Protocols (research)**: the Chameleon key with Titans neural memory, in `research/spine-protocol-research`. It is experimental and unreviewed, and the native wire does not use it.
- **Latent Streaming (research)**: streaming high-dimensional vectors (embeddings, latent representations) is not on the native wire. `LatentVector` and the codec types live in `research/spine-protocol-research`.
- **Human Compatibility**: Transpiles legacy web content (HTML/CSS/JS) into AI-native formats, and exposes an OpenAI-compatible `/v1/chat/completions` SSE endpoint so any existing LLM client can drive a SPINE stack without learning a new SDK.
- **Distributed Swarm Intelligence**: Skill-based task routing, DAG dependency tracking, and consensus-based knowledge sharing across agent clusters.
- **Long-Term Memory**: Persistent knowledge base with tagging, querying, and cross-cluster synchronization.

The cluster-node and human-transpile features are in the `spine-core` daemon only when it is built with `--features research`; the default daemon is research-free (see [docs/NATIVE_OPERATIONS.md](docs/NATIVE_OPERATIONS.md)).

## The Namespace: `spine://`

A stack is not a web until resources have **names** — something to refer to a
resource by that outlives the machine currently serving it, that a stranger can
link to, and that an agent can find without already knowing where it lives.
`spine-name` is that layer.

```
spine://did:<52-char base32 ed25519 key>/tools/search   ← self-certifying identity
spine://blob:<52-char base32 sha-256>/                  ← immutable, content-addressed
spine://cap:web.search/                                 ← an ability, not an endpoint
spine://host:node.example.org:9440/                     ← bootstrap escape hatch
```

Three departures from the WWW's URI, each because agents are the users:

| Property | WWW | SPINE |
|---|---|---|
| **Trust** | Name says nothing; a CA vouches for it later | The `did:` authority **is** an Ed25519 public key — records verify against the name itself, no CA in the resolution path |
| **Discovery** | Names an endpoint; finding one needs a search engine | `cap:` names an *ability* and resolves to ranked providers, federated across nodes |
| **Caching** | Opaque server-chosen ETag you must trust | `blob:` names are content hashes — immutable by construction, and every record carries a verifiable content-hash validator |

A **`NameRecord`** binds a name to its endpoints, capabilities, content hash,
typed links, and metadata in one signed object — DNS record and HTTP response
head fused, because an agent resolving a name wants all of it before it can
start work. Records converge by `seq` with no coordinator.

Resolution is a **Kademlia iterative lookup** over a 256-bit XOR keyspace,
carried by the existing agent mesh (`spine-agentic::naming`) and driven over a
live transport by `naming_mesh::MeshNameResolver` — envelope dispatch, request
correlation, timeouts, and unreachable-peer fallback — over three transports:

| Transport | Model | Why |
|---|---|---|
| `mesh_tcp` | one pooled byte stream per peer | the default; lowest overhead |
| `mesh_ws` | same, over a WebSocket upgrade | traverses proxies and firewalls that drop bare TCP; the only transport a browser-resident agent can open |
| `mesh_quic` | one connection per peer, **one stream per exchange** | concurrent lookups cannot head-of-line block one another |

TCP and WebSocket share one stream-generic code path and differ only in how a
connection is established. QUIC deliberately does not: modelling a QUIC
connection as a single byte pipe would discard the multiplexing that is the
whole reason to use it. Connections are symmetric in all three: a request is
answered on the stream it arrived on, so a responder never needs the asker's
address and resolution works through NAT. Because an agent's
keyspace position *is* its Ed25519 signing key, the bridge between mesh routing
(`AgentId`) and namespace routing (`NameKey`) falls straight out of the identity
the mesh already authenticates. Links are
**typed** (`requires`, `provides`, `child`, `snapshot`, …), so a crawl frontier
orders and budgets a traversal from the data alone — dependencies before
documentation — instead of inferring intent from prose.

### Getting the first contact

A Kademlia node converges on the right neighbours from any single honest
contact, but it cannot acquire the first one by routing — routing is what having
a contact enables. That first contact comes from configuration:

```toml
[namespace]
enabled = true
seeds = ["spine://host:seed.example.org:9440/", "10.0.0.7:9440"]
advertise = ["203.0.113.4:9440"]   # omit behind NAT: resolve without being dialable
```

The node dials each seed by address and sends a hello carrying its own public
key; the answer's signature verifies against the key *it* carries, which is also
the responder's keyspace position — so a seed cannot claim a place in the
keyspace it is unable to sign for. The ack brings back the seed's own neighbours,
and one self-lookup fills the buckets nearest the newcomer. Seeds are dialed
concurrently, and one that is down costs a failed dial rather than a refusal to
start.

The node's key is written to `spine-node.key` on first run and reused after,
because that key is not merely a credential: it is the node's position in the
keyspace and the authority behind every name it publishes. A node that
regenerated it would relocate in the DHT and orphan its own names on every
restart.

`spine://host:…` is the escape hatch that makes this expressible as a name at
all. It resolves with no network and no publisher — the address is *in* the name
— and is reported with a distinct `Address` provenance precisely because nothing
attested it. Use it to reach a seed; trust what the seed then proves about
itself.

### Where a name lives

Publishing stores the record locally and then places copies at the nodes closest
to its key — found by walking the keyspace, not by asking whoever happens to be
connected. Those are two different sets, and only the first is where a lookup for
that name will go looking.

```toml
[namespace]
maintain_secs = 3600      # re-offer held records to whoever is closest now
lapse_window_secs = 900   # report names this close to expiring
maintain_batch = 64       # most records one pass re-offers
```

The maintenance pass exists because the K closest nodes to a key are not the same
K nodes an hour later. Peers join and leave; without a periodic re-offer, a
record's copies drift away from the position lookups converge on while the record
itself is still perfectly valid.

It re-offers only the records this node is one of the K closest to. A copy held
while nearer nodes exist is one no lookup will route to, and re-offering it
spends a keyspace walk telling the rightful holders what they already have.
`maintain_batch` bounds the rest: each re-offer costs a walk, so an unbounded
pass on a large store is a self-inflicted flood every tick. Passes resume where
the last one stopped, and report how many records they deferred — a bounded pass
should never read as an exhaustive one.

It cannot renew anything, and does not pretend to. A record's expiry is signed
into it, so re-announcing one does not move it — only the holder of its signing
key can extend a name's life. Names close to lapsing are reported instead, which
is the honest thing a node that does not hold your key can do.

`publish` returns how many copies actually landed. A node with no keyspace peers
still broadcasts to warm whatever caches are listening, but reports zero
replicas: a broadcast may well have reached peers, just not peers chosen for
their position, so nothing about the record's durability follows from it.

### Resolving a name with nothing but curl

The namespace is reachable over plain HTTP through `spine-gateway`, for the large
part of the world that does not speak SPINE's protocol. This costs nothing in
trust: records are signed and verify against the name, so a client that checks
one is not trusting the gateway — only using it as a lens.

```bash
curl 'http://localhost:8080/v1/names/resolve?name=spine://did:rkeohxlubhyz…/tools/search'
curl 'http://localhost:8080/v1/names/providers?capability=web.search&limit=5'
curl -X POST http://localhost:8080/v1/names/publish -d '{"record": …}'
```

Namespace outcomes map onto the HTTP status codes that already mean them:
resolved is `200`, unchanged is `304` with the granted freshness in
`Cache-Control`, a name nobody has published is `404`, a malformed name is `400`.
An HTTP client's existing cache and retry behaviour is then correct without it
knowing anything about SPINE.

Every answer carries its provenance, in the body and in `X-Spine-Provenance`,
with an explicit `attested` flag. This matters most for `host:` names: the
address was read out of the name, nothing signed it, and nobody was asked.
Returning that in the same shape as a signed binding would quietly launder the
one binding in the namespace that is nobody's word.

```bash
cargo run -p spine-name --example agent_web   # publish, discover, resolve, crawl
```

```
1. Capability lookup — `web.search`:
   Search tool  2 endpoint(s)  spine://did:rkeohxlubhyzl7ks3mwtz…
   Index tool   1 endpoint(s)  spine://did:qe4xodvipulv6vvdkrtmg…
   (ranked by reachability — no search engine consulted)

4. Crawling from the search tool (depth 2):
   depth 0  via seed      Search tool  caps=["web.search"]
   depth 1  via requires  Fetcher      caps=["web.fetch"]
   depth 1  via peer      Index tool   caps=["web.search", "web.index"]
```

Reachable over the protocol as `ResolveName`, `ResolveNames` (batched — agents
discover names a dozen at a time), `FindProviders`, `PublishName`, `FetchName`,
and `CrawlNames`.

**Resolution traffic on the mesh is encrypted** (the client to daemon native wire is not: it relies on TLS). Records are signed, so they could never be
forged over a plaintext link — but a plaintext link leaks *what an agent is
looking for*, which is usually a direct read on its task. `spine-session::handshake`
gives the mesh an ML-KEM-768 (FIPS 203) + Ed25519 signed-ephemeral handshake with
AES-256-GCM frames: forward-secret per connection, and mutually authenticated
against the same Ed25519 key that positions a node in the DHT and signs its
records — so "the peer I dialed" and "the peer whose records I trust" are one
fact, and a dialer can pin it. This construction uses standard primitives in a
standard arrangement but **has not had external cryptographic review**; the
module docs state the threat model and its known limits.

## Agentic-First Primitives

The protocol layer (`spine-protocol::agentic`) defines four families that an LLM-agent stack needs at the wire level. Every type is `serde`-round-trippable, rides the same frames as the rest of SPINE, and is also reachable over plain HTTPS through the gateway.

| Primitive | Frames | Maps to |
|-----------|--------|---------|
| **Tool calling (MCP-shaped)** | `ToolCall { name, args, trace }` (the `tool_call` command) → `ToolOutcome::Ok { content }` or `ToolOutcome::Err { code, message, details }` | Anthropic MCP; OpenAI function calling |
| **Token streaming** | `StreamStart` → `StreamToken { seq, data }`* → `StreamEnd { reason, usage }` (frame types defined; the daemon sends no command streams yet) | OpenAI SSE `chat.completion.chunk`; Anthropic streaming |
| **Capability handshake** | `CapabilityQuery { selector }` (the `capability_query` command) → `CapabilityAdvertisement { capabilities }` | OpenAI tool list; MCP server `tools/list` |
| **Distributed tracing** | `TraceContext { trace_id, span_id, flags, state }` attached inline | W3C `traceparent`; OpenTelemetry |

\* `StreamToken::data` is `Text` | `Bytes` | `ToolCall` — so function calling and raw byte payloads fall out of the same frame without a second framing layer.

The gateway crate ships an OpenAI-compatible bridge (`src/spine-gateway/src/agentic_sse.rs`):

```
POST /v1/chat/completions              → SSE stream of OpenAI chat.completion.chunk
GET  /v1/agentic/capabilities          → CapabilityAdvertisement as JSON
POST /v1/embeddings                    → OpenAI-shaped embedding response (`research` feature only)
GET  /v1/agentic/codecs                → advertisement of registered encoder/decoder pairs (`research` feature only)
```

The codec routes do real work only in a gateway built with `--features research`, which pre-registers `TitansLatentCodec` (256-dim, f32, from `research/spine-protocol-research`) and defaults `model` to it. The default build keeps both routes registered and answers each with `503 Service Unavailable` and a message saying to rebuild with the `research` feature.

`StreamEndReason` is the exact OpenAI/Anthropic finish-reason taxonomy (`stop`, `length`, `tool_calls`, `content_filter`, `cancelled`, `error`), so an existing SDK switches over a SPINE stream with no translator.

## Neural Encoder-Decoder Protocols (research)

The neural-codec types are not part of the native protocol. `EncodedFrame`, `EncodedMetadata`, `CodecDescriptor`, `CodecAdvertisement`, `CodecNegotiation`, `EmbeddingRequest`, `EmbeddingResponse`, `Modality`, `DType`, `CodecError`, the `NeuralCodec` trait, `CodecRegistry`, `LatentVector` and `TitansLatentCodec` live in [`research/spine-protocol-research`](research/spine-protocol-research/README.md). `Message` carries none of them: the five codec variants (`Encoded`, `CodecAd`, `CodecNegotiation`, `EmbeddingRequest`, `EmbeddingResponse`) and `LatentMessage` were removed, as were `StreamData::Encoded` and `StreamStart::stream_codec`. They are recorded in that README as extension candidates for the wire's extension mechanism. `DecodeHints` stayed in `spine-protocol` and rides on `StreamStart`.

Codec ids are stable URIs (e.g. `spine:codec/titans/v1@dim=256,dtype=f32`). `TitansLatentCodec` wraps `spine-neural::NeuralLatentEncoder` and is a real Titans projection of text into a fixed-width latent, not a stub.

**Benchmarked for agentic use** (`research/spine-protocol-research/benches/neural_codec_bench.rs`, moved from `spine-protocol`; encode figures re-measured 2026-06-08 before the move). Encode cost scales with the projector and is the honest price of getting a latent: ~94 µs at dim 128, ~847 µs at dim 256, ~3.1 ms at dim 512, ~26 ms at dim 1024. The size figures (66–71 % smaller than JSON, e.g. dim 256: 1241 B binary vs 3942 B JSON; dim 1024: 4314 B vs 14803 B) were measured on the `Message::Encoded` frame before it was removed. The bench now serializes the bare frame with the CBOR codec, so re-run it before quoting a size.

## Core Components

SPINE is composed of 32 crates in two cargo workspaces, organized into a cohesive bioinspired architecture (the per-layer list below predates the split and still describes all of them):

### Two workspaces

- **Stack (repository root, `src/`)**, 20 crates, built by default with `cargo build` at the root: `spine-name`, `spine-authority`, `spine-cbor`, `spine-session`, `spine-mesh`, `spine-transport`, `spine-storage`, `spine-protocol`, `spine-parser`, `spine-cache`, `spine-wasm`, `spine-compiler`, `spine-stream`, `spine-core`, `spine-agent`, `spine-cli`, `spine-gateway`, `spine-grpc`, `spine-ffi`, `spine-web`. No research crate is in their default dependency tree; `spine-web` (`research`, `agentic`, ...), `spine-agent` (`research`, examples only) and `spine-core` (`research`) take them only through opt-in features. `spine-protocol` has no `research` feature.
- **Research (`research/`)**, 15 crates, a separate workspace with its own `Cargo.toml` and `Cargo.lock`, built with `cd research && cargo build`: `spine-agentic`, `spine-browser`, `spine-cluster`, `spine-crypto`, `spine-embedded`, `spine-gpu`, `spine-human`, `spine-k8s`, `spine-kernel`, `spine-knowledge`, `spine-mechgen`, `spine-neural`, `spine-nostd`, `spine-protocol-research` (experimental, unreviewed: the Chameleon key, `SpineConnection`, protocol evolution and `TitansLatentCodec`, moved out of `spine-protocol`), `spine-recursive`. Stack crates reach them only through opt-in `research` cargo features (optional path dependencies) and through optional features of `spine-web`; research crates may depend on stack crates.
- `research/Cargo.toml` copies `[workspace.package]`, `[workspace.dependencies]` and the profiles from the root `Cargo.toml`; the two must be kept in sync by hand.

### Kernel Layer

- **`spine-kernel`**: Ultra-low-level hardware primitives—SIMD intrinsics (AVX2/NEON), lock-free atomics, zero-copy ring buffers, custom allocators (arena/slab), sub-nanosecond RDTSC timing, and direct syscall interfaces.

### Foundation Layer

- **`spine-core`**: Multi-session orchestration engine managing concurrent AI agent connections.
- **`spine-parser`**: Recursive semantic parser translating HTML into **Unified Representation (UR)** optimized for LLM context windows.
- **`spine-compiler`**: Compiles **SPINE Source (HLS)** into **SPINE Binary (HLB)** for the "websites-as-programs" paradigm.
- **`spine-wasm`**: High-performance execution runtime for HLB using WebAssembly.

### Transport Layer

- **`spine-protocol`**: The native client to daemon wire, spine-wire v1: 15-byte frames, a HELLO/HELLO_ACK handshake, deterministic-CBOR bodies, optional zstd compression, 19 commands answered with `{ok}` or `{err: {code, message}}`, and the agentic-first message family (`ToolCall`, `StreamToken`, `CapabilityQuery`, `TraceContext`). It provides no encryption: confidentiality comes from TLS. See [`agentic.rs`](src/spine-protocol/src/agentic.rs).
- **`spine-transport`**: High-performance zero-copy transport layer with BBR congestion control and connection pooling.
- **`spine-stream`**: Reactive streaming layer with multiplexing, flow control, chunked transfer, and priority queuing.

### Intelligence Layer

- **`spine-neural`**: **Titans architecture** (Neural Long-Term Memory) with MIRAS variants for adaptive protocol encoding.
- **`spine-crypto`**: **Titans-based message prediction** (a library only; the wire does not use it), X3DH key exchange, and quantum-resistant lattice cryptography with three security levels (Standard, Hardened, PostQuantum).
- **`spine-recursive`**: **Recursive Language Model** for extended context retrieval (10M+ chars) via REPL environment per arXiv:2512.24601. Note: trades reasoning depth for context breadth.
- **`spine-knowledge`**: **Unified bioinspired memory** with episodic (hippocampus), semantic (neocortex), working (prefrontal cortex), and collective (social brain) subsystems. CRDT-based distributed single-source-of-truth.

### Agent Layer

- **`spine-agent`**: High-level SDK for building AI agents that can navigate, parse, and execute on the SPINE stack.
- **`spine-name`**: The namespace — the `spine://` URI scheme, self-certifying `did:`/`blob:`/`cap:` authorities, signed name records, the 256-bit DHT keyspace, the resolver cache, and the typed link graph with its crawl frontier.
- **`spine-authority`**: capability grants, part one of layer 3 (`docs/spec/03-authority.md`): signed grants, delegation chains that can only narrow, and the eleven-step verification, as a pure library (no I/O, no clock). Not yet wired into the node, the mesh, the SDK or the CLI; no external review.
- **`spine-agentic`**: Advanced agentic AI framework with swarm intelligence, cognitive architectures, and adversarial capabilities. Re-exports the `spine-mesh` stack (`naming`, `naming_mesh`, `mesh_tcp`, `mesh_ws`, `mesh_quic`, `signed_frame`, `identity`, `mesh`).
- **`spine-mesh`**: the production mesh stack — Ed25519 identities, signed mesh envelopes, signed name records with their mesh RPC, and the authenticated TCP, WebSocket and QUIC (stream-per-exchange) transports and the per-message signed frame. Depends on `spine-name`, `spine-session`, `spine-transport` and `spine-protocol` without its `research` feature, so no research crate is in its tree; `spine-agentic` re-exports it.
- **`spine-cbor`**: deterministic CBOR (RFC 8949 section 4.2, no tags, depth at most 32): the one canonical encoding for the native wire and for signed structures. Depends only on `ciborium`, `serde` and `thiserror`; `spine-protocol` re-exports it as `spine_protocol::cbor`.
- **`spine-session`**: the `handshake` — the authenticated, forward-secret channel the mesh runs over (ML-KEM-768 + Ed25519). No dependency on any other workspace crate; `spine-crypto` re-exports it as `spine_crypto::handshake`.
- **`spine-crypto`**: X3DH, RLWE lattice keys, and ML-KEM (FIPS 203) research primitives.
- **`spine-cluster`**: Distributed coordination layer with skill-based routing, consensus voting, and swarm plan orchestration.
- **`spine-human`**: Legacy web bridge (compatibility layer) with realistic mouse paths, typing delays, and human-like interaction patterns.

### Infrastructure Layer

- **`spine-gpu`**: GPU compute abstraction with `CpuBackend` (SIMD 8-wide) and `WgpuBackend` (WGSL shaders).
- **`spine-storage`**: Persistent storage backends — InMemory, SQLite (WAL mode), and RocksDB (column families).
- **`spine-cache`**: Tiered caching — L1 in-memory LRU (TTL, byte limits), L2 file-backed, L3 remote.
- **`spine-k8s`**: Kubernetes operator with CRD, autoscaler, and manifest generators.

### Application Layer

- **`spine-browser`**: Cross-platform GUI browser application for human users, built with `egui`.
- **`spine-cli`**: Command-line tool with init, connect (REPL), query, deploy, benchmark, and status commands.
- **`spine-gateway`**: REST API gateway with OpenAPI/Swagger UI (axum + utoipa) plus an OpenAI-compatible `/v1/chat/completions` SSE bridge that translates SPINE `StreamToken` frames into `chat.completion.chunk` events.

### Bindings & Interop

- **`spine-ffi`**: C FFI bindings (cdylib/staticlib) for language interop (Go, Java, Kotlin).
- **`spine-nostd`**: `#![no_std]` core primitives — Q8.8 fixed-point, FNV hashing, frame codec for embedded/WASM.
- **`spine-embedded`**: Minimal agent runtime for embedded/IoT targets (ARM Cortex-M, ESP32, RISC-V).
- **`spine-python`**\*: Python bindings via PyO3 + maturin (excluded from default build).
- **`spine-js`**\*: TypeScript/WASM bindings via wasm-bindgen + wasm-pack (excluded from default build).

## Intelligence Layer

SPINE features a sophisticated intelligence layer optimized for AI-to-AI communication:

### Titans Architecture (Neural Long-Term Memory)

Unlike traditional RNNs, LSTMs, or even standard Transformers, SPINE uses the **Titans architecture** from Google Research throughout the entire stack:

- **Test-Time Training**: Memory updates via online gradient descent during inference
- **Surprise-Gated Writes**: Memory only updates when predictions fail (high surprise)
- **Persistent Memory Tokens**: Compressed representations that survive across contexts
- **Unbounded Context**: No fixed context window—memory persists indefinitely
- **Anomaly Detection**: Built-in surprise metrics for detecting malicious or novel patterns

```rust
// Titans memory update rule: M_t = M_{t-1} - η * ∇L(M_{t-1}, x_t)
// Where L is surprise loss and η is gated by prediction error
let temporal = self.titans_memory.forward(&latent);
let surprise = self.titans_memory.get_surprise(); // Anomaly detection
```

> **Why Titans + MIRAS for Continual Learning?**
> 
> SPINE is designed as a **continual adaptation system** where agents must respond to evolving web content, new communication patterns, and adversarial conditions in real-time—without offline retraining.
> 
> The [Titans + MIRAS framework](https://research.google/blog/titans-miras-helping-ai-have-long-term-memory/) from Google Research is uniquely suited for this because:
> 
> 1. **Test-Time Memorization**: Unlike static models that require retraining, Titans updates its memory *while running*. When an agent encounters a new website structure or protocol variation, it adapts immediately.
> 
> 2. **Surprise-Based Prioritization**: The "surprise metric" (gradient magnitude) acts as a filter—routine, expected patterns are efficiently ignored while novel or anomalous inputs are prioritized for permanent storage. This mirrors how humans remember unexpected events.
> 
> 3. **Momentum + Forgetting**: Titans captures not just momentary surprises but also relevant follow-up context, while adaptive weight decay prevents memory overflow during extremely long sessions.
> 
> 4. **Deep Memory Architecture**: MIRAS shows that memory *depth* matters more than size. SPINE's multi-layer memory modules achieve better perplexity and scaling than fixed-size RNN states.
> 
> 5. **Efficiency**: Combines RNN-like O(1) inference speed with Transformer-like expressive power—critical for real-time agent communication.
> 
> This makes Titans ideal for a headless semantic browser where protocols must continuously evolve to resist fingerprinting, agents must adapt to every interaction, and security requires instant anomaly detection.

### Chameleon Protocol (research)

An **experimental, unreviewed moving-target idea** where a latent basis and key evolve per message based on neural projections. It lives in `research/spine-protocol-research`, not in `spine-protocol`, and the native wire does not use it.

## Virtual DOM & Incremental Updates

The SPINE Core maintains a **Virtual DOM** for each session, enabling efficient incremental updates:

- **HLB Execution**: SPINE Binary is executed in a sandboxed WASM environment, producing a Virtual DOM tree.
- **VDom Diffing**: The core computes the minimal set of patches (Create, Remove, SetAttr, etc.) between execution cycles.
- **Patch Streaming**: Only the changes are sent to the client, significantly reducing bandwidth for dynamic applications.

## Distributed Swarm Intelligence

SPINE enables **autonomous agent swarms** that collaborate on complex tasks:

### Skill-Based Task Routing

Each node in the cluster advertises its capabilities (skills). When a swarm plan is created, the scheduler automatically assigns tasks to the best-matched nodes:

```rust
// Node capabilities
let capabilities = NodeCapabilities {
    skills: vec!["research".to_string(), "synthesis".to_string()],
    ..Default::default()
};

// Scheduler matches tasks to nodes by skill overlap
let score = task.required_skills.iter()
    .filter(|s| node.skills.contains(s))
    .count();
```

### DAG Dependency Tracking

Tasks in a swarm plan form a **Directed Acyclic Graph (DAG)**. The scheduler respects dependencies, only executing tasks when their prerequisites are complete:

```rust
let tasks = vec![
    PlanTask { id: task1, dependencies: vec![], .. },      // Runs first
    PlanTask { id: task2, dependencies: vec![task1], .. }, // Waits for task1
    PlanTask { id: task3, dependencies: vec![task1, task2], .. }, // Waits for both
];
```

### Knowledge Consensus Protocol

Cluster nodes (`research/spine-cluster`) can propose knowledge to each other. A **2/3 majority vote** is required for consensus. This runs between nodes only: there is no client command for it, and the old `ProposeKnowledge` command was removed from the SDK and daemon (it was refused in every build).

### Long-Term Memory

Each agent has a persistent knowledge base with tagging and semantic querying:

```rust
// Store knowledge
client.store_knowledge("api_endpoint", json!("https://api.example.com"), vec!["config".to_string()]).await?;

// Query by tags
let results = client.query_knowledge("endpoint", vec!["config".to_string()], 10).await?;
```

### Unified Bioinspired Memory (spine-knowledge)

SPINE's memory architecture mirrors the human brain's organization:

```text
┌─────────────────────────────────────────────────────────────────────┐
│                    UNIFIED MEMORY ARCHITECTURE                       │
├─────────────────────────────────────────────────────────────────────┤
│  Episodic Memory (Hippocampus)                                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ • Titans-based surprise-gated learning                      │    │
│  │ • Stores experiences when surprise exceeds threshold        │    │
│  │ • Neural embedding via NeuralLatentEncoder                  │    │
│  └─────────────────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────────────────────┤
│  Semantic Memory (Neocortex)                                        │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ • Conceptual knowledge graph with relations                 │    │
│  │ • Hierarchical categorization (is-a, part-of, related)      │    │
│  │ • Large content storage with chunking                       │    │
│  └─────────────────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────────────────────┤
│  Working Memory (Prefrontal Cortex)                                 │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ • Goal-directed active context                              │    │
│  │ • Attention-based priority scoring                          │    │
│  │ • Limited capacity (like human ~7 items)                    │    │
│  └─────────────────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────────────────────┤
│  Collective Memory (Social Brain)                                   │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │ • CRDT-based distributed single-source-of-truth             │    │
│  │ • Vector clocks for causal ordering                         │    │
│  │ • Multi-node confirmation for trust scoring                 │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
```

```rust
// Create unified memory for an agent
let memory = UnifiedMemory::new(agent_id, UnifiedConfig::default());

// Episodic: Store surprise-gated experiences
memory.episodic.store("Discovered new API endpoint", context);

// Semantic: Learn concepts with relations
memory.semantic.store_concept("API", "Application Programming Interface", attrs);
memory.semantic.add_relation("REST", "API", "is-a");

// Working: Manage goal-directed context
memory.working.set_goal("Extract pricing data", 0.9);

// Collective: Share knowledge across agents (CRDT)
memory.collective.store("market_price", KnowledgeValue::Float(1234.56), tags, 0.95);
```

## Unified Representation (UR)

The UR is a recursive, tree-based structure that simplifies web content into actionable elements:

- **`Heading`**: H1-H6 elements with semantic hierarchy.
- **`Text`**: Cleaned, structured text content.
- **`Link`**: Navigable URIs with descriptive labels.
- **`Button`**: Actionable elements with unique IDs.
- **`Input`**: Form fields with semantic labels.
- **`Image`**: Image elements with alt text and source URLs.
- **`List`**: Ordered/unordered lists with nested items.
- **`Container`**: Logical groupings of child elements.

### Example UR Structure

```json
{
  "title": "Example Domain",
  "url": "https://example.com",
  "elements": [
    {
      "Heading": { "level": 1, "text": "Example Domain" }
    },
    {
      "Text": "This domain is for use in illustrative examples in documents."
    },
    {
      "Link": { "text": "More information...", "url": "https://www.iana.org/domains/example" }
    }
  ]
}
```

## SPINE Source Language (HLS)

HLS is a human-readable language designed to define web interfaces as executable programs. It compiles to **SPINE Binary (HLB)**, which agents can execute directly in the Virtual DOM runtime.

### Example HLS

```hls
element App {
  element Header {
    text "Welcome to SPINE"
  }
  element Content {
    button "Click Me" -> emit("button_clicked")
  }
}
```

### Advanced HLS Features

HLS now supports full programming constructs:

```hls
// Variables and State
let title = "Dashboard"
let items = [1, 2, 3]
state counter = 0

// Conditionals
if counter > 0 {
    element ActiveState {
        text "Counter is active"
    }
} else {
    element InactiveState {
        text "Counter is zero"
    }
}

// Loops
for item in items {
    element ListItem {
        text "Item content"
    }
}

// Expressions
let sum = 1 + 2 * 3
let combined = first ++ " " ++ last
let valid = count > 0 && enabled

// Built-in Functions
print("Debug message")
morph()              // Emit a MorphProtocol instruction (recorded by the VDOM runtime only; the wire framing is fixed)
decoy()              // Emit a Decoy instruction (recorded by the VDOM runtime only; no traffic is sent)
let size = len(items)

// Memory Operations
remember("user_preference", "dark_mode")
query_memory("preference")

// Capability Declarations
capability network   // Request network access
capability storage   // Request storage access

// Reasoning
let plan = reason("Find the search button")
```

### HLB Instructions

The HLS compiler generates these instructions:

- **`DefineElement`**: Creates a new element with a unique ID and tag.
- **`SetAttribute`**: Sets properties (e.g., text, class, style) on an element.
- **`AddChild`**: Establishes parent-child relationships.
- **`EmitEvent`**: Triggers events that agents can subscribe to.
- **`StreamLatent`**: Streams high-dimensional vectors for embeddings or other representations.

## Intelligence Layer Features

SPINE includes advanced features for high-security and low-latency agentic communication:

- **Chameleon Protocol (research)**: An experimental latent-space encoding that evolves per message using neural encoders (`research/spine-protocol-research`, not on the native wire).
- **Lattice keys (research)**: RLWE-based key evolution in `research/spine-crypto`. It is not used by the native wire, has had no external review, and no claim of quantum resistance is made for the daemon.
- **Titans Neural Encoding**: Neural Long-Term Memory with VAE and Attention mechanisms to project web content into high-dimensional latent spaces.

## Intelligence Layer

SPINE features a deep intelligence layer that optimizes for both performance and security in AI-to-AI communication.

### Chameleon Protocol (research, off the wire)

The Chameleon key (`research/spine-protocol-research`) uses **Titans Neural Long-Term Memory** to obscure communication patterns. It is experimental and unreviewed; `spine-protocol` no longer contains it.

- **Latent Morphing**: Messages are projected into a high-dimensional latent space using a Variational Autoencoder (VAE).
- **Dynamic Evolution**: The transformation matrices evolve over time based on quantum-resistant seeds, ensuring that the "language" of the protocol is constantly changing.
- **Keyed encoding, not a cipher**: the model weights and Titans memory state
  are derived from the shared secret, so the encoding is unreproducible without
  it. It provides no confidentiality; use TLS or the session layer.

## Security

Four documents, because they answer different questions:

| Document | Question it answers |
| --- | --- |
| [`SECURITY.md`](SECURITY.md) | Which versions get fixes, and how to report a vulnerability privately |
| [`THREAT_MODEL.md`](THREAT_MODEL.md) | Which adversaries this is built against, and which it is not |
| [`SECURITY_AUDIT.md`](SECURITY_AUDIT.md) | What an internal audit found, including what remains open |
| [`HANDSHAKE-REVIEW.md`](HANDSHAKE-REVIEW.md) | The mesh handshake in full, written for an external reviewer |

**What is solid.** Name records are signed and verify against the name itself,
so resolution needs no CA and a client that checks a record is not trusting the
resolver. The mesh handshake is ML-KEM-768 + Ed25519 with per-connection
ephemeral keys, AES-256-GCM frames, and per-direction keys so nonce counters
cannot collide. The same Ed25519 key positions a node in the DHT, signs its
records, and authenticates its connections, so "the peer I dialled" and "the
peer whose records I trust" are one fact.

**What is not.** The handshake has had **no external cryptographic review** —
the package to commission one is written and ready. The Chameleon layer is
obfuscation, not a cipher, and it is not on the native wire (it is in
`research/spine-protocol-research`); use TLS or the session layer for
confidentiality. The lattice and X3DH paths are research-grade. No crate here
is a FIPS 140-3 validated module.

**What an audit already found.** Four defects, all now fixed, all sharing one
shape: vetted primitives, conventional composition, and inadequate entropy fed
in — a counter-seeded ephemeral keypair, a 32-bit session nonce under a
non-rotating key, an unbounded record store, and a 64-bit-keyed encoder. None
would have been visible in a review of the algorithm list, which is the part a
security summary usually shows you. That is the argument for reading
`SECURITY_AUDIT.md` rather than this section.

## Deployment

SPINE is designed for high-performance native deployment in distributed clusters.

### Quick Start (Local Cluster)

`scripts/deploy.sh` builds the daemon with `--features research` and starts three independent `spine-core` processes on ports 8080 to 8082. It does not form a cluster: `spine-core` reads `PORT` only, not the `NODE_ID`, `SEED_NODES` or data-directory variables the script sets (see the note at the top of the script). The default daemon is research-free:

```bash
# Start three independent local daemons on 8080, 8081 and 8082
./scripts/deploy.sh
```

This will start:

- **Node 1**: Port 8080
- **Nodes 2 and 3**: Ports 8081 and 8082

They are not connected to each other. Logs are written to the `./data` directory.

### Manual Deployment

To deploy a single node manually:

```bash
# Build the core server
cargo build --release -p spine-core

# Run the server
./target/release/spine-core
```

The default `spine-core` is built without the `research` feature: the cluster node and its event listener and the human transpile on `Navigate` are compiled out. Add `--features research` to the `cargo build` line for them. See [Build with research features](docs/NATIVE_OPERATIONS.md#build-with-research-features).

### Environment Variables

- `RUST_LOG`: Logging level (`info`, `debug`, `trace`)
- `PORT`: The port to listen on for agent connections (default: `8080`)

Other settings live in `spine.toml` (see [docs/NATIVE_OPERATIONS.md](docs/NATIVE_OPERATIONS.md)). `NODE_ID`, `SEED_NODES`, `SPINE_KNOWLEDGE_DIR` and `SPINE_SESSIONS_DIR`, which earlier versions of this README listed, are not read by any code in the repository.

## Getting Started

### Prerequisites

- Rust (latest stable, 2021 edition)
- Windows, Linux, or macOS

### Building

```bash
cargo build --release              # the stack (repository root)
(cd research && cargo build)       # the research crates, a separate workspace
```

### Verifying

```bash
scripts/verify.sh              # tests + Clippy of the root workspace, and checks the run finished
```
The root workspace total measured on the Linux CI run for `a580b33` is 993 passed, 0 failed, 3 ignored across 45 suites; the older `1418` figure included the research crates.
Research tests run separately: `cd research && cargo test --workspace --exclude spine-browser`.

Preferred over running `cargo test` and `cargo clippy` by hand. Summing test
results cannot distinguish a passing run from a truncated one — a run that dies
early still reports `ok` for everything that got as far as reporting — so the
script also asserts the suite count, the ignored count, and the expected total.
Do not pipe it into `tail`: the pipeline returns that command's status and the
exit code is lost.

### Running the Core

Start the SPINE browser engine:

```bash
cargo run -p spine-core
```

The core will listen on `127.0.0.1:8080` for agent and browser connections (spine-wire v1 over TCP).

### Running the GUI Browser (Human Mode)

In a separate terminal, launch the cross-platform GUI:

```bash
cd research && cargo run -p spine-browser
```

The GUI allows you to:

- Enter URLs and navigate the web via the SPINE stack.
- Toggle **Human Mode** to transpile legacy HTML into AI-native HLS.
- View raw Unified Representations and agentic data streams.

### Running an Example Agent

In a separate terminal:

```bash
cargo run --example simple_agent -p spine-agent --features research
```

The example agent will:

### Running the Swarm Demo

See skill-based task allocation in action (no server required):

```bash
cargo run --example swarm_demo -p spine-agent
```

Output:
```
╔══════════════════════════════════════════════════════════════╗
║     🚀 SPINE SWARM PLANNING DEMONSTRATION 🚀            ║
╚══════════════════════════════════════════════════════════════╝

📡 SWARM CLUSTER INITIALIZED
  🖥️  Node-Alpha | Skills: ["research", "scraping"]
  🖥️  Node-Beta | Skills: ["synthesis", "writing"]
  🖥️  Node-Gamma | Skills: ["crypto", "analysis"]

🎯 GOAL: "Analyze quantum computing impact on encryption..."

[Tick 1] ✅ ASSIGNING: 'Research...' → Node-Alpha (matched 2/2 skills)
[Tick 2] ✅ ASSIGNING: 'Analyze crypto...' → Node-Gamma (matched 2/2 skills)
[Tick 3] ✅ ASSIGNING: 'Synthesize...' → Node-Beta (matched 2/2 skills)

✨ SWARM PLAN EXECUTION COMPLETE!
```

### Running the Knowledgeable Agent

Demonstrates long-term memory and capability enforcement:

```bash
cargo run --example knowledgeable_agent -p spine-agent
```

### Simple Agent Example

The simple_agent example is built with the SDK's `research` feature (`--features research`, as above); the default `spine-agent` build has no research crate in its tree. The simple_agent example will:

1. Connect to the core (plain TCP unless the example is changed to use `connect_tls`)
2. Navigate to `https://example.com`
3. Fetch the Unified Representation
4. Compile and execute an HLS program

### Example Output

```
Connected to SPINE Core
Navigating to https://example.com...
Fetching Unified Representation...
Page Title: Example Domain
Elements found: 5
Compiling HLS program...
Executing binary program...
Binary execution result: {"status": "executed", "instruction_count": 2}
```

## Protocol Features

### Chameleon Protocol (research, not on the native wire)

The Chameleon key encodes messages into a keyed latent space and changes that
encoding as a session proceeds. It is experimental and unreviewed. It is not in
`spine-protocol` or the SDK: it lives in `research/spine-protocol-research`
(`ChameleonKey`, `SpineConnection`, `connect_auto`), and the native wire
(spine-wire v1, `spine_protocol::connection::Connection`) has no chameleon, no pre-shared-key AES layer and no decoy
traffic.

**What it is:** a lossy, keyed projection of a byte string. The encoder's
weights are derived from the shared secret.

**What it is not:** a cipher. High-dimensional projection is not encryption, and
a latent representation is not a ciphertext with a security proof behind it.
Per-message key evolution is not forward secrecy: message hashes are derivable by
anyone holding the transcript. Genuine forward secrecy lives in the mesh
handshake (`spine-session::handshake`), described in
[`HANDSHAKE-REVIEW.md`](HANDSHAKE-REVIEW.md). For confidentiality on the native
wire use TLS (`AgentClient::connect_tls`); the native wire does not run over the session layer. See
[`SECURITY_AUDIT.md`](SECURITY_AUDIT.md): earlier versions of this section
claimed rather more, and a 2.0.0 audit found the encoder had been keyed by 64
bits derived with a non-cryptographic hash.

To try it: `cd research && cargo run -p spine-protocol-research --example chameleon_demo`.

### Compression

A spine-wire v1 frame body of at least 1 KiB can be compressed with **Zstd** (level 3) when the sender enables it; it is off by default and the receiver always accepts it.

### Binary Execution

Agents can send compiled HLB programs to the core for execution:

```rust
let binary = Compiler::compile("element App {}")?;
let result = client.execute_binary(binary).await?;
```

## Architecture

SPINE bypasses traditional browser rendering pipelines (DOM → Layout → Paint) in favor of a multi-layered stack optimized for AI agents, with a compatibility layer for humans.

### The SPINE Stack (32 Crates, in two workspaces: `src/` and `research/`)

1. **Kernel Layer**: `spine-kernel` — SIMD intrinsics, lock-free atomics, zero-copy ring buffers, custom allocators, RDTSC timing.
2. **Foundation Layer**: `spine-core` (orchestration), `spine-parser` (HTML → UR), `spine-compiler` (HLS → HLB), `spine-wasm` (WASM execution).
3. **Transport Layer**: `spine-protocol` (fixed framing, zstd), `spine-transport` (zero-copy I/O, BBR), `spine-stream` (reactive streaming, multiplexing).
4. **Intelligence Layer**: `spine-neural` (Titans architecture), `spine-crypto` (X3DH, quantum-resistant crypto), `spine-recursive` (extended context retrieval), `spine-knowledge` (CRDT-based distributed memory).
5. **Namespace Layer**: `spine-name` — the `spine://` scheme, self-certifying names, signed records, DHT keyspace, and resolution.
6. **Agent Layer**: `spine-agent` (SDK), `spine-agentic` (swarm intelligence + mesh-backed name resolution), `spine-cluster` (distributed coordination with Sybil resistance).
7. **Compatibility Layer**: `spine-human` — legacy web bridge for bot-detection bypass.
8. **Application Layer**: `spine-browser` — cross-platform GUI browser.
9. **Bindings Layer**: `spine-ffi` (C FFI), `spine-go` (Go/cgo), `spine-python`* (PyO3), `spine-js`* (WASM).

### Semantic Extraction Pipeline

```
Web Content → HTML Parser → Recursive UR Generator → Agent/Browser
```

This approach:

- **Reduces Latency**: No layout/paint calculations needed.
- **Optimizes for LLMs**: Structured data fits naturally into context windows.
- **Enables Binary Execution**: Websites become instruction streams, not documents.

### Multi-Session Concurrency

The core uses `DashMap` and `Tokio` to handle hundreds of concurrent agent sessions. Each session maintains:

- Current URL
- Cached HTML
- Session-specific state

### Protocol Stack

```
┌─────────────────────────────────────┐
│   Agent (High-Level SDK)            │
├─────────────────────────────────────┤
│   Swarm Intelligence Layer          │
│   ├─ Skill-Based Task Routing       │
│   ├─ DAG Dependency Tracking        │
│   ├─ Knowledge Consensus (2/3)      │
│   └─ Autonomous Planning            │
├─────────────────────────────────────┤
│   Research (not on the wire)        │
│   ├─ Titans Latent-Space Encoding   │
│   ├─ Chameleon key (experimental)   │
│   └─ spine-protocol-research crate  │
├─────────────────────────────────────┤
│   Compression (Zstd)                │
├─────────────────────────────────────┤
│   Frame: 15-byte v1 header          │
├─────────────────────────────────────┤
│   TCP (Port 8080)                   │
└─────────────────────────────────────┘
```

## Design Philosophy

1. **AI-First**: Every design decision prioritizes AI agent efficiency over human rendering.
2. **Binary Execution**: Websites are programs, not static documents.
3. **Latent Representations (research)**: embeddings and latent vectors are not on the native wire; they are extension candidates, recorded in `research/spine-protocol-research`.
4. **Explicit Encryption**: confidentiality on the native wire comes from TLS. High-dimensional projections are not encryption (see the Chameleon note above).

## Roadmap

- [x] Recursive UR parsing
- [x] Multi-session management
- [x] Transport encryption (TLS and mTLS; the pre-shared-key AES layer was removed)
- [x] Compression (Zstd)
- [x] HLS/HLB basic compiler
- [x] **Chameleon Protocol** (latent-space obfuscation; experimental, moved to `research/spine-protocol-research`)
- [x] **Moving-Target Defense** (per-message Chameleon key evolution; research crate only, not on the native wire. Protocol morphing and padding were removed.)
- [x] ~~Decoy Traffic injection~~ (removed with `ProtocolHandler`, which no longer exists)
- [x] **Advanced HLS Syntax** (variables, state, conditionals, loops, expressions)
- [x] **Virtual DOM Runtime** (binary execution with UR generation)
- [x] **WebAssembly Runtime** (HLB → WASM near-native execution)
- [x] **Distributed Agent Coordination** (cluster, load balancing, session affinity)
- [x] **Titans Neural Encoder** (Neural Long-Term Memory + VAE + Attention)
- [x] **Titans Message Predictor** (autoregressive byte-level prediction with anomaly detection)
- [x] **Quantum-Resistant Key Evolution** (RLWE lattice cryptography)
- [x] **Human Compatibility Layer** (HTML/CSS/JS → HLS transpilation)
- [x] **Cross-Platform GUI Browser** (egui-based human interface)
- [x] **Distributed Swarm Intelligence** (skill-based routing, DAG dependencies)
- [x] **Knowledge Consensus Protocol** (2/3 majority voting across cluster)
- [x] **Long-Term Memory** (persistent knowledge base with tags)
- [x] **Capability Enforcement** (permission-based HLB execution)
- [x] **HLS Memory Operations** (remember, query_memory, reason)
- [x] **Social Network Swarms** (hierarchical, small-world, scale-free, modular topologies)
- [x] **Collaborative Roles** (coordinator, executor, expert, validator, innovator, mediator)
- [x] **Adversarial Game Theory** (Nash equilibrium, minimax, regret matching)
- [x] **Zero-Copy Message Pooling** (power-of-2 size classes for efficient reuse)
- [x] **Compact Binary Protocol** (this item named a `CompactMessage` type that no longer exists in the repository; the native wire is spine-wire v1 with a 15-byte header)
- [x] **Full LTO Optimization** (30% binary size reduction)
- [x] **Transport Plugin System** (composable pipeline: metrics, rate-limiting, logging, tagging, size-limiting)
- [x] **WebSocket Bridge** (client/server `AsyncRead+AsyncWrite` adapters, used by the spine-wire v1 `Connection`)
- [x] **Multi-Transport Server** (`tokio::select!` TCP + WebSocket + QUIC accept loop)
- [x] **Agent `connect_ws()`** (WebSocket transport for `AgentClient`)
- [x] **QUIC Server Integration** (feature-gated `quinn` endpoint)
- [x] **Agent Capability Marketplace** (decentralized registry, discovery, bidding, contracts, reputation, audit log)
- [x] **The `spine://` namespace** (self-certifying `did:` names, immutable
      `blob:` content addresses, routable `cap:` capabilities, `host:` bootstrap)
- [x] **Federated resolution** (Kademlia lookup over TCP / WebSocket / QUIC, with
      a capability index so "who can do X" is routed rather than broadcast)
- [x] **Mesh handshake** (ML-KEM-768 + Ed25519, forward-secret per connection,
      authenticated against the same key that positions a node in the keyspace)
- [x] **Bootstrap from an address** (a seed proves its identity by signing with
      the key that *is* its keyspace position)
- [x] **Replication and maintenance** (records stored at the K closest nodes and
      re-offered as the neighbourhood changes, so a name outlives its publisher)
- [x] **The namespace over plain HTTP** (`/v1/names/*` on the gateway, mapping
      namespace outcomes onto the HTTP status codes that already mean them)
- [x] **Cryptographic audit, 2.0.0** (four findings fixed: counter-seeded
      ephemeral keys, AES-GCM nonce reuse, an unbounded record store, and a
      64-bit-keyed Chameleon encoder — see
      [`SECURITY_AUDIT.md`](SECURITY_AUDIT.md))

## Adversarial Multi-Agent Intelligence

SPINE is optimized for both **collaborative** and **adversarial** multi-agent scenarios.

### Game-Theoretic Reasoning

Built-in support for strategic decision-making in competitive environments:

```rust
// Create a payoff matrix for game-theoretic analysis
let mut matrix = PayoffMatrix::new(players, actions);
matrix.set_payoff(&[0, 0], &[-1.0, -1.0]); // Cooperate-Cooperate
matrix.set_payoff(&[1, 1], &[-2.0, -2.0]); // Defect-Defect

// Find Nash equilibria
let solver = NashEquilibriumSolver::new(GameType::MixedMotive);
let pure_nash = solver.find_pure_nash(&matrix);
let mixed_nash = solver.find_mixed_nash(&matrix);

// Minimax for zero-sum games
let minimax = MinimaxSolver::new(max_depth);
let (best_action, value) = minimax.solve(&matrix);
```

### Regret-Matching Agents

Agents learn optimal strategies through counterfactual regret minimization:

```rust
let mut agent = AdversarialAgent::new("Player1", num_actions);

// Select action using current strategy
let action = agent.select_action();

// Update strategy using regret matching (CFR-style)
agent.update_regret(action, payoff, &counterfactual_payoffs);

// Converges to Nash equilibrium over time
let exploitability = agent.exploitability(&matrix, player_idx);
```

### Social Network Swarms

Model agent relationships as graphical structures for coordination:

```rust
let network = SocialSwarmBuilder::new("Research Team")
    .topology(SocialTopology::SmallWorld { rewire_prob: 0.3 })
    .add_agent("Lead Researcher", vec![CollaborativeRole::Coordinator])
    .add_agent("Data Scientist", vec![CollaborativeRole::Expert { domain: "ML".into() }])
    .add_agent("Engineer", vec![CollaborativeRole::Executor])
    .build();

// Propagate influence through the network (PageRank-style)
network.propagate_influence(iterations, damping);

// Distribute tasks based on roles and influence
let distribution = network.distribute_task("Build ML pipeline", &required_roles);
```

### Network Topologies

| Topology         | Description                          | Use Case                |
| ---------------- | ------------------------------------ | ----------------------- |
| **Star**         | Central hub connects to all agents   | Command-and-control     |
| **Hierarchical** | Tree-structured command chain        | Corporate organizations |
| **FullMesh**     | All agents connected                 | Small, tight teams      |
| **Ring**         | Circular neighbor connections        | Token-passing protocols |
| **SmallWorld**   | Local clusters + long-range links    | Research collaborations |
| **ScaleFree**    | Power-law degree distribution        | Influencer networks     |
| **Modular**      | Dense clusters, sparse inter-cluster | Cross-functional teams  |
| **Dynamic**      | Evolves based on interactions        | Adaptive organizations  |

## Performance

### Measured performance (2026-05 audit)

> **Measured before the native wire became spine-wire v1 (2026-11-21); not re-measured.** Every figure in this section comes from `spine-transport`'s benches, which do not use `spine-protocol` and so never measured the daemon's native wire, before or after v1. Treat them as properties of `spine-transport`.

Numbers below come from `src/spine-transport/benches/{spine_vs_www,spine_vs_http2,agentic_ai_workload,llm_tok_per_sec,llm_shm_ipc}.rs`, all of which compare against **real** protocol implementations (the `h2` crate for HTTP/2, the `aes-gcm` crate for AES-256-GCM, real `serde_json` for JSON) on real TCP loopback or in-process shared memory. Full methodology, every caveat, and reproduction commands are in [`BENCHMARK_REPORT.md`](BENCHMARK_REPORT.md).

> **Re-verification (2026-06-08).** The `spine_vs_http2`, `agentic_ai_workload`, and `llm_tok_per_sec` benches were re-run. All *comparative* findings reproduced directionally and to the same order of magnitude (single-stream HTTP/2 latency win 1.6–2.4×, throughput win 1.8–2.3×; embedding batches ~6–25× over HTTP/2+JSON; large-batch token throughput 9–15× over HTTP/2 binary while JSON-SSE stays near ~10M tok/s). The **absolute peaks vary by machine and load** — the 2026-06-08 token-throughput peak was 548M tok/s vs the 728M recorded below, and embedding multipliers were noisy (8.1× rather than 23× at batch-32). Treat the absolutes in the tables below as representative single-run medians from the original audit, not run-invariant constants.

#### Transport-layer comparisons (single TCP connection, persistent, optimized SPINE)

| Baseline                       | SPINE latency win | SPINE throughput win |
| ------------------------------ | ----------------- | -------------------- |
| Raw TCP echo (no protocol)     | within ±10%       | within ±10%          |
| Real HTTP/1.1 (textual headers)| 1.74–1.87×        | 1.32–1.84×           |
| Real HTTP/2, single stream     | 1.47–1.93×        | 2.29–2.52×           |
| Real HTTP/2, N=4 concurrent    | —                 | **14.1×**            |
| Real HTTP/2, N=16 concurrent   | —                 | **21.5×**            |
| Real HTTP/2, N=64 concurrent   | —                 | **35.9×** (1.42M req/s)|

#### Agentic AI: embedding transmission (1536-dim, OpenAI ada-002 size)

| Workload                                   | HTTP/2+JSON  | HTTP/2+bincode | **SPINE**       |
| ------------------------------------------ | ------------ | -------------- | --------------- |
| Single embedding (async client)            | 41.7 µs      | 45.6 µs        | **32.2 µs**     |
| 8 embeddings batch (RAG retrieval)         | 592 µs       | 273 µs         | **68.8 µs** (8.6×)  |
| 32 embeddings batch                        | 2.35 ms      | 1.56 ms        | **102 µs** (23×)    |
| 128 embeddings batch (fleet broadcast)     | 7.13 ms      | 4.79 ms        | **357 µs** (20×)    |

#### LLM tokens/sec — single TCP connection

| Pattern                       | HTTP/2+OpenAI SSE | HTTP/2+binary | **SPINE async**       |
| ----------------------------- | ----------------- | ------------- | --------------------- |
| Batch 1,024 tokens            | 12.8 M tok/s      | 17.9 M tok/s  | **32.6 M tok/s**      |
| Batch 4,096 tokens            | 3.7 M tok/s ⚠     | 65.3 M tok/s  | **131 M tok/s**       |
| Batch 16,384 tokens           | 8.1 M tok/s       | 46.6 M tok/s  | **381 M tok/s** (8.2×)|
| Batch 65,536 tokens           | 11.9 M tok/s      | 40.5 M tok/s ⚠| **728 M tok/s** (18×) |
| Streaming, 1,024 tokens       | 19.96 M tok/s     | —             | **30.2 M tok/s**      |
| Pipelined K=16 (4096 tok/req) | —                 | 58.9 M tok/s  | **561 M tok/s** (9.5×)|

⚠ HTTP/2 binary regresses past 65 K tokens because the payload exceeds the default per-stream flow-control window. OpenAI SSE format collapses at 4 K tokens because of JSON string-formatting cost.

#### Same-host agent IPC — shared-memory ring (`llm_shm_ipc.rs`)

For agents on the same host, SPINE frames can be carried over a shared-memory SPSC ring — no kernel transit:

| Workload                       | TCP best   | **SHM**         |
| ------------------------------ | ---------- | --------------- |
| Batch 65,536 tokens            | 728 M tok/s| **1.33 Gtok/s** |
| Pipelined K=64 (4096 tok/req)  | 545 M tok/s| **1.05 Gtok/s** |

**1.33 billion tokens/sec on a single shared-memory ring.** For perspective: GPT-4-class LLMs generate ~50–200 tok/s/user. SPINE's transport ceiling sits ~6–7 orders of magnitude above what the model itself produces — transport is never the bottleneck.

#### RDMA / GPU-Direct (trait + loopback impl shipped; hardware backends are typed stubs)

The repo now contains an `RdmaTransport` trait (`src/spine-transport/src/rdma.rs`) with a working `LocalShmRdma` impl for development/CI, and typed stubs for `IbVerbsRdma` (Linux + Mellanox) and `GpuDirectRdma` (CUDA + nv_peer_mem). **No real RDMA hardware was available for this audit, so the numbers below are vendor-published, not measured here**:

| Substrate                       | Source       | Throughput   | At 4 B/token   |
| ------------------------------- | ------------ | ------------ | -------------- |
| TCP loopback (measured)         | this repo    | 2.91 GB/s    | 728 M tok/s    |
| SHM in-process (measured)       | this repo    | 5.3 GB/s     | 1.33 G tok/s   |
| ConnectX-5 RDMA WRITE           | vendor spec  | ~12 GB/s     | ~3.0 G tok/s   |
| ConnectX-6 RDMA WRITE           | vendor spec  | ~24 GB/s     | ~6.0 G tok/s   |
| ConnectX-7 RDMA WRITE           | vendor spec  | ~50 GB/s     | ~12.5 G tok/s  |
| GPU-Direct (CX-6 ↔ A100)        | vendor spec  | ~22 GB/s     | ~5.5 G tok/s   |

### Crypto

The native wire has no AEAD layer of its own (the pre-shared-key AES-GCM layer was removed); confidentiality there is TLS. The AEAD comparison in the transport benchmarks measured the `aes-gcm` crate that `rustls` also uses, so per-record overhead is the same as TLS's with that primitive. The mesh handshake (`spine-session`) uses AES-256-GCM too. The Chameleon, X3DH and RLWE code is research, not on the native wire, and is not measured here.

### Reproduction

```bash
cargo bench --package spine-transport --bench spine_vs_www
cargo bench --package spine-transport --bench spine_vs_http2
cargo bench --package spine-transport --bench agentic_ai_workload
cargo bench --package spine-transport --bench llm_tok_per_sec
cargo bench --package spine-transport --bench llm_shm_ipc
(cd research && cargo bench -p spine-protocol-research --bench neural_codec_bench)  # Titans neural codec, agentic path
cargo test  --package spine-transport rdma  # 4 trait tests
```

### Micro-benchmarks (re-measured 2026-06-08)

Absolute internal-operation timings, re-run on current hardware. These are the only
component/kernel numbers retained in the README because they are the ones reproduced this
session (`cd research && cargo bench -p spine-kernel --bench kernel_bench`, `-p spine-transport --bench
transport_bench`). Absolute figures are hardware-specific medians, not constants.

| Operation                     | Bench                         | Time     | Throughput     |
| ----------------------------- | ----------------------------- | -------- | -------------- |
| SIMD dot product (256)        | `kernel_bench/simd_dot_product` | 30.8 ns  | 62.0 GiB/s     |
| SIMD matmul (256×256)         | `kernel_bench/simd_matmul`    | 8.24 µs  | 15.9 Gelem/s   |
| SPSC ring push+pop            | `kernel_bench/spsc_ring`      | 1.21 ns  | 829 Melem/s    |
| Bump allocator (64 B)         | `kernel_bench/bump_allocator` | 349 ps   | 2.87 Galloc/s  |
| RDTSC timing                  | `kernel_bench/timing`         | 7.14 ns  | 3.3× vs `Instant::now` |
| Atomic test+set               | `kernel_bench/atomic_flags`   | 3.84 ns  | -              |
| Frame encode (8 KB)           | `transport_bench/frame_codec` | 149 ns   | 51 GiB/s       |
| Frame decode (8 KB)           | `transport_bench/frame_codec` | 123 ns   | 62 GiB/s       |
| Ring buffer (16 KB)           | `transport_bench/ring_buffer` | 391 ns   | 39 GiB/s       |
| BBR pacing decision           | `transport_bench/bbr`         | 302 ps   | -              |
| BBR on-ack update             | `transport_bench/bbr`         | 130 ns   | -              |
| Batch encode (64 frames)      | `transport_bench/batch_encode`| 3.31 µs  | 19.3 Melem/s   |

### Retracted / un-validated legacy tables → [`LEGACY.md`](LEGACY.md)

Earlier drafts of this README carried large speedup tables ("SPINE vs Traditional Web
Stack", "Real-World Application Benchmark", "SPINE vs Standard TCP/IP Stack", "Why SPINE
Dominates", and the original "Component Benchmarks" / "Kernel Primitives" figures). They
have been **moved to [`LEGACY.md`](LEGACY.md)** because their numbers could not be
validated on 2026-06-08 re-measurement:

- The comparison-ratio tables use hand-rolled fake baselines (`traditional_comparison.rs`),
  a category-error setup (`tcp_comparison.rs` — real sockets on one side, in-memory on the
  other), or a "Traditional Stack" column with no implementation in this repo. The 2026-05
  audit (`BENCHMARK_REPORT.md`) already established these are not like-for-like.
- The absolute "Component Benchmarks" figures did not reproduce — frame-codec throughput
  re-measured at ~51-62 GiB/s versus the 110-141 GiB/s claimed (~2× overstated), and
  several rows had no trustworthy backing bench.

Nothing in `LEGACY.md` should be cited. The current, reproduced numbers are the transport
and agentic tables above and in `BENCHMARK_REPORT.md`.

### Build Optimizations

```toml
[profile.release]
opt-level = 3        # Maximum optimization
lto = "fat"          # Full link-time optimization
codegen-units = 1    # Single codegen unit
panic = "abort"      # No unwinding overhead
strip = true         # Strip symbols
```

**Results**: 30% binary size reduction (20.6 MB → 14.4 MB for core)

### Removed: message pool, compact message and lightweight swarm snippets

Earlier versions of this README showed snippets using `MessagePool`, `CompactMessage` (a 28-byte header) and `LightweightSwarm`. None of those types exists anywhere in the repository now (checked 2026-11-21), so the snippets were removed. The native wire is spine-wire v1 (15-byte frame header; see [`docs/spec/00-wire.md`](docs/spec/00-wire.md)).

## Testing

SPINE includes comprehensive test coverage across all 32 crates, which now live in two workspaces:

```bash
# Run the stack tests (repository root)
cargo test --workspace

# Run the research crates' tests (separate workspace)
cd research && cargo test --workspace --exclude spine-browser

# Run tests for a specific research crate
cd research && cargo test -p spine-kernel
cd research && cargo test -p spine-neural
cd research && cargo test -p spine-crypto
```

> **Stale numbers.** The totals and the table below were measured before the
> research crates moved to `research/`. They were not re-measured for the split,
> and `cargo test --workspace` at the root no longer includes the research
> crates, so do not read the root total as the sum of this table.

### Test Summary (1,418 tests, 0 failures; stale, predates the research split and spine-wire v1)

Latest workspace run: `cargo test --workspace --no-fail-fast` →
**1,418 passed / 0 failed / 5 ignored** across all 32 crates. The five
ignored entries are `no_run` / `ignore`-marked doctest fixtures, not
hidden failures. The per-crate breakdown below is measured rather than
estimated — it is produced by running each crate on its own, and the
column sums to the workspace total.


| Crate           | Tests | Description                                                   |
| --------------- | ----- | ------------------------------------------------------------- |
| spine-agentic   | 244*  | Lifecycle, sandbox, scheduler, contracts, swarm               |
| spine-mesh      | 182*  | Identity, mesh envelope/node, naming, TCP/WS/QUIC, signed frame |
| spine-protocol  | 148   | Wire codec, framing, chaos, integration, property (count predates the research move) |
| spine-name      | 113   | The `spine://` namespace: URIs, records, keyspace, resolution |
| spine-transport | 85    | Zero-copy I/O, BBR, connection pooling, property              |
| spine-crypto    | 57    | RLWE, ML-KEM, Titans predictor, property                      |
| spine-session   | 26    | Mesh handshake: ML-KEM + Ed25519, framing, replay, bounds     |
| spine-gateway   | 56    | REST gateway, namespace over HTTP, auth, health checks        |
| spine-parser    | 53    | HTML parsing, UR extraction, agent links, property tests      |
| spine-core      | 48    | Session orchestration, config, namespace join, TLS/cert       |
| spine-cluster   | 37    | Load balancing, session management, Sybil resistance          |
| spine-kernel    | 36    | SIMD, allocators, atomics, ring buffers                       |
| spine-stream    | 35    | Reactive streams, multiplexing, flow control                  |
| spine-ffi       | 34    | C FFI null-safety, parse, compile, version                    |
| spine-wasm      | 30    | HLB compilation, execution, stack ops                         |
| spine-nostd     | 30    | Fixed-point math, FNV hashing, frame codec                    |
| spine-storage   | 24    | SQLite WAL, RocksDB, typed storage                            |
| spine-embedded  | 24    | Agent runtime, ring buffers, routing, watchdog                |
| spine-human     | 23    | Human interaction patterns, timing, bot-detection bypass      |
| spine-neural    | 21    | VAE, attention, keyed encoders, memory variants               |
| spine-knowledge | 21    | Episodic, semantic, collective memory, CRDT                   |
| spine-compiler  | 21    | HLS parsing, type checking, compilation                       |
| spine-grpc      | 16    | gRPC service definitions and codec                            |
| spine-cache     | 16    | LRU, tiered caching, TTL eviction                             |
| spine-recursive | 15    | Infinite context, LLM dispatchers                             |
| spine-cli       | 15    | Init scaffolding, config, addr/tag parsing                    |
| spine-k8s       | 13    | CRD generation, autoscaling, manifests                        |
| spine-gpu       | 12    | GPU compute, SIMD backend, WGSL shaders                       |
| spine-agent     | 12    | SDK API, namespace commands, connection handling              |
| spine-mechgen   | 5     | Mechanism generation                                          |
| spine-web       | 0     | Umbrella facade crate (re-exports only)                       |
| spine-browser   | 0     | Cross-platform GUI browser (egui)                             |

\* `spine-agentic` (258) and `spine-mesh` (168) are from the Linux CI run on `6e47224`, which reported those two unit-test suites; before the split `spine-agentic` was listed at 392. Moving `mesh_quic` and `signed_frame` into `spine-mesh` then shifted 14 tests (9 `#[tokio::test]` and 5 `#[test]` attributes), so the table now shows 244 and 182; that adjustment is an attribute count pending the next CI run. The other rows and the 1,418 total date from the last full local run and have not been re-measured, so the column does not currently sum to the total.

### Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark suite (spine-kernel is in the research workspace)
(cd research && cargo bench -p spine-kernel)
cargo bench -p spine-transport
```

## Contributing

SPINE is an experimental research project. Contributions are welcome, especially in:

- HLS language design
- Binary execution optimization
- Protocol efficiency improvements
- Kernel primitive optimizations

## Documentation

- [Architecture Overview](ARCHITECTURE.md) - System design and component interactions
- [Mathematical Proofs](MATHEMATICAL_PROOFS.md) - Formal proofs of time, space, and security optimality
- [HLS Specification](HLS_SPEC.md) - SPINE Source language reference
- [Optimizations](OPTIMIZATIONS.md) - Performance optimization techniques

## License

SPINE is **dual-licensed**:

- **Open source:** [GNU Affero General Public License v3.0 or later](LICENSE)
  (AGPL-3.0-or-later). You may use, modify, and distribute SPINE under the AGPL,
  including its Section 13 requirement to make modified source available to users
  who interact with the software over a network.
- **Commercial:** organizations that cannot meet the AGPL's copyleft / network
  source-disclosure terms (e.g. closed-source SaaS, on-premise, or embedded
  products) can obtain a separate commercial license. Contact
  **opensource@nervosys.ai** — see the *Commercial Dual-License Option* section
  of [LICENSE](LICENSE).

Copyright © 2026 Nervosys LLC
