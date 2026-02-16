# SPINE Documentation

**SPINE** is a headless semantic browser with adaptive encryption — an efficient tool for AI agents to extract meaning, communicate securely, and coordinate in swarms.

## What is SPINE?

SPINE provides a complete agentic web stack:

- **Headless semantic browsing** — Navigate web pages and extract structured Unified Representations instead of raw HTML
- **Adaptive encryption** — X3DH key exchange, Chameleon latent-space cryptography, quantum-resistant RLWE
- **Swarm intelligence** — Multi-agent coordination with capability discovery and task delegation
- **Neural protocols** — Learned message encoding with Titans architecture for efficient communication
- **HLS scripting** — Domain-specific language compiled to WebAssembly for server-side execution
- **Knowledge base** — Distributed CRDT-based memory with episodic, semantic, working, and collective subsystems

## Architecture at a Glance

```
┌─────────────────────────────────────────────────┐
│                  spine-agent                     │  High-level SDK
├─────────────┬─────────────┬─────────────────────┤
│ spine-cli   │ spine-gateway│ spine-browser       │  Interfaces
├─────────────┴─────────────┴─────────────────────┤
│                 spine-agentic                    │  Swarm / Neural
├──────────┬──────────┬───────────┬───────────────┤
│ protocol │ parser   │ compiler  │ knowledge     │  Core
├──────────┴──────────┴───────────┴───────────────┤
│  transport │ stream │ crypto │ neural │ wasm    │  Infrastructure
├─────────────────────────────────────────────────┤
│                  spine-kernel                    │  Hardware primitives
└─────────────────────────────────────────────────┘
```

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

    // Search
    let results = client.search("rust programming").await?;
    println!("Results: {}", results);

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
