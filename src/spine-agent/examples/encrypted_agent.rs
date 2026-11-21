//! # Encrypted Agent
//!
//! Demonstrates connecting over TLS (server-authenticated, with an optional
//! client certificate). Confidentiality on the native wire comes from TLS.

use anyhow::Result;
use spine_agent::AgentClient;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    // Connect with TLS for transport-level encryption
    let mut client = AgentClient::connect_tls(
        "127.0.0.1:8443",
        "localhost",
        Some(std::path::Path::new("certs/ca.pem")),
        None, // No client certificate
    )
    .await?;

    println!("Connected with TLS");

    client.navigate("https://example.com").await?;
    println!("Navigated");

    let ur = client.get_ur().await?;
    println!("Title: {} ({} elements)", ur.title, ur.elements.len());

    Ok(())
}
