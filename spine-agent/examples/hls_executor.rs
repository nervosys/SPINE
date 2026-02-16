//! # HLS Script Executor
//!
//! Demonstrates compiling and executing HLS (Hyperlight Scripting)
//! programs through the SPINE agent API, including reactive state
//! management and WASM execution.

use anyhow::Result;
use spine_agent::{AgentClient, Compiler};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut client = AgentClient::connect("127.0.0.1:8080").await?;

    // 1. Simple expression evaluation
    println!("=== Simple Expression ===");
    let result = client.execute_hls("let x = 42; x * 2").await?;
    println!("42 * 2 = {:?}", result);

    // 2. String manipulation
    println!("\n=== String Operations ===");
    let result = client
        .execute_hls(
            r#"
        let greeting = "Hello";
        let name = "SPINE";
        greeting
    "#,
        )
        .await?;
    println!("Result: {:?}", result);

    // 3. Conditional logic
    println!("\n=== Conditional Logic ===");
    let result = client
        .execute_hls(
            r#"
        let score = 85;
        let grade = if score >= 90 {
            "A"
        } else if score >= 80 {
            "B"
        } else {
            "C"
        };
        grade
    "#,
        )
        .await?;
    println!("Grade: {:?}", result);

    // 4. Loop with accumulator
    println!("\n=== Loop ===");
    let result = client
        .execute_hls(
            r#"
        let sum = 0;
        let i = 1;
        while i <= 10 {
            sum = sum + i;
            i = i + 1;
        }
        sum
    "#,
        )
        .await?;
    println!("Sum 1..10 = {:?}", result);

    // 5. Offline compilation to check syntax
    println!("\n=== Offline Compilation ===");
    match Compiler::compile("let x = 1 + 2; x") {
        Ok(binary) => println!("Compiled to {} instructions", binary.instructions.len()),
        Err(e) => println!("Compile error: {}", e),
    }

    Ok(())
}
