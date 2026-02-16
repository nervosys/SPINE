//! # Reconnect with Retry
//!
//! Demonstrates the auto-reconnect feature with exponential backoff,
//! useful for long-running agents that must survive server restarts.

use anyhow::Result;
use spine_agent::AgentClient;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    println!("Connecting with retry (max 5 attempts, 1s base delay)...");

    // connect_with_retry uses exponential backoff: 1s, 2s, 4s, 8s, ...
    // capped at 60s between attempts
    let mut client = AgentClient::connect_with_retry(
        "127.0.0.1:8080",
        5,                      // max retries
        Duration::from_secs(1), // base delay
    )
    .await?;

    println!("Connected!");

    // Normal operations — if the connection drops, you'd need to reconnect
    let latency = client.ping().await?;
    println!("Ping: {}ms", latency);

    // Demonstrate a long-running polling loop
    println!("\nStarting polling loop (5 iterations)...");
    for i in 1..=5 {
        match client.ping().await {
            Ok(ms) => println!("  [{}] pong: {}ms", i, ms),
            Err(e) => {
                println!("  [{}] lost connection: {}", i, e);
                println!("  Reconnecting...");
                client =
                    AgentClient::connect_with_retry("127.0.0.1:8080", 3, Duration::from_secs(1))
                        .await?;
                println!("  Reconnected!");
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    println!("\nDone. Connection resilient to temporary failures.");
    Ok(())
}
