# Crate Map

## Workspace Crates (19 total)

### User-Facing

| Crate           | Type   | Description                                                                  |
| --------------- | ------ | ---------------------------------------------------------------------------- |
| `spine-cli`     | Binary | CLI tool (`spine` command) — init, connect, query, deploy, benchmark, status |
| `spine-gateway` | Binary | OpenAPI REST gateway with Swagger UI                                         |
| `spine-browser` | Binary | Cross-platform GUI browser (egui)                                            |

### SDK

| Crate         | Type    | Description                                            |
| ------------- | ------- | ------------------------------------------------------ |
| `spine-agent` | Library | High-level agent SDK with TCP/TLS/WebSocket transports |

### Intelligence

| Crate           | Type    | Description                                                                |
| --------------- | ------- | -------------------------------------------------------------------------- |
| `spine-agentic` | Library | Swarm intelligence, neural protocols, social cognition, protocol evolution |

### Core

| Crate             | Type       | Description                                                                     |
| ----------------- | ---------- | ------------------------------------------------------------------------------- |
| `spine-core`      | Binary+Lib | Multi-session orchestration server with config management                       |
| `spine-protocol`  | Library    | Message types, frame encoding, protocol handler, speculation                    |
| `spine-parser`    | Library    | Recursive semantic HTML → Unified Representation parser                         |
| `spine-compiler`  | Library    | HLS (Hyperlight Scripting) → SpineBinary compiler                               |
| `spine-knowledge` | Library    | Unified bioinspired memory (episodic, semantic, working, collective) with CRDTs |

### Infrastructure

| Crate             | Type    | Description                                                            |
| ----------------- | ------- | ---------------------------------------------------------------------- |
| `spine-transport` | Library | Zero-copy I/O, BBR congestion control, WebSocket bridge, plugin system |
| `spine-stream`    | Library | Reactive streams, multiplexing, flow control, priority queuing         |
| `spine-crypto`    | Library | Titans prediction, quantum cryptography, X3DH, MIRAS memory            |
| `spine-neural`    | Library | VAE encoder, Titans memory, attention, learned projections             |
| `spine-wasm`      | Library | WebAssembly runtime (wasmtime) for HLS execution                       |
| `spine-human`     | Library | Legacy web bridge — realistic mouse/keyboard simulation                |
| `spine-recursive` | Library | Recursive Language Model for infinite context (10M+ chars)             |

### Primitives

| Crate          | Type    | Description                                                                |
| -------------- | ------- | -------------------------------------------------------------------------- |
| `spine-kernel` | Library | SIMD intrinsics, custom allocators, lock-free atomics, ring buffers, RDTSC |

## Dependency Graph (simplified)

```
spine-cli ──────┐
spine-gateway ──┤
spine-browser ──┼── spine-agent ── spine-agentic ──┬── spine-protocol
                │                                   ├── spine-neural
                │                                   ├── spine-crypto
                │                                   └── spine-knowledge
                │
                └── spine-core ── spine-protocol ──┬── spine-parser
                                                    ├── spine-compiler
                                                    ├── spine-transport ── spine-kernel
                                                    ├── spine-stream
                                                    └── spine-wasm
```
