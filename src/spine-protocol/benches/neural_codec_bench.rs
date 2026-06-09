//! Neural codec benchmark for **agentic web usage**.
//!
//! The other benches measure transport (frames on the wire) and the wire codec
//! (a hand-built `EncodedFrame`). This one exercises the *real* neural codec —
//! `spine_protocol::TitansLatentCodec`, which wraps `spine-neural`'s Titans
//! `NeuralLatentEncoder` — along the path an agent actually uses: take some
//! natural-language content, project it into a fixed-width latent, and put a
//! self-describing `EncodedFrame` on the SPINE wire.
//!
//! Three things matter for agentic use, and each is measured here:
//!   1. **encode throughput** of the neural projector at common embed dims
//!      (`neural_codec_encode`),
//!   2. **full agentic path latency** — `codec.encode` then `wire::encode`
//!      (`neural_codec_agentic_path`),
//!   3. **on-wire frame size** — SPINE binary (CBOR byte-string latent) vs the
//!      JSON an HTTP+JSON agent would send (printed once at startup).
//!
//! Run: `cargo bench -p spine-protocol --bench neural_codec_bench`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use spine_protocol::{wire, CodecRegistry, Message, NeuralCodec, TitansLatentCodec};
use std::sync::Arc;

/// Embedding widths an agent stack commonly negotiates.
const DIMS: &[usize] = &[128, 256, 512, 1024];

/// A representative agent payload: natural-language content an agent wants to
/// embed and stream to a peer (capability report + reasoning trace).
fn sample_text() -> Vec<u8> {
    "The agent observed an anomalous spike in outbound traffic from node-7 and \
     proposes rerouting the embedding broadcast through the gRPC bridge while \
     the Chameleon morph re-keys. Capability: network. Confidence: 0.91."
        .repeat(4)
        .into_bytes()
}

/// (1) Neural encode throughput: text bytes -> Titans latent -> EncodedFrame.
fn bench_codec_encode(c: &mut Criterion) {
    let input = sample_text();
    let mut group = c.benchmark_group("neural_codec_encode");
    group.throughput(Throughput::Bytes(input.len() as u64));
    for &dim in DIMS {
        let codec = TitansLatentCodec::new(dim);
        group.bench_with_input(BenchmarkId::from_parameter(dim), &dim, |b, _| {
            b.iter(|| black_box(codec.encode(black_box(&input)).unwrap()));
        });
    }
    group.finish();
}

/// (2) Full agentic path: text -> neural latent -> EncodedFrame -> SPINE wire
/// bytes. Also prints the one-time on-wire size comparison (3).
fn bench_agentic_path(c: &mut Criterion) {
    let input = sample_text();

    // One-time on-wire size report — the encoding efficiency an agent pays per
    // latent versus sending the same self-describing frame as JSON.
    eprintln!("\nneural-codec EncodedFrame size (input {} B):", input.len());
    eprintln!("  dim   json B   wire B   saved");
    for &dim in DIMS {
        let codec = TitansLatentCodec::new(dim);
        let msg = Message::Encoded(codec.encode(&input).unwrap());
        let json = serde_json::to_vec(&msg).unwrap().len();
        let wire = wire::encode(&msg).unwrap().len();
        let saved = 100 - (wire * 100 / json.max(1));
        eprintln!("  {dim:>4}  {json:>6}  {wire:>6}   {saved:>2}%");
    }

    let mut group = c.benchmark_group("neural_codec_agentic_path");
    for &dim in DIMS {
        let codec = TitansLatentCodec::new(dim);
        group.bench_with_input(BenchmarkId::from_parameter(dim), &dim, |b, _| {
            b.iter(|| {
                let frame = codec.encode(black_box(&input)).unwrap();
                let msg = Message::Encoded(frame);
                black_box(wire::encode(black_box(&msg)).unwrap())
            });
        });
    }
    group.finish();
}

/// (3) Registry-mediated encode: id lookup + dynamic dispatch, the path the
/// gateway's `/v1/embeddings` and the codec-negotiation handshake take.
fn bench_registry_encode(c: &mut Criterion) {
    let input = sample_text();
    let registry = CodecRegistry::new();
    let codec = Arc::new(TitansLatentCodec::new(256));
    let id = codec.id().to_string();
    registry.register(codec);
    c.bench_function("neural_codec_registry_encode_256", |b| {
        b.iter(|| black_box(registry.encode(black_box(&id), black_box(&input)).unwrap()));
    });
}

criterion_group!(
    neural_codec,
    bench_codec_encode,
    bench_agentic_path,
    bench_registry_encode,
);
criterion_main!(neural_codec);
