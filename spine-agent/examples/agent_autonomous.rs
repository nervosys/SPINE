use anyhow::Result;
use spine_agent::{AgentClient, Compiler};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║   SPINE: Autonomous Agentic Navigation Demo                   ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("[INIT] Connecting to SPINE Browser...");
    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    // Enable Chameleon Protocol
    let secret: [u8; 32] = [0x42; 32];
    client.handler.enable_chameleon(secret);
    println!("[✓] Chameleon Protocol enabled");

    // Enable speculative decoding
    client.handler.enable_speculation(true, true);
    println!("[✓] Speculative Decoding enabled\n");

    println!("[1] Compiling autonomous HLS program...\n");

    // This HLS program uses the new 'navigate' and 'search' statements
    // to perform autonomous actions from within the browser runtime.
    let hls_source = r#"
        state step = "start"
        state query = "SPINE Browser engine"
        
        fn render() {
            element Dashboard {
                element Status {
                    text "Current Step: " ++ step
                }
                
                if step == "start" {
                    element Action {
                        text "Initiating search for: " ++ query
                        on_mount -> {
                            search(query)
                            step = "searching"
                        }
                    }
                } else if step == "searching" {
                    element Results {
                        text "Search broadcast to cluster. Waiting for results..."
                        // In a real app, we'd handle search results via events
                        element Button {
                            text "Simulate Result Found"
                            on_click -> {
                                navigate("https://github.com/nervosys/Hyperlight")
                                step = "navigating"
                            }
                        }
                    }
                } else if step == "navigating" {
                    element Final {
                        text "Navigated to GitHub repository."
                        element Link {
                            attribute href "https://github.com/nervosys/Hyperlight"
                            text "View Source"
                        }
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

    println!("[2] Executing autonomous binary...");
    let response = client.execute_binary(binary).await?;

    // execute_binary returns serde_json::Value
    println!("\n[RESULT] Execution response: {}", response);
    if let Some(actions) = response.get("actions") {
        println!("    Actions requested by WASM: {}", actions);
    }
    if let Some(stats) = response.get("stats") {
        if let Some(count) = stats.get("instructions_executed") {
            println!("    Instructions executed: {}", count);
        }
    }

    println!("\n[3] Simulating event loop...");
    // In a real scenario, the agent would listen for events and re-execute
    // For this demo, we'll just trigger the 'Click' event to move to the next step

    println!("[4] Clicking 'Simulate Result Found' button...");
    let patches = client
        .handle_event(0, "click", serde_json::json!({}))
        .await?;

    // handle_event returns Vec<VDomPatch>
    println!(
        "    ✓ Event handled. Received {} VDOM patches.",
        patches.len()
    );

    println!("\n[DONE] Autonomous agent demo complete.");
    Ok(())
}
