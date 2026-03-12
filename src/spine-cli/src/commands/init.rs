//! `spine init` — Scaffold a new SPINE project

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::PathBuf;

const DEFAULT_SPINE_TOML: &str = r#"# SPINE Server Configuration
# See https://github.com/nervosys/SPINE for documentation

[server]
host = "127.0.0.1"
port = 8080
ws_port_offset = 1
quic_port_offset = 2
metrics_port = 9090
max_sessions = 1000
max_connections_per_ip = 50
idle_timeout_secs = 300
session_watchdog_secs = 600
persistence_interval_secs = 60
shutdown_timeout_secs = 30

[tls]
enabled = false
cert_path = "certs/cert.pem"
key_path = "certs/key.pem"
ca_path = "certs/ca.pem"

[cluster]
port_offset = 1000
region = "us-west"
skills = ["research", "synthesis", "scraping"]

[logging]
format = "pretty"
level = "info"
"#;

const DEFAULT_SPINE_TOML_TLS: &str = r#"# SPINE Server Configuration (TLS enabled)
# See https://github.com/nervosys/SPINE for documentation

[server]
host = "0.0.0.0"
port = 8443
ws_port_offset = 1
quic_port_offset = 2
metrics_port = 9090
max_sessions = 1000
max_connections_per_ip = 50
idle_timeout_secs = 300
session_watchdog_secs = 600
persistence_interval_secs = 60
shutdown_timeout_secs = 30

[tls]
enabled = true
cert_path = "certs/cert.pem"
key_path = "certs/key.pem"
ca_path = "certs/ca.pem"

[cluster]
port_offset = 1000
region = "us-west"
skills = ["research", "synthesis", "scraping"]

[logging]
format = "json"
level = "info"
"#;

const EXAMPLE_AGENT: &str = r#"//! Example SPINE agent
//!
//! Connects to a SPINE server, navigates to a website, and extracts its
//! Unified Representation.

use spine_agent::AgentClient;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    // Connect to the local SPINE server
    let mut client = AgentClient::connect("127.0.0.1:8080").await?;
    println!("Connected to SPINE server");

    // Check latency
    let latency = client.ping().await?;
    println!("Ping: {}ms", latency);

    // Navigate to a page
    client.navigate("https://example.com").await?;
    println!("Navigated to example.com");

    // Get the semantic representation
    let ur = client.get_ur().await?;
    println!("Page: {}", ur.title);
    println!("Elements: {}", ur.elements.len());

    // Print each element
    for element in &ur.elements {
        println!("  {:?}", element);
    }

    Ok(())
}
"#;

const EXAMPLE_KNOWLEDGE: &str = r#"//! Knowledge base example
//!
//! Demonstrates storing and querying the distributed knowledge base.

use spine_agent::AgentClient;
use anyhow::Result;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    // Store some knowledge
    client.store_knowledge(
        "rust-features",
        json!({
            "memory_safety": true,
            "zero_cost_abstractions": true,
            "fearless_concurrency": true,
        }),
        vec!["programming".into(), "rust".into()],
    ).await?;
    println!("Stored knowledge: rust-features");

    // Query by tags
    let results = client.query_knowledge(
        "programming language features",
        vec!["rust".into()],
        10,
    ).await?;
    println!("Found {} results:", results.len());
    for r in &results {
        println!("  {}", serde_json::to_string_pretty(r)?);
    }

    Ok(())
}
"#;

const EXAMPLE_SWARM: &str = r#"//! Swarm intelligence example
//!
//! Creates a collaborative plan and delegates tasks across agents.

use spine_agent::AgentClient;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    // Enable autonomous mode
    client.set_autonomous_mode(true).await?;
    println!("Autonomous mode enabled");

    // Create a swarm plan
    let plan_id = client.create_swarm_plan("Research and summarize AI news").await?;
    println!("Created plan: {}", plan_id);

    // Initiate a swarm search
    client.swarm_search("latest AI developments 2026", 3).await?;
    println!("Swarm search initiated");

    // Check agentic state
    let state = client.get_agentic_state().await?;
    println!("Agent state: {}", serde_json::to_string_pretty(&state)?);

    Ok(())
}
"#;

