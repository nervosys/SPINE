//! Demonstration of infinite context handling with Recursive Language Models
//!
//! This example demonstrates processing 10M+ character contexts that would be
//! impossible with traditional LLM approaches.

use spine_recursive::{MockSubLlmDispatcher, RecursiveLM, ReplEnvironment, RlmConfig};
use std::sync::Arc;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║         Infinite Context Processing with RLM                  ║");
    println!("║         Processing 10M+ Characters Without Context Rot        ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    // Scale test - increasing context sizes
    let context_sizes = vec![
        100_000,    // 100K chars (~25K tokens)
        500_000,    // 500K chars (~125K tokens)
        1_000_000,  // 1M chars (~250K tokens)
        5_000_000,  // 5M chars (~1.25M tokens)
        10_000_000, // 10M chars (~2.5M tokens) - Beyond any model context window!
    ];

    let root_dispatcher = Arc::new(MockSubLlmDispatcher::new("gpt-5"));
    let sub_dispatcher = Arc::new(MockSubLlmDispatcher::new("gpt-5-mini"));

    println!("Testing RLM scalability across context sizes:\n");
    println!("┌────────────────┬───────────────┬──────────────┬───────────────┬────────────┐");
    println!("│ Context Size   │ Est. Tokens   │ Load Time    │ Query Time    │ Sub-calls  │");
    println!("├────────────────┼───────────────┼──────────────┼───────────────┼────────────┤");

    for size in context_sizes {
        let config = RlmConfig {
            max_recursion_depth: 2,
            default_chunk_size: 200_000, // 200K chars per chunk as recommended
            max_context_size: 50_000_000, // 50M chars limit
            ..Default::default()
        };
        let chunk_size = config.default_chunk_size;

        let rlm = RecursiveLM::new(config, root_dispatcher.clone(), sub_dispatcher.clone());

        // Generate content
        let content = generate_varied_content(size);

        // Load context
        let load_start = Instant::now();
        let meta = rlm.load_context("massive_doc", content).await?;
        let load_time = load_start.elapsed();

        // Query - this would fail with traditional LLMs for large contexts
        let query_start = Instant::now();
        let _response = rlm
            .query("Find information about quantum computing")
            .await?;
        let query_time = query_start.elapsed();

        let stats = rlm.stats();

        println!(
            "│ {:>12}   │ {:>11}   │ {:>10.2?}   │ {:>11.2?}   │ {:>8}   │",
            format_size(size),
            format_tokens(size / 4),
            load_time,
            query_time,
            stats.repl_stats.sub_llm_calls
        );

        // For very large contexts, also show chunk info
        if size >= 1_000_000 {
            println!(
                "│   └─ {} chunks × ~{}K chars each                                      │",
                meta.num_chunks,
                chunk_size / 1000
            );
        }
    }

    println!("└────────────────┴───────────────┴──────────────┴───────────────┴────────────┘");

    // Demonstrate REPL operations on massive context
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("📚 REPL Operations on 10M Character Context");
    println!("═══════════════════════════════════════════════════════════════\n");

    let dispatcher = Arc::new(MockSubLlmDispatcher::new("model"));
    let repl = ReplEnvironment::new(dispatcher, 3);

    // Load a 10M character context
    let huge_content = generate_varied_content(10_000_000);
    println!("Loading 10M characters into REPL...");
    let start = Instant::now();
    let meta = repl.load_context("huge_doc", huge_content, 200_000).await?;
    println!("✅ Loaded in {:?}", start.elapsed());
    println!("   • Total chars: {}", meta.total_chars);
    println!("   • Chunks: {}", meta.num_chunks);

    // Demonstrate O(1) chunk access
    println!("\n🔍 Random chunk access (should be O(1)):");
    let chunk_indices = [
        0,
        meta.num_chunks / 4,
        meta.num_chunks / 2,
        meta.num_chunks - 1,
    ];

    for &idx in &chunk_indices {
        let start = Instant::now();
        let chunk = repl.get_chunk("huge_doc", idx).await?;
        let duration = start.elapsed();
        println!("   Chunk {}: {} chars in {:?}", idx, chunk.len(), duration);
    }

    // Demonstrate keyword search across massive context
    println!("\n🔎 Keyword search across 10M chars:");
    let start = Instant::now();
    let results = repl.search_keyword("huge_doc", "quantum").await?;
    println!(
        "   Found {} chunks containing 'quantum' in {:?}",
        results.len(),
        start.elapsed()
    );

    // Demonstrate regex search
    println!("\n🔎 Regex search across 10M chars:");
    let start = Instant::now();
    let results = repl.search_regex("huge_doc", r"Section \d+").await?;
    println!(
        "   Found {} chunks matching 'Section \\d+' in {:?}",
        results.len(),
        start.elapsed()
    );

    // Show final stats
    println!("\n📊 Final REPL Statistics:");
    let stats = repl.get_stats();
    println!("   • Total chars loaded: {}", stats.total_chars_loaded);
    println!("   • Chunks accessed: {}", stats.chunks_accessed);
    println!("   • Keyword searches: {}", stats.keyword_searches);
    println!("   • Regex searches: {}", stats.regex_searches);
    println!("   • Sub-LLM calls: {}", stats.sub_llm_calls);

    println!("\n✅ Successfully demonstrated infinite context processing!");
    println!("   Traditional LLMs would fail on 10M char input.");
    println!("   RLM handles it by treating context as environment, not neural input.");

    Ok(())
}

/// Generate varied content with searchable patterns
fn generate_varied_content(target_size: usize) -> String {
    let mut content = String::with_capacity(target_size);
    let mut section = 1;

    let topics = [
        (
            "quantum computing",
            "revolutionizing cryptography and optimization",
        ),
        ("neural networks", "mimicking biological brain structures"),
        ("machine learning", "enabling pattern recognition at scale"),
        ("deep learning", "achieving superhuman performance on tasks"),
        (
            "natural language",
            "understanding and generating human text",
        ),
        ("computer vision", "interpreting visual information"),
        ("reinforcement learning", "learning through trial and error"),
        ("transformers", "attention-based sequence modeling"),
        ("generative AI", "creating novel content"),
        ("AI safety", "ensuring beneficial AI development"),
    ];

    while content.len() < target_size {
        let (topic, desc) = topics[section % topics.len()];

        // Add varied content structure
        if section % 10 == 0 {
            content.push_str(&format!(
                "\n\n{}\nChapter {}: Major Developments in {}\n{}\n\n",
                "=".repeat(60),
                section / 10,
                topic,
                "=".repeat(60)
            ));
        }

        content.push_str(&format!(
            "Section {}: {}\n\
            The field of {} focuses on {}.\n\
            Recent advances in {} have shown promising results.\n\
            Key researchers in {} have published groundbreaking work.\n\
            Applications of {} span multiple industries.\n\
            Future developments in {} will likely transform society.\n\n",
            section, topic, topic, desc, topic, topic, topic, topic
        ));

        // Add some data patterns
        if section % 5 == 0 {
            content.push_str(&format!(
                "Data Point {}: accuracy={}.{}%, latency={}ms\n",
                section,
                90 + (section % 10),
                section % 100,
                10 + (section % 50)
            ));
        }

        section += 1;
    }

    content.truncate(target_size);
    content
}

fn format_size(size: usize) -> String {
    if size >= 1_000_000 {
        format!("{}M", size / 1_000_000)
    } else if size >= 1_000 {
        format!("{}K", size / 1_000)
    } else {
        format!("{}", size)
    }
}

fn format_tokens(tokens: usize) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{}K", tokens / 1_000)
    } else {
        format!("{}", tokens)
    }
}
