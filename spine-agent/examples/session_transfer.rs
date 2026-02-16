//! # Session Transfer
//!
//! Demonstrates cluster-aware features: transferring sessions between
//! nodes, checking capabilities, and using the knowledge base for
//! distributed state.

use anyhow::Result;
use serde_json::json;
use spine_agent::AgentClient;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    // Check server capabilities
    println!("=== Server Capabilities ===");
    let caps = client.get_capabilities().await?;
    for cap in &caps {
        println!("  • {}", cap);
    }

    // Navigate and build up session state
    println!("\n=== Building Session State ===");
    client.navigate("https://example.com").await?;
    let ur = client.get_ur().await?;
    println!(
        "  Current page: {} ({} elements)",
        ur.title,
        ur.elements.len()
    );

    // Store session context in knowledge base
    client
        .store_knowledge(
            "session:context",
            json!({
                "current_url": "https://example.com",
                "element_count": ur.elements.len(),
                "timestamp": chrono_now(),
            }),
            vec!["session".into(), "context".into()],
        )
        .await?;
    println!("  Session context stored");

    // View session history
    println!("\n=== Session History ===");
    let history = client.get_history().await?;
    for (i, cmd) in history.iter().enumerate() {
        println!("  [{}] {:?}", i, cmd);
    }

    // Attempt session transfer (requires multi-node setup)
    println!("\n=== Session Transfer ===");
    let target_node = Uuid::new_v4(); // In practice, discover via cluster
    match client.transfer_session(target_node).await {
        Ok(()) => println!("  Session transferred to {}", target_node),
        Err(e) => println!("  Transfer not available (single node): {}", e),
    }

    // Propose knowledge for consensus
    println!("\n=== Knowledge Proposals ===");
    client
        .propose_knowledge(
            "fact:rust",
            json!({"statement": "Rust prevents data races at compile time"}),
            vec!["facts".into(), "programming".into()],
        )
        .await?;
    println!("  Knowledge proposal submitted for consensus");

    Ok(())
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{}", secs)
}
