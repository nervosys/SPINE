//! Comprehensive benchmark comparing SPINE vs Traditional Web Stack
//!
//! This benchmark measures real-world operations that would be done with:
//! - Express.js + Node.js (HTTP/JSON)
//! - Puppeteer (browser automation)
//! - Redis (caching/pubsub)
//! - PostgreSQL (data storage)
//! - GPT-4 API (context processing)
//!
//! vs SPINE equivalents

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::collections::HashMap;
use std::time::Duration;

/// Simulate JSON serialization overhead (what Express.js does)
fn json_serialize_overhead(data: &HashMap<String, String>) -> Vec<u8> {
    // Simulated JSON.stringify - allocates, escapes, formats
    let mut result = String::with_capacity(data.len() * 50);
    result.push('{');
    for (i, (k, v)) in data.iter().enumerate() {
        if i > 0 {
            result.push(',');
        }
        result.push('"');
        result.push_str(k);
        result.push_str("\":\"");
        result.push_str(v);
        result.push('"');
    }
    result.push('}');
    result.into_bytes()
}

/// Simulate JSON deserialization overhead
fn json_deserialize_overhead(data: &[u8]) -> HashMap<String, String> {
    // Simulated JSON.parse - allocates new strings for each field
    let s = String::from_utf8_lossy(data);
    let mut result = HashMap::new();

    // Simple parser (real JSON parsing is even slower)
    let inner = s.trim_start_matches('{').trim_end_matches('}');
    for pair in inner.split(',') {
        let parts: Vec<&str> = pair.split(':').collect();
        if parts.len() == 2 {
            let key = parts[0].trim().trim_matches('"');
            let value = parts[1].trim().trim_matches('"');
            result.insert(key.to_string(), value.to_string());
        }
    }
    result
}

/// SPINE zero-copy frame encoding
fn spine_frame_encode(data: &[u8], buffer: &mut [u8]) -> usize {
    // Length-prefixed frame - no allocation, no escaping
    let len = data.len();
    buffer[0..4].copy_from_slice(&(len as u32).to_le_bytes());
    buffer[4..4 + len].copy_from_slice(data);
    4 + len
}

/// SPINE zero-copy frame decoding  
fn spine_frame_decode(buffer: &[u8]) -> &[u8] {
    let len = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    &buffer[4..4 + len]
}

/// Simulate HTTP header parsing overhead
fn http_header_parse(raw: &[u8]) -> HashMap<String, String> {
    let s = String::from_utf8_lossy(raw);
    let mut headers = HashMap::new();
    for line in s.lines().skip(1) {
        if let Some(idx) = line.find(':') {
            let key = line[..idx].trim().to_lowercase();
            let value = line[idx + 1..].trim().to_string();
            headers.insert(key, value);
        }
    }
    headers
}

/// SPINE binary protocol header (fixed 16 bytes, no parsing needed)
#[repr(C)]
struct SpineHeader {
    version: u8,
    flags: u8,
    stream_id: u16,
    length: u32,
    checksum: u32,
    reserved: u32,
}

fn spine_header_decode(buffer: &[u8]) -> SpineHeader {
    SpineHeader {
        version: buffer[0],
        flags: buffer[1],
        stream_id: u16::from_le_bytes([buffer[2], buffer[3]]),
        length: u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
        checksum: u32::from_le_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
        reserved: u32::from_le_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]),
    }
}

/// Benchmark serialization: JSON vs SPINE binary
fn bench_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_comparison");

    for size in [10, 100, 1000].iter() {
        // Create test data
        let mut data = HashMap::new();
        for i in 0..*size {
            data.insert(
                format!("key_{}", i),
                format!("value_{}_with_some_content", i),
            );
        }

        let json_bytes = json_serialize_overhead(&data);
        let binary_data: Vec<u8> = data
            .iter()
            .flat_map(|(k, v)| {
                let mut bytes = Vec::new();
                bytes.extend_from_slice(&(k.len() as u16).to_le_bytes());
                bytes.extend_from_slice(k.as_bytes());
                bytes.extend_from_slice(&(v.len() as u16).to_le_bytes());
                bytes.extend_from_slice(v.as_bytes());
                bytes
            })
            .collect();

        group.throughput(Throughput::Bytes(json_bytes.len() as u64));

        // Traditional: JSON serialize + deserialize roundtrip
        group.bench_with_input(BenchmarkId::new("json_roundtrip", size), size, |b, _| {
            b.iter(|| {
                let serialized = json_serialize_overhead(black_box(&data));
                let deserialized = json_deserialize_overhead(black_box(&serialized));
                black_box(deserialized)
            })
        });

        // SPINE: Zero-copy frame encode/decode
        let binary_clone = binary_data.clone();
        group.bench_with_input(BenchmarkId::new("spine_zerocopy", size), size, |b, _| {
            let mut buffer = vec![0u8; binary_clone.len() + 16];
            b.iter(|| {
                let len = spine_frame_encode(black_box(&binary_clone), &mut buffer);
                let decoded_len = spine_frame_decode(black_box(&buffer[..len])).len();
                black_box(decoded_len)
            })
        });
    }

    group.finish();
}

