# Crate Map

The repository has two Cargo workspaces. The root workspace (`src/*`) holds the
19 crates that make up the stack. The `research/` workspace holds experimental
crates that are not built with the root workspace and are not on the native wire.

## Root Workspace (19 crates)

### Interfaces

| Crate           | Type   | Description                                                                  |
| --------------- | ------ | ---------------------------------------------------------------------------- |
| `spine-cli`     | Binary | CLI tool (`spine` command): mcp, task, name, init, connect, query, deploy, benchmark, status, cert |
| `spine-gateway` | Binary | OpenAPI REST gateway with Swagger UI                                         |
| `spine-grpc`    | Library | gRPC/protobuf bridge exposing tool calls, capabilities and streaming         |
| `spine-web`     | Library | Umbrella facade; re-exports the component crates behind Cargo features       |

### SDK

| Crate         | Type    | Description                                                         |
| ------------- | ------- | ------------------------------------------------------------------- |
| `spine-agent` | Library | Agent SDK (`AgentClient`) with TCP, TLS and WebSocket connections   |

### Core

| Crate             | Type       | Description                                                                     |
| ----------------- | ---------- | ------------------------------------------------------------------------------- |
| `spine-core`      | Binary+Lib | The daemon: sessions, command dispatch, configuration, outbound fetch policy    |
| `spine-protocol`  | Library    | spine-wire v1: frames, deterministic CBOR, `HELLO`/`HELLO_ACK`, `Connection`, command and error types |
| `spine-cbor`      | Library    | Deterministic CBOR (RFC 8949 section 4.2): `to_vec`, `from_slice`, `check_deterministic`; re-exported as `spine_protocol::cbor` |
| `spine-parser`    | Library    | Semantic HTML to Unified Representation parser, structured extraction           |
| `spine-compiler`  | Library    | HLS (SPINE Scripting) to SpineBinary compiler                                   |
| `spine-wasm`      | Library    | WebAssembly runtime (wasmtime) for HLS execution                                |
| `spine-name`      | Library    | `spine://` URIs, self-certifying names, signed name records, DHT keyspace, resolver |
| `spine-mesh`      | Library    | Agent mesh: Ed25519 identities, signed envelopes, name-record RPC, TCP/WebSocket/QUIC transports |
| `spine-session`   | Library    | Authenticated, forward-secret mesh session handshake (ML-KEM-768, Ed25519, AES-256-GCM). Custom cryptography without external review |

### Infrastructure

| Crate             | Type    | Description                                                             |
| ----------------- | ------- | ----------------------------------------------------------------------- |
| `spine-transport` | Library | Zero-copy I/O, BBR congestion control, WebSocket bridge, plugin pipeline |
| `spine-stream`    | Library | Reactive streams, multiplexing, flow control, priority queuing          |
| `spine-storage`   | Library | Persistent storage backends (in-memory, SQLite, RocksDB)                |
| `spine-cache`     | Library | Tiered caching (L1 LRU, L2 file-backed, L3 remote trait)                |

### Bindings

| Crate          | Type    | Description                                                              |
| -------------- | ------- | ------------------------------------------------------------------------ |
| `spine-ffi`    | Library | C ABI (cdylib/staticlib), 15 functions in `spine.h`, wraps `AgentClient` over plain TCP |
| `spine-go`     | Go pkg  | Go bindings over `spine-ffi` with cgo; does not speak the wire itself    |
| `spine-python` | Library | Python bindings (PyO3 + maturin), outside both workspaces; wraps `AgentClient` over plain TCP |
| `spine-js`     | Library | wasm-bindgen bindings, outside both workspaces; parses HTML and compiles HLS offline, no network |

## Research Workspace (`research/`)

Experimental and unreviewed. Build with `cd research && cargo build`. None of
this is on the native wire.

| Crate                     | Description                                                              |
| ------------------------- | ------------------------------------------------------------------------ |
| `spine-protocol-research` | Neural codecs, Chameleon key and latent AEAD, Titans latent codec, protocol evolution |
| `spine-agentic`           | Swarm intelligence, social cognition, workflows, tracing and replay      |
| `spine-knowledge`         | Bioinspired memory (episodic, semantic, working, collective) with CRDTs  |
| `spine-crypto`, `spine-neural`, `spine-recursive` | Transformer prediction and key-evolution, neural latent encoder, recursive long-context model |
| `spine-cluster`, `spine-human` | Cluster node and human-input transpiler (behind `spine-core`'s `research` feature) |
| `spine-kernel`, `spine-nostd`, `spine-embedded`, `spine-gpu`, `spine-k8s` | Low-level primitives, embedded runtime, GPU compute and Kubernetes operator |
| `spine-browser`           | egui GUI browser                                                         |
| `spine-mechgen`           | MechGen ABL bridge: deserializes ABL envelopes into `spine-agentic` types |

`spine-agent` has a `research` feature, but only to pull in `spine-agentic` for
its examples; the SDK has no research-gated code. The `spine-protocol` crate has
no `research` feature.

## Dependency Graph (simplified)

```
spine-cli, spine-gateway, spine-ffi -- spine-agent --+-- spine-protocol
                                                     +-- spine-parser, spine-compiler, spine-wasm
                                                     +-- spine-name
                                                     +-- spine-transport

spine-core -- spine-protocol, spine-parser, spine-wasm, spine-name,
              spine-mesh, spine-storage, spine-transport, spine-agent

spine-mesh -- spine-session, spine-name, spine-transport, spine-protocol
```
