# Architecture Overview

SPINE is a headless semantic browser with adaptive encryption. It is organized as a Rust workspace of 19 crates, each with a focused responsibility.

## Design Principles

1. **Semantic-first** — All content is parsed into Unified Representations, not raw HTML
2. **Agent-native** — Built for programmatic access, not human browsing
3. **Secure by default** — Layered encryption from TLS to latent-space cryptography
4. **Bioinspired** — Memory, learning, and coordination modeled on biological systems
5. **Zero-copy where possible** — Kernel primitives and transport avoid unnecessary allocations

## System Layers

```
┌─────────────────────────────────────────────────────────────┐
│                      User Interfaces                         │
│   spine-cli · spine-gateway · spine-browser                  │
├─────────────────────────────────────────────────────────────┤
│                      Agent SDK                               │
│   spine-agent (connect, navigate, search, execute, swarm)    │
├─────────────────────────────────────────────────────────────┤
│                   Agentic Intelligence                       │
│   spine-agentic (neural protocols, swarm, social cognition)  │
├──────────┬──────────┬──────────┬──────────┬─────────────────┤
│ Protocol │ Parser   │ Compiler │ Knowledge│ Core             │
│ messages │ HTML→UR  │ HLS→WASM │ CRDT mem │ server+config    │
├──────────┴──────────┴──────────┴──────────┴─────────────────┤
│                    Infrastructure                            │
│  transport · stream · crypto · neural · wasm · human         │
├─────────────────────────────────────────────────────────────┤
│                    spine-kernel                               │
│  SIMD · allocators · atomics · ring buffers · RDTSC          │
└─────────────────────────────────────────────────────────────┘
```

## Data Flow

1. **Agent connects** via TCP, TLS, or WebSocket to spine-core server
2. **Requests** are serialized as `Message` enum through `ProtocolHandler`
3. **Server** fetches web content, parses to `UnifiedRepresentation`
4. **Response** flows back through optional compression, encryption, speculation
5. **Agent** receives structured data for processing

## Security Layers

| Layer        | Component       | Purpose                     |
| ------------ | --------------- | --------------------------- |
| Transport    | TLS 1.3         | Channel encryption          |
| Protocol     | Chameleon keys  | Moving-target defense       |
| Latent       | Neural encoding | Latent-space cryptography   |
| Post-quantum | RLWE lattice    | Future-proof key exchange   |
| Identity     | X3DH            | Initial trust establishment |
| Anti-Sybil   | Stake + PoW     | Decentralized trust         |
