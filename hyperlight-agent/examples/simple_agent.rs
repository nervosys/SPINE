use hyperlight_agent::{AgentClient, Compiler};
use hyperlight_protocol::{BrowserCommand, Message};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║   Hyperlight: Advanced HLS Compiler + Virtual DOM Runtime    ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");
    
    println!("[INIT] Connecting to Hyperlight Browser...");
    let mut client = AgentClient::connect("127.0.0.1:8080").await?;
    
    // Enable Chameleon Protocol (latent-space cryptography + moving target)
    let secret: [u8; 32] = [
        0x48, 0x79, 0x70, 0x65, 0x72, 0x6c, 0x69, 0x67,  // "Hyperlig"
        0x68, 0x74, 0x43, 0x68, 0x61, 0x6d, 0x65, 0x6c,  // "htChamel"
        0x65, 0x6f, 0x6e, 0x50, 0x72, 0x6f, 0x74, 0x6f,  // "eonProto"
        0x63, 0x6f, 0x6c, 0x53, 0x65, 0x63, 0x72, 0x65,  // "colSecre"
    ];
    client.handler.enable_chameleon(secret);
    println!("[✓] Chameleon Protocol enabled");
    
    // Enable speculative decoding for both directions
    client.handler.enable_speculation(true, true);
    println!("[✓] Speculative Decoding enabled (input + output)\n");

    // 1. Navigate to a website
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 1: Web Navigation with Speculation                    │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("[1] Navigating to https://example.com...");
    client.navigate("https://example.com").await?;
    println!("    ✓ Navigation complete (protocol morphed)\n");
    
    // 2. Fetch the Unified Representation multiple times to train predictor
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 2: Training Speculative Predictor                     │");
    println!("└─────────────────────────────────────────────────────────────┘");
    
    for i in 1..=3 {
        println!("[{}] Fetching Unified Representation (training iteration)...", i + 1);
        let ur = client.get_ur().await?;
        println!("    Page: {} | Elements: {}", ur.title, ur.elements.len());
    }
    
    // Show speculation stats
    let stats = client.handler.get_speculation_stats();
    println!("\n[STATS] After training:");
    println!("    Output predictions: {}", stats.output_predictions);
    println!("    Output hits: {} ({:.1}% accuracy)", 
             stats.output_hits, stats.output_accuracy() * 100.0);
    println!("    Bytes saved: {} bytes\n", stats.bytes_saved);
    
    // 3. Compile and Execute Advanced HLS Program with conditionals, loops, state
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 3: Advanced HLS Compiler Demo                         │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("[5] Compiling advanced HLS program with loops and conditionals...\n");
    
    // Demonstrate advanced language features
    let hls_source = r#"
        // Hyperlight Source Language - Advanced Features Demo
        // Variables and State
        let title = "Dashboard"
        let count = 3
        state theme = "dark"
        
        // Root element with nested children
        element container {
            element header {
                text "Hyperlight Virtual DOM Demo"
            }
            
            // Conditional rendering based on state
            if count > 0 {
                element status {
                    text "Active Items: 3"
                }
            }
            
            // Loop to create multiple items
            for item in [1, 2, 3] {
                element card {
                    text "Card Content"
                }
            }
            
            // Nested element structure
            element footer {
                element nav {
                    text "Navigation"
                }
                element copyright {
                    text "Hyperlight 2025"
                }
            }
        }
    "#;
    
    let binary = Compiler::compile(hls_source)?;
    println!("    ✓ Compiled {} instructions from advanced HLS", binary.instructions.len());
    
    // Print the instructions for inspection
    println!("\n    Generated Instructions:");
    for (i, inst) in binary.instructions.iter().take(10).enumerate() {
        println!("      [{}] {:?}", i, inst);
    }
    if binary.instructions.len() > 10 {
        println!("      ... and {} more", binary.instructions.len() - 10);
    }
    
    println!("\n[6] Executing HLB in Virtual DOM Runtime...");
    client.handler.send_message(&Message::Request(hyperlight_protocol::Request {
        id: "exec-1".to_string(),
        command: BrowserCommand::ExecuteBinary(binary),
    })).await?;

    if let Message::Response(resp) = client.handler.receive_message().await? {
        if let Some(result) = &resp.result {
            println!("\n    ═══════════════════════════════════════════════════════════");
            println!("    VDOM Execution Result:");
            println!("    ═══════════════════════════════════════════════════════════");
            
            if let Some(stats) = result.get("stats") {
                println!("    Instructions: {}", stats.get("instructions_executed").unwrap_or(&serde_json::json!(0)));
                println!("    Elements Created: {}", stats.get("elements_created").unwrap_or(&serde_json::json!(0)));
                println!("    Attributes Set: {}", stats.get("attributes_set").unwrap_or(&serde_json::json!(0)));
                println!("    Events Emitted: {}", stats.get("events_emitted").unwrap_or(&serde_json::json!(0)));
                println!("    Execution Time: {}µs", stats.get("execution_time_us").unwrap_or(&serde_json::json!(0)));
            }
            
            if let Some(ur) = result.get("ur") {
                println!("\n    Generated Unified Representation:");
                println!("    ───────────────────────────────────────────────────────────");
                for line in ur.as_str().unwrap_or("").lines().take(15) {
                    println!("    {}", line);
                }
            }
        }
    }
    
    // 4. Demonstrate explicit protocol morphing
    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 4: Protocol Morphing + Decoy Injection                │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("[7] Triggering explicit protocol morph...");
    client.handler.send_message(&Message::Request(hyperlight_protocol::Request {
        id: "morph-1".to_string(),
        command: BrowserCommand::Morph,
    })).await?;

    if let Message::Response(resp) = client.handler.receive_message().await? {
        println!("    ✓ Morph complete: {:?}\n", resp.result);
    }
    
    // 5. Send decoy traffic
    println!("[8] Injecting decoy traffic...");
    for i in 0..3 {
        client.handler.send_decoy().await?;
        println!("    Decoy {} injected", i + 1);
    }
    
    // 6. Semantic Search
    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 5: Semantic Search & Distributed Clustering           │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("[9] Performing semantic search for 'neural latent spaces'...");
    let search_results = client.search("neural latent spaces").await?;
    println!("    ✓ Search complete. Results: {}\n", search_results);

    // 7. Session Transfer
    let target_node = uuid::Uuid::new_v4();
    println!("[10] Simulating session transfer to node {}...", target_node);
    client.transfer_session(target_node).await?;
    println!("    ✓ Transfer request sent to cluster\n");

    // Final stats
    let final_stats = client.handler.get_speculation_stats();
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    Final Statistics                          ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ Output Predictions: {:<8} | Output Hits: {:<8}         ║", 
             final_stats.output_predictions, final_stats.output_hits);
    println!("║ Input Predictions:  {:<8} | Input Hits:  {:<8}         ║",
             final_stats.input_predictions, final_stats.input_hits);
    println!("║ Bytes Saved:        {:<8} | Precompute Hits: {:<4}       ║",
             final_stats.bytes_saved, final_stats.precompute_hits);
    println!("║ Output Accuracy:    {:<6.1}%  | Input Accuracy:  {:<6.1}%    ║",
             final_stats.output_accuracy() * 100.0,
             final_stats.input_accuracy() * 100.0);
    println!("╚══════════════════════════════════════════════════════════════╝");
    
    println!("\n[SUMMARY]");
    println!("  • Latent-space encoding: implicit encryption via projection");
    println!("  • Moving target: protocol morphs after each exchange");
    println!("  • Speculative decoding: predict messages, send confirmations");
    println!("  • Bandwidth savings: skip full payload on prediction hits");
    println!("  • Pre-computation: prepare responses before requests arrive");
    
    Ok(())
}
