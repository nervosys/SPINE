# Architecture Overview

SPINE is a headless semantic browser and agent web stack. The root Cargo workspace has 18 crates, each with a focused responsibility; experimental crates live in a separate `research/` workspace (see [Crate Map](./crates.md)).

## Design Principles

1. **Semantic-first** — All content is parsed into Unified Representations, not raw HTML
2. **Agent-native** — Built for programmatic access, not human browsing
3. **One wire** — A single versioned binary protocol (spine-wire v1) with deterministic encoding, so every value has exactly one byte representation
4. **Honest security claims** — Native-path confidentiality comes from TLS; the mesh has its own signed and encrypted handshake; research code is labelled as such
5. **Zero-copy where possible** — Transport avoids unnecessary allocations

## System Layers

```
┌─────────────────────────────────────────────────────────────┐
│                      User Interfaces                         │
│   spine-cli · spine-gateway · spine-grpc                     │
├─────────────────────────────────────────────────────────────┤
│                      Agent SDK                               │
│   spine-agent (connect, navigate, execute, knowledge, names) │
├──────────┬──────────┬──────────┬──────────┬─────────────────┤
│ Protocol │ Parser   │ Compiler │ Name     │ Core             │
│ wire v1  │ HTML→UR  │ HLS→WASM │ + Mesh   │ daemon + config  │
├──────────┴──────────┴──────────┴──────────┴─────────────────┤
│                    Infrastructure                            │
│  transport · stream · session · storage · cache · wasm       │
└─────────────────────────────────────────────────────────────┘
```

## Data Flow

1. **Agent connects** via TCP, TLS, or WebSocket to the `spine-core` daemon and exchanges `HELLO` / `HELLO_ACK`
2. **Commands** travel as `REQUEST` frames whose body is a `BrowserCommand` in deterministic CBOR, handled by `spine_protocol::connection::Connection`
3. **Daemon** fetches web content under its outbound policy and parses it to a `UnifiedRepresentation`
4. **Reply** is a `RESPONSE` frame with the same request id, whose body is `{ok: result}` or `{err: {code, message}}`, optionally zstd-compressed
5. **Agent** receives structured data for processing

## Security Layers

| Layer     | Component                                  | Purpose                                              |
| --------- | ------------------------------------------ | ---------------------------------------------------- |
| Transport | TLS, optional mutual TLS                   | Confidentiality and authentication on the native wire |
| Wire      | spine-wire v1 framing and strict CBOR      | Bounded frames, one encoding per value; no encryption |
| Mesh      | `spine-session` handshake, signed envelopes | Node-to-node authentication and encryption (a separate protocol) |
| Identity  | Ed25519 keys and signed name records       | Self-certifying names and authorship                  |

There is no SPINE-level encryption, obfuscation, padding or traffic-analysis
resistance on the native wire. See [Security](./security.md).
