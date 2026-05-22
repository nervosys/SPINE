//! Agentic AI workload benchmark: embedding/latent vector transmission.
//!
//! The dominant traffic pattern in agentic AI systems is **embedding
//! transmission** between agents: dense f32 vectors at 768-dim (BERT-class),
//! 1536-dim (OpenAI ada-002 / text-embedding-3-small), or 3072-dim
//! (text-embedding-3-large). This bench measures the cost of moving those
//! vectors between two processes on the same machine using:
//!
//! * **HTTP/2 + JSON** — the canonical "traditional AI agent" comm pattern
//!   (OpenAI/Anthropic-style REST clients, MCP-over-HTTP, etc). f32s are
//!   serialized as a JSON array of numbers.
//! * **HTTP/2 + bincode** — same HTTP/2 transport but with a binary payload
//!   instead of JSON. Isolates the *serialization* cost from the protocol.
//! * **SPINE pipelined** — single TCP connection, binary frames containing
//!   the raw f32 bytes (zero serialization overhead). Same batched-write
//!   server design as spine_vs_http2.rs.
//!
//! Two scenarios:
//!
//! 1. **Single embedding** — one f32 vector per request, common dimensions.
//! 2. **Batch embedding** — N vectors per request (RAG retrieval / batch
//!    similarity scoring pattern).
//!
//! ## Honest scope caveats
//!
//! * The `spine-agentic::NeuralProtocol` type in the codebase is documented
//!   as a *stub* (the full neuromorphic PHY layer was removed in a prior
//!   dead-code cleanup). A real "neural protocol" benchmark would require
//!   that PHY implementation; what we measure here is the *transport
//!   layer*, which is what actually carries agent traffic today.
//! * Both HTTP/2 paths use the `h2` crate over plain TCP (h2c). No TLS, so
//!   crypto cost is excluded from both sides equally.

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde::{Deserialize, Serialize};
use spine_transport::{Frame, FrameFlags, FrameHeader};
use std::io::{Read as _, Write as _};
use std::net::{TcpListener as StdListener, TcpStream as StdStream};
use std::thread;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::runtime::Runtime;

// =============================================================================
// Common embedding type
// =============================================================================

#[derive(Serialize, Deserialize, Clone)]
struct Embedding {
    vector: Vec<f32>,
}

#[derive(Serialize, Deserialize, Clone)]
struct EmbeddingBatch {
    embeddings: Vec<Embedding>,
}

fn make_embedding(dim: usize) -> Embedding {
    Embedding {
        vector: (0..dim).map(|i| (i as f32) * 0.001).collect(),
    }
}

fn make_batch(n: usize, dim: usize) -> EmbeddingBatch {
    EmbeddingBatch {
        embeddings: (0..n).map(|_| make_embedding(dim)).collect(),
    }
}

// =============================================================================
// HTTP/2 echo server — accepts a body, echoes it back
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
        .uri("http://x/embed")
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
// SPINE echo server (pipelined + batched, same as spine_vs_http2.rs)
// =============================================================================

fn spawn_spine_batched(listener: StdListener) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        stream.set_nodelay(true).unwrap();
        let mut write_stream = stream.try_clone().unwrap();
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
            if !out.is_empty() && write_stream.write_all(&out).is_err() {
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
            match stream.read(&mut buf[tail..]) {
                Ok(0) => break 'c,
                Ok(m) => tail += m,
                Err(_) => break 'c,
            }
        }
    })
}

// Encode embedding as raw f32 bytes (zero serialization).
fn embedding_to_raw_bytes(emb: &Embedding) -> Vec<u8> {
    let mut v = Vec::with_capacity(emb.vector.len() * 4);
    for f in &emb.vector {
        v.extend_from_slice(&f.to_le_bytes());
    }
    v
}

fn batch_to_raw_bytes(batch: &EmbeddingBatch) -> Vec<u8> {
    let mut v = Vec::with_capacity(batch.embeddings.len() * batch.embeddings[0].vector.len() * 4);
    for emb in &batch.embeddings {
        for f in &emb.vector {
            v.extend_from_slice(&f.to_le_bytes());
        }
    }
    v
}

// =============================================================================
// Scenario 1: single embedding per request
// =============================================================================

