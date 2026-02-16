# Quick Start

## 1. Start the Server

```bash
# Using the CLI
cargo run -p spine-cli -- deploy

# Or directly
cargo run -p spine-core
```

The server listens on `127.0.0.1:8080` by default.

## 2. Connect an Agent

Create a new project:

```bash
cargo run -p spine-cli -- init my-agent
cd my-agent
```

Or add SPINE to an existing project:

```toml
[dependencies]
spine-agent = { git = "https://github.com/nervosys/SPINE" }
tokio = { version = "1", features = ["full"] }
anyhow = "1.0"
```

## 3. Write Your First Agent

```rust
use spine_agent::AgentClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    // Navigate to a page
    client.navigate("https://example.com").await?;

    // Get structured content
    let ur = client.get_ur().await?;
    println!("Page: {}", ur.title);
    println!("Elements: {}", ur.elements.len());

    // Execute HLS script
    let result = client.execute_hls("let x = 42 * 2; x").await?;
    println!("Computed: {:?}", result);

    // Measure latency
    let ms = client.ping().await?;
    println!("Latency: {}ms", ms);

    Ok(())
}
```

## 4. Run It

```bash
cargo run
```

## Next Steps

- [Configuration](./configuration.md) — Customize server settings
- [Navigation & URs](../sdk/navigation.md) — Deep dive into web extraction
- [HLS Scripting](../sdk/hls.md) — Server-side scripting language
- [Swarm Intelligence](../sdk/swarm.md) — Multi-agent coordination
