//! SPINE vs HTTP/2 (h2c, no TLS) — multiplexed-protocol head-to-head.
//!
//! The `spine_vs_www.rs` bench compares SPINE to HTTP/1.1 — a soft target
//! because HTTP/1.1 is textual and serial-per-connection. This bench compares
//! SPINE to a *real, modern* multiplexed binary protocol (HTTP/2 via the `h2`
//! crate, no TLS so we measure protocol overhead, not crypto).
//!
//! Both sides:
//! * Use a single persistent TCP connection, established before `b.iter`.
//! * Use a multiplexed binary frame format with stream IDs.
//! * Are driven by tokio.
//!
//! Differences we expect to show:
//! * Per-request overhead (HTTP/2 has HPACK header table, SPINE has 12-byte
//!   fixed binary header).
//! * Flow-control accounting (HTTP/2 has per-stream + connection windows,
//!   SPINE has none here).

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use spine_transport::{Frame, FrameFlags, FrameHeader};
use std::io::{Read as _, Write as _};
use std::net::{TcpListener as StdListener, TcpStream as StdStream};
use std::thread;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;

// =============================================================================
// HTTP/2 (h2 crate, cleartext) — server + client on a persistent connection
// =============================================================================

async fn spawn_h2_echo_on(listener: TcpListener) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let Ok((tcp, _)) = listener.accept().await else {
            return;
        };
        tcp.set_nodelay(true).unwrap();
        let mut conn = match h2::server::handshake(tcp).await {
            Ok(c) => c,
            Err(_) => return,
        };
        while let Some(result) = conn.accept().await {
            let Ok((req, mut respond)) = result else {
                continue;
            };
            tokio::spawn(async move {
                let mut body = req.into_body();
                let mut buf = Vec::new();
                while let Some(chunk) = body.data().await {
                    let Ok(data) = chunk else {
                        return;
                    };
                    let _ = body.flow_control().release_capacity(data.len());
                    buf.extend_from_slice(&data);
                }
                let response = http::Response::builder().status(200).body(()).unwrap();
                let mut send = match respond.send_response(response, false) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let _ = send.send_data(Bytes::from(buf), true);
            });
        }
    })
}

// Connect an h2 client. Returns the SendRequest handle and a connection-driver
// task that must be kept alive while making requests.
async fn h2_connect(port: u16) -> (h2::client::SendRequest<Bytes>, tokio::task::JoinHandle<()>) {
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tcp.set_nodelay(true).unwrap();
    let (send, conn) = h2::client::handshake(tcp).await.unwrap();
    let drive = tokio::spawn(async move {
        let _ = conn.await;
    });
    (send, drive)
}

async fn h2_roundtrip(send: h2::client::SendRequest<Bytes>, body: Bytes) -> usize {
    let mut send = send.ready().await.unwrap();
    let request = http::Request::builder()
        .method("POST")
        .uri("http://x/echo")
        .body(())
        .unwrap();
    let (response, mut req_body) = send.send_request(request, false).unwrap();
    req_body.send_data(body, true).unwrap();
    let resp = response.await.unwrap();
    let mut resp_body = resp.into_body();
    let mut total = 0;
    while let Some(chunk) = resp_body.data().await {
        let data = chunk.unwrap();
        let _ = resp_body.flow_control().release_capacity(data.len());
        total += data.len();
    }
    total
}

// =============================================================================
// SPINE echo server (matches the spine_vs_www version) on a persistent conn.
// =============================================================================

fn spawn_spine_echo_on(listener: StdListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            stream.set_nodelay(true).unwrap();
            let mut write_stream = stream.try_clone().unwrap();
            let mut buf = vec![0u8; 512 * 1024];
            let mut filled: usize = 0;

            'conn: loop {
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
                let tail = filled - total;
                if tail > 0 {
                    buf.copy_within(total..filled, 0);
                }
                filled = tail;
            }
        }
    })
}

// =============================================================================
// LATENCY — single request roundtrip, persistent connection
// =============================================================================

fn bench_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("spine_vs_http2_latency");
    group.sample_size(30);

    for size in [64usize, 512, 4096].iter() {
        // ----- HTTP/2 -----
        group.bench_with_input(BenchmarkId::new("http2", size), size, |b, &size| {
            let rt = Runtime::new().unwrap();
            let (send, _drive) = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let _server = spawn_h2_echo_on(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(vec![0xABu8; size]);

            b.to_async(&rt).iter(|| {
                let body = body.clone();
                let send = send.clone();
                async move {
                    let n = h2_roundtrip(send, body).await;
                    black_box(n);
                }
            });
        });

        // ----- SPINE -----
        group.bench_with_input(BenchmarkId::new("spine", size), size, |b, &size| {
            let listener = StdListener::bind(("127.0.0.1", 0u16)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let _server = spawn_spine_echo_on(listener);
            thread::sleep(Duration::from_millis(50));
            let mut stream = StdStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_nodelay(true).unwrap();
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
            let mut encoded = Vec::with_capacity(12 + size);
            encoded.extend_from_slice(&frame.header_bytes());
            encoded.extend_from_slice(&frame.payload);
            let mut recv = vec![0u8; encoded.len()];

            b.iter(|| {
                stream.write_all(&encoded).unwrap();
                stream.read_exact(&mut recv).unwrap();
                black_box(&recv);
            });
        });
    }
    group.finish();
}

// =============================================================================
// THROUGHPUT — large body
// =============================================================================

fn bench_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("spine_vs_http2_throughput");
    group.sample_size(20);

    for size in [4096usize, 32_768].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::new("http2", size), size, |b, &size| {
            let rt = Runtime::new().unwrap();
            let (send, _drive) = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let _server = spawn_h2_echo_on(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(vec![0xABu8; size]);

            b.to_async(&rt).iter(|| {
                let body = body.clone();
                let send = send.clone();
                async move {
                    let n = h2_roundtrip(send, body).await;
                    black_box(n);
                }
            });
        });

        group.bench_with_input(BenchmarkId::new("spine", size), size, |b, &size| {
            let listener = StdListener::bind(("127.0.0.1", 0u16)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let _server = spawn_spine_echo_on(listener);
            thread::sleep(Duration::from_millis(50));
            let mut stream = StdStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_nodelay(true).unwrap();
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
            let mut encoded = Vec::with_capacity(12 + size);
            encoded.extend_from_slice(&frame.header_bytes());
            encoded.extend_from_slice(&frame.payload);
            let mut recv = vec![0u8; encoded.len()];

            b.iter(|| {
                stream.write_all(&encoded).unwrap();
                stream.read_exact(&mut recv).unwrap();
                black_box(&recv);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_latency, bench_throughput);
criterion_main!(benches);
