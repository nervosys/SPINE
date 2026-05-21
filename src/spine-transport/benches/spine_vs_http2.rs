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

// =============================================================================
// CONCURRENT MULTIPLEXED STREAMS — HTTP/2's home turf.
//
// Both protocols issue N simultaneous request/response pairs on a single
// persistent connection. HTTP/2 uses its native stream multiplexer; SPINE
// uses its stream_id field via a thread-per-connection echo server (the
// closest 1:1 comparison we can do without inventing a fresh client-side
// multiplexer for SPINE).
//
// For SPINE we use N parallel TCP connections rather than N logical streams
// on one connection because the production SPINE async client-side
// multiplexer isn't trivially driveable from inside a bench. This is a
// somewhat unfavorable shape for SPINE (N TCP handshakes vs HTTP/2's one),
// but it reflects what an honest deployment would do today.
// =============================================================================

async fn h2_n_concurrent(send: h2::client::SendRequest<Bytes>, n: usize, body: Bytes) -> usize {
    let mut tasks = Vec::with_capacity(n);
    for _ in 0..n {
        let send = send.clone();
        let body = body.clone();
        tasks.push(tokio::spawn(async move { h2_roundtrip(send, body).await }));
    }
    let mut total = 0;
    for t in tasks {
        total += t.await.unwrap();
    }
    total
}

