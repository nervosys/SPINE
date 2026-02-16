//! # Latent Vector Analysis
//!
//! Demonstrates extracting latent (neural embedding) representations
//! of web pages and computing semantic similarity between them.

use anyhow::Result;
use spine_agent::AgentClient;
use spine_agentic::ProtocolDomain;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    let urls = vec!["https://example.com", "https://httpbin.org/html"];

    let mut vectors: Vec<(String, Vec<f32>)> = Vec::new();

    for url in &urls {
        println!("Encoding {}...", url);
        client.navigate(url).await?;

        // Get 256-dimensional latent representation
        let vec = client.get_latent_ur(256).await?;
        println!(
            "  Vector: [{:.4}, {:.4}, {:.4}, ...] (dim={})",
            vec[0],
            vec[1],
            vec[2],
            vec.len()
        );
        vectors.push((url.to_string(), vec));
    }

    // Compute pairwise cosine similarity
    println!("\n=== Pairwise Similarity ===");
    for i in 0..vectors.len() {
        for j in (i + 1)..vectors.len() {
            let sim = cosine_similarity(&vectors[i].1, &vectors[j].1);
            println!("  {} <-> {}: {:.4}", vectors[i].0, vectors[j].0, sim);
        }
    }

    // Neural transmission
    println!("\n=== Neural Transmission ===");
    let data = b"semantic payload for neural encoding";
    let domain = ProtocolDomain::Text;
    match client.transmit_neural(data, domain).await {
        Ok(result) => println!("  Transmitted: {:?}", result),
        Err(e) => println!("  (Server-side neural transmission not available: {})", e),
    }

    Ok(())
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom > 1e-10 {
        dot / denom
    } else {
        0.0
    }
}
