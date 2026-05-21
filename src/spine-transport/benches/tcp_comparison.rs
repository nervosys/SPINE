//! **NOT A FAIR BENCHMARK — category error.**
//!
//! Audited 2026-05: the `std_tcp` rows do real loopback `read`/`write` syscalls,
//! while the `SPINE_frame` rows do **pure in-memory** `codec.encode`/`decode`
//! with no socket touched. Apples-to-oranges; the resulting "SPINE is 500×
//! faster" claims in the original ROADMAP came from here and have been
//! retracted.
//!
//! Retained for history; do not cite. For honest measurements see
//! `network_realistic.rs` (both sides use real TCP) and `spine_vs_www.rs`
//! (both sides use real-protocol wire format).

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bytes::Bytes;
use spine_transport::{
    buffer::{HierarchicalAllocator, RingBuffer, SlabAllocator, VectoredBuffer},
    congestion::{BbrController, RateLimiter},
    frame::{FrameAggregator, FrameBuilder, FrameCodec},
    pool::PoolConfig,
    BufferAllocator, Frame, FrameFlags, FrameHeader,
};

static PORT_COUNTER: AtomicU16 = AtomicU16::new(30000);

fn get_port() -> u16 {
    // Wrap around to avoid running out of ports
    let port = PORT_COUNTER.fetch_add(1, Ordering::SeqCst);
    if port > 60000 {
        PORT_COUNTER.store(30000, Ordering::SeqCst);
    }
    port
}

/// Standard TCP echo server
fn spawn_tcp_echo_server(port: u16) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
        listener.set_nonblocking(false).unwrap();

        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = vec![0u8; 65536];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if stream.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    })
}

/// Create a test frame
fn make_frame(size: usize) -> Frame {
    Frame {
        header: FrameHeader {
            length: size as u32,
            flags: FrameFlags::empty(),
            sequence: 1,
            stream_id: 1,
            _reserved: 0,
        },
        payload: Bytes::from(vec![0xABu8; size]),
    }
}

/// Benchmark: Raw TCP roundtrip latency
fn bench_tcp_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency_comparison");

    for size in [64, 256, 1024, 4096].iter() {
        // Standard TCP latency
        group.bench_with_input(BenchmarkId::new("std_tcp", size), size, |b, &size| {
            let port = get_port();
            let _server = spawn_tcp_echo_server(port);
            thread::sleep(Duration::from_millis(50));

            let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
            stream.set_nodelay(true).unwrap();

            let data = vec![0xABu8; size];
            let mut recv_buf = vec![0u8; size];

            b.iter(|| {
                stream.write_all(&data).unwrap();
                stream.read_exact(&mut recv_buf).unwrap();
                black_box(&recv_buf);
            });
        });

        // SPINE frame encode/decode (simulated processing)
        group.bench_with_input(BenchmarkId::new("SPINE_frame", size), size, |b, &size| {
            let mut codec = FrameCodec::new(size * 2);
            let frame = make_frame(size);

            b.iter(|| {
                // Encode
                let encoded = codec.encode(&frame);

                // Decode
                let decoded = codec.decode(&encoded).unwrap();
                black_box(decoded);
            });
        });
    }

    group.finish();
}

