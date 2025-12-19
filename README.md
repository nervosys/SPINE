# Hyperlight

**Hyperlight** is a revolutionary web browser stack built from the ground up for **agentic AI** and **human-AI collaboration**. Unlike traditional browsers designed for human rendering, Hyperlight provides:

- **Semantic Extraction**: Directly parses web content into structured representations without rendering pipelines.
- **Binary Execution**: Treats websites as executable programs with instruction-based semantics.
- **Secure & Compressed Protocols**: AES-256-GCM encryption and Zstd compression for high-throughput AI communication.
- **Latent Streaming**: Native support for streaming high-dimensional vectors (embeddings, latent representations) to agents.
- **Human Compatibility**: Transpiles legacy web content (HTML/CSS/JS) into AI-native formats for seamless human-AI interaction.

## Core Components

Hyperlight is composed of 11 specialized crates:

- **`hyperlight-core`**: Multi-session orchestration engine managing concurrent AI agent connections.
- **`hyperlight-parser`**: Recursive semantic parser translating HTML into **Unified Representation (UR)** optimized for LLM context windows.
- **`hyperlight-protocol`**: Low-latency TCP-based protocol with encryption, compression, and binary program execution support.
- **`hyperlight-compiler`**: Compiles **Hyperlight Source (HLS)** into **Hyperlight Binary (HLB)** for the "websites-as-programs" paradigm.
- **`hyperlight-agent`**: High-level SDK for building AI agents that can navigate, parse, and execute on the Hyperlight stack.
- **`hyperlight-human`**: Transpiler for legacy web content (HTML/CSS/JS) into Hyperlight-native HLS/HLB.
- **`hyperlight-browser`**: Cross-platform GUI browser application for human users, built with `egui`.
- **`hyperlight-wasm`**: High-performance execution runtime for HLB using WebAssembly.
- **`hyperlight-cluster`**: Distributed coordination layer for scaling across multiple nodes.
- **`hyperlight-neural`**: Neural network-based encoding (VAE, LSTM, Attention) for the Chameleon Protocol.
- **`hyperlight-crypto`**: Advanced cryptographic primitives including transformer-based prediction and quantum-resistant keys.

## Intelligence Layer

Hyperlight features a sophisticated intelligence layer that optimizes communication and security:

- **Speculative Decoding**: Uses a Transformer-based predictor to anticipate next messages, allowing the receiver to pre-compute responses and achieve zero-latency delivery on hits.
- **Chameleon Protocol**: A moving-target defense system where the protocol's latent basis and encryption keys evolve per-message based on neural projections.
- **Latent Streaming**: Asynchronous streaming of high-dimensional vectors from the WASM runtime to the agent, enabling real-time neural state synchronization.

## Virtual DOM & Incremental Updates

The Hyperlight core maintains a **Virtual DOM** for each session, enabling efficient incremental updates:

- **HLB Execution**: Hyperlight Binary is executed in a sandboxed WASM environment, producing a Virtual DOM tree.
- **VDom Diffing**: The core computes the minimal set of patches (Create, Remove, SetAttr, etc.) between execution cycles.
- **Patch Streaming**: Only the changes are sent to the client, significantly reducing bandwidth for dynamic applications.

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

## Hyperlight Source Language (HLS)

HLS is a human-readable language designed to define web interfaces as executable programs. It compiles to **Hyperlight Binary (HLB)**, which agents can execute directly in the Virtual DOM runtime.

### Example HLS

```hls
element App {
  element Header {
    text "Welcome to Hyperlight"
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
```

### HLB Instructions

The HLS compiler generates these instructions:

- **`DefineElement`**: Creates a new element with a unique ID and tag.
- **`SetAttribute`**: Sets properties (e.g., text, class, style) on an element.
- **`AddChild`**: Establishes parent-child relationships.
- **`EmitEvent`**: Triggers events that agents can subscribe to.
- **`StreamLatent`**: Streams high-dimensional vectors for embeddings or other representations.

## Intelligence Layer Features

Hyperlight includes advanced features for high-security and low-latency agentic communication:

- **Chameleon Protocol**: A latent-space cryptographic system that evolves the protocol's "shape" per-message using neural encoders.
- **Speculative Decoding**: Bidirectional message prediction using transformer models to reduce perceived latency by pre-computing responses.
- **Quantum-Resistant Keys**: Lattice-based key evolution that resists quantum computing attacks.
- **Neural Latent Encoding**: Variational Autoencoders (VAE) combined with LSTM and Attention mechanisms to project web content into high-dimensional latent spaces.

## Intelligence Layer

Hyperlight features a deep intelligence layer that optimizes for both performance and security in AI-to-AI communication.

### Speculative Decoding

