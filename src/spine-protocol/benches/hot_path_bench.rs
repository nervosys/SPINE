//! # Protocol Hot-Path Benchmarks
//!
//! Criterion benchmarks for SPINE protocol critical paths:
//! - Message serialization/deserialization
//! - Protocol handler duplex roundtrip
//! - Encrypted protocol roundtrip
//! - Parser HTML extraction (via dev-dep)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

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

fn bench_protocol_roundtrip(c: &mut Criterion) {
    use spine_protocol::{Message, ProtocolHandler};

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let mut group = c.benchmark_group("protocol_roundtrip");
    group.sample_size(50);

    group.bench_function("ping_pong_duplex", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (client_io, server_io) = tokio::io::duplex(64 * 1024);
                let mut client = ProtocolHandler::new(client_io);
                let mut server = ProtocolHandler::new(server_io);

                client
                    .send_message_raw(&Message::Ping { timestamp: 42 })
                    .await
                    .unwrap();
                let msg = server.receive_message().await.unwrap();
                black_box(msg);
            });
        });
    });

    group.bench_function("encrypted_ping_pong_duplex", |b| {
        let key: [u8; 32] = [0x42; 32];
        b.iter(|| {
            rt.block_on(async {
                let (client_io, server_io) = tokio::io::duplex(64 * 1024);
                let mut client = ProtocolHandler::new(client_io);
                let mut server = ProtocolHandler::new(server_io);
                client.enable_encryption(key);
                server.enable_encryption(key);

                client
                    .send_message_raw(&Message::Ping { timestamp: 42 })
                    .await
                    .unwrap();
                let msg = server.receive_message().await.unwrap();
                black_box(msg);
            });
        });
    });

    group.bench_function("chameleon_aead_send", |b| {
        let secret: [u8; 32] = [0xCA; 32];
        b.iter(|| {
            rt.block_on(async {
                let (client_io, _server_io) = tokio::io::duplex(64 * 1024);
                let mut client = ProtocolHandler::new(client_io);
                client.enable_chameleon_aead(secret);

                client
                    .send_message_raw(&Message::Ping { timestamp: 42 })
                    .await
                    .unwrap();
            });
        });
    });

    group.bench_function("10_msg_burst_plaintext", |b| {
        b.iter(|| {
            rt.block_on(async {
                let (client_io, server_io) = tokio::io::duplex(64 * 1024);
                let mut client = ProtocolHandler::new(client_io);
                let mut server = ProtocolHandler::new(server_io);

                for i in 0..10u64 {
                    client
                        .send_message_raw(&Message::Ping { timestamp: i })
                        .await
                        .unwrap();
                    let msg = server.receive_message().await.unwrap();
                    black_box(msg);
                }
            });
        });
    });

    group.finish();
}

// ======================== Latent Vector ========================

fn bench_latent_vector(c: &mut Criterion) {
    use spine_protocol::{LatentVector, Message};

    let mut group = c.benchmark_group("latent_vector");

    for dim in [8, 64, 256, 1024] {
        let vec = LatentVector {
            components: (0..dim).map(|i| i as f32 * 0.001).collect(),
            dim_hint: dim as u16,
            epoch: 1,
        };
        let msg = Message::LatentMessage(vec);

        group.throughput(Throughput::Bytes((dim * 4) as u64));
        group.bench_with_input(
            BenchmarkId::new("serialize", dim),
            &msg,
            |b, msg| {
                b.iter(|| serde_json::to_vec(black_box(msg)).unwrap());
            },
        );

        let bytes = serde_json::to_vec(&msg).unwrap();
        group.bench_with_input(
            BenchmarkId::new("deserialize", dim),
            &bytes,
            |b, bytes| {
                b.iter(|| serde_json::from_slice::<Message>(black_box(bytes)).unwrap());
            },
        );
    }

    group.finish();
}

criterion_group!(
    protocol,
    bench_protocol_message_serde,
    bench_protocol_roundtrip,
    bench_latent_vector,
);

criterion_main!(protocol);