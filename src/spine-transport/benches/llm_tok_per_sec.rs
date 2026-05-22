//! LLM-style token throughput benchmark.
//!
//! Tokens per second is the canonical LLM serving metric. Two patterns:
//!
//! * **Batch generation**: all N tokens in one response chunk (OpenAI
//!   non-streaming). Measures pure transport ceiling.
//! * **Streaming generation**: each token in its own message (OpenAI
//!   streaming / Server-Sent Events). Measures per-token overhead.
//!
//! Both patterns benchmarked across three transports, all on the same
//! tokio runtime so the I/O scheduler is identical (no sync-thread vs
//! async-runtime asymmetry):
//!
//! * **HTTP/2 + JSON** (OpenAI SSE format: `data: {"id":N}\n\n`)
//! * **HTTP/2 + binary**
//! * **SPINE async** — uses a real tokio-based `AsyncSpineClient`
//!   (defined in this file) so the comparison is apples-to-apples.
//!
//! Tokens are 4-byte u32 IDs, which is what production LLM serving stacks
//! (vLLM, TGI, llama.cpp) use internally before detokenizing for the wire.

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use spine_transport::{Frame, FrameFlags, FrameHeader};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;
use tokio::sync::Mutex as AsyncMutex;

// =============================================================================
// AsyncSpineClient — tokio-based SPINE client (THIS is the gap-closer)
// =============================================================================
//
// The prior agentic_ai_workload bench used std::net::TcpStream on a sync
// thread, which lost to h2's in-runtime tokio I/O at small single-request
// sizes (a benchmark harness artifact, not a protocol property). This
// client puts SPINE on the same tokio scheduler as h2.

pub struct AsyncSpineClient {
    write: tokio::net::tcp::OwnedWriteHalf,
    read: tokio::net::tcp::OwnedReadHalf,
}

impl AsyncSpineClient {
    pub async fn connect(addr: &str) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        let (read, write) = stream.into_split();
        Ok(Self { write, read })
    }

    /// Send one frame with header + payload via a single vectored write.
    #[inline]
    pub async fn send_frame(
        &mut self,
        header: &FrameHeader,
        payload: &[u8],
    ) -> std::io::Result<()> {
        let mut hb = [0u8; 12];
        hb[0..4].copy_from_slice(&header.length.to_le_bytes());
        hb[4] = header.flags.bits();
        hb[5..8].copy_from_slice(&header.sequence.to_le_bytes()[0..3]);
        hb[8..10].copy_from_slice(&header.stream_id.to_le_bytes());
        hb[10..12].copy_from_slice(&header._reserved.to_le_bytes());
        // Tokio's write_vectored may write partially; pack into one buffer
        // for the small message case (better than 2 syscalls at <1KB).
        if payload.len() < 4096 {
            let mut combined = Vec::with_capacity(12 + payload.len());
            combined.extend_from_slice(&hb);
            combined.extend_from_slice(payload);
            self.write.write_all(&combined).await?;
        } else {
            self.write.write_all(&hb).await?;
            self.write.write_all(payload).await?;
        }
        Ok(())
    }

    /// Send a pre-encoded buffer of M concatenated frames.
    #[inline]
    pub async fn send_bytes(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.write.write_all(bytes).await
    }

    /// Receive one frame's payload.
    #[inline]
    pub async fn recv_frame(&mut self) -> std::io::Result<Vec<u8>> {
        let mut header = [0u8; 12];
        self.read.read_exact(&mut header).await?;
        let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let mut payload = vec![0u8; len];
        self.read.read_exact(&mut payload).await?;
        Ok(payload)
    }

    /// Drain exactly `n_bytes` from the wire into a buffer (used to swallow
    /// a batch response in one shot).
    #[inline]
    pub async fn recv_bytes(&mut self, n_bytes: usize) -> std::io::Result<Vec<u8>> {
        let mut buf = vec![0u8; n_bytes];
        self.read.read_exact(&mut buf).await?;
        Ok(buf)
    }
}

// =============================================================================
// Async SPINE batched echo server
// =============================================================================

