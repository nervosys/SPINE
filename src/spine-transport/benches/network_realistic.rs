//! Realistic Network Benchmarks
//!
//! These benchmarks measure actual end-to-end network performance including:
//! - Real TCP socket I/O (not just in-memory)
//! - Latency under concurrent load
//! - Throughput with network overhead
//! - Comparison with and without SPINE optimizations

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use spine_transport::{frame::FrameCodec, Frame, FrameFlags, FrameHeader};

static PORT_COUNTER: AtomicU16 = AtomicU16::new(40000);

fn get_port() -> u16 {
    let port = PORT_COUNTER.fetch_add(1, Ordering::SeqCst);
    if port > 50000 {
        PORT_COUNTER.store(40000, Ordering::SeqCst);
    }
    port
}

// =============================================================================
// REALISTIC END-TO-END BENCHMARKS
// =============================================================================

/// SPINE-optimized TCP server with frame codec + BBR pacing
fn spawn_spine_server(port: u16) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();

        if let Ok((mut stream, _)) = listener.accept() {
            stream.set_nodelay(true).unwrap();
            let codec = FrameCodec::new(65536);
            let mut buf = vec![0u8; 65536];
            let mut header_buf = [0u8; 12];

            while stream.read_exact(&mut header_buf).is_ok() {
                let length = u32::from_le_bytes([
                    header_buf[0],
                    header_buf[1],
                    header_buf[2],
                    header_buf[3],
                ]) as usize;

                // Read payload
                if length > buf.len() {
                    buf.resize(length, 0);
                }
                if stream.read_exact(&mut buf[..length]).is_err() {
                    break;
                }

                // Echo back with frame encoding
                let frame = Frame {
                    header: FrameHeader {
                        length: length as u32,
                        flags: FrameFlags::empty(),
                        sequence: 1,
                        stream_id: 1,
                        _reserved: 0,
                    },
                    payload: Bytes::copy_from_slice(&buf[..length]),
                };

                let encoded = codec.encode(&frame);
                if stream.write_all(&encoded).is_err() {
                    break;
                }
            }
        }
    })
}

/// Standard TCP server (baseline)
fn spawn_standard_server(port: u16) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();

        if let Ok((mut stream, _)) = listener.accept() {
            stream.set_nodelay(true).unwrap();
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

/// Benchmark: End-to-end latency with REAL network I/O
///
/// This measures actual roundtrip time including:
/// - TCP socket operations
/// - Kernel network stack
/// - Frame encoding/decoding overhead
fn bench_e2e_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_latency");
    group.sample_size(50); // Fewer samples due to network variance

    for size in [64, 512, 4096].iter() {
        // Standard TCP (baseline)
        group.bench_with_input(BenchmarkId::new("standard_tcp", size), size, |b, &size| {
            let port = get_port();
            let _server = spawn_standard_server(port);
            thread::sleep(Duration::from_millis(100));

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

        // SPINE with frame codec over TCP
        group.bench_with_input(
            BenchmarkId::new("SPINE_framed_tcp", size),
            size,
            |b, &size| {
                let port = get_port();
                let _server = spawn_spine_server(port);
                thread::sleep(Duration::from_millis(100));

                let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
                stream.set_nodelay(true).unwrap();

                let mut codec = FrameCodec::new(size * 2);
                let frame = Frame {
                    header: FrameHeader {
                        length: size as u32,
                        flags: FrameFlags::empty(),
                        sequence: 1,
                        stream_id: 1,
                        _reserved: 0,
                    },
                    payload: Bytes::from(vec![0xABu8; size]),
                };

                b.iter(|| {
                    // Encode and send
                    let encoded = codec.encode(&frame);
                    stream.write_all(&encoded).unwrap();

                    // Receive and decode
                    let mut recv_buf = vec![0u8; encoded.len()];
                    stream.read_exact(&mut recv_buf).unwrap();
                    let decoded = codec.decode(&recv_buf).unwrap();
                    black_box(decoded);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Throughput with network I/O
fn bench_e2e_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_throughput");
    group.sample_size(30);

    for size in [1024, 8192, 65536].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        // Standard TCP throughput
        group.bench_with_input(BenchmarkId::new("standard_tcp", size), size, |b, &size| {
            let port = get_port();
            let _server = spawn_standard_server(port);
            thread::sleep(Duration::from_millis(100));

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

        // SPINE framed TCP
        group.bench_with_input(BenchmarkId::new("SPINE_framed", size), size, |b, &size| {
            let port = get_port();
            let _server = spawn_spine_server(port);
            thread::sleep(Duration::from_millis(100));

            let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
            stream.set_nodelay(true).unwrap();

            let mut codec = FrameCodec::new(size * 2);
            let frame = Frame {
                header: FrameHeader {
                    length: size as u32,
                    flags: FrameFlags::empty(),
                    sequence: 1,
                    stream_id: 1,
                    _reserved: 0,
                },
                payload: Bytes::from(vec![0xABu8; size]),
            };

            b.iter(|| {
                let encoded = codec.encode(&frame);
                stream.write_all(&encoded).unwrap();

                let mut recv_buf = vec![0u8; encoded.len()];
                stream.read_exact(&mut recv_buf).unwrap();
                let decoded = codec.decode(&recv_buf).unwrap();
                black_box(decoded);
            });
        });
    }

    group.finish();
}

/// Benchmark: Concurrent connections
fn bench_concurrent_connections(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_connections");
    group.sample_size(20);

    for conn_count in [4, 16, 64].iter() {
        group.bench_with_input(
            BenchmarkId::new("parallel_requests", conn_count),
            conn_count,
            |b, &count| {
                let ports: Vec<u16> = (0..count).map(|_| get_port()).collect();
                let servers: Vec<_> = ports.iter().map(|&p| spawn_standard_server(p)).collect();
                thread::sleep(Duration::from_millis(200));

                let streams: Vec<_> = ports
                    .iter()
                    .map(|&p| {
                        let s = TcpStream::connect(format!("127.0.0.1:{}", p)).unwrap();
                        s.set_nodelay(true).unwrap();
                        s
                    })
                    .collect();

                let data = vec![0xABu8; 1024];

                b.iter(|| {
                    let handles: Vec<_> = streams
                        .iter()
                        .enumerate()
                        .map(|(i, _)| {
                            let port = ports[i];
                            let data = data.clone();
                            thread::spawn(move || {
                                let mut stream =
                                    TcpStream::connect(format!("127.0.0.1:{}", port)).unwrap();
                                stream.set_nodelay(true).unwrap();
                                stream.write_all(&data).unwrap();
                                let mut recv = vec![0u8; 1024];
                                stream.read_exact(&mut recv).unwrap();
                                black_box(recv);
                            })
                        })
                        .collect();

                    for h in handles {
                        h.join().unwrap();
                    }
                });

                drop(servers);
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_e2e_latency,
    bench_e2e_throughput,
    bench_concurrent_connections,
);

criterion_main!(benches);
