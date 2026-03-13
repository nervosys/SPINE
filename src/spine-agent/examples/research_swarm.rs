//! # SPINE Demo: Collaborative Web Research Swarm
//!
//! A self-contained demonstration of SPINE's agent capabilities:
//! 1. Starts a local spine-core server
//! 2. Connects multiple agents over TCP
//! 3. Agents collaboratively research a topic across multiple URLs
//! 4. Results aggregated via knowledge base
//! 5. Demonstrates Chameleon protocol, speculation, and HLS execution
//!
//! ```
//! # Requires a running spine-core server:
//! # cargo run -p spine-core &
//! # Then run the demo:
//! cargo run --example research_swarm
//! ```

use anyhow::Result;
use serde_json::json;
use spine_agent::{AgentClient, Compiler};
use std::time::Instant;
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:8080";

/// A research agent that scrapes a URL and stores findings.
async fn research_agent(
    agent_id: usize,
    url: &str,
    topic: &str,
) -> Result<serde_json::Value> {
    let mut client = AgentClient::<TcpStream>::connect(SERVER_ADDR).await?;

    // Enable Chameleon Protocol for stealth browsing
    let secret: [u8; 32] = [0x53, 0x50, 0x49, 0x4E, 0x45, 0x2D, 0x52, 0x65,
                             0x73, 0x65, 0x61, 0x72, 0x63, 0x68, 0x2D, 0x44,
                             0x65, 0x6D, 0x6F, 0x2D, 0x53, 0x65, 0x63, 0x72,
                             0x65, 0x74, 0x4B, 0x65, 0x79, 0x21, 0x21, 0x21];
    client.handler.enable_chameleon_aead(secret);
    client.handler.enable_speculation(true, true);

    println!("  [Agent {}] Navigating to {}...", agent_id, url);
    let start = Instant::now();
    client.navigate(url).await?;
    let nav_time = start.elapsed();

    let ur = client.get_ur().await?;
    println!(
        "  [Agent {}] Fetched UR: \"{}\" ({} elements, {:.0}ms)",
        agent_id,
        ur.title,
        ur.elements.len(),
        nav_time.as_secs_f64() * 1000.0
    );

    // Store findings in knowledge base
    let key = format!("research:{}:{}", topic, agent_id);
    let findings = json!({
        "agent_id": agent_id,
        "url": url,
        "title": ur.title,
        "element_count": ur.elements.len(),
        "topic": topic,
    });

    client
        .store_knowledge(
            &key,
            findings.clone(),
            vec![topic.into(), "research".into(), format!("agent-{}", agent_id)],
        )
        .await?;

    // Get speculation stats
    let stats = client.get_speculation_stats();
    println!(
        "  [Agent {}] Speculation: {:.1}% output accuracy, {} bytes saved",
        agent_id,
        stats.output_accuracy() * 100.0,
        stats.bytes_saved
    );

    Ok(findings)
}

/// Aggregator agent that queries knowledge base and generates summary.
async fn aggregator_agent(topic: &str, expected_results: usize) -> Result<()> {
    let mut client = AgentClient::<TcpStream>::connect(SERVER_ADDR).await?;

    println!("\n  [Aggregator] Querying knowledge base for topic: {}", topic);
    let results = client
        .query_knowledge(topic, vec!["research".into()], expected_results)
        .await?;

    println!("  [Aggregator] Found {} research results:", results.len());
    for (i, result) in results.iter().enumerate() {
        if let Some(title) = result.get("title").and_then(|v| v.as_str()) {
            let url = result.get("url").and_then(|v| v.as_str()).unwrap_or("?");
            println!("    {}. \"{}\" — {}", i + 1, title, url);
        }
    }

    // Execute HLS summary template
    let hls = r#"
        let topic = "Research Summary"
        let count = 3

        element Summary {
            element Title {
                text "SPINE Research: " ++ topic
            }
            element Stats {
                text "Agents deployed: " ++ str(count)
                text "Topic: collaborative web research"
            }
        }
    "#;

    println!("\n  [Aggregator] Compiling HLS summary template...");
    let binary = Compiler::compile(hls)?;
    let result = client.execute_binary(binary).await?;
    println!("  [Aggregator] HLS execution result: {}", result);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║   SPINE Demo: Collaborative Web Research Swarm               ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Check if server is running
    println!("[1/4] Verifying server connectivity...");
    match AgentClient::<TcpStream>::connect(SERVER_ADDR).await {
        Ok(mut client) => {
            let latency = client.ping().await?;
            println!("  Server at {} is up (ping: {}ms)", SERVER_ADDR, latency);
        }
        Err(e) => {
            eprintln!("  ERROR: Cannot connect to SPINE server at {}", SERVER_ADDR);
            eprintln!("  Start it first: cargo run -p spine-core");
            eprintln!("  Details: {}", e);
            std::process::exit(1);
        }
    }

    // Deploy research agents
    let topic = "rust-programming";
    let urls = [
        "https://example.com",
        "https://httpbin.org/html",
        "https://www.rust-lang.org"];

    println!("\n[2/4] Deploying {} research agents...", urls.len());
    let start = Instant::now();

    let mut handles = Vec::new();
    for (i, url) in urls.iter().enumerate() {
        let url = url.to_string();
        let topic = topic.to_string();
        handles.push(tokio::spawn(async move {
            research_agent(i, &url, &topic).await
        }));
    }

    let mut all_results = Vec::new();
    for handle in handles {
        match handle.await? {
            Ok(result) => all_results.push(result),
            Err(e) => eprintln!("  Agent error: {}", e),
        }
    }

    let elapsed = start.elapsed();
    println!(
        "\n[3/4] Research phase complete: {} agents, {:.1}s total",
        all_results.len(),
        elapsed.as_secs_f64()
    );

    // Aggregate results
    println!("\n[4/4] Aggregating results...");
    aggregator_agent(topic, urls.len()).await?;

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║   Demo Complete                                              ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║   Research agents: {}                                        ║", urls.len());
    println!("║   Total time: {:.1}s                                         ║", elapsed.as_secs_f64());
    println!("║   Protocol: Chameleon AEAD + Speculative Decoding            ║");
    println!("╚══════════════════════════════════════════════════════════════╝");

    Ok(())
}
