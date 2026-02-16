//! # Encrypted Agent
//!
//! Demonstrates connecting over TLS and enabling Chameleon Protocol
//! for latent-space cryptography with protocol morphing.

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

    // Enable Chameleon Protocol — latent-space encryption with moving target defense
    let secret: [u8; 32] = *b"MySecretKeyForChameleonProtocol!";
    client.handler.enable_chameleon(secret);
    println!("Chameleon Protocol enabled");

    // Enable speculative decoding for bandwidth savings
    client.handler.enable_speculation(true, true);
    println!("Speculative Decoding enabled");

    // Navigate — each request morphs the protocol
    client.navigate("https://example.com").await?;
    println!("Navigated (protocol morphed)");

    let ur = client.get_ur().await?;
    println!("Title: {} ({} elements)", ur.title, ur.elements.len());

    // Morph explicitly
    client.morph().await?;
    println!("Protocol morphed again");

    // Check speculation stats
    let stats = client.get_speculation_stats();
    println!(
        "Predictions: {}, Hits: {}, Saved: {} bytes",
        stats.output_predictions, stats.output_hits, stats.bytes_saved
    );

    Ok(())
}