const EXAMPLE_BENCHMARK: &str = r#"//! Benchmark example
//!
//! Measures latency and throughput of a SPINE server.

use spine_agent::AgentClient;
use anyhow::Result;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    // Latency benchmark
    let mut latencies = Vec::new();
    for _ in 0..50 {
        let start = Instant::now();
        client.ping().await?;
        latencies.push(start.elapsed().as_micros() as f64);
    }
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    println!("Ping Latency (50 iterations):");
    println!("  Min:  {:.1} µs", latencies[0]);
    println!("  P50:  {:.1} µs", latencies[25]);
    println!("  P99:  {:.1} µs", latencies[49]);
    println!("  Mean: {:.1} µs", latencies.iter().sum::<f64>() / 50.0);

    // UR fetch throughput
    client.navigate("https://example.com").await?;
    let start = Instant::now();
    for _ in 0..100 {
        let _ = client.get_ur().await?;
    }
    let elapsed = start.elapsed();
    println!("\nUR Fetch Throughput:");
    println!("  100 fetches in {:.2}s", elapsed.as_secs_f64());
    println!("  {:.1} fetches/sec", 100.0 / elapsed.as_secs_f64());

    // Speculation stats
    let stats = client.get_speculation_stats();
    println!("\nSpeculation Stats:");
    println!("  Predictions: {}", stats.output_predictions);
    println!("  Hits: {}", stats.output_hits);
    println!("  Accuracy: {:.1}%", stats.output_accuracy() * 100.0);
    println!("  Bytes saved: {}", stats.bytes_saved);

    Ok(())
}
"#;

const GITIGNORE: &str = r#"/target
/sessions
*.log
"#;

pub async fn run(path: PathBuf, name: Option<String>, tls: bool, examples: bool) -> Result<()> {
    let project_name = name.unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("my-spine-project")
            .to_string()
    });

    eprintln!(
        "{} Initializing SPINE project: {}",
        "▸".green().bold(),
        project_name.cyan()
    );

    // Create directories
    let dirs = ["src", "sessions", "certs"];
    for d in &dirs {
        let dir = path.join(d);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create directory: {}", dir.display()))?;
    }
    eprintln!("  {} Created directories", "✓".green());

    // Write spine.toml
    let config_content = if tls {
        DEFAULT_SPINE_TOML_TLS
    } else {
        DEFAULT_SPINE_TOML
    };
    std::fs::write(path.join("spine.toml"), config_content)?;
    eprintln!("  {} Created spine.toml", "✓".green());

    // Write .gitignore
    std::fs::write(path.join(".gitignore"), GITIGNORE)?;
    eprintln!("  {} Created .gitignore", "✓".green());

    // Write Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{project_name}"
version = "0.1.0"
edition = "2021"

[dependencies]
spine-agent = {{ git = "https://github.com/nervosys/SPINE.git" }}
spine-protocol = {{ git = "https://github.com/nervosys/SPINE.git" }}
tokio = {{ version = "1", features = ["full"] }}
anyhow = "1.0"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1.0"
env_logger = "0.10"
log = "0.4"
"#
    );
    std::fs::write(path.join("Cargo.toml"), cargo_toml)?;
    eprintln!("  {} Created Cargo.toml", "✓".green());

    // Write main.rs
    std::fs::write(path.join("src/main.rs"), EXAMPLE_AGENT)?;
    eprintln!("  {} Created src/main.rs", "✓".green());

    // Write example files
    if examples {
        let examples_dir = path.join("examples");
        std::fs::create_dir_all(&examples_dir)?;
        std::fs::write(examples_dir.join("knowledge.rs"), EXAMPLE_KNOWLEDGE)?;
        std::fs::write(examples_dir.join("swarm.rs"), EXAMPLE_SWARM)?;
        std::fs::write(examples_dir.join("benchmark.rs"), EXAMPLE_BENCHMARK)?;
        eprintln!("  {} Created examples/", "✓".green());
    }

    eprintln!("\n{} Project ready! Next steps:", "✓".green().bold());
    eprintln!("  1. Start a SPINE server:  {}", "spine deploy".cyan());
    eprintln!("  2. Run your agent:        {}", "cargo run".cyan());
    eprintln!("  3. Check server health:   {}", "spine status".cyan());

    Ok(())
}
