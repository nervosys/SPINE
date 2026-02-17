use anyhow::Result;
use spine_agent::{AgentClient, Compiler};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║   SPINE: Advanced HLS Compiler + Virtual DOM Runtime         ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("[INIT] Connecting to SPINE Browser...");
    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    // Enable Chameleon Protocol (latent-space cryptography + moving target)
    let secret: [u8; 32] = [
        0x48, 0x79, 0x70, 0x65, 0x72, 0x6c, 0x69, 0x67, // "Hyperlig"
        0x68, 0x74, 0x43, 0x68, 0x61, 0x6d, 0x65, 0x6c, // "htChamel"
        0x65, 0x6f, 0x6e, 0x50, 0x72, 0x6f, 0x74, 0x6f, // "eonProto"
        0x63, 0x6f, 0x6c, 0x53, 0x65, 0x63, 0x72, 0x65, // "colSecre"
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
        println!(
            "[{}] Fetching Unified Representation (training iteration)...",
            i + 1
        );
        let ur = client.get_ur().await?;
        println!("    Page: {} | Elements: {}", ur.title, ur.elements.len());
    }

    // Show speculation stats
    let stats = client.handler.get_speculation_stats();
    println!("\n[STATS] After training:");
    println!("    Output predictions: {}", stats.output_predictions);
    println!(
        "    Output hits: {} ({:.1}% accuracy)",
        stats.output_hits,
        stats.output_accuracy() * 100.0
    );
    println!("    Bytes saved: {} bytes\n", stats.bytes_saved);

    // 3. Compile and Execute Advanced HLS Program with reactive state
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 3: Reactive State & Human Interaction Demo            │");
    println!("└─────────────────────────────────────────────────────────────┘");

    // Enable human-like interaction patterns
    client.enable_human_mode(spine_human::HumanInteractionEngine::default());
    println!("[✓] Human Interaction Engine enabled (60 WPM, 250ms reaction)\n");

    println!("[5] Compiling reactive HLS program...\n");

    // Demonstrate reactive state and event handling
    let hls_source = r#"
        state counter = 0
        let title = "Reactive Counter"
        
        element App {
            element Header {
                text title
            }
            
            element CounterDisplay {
                text "Current Count: " ++ str(counter)
            }
            
            element Controls {
                element Button {
                    attribute id "increment-btn"
                    text "Increment"
                    on_click -> {
                        counter = counter + 1
                        emit("counter_updated", { new_value: counter })
                    }
                }
            }
        }
    "#;

    let binary = Compiler::compile(hls_source)?;
    println!(
        "    ✓ Compilation successful ({} instructions)",
        binary.instructions.len()
    );

    println!("[6] Executing binary on remote browser...");
    let _ = client.execute_binary(binary).await?;
    println!("    ✓ Execution complete. Initial VDOM generated.\n");

    // 4. Simulate human interaction triggering reactive updates
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 4: Human-like Interaction & Reactive Updates          │");
    println!("└─────────────────────────────────────────────────────────────┘");

    println!("[7] Simulating human click on 'Increment' button (ID: 10)...");
    // In our VM, the button might have ID 10 (increment-btn)
    let patches = client
        .handle_event(10, "click", serde_json::Value::Null)
        .await?;

    println!(
        "    ✓ Event handled. Received {} VDOM patches:",
        patches.len()
    );
    for (i, patch) in patches.iter().enumerate() {
        println!("      [{}] {:?}", i + 1, patch);
    }

    println!("\n[8] Simulating human typing in search field...");
    client.type_text("search-input", "SPINE Protocol").await?;
    println!("    ✓ Typing complete with realistic delays.\n");

    // 5. Distributed Search across the cluster
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 5: Distributed Cluster Search                         │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("[9] Performing distributed search for 'Protocol'...");
    let search_results = client.search("Protocol").await?;
    println!("    ✓ Search complete. Results: {}\n", search_results);

    println!("[DONE] SPINE Agent demonstration complete.");
    Ok(())
}
