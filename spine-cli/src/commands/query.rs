//! `spine query` — Send one-shot queries to a SPINE server

use anyhow::Result;
use colored::Colorize;
use spine_agent::AgentClient;
use std::time::Instant;
use tokio::net::TcpStream;

/// Navigate to a URL and print the Unified Representation.
pub async fn navigate(addr: String, url: String, format: String) -> Result<()> {
    let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
    client.navigate(&url).await?;
    let ur = client.get_ur().await?;

    match format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&ur)?),
        "compact" => println!("{}", serde_json::to_string(&ur)?),
        _ => {
            println!("{}: {}", "Title".bold(), ur.title);
            println!("{}: {}", "Elements".bold(), ur.elements.len());
            for el in &ur.elements {
                println!("  {:?}", el);
            }
        }
    }
    Ok(())
}

/// Search the semantic web.
pub async fn search(addr: String, query: String) -> Result<()> {
    let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
    let result = client.search(&query).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// Store a key-value pair in the knowledge base.
pub async fn store(addr: String, key: String, value: String, tags: String) -> Result<()> {
    let val: serde_json::Value =
        serde_json::from_str(&value).unwrap_or_else(|_| serde_json::Value::String(value.clone()));
    let tag_list: Vec<String> = if tags.is_empty() {
        vec![]
    } else {
        tags.split(',').map(|t| t.trim().to_string()).collect()
    };
    let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
    client.store_knowledge(&key, val, tag_list).await?;
    eprintln!("{} Stored: {}", "✓".green(), key);
    Ok(())
}

/// Query the knowledge base.
pub async fn knowledge(addr: String, query: String, tags: String, limit: usize) -> Result<()> {
    let tag_list: Vec<String> = if tags.is_empty() {
        vec![]
    } else {
        tags.split(',').map(|t| t.trim().to_string()).collect()
    };
    let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
    let results = client.query_knowledge(&query, tag_list, limit).await?;
    println!("{}", serde_json::to_string_pretty(&results)?);
    Ok(())
}

/// Fetch raw HTML.
pub async fn html(addr: String, url: String) -> Result<()> {
    let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
    client.navigate(&url).await?;
    let raw = client.get_raw_html().await?;
    println!("{}", raw);
    Ok(())
}

/// Execute HLS code.
pub async fn exec(addr: String, script: String) -> Result<()> {
    // If it's a file path, read the file
    let code = if std::path::Path::new(&script).exists() {
        std::fs::read_to_string(&script)?
    } else {
        script
    };
    let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
    let result = client.execute_hls(&code).await?;
    println!("{:?}", result);
    Ok(())
}

/// Ping the server N times.
pub async fn ping(addr: String, count: usize) -> Result<()> {
    let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
    let mut latencies = Vec::with_capacity(count);

    for i in 0..count {
        let start = Instant::now();
        let ms = client.ping().await?;
        let rtt = start.elapsed();
        latencies.push(rtt.as_micros() as f64);
        println!(
            "pong from {}: seq={} time={:.1}µs (reported={}ms)",
            addr,
            i,
            rtt.as_micros() as f64,
            ms
        );
    }

    if !latencies.is_empty() {
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let min = latencies[0];
        let max = latencies[latencies.len() - 1];
        let avg = latencies.iter().sum::<f64>() / latencies.len() as f64;
        println!(
            "\n--- {} ping statistics ---\n{} transmitted, min/avg/max = {:.1}/{:.1}/{:.1} µs",
            addr, count, min, avg, max
        );
    }
    Ok(())
}
