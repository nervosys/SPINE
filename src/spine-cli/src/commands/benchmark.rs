//! `spine benchmark` — Performance benchmarks

use anyhow::Result;
use colored::Colorize;
use spine_agent::AgentClient;
use std::time::Instant;
use tokio::net::TcpStream;

pub async fn run(addr: String, iterations: usize, concurrency: usize) -> Result<()> {
    eprintln!(
        "{} Benchmarking {} ({} iterations, {} concurrent)",
        "▸".green().bold(),
        addr.cyan(),
        iterations,
        concurrency
    );
    eprintln!();

    if concurrency <= 1 {
        single_bench(&addr, iterations).await
    } else {
        concurrent_bench(&addr, iterations, concurrency).await
    }
}

async fn single_bench(addr: &str, iterations: usize) -> Result<()> {
    let mut client = AgentClient::<TcpStream>::connect(addr).await?;

    // ── Ping latency ──
    eprintln!("{}", "═══ Ping Latency ═══".bold());
    let mut latencies = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        client.ping().await?;
        latencies.push(start.elapsed().as_nanos() as f64);
    }
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = latencies.len();
    eprintln!("  Min:   {:>10.1} µs", latencies[0] / 1000.0);
    eprintln!("  P50:   {:>10.1} µs", latencies[n / 2] / 1000.0);
    eprintln!("  P99:   {:>10.1} µs", latencies[n * 99 / 100] / 1000.0);
    eprintln!("  Max:   {:>10.1} µs", latencies[n - 1] / 1000.0);
    eprintln!(
        "  Mean:  {:>10.1} µs",
        latencies.iter().sum::<f64>() / n as f64 / 1000.0
    );

    // ── UR Fetch Throughput ──
    eprintln!();
    eprintln!("{}", "═══ UR Fetch Throughput ═══".bold());
    client.navigate("https://example.com").await?;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = client.get_ur().await?;
    }
    let elapsed = start.elapsed();
    eprintln!(
        "  {} fetches in {:.3}s → {:.1} fetches/sec",
        iterations,
        elapsed.as_secs_f64(),
        iterations as f64 / elapsed.as_secs_f64()
    );

    // ── Latent Vector ──
    eprintln!();
    eprintln!("{}", "═══ Latent Vector ═══".bold());
    // The latent encode needs a daemon built with the `research` feature; a
    // research-free daemon answers `unsupported_without_research`.
    let start = Instant::now();
    let mut latent_done = 0usize;
    let mut latent_unsupported = None;
    for _ in 0..iterations.min(50) {
        match client.get_latent_ur(256).await {
            Ok(_) => latent_done += 1,
            Err(e) if e.to_string().starts_with("unsupported_without_research") => {
                latent_unsupported = Some(e.to_string());
                break;
            }
            Err(e) => return Err(e),
        }
    }
    let elapsed = start.elapsed();
    if let Some(msg) = latent_unsupported {
        eprintln!("  skipped: {msg}");
    } else {
        eprintln!(
            "  {} encodes in {:.3}s → {:.1} encodes/sec",
            latent_done,
            elapsed.as_secs_f64(),
            latent_done as f64 / elapsed.as_secs_f64()
        );
    }

    eprintln!();
    eprintln!("{} Benchmark complete", "✓".green().bold());
    Ok(())
}

async fn concurrent_bench(addr: &str, iterations: usize, concurrency: usize) -> Result<()> {
    eprintln!("{}", "═══ Concurrent Connections ═══".bold());

    let per_task = iterations / concurrency;
    let addr_owned = addr.to_string();

    let start = Instant::now();
    let mut handles = Vec::new();

    for task_id in 0..concurrency {
        let a = addr_owned.clone();
        handles.push(tokio::spawn(async move {
            let mut client = AgentClient::<TcpStream>::connect(&a).await?;
            let mut latencies = Vec::with_capacity(per_task);

            for _ in 0..per_task {
                let t = Instant::now();
                client.ping().await?;
                latencies.push(t.elapsed().as_nanos() as f64);
            }

            Ok::<(usize, Vec<f64>), anyhow::Error>((task_id, latencies))
        }));
    }

    let mut all_latencies = Vec::new();
    for h in handles {
        let (id, lats) = h.await??;
        eprintln!(
            "  Task {}: {} pings, avg {:.1} µs",
            id,
            lats.len(),
            lats.iter().sum::<f64>() / lats.len() as f64 / 1000.0
        );
        all_latencies.extend(lats);
    }

    let total_time = start.elapsed();
    all_latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = all_latencies.len();

    eprintln!();
    eprintln!("{}", "═══ Aggregate ═══".bold());
    eprintln!("  Total:  {} pings in {:.3}s", n, total_time.as_secs_f64());
    eprintln!(
        "  Rate:   {:.1} pings/sec",
        n as f64 / total_time.as_secs_f64()
    );
    eprintln!("  P50:    {:.1} µs", all_latencies[n / 2] / 1000.0);
    eprintln!("  P99:    {:.1} µs", all_latencies[n * 99 / 100] / 1000.0);

    eprintln!();
    eprintln!("{} Benchmark complete", "✓".green().bold());
    Ok(())
}
