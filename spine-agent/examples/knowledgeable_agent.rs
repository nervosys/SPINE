use spine_agent::AgentClient;
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    println!("Starting Knowledgeable Agent...");

    // 1. Connect to the SPINE Core
    let mut client = AgentClient::connect("127.0.0.1:8080").await?;
    println!("Connected to SPINE Core");

    // 2. Store some knowledge
    println!("Storing knowledge about SPINE...");
    client.store_knowledge(
        "hyperlight_version", 
        json!("0.1.0-alpha"), 
        vec!["metadata".to_string(), "version".to_string()]
    ).await?;

    client.store_knowledge(
        "project_goal", 
        json!("Build a high-performance browser engine for AI agents"), 
        vec!["mission".to_string()]
    ).await?;

    // 3. Query knowledge
    println!("Querying knowledge...");
    let results = client.query_knowledge("SPINE", vec![], 10).await?;
    for res in results {
        println!("Found: {} = {}", res["key"], res["value"]);
    }

    // 4. Execute HLS with capabilities
    println!("Executing HLS with capabilities...");
    let script = r#"
        capability network
        capability storage
        
        on_mount -> {
            print("Agent is active and has network/storage capabilities");
            navigate("https://example.com");
        }
        
        element App {
            text "Knowledgeable Agent UI"
        }
    "#;

    let result = client.execute_hls(script).await?;
    println!("HLS Execution Result: {:?}", result.stats);

    // 5. Check session history
    println!("Fetching session history...");
    let history = client.get_history().await?;
    println!("Session History ({} commands):", history.len());
    for (i, cmd) in history.iter().enumerate() {
        println!("{}. {:?}", i + 1, cmd);
    }

    println!("Agent task complete.");
    Ok(())
}
