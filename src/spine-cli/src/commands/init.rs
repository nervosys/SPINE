//! `spine init` — Scaffold a new SPINE project

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::PathBuf;

const DEFAULT_SPINE_TOML: &str = r#"# SPINE Server Configuration
# See https://github.com/nervosys/SPINE for documentation

[server]
# Loopback only. Binding any other address (for example "0.0.0.0") is refused
# at startup unless `mutual_tls = true` under [tls] (with TLS enabled and a
# client CA), or `allow_unauthenticated_remote = true` here, which says the
# network itself is trusted: every native client has full control of the node.
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
# Every interface. This is valid only because `mutual_tls = true` under [tls]
# identifies each client by a certificate signed by `ca_path`; every native
# client has full control of this node. Without mutual TLS, a host that is not
# loopback is refused at startup unless `allow_unauthenticated_remote = true`
# is set here, which says the network itself is trusted. For a node reached
# only from this machine, use host = "127.0.0.1".
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
# The CA that signs the client certificates this node accepts.
ca_path = "certs/ca.pem"
mutual_tls = true

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
        std::fs::write(examples_dir.join("benchmark.rs"), EXAMPLE_BENCHMARK)?;
        eprintln!("  {} Created examples/", "✓".green());
    }

    eprintln!("\n{} Project ready! Next steps:", "✓".green().bold());
    eprintln!("  1. Start a SPINE server:  {}", "spine deploy".cyan());
    eprintln!("  2. Run your agent:        {}", "cargo run".cyan());
    eprintln!("  3. Check server health:   {}", "spine status".cyan());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> spine_core::SpineConfig {
        toml::from_str(text).expect("the template is valid TOML for the daemon")
    }

    #[test]
    fn the_plain_template_binds_loopback_and_validates_without_the_unauthenticated_flag() {
        let config = parse(DEFAULT_SPINE_TOML);
        assert_eq!(config.server.host, "127.0.0.1");
        assert!(!config.server.allow_unauthenticated_remote);
        config.validate().unwrap();
    }

    #[test]
    fn the_tls_template_is_mutual_tls_and_does_not_set_the_unauthenticated_flag() {
        let config = parse(DEFAULT_SPINE_TOML_TLS);
        assert_eq!(config.server.host, "0.0.0.0");
        assert!(config.tls.enabled && config.tls.mutual_tls);
        assert!(!config.server.allow_unauthenticated_remote);
        config.validate().unwrap();
    }
}
