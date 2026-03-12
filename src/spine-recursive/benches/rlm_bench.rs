//! Benchmarks for Recursive Language Model operations
//!
//! Tests performance of:
//! - Context loading and chunking
//! - Chunk access patterns
//! - Search operations (keyword, regex)
//! - Sub-LLM call overhead

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use spine_recursive::{
    ContextVariable, MockSubLlmDispatcher, RecursiveLM, ReplEnvironment, RlmConfig,
};
use std::sync::Arc;
use tokio::runtime::Runtime;

fn bench_context_chunking(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_chunking");

    let sizes = [10_000, 100_000, 1_000_000, 10_000_000];

    for size in sizes {
        group.throughput(Throughput::Bytes(size as u64));

        let content = generate_content(size);

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}K", size / 1000)),
            &content,
            |b, content| {
                b.iter(|| black_box(ContextVariable::new("test", content.clone(), 200_000)));
            },
        );
    }

    group.finish();
}

fn bench_chunk_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunk_access");

    // Create a large context variable
    let content = generate_content(10_000_000);
    let var = ContextVariable::new("large", content, 200_000);
    let num_chunks = var.num_chunks();

    // Sequential access
    group.bench_function("sequential", |b| {
        let mut idx = 0;
        b.iter(|| {
            let chunk = black_box(var.get_chunk(idx));
            idx = (idx + 1) % num_chunks;
            chunk
        });
    });

    // Random access
    group.bench_function("random", |b| {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        b.iter(|| {
            let idx = rng.gen_range(0..num_chunks);
            black_box(var.get_chunk(idx))
        });
    });

    group.finish();
}

fn bench_keyword_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("keyword_search");

    let sizes = [100_000, 1_000_000, 10_000_000];

    for size in sizes {
        let content = generate_content_with_keywords(size);
        let var = ContextVariable::new("search_test", content, 200_000);

        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}K", size / 1000)),
            &var,
            |b, var| {
                b.iter(|| black_box(var.search_keyword("quantum")));
            },
        );
    }

    group.finish();
}

fn bench_regex_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("regex_search");

    let sizes = [100_000, 1_000_000];

    for size in sizes {
        let content = generate_content_with_patterns(size);
        let var = ContextVariable::new("regex_test", content, 200_000);

        group.throughput(Throughput::Bytes(size as u64));

        // Simple regex
        group.bench_with_input(
            BenchmarkId::new("simple", format!("{}K", size / 1000)),
            &var,
            |b, var| {
                b.iter(|| black_box(var.search_regex(r"Section \d+")));
            },
        );

        // Complex regex
        group.bench_with_input(
            BenchmarkId::new("complex", format!("{}K", size / 1000)),
            &var,
            |b, var| {
                b.iter(|| black_box(var.search_regex(r"accuracy=\d+\.\d+%")));
            },
        );
    }

    group.finish();
}

fn bench_repl_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("repl_operations");

    let dispatcher = Arc::new(MockSubLlmDispatcher::new("mock"));
    let repl = Arc::new(rt.block_on(async {
        let repl = ReplEnvironment::new(dispatcher, 3);
        let content = generate_content(1_000_000);
        repl.load_context("doc", content, 200_000).await.unwrap();
        repl
    }));

    // Chunk retrieval
    group.bench_function("get_chunk", |b| {
        let repl = repl.clone();
        b.to_async(&rt)
            .iter(|| async { black_box(repl.get_chunk("doc", 0).await) });
    });

    // Keyword search
    group.bench_function("search_keyword", |b| {
        let repl = repl.clone();
        b.to_async(&rt)
            .iter(|| async { black_box(repl.search_keyword("doc", "content").await) });
    });

    // Get lines
    group.bench_function("get_lines", |b| {
        let repl = repl.clone();
        b.to_async(&rt)
            .iter(|| async { black_box(repl.get_lines("doc", 0, 100).await) });
    });

    group.finish();
}

fn bench_rlm_query(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("rlm_query");
    group.sample_size(20); // Fewer samples due to async overhead

    let root = Arc::new(MockSubLlmDispatcher::new("root"));
    let sub = Arc::new(MockSubLlmDispatcher::new("sub"));

    let rlm = Arc::new(rt.block_on(async {
        let config = RlmConfig {
            max_recursion_depth: 2,
            default_chunk_size: 100_000,
            ..Default::default()
        };
        let rlm = RecursiveLM::new(config, root, sub);
        let content = generate_content(500_000);
        rlm.load_context("doc", content).await.unwrap();
        rlm
    }));

    group.bench_function("search_query", |b| {
        let rlm = rlm.clone();
        b.to_async(&rt)
            .iter(|| async { black_box(rlm.query("find quantum computing").await) });
    });

    group.bench_function("count_query", |b| {
        let rlm = rlm.clone();
        b.to_async(&rt)
            .iter(|| async { black_box(rlm.query("count mentions of neural").await) });
    });

    group.finish();
}

// Helper functions
fn generate_content(size: usize) -> String {
    let base = "This is test content for benchmarking. ";
    base.repeat(size / base.len() + 1)[..size].to_string()
}

fn generate_content_with_keywords(size: usize) -> String {
    let mut content = String::with_capacity(size);
    let mut i = 0;
    while content.len() < size {
        if i % 100 == 0 {
            content.push_str("quantum computing is revolutionary. ");
        } else {
            content.push_str("regular content without special keywords. ");
        }
        i += 1;
    }
    content.truncate(size);
    content
}

fn generate_content_with_patterns(size: usize) -> String {
    let mut content = String::with_capacity(size);
    let mut i = 0;
    while content.len() < size {
        content.push_str(&format!(
            "Section {}: accuracy={}.{}% performance data. ",
            i,
            90 + (i % 10),
            i % 100
        ));
        i += 1;
    }
    content.truncate(size);
    content
}

criterion_group!(
    benches,
    bench_context_chunking,
    bench_chunk_access,
    bench_keyword_search,
    bench_regex_search,
    bench_repl_operations,
    bench_rlm_query,
);

criterion_main!(benches);