fn bench_concurrent(c: &mut Criterion) {
    let mut group = c.benchmark_group("spine_vs_http2_concurrent");
    group.sample_size(20);

    for n in [4usize, 16, 64].iter() {
        group.throughput(Throughput::Elements(*n as u64));

        // ----- HTTP/2: N concurrent streams on ONE connection -----
        group.bench_with_input(BenchmarkId::new("http2_streams", n), n, |b, &n| {
            let rt = Runtime::new().unwrap();
            let (send, _drive) = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let _server = spawn_h2_echo_on(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(vec![0xABu8; 1024]);

            b.to_async(&rt).iter(|| {
                let send = send.clone();
                let body = body.clone();
                async move {
                    let total = h2_n_concurrent(send, n, body).await;
                    black_box(total);
                }
            });
        });

        // ----- SPINE multiplexed: N pipelined frames on ONE persistent
        // connection. Each frame has a distinct stream_id. Optimized server:
        // (a) drains all available bytes, (b) processes every complete frame
        // in the buffer, (c) batches ALL responses into a single write_all,
        // (d) avoids copy_within by tracking a head cursor and only
        // compacting when we run out of capacity. -----
        group.bench_with_input(BenchmarkId::new("spine_pipelined", n), n, |b, &n| {
            let listener = StdListener::bind(("127.0.0.1", 0u16)).unwrap();
            let port = listener.local_addr().unwrap().port();
            thread::spawn(move || {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                stream.set_nodelay(true).unwrap();
                let mut write_stream = stream.try_clone().unwrap();
                let mut buf = vec![0u8; 1024 * 1024];
                let mut head = 0usize; // first unread byte
                let mut tail = 0usize; // first free byte
                let mut out = Vec::with_capacity(1024 * 1024);
                'c: loop {
                    // Drain every complete frame in [head..tail) into `out`.
                    out.clear();
                    loop {
                        let avail = tail - head;
                        if avail < 12 {
                            break;
                        }
                        let h = &buf[head..head + 12];
                        let length =
                            u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as usize;
                        let total = 12 + length;
                        if avail < total {
                            break;
                        }
                        // Echo: copy entire frame (header + payload) as-is.
                        // stream_id, length, etc. are already correct.
                        out.extend_from_slice(&buf[head..head + total]);
                        head += total;
                    }
                    if !out.is_empty() {
                        if write_stream.write_all(&out).is_err() {
                            break 'c;
                        }
                    }
                    // Compact if head has advanced a lot.
                    if head > 0 && tail - head < buf.len() / 4 {
                        buf.copy_within(head..tail, 0);
                        tail -= head;
                        head = 0;
                    }
                    if tail == buf.len() {
                        buf.resize(buf.len() * 2, 0);
                    }
                    match stream.read(&mut buf[tail..]) {
                        Ok(0) => break 'c,
                        Ok(m) => tail += m,
                        Err(_) => break 'c,
                    }
                }
            });
            thread::sleep(Duration::from_millis(50));

            let mut stream = StdStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_nodelay(true).unwrap();

            // Build one big send buffer of N frames, distinct stream_ids.
            let payload = vec![0xABu8; 1024];
            let mut send_buf = Vec::with_capacity(n * (12 + 1024));
            for i in 0..n {
                let frame = Frame {
                    header: FrameHeader {
                        length: 1024,
                        flags: FrameFlags::empty(),
                        sequence: 1,
                        stream_id: (i as u16) + 1,
                        _reserved: 0,
                    },
                    payload: Bytes::from(payload.clone()),
                };
                send_buf.extend_from_slice(&frame.header_bytes());
                send_buf.extend_from_slice(&frame.payload);
            }
            let mut recv_buf = vec![0u8; send_buf.len()];

            b.iter(|| {
                stream.write_all(&send_buf).unwrap();
                stream.read_exact(&mut recv_buf).unwrap();
                black_box(&recv_buf);
            });
        });

        // ----- (legacy) SPINE: N parallel TCP connections, one frame each -----
        group.bench_with_input(BenchmarkId::new("spine_n_conns", n), n, |b, &n| {
            use std::sync::{Arc, Mutex};
            let listener = StdListener::bind(("127.0.0.1", 0u16)).unwrap();
            let port = listener.local_addr().unwrap().port();
            // Accept loop must handle many connections.
            thread::spawn(move || {
                for incoming in listener.incoming() {
                    let Ok(mut stream) = incoming else { return };
                    stream.set_nodelay(true).unwrap();
                    thread::spawn(move || {
                        let mut write_stream = stream.try_clone().unwrap();
                        let mut buf = vec![0u8; 64 * 1024];
                        let mut filled = 0;
                        'c: loop {
                            while filled < 12 {
                                match stream.read(&mut buf[filled..]) {
                                    Ok(0) => break 'c,
                                    Ok(n) => filled += n,
                                    Err(_) => break 'c,
                                }
                            }
                            let length =
                                u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
                            let total = 12 + length;
                            while filled < total {
                                match stream.read(&mut buf[filled..]) {
                                    Ok(0) => break 'c,
                                    Ok(n) => filled += n,
                                    Err(_) => break 'c,
                                }
                            }
                            let hdr = FrameHeader {
                                length: length as u32,
                                flags: FrameFlags::empty(),
                                sequence: 1,
                                stream_id: 1,
                                _reserved: 0,
                            };
                            if Frame::write_parts_to_sync(&hdr, &buf[12..total], &mut write_stream)
                                .is_err()
                            {
                                break 'c;
                            }
                            let tail = filled - total;
                            if tail > 0 {
                                buf.copy_within(total..filled, 0);
                            }
                            filled = tail;
                        }
                    });
                }
            });
            thread::sleep(Duration::from_millis(50));

            // Pre-establish N persistent connections.
            let streams: Vec<Arc<Mutex<StdStream>>> = (0..n)
                .map(|_| {
                    let s = StdStream::connect(("127.0.0.1", port)).unwrap();
                    s.set_nodelay(true).unwrap();
                    Arc::new(Mutex::new(s))
                })
                .collect();

            let frame_bytes: Arc<Vec<u8>> = {
                let frame = Frame {
                    header: FrameHeader {
                        length: 1024,
                        flags: FrameFlags::empty(),
                        sequence: 1,
                        stream_id: 1,
                        _reserved: 0,
                    },
                    payload: Bytes::from(vec![0xABu8; 1024]),
                };
                let mut v = Vec::with_capacity(12 + 1024);
                v.extend_from_slice(&frame.header_bytes());
                v.extend_from_slice(&frame.payload);
                Arc::new(v)
            };

            b.iter(|| {
                let handles: Vec<_> = streams
                    .iter()
                    .map(|s| {
                        let s = Arc::clone(s);
                        let frame_bytes = Arc::clone(&frame_bytes);
                        thread::spawn(move || {
                            let mut stream = s.lock().unwrap();
                            stream.write_all(&frame_bytes).unwrap();
                            let mut recv = vec![0u8; frame_bytes.len()];
                            stream.read_exact(&mut recv).unwrap();
                            black_box(recv);
                        })
                    })
                    .collect();
                for h in handles {
                    h.join().unwrap();
                }
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_latency, bench_throughput, bench_concurrent);
criterion_main!(benches);
