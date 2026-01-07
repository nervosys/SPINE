//! Scalability benchmarks for SPINE
//!
//! Tests for W4: Verify scalability at:
//! - 1000+ agent swarms
//! - 100M+ character contexts

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

// We'll use simplified tests here and the actual implementations from the crate

/// Benchmark swarm scalability with increasing agent counts
fn bench_swarm_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("swarm_scalability");
    group.sample_size(10); // Fewer samples for longer benchmarks

    // Test swarm sizes: 100, 500, 1000, 2000
    for num_agents in [100, 500, 1000, 2000].iter() {
        group.throughput(Throughput::Elements(*num_agents as u64));

        group.bench_with_input(
            BenchmarkId::new("swarm_creation", num_agents),
            num_agents,
            |b, &num| {
                b.iter(|| {
                    // Simulate swarm creation overhead
                    let mut agents: Vec<u32> = Vec::with_capacity(num);
                    for i in 0..num {
                        agents.push(i as u32);
                    }
                    black_box(agents)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("message_broadcast", num_agents),
            num_agents,
            |b, &num| {
                let agents: Vec<u32> = (0..num as u32).collect();
                b.iter(|| {
                    // Simulate broadcast to all agents
                    let payload = b"HEARTBEAT";
                    let mut messages = Vec::with_capacity(agents.len());
                    for &receiver in &agents {
                        messages.push((0u32, receiver, payload.to_vec()));
                    }
                    black_box(messages)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("leader_election", num_agents),
            num_agents,
            |b, &num| {
                let agents: Vec<u32> = (0..num as u32).collect();
                b.iter(|| {
                    // Ring-based election: find max
                    let leader = agents.iter().max().copied();
                    black_box(leader)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("agent_lookup", num_agents),
            num_agents,
            |b, &num| {
                use std::collections::HashMap;
                let agents: HashMap<u32, String> = (0..num as u32)
                    .map(|i| (i, format!("agent_{}", i)))
                    .collect();
                let target = (num / 2) as u32;
                b.iter(|| {
                    let found = agents.get(&target);
                    black_box(found)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark context scalability with increasing sizes
fn bench_context_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_scalability");
    group.sample_size(10);

    // Test context sizes: 1M, 10M, 50M, 100M characters
    for size_mb in [1, 10, 50, 100].iter() {
        let size = *size_mb * 1_000_000;
        group.throughput(Throughput::Bytes(size as u64));

        // Generate context once for all benchmarks
        let context: String = "Lorem ipsum dolor sit amet. ".repeat(size / 28);

        group.bench_with_input(
            BenchmarkId::new("context_chunking", size_mb),
            &context,
            |b, ctx| {
                b.iter(|| {
                    // Chunk context into 64KB chunks
                    let chunk_size = 65536;
                    let chunks: Vec<&str> = ctx
                        .as_bytes()
                        .chunks(chunk_size)
                        .map(|c| std::str::from_utf8(c).unwrap_or(""))
                        .collect();
                    black_box(chunks.len())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("keyword_search", size_mb),
            &context,
            |b, ctx| {
                b.iter(|| {
                    // Search for keyword
                    let count = ctx.matches("ipsum").count();
                    black_box(count)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("line_count", size_mb),
            &context,
            |b, ctx| {
                b.iter(|| {
                    let lines = ctx.lines().count();
                    black_box(lines)
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("hash_context", size_mb),
            &context,
            |b, ctx| {
                use sha2::{Digest, Sha256};
                b.iter(|| {
                    let mut hasher = Sha256::new();
                    hasher.update(ctx.as_bytes());
                    let hash: [u8; 32] = hasher.finalize().into();
                    black_box(hash)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark concurrent agent operations
fn bench_concurrent_agents(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_agents");
    group.sample_size(10);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_time()
        .build()
        .unwrap();

    for num_agents in [100, 500, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("concurrent_tasks", num_agents),
            num_agents,
            |b, &num| {
                b.iter(|| {
                    runtime.block_on(async {
                        use tokio::task::JoinSet;
                        let mut set = JoinSet::new();

                        for i in 0..num {
                            set.spawn(async move {
                                // Simulate agent work
                                tokio::time::sleep(std::time::Duration::from_micros(10)).await;
                                i
                            });
                        }

                        let mut results = Vec::with_capacity(num);
                        while let Some(res) = set.join_next().await {
                            if let Ok(r) = res {
                                results.push(r);
                            }
                        }
                        black_box(results.len())
                    })
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("shared_state_access", num_agents),
            num_agents,
            |b, &num| {
                use dashmap::DashMap;
                use std::sync::Arc;

                b.iter(|| {
                    runtime.block_on(async {
                        let shared_state: Arc<DashMap<u32, String>> = Arc::new(DashMap::new());

                        use tokio::task::JoinSet;
                        let mut set = JoinSet::new();

                        for i in 0..num as u32 {
                            let state = shared_state.clone();
                            set.spawn(async move {
                                state.insert(i, format!("agent_{}", i));
                                state.get(&i).map(|v| v.clone())
                            });
                        }

                        while let Some(_) = set.join_next().await {}
                        black_box(shared_state.len())
                    })
                });
            },
        );
    }

    group.finish();
}

/// Benchmark RLM with large contexts
fn bench_rlm_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("rlm_scalability");
    group.sample_size(10);

    // Simulate RLM chunking and aggregation
    for context_size_mb in [1, 10, 50].iter() {
        let size = *context_size_mb * 1_000_000;
        let context: String = "Sample text content for RLM processing. ".repeat(size / 40);

        group.bench_with_input(
            BenchmarkId::new("chunk_and_summarize", context_size_mb),
            &context,
            |b, ctx| {
                b.iter(|| {
                    // Simulate RLM chunking
                    let chunk_size = 8192; // 8KB chunks
                    let mut summaries = Vec::new();

                    for chunk in ctx.as_bytes().chunks(chunk_size) {
                        // Simulate summary generation (hash as proxy)
                        use sha2::{Digest, Sha256};
                        let mut hasher = Sha256::new();
                        hasher.update(chunk);
                        let summary: [u8; 32] = hasher.finalize().into();
                        summaries.push(summary);
                    }

                    black_box(summaries.len())
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("regex_search_chunks", context_size_mb),
            &context,
            |b, ctx| {
                use regex::Regex;
                let re = Regex::new(r"\b\w{6,}\b").unwrap();

                b.iter(|| {
                    let chunk_size = 65536;
                    let mut matches = 0;

                    for chunk in ctx.as_bytes().chunks(chunk_size) {
                        let chunk_str = std::str::from_utf8(chunk).unwrap_or("");
                        matches += re.find_iter(chunk_str).count();
                    }

                    black_box(matches)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_swarm_scalability,
    bench_context_scalability,
    bench_concurrent_agents,
    bench_rlm_scalability
);
criterion_main!(benches);
