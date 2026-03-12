# SPINE Architecture

This document provides a deep dive into the technical architecture of the SPINE web stack.

## System Overview

SPINE is a **headless semantic browser with adaptive encryption** designed for AI agents. It consists of twenty-five crates that work together to provide semantic web extraction, binary program execution, secure agent communication, distributed scaling, high-performance streaming, and human-readable web compatibility.

```
┌──────────────────────────────────────────────────────────┐
│                     User Layer                           │
│  • spine-browser: Traditional GUI Browser           │
│  • spine-agent: High-level SDK for AI applications  │
└────────────────────┬─────────────────────────────────────┘
                     │
                     │ Chameleon Protocol + Speculative Decoding
                     │
┌────────────────────▼─────────────────────────────────────┐
│                  Protocol Layer                          │
│  (spine-protocol: Message encoding/decoding)        │
│  • Titans Neural Encoding (spine-neural)            │
│  • Titans Prediction (spine-crypto)                 │
│  • Quantum-Resistant Key Evolution (spine-crypto)   │
│  • Moving-Target Defense (Morphing)                      │
└────────────────────┬─────────────────────────────────────┘
                     │
┌────────────────────▼─────────────────────────────────────┐
│                   Core Engine                            │
│  (spine-core: Session management & orchestration)   │
│  • Multi-agent session handling (DashMap)                │
│  • Web content fetching (reqwest)                        │
│  • WASM Execution Runtime (spine-wasm)              │
└──────┬──────────────────────────┬────────────────────────┘
       │                          │
       │ HTML                     │ HLS Source
       │                          │
┌──────▼────────────┐      ┌──────▼──────────────┐
│  Parser Layer     │      │  Compiler Layer     │
│ (spine-parser)    │      │ (spine-compiler)    │
│                   │      │                     │
│                   │      │                     │
│ HTML → UR         │      │ HLS → HLB           │
│ (Recursive        │      │ (nom-based parser)  │
│  semantic         │      │                     │
│  extraction)      │      │                     │
└───────────────────┘      └─────────────────────┘
       │                          │
       └───────────┬──────────────┘
                   │
┌──────────────────▼───────────────────────────────────────┐
│                Compatibility Layer                       │
│  (spine-human: Legacy Web Bridge)                    │
│  • Backwards compatibility for traditional web          │
└──────────────────────────────────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────────────┐
│                Distributed Layer                         │
│  (spine-cluster: Scaling & Coordination)            │
│  • Consistent Hashing for Session Affinity               │
│  • Heartbeat-based Node Discovery                        │
│  • Distributed Load Balancing                            │
└──────────────────────────────────────────────────────────┘
```

## Component Details

### 1. spine-core

**Purpose**: Central orchestration engine that manages agent sessions and web content fetching.

**Key Responsibilities**:

- Accept incoming TCP connections from agents
- Create and manage isolated sessions (each with its own state)
- Route commands to appropriate handlers
- Fetch live web content via HTTP/HTTPS
- Coordinate between parser and protocol layers
- **Long-term Memory (Knowledge Base)**: Persistent fact storage for agents
- **Session History**: Full audit trail of agent actions
- **Capability Enforcement**: Permission-based execution for HLS scripts
- **Automated Persistence**: Periodic disk-sync for sessions and knowledge base

**Technology Stack**:

- `tokio`: Async runtime for high-concurrency session handling
- `reqwest`: HTTP client for web content fetching
- `dashmap`: Concurrent hash map for lock-free session storage
- `uuid`: Unique session identifier generation

**Session Lifecycle**:

1. Agent connects → New TCP stream
2. Core generates session ID (UUID)
3. Session stored in `DashMap<String, Session>`
4. Commands routed to session-specific handlers
5. Connection closes → Session optionally persisted for reconnection

**Command Processing**:

```rust
match command {
    Navigate { url } => {
        // Fetch HTML from URL
        // Store in session.current_html
        // Return success response
    }
    GetUR => {
        // Parse session.current_html
        // Return UnifiedRepresentation
    }
    ExecuteBinary(bin) => {
        // Execute HLB instructions using spine-wasm
        // Return execution result
    }
    Click { element_id } => {
        // Simulate user interaction
    }
    Type { element_id, text } => {
        // Simulate typing
    }
}
```

### 2. spine-parser

**Purpose**: Recursive semantic HTML parser that generates the Unified Representation.

**Key Innovation**: Unlike traditional parsers that preserve DOM structure, the SPINE parser **collapses** the HTML tree into a flat, semantic representation optimized for LLM consumption.

**Parsing Strategy**:

1. Use `scraper` to build an initial DOM tree
2. Recursively traverse the tree with `ego_tree::NodeRef`
3. Extract semantic elements based on HTML tags
4. Flatten nested structures while preserving logical relationships

**Element Extraction Logic**:

```rust
fn parse_node(node: NodeRef<Node>) -> Vec<Element> {
    match node.value() {
        Node::Element(ref elem) => {
            match elem.name() {
                "h1"..."h6" => extract_heading(node),
                "a" => extract_link(node),
                "button" => extract_button(node),
                "img" => extract_image(node),
                "ul" | "ol" => extract_list(node),
                "div" | "section" => extract_container(node),
                _ => recurse_children(node),
            }
        }
        Node::Text(text) => {
            if !text.trim().is_empty() {
                vec![Element::Text(text.trim().to_string())]
            } else {
                vec![]
            }
        }
    }
}
```

**Why This Approach?**

- **Context Window Efficiency**: Removes irrelevant tags, attributes, and whitespace.
- **Semantic Clarity**: LLMs understand "This is a button" better than `<button class="btn btn-primary">`.
- **Actionable Elements**: Every element in the UR has a clear purpose (navigate, click, read).

