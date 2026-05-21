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

/// SPINE-optimized TCP server: one read() into a large frame buffer + parse
/// in place + vectored write. Aims for one read + one write syscall per
/// roundtrip with zero intermediate copies.
fn spawn_spine_server(port: u16) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            stream.set_nodelay(true).unwrap();
            let mut write_stream = stream.try_clone().unwrap();
            let mut buf = vec![0u8; 128 * 1024];
            let mut filled: usize = 0;

            'conn: loop {
                // Ensure we have at least a header.
                while filled < 12 {
                    if filled == buf.len() {
                        buf.resize(buf.len() * 2, 0);
                    }
                    match stream.read(&mut buf[filled..]) {
                        Ok(0) => break 'conn,
                        Ok(n) => filled += n,
                        Err(_) => break 'conn,
                    }
                }

                let length = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
                let total = 12 + length;

                // Ensure we have the whole frame.
                if total > buf.len() {
                    buf.resize(total.max(buf.len() * 2), 0);
                }
                while filled < total {
                    match stream.read(&mut buf[filled..]) {
                        Ok(0) => break 'conn,
                        Ok(n) => filled += n,
                        Err(_) => break 'conn,
                    }
                }

                let hdr = FrameHeader {
                    length: length as u32,
                    flags: FrameFlags::empty(),
                    sequence: 1,
                    stream_id: 1,
                    _reserved: 0,
                };
                if Frame::write_parts_to_sync(&hdr, &buf[12..total], &mut write_stream).is_err() {
                    break 'conn;
                }

                // Slide any tail (next request bytes already received) to the front.
                let tail = filled - total;
                if tail > 0 {
                    buf.copy_within(total..filled, 0);
                }
                filled = tail;
            }
        }
    })
}

/// Standard TCP server (baseline). Accepts in a loop so it can handle many
/// successive connections — previously only `accept()`d once which broke any
/// bench that re-connected per iteration.
fn spawn_standard_server(port: u16) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
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

/// Benchmark: Concurrent connections — N parallel roundtrips on N *pre-established*
/// TCP connections. Earlier version opened a fresh TCP connection on every
/// inner iteration, exhausting the Windows ephemeral-port range and hanging.
/// This version reuses connections (the realistic HTTP-keep-alive / SPINE-pool
/// case) and measures the wall-clock cost of N concurrent roundtrips.
fn bench_concurrent_connections(c: &mut Criterion) {
    use std::sync::{Arc, Mutex};
    let mut group = c.benchmark_group("concurrent_connections");
    group.sample_size(20);

    for conn_count in [4usize, 16, 64].iter() {
        group.bench_with_input(
            BenchmarkId::new("parallel_requests", conn_count),
            conn_count,
            |b, &count| {
                // One server per stream (each server's accept loop only ever
                // sees one peer in this bench, but spawning per-connection is
                // realistic and keeps the test deterministic).
                let ports: Vec<u16> = (0..count).map(|_| get_port()).collect();
                let _servers: Vec<_> =
                    ports.iter().map(|&p| spawn_standard_server(p)).collect();
                thread::sleep(Duration::from_millis(200));

                // Pre-establish N persistent connections; reuse them across
                // every iteration. Mutex<TcpStream> because criterion's `b.iter`
                // closure is `Fn`, not `FnMut`, and we mutate the stream.
                let streams: Vec<Arc<Mutex<TcpStream>>> = ports
                    .iter()
                    .map(|&p| {
                        let s = TcpStream::connect(format!("127.0.0.1:{}", p)).unwrap();
                        s.set_nodelay(true).unwrap();
                        Arc::new(Mutex::new(s))
                    })
                    .collect();

                let data: Arc<Vec<u8>> = Arc::new(vec![0xABu8; 1024]);

                b.iter(|| {
                    let handles: Vec<_> = streams
                        .iter()
                        .map(|s| {
                            let s = Arc::clone(s);
                            let data = Arc::clone(&data);
                            thread::spawn(move || {
                                let mut stream = s.lock().unwrap();
                                stream.write_all(&data).unwrap();
                                let mut recv = [0u8; 1024];
                                stream.read_exact(&mut recv).unwrap();
                                black_box(recv);
                            })
                        })
                        .collect();
                    for h in handles {
                        h.join().unwrap();
                    }
                });
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