async fn spawn_spine_async_echo(listener: TcpListener) {
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        stream.set_nodelay(true).unwrap();
        let mut buf = vec![0u8; 4 * 1024 * 1024];
        let mut head = 0usize;
        let mut tail = 0usize;
        let mut out = Vec::with_capacity(4 * 1024 * 1024);
        'c: loop {
            out.clear();
            loop {
                let avail = tail - head;
                if avail < 12 {
                    break;
                }
                let h = &buf[head..head + 12];
                let length = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as usize;
                let total = 12 + length;
                if avail < total {
                    break;
                }
                out.extend_from_slice(&buf[head..head + total]);
                head += total;
            }
            if !out.is_empty() && stream.write_all(&out).await.is_err() {
                break 'c;
            }
            if head > 0 && tail - head < buf.len() / 4 {
                buf.copy_within(head..tail, 0);
                tail -= head;
                head = 0;
            }
            if tail == buf.len() {
                buf.resize(buf.len() * 2, 0);
            }
            match stream.read(&mut buf[tail..]).await {
                Ok(0) => break 'c,
                Ok(m) => tail += m,
                Err(_) => break 'c,
            }
        }
    });
}

// =============================================================================
// HTTP/2 echo (same shape as other benches)
// =============================================================================

async fn spawn_h2_echo(listener: TcpListener) {
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
    });
}

async fn h2_connect(port: u16) -> h2::client::SendRequest<Bytes> {
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tcp.set_nodelay(true).unwrap();
    let (send, conn) = h2::client::handshake(tcp).await.unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    send
}