fn bench_single_embedding(c: &mut Criterion) {
    let mut group = c.benchmark_group("agentic_single_embedding");
    group.sample_size(20);

    // Common embedding dimensions:
    //   768  = BERT-base, sentence-transformers
    //   1536 = OpenAI text-embedding-ada-002, text-embedding-3-small
    //   3072 = OpenAI text-embedding-3-large
    for &dim in [768usize, 1536, 3072].iter() {
        let bytes_per_vector = (dim * 4) as u64;
        group.throughput(Throughput::Bytes(bytes_per_vector));

        let embedding = make_embedding(dim);
        let json_bytes = serde_json::to_vec(&embedding).unwrap();
        let bincode_bytes = bincode::serialize(&embedding).unwrap();
        let raw_bytes = embedding_to_raw_bytes(&embedding);

        // ----- HTTP/2 + JSON -----
        group.bench_with_input(BenchmarkId::new("http2_json", dim), &dim, |b, _| {
            let rt = Runtime::new().unwrap();
            let (send, _drive) = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let _server = spawn_h2_echo_on(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(json_bytes.clone());
            b.to_async(&rt).iter(|| {
                let send = send.clone();
                let body = body.clone();
                async move {
                    let n = h2_roundtrip(send, body).await;
                    black_box(n);
                }
            });
        });

        // ----- HTTP/2 + bincode -----
        group.bench_with_input(BenchmarkId::new("http2_bincode", dim), &dim, |b, _| {
            let rt = Runtime::new().unwrap();
            let (send, _drive) = rt.block_on(async {
                let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let _server = spawn_h2_echo_on(listener).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                h2_connect(port).await
            });
            let body = Bytes::from(bincode_bytes.clone());
            b.to_async(&rt).iter(|| {
                let send = send.clone();
                let body = body.clone();
                async move {
                    let n = h2_roundtrip(send, body).await;
                    black_box(n);
                }
            });
        });

        // ----- SPINE binary frame -----
        group.bench_with_input(BenchmarkId::new("spine_raw", dim), &dim, |b, _| {
            let listener = StdListener::bind(("127.0.0.1", 0u16)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let _server = spawn_spine_batched(listener);
            thread::sleep(Duration::from_millis(50));
            let mut stream = StdStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_nodelay(true).unwrap();

            let frame = Frame {
                header: FrameHeader {
                    length: raw_bytes.len() as u32,
                    flags: FrameFlags::empty(),
                    sequence: 1,
                    stream_id: 1,
                    _reserved: 0,
                },
                payload: Bytes::from(raw_bytes.clone()),
            };
            let mut send_buf = Vec::with_capacity(12 + raw_bytes.len());
            send_buf.extend_from_slice(&frame.header_bytes());
            send_buf.extend_from_slice(&frame.payload);
            let mut recv_buf = vec![0u8; send_buf.len()];

            b.iter(|| {
                stream.write_all(&send_buf).unwrap();
                stream.read_exact(&mut recv_buf).unwrap();
                black_box(&recv_buf);
            });
        });
    }

    group.finish();
}

// =============================================================================
// Scenario 2: batch of embeddings per request (RAG retrieval pattern)
// =============================================================================

fn bench_batch_embeddings(c: &mut Criterion) {
    let mut group = c.benchmark_group("agentic_batch_embeddings");
    group.sample_size(20);
    let dim = 1536; // pin OpenAI ada-002

    for &batch_n in [8usize, 32, 128].iter() {
        let total_floats = (batch_n * dim) as u64;
        group.throughput(Throughput::Bytes(total_floats * 4));

        let batch = make_batch(batch_n, dim);
        let json_bytes = serde_json::to_vec(&batch).unwrap();
        let bincode_bytes = bincode::serialize(&batch).unwrap();
        let raw_bytes = batch_to_raw_bytes(&batch);

        group.bench_with_input(
            BenchmarkId::new("http2_json", batch_n),
            &batch_n,
            |b, _| {
                let rt = Runtime::new().unwrap();
                let (send, _drive) = rt.block_on(async {
                    let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                    let port = listener.local_addr().unwrap().port();
                    let _server = spawn_h2_echo_on(listener).await;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    h2_connect(port).await
                });
                let body = Bytes::from(json_bytes.clone());
                b.to_async(&rt).iter(|| {
                    let send = send.clone();
                    let body = body.clone();
                    async move {
                        let n = h2_roundtrip(send, body).await;
                        black_box(n);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("http2_bincode", batch_n),
            &batch_n,
            |b, _| {
                let rt = Runtime::new().unwrap();
                let (send, _drive) = rt.block_on(async {
                    let listener = TcpListener::bind(("127.0.0.1", 0u16)).await.unwrap();
                    let port = listener.local_addr().unwrap().port();
                    let _server = spawn_h2_echo_on(listener).await;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    h2_connect(port).await
                });
                let body = Bytes::from(bincode_bytes.clone());
                b.to_async(&rt).iter(|| {
                    let send = send.clone();
                    let body = body.clone();
                    async move {
                        let n = h2_roundtrip(send, body).await;
                        black_box(n);
                    }
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("spine_raw", batch_n), &batch_n, |b, _| {
            let listener = StdListener::bind(("127.0.0.1", 0u16)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let _server = spawn_spine_batched(listener);
            thread::sleep(Duration::from_millis(50));
            let mut stream = StdStream::connect(("127.0.0.1", port)).unwrap();
            stream.set_nodelay(true).unwrap();

            let frame = Frame {
                header: FrameHeader {
                    length: raw_bytes.len() as u32,
                    flags: FrameFlags::empty(),
                    sequence: 1,
                    stream_id: 1,
                    _reserved: 0,
                },
                payload: Bytes::from(raw_bytes.clone()),
            };
            let mut send_buf = Vec::with_capacity(12 + raw_bytes.len());
            send_buf.extend_from_slice(&frame.header_bytes());
            send_buf.extend_from_slice(&frame.payload);
            let mut recv_buf = vec![0u8; send_buf.len()];

            b.iter(|| {
                stream.write_all(&send_buf).unwrap();
                stream.read_exact(&mut recv_buf).unwrap();
                black_box(&recv_buf);
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_single_embedding, bench_batch_embeddings);
criterion_main!(benches);