### 3. spine-protocol

**Purpose**: Low-latency, secure communication protocol between agents and the core.

**Chameleon Protocol**:

- **Latent-Space Cryptography**: Messages are projected into a high-dimensional latent space using `spine-neural`.
- **Moving-Target Defense**: The protocol "morphs" its encoding scheme periodically to resist traffic analysis.
- **Quantum-Resistant**: Uses Ring-LWE lattice-based cryptography from `spine-crypto` for key evolution.

**Speculative Decoding**:

- **Titans Prediction**: Uses a `TitansPredictor` with Neural Long-Term Memory to predict the next bytes of a message before they are fully received.
- **Anomaly Detection**: Surprise scores identify novel or malicious patterns.
- **Latency Reduction**: Allows the agent to begin processing responses while they are still in transit.

**Message Format**:

```
┌─────────────────┬──────────────────────────┐
│  Length (4B)    │  Payload (Variable)      │
│  (Big-endian)   │  (JSON)                  │
└─────────────────┴──────────────────────────┘
```

**Encryption Layer**:

- Algorithm: AES-256-GCM (Authenticated Encryption with Associated Data)
- Key Exchange: X3DH (Extended Triple Diffie-Hellman) for initial trust establishment
- Security Levels: Standard (X25519), Hardened (X25519+RLWE), PostQuantum (RLWE-only)
- Forward Secrecy: Double Ratchet with hash-chain key evolution

**Compression Layer**:

- Algorithm: Zstd (Zstandard)
- Level: 3 (balanced speed/ratio)
- Rationale: UR payloads can be 10-100KB; compression reduces this to 2-10KB

**Message Types**:

- **Request**: Agent → Core (commands like Navigate, GetUR)
- **Response**: Core → Agent (results or errors)
- **Event**: Core → Agent (async notifications like page load completion)
- **BinaryProgram**: Agent → Core (HLB execution request)

**Processing Pipeline**:

```
Outgoing:
Message → JSON Serialize → Zstd Compress → AES Encrypt → Length Prefix → TCP Send

Incoming:
TCP Receive → Strip Length → AES Decrypt → Zstd Decompress → JSON Deserialize → Message
```

### 4. spine-compiler

**Purpose**: Compiles SPINE Source (HLS) into SPINE Binary (HLB).

**HLS Language Design**:

HLS is a declarative language for defining web interfaces as instruction streams. It's inspired by HTML but designed for AI-native execution.

**Syntax Example**:

```hls
element App {
  attribute title "My Application"
  
  element Header {
    text "Welcome to SPINE"
  }
  
  element Content {
    button "Click Me" {
      on_click -> emit("button_clicked", { id: 1 })
    }
  }
}
```

**Compilation Process**:

1. **Lexing**: Tokenize HLS source
2. **Parsing**: Build AST using `nom` parser combinators
3. **Code Generation**: Emit HLB instructions
4. **Optimization**: (Future) Dead code elimination, instruction merging

**HLB Instruction Set**:

```rust
pub enum Instruction {
    DefineElement { id: u32, tag: String },
    SetAttribute { id: u32, key: String, value: String },
    AddChild { parent_id: u32, child_id: u32 },
    EmitEvent { name: String, payload: serde_json::Value },
    StreamLatent { vector: Vec<f32> },
    MorphProtocol { seed: u64 },
    Decoy { noise: Vec<f32> },
}
```

**Advanced Language Features**:

The HLS compiler now supports full programming constructs:

```hls
// Variables and State
let title = "Dashboard"
state counter = 0

// Conditional rendering
if counter > 0 {
    element StatusActive {}
} else {
    element StatusInactive {}
}

// Loop iteration
for item in [1, 2, 3] {
    element ListItem {
        text "Item content"
    }
}

// Expression evaluation
let sum = 1 + 2 * 3
let combined = first ++ " " ++ last
let is_valid = count > 0 && enabled
```

**Execution Model**:

HLB programs are executed by the core in a Virtual DOM environment. Unlike HTML (which is rendered), HLB is **run** like bytecode:

1. `DefineElement` creates an element in the virtual DOM
2. `SetAttribute` modifies element state
3. `AddChild` builds the tree structure
4. `EmitEvent` triggers callbacks to the agent
5. `StreamLatent` sends embeddings/vectors back to the agent
6. `MorphProtocol` triggers Chameleon Protocol morphing
7. `Decoy` injects noise for traffic analysis resistance

### 5. spine-wasm

**Purpose**: High-performance execution runtime for SPINE Binary (HLB).

**Architecture**:

- **Transpilation**: Converts HLB bytecode into WebAssembly Text (WAT).
- **JIT Execution**: Uses `wasmtime` to execute the generated WASM with near-native performance.
- **Host Bindings**: Provides a secure interface for WASM programs to interact with the Virtual DOM.

### 6. spine-cluster

**Purpose**: Distributed coordination layer for scaling SPINE across multiple nodes.

**Key Features**:

- **Consistent Hashing**: Ensures session affinity so an agent always talks to the same node.
- **Heartbeat Discovery**: Nodes broadcast their presence and health status.
- **Load Balancing**: Distributes new sessions across the cluster based on load.

### 7. spine-neural

**Purpose**: Neural network-based encoding for the Chameleon Protocol using the **Titans architecture**.

**Architecture**:

- **VAE (Variational Autoencoder)**: Learns stochastic latent projections of protocol messages.
- **Titans (Neural Long-Term Memory)**: Test-time training with persistent memory tokens and surprise-gated updates.
- **Attention**: Multi-head attention for history-aware message prediction.

**Why Titans over LSTM?**