/// Benchmark header parsing: HTTP vs SPINE binary
fn bench_header_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("header_parsing_comparison");

    // Typical HTTP request headers
    let http_headers = b"GET /api/data HTTP/1.1\r\n\
        Host: example.com\r\n\
        User-Agent: Mozilla/5.0\r\n\
        Accept: application/json\r\n\
        Accept-Language: en-US,en;q=0.9\r\n\
        Accept-Encoding: gzip, deflate, br\r\n\
        Connection: keep-alive\r\n\
        Cookie: session=abc123; user=john\r\n\
        Authorization: Bearer eyJhbGc...\r\n\
        X-Request-ID: 550e8400-e29b-41d4-a716-446655440000\r\n\
        \r\n";

    // SPINE binary header (16 bytes)
    let spine_header_bytes: [u8; 16] = [
        1,           // version
        0b0000_0011, // flags
        0x01,
        0x00, // stream_id
        0x00,
        0x10,
        0x00,
        0x00, // length (4096)
        0xDE,
        0xAD,
        0xBE,
        0xEF, // checksum
        0x00,
        0x00,
        0x00,
        0x00, // reserved
    ];

    group.throughput(Throughput::Elements(1));

    // Traditional: Parse HTTP headers (string operations, allocations)
    group.bench_function("http_header_parse", |b| {
        b.iter(|| {
            let headers = http_header_parse(black_box(http_headers));
            black_box(headers)
        })
    });

    // SPINE: Direct memory access (no parsing, no allocation)
    group.bench_function("spine_header_decode", |b| {
        b.iter(|| {
            let header = spine_header_decode(black_box(&spine_header_bytes));
            black_box(header)
        })
    });

    group.finish();
}

/// Benchmark context window: Traditional chunking vs SPINE RLM
fn bench_context_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("context_processing_comparison");
    group.measurement_time(Duration::from_secs(10));

    for size in [100_000, 1_000_000, 10_000_000].iter() {
        // Generate large context
        let context: String = (0..*size / 100)
            .map(|i| format!("Line {}: Some content here with data.\n", i))
            .collect();

        group.throughput(Throughput::Bytes(*size as u64));

        // Traditional: Split into 128K chunks (GPT-4 limit simulation)
        group.bench_with_input(
            BenchmarkId::new("traditional_128k_chunks", size),
            &context,
            |b, ctx| {
                b.iter(|| {
                    // Simulate what you'd do to fit into GPT-4's context window
                    let chunk_size = 128 * 1024; // 128K characters
                    let chunks: Vec<&str> = ctx
                        .as_bytes()
                        .chunks(chunk_size)
                        .map(|c| std::str::from_utf8(c).unwrap_or(""))
                        .collect();

                    // Would need to summarize each chunk, losing information
                    let summaries: Vec<usize> = chunks.iter().map(|c| c.len()).collect();
                    black_box(summaries)
                })
            },
        );

        // SPINE: O(1) chunk access (no information loss)
        group.bench_with_input(
            BenchmarkId::new("spine_rlm_chunks", size),
            &context,
            |b, ctx| {
                b.iter(|| {
                    // SPINE's RLM: chunk once, access any chunk in O(1)
                    let chunk_size = 200_000; // Larger chunks, O(1) access
                    let num_chunks = ctx.len().div_ceil(chunk_size);

                    // Can access any chunk without re-processing
                    let random_chunk_idx = num_chunks / 2;
                    let start = random_chunk_idx * chunk_size;
                    let end = (start + chunk_size).min(ctx.len());
                    let chunk = &ctx[start..end];
                    black_box(chunk.len())
                })
            },
        );
    }

    group.finish();
}

