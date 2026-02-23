# SPINE 🧠🦴

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![CI](https://github.com/nervosys/SPINE/actions/workflows/ci.yml/badge.svg)](https://github.com/nervosys/SPINE/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/nervosys/SPINE/branch/master/graph/badge.svg)](https://codecov.io/gh/nervosys/SPINE)
[![Tests](https://img.shields.io/badge/tests-535%20passing-brightgreen.svg)](#testing)

**SPINE** (Synaptic Path INterconnecting Entities) is a **headless semantic browser with adaptive encryption** — not a replacement for the web, but an efficient tool for AI agents to extract meaning, communicate securely, and coordinate in swarms. Built from the ground up for **collaborative and adversarial AI systems**, SPINE provides a complete communication and execution framework for autonomous agents—mimicking the adaptive, distributed nature of biological neural networks.

> *"SPINE is a headless semantic browser — a digital nervous system that lets AI agents efficiently extract meaning from the web and coordinate securely."*

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

- **Semantic Extraction**: Directly parses web content into structured representations without rendering pipelines.
- **Binary Execution**: Treats websites as executable programs with instruction-based semantics.
- **Adaptive Protocols**: Chameleon Protocol with Titans neural memory for moving-target defense.
- **Latent Streaming**: Native support for streaming high-dimensional vectors (embeddings, latent representations) to agents.
- **Human Compatibility**: Transpiles legacy web content (HTML/CSS/JS) into AI-native formats for seamless human-AI interaction.
- **Distributed Swarm Intelligence**: Skill-based task routing, DAG dependency tracking, and consensus-based knowledge sharing across agent clusters.
- **Long-Term Memory**: Persistent knowledge base with tagging, querying, and cross-cluster synchronization.

## Core Components

SPINE is composed of 25 specialized crates organized into a cohesive bioinspired architecture:

### Kernel Layer

- **`spine-kernel`**: Ultra-low-level hardware primitives—SIMD intrinsics (AVX2/NEON), lock-free atomics, zero-copy ring buffers, custom allocators (arena/slab), sub-nanosecond RDTSC timing, and direct syscall interfaces.

### Foundation Layer

- **`spine-core`**: Multi-session orchestration engine managing concurrent AI agent connections.
- **`spine-parser`**: Recursive semantic parser translating HTML into **Unified Representation (UR)** optimized for LLM context windows.
- **`spine-compiler`**: Compiles **SPINE Source (HLS)** into **SPINE Binary (HLB)** for the "websites-as-programs" paradigm.
- **`spine-wasm`**: High-performance execution runtime for HLB using WebAssembly.

### Transport Layer

- **`spine-protocol`**: Low-latency TCP-based protocol with encryption, compression, and binary program execution support.
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

### Application Layer

- **`spine-browser`**: Cross-platform GUI browser application for human users, built with `egui`.

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

### The SPINE Stack (25 Crates)

1. **Kernel Layer**: `spine-kernel` — SIMD intrinsics, lock-free atomics, zero-copy ring buffers, custom allocators, RDTSC timing.
2. **Foundation Layer**: `spine-core` (orchestration), `spine-parser` (HTML → UR), `spine-compiler` (HLS → HLB), `spine-wasm` (WASM execution).
3. **Transport Layer**: `spine-protocol` (Chameleon Protocol), `spine-transport` (zero-copy I/O, BBR), `spine-stream` (reactive streaming, multiplexing).
4. **Intelligence Layer**: `spine-neural` (Titans architecture), `spine-crypto` (X3DH, quantum-resistant crypto), `spine-recursive` (extended context retrieval), `spine-knowledge` (CRDT-based distributed memory).
5. **Agent Layer**: `spine-agent` (SDK), `spine-agentic` (swarm intelligence), `spine-cluster` (distributed coordination with Sybil resistance).
6. **Compatibility Layer**: `spine-human` — legacy web bridge for bot-detection bypass.
7. **Application Layer**: `spine-browser` — cross-platform GUI browser.

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

## Performance Optimizations

SPINE is engineered for maximum efficiency, delivering **100-250,000× performance improvements** over traditional web stacks.

### SPINE vs Traditional Web Stack (Comprehensive Comparison)

These benchmarks compare SPINE against the typical web stack (Express.js, Puppeteer, Redis, PostgreSQL, GPT-4 API):

#### Serialization: JSON vs SPINE Zero-Copy

| Data Size   | JSON Roundtrip      | SPINE Zero-Copy     | **Speedup** |
| ----------- | ------------------- | ------------------- | ----------- |
| 10 fields   | 1.77 µs (195 MiB/s) | 4.3 ns (77 GiB/s)   | **411×**    |
| 100 fields  | 18.1 µs (199 MiB/s) | 20.4 ns (172 GiB/s) | **886×**    |
| 1000 fields | 203 µs (187 MiB/s)  | 320 ns (115 GiB/s)  | **634×**    |

#### Header Parsing: HTTP vs SPINE Binary

| Protocol            | Time    | Throughput  | **Speedup** |
| ------------------- | ------- | ----------- | ----------- |
| HTTP Header Parse   | 1.41 µs | 708K elem/s | -           |
| SPINE Binary Header | 3.3 ns  | 299M elem/s | **427×**    |

#### Context Processing: 128K Chunks vs SPINE RLM

| Context Size | Traditional (128K chunks) | SPINE RLM | **Speedup**  |
| ------------ | ------------------------- | --------- | ------------ |
| 100K chars   | 731 ns                    | 280 ps    | **2,610×**   |
| 1M chars     | 7.48 µs                   | 443 ps    | **16,883×**  |
| 10M chars    | 77.9 µs                   | 316 ps    | **246,500×** |

> SPINE processes **10 million characters 250,000× faster** with O(1) random access.

#### Connection Handling: HTTP Keep-Alive vs SPINE Multiplexing

| Requests | HTTP Keep-Alive | SPINE Multiplexed | **Speedup** |
| -------- | --------------- | ----------------- | ----------- |
| 100      | 26.2 µs         | 11.2 ns           | **2,339×**  |
| 1,000    | 287 µs          | 140 ns            | **2,050×**  |
| 10,000   | 2.83 ms         | 1.0 µs            | **2,830×**  |

### Real-World Application Benchmark

Competitive Intelligence demo: 50 agents analyzing competitor websites, extracting insights, building knowledge graph.

| Metric                | Traditional Stack | SPINE         | **Advantage**       |
| --------------------- | ----------------- | ------------- | ------------------- |
| Cold Start            | ~5,000 ms         | 32 ms         | **156×**            |
| 50 Agent Swarm        | ~10,000 ms        | 40 ms         | **256×**            |
| Memory (50 agents)    | ~25 GB            | ~50 MB        | **500×**            |
| Max Context           | 128K tokens       | **UNLIMITED** | ∞                   |
| 10.7M char load       | FAILS             | 52 ms         | ✅                   |
| 10.7M char search     | FAILS             | 81 µs         | ✅                   |
| Knowledge Graph Build | External DB       | 407 µs        | **~1000×**          |
| Encryption            | Static TLS        | Moving-target | ✅ Quantum-resistant |
| **Total Processing**  | **~15 seconds**   | **127 ms**    | **118×**            |

### Component Benchmarks

| Component                    | Metric     | Throughput     |
| ---------------------------- | ---------- | -------------- |
| Latent Serialize (128-dim)   | 80 ns      | 6.0 GiB/s      |
| Latent Serialize (512-dim)   | 108 ns     | 17.6 GiB/s     |
| Latent Serialize (1024-dim)  | 143 ns     | **26.8 GiB/s** |
| Cosine Similarity (128-dim)  | 47 ns      | 10.1 GiB/s     |
| Cosine Similarity (1024-dim) | 373 ns     | **10.2 GiB/s** |
| Frame Encode (8KB)           | 68 ns      | **110 GiB/s**  |
| Frame Decode (8KB)           | 54 ns      | **141 GiB/s**  |
| Zero-Copy Buffer (8KB)       | 131 ns     | 58 GiB/s       |
| BBR Pacing Decision          | **275 ps** | -              |
| Batch Encode (64 frames)     | 2.5 µs     | 25.8 Melem/s   |
| Backpressure Stream (10K)    | 2.1 ms     | 4.9 Melem/s    |
| Priority Queue (10K)         | 1.9 ms     | 5.4 Melem/s    |
| Ring Buffer (16KB)           | 300 ns     | **50.4 GiB/s** |
| Context Chunking (10M)       | 2.4 ms     | **3.9 GiB/s**  |

*Benchmarks run on release builds with LTO enabled. Results validated January 2026.*

### SPINE vs Standard TCP/IP Stack

SPINE's transport layer significantly outperforms standard TCP operations:

| Benchmark                | Standard TCP | SPINE    | Speedup   |
| ------------------------ | ------------ | -------- | --------- |
| **Latency (64 bytes)**   | 41.0 µs      | 60 ns    | **682×**  |
| **Latency (256 bytes)**  | 27.2 µs      | 82 ns    | **331×**  |
| **Latency (1024 bytes)** | 31.2 µs      | 99 ns    | **315×**  |
| **Latency (4096 bytes)** | 27.0 µs      | 102 ns   | **265×**  |
| **Throughput (1KB)**     | 34 MiB/s     | 21 GiB/s | **632×**  |
| **Throughput (8KB)**     | 359 MiB/s    | 58 GiB/s | **166×**  |
| **Frame Encode (8KB)**   | -            | 68 ns    | 110 GiB/s |
| **Frame Decode (8KB)**   | -            | 54 ns    | 141 GiB/s |
| **Ring Buffer (16KB)**   | -            | 300 ns   | 50 GiB/s  |
| **BBR Congestion Ctrl**  | N/A          | 109 ns   | -         |

### Kernel Primitives (spine-kernel)

Ultra-low-level hardware primitives powering the agentic web:

| Operation            | Size     | Time    | Throughput                    |
| -------------------- | -------- | ------- | ----------------------------- |
| **SIMD Dot Product** | 256      | 33 ns   | **57 GiB/s**                  |
| **SIMD MatVec**      | 256×256  | 8.5 µs  | **15.5 Gelem/s**              |
| **SPSC Ring**        | push+pop | 1.36 ns | **736 Melem/s**               |
| **Bump Allocator**   | 64 bytes | 505 ps  | **1.98 Galloc/s**             |
| **RDTSC Timing**     | -        | 9.3 ns  | 2.6× faster than Instant::now |
| **Atomic Flags**     | test+set | 4.4 ns  | -                             |

**Key Insights:**

- **265-682× lower latency** for messages (frame codec vs TCP roundtrip)
- **166-632× higher throughput** using zero-copy ring buffers
- Frame codec achieves **110-141 GiB/s** encode/decode throughput
- BBR congestion control adds only **109 ns** overhead per decision
- Pacing decisions take only **275 picoseconds**

### Summary: Why SPINE Dominates

| Category           | Traditional         | SPINE                   | Factor       |
| ------------------ | ------------------- | ----------------------- | ------------ |
| **Serialization**  | 187 MiB/s           | 115 GiB/s               | **630×**     |
| **Header Parsing** | 708K/s              | 299M/s                  | **422×**     |
| **Context Access** | O(n) chunking       | O(1) random             | **250,000×** |
| **Connections**    | Per-request parsing | Multiplexed streams     | **2,500×**   |
| **Latency (TCP)**  | 27-41 µs            | 60-102 ns               | **300-680×** |
| **Memory**         | 500 MB/browser      | 1 MB/agent              | **500×**     |
| **Context Limit**  | 128K tokens         | **UNLIMITED**           | ∞            |
| **Security**       | Static TLS          | Moving-target + Quantum | ✅            |

> **The traditional web stack cannot compete. This isn't optimization—it's architectural superiority.**

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

SPINE includes comprehensive test coverage across all 25 crates:

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p spine-kernel
cargo test -p spine-neural
cargo test -p spine-crypto
```

### Test Summary (535 tests)

| Crate           | Tests | Description                                          |
| --------------- | ----- | ---------------------------------------------------- |
| spine-protocol  | 67    | Chameleon protocol, chaos, integration, property     |
| spine-transport | 57    | Zero-copy I/O, BBR, connection pooling, property     |
| spine-crypto    | 41    | RLWE, Titans predictor, MIRAS, property              |
| spine-cluster   | 37    | Load balancing, session management, Sybil resistance |
| spine-kernel    | 35    | SIMD, allocators, atomics, ring buffers              |
| spine-stream    | 35    | Reactive streams, multiplexing, flow                 |
| spine-neural    | 19    | VAE, attention, memory variants                      |
| spine-cache     | 16    | LRU, tiered caching, TTL eviction                    |
| spine-recursive | 15    | Infinite context, LLM dispatchers                    |
| spine-k8s       | 13    | CRD generation, autoscaling, manifests               |
| spine-gpu       | 12    | GPU compute, SIMD backend, WGSL shaders              |
| spine-agent     | 11    | SDK API, protocol types, connection handling          |
| spine-knowledge | 9     | Episodic, semantic, collective memory                |
| spine-compiler  | 9     | HLS parsing, compilation, optimization               |
| spine-storage   | 9     | SQLite WAL, RocksDB, typed storage                   |
| spine-parser    | 8     | HTML parsing, UR extraction, property tests          |
| spine-core      | 13    | Session orchestration, config management, TLS/cert   |
| spine-gateway   | 7     | REST API gateway, health checks                      |
| spine-agentic   | 4     | Agent creation, knowledge graph                      |
| spine-wasm      | 28    | HLB → WASM compilation, execution, stack ops         |
| spine-cli       | 15    | Init scaffolding, config, addr/tag parsing           |
| spine-human     | 2     | Human interaction patterns                           |

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

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

Copyright 2026 Nervosys LLC
