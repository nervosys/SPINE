# SPINE 🧠🦴

[![License](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![CI](https://github.com/nervosys/SPINE/actions/workflows/ci.yml/badge.svg)](https://github.com/nervosys/SPINE/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/nervosys/SPINE/branch/master/graph/badge.svg)](https://codecov.io/gh/nervosys/SPINE)
[![Tests](https://img.shields.io/badge/tests-1060%20passing-brightgreen.svg)](#testing)

**SPINE** (Synaptic Pathways INterconnecting Entities) is an **agentic-first web stack for the 21st century** — a complete communication, execution, and coordination layer designed from frame zero around the things modern LLM agents actually need (tokens, tools, capabilities, traces, swarms) rather than the things browsers were built for (documents, layouts, sessions). HTTP/REST and OpenAI-style SSE are first-class wire formats, but they're surfaces, not the substrate.

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

SPINE leads the composite, edging gRPC by +0.07 and the OpenAI API by **+0.21**. It is strongest on the agent-native axes it was designed for (token streaming, capability handshakes, inline W3C trace context) and at protobuf-parity on encoding (CBOR + byte-string tensor payloads — a 1536-dim embedding frame is **68% smaller than JSON** by default, 89% with opt-in compression; `wire_sizes.rs`). **Interop (0.67) is its weakest axis** — three deployable bridges (MCP stdio server, OpenAI-compatible gateway, reflection-enabled gRPC `AgentService`) map the agentic *surface*, not SPINE's native binary frames, and SPINE has ~zero native install base. Full methodology and every caveat: [`BENCHMARK_REPORT.md`](BENCHMARK_REPORT.md) and the [Performance](#performance) section below.

## Why "SPINE"?

The name SPINE reflects the bioinspired principles at its core:

- **Synaptic Communication**: Neural-inspired message passing with Titans architecture
- **Adaptive Signal Routing**: Like biological spinal cords routing signals between brain and body
- **Distributed Processing**: Swarm intelligence mimicking neural population coding
- **Moving-Target Defense**: Chameleon Protocol inspired by biological camouflage
- **Memory Consolidation**: MIRAS variants implementing continual learning like hippocampal replay

## Why a Headless Semantic Browser?

The traditional web stack (HTTP/HTML/CSS/JS) serves humans well, but AI agents need different capabilities. SPINE provides an efficient semantic layer on top of the existing web:

| Traditional Browsing                   | SPINE Semantic Browsing                  |
| -------------------------------------- | ---------------------------------------- |
| Documents for human reading            | Programs for AI execution                |
| Rendering-first (DOM → Layout → Paint) | Semantics-first (UR extraction)          |
| Stateless request/response             | Persistent neural memory                 |
| Static protocols (fingerprintable)     | Moving-target defense (Chameleon)        |
| Single-agent browsing                  | Multi-agent swarm coordination           |
| No neural nets                         | Titans architecture (test-time training) |

## Core Principles

- **Agentic-First Framing**: Tool calls, token streams, capability handshakes, and W3C trace context are first-class [`Message`](src/spine-protocol/src/agentic.rs) variants — not JSON glued on top of HTTP after the fact. See *Agentic-First Primitives* below.
- **Semantic Extraction**: Directly parses web content into structured representations without rendering pipelines.
- **Binary Execution**: Treats websites as executable programs with instruction-based semantics.
- **Adaptive Protocols**: Chameleon Protocol with Titans neural memory for moving-target defense.
- **Latent Streaming**: Native support for streaming high-dimensional vectors (embeddings, latent representations) to agents.
- **Human Compatibility**: Transpiles legacy web content (HTML/CSS/JS) into AI-native formats, and exposes an OpenAI-compatible `/v1/chat/completions` SSE endpoint so any existing LLM client can drive a SPINE stack without learning a new SDK.
- **Distributed Swarm Intelligence**: Skill-based task routing, DAG dependency tracking, and consensus-based knowledge sharing across agent clusters.
- **Long-Term Memory**: Persistent knowledge base with tagging, querying, and cross-cluster synchronization.

## Agentic-First Primitives

The protocol layer (`spine-protocol::agentic`) defines four families that an LLM-agent stack needs at the wire level. Every type is `serde`-round-trippable, lands on the same encrypted Chameleon path as the rest of SPINE, and is also reachable over plain HTTPS through the gateway.

| Primitive | Frames | Maps to |
|-----------|--------|---------|
| **Tool calling (MCP-shaped)** | `ToolCall { id, name, args }` → `ToolResult { id, outcome }` | Anthropic MCP; OpenAI function calling |
| **Token streaming** | `StreamStart` → `StreamToken { seq, data }`* → `StreamEnd { reason, usage }` | OpenAI SSE `chat.completion.chunk`; Anthropic streaming |
| **Capability handshake** | `CapabilityQuery { selector }` → `CapabilityAdvertisement { capabilities }` | OpenAI tool list; MCP server `tools/list` |
| **Distributed tracing** | `TraceContext { trace_id, span_id, flags, state }` attached inline | W3C `traceparent`; OpenTelemetry |

\* `StreamToken::data` is `Text` | `Bytes` | `ToolCall` | `Encoded` — so function calling and **raw latent streaming** both fall out of the same frame without a second framing layer.

The gateway crate ships an OpenAI-compatible bridge (`src/spine-gateway/src/agentic_sse.rs`):

```
POST /v1/chat/completions              → SSE stream of OpenAI chat.completion.chunk
GET  /v1/agentic/capabilities          → CapabilityAdvertisement as JSON
POST /v1/embeddings                    → OpenAI-shaped embedding response, computed via the registered NeuralCodec
GET  /v1/agentic/codecs                → CodecAdvertisement of registered encoder/decoder pairs
```

`StreamEndReason` is the exact OpenAI/Anthropic finish-reason taxonomy (`stop`, `length`, `tool_calls`, `content_filter`, `cancelled`, `error`), so an existing SDK switches over a SPINE stream with no translator.

## Neural Encoder-Decoder Protocols

Text on the wire throws away both bandwidth and signal. SPINE makes the **latent form** a first-class payload — every encoded chunk carries its codec id, modality, shape, and dtype inline so the receiver can validate, decode, or forward without an out-of-band schema. The contract is defined in [`spine-protocol::agentic_codec`](src/spine-protocol/src/agentic_codec.rs).

| Family | Types | Purpose |
|--------|-------|---------|
| **Self-describing payload** | `EncodedFrame { codec, variant, data, metadata }` + `EncodedMetadata { modality, shape, dtype, original_len, source_hash }` | Every latent is its own schema. `declared_size_consistent()` is a one-line sanity check. |
| **Codec discovery** | `CodecDescriptor { id, direction, modality, embedding_dim, vocab_size, dtype, semantic_embedding }` + `CodecAdvertisement` | Peers advertise what encoders/decoders they speak — match by id, by modality, or by semantic embedding. |
| **Codec negotiation** | `CodecNegotiation { offered, accepted, reason }` | Either side offers a ranked list; the other picks one or falls back to plain text. |
| **Decoder hints** | `DecodeHints { temperature, top_p, top_k, max_tokens, stop_sequences, repetition_penalty, presence_penalty, frequency_penalty, seed }` | Sampling parameters travel inline on `StreamStart`; field names match OpenAI/Anthropic so SDK knobs map 1:1. |
| **Embedding endpoint** | `EmbeddingRequest { input: Text \| Texts \| Encoded, codec }` → `EmbeddingResponse { codec, embeddings: Vec<EncodedFrame> }` | OpenAI-compatible embed API at the wire level; `Encoded` input enables cross-codec transcoding. |

`Modality` covers `Text`, `Image`, `Audio`, `Video`, `Embedding`, `HiddenState`, `Multimodal`, `Other(String)`; `DType` covers `F32`, `F16`, `BF16`, `I8`, `U8`, `I16`, `I32`, `Q4`, `Q8`. Codec ids are stable URIs (e.g. `spine:codec/titans/v1@dim=256,dtype=f32`) so a peer's advertisement is decodable without prior knowledge.

A symmetric runtime contract — `trait NeuralCodec { encode, decode, describe }` + `CodecRegistry` — sits alongside the types. The crate ships a working `TitansLatentCodec` that wraps `spine-neural::NeuralLatentEncoder` so every layer of the protocol (advertise → negotiate → encode → decode) round-trips against a real Titans projector, not a stub. Streaming latents over the same wire that streams text:

```rust
StreamData::Encoded(EncodedFrame {
    codec: "spine:codec/titans/v1@dim=256,dtype=f32".into(),
    metadata: EncodedMetadata { modality: Modality::HiddenState, shape: vec![256], dtype: DType::F32, .. },
    data: latent_bytes,
    ..
})
```

## Core Components

SPINE is composed of 28 specialized crates organized into a cohesive bioinspired architecture:

### Kernel Layer

- **`spine-kernel`**: Ultra-low-level hardware primitives—SIMD intrinsics (AVX2/NEON), lock-free atomics, zero-copy ring buffers, custom allocators (arena/slab), sub-nanosecond RDTSC timing, and direct syscall interfaces.

### Foundation Layer

- **`spine-core`**: Multi-session orchestration engine managing concurrent AI agent connections.
- **`spine-parser`**: Recursive semantic parser translating HTML into **Unified Representation (UR)** optimized for LLM context windows.
- **`spine-compiler`**: Compiles **SPINE Source (HLS)** into **SPINE Binary (HLB)** for the "websites-as-programs" paradigm.
- **`spine-wasm`**: High-performance execution runtime for HLB using WebAssembly.

### Transport Layer

- **`spine-protocol`**: Low-latency TCP-based protocol with encryption, compression, binary program execution, and the agentic-first frame family (`ToolCall`, `StreamToken`, `CapabilityQuery`, `TraceContext`). See [`agentic.rs`](src/spine-protocol/src/agentic.rs).
- **`spine-transport`**: High-performance zero-copy transport layer with BBR congestion control and connection pooling.
- **`spine-stream`**: Reactive streaming layer with multiplexing, flow control, chunked transfer, and priority queuing.

### Intelligence Layer

- **`spine-neural`**: **Titans architecture** (Neural Long-Term Memory) with MIRAS variants for adaptive protocol encoding.
- **`spine-crypto`**: **Titans-based speculative decoding**, X3DH key exchange, and quantum-resistant lattice cryptography with three security levels (Standard, Hardened, PostQuantum).
- **`spine-recursive`**: **Recursive Language Model** for extended context retrieval (10M+ chars) via REPL environment per arXiv:2512.24601. Note: trades reasoning depth for context breadth.
- **`spine-knowledge`**: **Unified bioinspired memory** with episodic (hippocampus), semantic (neocortex), working (prefrontal cortex), and collective (social brain) subsystems. CRDT-based distributed single-source-of-truth.

### Agent Layer

- **`spine-agent`**: High-level SDK for building AI agents that can navigate, parse, and execute on the SPINE stack.
- **`spine-agentic`**: Advanced agentic AI framework with swarm intelligence, cognitive architectures, and adversarial capabilities.
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

### Titans-Based Speculative Decoding

Uses a **TitansPredictor** with Neural Long-Term Memory to anticipate next messages, enabling:

- Zero-latency delivery when predictions match
- Anomaly detection via surprise scores
- Adaptive learning from communication patterns

### Chameleon Protocol

A **moving-target defense system** where the protocol's latent basis and encryption keys evolve per-message based on neural projections.

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

Agents can propose knowledge to the cluster. A **2/3 majority vote** is required for consensus:

```rust
// Propose a fact
client.propose_knowledge("quantum_threat", json!("High"), vec!["security"]).await?;

// Cluster votes automatically based on confidence
// If consensus reached, knowledge is committed to all nodes
```

### Long-Term Memory

Each agent has a persistent knowledge base with tagging and semantic querying:

```rust
// Store knowledge
client.store_knowledge("api_endpoint", json!("https://api.example.com"), vec!["config"]).await?;

// Query by tags
let results = client.query_knowledge("endpoint", vec!["config"], 10).await?;
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
morph()              // Trigger protocol morphing
decoy()              // Inject decoy traffic
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

- **Chameleon Protocol**: A latent-space cryptographic system that evolves the protocol's "shape" per-message using neural encoders.
- **Titans Speculative Decoding**: Message prediction using Neural Long-Term Memory to reduce perceived latency by pre-computing responses.
- **Quantum-Resistant Keys**: Lattice-based key evolution that resists quantum computing attacks.
- **Titans Neural Encoding**: Neural Long-Term Memory with VAE and Attention mechanisms to project web content into high-dimensional latent spaces.

## Intelligence Layer

SPINE features a deep intelligence layer that optimizes for both performance and security in AI-to-AI communication.

### Titans Speculative Decoding

Inspired by LLM inference techniques, SPINE uses a **TitansPredictor** with Neural Long-Term Memory to anticipate the next likely messages in a protocol stream.

- **Zero-Bandwidth Hits**: If a prediction is correct, the receiver reconstructs the message from its local cache, sending only a tiny confirmation hash.
- **Latency Reduction**: The core engine can pre-compute responses for predicted requests before they even arrive.
- **Pattern Obfuscation**: Speculative traffic makes the protocol stream appear as high-entropy noise to external observers.
- **Anomaly Detection**: High surprise scores indicate novel or potentially malicious patterns.

### Chameleon Protocol (Moving-Target Defense)

The Chameleon Protocol uses **Titans Neural Long-Term Memory** to hide communication patterns.

- **Latent Morphing**: Messages are projected into a high-dimensional latent space using a Variational Autoencoder (VAE).
- **Dynamic Evolution**: The transformation matrices evolve over time based on quantum-resistant seeds, ensuring that the "language" of the protocol is constantly changing.
- **Implicit Encryption**: The latent space projection itself acts as a form of encryption where the model weights and Titans memory state are the keys.

## Deployment

SPINE is designed for high-performance native deployment in distributed clusters.

### Quick Start (Local Cluster)

The easiest way to deploy a local SPINE cluster is using the provided deployment script:

```bash
# Start a 3-node local cluster (1 seed, 2 worker nodes)
./scripts/deploy.sh
```

This will start:

- **Seed Node**: Port 8080 (Central orchestrator)
- **Worker Nodes**: Ports 8081 and 8082, connected to the seed node.

Logs and data will be stored in the `./data` directory.

### Manual Deployment

To deploy a single node manually:

```bash
# Build the core server
cargo build --release -p spine-core

# Run the server
./target/release/spine-core
```

### Environment Variables

- `RUST_LOG`: Logging level (`info`, `debug`, `trace`)
- `PORT`: The port to listen on for agent connections (default: `8080`)
- `NODE_ID`: Unique identifier for the node
- `SEED_NODES`: Comma-separated list of seed nodes (e.g., `127.0.0.1:8080`)
- `SPINE_KNOWLEDGE_DIR`: Path for persistent knowledge storage
- `SPINE_SESSIONS_DIR`: Path for session data storage

## Getting Started

### Prerequisites

- Rust (latest stable, 2021 edition)
- Windows, Linux, or macOS

### Building

```bash
cargo build --release
```

### Running the Core

Start the SPINE browser engine:

```bash
cargo run -p spine-core
```

The core will listen on `127.0.0.1:8082` for agent and browser connections.

### Running the GUI Browser (Human Mode)

In a separate terminal, launch the cross-platform GUI:

```bash
cargo run -p spine-browser
```

The GUI allows you to:

- Enter URLs and navigate the web via the SPINE stack.
- Toggle **Human Mode** to transpile legacy HTML into AI-native HLS.
- View raw Unified Representations and agentic data streams.

### Running an Example Agent

In a separate terminal:

```bash
cargo run --example simple_agent -p spine-agent
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

The simple_agent example will:

1. Connect to the core with encryption enabled
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

### Chameleon Protocol (Moving-Target Defense)

SPINE introduces the **Chameleon Protocol**, a revolutionary approach to secure communication that treats latent-space representations as a form of implicit encryption.

**Core Insight**: High-dimensional vector spaces are inherently encrypted—the transformation matrix IS the key. By evolving the transformation based on message history, we create a protocol that is impossible to statically analyze.

#### Key Components

1. **Latent-Space Cryptography**: Data is projected into a high-dimensional vector space using a dynamically evolving basis. The projection matrix serves as the encryption key.

2. **Moving Target Defense**: After every message exchange:
   - The basis vectors rotate
   - The dimensionality may change (64-256 dimensions)
   - The header format morphs
   - The padding strategy shifts

3. **Forward Secrecy**: Each message's hash is incorporated into the key derivation, ensuring past messages cannot be decrypted even if the current key is compromised.

4. **Decoy Traffic**: Agents can inject noise traffic to confuse traffic analysis.

#### Enabling Chameleon Protocol

```rust
let secret: [u8; 32] = /* shared secret */;
client.handler.enable_chameleon(secret);

// Protocol now automatically:
// - Encodes messages into latent space
// - Morphs after each message
// - Evolves key material continuously
```

### Speculative Decoding

Inspired by LLM speculative decoding, SPINE predicts messages before they arrive and sends minimal confirmations when predictions match.

#### How It Works

1. **Output Speculation**: Before sending, check if the receiver predicted this message
   - If **hit**: Send only a hash confirmation (8 bytes vs. kilobytes)
   - If **miss**: Send full payload

2. **Input Speculation**: Predict what the sender will send next
   - Train on message patterns (n-grams, Markov chains)
   - Pre-compute responses for likely requests
   - Reduce latency by preparing before arrival

3. **Adaptive Learning**: The predictor improves over time
   - Observes message sequences
   - Builds transition probability tables
   - Adjusts confidence scores

#### Enabling Speculation

```rust
// Enable speculative decoding for both directions
client.handler.enable_speculation(true, true);

// Check statistics
let stats = client.handler.get_speculation_stats();
println!("Accuracy: {:.1}%", stats.output_accuracy() * 100.0);
println!("Bytes saved: {}", stats.bytes_saved);
```

#### Speculation Statistics

```
╔══════════════════════════════════════════════════════════════╗
║                    Speculation Statistics                     ║
╠══════════════════════════════════════════════════════════════╣
║ Output Predictions: 15       | Output Hits: 12               ║
║ Input Predictions:  15       | Input Hits:  11               ║
║ Bytes Saved:        48KB     | Precompute Hits: 3            ║
║ Output Accuracy:    80.0%    | Input Accuracy:  73.3%        ║
╚══════════════════════════════════════════════════════════════╝
```

### Traditional Encryption (Fallback)

For compatibility, SPINE also supports **AES-256-GCM** encryption:

```rust
let key = [0u8; 32]; // Use a secure key in production
client.handler.enable_encryption(key);
```

### Compression

All messages are automatically compressed using **Zstd** (compression level 3) to reduce bandwidth for large UR payloads.

### Binary Execution

Agents can send compiled HLB programs to the core for execution:

```rust
let binary = Compiler::compile("element App {}")?;
client.handler.send_message(&Message::Request(Request {
    id: "exec-1".to_string(),
    command: BrowserCommand::ExecuteBinary(binary),
})).await?;
```

## Architecture

SPINE bypasses traditional browser rendering pipelines (DOM → Layout → Paint) in favor of a multi-layered stack optimized for AI agents, with a compatibility layer for humans.

### The SPINE Stack (28 Crates)

1. **Kernel Layer**: `spine-kernel` — SIMD intrinsics, lock-free atomics, zero-copy ring buffers, custom allocators, RDTSC timing.
2. **Foundation Layer**: `spine-core` (orchestration), `spine-parser` (HTML → UR), `spine-compiler` (HLS → HLB), `spine-wasm` (WASM execution).
3. **Transport Layer**: `spine-protocol` (Chameleon Protocol), `spine-transport` (zero-copy I/O, BBR), `spine-stream` (reactive streaming, multiplexing).
4. **Intelligence Layer**: `spine-neural` (Titans architecture), `spine-crypto` (X3DH, quantum-resistant crypto), `spine-recursive` (extended context retrieval), `spine-knowledge` (CRDT-based distributed memory).
5. **Agent Layer**: `spine-agent` (SDK), `spine-agentic` (swarm intelligence), `spine-cluster` (distributed coordination with Sybil resistance).
6. **Compatibility Layer**: `spine-human` — legacy web bridge for bot-detection bypass.
7. **Application Layer**: `spine-browser` — cross-platform GUI browser.
8. **Bindings Layer**: `spine-ffi` (C FFI), `spine-go` (Go/cgo), `spine-python`* (PyO3), `spine-js`* (WASM).

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
│   Titans Speculative Decoding       │
│   ├─ Neural Long-Term Memory        │
│   ├─ Surprise-Gated Prediction      │
│   ├─ Pre-computed Response Cache    │
│   └─ Confirmation/Delta Encoding    │
├─────────────────────────────────────┤
│   Chameleon Layer                   │
│   ├─ Titans Latent-Space Encoding   │
│   ├─ Moving-Target Defense          │
│   └─ Decoy Traffic Generation       │
├─────────────────────────────────────┤
│   Compression (Zstd)                │
├─────────────────────────────────────┤
│   Morphing Frame Format             │
│   ├─ Variable Header Size           │
│   ├─ Dynamic Endianness             │
│   └─ Chaotic Padding                │
├─────────────────────────────────────┤
│   TCP (Port 8082)                   │
└─────────────────────────────────────┘
```

## Design Philosophy

1. **AI-First**: Every design decision prioritizes AI agent efficiency over human rendering.
2. **Binary Execution**: Websites are programs, not static documents.
3. **Latent Representations**: Native support for streaming embeddings and high-dimensional vectors.
4. **Moving-Target Security**: The protocol itself is a moving target—impossible to fingerprint or replay.
5. **Implicit Encryption**: High-dimensional projections provide encryption without explicit ciphertext.
6. **Speculative Intelligence**: Predict messages to reduce latency and bandwidth.

## Roadmap

- [x] Recursive UR parsing
- [x] Multi-session management
- [x] Traditional encryption (AES-256-GCM)
- [x] Compression (Zstd)
- [x] HLS/HLB basic compiler
- [x] **Chameleon Protocol** (latent-space cryptography)
- [x] **Moving-Target Defense** (dynamic protocol morphing)
- [x] **Decoy Traffic** injection
- [x] **Titans Speculative Decoding** (prediction-accelerated communication with NLM)
- [x] **Bi-directional Speculation** (input + output prediction)
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
- [x] **Autonomous Agent Loop** (ReasoningEngine with plan execution)
- [x] **Session History & Audit Trail** (full command logging)
- [x] **Capability Enforcement** (permission-based HLB execution)
- [x] **Swarm Planning API** (CreateSwarmPlan, ExecutePlanTask)
- [x] **HLS Memory Operations** (remember, query_memory, reason)
- [x] **Social Network Swarms** (hierarchical, small-world, scale-free, modular topologies)
- [x] **Collaborative Roles** (coordinator, executor, expert, validator, innovator, mediator)
- [x] **Adversarial Game Theory** (Nash equilibrium, minimax, regret matching)
- [x] **Zero-Copy Message Pooling** (power-of-2 size classes for efficient reuse)
- [x] **Compact Binary Protocol** (28-byte headers, minimal overhead)
- [x] **Full LTO Optimization** (30% binary size reduction)
- [x] **Transport Plugin System** (composable pipeline: metrics, rate-limiting, logging, tagging, size-limiting)
- [x] **WebSocket Bridge** (client/server `AsyncRead+AsyncWrite` adapters for `ProtocolHandler`)
- [x] **Multi-Transport Server** (`tokio::select!` TCP + WebSocket + QUIC accept loop)
- [x] **Agent `connect_ws()`** (WebSocket transport for `AgentClient`)
- [x] **QUIC Server Integration** (feature-gated `quinn` endpoint)
- [x] **Agent Capability Marketplace** (decentralized registry, discovery, bidding, contracts, reputation, audit log)

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

Per-record AEAD overhead is **identical** to TLS when both use the same primitive — SPINE encrypts with the same `aes-gcm` crate (AES-256-GCM) as `rustls`. SPINE's *crypto layer* wins (Chameleon Protocol's moving-target defense, X3DH key exchange, RLWE post-quantum) operate *above* the AEAD primitive and are measured separately in their respective crates.

### Reproduction

```bash
cargo bench --package spine-transport --bench spine_vs_www
cargo bench --package spine-transport --bench spine_vs_http2
cargo bench --package spine-transport --bench agentic_ai_workload
cargo bench --package spine-transport --bench llm_tok_per_sec
cargo bench --package spine-transport --bench llm_shm_ipc
cargo test  --package spine-transport rdma  # 4 trait tests
```

### Micro-benchmarks (re-measured 2026-06-08)

Absolute internal-operation timings, re-run on current hardware. These are the only
component/kernel numbers retained in the README because they are the ones reproduced this
session (`cargo bench -p spine-kernel --bench kernel_bench`, `-p spine-transport --bench
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

### Zero-Copy Message Pool

Efficient buffer management for high-throughput messaging:

```rust
let pool = MessagePool::new();

// Allocate from pool (power-of-2 size classes)
let mut buffer = pool.allocate(4096);
buffer.write(&data);

// Buffer returned to pool on drop (zero-copy reuse)
```

### Compact Binary Protocol

Minimal overhead messaging (28-byte header):

```rust
let msg = CompactMessage::new(
    message_types::TASK_ASSIGN,
    sender_id,
    receiver_id,
    payload,
);

// Serialize directly to wire format
let bytes = msg.to_bytes();
```

### Lightweight Swarm Coordination

Optimized for minimal memory and CPU overhead:

```rust
let mut swarm = LightweightSwarm::new(swarm_id);
swarm.add_agent(agent_id);
swarm.broadcast(sender, message_types::BROADCAST, &payload);
```

## Testing

SPINE includes comprehensive test coverage across all 28 crates:

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p spine-kernel
cargo test -p spine-neural
cargo test -p spine-crypto
```

### Test Summary (1,060 tests, 0 failures)

Latest workspace run: `cargo test --workspace --no-fail-fast` →
**1,060 passed / 0 failed / 5 ignored** across all 28 crates. The five
ignored entries are `no_run` / `ignore`-marked doctest fixtures, not
hidden failures. Per-crate breakdown below remains an approximation —
exact counts shift with each addition.


| Crate           | Tests | Description                                              |
| --------------- | ----- | -------------------------------------------------------- |
| spine-agentic   | 207   | Lifecycle, sandbox, scheduler, contracts, mesh, swarm    |
| spine-protocol  | 80    | Chameleon protocol, chaos, integration, property         |
| spine-transport | 57    | Zero-copy I/O, BBR, connection pooling, property         |
| spine-crypto    | 52    | RLWE, ML-KEM, Titans predictor, MIRAS, property         |
| spine-cluster   | 37    | Load balancing, session management, Sybil resistance     |
| spine-core      | 35    | Session orchestration, config, TLS/cert, CT logs         |
| spine-kernel    | 35    | SIMD, allocators, atomics, ring buffers                  |
| spine-stream    | 35    | Reactive streams, multiplexing, flow                     |
| spine-nostd     | 30    | Fixed-point math, FNV hashing, frame codec               |
| spine-wasm      | 30    | HLB compilation, execution, stack ops                    |
| spine-embedded  | 24    | Agent runtime, ring buffers, routing, watchdog           |
| spine-compiler  | 21    | HLS parsing, type checking, compilation                  |
| spine-neural    | 19    | VAE, attention, memory variants                          |
| spine-ffi       | 18    | C FFI null-safety, parse, compile, version               |
| spine-cache     | 16    | LRU, tiered caching, TTL eviction                        |
| spine-recursive | 15    | Infinite context, LLM dispatchers                        |
| spine-cli       | 15    | Init scaffolding, config, addr/tag parsing               |
| spine-k8s       | 13    | CRD generation, autoscaling, manifests                   |
| spine-gpu       | 12    | GPU compute, SIMD backend, WGSL shaders                  |
| spine-agent     | 11    | SDK API, protocol types, connection handling              |
| spine-knowledge | 9     | Episodic, semantic, collective memory                    |
| spine-storage   | 9     | SQLite WAL, RocksDB, typed storage                       |
| spine-parser    | 8     | HTML parsing, UR extraction, property tests              |
| spine-gateway   | 7     | REST API gateway, health checks                          |
| spine-human     | 2     | Human interaction patterns                               |

### Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark suite
cargo bench -p spine-kernel
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