/// Benchmark: Throughput comparison
fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_comparison");

    // Test different payload sizes
    for size in [1024, 8192, 65536, 262144].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        // Standard TCP throughput
        group.bench_with_input(BenchmarkId::new("std_tcp", size), size, |b, &size| {
            let port = get_port();
            let _server = spawn_tcp_echo_server(port);
            thread::sleep(Duration::from_millis(50));

            let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
            stream.set_nodelay(true).unwrap();

            let data = vec![0xABu8; size];
            let mut recv_buf = vec![0u8; size];

            b.iter(|| {
                stream.write_all(&data).unwrap();
                stream.read_exact(&mut recv_buf).unwrap();
                black_box(&recv_buf);
            });
        });

        // SPINE with zero-copy buffers
        group.bench_with_input(
            BenchmarkId::new("SPINE_zerocopy", size),
            size,
            |b, &size| {
                let ring = RingBuffer::new(size * 4);
                let data = vec![0xABu8; size];

                b.iter(|| {
                    // Write to ring buffer (zero-copy semantics)
                    ring.write(&data);

                    // Read from ring buffer
                    let mut out = vec![0u8; size];
                    ring.read(&mut out);
                    black_box(&out);
                });
            },
        );

        // SPINE frame codec
        group.bench_with_input(
            BenchmarkId::new("SPINE_frame_codec", size),
            size,
            |b, &size| {
                let mut codec = FrameCodec::new(size * 2);
                let frame = make_frame(size);

                b.iter(|| {
                    let encoded = codec.encode(&frame);
                    let decoded = codec.decode(&encoded).unwrap();
                    black_box(decoded);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Buffer allocation comparison
fn bench_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("allocation_comparison");

    for count in [100, 1000, 10000].iter() {
        // Standard Vec allocation
        group.bench_with_input(BenchmarkId::new("std_vec", count), count, |b, &count| {
            b.iter(|| {
                let mut buffers: Vec<Vec<u8>> = Vec::with_capacity(count);
                for _ in 0..count {
                    buffers.push(vec![0u8; 1024]);
                }
                black_box(buffers);
            });
        });

        // SPINE slab allocator
        group.bench_with_input(BenchmarkId::new("SPINE_slab", count), count, |b, &count| {
            b.iter(|| {
                let allocator = SlabAllocator::new(1024, count);
                let mut handles = Vec::with_capacity(count);
                for _ in 0..count {
                    let buf = allocator.borrow();
                    handles.push(buf);
                }
                black_box(handles);
            });
        });

        // SPINE hierarchical allocator
        group.bench_with_input(
            BenchmarkId::new("SPINE_hierarchical", count),
            count,
            |b, &count| {
                b.iter(|| {
                    let allocator = HierarchicalAllocator::new();
                    let mut handles = Vec::with_capacity(count);
                    for _ in 0..count {
                        let buf = allocator.allocate(1024);
                        handles.push(buf);
                    }
                    black_box(handles);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Frame aggregation vs naive concatenation
fn bench_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("aggregation_comparison");

    for frame_count in [10, 50, 100].iter() {
        // Naive concatenation
        group.bench_with_input(
            BenchmarkId::new("naive_concat", frame_count),
            frame_count,
            |b, &count| {
                let frames: Vec<Vec<u8>> = (0..count).map(|i| vec![i as u8; 100]).collect();

                b.iter(|| {
                    let mut result = Vec::new();
                    for frame in &frames {
                        result.extend_from_slice(frame);
                    }
                    black_box(result);
                });
            },
        );

        // SPINE frame aggregator
        group.bench_with_input(
            BenchmarkId::new("SPINE_aggregator", frame_count),
            frame_count,
            |b, &count| {
                let frames: Vec<Frame> = (0..count).map(|_| make_frame(100)).collect();

                b.iter(|| {
                    let mut aggregator = FrameAggregator::new(count * 200, count);
                    for frame in &frames {
                        aggregator.add(frame.clone());
                    }
                    let result = aggregator.take();
                    black_box(result);
                });
            },
        );

        // SPINE vectored buffer
        group.bench_with_input(
            BenchmarkId::new("SPINE_vectored", frame_count),
            frame_count,
            |b, &count| {
                let chunks: Vec<Vec<u8>> = (0..count).map(|i| vec![i as u8; 100]).collect();

                b.iter(|| {
                    let mut vbuf = VectoredBuffer::new();
                    for chunk in &chunks {
                        vbuf.push(Bytes::from(chunk.clone()));
                    }
                    let len = vbuf.len();
                    black_box(len);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Congestion control overhead
fn bench_congestion_control(c: &mut Criterion) {
    let mut group = c.benchmark_group("congestion_control");

    // No congestion control (baseline)
    group.bench_function("no_control", |b| {
        let mut bytes_sent = 0u64;
        b.iter(|| {
            bytes_sent += 1500; // MTU-sized packet
            black_box(bytes_sent);
        });
    });

    // SPINE BBR congestion control
    group.bench_function("SPINE_bbr", |b| {
        let mut bbr = BbrController::new();

        b.iter(|| {
            let pacing = bbr.pacing_rate();
            bbr.on_ack(1500, Duration::from_micros(100));
            black_box(pacing);
        });
    });

    // SPINE rate limiter
    group.bench_function("SPINE_rate_limiter", |b| {
        let mut limiter = RateLimiter::new(1_000_000_000, 10_000_000); // 1 Gbps, 10MB burst

        b.iter(|| {
            let allowed = limiter.try_consume(1500);
            black_box(allowed);
        });
    });

    group.finish();
}

/// Benchmark: Connection pool vs new connections
fn bench_connection_pool(c: &mut Criterion) {
    let mut group = c.benchmark_group("connection_pool");

    // Simulated new connection cost (TCP handshake is ~1-3ms typically)
    group.bench_function("new_connection_sim", |b| {
        b.iter(|| {
            // Simulate TCP handshake overhead (~1μs simulated, real is ~1ms)
            let start = Instant::now();
            while start.elapsed() < Duration::from_nanos(1000) {
                // Busy wait to simulate connection setup
            }
            black_box(start);
        });
    });

    // SPINE pool config creation (pool itself needs async runtime)
    group.bench_function("SPINE_pool_config", |b| {
        b.iter(|| {
            let config = PoolConfig::default();
            black_box(config);
        });
    });

    group.finish();
}

/// Benchmark: Frame codec vs raw serialization
fn bench_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization");

    for size in [64, 512, 4096, 32768].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        // Raw byte copy (baseline)
        group.bench_with_input(BenchmarkId::new("raw_copy", size), size, |b, &size| {
            let data = vec![0xABu8; size];
            let mut dest = vec![0u8; size + 12]; // Header overhead

            b.iter(|| {
                // Manual header
                dest[0..4].copy_from_slice(&(size as u32).to_le_bytes());
                dest[4..8].copy_from_slice(&1u32.to_le_bytes()); // stream_id
                dest[8..12].copy_from_slice(&0u32.to_le_bytes()); // flags
                dest[12..].copy_from_slice(&data);
                black_box(&dest);
            });
        });

        // SPINE frame codec
        group.bench_with_input(BenchmarkId::new("SPINE_codec", size), size, |b, &size| {
            let codec = FrameCodec::new(size * 2);
            let frame = make_frame(size);

            b.iter(|| {
                let encoded = codec.encode(&frame);
                black_box(encoded);
            });
        });

        // SPINE frame builder (optimized)
        group.bench_with_input(BenchmarkId::new("SPINE_builder", size), size, |b, &size| {
            let data = vec![0xABu8; size];

            b.iter(|| {
                let frame = FrameBuilder::new()
                    .stream_id(1)
                    .payload(Bytes::from(data.clone()))
                    .build();
                black_box(frame);
            });
        });
    }

    group.finish();
}

/// Summary benchmark: End-to-end comparison
fn bench_end_to_end(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end");
    group.sample_size(50); // Fewer samples for slower tests

    let payload_size = 4096;
    let iterations = 100;

    // Standard TCP roundtrip
    group.bench_function("std_tcp_100_msgs", |b| {
        let port = get_port();
        let _server = spawn_tcp_echo_server(port);
        thread::sleep(Duration::from_millis(50));

        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
        stream.set_nodelay(true).unwrap();

        let data = vec![0xABu8; payload_size];
        let mut recv_buf = vec![0u8; payload_size];

        b.iter(|| {
            for _ in 0..iterations {
                stream.write_all(&data).unwrap();
                stream.read_exact(&mut recv_buf).unwrap();
            }
            black_box(&recv_buf);
        });
    });

    // SPINE optimized pipeline
    group.bench_function("SPINE_100_msgs", |b| {
        let mut codec = FrameCodec::new(payload_size * 2);
        let ring = RingBuffer::new(payload_size * 4);
        let frame = make_frame(payload_size);

        b.iter(|| {
            for _ in 0..iterations {
                // Encode frame
                let encoded = codec.encode(&frame);

                // Zero-copy buffer operations
                ring.write(&encoded);
                let mut out = vec![0u8; encoded.len()];
                ring.read(&mut out);

                // Decode frame
                let decoded = codec.decode(&out).unwrap();
                black_box(decoded);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_tcp_latency,
    bench_throughput,
    bench_allocation,
    bench_aggregation,
    bench_congestion_control,
    bench_connection_pool,
    bench_serialization,
    bench_end_to_end,
);

criterion_main!(benches);