| LSTM                         | Titans                                      |
| ---------------------------- | ------------------------------------------- |
| Fixed hidden state size      | Persistent memory tokens                    |
| Gradient-based training only | Test-time training (online adaptation)      |
| Forgets over long sequences  | Unbounded context via memory consolidation  |
| No anomaly detection         | Surprise-gated writes for novelty detection |

**Titans + MIRAS for Anomaly Detection + Pattern Adaptation**:

SPINE uses the [Titans + MIRAS framework](https://research.google/blog/titans-miras-helping-ai-have-long-term-memory/) because a headless semantic browser requires **continual adaptation**—the ability to detect anomalies and adapt to new patterns without offline retraining:

- **Test-Time Memorization**: Memory updates occur *during inference*, not just training. When the protocol encounters a new communication pattern, it adapts instantly.
- **Surprise-Based Gating**: The gradient magnitude acts as a "surprise metric"—routine data is ignored, novel/anomalous data is prioritized for permanent storage.
- **Momentum**: Captures not just immediate surprises but also relevant follow-up context.
- **Adaptive Forgetting**: Weight decay prevents memory overflow during unbounded sessions.
- **Deep Memory > Wide Memory**: MIRAS research shows deeper memory architectures outperform wider fixed-size states.

This is essential for:
1. Protocol evolution (Chameleon must continuously adapt to resist fingerprinting)
2. Real-time anomaly detection (surprise scores identify attacks)
3. Agent adaptation (each interaction improves future predictions)

### 8. spine-crypto

**Purpose**: Advanced cryptographic primitives and **Titans-based prediction**.

**Key Features**:

- **TitansPredictor**: Neural Long-Term Memory for byte-level speculative decoding with unbounded context.
- **Anomaly Detection**: Surprise-gated updates identify novel or malicious message patterns.
- **Quantum-Resistant Keys**: Ring-LWE lattice cryptography for post-quantum security.
- **Key Evolution**: Hash-chain based forward secrecy.

**Why Titans over standard Transformers?**

| Transformer                | Titans                           |
| -------------------------- | -------------------------------- |
| O(n²) attention complexity | O(1) memory complexity           |
| Fixed context window       | Unbounded persistent memory      |
| Static weights             | Test-time training adaptation    |
| No novelty detection       | Surprise-gated anomaly detection |

### 9. spine-agent

**Purpose**: High-level SDK for building AI agents that interact with SPINE.

**API Design Philosophy**:

- **Simplicity**: One-liner navigation and parsing
- **Async-First**: All methods return `Future`s for non-blocking I/O
- **Error Handling**: Uses `anyhow::Result` for ergonomic error propagation

**Example Usage**:

```rust
let mut client = AgentClient::connect("127.0.0.1:8080").await?;
client.handler.enable_encryption(key);

// Navigate to a website
client.navigate("https://example.com").await?;

// Get structured representation
let ur = client.get_ur().await?;
for element in ur.elements {
    match element {
        Element::Link { text, url } => {
            println!("Found link: {} -> {}", text, url);
        }
        _ => {}
    }
}

// Compile and execute HLS
let binary = Compiler::compile("element App {}")?;
client.handler.send_message(&Message::Request(Request {
    id: "exec-1".to_string(),
    command: BrowserCommand::ExecuteBinary(binary),
})).await?;
```

### 10. spine-human

**Purpose**: Legacy web bridge (compatibility layer) for traditional web content, transpiling HTML/CSS/JS into SPINE-native HLS/HLB.

**Key Responsibilities**:

- **HTML Parsing**: Uses `scraper` and `ego-tree` to traverse legacy DOM structures.
- **HLS Generation**: Recursively converts HTML nodes into semantic HLS instructions.
- **Backwards Compatibility**: Enables human users to access the traditional web through the SPINE stack.

### 11. spine-browser

**Purpose**: Cross-platform GUI browser application for human users.

**Key Features**:

- **Modern GUI**: Built with `egui` and `eframe` for high-performance, cross-platform rendering.
- **Agent Integration**: Uses `spine-agent` to communicate with the core engine.
- **Human Mode**: Seamlessly toggles between raw agentic views and transpiled human-readable content.
- **Async Runtime**: Integrates `tokio` to handle background network tasks without blocking the UI thread.

## Virtual DOM Runtime

**Purpose**: Execute HLB instructions and maintain an in-memory DOM representation.

**Architecture**:

```rust
pub struct VirtualDom {
    pub nodes: HashMap<u32, VNode>,
    pub roots: Vec<u32>,
}

pub struct VNode {
    pub id: u32,
    pub tag: String,
    pub attributes: HashMap<String, String>,
    pub children: Vec<u32>,
    pub parent_id: u32,
}
```

**Execution Flow**:

```
HLB Binary → HlbRuntime::execute() → ExecutionResult
                                          │
                                          ├─→ VirtualDom (element tree)
                                          ├─→ Events (emitted during execution)
                                          ├─→ MorphRequests (protocol changes)
                                          ├─→ Decoys (injected noise)
                                          └─→ Stats (performance metrics)
```

**Key Features**:

- **Lazy Evaluation**: Conditionals and loops are evaluated at compile-time when possible
- **Differential Rendering**: VDOM can compute patches between states for efficient updates
- **UR Generation**: VDOM can emit Unified Representation format for consistency

**Execution Result Structure**:

```rust
pub struct ExecutionResult {
    pub vdom: VirtualDom,
    pub events: Vec<VEvent>,
    pub morph_requests: Vec<MorphRequest>,
    pub decoys: Vec<DecoyInjection>,
    pub latent_streams: Vec<LatentStream>,
    pub stats: ExecutionStats,
}
```


### 12. spine-kernel

**Purpose**: Ultra-low-level hardware primitives for the agentic web.

**Key Components**:
- **SIMD Intrinsics**: AVX2/NEON dot product (57 GiB/s), softmax, matrix-vector multiply (15.5 Gelem/s).
- **Custom Allocators**: BumpAllocator (505 ps), SlabAllocator, ArenaAllocator.
- **Lock-Free Atomics**: PaddedAtomicU64, SeqLock, LockFreeStack, AtomicFlags (4.4 ns).
- **Ring Buffers**: SPSC/MPSC wait-free queues (1.36 ns per op, 700M ops/sec).
- **RDTSC Timing**: Sub-nanosecond measurement (2.6× faster than `Instant::now`).
- **Direct Syscalls**: mmap/munmap, CPU affinity, NUMA info, thread priority.
- **io_uring Support**: Linux kernel bypass I/O (optional feature).

### 13. spine-recursive

**Purpose**: Recursive Language Model for extended context retrieval (10M+ chars) based on arXiv:2512.24601.

**Important**: Provides extended context retrieval with a documented reasoning tradeoff — trades reasoning depth for context breadth.

**Key Components**:
- REPL-based environment externalization for unbounded context windows.
- Multiple LLM dispatchers (OpenAI, Anthropic, load-balanced).
- Graceful degradation (OfflineDispatcher, AdaptiveDispatcher).

### 14. spine-knowledge

**Purpose**: Unified bioinspired memory with CRDT-based distributed knowledge base.

**Memory Hierarchy**:
- **Episodic Memory** (hippocampus): Event-based temporal memory.
- **Semantic Memory** (neocortex): Fact storage and retrieval.
- **Working Memory** (prefrontal cortex): Active task context.
- **Collective Memory** (social brain): Swarm-wide shared knowledge via CRDTs.

## Advanced Features

### Titans Speculative Decoding

SPINE reduces perceived latency by predicting the next likely messages in a sequence using Neural Long-Term Memory.
1. The `TitansPredictor` analyzes historical message patterns with persistent memory.
2. The `ProtocolHandler` pre-computes responses for predicted requests.
3. If the agent's next request matches a prediction, the response is served with zero-bandwidth reconstruction from the local cache.
4. High surprise scores trigger anomaly alerts for security monitoring.

### MIRAS-Adaptive Encoding

The MIRAS framework (Memory, Inference, Retrieval, and Storage) provides **anomaly detection + pattern adaptation** variants that automatically adapt to traffic patterns:

**MIRAS Variants**:

| Variant    | Use Case              | Update Strategy                    |
| ---------- | --------------------- | ---------------------------------- |
| **Titans** | Baseline              | Surprise-gated writes              |
| **YAAD**   | High anomaly traffic  | Outlier-robust gradient clipping   |
| **MONETA** | Long-running sessions | Lp-norm stability (prevents drift) |
| **MEMORA** | Mixed traffic         | Probability-constrained updates    |

**Adaptive Switching Logic**:

```rust
let variant = if anomaly > threshold * 2.0 {
    MirasVariant::Yaad       // Outlier robustness
} else if anomaly > threshold {
    MirasVariant::Memora     // Balanced updates
} else if message_count > 10000 {
    MirasVariant::Moneta     // Long-running stability
} else {
    MirasVariant::Titans     // Baseline
};
```

**MIRAS Integration Points**:

1. **ChameleonKey (spine-protocol)**: MIRAS-adaptive latent encoding with automatic variant switching based on traffic anomalies.
2. **MirasTitansPredictor (spine-crypto)**: Dual-track surprise monitoring from both Titans and MIRAS encoders.
3. **MirasNeuralEncoder (spine-neural)**: Core MIRAS memory implementations (YAAD, MONETA, MEMORA).

**Combined Surprise Detection**:

```rust
let combined_surprise = (titans_surprise + miras_surprise) / 2.0;
```

This enables more robust anomaly detection by combining Titans' byte-level predictions with MIRAS's latent-space pattern recognition.

### Chameleon Protocol

A moving-target defense protocol that hides communication patterns in latent space.
1. Messages are encoded into high-dimensional vectors using the `NeuralLatentEncoder`.
2. The latent space is dynamically morphed using quantum-resistant seeds.
3. To an outside observer, the traffic appears as random noise or a different protocol entirely.

## Performance Characteristics

### Latency Targets

- **Connection Establishment**: <10ms (local), <100ms (remote)
- **Navigate Command**: 100-500ms (depends on network + site)
- **GetUR Command**: 10-50ms (parsing time for typical pages)
- **Binary Execution**: <5ms (for simple programs)

### Throughput

- **Concurrent Sessions**: 1000+ (tested on 8-core CPU)
- **Message Rate**: 10,000+ messages/sec per session
- **UR Size**: Typical 5-50KB (compressed: 1-10KB)

### Memory Usage

- **Core**: ~50MB baseline + ~5MB per active session
- **Agent**: ~10MB per connection

## Security Model

### Current State

- ✅ AES-256-GCM encryption
- ✅ Zstd compression
- ✅ X3DH key exchange (proper initial trust, no pre-shared secrets)
- ✅ Security Levels: Standard (X25519), Hardened (X25519+RLWE), PostQuantum (RLWE-only)
- ✅ Sybil resistance: Stake-weighted voting, node reputation, proof-of-work for identity
- ✅ Quantum-resistant RLWE lattice cryptography with forward-secure evolution
- ✅ TLS 1.3 for transport security

### Production Requirements

- [ ] Certificate-based authentication
- [ ] Real-time session monitoring dashboard

## Future Enhancements

### 1. Advanced HLS Features

- [x] User-defined functions: `fn greet(name) { ... }`
- [x] Reactive state with auto-rerender
- [ ] Type system for HLS

### 2. Observability

- [x] OpenTelemetry tracing integration
- [x] Prometheus metrics for cluster health
- [ ] Real-time session monitoring dashboard

### 3. Security Hardening

- [x] TLS 1.3 for transport security
- [x] X3DH key exchange
- [x] Sybil resistance
- [ ] Certificate-based authentication

## Conclusion

SPINE provides a streamlined tool for AI agents to interact with the web. By eliminating rendering overhead and providing structured, semantic representations, it enables agents to operate at speeds and scales previously impossible with traditional browsers.

---

## The Agentic Web Stack (spine-agentic)

### Vision: Beyond the Human Web

The traditional web was designed for humans: point, click, read, scroll. SPINE is a **headless semantic browser with adaptive encryption** — not a replacement for the web, but an efficient tool for AI agents to extract meaning, communicate securely, and coordinate in swarms.

```
┌─────────────────────────────────────────────────────────────────┐
│                    AGENTIC WEB STACK                            │
├─────────────────────────────────────────────────────────────────┤
│  Layer 5: Collective Intelligence                               │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
│  │ SwarmMind   │ │ Consensus   │ │ Emergence   │               │
│  └─────────────┘ └─────────────┘ └─────────────┘               │
├─────────────────────────────────────────────────────────────────┤
│  Layer 4: Agent Cognition                                       │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
│  │ Goals       │ │ Planning    │ │ Adaptation  │               │
│  │ Intentions  │ │ Reasoning   │ │ Memory      │               │
│  └─────────────┘ └─────────────┘ └─────────────┘               │
├─────────────────────────────────────────────────────────────────┤
│  Layer 3: Semantic Web                                          │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
│  │ Knowledge   │ │ Ontology    │ │ Inference   │               │
│  │ Graph       │ │ Mapping     │ │ Engine      │               │
│  └─────────────┘ └─────────────┘ └─────────────┘               │
├─────────────────────────────────────────────────────────────────┤
│  Layer 2: Latent Communication                                  │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
│  │ Chameleon   │ │ Speculative │ │ Neural      │               │
│  │ Protocol    │ │ Decoding    │ │ Encoding    │               │
│  └─────────────┘ └─────────────┘ └─────────────┘               │
├─────────────────────────────────────────────────────────────────┤
│  Layer 1: Transport                                             │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐               │
│  │ QUIC/0-RTT  │ │ TCP/TLS     │ │ WebSocket   │               │
│  └─────────────┘ └─────────────┘ └─────────────┘               │
└─────────────────────────────────────────────────────────────────┘
```

### Core Concepts

#### 1. Semantic Resource Locators

Agents don't navigate by URLs—they navigate by **meaning**:

- **Traditional URL**: ResourceLocator::Url("https://weather.gov/sf")
- **Semantic**: ResourceLocator::Semantic { concept: "weather", constraints: ["location:sf"] }
- **Latent Space**: ResourceLocator::LatentCoord { space: "knowledge", coordinates: [0.5, -0.3] }

#### 2. Intentions & Goals

Agents express **what** they want, not **how** to get it. The runtime handles navigation, extraction, and verification.

#### 3. Agent-to-Agent Communication

Messages include **latent encodings** for semantic matching, enabling agents to find relevant knowledge across the network.

#### 4. Collective Intelligence (Swarms)

Agents form swarms with roles: Leader, Coordinator, Worker, Validator, Observer. Swarms enable parallel execution, consensus building, and emergent intelligence.

#### 5. Knowledge Graph with Semantic Search

Agents maintain persistent knowledge with embedding-based retrieval for similarity queries.

### Integration with SPINE stack

| Agentic Layer   | SPINE Component | Purpose                                        |
| --------------- | --------------- | ---------------------------------------------- |
| Transport       | spine-protocol  | QUIC/TCP with Chameleon encryption             |
| Neural Encoding | spine-neural    | VAE + MIRAS latent projections                 |
| Prediction      | spine-crypto    | Titans predictor for speculative communication |
| Planning        | spine-compiler  | HLS→HLB for executable plans                   |
| Knowledge       | spine-cluster   | Distributed knowledge synchronization          |

### Advanced Agentic Features

#### 6. Decentralized Identity (DID)

Every agent has a cryptographic identity following W3C DID standards:

```rust
let did = AgentDID::generate("ResearchAgent");
did.add_service(ServiceEndpoint {
    service_type: ServiceType::AgentMessaging,
    endpoint: "wss://agent.spine.example/msg".into(),
    protocols: vec!["chameleon-v1".into()],
});
let signature = did.sign(message);
let verified = did.verify(message, &signature);
```

#### 7. Protocol Negotiation

Agents negotiate communication protocols semantically:

```rust
let mut negotiation = ProtocolNegotiation::initiate(
    agent_a_did,
    agent_b_did,
    vec![
        CommunicationProtocol::LatentSpace { encoder: "titans-v2".into(), dimension: 256 },
        CommunicationProtocol::SemanticJSON { schema_version: "2.0".into() },
    ],
);
negotiation.respond(&agent_b_acceptable);
// Status: Agreed(SemanticJSON)
```

#### 8. Emergent Agent Composition

Specialists combine into composite agents:

```rust
let mut composite = CompositeAgent::new(
    "ResearchTeam",
    CompositionStrategy::Parallel { aggregation: AggregationMethod::BestConfidence },
);
composite.add_component(data_miner, ComponentRole::Specialist { capability: ContentExtraction }, 1.0);
composite.add_component(writer, ComponentRole::Primary { domains: vec!["synthesis".into()] }, 0.8);
let routes = composite.route(&AgentCapability::ContentExtraction); // Returns relevant specialists
```

#### 9. Agent Marketplace

Discover, procure, and rate agent services:

```rust
let marketplace = AgentMarketplace::new();
marketplace.list_service(MarketplaceListing {
    title: "Research Assistant".into(),
    pricing: PricingModel::PerRequest { credits: 50 },
    sla: ServiceLevelAgreement { max_response_time_ms: 3000, uptime_percentage: 99.5, .. },
    ..
});
let tx = marketplace.procure(listing_id, consumer_id).await?;
marketplace.complete_transaction(tx, true, Some(5), Some("Great service!")).await;
```

#### 10. Temporal Reasoning

Causal chains, predictions, and schedule validation:

```rust
let reasoner = TemporalReasoner::new();
reasoner.record_event(TemporalEvent {
    event_type: "search_initiated".into(),
    causes: vec![user_request_id],
    effects: vec![results_delivered_id],
    ..
}).await;
let chain = reasoner.find_causal_chain(cause_id, effect_id).await;
let prediction = reasoner.predict("user_followup", 0.8, &[cause_id]);
```

#### 11. Context Bridging

Share context across agent boundaries with access policies:

```rust
let bridge = ContextBridge::new();
let pool_id = bridge.create_pool("research-session", owner_id, initial_context);
bridge.join_pool(&pool_id, collaborator_id)?;
bridge.share(&pool_id, updated_context, &owner_id)?;
let ctx = bridge.read(&pool_id, &collaborator_id)?;
```

#### 12. Fluent Agent Builder

Quick agent setup with chainable API:

```rust
let system = agent("MarketAnalyst")
    .with_capabilities(vec![ContentExtraction, KnowledgeManagement])
    .with_trust(TrustLevel::Verified)
    .with_did()
    .with_marketplace("Analysis Service", "Market insights", PricingModel::PerRequest { credits: 25 })
    .with_protocols(vec![CommunicationProtocol::LatentSpace { encoder: "titans".into(), dimension: 128 }])
    .build()
    .await;
```

### The Future of Web Interaction

With the Agentic Web Stack, SPINE enables:

1. **Semantic Navigation**: Find resources by meaning, not addresses
2. **Latent Communication**: Messages carry neural embeddings
3. **Collective Problem-Solving**: Swarms tackle complex tasks
4. **Anomaly Detection + Pattern Adaptation**: MIRAS-based pattern recognition
5. **Autonomous Operation**: Multi-step workflow execution
6. **Decentralized Identity**: Cryptographic agent identities (W3C DID)
7. **Protocol Agility**: Negotiate optimal communication protocols
8. **Emergent Composition**: Specialists combine into superagents
9. **Marketplace Economy**: Service discovery, reputation, and transactions
10. **Temporal Intelligence**: Causal reasoning and prediction
11. **Context Bridging**: Cross-agent knowledge sharing with policies

The agentic web isn't coming—it's here.

## Performance Architecture

### Build Optimization Profile

SPINE uses aggressive compilation optimizations for minimal binary size and maximum runtime performance:

```toml
[profile.release]
opt-level = 3        # Maximum optimization level
lto = "fat"          # Full link-time optimization across all crates
codegen-units = 1    # Single codegen unit for best optimization
panic = "abort"      # No unwinding overhead
strip = true         # Strip debug symbols
```

**Results**:
- **30% binary size reduction** with full LTO
- Core binary: 20.6 MB → 14.4 MB
- Browser binary: 10.6 MB → 7.7 MB

### Zero-Copy Memory Architecture

#### Message Pool Design

The `MessagePool` uses power-of-2 size classes for efficient buffer reuse:

```
Size Classes: 64B, 128B, 256B, 512B, 1KB, 2KB, 4KB, 8KB, 16KB, 32KB, 64KB, 128KB, 256KB, 512KB, 1MB

┌─────────────────────────────────────────────────────────────┐
│ MessagePool                                                 │
├─────────────────────────────────────────────────────────────┤
│ pools: Vec<DashMap<usize, Vec<Vec<u8>>>>                   │
│   └─ One pool per size class                                │
│   └─ Lock-free concurrent access                            │
│   └─ Max 1000 buffers per class                             │
├─────────────────────────────────────────────────────────────┤
│ allocate(size) → PooledBuffer                               │
│   1. Find smallest size class >= requested                  │
│   2. Try to pop from pool (O(1))                            │
│   3. If empty, allocate new Vec                             │
├─────────────────────────────────────────────────────────────┤
│ return_buffer(buffer, size_class)                           │
│   1. Clear buffer content                                   │
│   2. Push back to pool (O(1))                               │
└─────────────────────────────────────────────────────────────┘
```

#### Compact Message Format

28-byte header with minimal overhead:

```
┌────────────────────────────────────────────────────────────┐
│ CompactHeader (28 bytes, packed)                           │
├────────────────────────────────────────────────────────────┤
│ msg_type:  u8   │ Message type (PING, REQUEST, TASK, etc.) │
│ priority:  u8   │ Priority level (0-255)                   │
│ flags:     u16  │ Feature flags                            │
│ sender:    u32  │ Sender agent ID                          │
│ receiver:  u32  │ Receiver agent ID                        │
│ timestamp: u64  │ Unix timestamp                           │
│ sequence:  u64  │ Message sequence number                  │
├────────────────────────────────────────────────────────────┤
│ payload_len: u32                                           │
│ payload:     Vec<u8>                                       │
└────────────────────────────────────────────────────────────┘
```

### Adversarial Game Theory Engine

#### Nash Equilibrium Solver

Supports both pure and mixed strategy equilibria:

```
find_pure_nash(matrix):
  for each action_profile:
    if no_player_can_improve(action_profile):
      equilibria.push(action_profile)
  return equilibria

find_mixed_nash(matrix):  // Fictitious Play
  initialize strategies to uniform
  for iteration in 1..max_iterations:
    for each player:
      best_response = argmax(expected_payoff(strategies))
      action_counts[player][best_response] += 1
      strategies[player] = normalize(action_counts[player])
  return strategies
```

#### Minimax Solver (Zero-Sum Games)

Alpha-beta pruning for efficient search:

```
minimax(state, depth, alpha, beta, maximizing):
  if depth == 0 or terminal(state):
    return evaluate(state)
  
  if maximizing:
    value = -∞
    for action in actions:
      value = max(value, minimax(result(action), depth-1, alpha, beta, false))
      alpha = max(alpha, value)
      if beta <= alpha:
        break  // Prune
    return value
  else:
    value = +∞
    for action in actions:
      value = min(value, minimax(result(action), depth-1, alpha, beta, true))
      beta = min(beta, value)
      if beta <= alpha:
        break  // Prune
    return value
```

#### Regret Matching (CFR-Style Learning)

Agents converge to Nash equilibrium through counterfactual regret:

```
update_regret(action, payoff, counterfactual_payoffs):
  // Accumulate regret for each action
  for i in 0..num_actions:
    regret[i] += counterfactual_payoffs[i] - payoff
  
  // Compute regret-matching strategy
  positive_regret = regret.map(|r| max(0, r))
  total = sum(positive_regret)
  
  if total > 0:
    strategy = positive_regret / total
  else:
    strategy = uniform(num_actions)
  
  // Update running average strategy
  avg_strategy = (avg_strategy * n + strategy) / (n + 1)
```

### Social Network Swarm Architecture

#### Topology Construction

```
SocialSwarmNetwork:
  ├─ Star: Hub-and-spoke with central coordinator
  ├─ Hierarchical: Tree structure with depth/branching params
  ├─ FullMesh: Complete graph (all pairs connected)
  ├─ Ring: Circular bidirectional connections
  ├─ SmallWorld: Ring + random rewiring (Watts-Strogatz)
  ├─ ScaleFree: Preferential attachment (Barabási-Albert)
  ├─ Modular: Dense clusters + sparse inter-cluster
  └─ Dynamic: Triadic closure + strength-based evolution
```

#### Influence Propagation (PageRank-Style)

```
propagate_influence(iterations, damping=0.85):
  for each agent:
    influence[agent] = 1/N
  
  for i in 1..iterations:
    for each agent:
      incoming = (1 - damping) / N
      for each relationship where to == agent:
        out_degree = count(relationships where from == rel.from)
        incoming += damping * influence[rel.from] * rel.strength * rel.trust / out_degree
      new_influence[agent] = incoming
    
    normalize(new_influence)
    influence = new_influence
```

#### Role-Based Task Distribution

```
distribute_task(description, required_roles):
  1. Find coordinator (highest influence among Coordinator role)
  2. Assign coordinator task with priority 1.0
  
  3. For each required_role:
     Find agents with matching role
     Assign with priority 0.8, dependency on coordinator
  
  4. Add executors for main work
     Assign with priority 0.7, dependency on coordinator
  
  return TaskDistribution { task_id, assignments, coordinator }
```

---

## 15. spine-transport

**Purpose**: High-performance zero-copy transport layer with BBR congestion control.

**Key Components**:

- **Zero-Copy I/O**: Uses `io_uring` on Linux for kernel-bypassing I/O
- **BBR Congestion Control**: Bottleneck Bandwidth and Round-trip propagation time
- **Connection Pooling**: Reusable connections with health checking
- **Frame Protocol**: Binary framing with CRC32 checksums
- **Write Coalescing**: Batches small writes for efficiency

**Architecture**:

```
┌─────────────────────────────────────────────────────────┐
│                  Application Layer                      │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              Connection Pool                            │
│  • Health checking         • Automatic reconnection     │
│  • Load balancing          • Connection limits          │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              BBR Congestion Control                     │
│  • Bandwidth estimation    • RTT tracking               │
│  • Pacing rate control     • ProbeRTT state             │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              Frame Layer                                │
│  • Binary framing          • CRC32 checksums            │
│  • Write coalescing        • Scatter-gather I/O         │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              Zero-Copy I/O (io_uring)                   │
│  • Kernel bypass           • Completion queues          │
│  • Registered buffers      • SQ/CQ batching             │
└─────────────────────────────────────────────────────────┘
```

---

## 16. spine-stream

**Purpose**: Reactive streaming layer with multiplexing, flow control, and priority queuing.

**Key Components**:

- **Stream Multiplexing**: Multiple logical streams over single connection
- **Flow Control**: AIMD congestion control with RTT estimation
- **Chunked Transfer**: Large data transfer with compression
- **Priority Queuing**: Weighted fair queuing and deadline scheduling
- **Latent Streaming**: Native streaming of neural embeddings

**Architecture**:

```
┌─────────────────────────────────────────────────────────┐
│                  Application Layer                      │
│  • BackpressureStream      • BatchingStream             │
│  • RateLimitedStream       • WindowedStream             │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              Stream Multiplexer                         │
│  • Stream ID allocation    • Per-stream flow control    │
│  • Priority scheduling     • Stream lifecycle mgmt      │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              Flow Controller (AIMD)                     │
│  • Sliding window          • Congestion avoidance       │
│  • RTT estimation          • Slow start                 │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              Chunked Transfer                           │
│  • Chunk fragmentation     • Reassembly                 │
│  • zstd/lz4 compression    • CRC32 verification         │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              spine-transport                       │
└─────────────────────────────────────────────────────────┘
```

**Stream Message Types**:

```rust
enum StreamPayload {
    Bytes(Vec<u8>),                    // Raw bytes
    LatentVector { dimensions, vector }, // Neural embeddings
    LatentBatch { ... },               // Batched embeddings
    Chunk { meta, data },              // Chunked transfer
    Control(StreamControl),            // Flow control
    Event(StreamEvent),                // Stream events
    Compressed { algorithm, data },    // Compressed data
}
```

---

## 17. spine-agentic

**Purpose**: Advanced agentic AI framework with swarm intelligence and cognitive architectures.

**Key Capabilities**:

- **Swarm Intelligence**: Multi-agent coordination with emergent behaviors
- **Cognitive Architecture**: Goal decomposition, reasoning, and learning
- **Adversarial Capabilities**: Game-theoretic agents with counterfactual regret
- **Social Networks**: Influence propagation and trust modeling
- **Neural Compression**: Latent-space communication encoding

**Architecture**:

```
┌─────────────────────────────────────────────────────────┐
│              Agentic Web Runtime                        │
│  • Agent registry          • Swarm management           │
│  • Task execution          • Knowledge sharing          │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              Cognitive Layer                            │
│  • ReasoningEngine         • GoalDecomposer             │
│  • AgentLearner            • SkillLibrary               │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              Communication Layer                        │
│  • SemanticRouter          • MessageBroker              │
│  • ContextBridge           • PerformativeProtocol       │
└─────────────────────────┬───────────────────────────────┘
                          │
┌─────────────────────────▼───────────────────────────────┐
│              Swarm Intelligence                         │
│  • EmergentBehaviorDetector • SocialSwarmNetwork        │
│  • CollectiveIntelligence   • TaskDistribution          │
└─────────────────────────────────────────────────────────┘
```

**Agent Types**:

- **Worker**: Executes assigned tasks
- **Coordinator**: Manages workflow orchestration
- **Explorer**: Discovers new information
- **Guardian**: Monitors and enforces policies
- **Learner**: Adapts from experience

---

## 18. spine-gpu

**Purpose**: GPU-accelerated neural encoding with cross-platform compute abstraction.

**Key Components**:

- **ComputeBackend Trait**: Unified interface for mat-vec multiply, softmax, layer-norm, VAE forward
- **CpuBackend**: SIMD 8-wide unrolled dot products (AVX2/NEON), fast rsqrt
- **WgpuBackend**: WGSL compute shaders via wgpu (Vulkan/Metal/DX12/WebGPU)
- **GpuAccelerator**: Auto-selects best available backend at runtime

---

## 19. spine-storage

**Purpose**: Persistent storage backends for knowledge base and cluster state.

**Key Components**:

- **StorageBackend Trait**: `get/put/delete/scan/keys/batch_put/count/clear`
- **InMemoryBackend**: Fast ephemeral HashMap-based storage for testing
- **SqliteBackend**: Embedded relational storage with WAL journaling (default)
- **RocksDbBackend**: High-performance LSM-tree storage with column families (optional feature)
- **TypedStorage**: Generic wrapper providing serde serialization over any backend
- **PersistentKnowledge**: Adapter integrating storage with spine-knowledge

---

## 20. spine-cache

**Purpose**: Tiered caching system with three levels for latency-optimized data access.

**Key Components**:

- **L1 Cache**: In-memory LRU with TTL and byte-size limits (~10 ns access)
- **L2 Cache**: Memory-mapped file-backed cache (~1 us access)
- **L3 Cache**: Remote/delegated cache trait (~1 ms access)
- **TieredCache**: Cascading lookup (L1 -> L2 -> L3) with promotion-on-hit

---

## 21. spine-k8s

**Purpose**: Kubernetes operator for managing SPINE cluster deployments with auto-scaling.

**Key Components**:

- **SpineClusterSpec CRD**: Declarative cluster configuration (replicas, image, resources, scaling)
- **AutoScaler**: CPU, memory, and connection-count based scaling with configurable thresholds
- **ManifestGenerator**: StatefulSet, Service, and HPA Kubernetes manifest generation
- **HealthMonitor**: Periodic health checks with automatic pod restart policies

---

## 22. spine-cli

**Purpose**: Command-line interface for the SPINE agentic web stack.

**Commands**: init, connect (REPL), query, deploy, benchmark, status. Built with clap derive macros.

---

## 23. spine-gateway

**Purpose**: OpenAPI REST gateway exposing SPINE APIs to non-Rust clients.

**Key Components**:

- **Axum Router**: RESTful endpoints for sessions, navigation, search, HLS execution
- **Swagger UI**: Auto-generated interactive API docs via utoipa
- **Health Endpoints**: /health, /ready, /metrics for production monitoring

---

## 24. spine-python (excluded from workspace)

**Purpose**: Python bindings via PyO3 + maturin. Exposes PyClient, PyUnifiedRepresentation, PySpineBinary.

---

## 25. spine-js (excluded from workspace)

**Purpose**: TypeScript/WASM bindings via wasm-bindgen + wasm-pack. Exposes parseHtml(), compileHls().

---

## 26. spine-ffi

**Purpose**: C-compatible foreign function interface for cross-language interop.

**Key Components**:

- **16 Exported Functions**: connect, disconnect, navigate, get_ur, get_raw_html, search, execute_hls, ping, morph, get_capabilities, store_knowledge, query_knowledge, parse_html, compile_hls, version, free_string
- **C Header**: `include/spine.h` with full documentation
- **Thread-Local Error Handling**: `spine_last_error()` for retrieving error messages
- **Build Targets**: `cdylib` (.so/.dylib/.dll) and `staticlib` (.a/.lib)

---

## 27. spine-go (non-Rust)

**Purpose**: Go bindings using cgo and the spine-ffi C library.

**Key Components**:

- **Client Type**: Connect, Navigate, GetUR, GetRawHTML, Search, ExecuteHLS, Ping, Morph, GetCapabilities, StoreKnowledge, QueryKnowledge, Close
- **Offline Functions**: ParseHTML, CompileHLS, Version (no server needed)
- **Go-Native Types**: UnifiedRepresentation, Element, SpineBinary, ExecutionResult
