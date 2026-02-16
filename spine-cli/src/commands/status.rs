//! `spine status` — Check server health and metrics

use anyhow::Result;
use colored::Colorize;

pub async fn run(addr: String) -> Result<()> {
    // Derive the metrics port from the main port
    // Default: metrics = 9090 when server = 8080
    let parts: Vec<&str> = addr.split(':').collect();
    let host = if parts.len() >= 2 {
        parts[0]
    } else {
        "127.0.0.1"
    };
    let port: u16 = if parts.len() >= 2 {
        parts[1].parse().unwrap_or(8080)
    } else {
        8080
    };
    let metrics_port = if port == 8080 { 9090 } else { port + 1010 };

    eprintln!(
        "{} Checking health of {}...",
        "▸".green().bold(),
        addr.cyan()
    );

    // Try /health endpoint
    let health_url = format!("http://{}:{}/health", host, metrics_port);
    if let Ok(resp) = reqwest::get(&health_url).await {
        if resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            eprintln!("{} Server is healthy", "✓".green().bold());
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{}", body);
            }
        } else {
            eprintln!("{} Server returned {}", "✗".red().bold(), resp.status());
        }
    } else {
        eprintln!(
            "{} Cannot reach health endpoint at {}",
            "✗".red().bold(),
            health_url
        );
    }

    // Try /ready endpoint
    let ready_url = format!("http://{}:{}/ready", host, metrics_port);
    if let Ok(resp) = reqwest::get(&ready_url).await {
        let status = if resp.status().is_success() {
            "ready".green().to_string()
        } else {
            "not ready".red().to_string()
        };
        eprintln!("  Readiness: {}", status);
    }

    // Try /metrics endpoint (Prometheus format)
    let metrics_url = format!("http://{}:{}/metrics", host, metrics_port);
    if let Ok(resp) = reqwest::get(&metrics_url).await {
        if resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            eprintln!();
            eprintln!("{}", "═══ Metrics ═══".bold());
            for line in body.lines() {
                if line.starts_with('#') {
                    continue;
                }
                if line.contains("spine_") {
                    println!("  {}", line);
                }
            }
        }
    }

    // Also try a direct TCP ping
    eprintln!();
    eprintln!("{}", "═══ Connection Test ═══".bold());
    match spine_agent::AgentClient::<tokio::net::TcpStream>::connect(&addr).await {
        Ok(mut client) => match client.ping().await {
            Ok(ms) => eprintln!("  {} TCP ping: {}ms", "✓".green(), ms),
            Err(e) => eprintln!("  {} Ping failed: {}", "✗".red(), e),
        },
        Err(e) => {
            eprintln!("  {} Cannot connect to {}: {}", "✗".red(), addr, e);
        }
    }

    Ok(())
}