//! # WebSocket Agent
//!
//! Demonstrates connecting via WebSocket transport instead of raw TCP.
//! Useful when SPINE is deployed behind a reverse proxy (nginx, Caddy)
//! or when clients are in browser-adjacent environments.

use anyhow::Result;
use spine_agent::AgentClient;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    // Connect via WebSocket — server must have WS listener enabled
    // Default WS port = TCP port + 1 (e.g., 8081 if TCP is 8080)
    let mut client = AgentClient::connect_ws("ws://127.0.0.1:8081").await?;
    println!("Connected via WebSocket");

    // All the same operations work over WebSocket
    let latency = client.ping().await?;
    println!("Ping: {}ms", latency);

    client.navigate("https://example.com").await?;
    let ur = client.get_ur().await?;
    println!("Page: {} ({} elements)", ur.title, ur.elements.len());

    // Fetch raw HTML
    let html = client.get_raw_html().await?;
    println!("HTML length: {} bytes", html.len());

    Ok(())
}
