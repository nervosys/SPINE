//! # Protocol Hot-Path Benchmarks
//!
//! Criterion benchmarks for SPINE protocol critical paths:
//! - Message serialization/deserialization
//! - spine-wire v1 connection duplex roundtrip
//! - Parser HTML extraction (via dev-dep)

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

// ======================== Protocol Serde ========================

fn bench_protocol_message_serde(c: &mut Criterion) {
    use spine_protocol::{BrowserCommand, Message, Request, Response};

    let mut group = c.benchmark_group("protocol_serde");

    let navigate_msg = Message::Request(Request {
        id: "bench-1".into(),
        command: BrowserCommand::Navigate {
            url: "https://example.com/very/long/path/to/resource".into(),
        },
    });

    let response_msg = Message::Response(Response {
        id: "bench-1".into(),
        result: Some(serde_json::json!({
            "title": "Example Page",
            "elements": 42,
            "latent": [0.1, 0.2, 0.3, 0.4, 0.5]
        })),
        error: None,
    });

    group.bench_function("serialize_navigate", |b| {
        b.iter(|| serde_json::to_vec(black_box(&navigate_msg)).unwrap());
    });

    group.bench_function("serialize_response", |b| {
        b.iter(|| serde_json::to_vec(black_box(&response_msg)).unwrap());
    });

    let navigate_bytes = serde_json::to_vec(&navigate_msg).unwrap();
    let response_bytes = serde_json::to_vec(&response_msg).unwrap();

    group.bench_function("deserialize_navigate", |b| {
        b.iter(|| serde_json::from_slice::<Message>(black_box(&navigate_bytes)).unwrap());
    });

    group.bench_function("deserialize_response", |b| {
        b.iter(|| serde_json::from_slice::<Message>(black_box(&response_bytes)).unwrap());
    });

    group.finish();
}

// ======================== Protocol Roundtrip ========================

/// A handshaken client and server over an in-process stream.
async fn connection_pair() -> (
    spine_protocol::connection::Connection<tokio::io::DuplexStream>,
    spine_protocol::connection::Connection<tokio::io::DuplexStream>,
) {
    use spine_protocol::connection::Connection;
    use spine_protocol::native;

    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let config = native::local_config();
    let (client, server) = tokio::join!(
        Connection::client_handshake(
            client_io,
            native::client_hello(),
            native::DEFAULT_HELLO_DEADLINE
        ),
        Connection::server_handshake(server_io, &config, native::DEFAULT_HELLO_DEADLINE)
    );
    (client.unwrap(), server.unwrap())
}

fn bench_protocol_roundtrip(c: &mut Criterion) {
    use spine_protocol::{native, BrowserCommand, Message, Request};

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let mut group = c.benchmark_group("protocol_roundtrip");
    group.sample_size(50);

    let request = |i: u64| {
        Message::Request(Request {
            id: i.to_string(),
            command: BrowserCommand::GetUR,
        })
    };

    group.bench_function("handshake_and_request_duplex", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (mut client, mut server) = connection_pair().await;

                native::send_request(&mut client, &request(42))
                    .await
                    .unwrap();
                let frame = server.recv().await.unwrap().unwrap();
                black_box(frame);
            });
        });
    });

    group.bench_function("10_request_burst_on_one_connection", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (mut client, mut server) = connection_pair().await;

                for i in 0..10u64 {
                    let id = native::send_request(&mut client, &request(i))
                        .await
                        .unwrap();
                    let frame = server.recv().await.unwrap().unwrap();
                    native::send_reply(&mut server, frame.request_id, &native::error_reply("ok"))
                        .await
                        .unwrap();
                    black_box(native::await_reply(&mut client, id).await.unwrap());
                }
            });
        });
    });

    group.finish();
}

// ======================== Wire codec: CBOR vs JSON ========================

/// Head-to-head throughput for the spine-wire v1 body codec (`cbor::to_vec` /
/// `cbor::from_slice`, deterministic CBOR) against a `serde_json` body, on the
/// frames where it matters most: a 1 KiB embedding (the binary path that wins
/// big) and a structured tool call. Pairs the *size* numbers from
/// `examples/wire_sizes.rs` with *speed* numbers.
fn bench_wire_codec(c: &mut Criterion) {
    use spine_protocol::cbor;
    use spine_protocol::{Message, StreamData, StreamToken, ToolCall};

    let embedding: Vec<u8> = (0..1024u32)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect();
    let frame = Message::StreamToken(StreamToken {
        id: "stream-7f3a".into(),
        seq: 1,
        data: StreamData::Bytes(embedding),
        usage: None,
    });
    let tool_call = Message::ToolCall(ToolCall {
        id: "b3c1f2a4-0d8e-4c9a-9f1b-2e7d6c5a4b30".into(),
        name: "agent.web/fetch_url".into(),
        args: serde_json::json!({
            "url": "https://example.com/api/v2/resource?id=12345&fields=title,body,author",
            "method": "GET",
            "headers": {"accept": "application/json"},
            "timeout_ms": 30000
        }),
        trace: None,
    });

    let mut group = c.benchmark_group("wire_codec");
    for (name, msg) in [("embedding_1kib", &frame), ("tool_call", &tool_call)] {
        let json_bytes = serde_json::to_vec(msg).unwrap();
        let wire_bytes = cbor::to_vec(msg).unwrap();
        group.throughput(Throughput::Bytes(json_bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("encode_json", name), msg, |b, msg| {
            b.iter(|| serde_json::to_vec(black_box(msg)).unwrap());
        });
        group.bench_with_input(BenchmarkId::new("encode_wire", name), msg, |b, msg| {
            b.iter(|| cbor::to_vec(black_box(msg)).unwrap());
        });
        group.bench_with_input(
            BenchmarkId::new("decode_json", name),
            &json_bytes,
            |b, bytes| {
                b.iter(|| serde_json::from_slice::<Message>(black_box(bytes)).unwrap());
            },
        );
        group.bench_with_input(
            BenchmarkId::new("decode_wire", name),
            &wire_bytes,
            |b, bytes| {
                b.iter(|| cbor::from_slice::<Message>(black_box(bytes)).unwrap());
            },
        );
    }
    group.finish();
}

criterion_group!(
    protocol,
    bench_protocol_message_serde,
    bench_protocol_roundtrip,
    bench_wire_codec,
);

criterion_main!(protocol);