Inspired by LLM inference techniques, Hyperlight uses a **Transformer-based predictor** to anticipate the next likely messages in a protocol stream.

- **Zero-Bandwidth Hits**: If a prediction is correct, the receiver reconstructs the message from its local cache, sending only a tiny confirmation hash.
- **Latency Reduction**: The core engine can pre-compute responses for predicted requests before they even arrive.
- **Pattern Obfuscation**: Speculative traffic makes the protocol stream appear as high-entropy noise to external observers.

### Chameleon Protocol (Moving-Target Defense)

The Chameleon Protocol uses **Neural Latent Encoding** to hide communication patterns.

- **Latent Morphing**: Messages are projected into a high-dimensional latent space using a Variational Autoencoder (VAE).
- **Dynamic Evolution**: The transformation matrices evolve over time based on quantum-resistant seeds, ensuring that the "language" of the protocol is constantly changing.
- **Implicit Encryption**: The latent space projection itself acts as a form of encryption where the model weights and temporal state (LSTM) are the keys.

## Getting Started

### Prerequisites

- Rust (latest stable, 2021 edition)
- Windows, Linux, or macOS

### Building

```bash
cargo build --release
```

### Running the Core

Start the Hyperlight browser engine:

```bash
cargo run -p hyperlight-core
```

The core will listen on `127.0.0.1:8080` for agent and browser connections.

### Running the GUI Browser (Human Mode)

In a separate terminal, launch the cross-platform GUI:

```bash
cargo run -p hyperlight-browser
```

The GUI allows you to:
- Enter URLs and navigate the web via the Hyperlight stack.
- Toggle **Human Mode** to transpile legacy HTML into AI-native HLS.
- View raw Unified Representations and agentic data streams.

### Running an Example Agent

In a separate terminal:

```bash
cargo run --example simple_agent
```

The example agent will:
1. Connect to the core with encryption enabled
2. Navigate to `https://example.com`
3. Fetch the Unified Representation
4. Compile and execute an HLS program

### Example Output

```
Connected to Hyperlight Core
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

Hyperlight introduces the **Chameleon Protocol**, a revolutionary approach to secure communication that treats latent-space representations as a form of implicit encryption.

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

Inspired by LLM speculative decoding, Hyperlight predicts messages before they arrive and sends minimal confirmations when predictions match.

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

For compatibility, Hyperlight also supports **AES-256-GCM** encryption:

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

Hyperlight eliminates traditional browser rendering pipelines (DOM → Layout → Paint) and replaces them with a multi-layered stack optimized for both AI and humans.

### The Hyperlight Stack

1.  **User Layer**: `hyperlight-browser` (GUI) provides a human-friendly interface.
2.  **Compatibility Layer**: `hyperlight-human` transpiles legacy web content into AI-native formats.
3.  **Agent Layer**: `hyperlight-agent` (SDK) enables autonomous interaction.
4.  **Core Layer**: `hyperlight-core` orchestrates sessions and fetches content.
5.  **Execution Layer**: `hyperlight-compiler` and `hyperlight-wasm` run websites as programs.
6.  **Intelligence Layer**: `hyperlight-neural` and `hyperlight-crypto` provide secure, predictive communication.
7.  **Infrastructure Layer**: `hyperlight-cluster` enables distributed scaling.

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
│   Speculative Decoding Layer        │
│   ├─ N-gram Prediction              │
│   ├─ Markov Chain Transitions       │
│   ├─ Pre-computed Response Cache    │
│   └─ Confirmation/Delta Encoding    │
├─────────────────────────────────────┤
│   Chameleon Layer                   │
│   ├─ Latent-Space Encoding          │
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
│   TCP (Port 8080)                   │
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
- [x] **Speculative Decoding** (prediction-accelerated communication)
- [x] **Bi-directional Speculation** (input + output prediction)
- [x] **Advanced HLS Syntax** (variables, state, conditionals, loops, expressions)
- [x] **Virtual DOM Runtime** (binary execution with UR generation)
- [x] **WebAssembly Runtime** (HLB → WASM near-native execution)
- [x] **Distributed Agent Coordination** (cluster, load balancing, session affinity)
- [x] **Neural Latent Encoder** (VAE, LSTM, Attention for learned projections)
- [x] **Transformer Message Predictor** (autoregressive byte-level prediction)
- [x] **Quantum-Resistant Key Evolution** (RLWE lattice cryptography)
- [x] **Human Compatibility Layer** (HTML/CSS/JS → HLS transpilation)
- [x] **Cross-Platform GUI Browser** (egui-based human interface)

## Contributing

Hyperlight is an experimental research project. Contributions are welcome, especially in:
- HLS language design
- Binary execution optimization
- Protocol efficiency improvements

## License

This project is open-source and available under the MIT License.
