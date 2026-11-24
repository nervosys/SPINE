//! # Protocol Hot-Path Benchmarks
//!
//! Criterion benchmarks for SPINE protocol critical paths:
//! - Body serialization/deserialization (commands, replies)
//! - spine-wire v1 connection duplex roundtrip
//! - Parser HTML extraction (via dev-dep)

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

// ======================== Protocol Serde ========================

fn bench_protocol_message_serde(c: &mut Criterion) {
    use spine_protocol::{BrowserCommand, Reply};

    let mut group = c.benchmark_group("protocol_serde");

    // A REQUEST body is a command; a RESPONSE body is `{ok: T}`.
    let navigate_cmd = BrowserCommand::Navigate {
        url: "https://example.com/very/long/path/to/resource".into(),
    };

    let response: Reply<serde_json::Value> = Reply::Ok(serde_json::json!({
        "title": "Example Page",
        "elements": 42,
        "latent": [0.1, 0.2, 0.3, 0.4, 0.5]
    }));

    group.bench_function("serialize_navigate", |b| {
        b.iter(|| serde_json::to_vec(black_box(&navigate_cmd)).unwrap());
    });

    group.bench_function("serialize_response", |b| {
        b.iter(|| serde_json::to_vec(black_box(&response)).unwrap());
    });

    let navigate_bytes = serde_json::to_vec(&navigate_cmd).unwrap();
    let response_bytes = serde_json::to_vec(&response).unwrap();

    group.bench_function("deserialize_navigate", |b| {
        b.iter(|| serde_json::from_slice::<BrowserCommand>(black_box(&navigate_bytes)).unwrap());
    });

    group.bench_function("deserialize_response", |b| {
        b.iter(|| {
            serde_json::from_slice::<Reply<serde_json::Value>>(black_box(&response_bytes)).unwrap()
        });
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
    use spine_protocol::{native, BrowserCommand};

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let mut group = c.benchmark_group("protocol_roundtrip");
    group.sample_size(50);

    let command = BrowserCommand::GetUR;
    let ok = serde_json::json!({"ok": true});

    group.bench_function("handshake_and_request_duplex", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (mut client, mut server) = connection_pair().await;

                native::send_command(&mut client, &command).await.unwrap();
                let frame = server.recv().await.unwrap().unwrap();
                black_box(frame);
            });
        });
    });

    group.bench_function("10_request_burst_on_one_connection", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (mut client, mut server) = connection_pair().await;

                for _ in 0..10u64 {
                    let id = native::send_command(&mut client, &command).await.unwrap();
                    let frame = server.recv().await.unwrap().unwrap();
                    native::send_ok(&mut server, frame.request_id, &ok)
                        .await
                        .unwrap();
                    black_box(
                        native::await_reply::<_, serde_json::Value>(&mut client, id)
                            .await
                            .unwrap(),
                    );
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
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use spine_protocol::cbor;
    use spine_protocol::{BrowserCommand, StreamData, StreamToken, ToolCall};

    let embedding: Vec<u8> = (0..1024u32)
        .map(|i| (i.wrapping_mul(31) % 251) as u8)
        .collect();
    // The body of a STREAM_TOKEN frame: no id, the frame header carries it.
    let token = StreamToken {
        seq: 1,
        data: StreamData::Bytes(embedding),
        usage: None,
    };
    // The body of a REQUEST frame: the command.
    let tool_call = BrowserCommand::ToolCall(ToolCall {
        name: "agent.web/fetch_url".into(),
        args: serde_json::json!({
            "url": "https://example.com/api/v2/resource?id=12345&fields=title,body,author",
            "method": "GET",
            "headers": {"accept": "application/json"},
            "timeout_ms": 30000
        }),
        trace: None,
    });

    fn run<T: Serialize + DeserializeOwned>(
        group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
        name: &str,
        value: &T,
    ) {
        let json_bytes = serde_json::to_vec(value).unwrap();
        let wire_bytes = cbor::to_vec(value).unwrap();
        group.throughput(Throughput::Bytes(json_bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("encode_json", name), value, |b, value| {
            b.iter(|| serde_json::to_vec(black_box(value)).unwrap());
        });
        group.bench_with_input(BenchmarkId::new("encode_wire", name), value, |b, value| {
            b.iter(|| cbor::to_vec(black_box(value)).unwrap());
        });
        group.bench_with_input(
            BenchmarkId::new("decode_json", name),
            &json_bytes,
            |b, bytes| {
                b.iter(|| serde_json::from_slice::<T>(black_box(bytes)).unwrap());
            },
        );
        group.bench_with_input(
            BenchmarkId::new("decode_wire", name),
            &wire_bytes,
            |b, bytes| {
                b.iter(|| cbor::from_slice::<T>(black_box(bytes)).unwrap());
            },
        );
    }

    let mut group = c.benchmark_group("wire_codec");
    run(&mut group, "embedding_1kib", &token);
    run(&mut group, "tool_call", &tool_call);
    group.finish();
}

criterion_group!(
    protocol,
    bench_protocol_message_serde,
    bench_protocol_roundtrip,
    bench_wire_codec,
);

criterion_main!(protocol);
