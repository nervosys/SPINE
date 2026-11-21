# SPINE Documentation

**SPINE** is a headless semantic browser and agent web stack: a daemon that fetches pages and returns structured Unified Representations, a binary wire protocol (spine-wire v1) to talk to it, and a signed mesh and naming layer for agents to find each other.

## What is SPINE?

SPINE provides an agentic web stack:

- **Headless semantic browsing** — Navigate web pages and extract structured Unified Representations instead of raw HTML
- **Native wire** — spine-wire v1: length-prefixed binary frames, deterministic CBOR bodies, a `HELLO`/`HELLO_ACK` handshake and a closed set of 19 commands (see [Protocol](./architecture/protocol.md))
- **Confidentiality** — On the native client wire, confidentiality and authentication come from TLS (optionally mutual TLS) and nothing else. The mesh path between nodes is a different protocol with its own ML-KEM-768 and Ed25519 handshake (`spine-session`)
- **Naming and mesh** — `spine://` self-certifying names, signed name records, and an Ed25519-signed agent mesh
- **HLS scripting** — Domain-specific language compiled to WebAssembly for server-side execution
- **Knowledge store** — Per-session key/value entries with tags, through `store_knowledge` and `query_knowledge`
- **Research (not shipped)** — Swarm intelligence, neural codecs, latent-space encoding, the memory subsystems and other experiments live in the separate `research/` workspace and are not on the native wire

## Architecture at a Glance

```
┌───────────────────────────────────────────────────────┐
│ spine-cli · spine-gateway · spine-agent (SDK)          │  Interfaces
├───────────────────────────────────────────────────────┤
│ spine-core (daemon)                                    │  Sessions, commands
├───────────┬───────────┬───────────┬───────────────────┤
│ protocol  │ parser    │ compiler  │ name · mesh       │  Core
├───────────┴───────────┴───────────┴───────────────────┤
│ transport · stream · session · storage · cache · wasm  │  Infrastructure
└───────────────────────────────────────────────────────┘
```

The experimental crates in `research/` are a separate Cargo workspace and are not part of this stack.

## Quick Example

```rust
use spine_agent::AgentClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    // Navigate and extract meaning
    client.navigate("https://example.com").await?;
    let ur = client.get_ur().await?;
    println!("Title: {}", ur.title);

    Ok(())
}
```

## Sections

- **[Getting Started](./getting-started/installation.md)** — Install, configure, and run your first agent
- **[Architecture](./architecture/overview.md)** — System design and crate organization
- **[Agent SDK](./sdk/connecting.md)** — Build agents with the Rust SDK
- **[CLI Reference](./cli/commands.md)** — Command-line interface guide
- **[Gateway API](./gateway/rest.md)** — REST API for non-Rust clients
- **[Contributing](./contributing/setup.md)** — Development setup and testing
