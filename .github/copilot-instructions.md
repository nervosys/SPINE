<!-- Use this file to provide workspace-specific custom instructions to Copilot. For more details, visit https://code.visualstudio.com/docs/copilot/copilot-customization#_use-a-githubcopilotinstructionsmd-file -->

## Project Status: COMPLETE ✅

### Core Features
- [x] Verify that the copilot-instructions.md file in the .github directory is created.
- [x] Clarify Project Requirements: Rust-based agentic AI web browser named Hyperlight.
- [x] Scaffold the Project: Initialized Rust workspace with core, parser, protocol, and agent crates.
- [x] Customize the Project: Implemented unified representation parser and low-latency protocol.
- [x] Install Required Extensions
- [x] Compile the Project
- [x] Create and Run Task
- [x] Launch the Project
- [x] Ensure Documentation is Complete
- [x] Enhance Web Fetching and Parsing: Added reqwest and improved UR extraction.
- [x] Implement Browser Actions: Added Navigate and GetUR commands.
- [x] Expand Agent API: Created high-level AgentClient API and example.

### Advanced Features
- [x] HLS Compiler: Full language with variables, state, conditionals, loops, expressions
- [x] Virtual DOM Runtime: HLB execution with UR generation
- [x] Chameleon Protocol: Latent-space cryptography with moving-target defense
- [x] Speculative Decoding: Bidirectional message prediction
- [x] WebAssembly Runtime: HLB → WASM near-native execution (hyperlight-wasm)
- [x] Distributed Clustering: Load balancing, session affinity, leader election (hyperlight-cluster)
- [x] Neural Latent Encoder: VAE, LSTM, Attention for learned projections (hyperlight-neural)
- [x] Transformer Predictor: Autoregressive byte-level message prediction (hyperlight-crypto)
- [x] Quantum-Resistant Keys: RLWE lattice cryptography (hyperlight-crypto)

### Workspace Structure (9 crates)
- `hyperlight-core`: Multi-session orchestration engine
- `hyperlight-parser`: Recursive semantic HTML parser
- `hyperlight-protocol`: TCP protocol with encryption/compression
- `hyperlight-agent`: High-level SDK for AI agents
- `hyperlight-compiler`: HLS → HLB compiler
- `hyperlight-wasm`: WebAssembly runtime
- `hyperlight-cluster`: Distributed coordination
- `hyperlight-neural`: Neural network-based encoding
- `hyperlight-crypto`: Transformer prediction + quantum cryptography