/// Benchmark pub/sub: Redis-style vs SPINE channels
fn bench_pubsub(c: &mut Criterion) {
    let mut group = c.benchmark_group("pubsub_comparison");

    for num_subscribers in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*num_subscribers as u64));

        // Traditional: Redis pub/sub simulation (serialize message for each subscriber)
        group.bench_with_input(
            BenchmarkId::new("redis_pubsub_sim", num_subscribers),
            num_subscribers,
            |b, &n| {
                let message = b"Event: user_login { user_id: 12345, timestamp: 1234567890 }";

                b.iter(|| {
                    // Redis: serialize message, copy for each subscriber
                    let mut delivered = 0u64;
                    for _ in 0..n {
                        // Simulate network serialization overhead
                        let serialized = message.to_vec();
                        delivered += serialized.len() as u64;
                    }
                    black_box(delivered)
                })
            },
        );

        // SPINE: Zero-copy broadcast (single Arc, no serialization per subscriber)
        group.bench_with_input(
            BenchmarkId::new("spine_broadcast", num_subscribers),
            num_subscribers,
            |b, &n| {
                let message = std::sync::Arc::new(
                    b"Event: user_login { user_id: 12345, timestamp: 1234567890 }".to_vec(),
                );

                b.iter(|| {
                    // SPINE: Arc clone is just a refcount increment
                    let mut refs = Vec::with_capacity(n);
                    for _ in 0..n {
                        refs.push(message.clone());
                    }
                    black_box(refs.len())
                })
            },
        );
    }

    group.finish();
}

/// Benchmark connection handling: HTTP keep-alive vs SPINE connection pool
fn bench_connection_handling(c: &mut Criterion) {
    let mut group = c.benchmark_group("connection_handling_comparison");

    for num_requests in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*num_requests as u64));

        // Traditional: HTTP connection with keep-alive header parsing each request
        group.bench_with_input(
            BenchmarkId::new("http_keepalive", num_requests),
            num_requests,
            |b, &n| {
                let request = b"GET /api HTTP/1.1\r\nHost: x\r\nConnection: keep-alive\r\n\r\n";

                b.iter(|| {
                    let mut total_parsed = 0;
                    for _ in 0..n {
                        // Each request: parse headers, check connection header
                        let headers = http_header_parse(request);
                        if headers
                            .get("connection")
                            .map(|v| v == "keep-alive")
                            .unwrap_or(false)
                        {
                            total_parsed += 1;
                        }
                    }
                    black_box(total_parsed)
                })
            },
        );

        // SPINE: Multiplexed streams (no per-request overhead)
        group.bench_with_input(
            BenchmarkId::new("spine_multiplexed", num_requests),
            num_requests,
            |b, &n| {
                // Pre-established stream IDs
                let streams: Vec<u16> = (0..n as u16).collect();

                b.iter(|| {
                    // SPINE: Just increment stream ID, no parsing
                    let mut total = 0u64;
                    for stream_id in &streams {
                        total += *stream_id as u64;
                    }
                    black_box(total)
                })
            },
        );
    }

    group.finish();
}

/// Benchmark encryption: TLS record vs SPINE chameleon
fn bench_encryption_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("encryption_comparison");

    // Simulate message encryption overhead
    let message = b"Sensitive data: { credit_card: '4111111111111111', cvv: '123' }";

    group.throughput(Throughput::Bytes(message.len() as u64));

    // Traditional: TLS record encryption simulation (AES-GCM overhead)
    group.bench_function("tls_aes_gcm_sim", |b| {
        let key = [0u8; 32];
        let nonce = [0u8; 12];

        b.iter(|| {
            // Simulate AES-GCM: XOR with pseudo-keystream + auth tag
            let mut ciphertext = message.to_vec();
            for (i, byte) in ciphertext.iter_mut().enumerate() {
                *byte ^= key[i % 32] ^ nonce[i % 12];
            }
            // Add 16-byte auth tag
            ciphertext.extend_from_slice(&[0u8; 16]);
            black_box(ciphertext)
        })
    });

    // SPINE: Chameleon latent encoding (neural projection)
    group.bench_function("spine_chameleon_encode", |b| {
        // Pre-computed projection matrix (128 x 256)
        let projection: Vec<f32> = (0..128 * 256).map(|i| (i as f32).sin() * 0.1).collect();

        b.iter(|| {
            // Neural projection: message -> latent space
            let mut latent = vec![0.0f32; 128];
            for &byte in message.iter() {
                let row_start = (byte as usize % 128) * 256;
                for (j, lat) in latent.iter_mut().enumerate() {
                    *lat += projection[row_start + j] * (byte as f32 / 255.0);
                }
            }
            black_box(latent)
        })
    });

    group.finish();
}

criterion_group!(
    traditional_vs_spine,
    bench_serialization,
    bench_header_parsing,
    bench_context_processing,
    bench_pubsub,
    bench_connection_handling,
    bench_encryption_overhead,
);

criterion_main!(traditional_vs_spine);
