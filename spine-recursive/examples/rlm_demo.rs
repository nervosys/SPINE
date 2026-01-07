//! Demonstration of Recursive Language Model (RLM) for infinite context processing
//!
//! This example shows how RLMs can process arbitrarily large contexts by treating
//! them as part of the environment rather than feeding directly into the neural network.

use spine_recursive::{
    ContextMetadata, MockSubLlmDispatcher, RecursiveLM, RlmConfig, RlmResponse,
};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║     SPINE Recursive Language Model (RLM) Demo           ║");
    println!("║     Based on Zhang et al., 2025 - arXiv:2512.24601           ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    // Create dispatchers (in production, these would be real LLM APIs)
    let root_dispatcher = Arc::new(MockSubLlmDispatcher::new("gpt-5"));
    let sub_dispatcher = Arc::new(MockSubLlmDispatcher::new("gpt-5-mini"));

    // Configure RLM
    let config = RlmConfig {
        max_recursion_depth: 3,
        default_chunk_size: 10_000,    // 10K chars per chunk
        max_context_size: 100_000_000, // 100M chars
        speculative_subcalls: true,
        subcall_batch_size: 5,
        ..Default::default()
    };

    println!("📋 RLM Configuration:");
    println!("   • Max recursion depth: {}", config.max_recursion_depth);
    println!(
        "   • Default chunk size: {} chars",
        config.default_chunk_size
    );
    println!("   • Max context size: {} chars", config.max_context_size);
    println!(
        "   • Speculative sub-calls: {}",
        config.speculative_subcalls
    );
    println!();

    // Create RLM
    let rlm = RecursiveLM::new(config, root_dispatcher, sub_dispatcher);

    // Generate large synthetic context (simulating 10M+ tokens)
    println!("📚 Generating large context (simulating 10M+ token document)...");
    let large_context = generate_synthetic_context(500_000); // 500K chars
    println!(
        "   Generated {} characters ({} estimated tokens)",
        large_context.len(),
        large_context.len() / 4
    );

    // Load context into RLM
    println!("\n📥 Loading context into REPL environment...");
    let metadata = rlm.load_context("research_corpus", large_context).await?;
    print_metadata(&metadata);

    // Demonstrate different query strategies
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("🔍 QUERY 1: Search-type query (Filter and Search Strategy)");
    println!("═══════════════════════════════════════════════════════════════");

    let query1 = "Find all references to quantum computing in section 42";
    println!("Query: \"{}\"\n", query1);

    let response1 = rlm.query(query1).await?;
    print_response(&response1);

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("📊 QUERY 2: Aggregation query (Chunk and Aggregate Strategy)");
    println!("═══════════════════════════════════════════════════════════════");

    let query2 = "Count how many times 'neural network' is mentioned";
    println!("Query: \"{}\"\n", query2);

    let response2 = rlm.query(query2).await?;
    print_response(&response2);

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("📝 QUERY 3: Summary query (Hierarchical Summarize Strategy)");
    println!("═══════════════════════════════════════════════════════════════");

    let query3 = "Summarize the key findings about machine learning";
    println!("Query: \"{}\"\n", query3);

    let response3 = rlm.query(query3).await?;
    print_response(&response3);

    // Print final statistics
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("📈 Final RLM Statistics");
    println!("═══════════════════════════════════════════════════════════════");

    let stats = rlm.stats();
    println!("   • Total queries processed: {}", stats.queries_processed);
    println!("   • Total input chars: {}", stats.total_input_chars);
    println!("   • Total output chars: {}", stats.total_output_chars);
    println!("   • Root LLM calls: {}", stats.root_llm_calls);
    println!("   • Sub-LLM calls: {}", stats.sub_llm_calls);
    println!("\n   REPL Statistics:");
    println!(
        "   • Variables loaded: {}",
        stats.repl_stats.total_variables
    );
    println!("   • Chars loaded: {}", stats.repl_stats.total_chars_loaded);
    println!("   • Chunks accessed: {}", stats.repl_stats.chunks_accessed);
    println!("   • Regex searches: {}", stats.repl_stats.regex_searches);
    println!(
        "   • Keyword searches: {}",
        stats.repl_stats.keyword_searches
    );

    println!("\n✅ Demo complete! RLM successfully processed context beyond");
    println!("   typical model context windows using recursive decomposition.");

    Ok(())
}

/// Generate synthetic research corpus content
fn generate_synthetic_context(target_size: usize) -> String {
    let mut content = String::with_capacity(target_size);
    let mut section = 1;

    let topics = [
        "quantum computing",
        "neural networks",
        "machine learning",
        "deep learning",
        "artificial intelligence",
        "natural language processing",
        "computer vision",
        "reinforcement learning",
        "transformer architecture",
        "attention mechanisms",
    ];

    while content.len() < target_size {
        let topic = topics[section % topics.len()];
        content.push_str(&format!(
            "\n=== Section {} ===\n\
            This section discusses {} and its applications.\n\
            The field of {} has seen remarkable progress in recent years.\n\
            Key developments include advances in {} algorithms and methodologies.\n\
            Researchers have demonstrated that {} can achieve state-of-the-art results.\n\
            Future work in {} promises even more exciting breakthroughs.\n\
            The intersection of {} with other fields creates novel opportunities.\n\n",
            section, topic, topic, topic, topic, topic, topic
        ));
        section += 1;
    }

    content.truncate(target_size);
    content
}

/// Print context metadata
fn print_metadata(meta: &ContextMetadata) {
    println!("   Context loaded: '{}'", meta.name);
    println!("   Total characters: {}", meta.total_chars);
    println!("   Number of chunks: {}", meta.num_chunks);
    println!(
        "   Chunk sizes: {:?}...",
        &meta.chunk_sizes[..meta.chunk_sizes.len().min(5)]
    );
}

/// Print RLM response
fn print_response(response: &RlmResponse) {
    println!("📤 Response:");
    println!("   {}", response.answer);
    println!("\n🔄 Trajectory ({} steps):", response.trajectory.len());
    for (i, step) in response.trajectory.iter().enumerate().take(5) {
        println!("   {}. {} → {}", i + 1, step.action, step.result);
    }
    if response.trajectory.len() > 5 {
        println!("   ... ({} more steps)", response.trajectory.len() - 5);
    }
    println!("\n📊 Stats:");
    println!("   • Duration: {:?}", response.stats.duration);
    println!("   • Root calls: {}", response.stats.root_calls);
    println!("   • Sub-calls: {}", response.stats.sub_calls);
    println!("   • Chunks processed: {}", response.stats.chunks_processed);
    println!("   • Estimated cost: ${:.4}", response.stats.total_cost);
}