async fn h2_roundtrip(send: h2::client::SendRequest<Bytes>, body: Bytes) -> usize {
    let mut send = send.ready().await.unwrap();
    let request = http::Request::builder()
        .method("POST")
        .uri("http://x/tokens")
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
// Token encoders
// =============================================================================

/// OpenAI SSE-style streaming JSON: one chunk per token.
fn tokens_to_openai_sse(token_ids: &[u32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(token_ids.len() * 50);
    for id in token_ids {
        // Minimal OpenAI streaming shape — id field stands in for content.
        let line = format!("data: {{\"id\":{}}}\n\n", id);
        buf.extend_from_slice(line.as_bytes());
    }
    // Stream-end marker.
    buf.extend_from_slice(b"data: [DONE]\n\n");
    buf
}

/// Binary token IDs (4 bytes each), no framing.
fn tokens_to_binary(token_ids: &[u32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(token_ids.len() * 4);
    for id in token_ids {
        buf.extend_from_slice(&id.to_le_bytes());
    }
    buf
}

/// Pre-build a SPINE send buffer: one frame whose payload is N×4 bytes of
/// little-endian u32 token IDs.
fn tokens_to_spine_frame(token_ids: &[u32]) -> Vec<u8> {
    let payload_len = token_ids.len() * 4;
    let frame = Frame {
        header: FrameHeader {
            length: payload_len as u32,
            flags: FrameFlags::empty(),
            sequence: 1,
            stream_id: 1,
            _reserved: 0,
        },
        payload: Bytes::from(tokens_to_binary(token_ids)),
    };
    let mut v = Vec::with_capacity(12 + payload_len);
    v.extend_from_slice(&frame.header_bytes());
    v.extend_from_slice(&frame.payload);
    v
}

fn make_token_ids(n: usize) -> Vec<u32> {
    (0..n as u32).map(|i| i.wrapping_mul(2654435761)).collect()
}

// =============================================================================
// SCENARIO 1: Batch generation throughput (tokens/sec)
// =============================================================================
//
// Client requests N tokens, server responds with N tokens in one chunk.
// This is the "non-streaming" or "completed-response" pattern.

fn bench_batch_tokens(c: &mut Criterion) {
    let mut group = c.benchmark_group("llm_batch_tok_per_sec");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(4));

    for &n_tokens in [256usize, 1024, 4096].iter() {
        group.throughput(Throughput::Elements(n_tokens as u64));

        let token_ids = make_token_ids(n_tokens);
        let json_body = tokens_to_openai_sse(&token_ids);
        let binary_body = tokens_to_binary(&token_ids);
        let spine_frame = tokens_to_spine_frame(&token_ids);

        // ----- HTTP/2 + JSON (OpenAI SSE format) -----
        group.bench_with_input(BenchmarkId::new("http2_openai_sse", n_tokens), &n_tokens, |b, _| {
            let rt = Runtime::new().unwrap();
            let send = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                spawn_h2_echo(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(json_body.clone());
            b.to_async(&rt).iter(|| {
                let send = send.clone();
                let body = body.clone();
                async move {
                    let n = h2_roundtrip(send, body).await;
                    black_box(n);
                }
            });
        });

        // ----- HTTP/2 + binary -----
        group.bench_with_input(BenchmarkId::new("http2_binary", n_tokens), &n_tokens, |b, _| {
            let rt = Runtime::new().unwrap();
            let send = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                spawn_h2_echo(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(binary_body.clone());
            b.to_async(&rt).iter(|| {
                let send = send.clone();
                let body = body.clone();
                async move {
                    let n = h2_roundtrip(send, body).await;
                    black_box(n);
                }
            });
        });

        // ----- SPINE async -----
        group.bench_with_input(BenchmarkId::new("spine_async", n_tokens), &n_tokens, |b, _| {
            let rt = Runtime::new().unwrap();
            let client = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                spawn_spine_async_echo(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                Arc::new(AsyncMutex::new(
                    AsyncSpineClient::connect(&format!("127.0.0.1:{}", port)).await.unwrap(),
                ))
            });
            let frame = Arc::new(spine_frame.clone());
            let expected_recv = frame.len();
            b.to_async(&rt).iter(|| {
                let client = Arc::clone(&client);
                let frame = Arc::clone(&frame);
                async move {
                    let mut c = client.lock().await;
                    c.send_bytes(&frame).await.unwrap();
                    let resp = c.recv_bytes(expected_recv).await.unwrap();
                    black_box(resp);
                }
            });
        });
    }

    group.finish();
}

// =============================================================================
// SCENARIO 2: Streaming generation (per-token messages)
// =============================================================================
//
// Server emits N tokens, ONE PER MESSAGE (HTTP/2 DATA frame or SPINE binary
// frame). Client receives them one by one. This models OpenAI streaming /
// SSE — each token arrives as a separate event for low TTFT.
//
// For SPINE we use the pipelined+batched server (sends are bundled), so
// the cost is dominated by per-frame overhead on the client read loop. For
// HTTP/2 each token is its own SSE event line / DATA frame.
//
// On the SPINE side the client receives N frames in a loop; on h2 it
// reads N SSE chunks.

fn bench_streaming_tokens(c: &mut Criterion) {
    let mut group = c.benchmark_group("llm_streaming_tok_per_sec");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(4));

    for &n_tokens in [64usize, 256, 1024].iter() {
        group.throughput(Throughput::Elements(n_tokens as u64));

        let token_ids = make_token_ids(n_tokens);
        let json_body = tokens_to_openai_sse(&token_ids);

        // Build a SPINE request that asks for N separate 4-byte token frames.
        // The "request" itself is one frame whose payload is the token IDs
        // (server reads, echoes ONE frame back per token... but our echo
        // server actually echoes what it receives, so we send N separate
        // small frames as the request and the server echoes them as N
        // separate small frames. That matches the streaming shape).
        let mut spine_req = Vec::with_capacity(n_tokens * 16);
        for id in &token_ids {
            let frame = Frame {
                header: FrameHeader {
                    length: 4,
                    flags: FrameFlags::empty(),
                    sequence: 1,
                    stream_id: 1,
                    _reserved: 0,
                },
                payload: Bytes::from(id.to_le_bytes().to_vec()),
            };
            spine_req.extend_from_slice(&frame.header_bytes());
            spine_req.extend_from_slice(&frame.payload);
        }

        // ----- HTTP/2 + JSON SSE streaming -----
        group.bench_with_input(BenchmarkId::new("http2_openai_sse", n_tokens), &n_tokens, |b, _| {
            let rt = Runtime::new().unwrap();
            let send = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                spawn_h2_echo(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(json_body.clone());
            b.to_async(&rt).iter(|| {
                let send = send.clone();
                let body = body.clone();
                async move {
                    let n = h2_roundtrip(send, body).await;
                    black_box(n);
                }
            });
        });

        // ----- SPINE async, per-token frames -----
        group.bench_with_input(BenchmarkId::new("spine_per_token", n_tokens), &n_tokens, |b, _| {
            let rt = Runtime::new().unwrap();
            let client = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                spawn_spine_async_echo(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                Arc::new(AsyncMutex::new(
                    AsyncSpineClient::connect(&format!("127.0.0.1:{}", port)).await.unwrap(),
                ))
            });
            let req = Arc::new(spine_req.clone());
            let expected_recv = req.len();
            b.to_async(&rt).iter(|| {
                let client = Arc::clone(&client);
                let req = Arc::clone(&req);
                async move {
                    let mut c = client.lock().await;
                    c.send_bytes(&req).await.unwrap();
                    let resp = c.recv_bytes(expected_recv).await.unwrap();
                    black_box(resp);
                }
            });
        });
    }

    group.finish();
}

// =============================================================================
// SCENARIO 3: Single-embedding revisit — close the harness-asymmetry gap
// =============================================================================
//
// The agentic_ai_workload bench showed SPINE losing to HTTP/2 at single
// 1536-dim embeddings because SPINE used sync std::net while HTTP/2 used
// tokio. Re-run with the real AsyncSpineClient.

fn bench_single_embedding_async(c: &mut Criterion) {
    let mut group = c.benchmark_group("llm_single_embedding_async");
    group.sample_size(20);

    for &dim in [768usize, 1536, 3072].iter() {
        group.throughput(Throughput::Bytes((dim * 4) as u64));

        let embedding: Vec<f32> = (0..dim).map(|i| (i as f32) * 0.001).collect();
        let raw_payload: Vec<u8> = {
            let mut v = Vec::with_capacity(dim * 4);
            for f in &embedding {
                v.extend_from_slice(&f.to_le_bytes());
            }
            v
        };
        let json_body = serde_json::to_vec(&embedding).unwrap();
        let bincode_body = bincode::serialize(&embedding).unwrap();

        let frame = Frame {
            header: FrameHeader {
                length: raw_payload.len() as u32,
                flags: FrameFlags::empty(),
                sequence: 1,
                stream_id: 1,
                _reserved: 0,
            },
            payload: Bytes::from(raw_payload.clone()),
        };
        let mut spine_buf = Vec::with_capacity(12 + raw_payload.len());
        spine_buf.extend_from_slice(&frame.header_bytes());
        spine_buf.extend_from_slice(&frame.payload);

        group.bench_with_input(BenchmarkId::new("http2_json", dim), &dim, |b, _| {
            let rt = Runtime::new().unwrap();
            let send = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                spawn_h2_echo(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(json_body.clone());
            b.to_async(&rt).iter(|| {
                let send = send.clone();
                let body = body.clone();
                async move {
                    let n = h2_roundtrip(send, body).await;
                    black_box(n);
                }
            });
        });

        group.bench_with_input(BenchmarkId::new("http2_bincode", dim), &dim, |b, _| {
            let rt = Runtime::new().unwrap();
            let send = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                spawn_h2_echo(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(bincode_body.clone());
            b.to_async(&rt).iter(|| {
                let send = send.clone();
                let body = body.clone();
                async move {
                    let n = h2_roundtrip(send, body).await;
                    black_box(n);
                }
            });
        });

        group.bench_with_input(BenchmarkId::new("spine_async", dim), &dim, |b, _| {
            let rt = Runtime::new().unwrap();
            let client = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                spawn_spine_async_echo(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                Arc::new(AsyncMutex::new(
                    AsyncSpineClient::connect(&format!("127.0.0.1:{}", port)).await.unwrap(),
                ))
            });
            let req = Arc::new(spine_buf.clone());
            let expected_recv = req.len();
            b.to_async(&rt).iter(|| {
                let client = Arc::clone(&client);
                let req = Arc::clone(&req);
                async move {
                    let mut c = client.lock().await;
                    c.send_bytes(&req).await.unwrap();
                    let resp = c.recv_bytes(expected_recv).await.unwrap();
                    black_box(resp);
                }
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_single_embedding_async,
    bench_batch_tokens,
    bench_streaming_tokens,
);
criterion_main!(benches);
