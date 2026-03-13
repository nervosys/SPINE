# Crate Map

## Workspace Crates (26 total)

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
| `spine-parser`    | Library    | Recursive semantic HTML to Unified Representation parser                        |
| `spine-compiler`  | Library    | HLS (SPINE Scripting) to SpineBinary compiler                                   |
| `spine-knowledge` | Library    | Unified bioinspired memory (episodic, semantic, working, collective) with CRDTs |

### Infrastructure

| Crate             | Type    | Description                                                             |
| ----------------- | ------- | ----------------------------------------------------------------------- |
| `spine-transport` | Library | Zero-copy I/O, BBR congestion control, WebSocket bridge, plugin system  |
| `spine-stream`    | Library | Reactive streams, multiplexing, flow control, priority queuing          |
| `spine-crypto`    | Library | Titans prediction, quantum cryptography, X3DH, MIRAS memory             |
| `spine-neural`    | Library | VAE encoder, Titans memory, attention, learned projections              |
| `spine-cluster`   | Library | Distributed coordination, Raft consensus, Sybil resistance, marketplace |
| `spine-wasm`      | Library | WebAssembly runtime (wasmtime) for HLS execution                        |
| `spine-human`     | Library | Legacy web bridge — realistic mouse/keyboard simulation                 |
| `spine-recursive` | Library | Recursive Language Model for infinite context (10M+ chars)              |
| `spine-gpu`       | Library | GPU compute abstraction (CPU SIMD fallback, wgpu Vulkan/Metal/DX12)     |
| `spine-storage`   | Library | Persistent storage (InMemory, SQLite WAL, RocksDB LSM)                  |
| `spine-cache`     | Library | Tiered caching (L1 LRU, L2 file-backed, L3 remote)                      |
| `spine-k8s`       | Library | Kubernetes operator CRD, autoscaler, manifest generators                |

### Primitives

| Crate            | Type    | Description                                                                |
| ---------------- | ------- | -------------------------------------------------------------------------- |
| `spine-kernel`   | Library | SIMD intrinsics, custom allocators, lock-free atomics, ring buffers, RDTSC |
| `spine-nostd`    | Library | `#![no_std]` core primitives (Q8.8 fixed-point, FNV hashing, frame codec)  |
| `spine-embedded` | Library | Minimal agent runtime for embedded/IoT targets                             |

### Bindings

| Crate          | Type    | Description                                            |
| -------------- | ------- | ------------------------------------------------------ |
| `spine-ffi`    | Library | C FFI bindings (cdylib/staticlib) for language interop |
| `spine-python` | Library | Python bindings via PyO3 + maturin (excluded)          |
| `spine-js`     | Library | TypeScript/WASM bindings via wasm-bindgen (excluded)   |
| `spine-go`     | Go pkg  | Go bindings via cgo + spine-ffi (non-Rust)             |

## Dependency Graph (simplified)

```
spine-cli ------+
spine-gateway --+
spine-browser --+-- spine-agent -- spine-agentic --+-- spine-protocol
                |                                   +-- spine-neural
                |                                   +-- spine-crypto
                |                                   +-- spine-knowledge -- spine-storage
                |                                                          spine-cache
                |
                +-- spine-core -- spine-protocol --+-- spine-parser
                                                    +-- spine-compiler
                                                    +-- spine-transport -- spine-kernel
                                                    +-- spine-stream
                                                    +-- spine-cluster (Raft, Sybil)
                                                    +-- spine-wasm
                                                    +-- spine-gpu
```