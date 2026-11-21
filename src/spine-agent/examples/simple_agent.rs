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

    println!();

    // 1. Navigate to a website
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 1: Web Navigation                                     │");
    println!("└─────────────────────────────────────────────────────────────┘");
    println!("[1] Navigating to https://example.com...");
    client.navigate("https://example.com").await?;
    println!("    ✓ Navigation complete\n");

    // 2. Fetch the Unified Representation several times
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 2: Repeated Unified Representation Fetches            │");
    println!("└─────────────────────────────────────────────────────────────┘");

    for i in 1..=3 {
        println!("[{}] Fetching Unified Representation...", i + 1);
        let ur = client.get_ur().await?;
        println!("    Page: {} | Elements: {}", ur.title, ur.elements.len());
    }

    // 3. Compile and Execute Advanced HLS Program with reactive state
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ Phase 3: Reactive State                                      │");
    println!("└─────────────────────────────────────────────────────────────┘");

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

    println!("[DONE] SPINE Agent demonstration complete.");
    Ok(())
}
