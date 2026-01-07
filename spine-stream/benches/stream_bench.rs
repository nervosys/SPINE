//! Benchmarks for spine-stream

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tokio::runtime::Runtime;

fn benchmark_backpressure_stream(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("backpressure_stream");

    for size in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("send_recv", size), size, |b, &size| {
            b.to_async(&rt).iter(|| async move {
                use spine_stream::BackpressureStream;

                let (tx, mut rx) = BackpressureStream::<u64>::new(1024);

                let sender = tokio::spawn(async move {
                    for i in 0..size {
                        tx.send(i as u64).await.unwrap();
                    }
                });

                let mut count = 0;
                while let Some(_) = rx.recv().await {
                    count += 1;
                    if count >= size {
                        break;
                    }
                }

                sender.abort();
            });
        });
    }

    group.finish();
}

fn benchmark_priority_queue(c: &mut Criterion) {
    let mut group = c.benchmark_group("priority_queue");

    for size in [100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("push_pop", size), size, |b, &size| {
            use spine_stream::{PriorityQueue, StreamMessage, StreamPayload};
            use uuid::Uuid;

            b.iter(|| {
                let pq = PriorityQueue::new(size + 1);

                for i in 0..size {
                    let msg = StreamMessage {
                        id: Uuid::new_v4(),
                        stream_id: 1,
                        sequence: i as u64,
                        payload: StreamPayload::Bytes(b"test".to_vec()),
                        priority: (i % 8) as u8,
                        timestamp_ns: 0,
                        correlation_id: None,
                    };
                    pq.push(msg).unwrap();
                }

                while pq.pop().is_some() {}
            });
        });
    }

    group.finish();
}

fn benchmark_chunking(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("chunking");

    for size in [1024, 10240, 102400].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::new("chunk_send", size), size, |b, &size| {
            b.to_async(&rt).iter(|| async move {
                use spine_stream::{ChunkConfig, ChunkedReceiver, ChunkedSender};
                use tokio::sync::mpsc;

                let (tx, mut rx) = mpsc::channel(1024);
                let config = ChunkConfig {
                    max_chunk_size: 1024,
                    compress_chunks: false,
                    ..Default::default()
                };

                let sender = ChunkedSender::new(config.clone(), tx, 1);
                let receiver = ChunkedReceiver::new(config);

                let data = Bytes::from(vec![0u8; size]);
                sender.send(data).await.unwrap();

                while let Some(msg) = rx.recv().await {
                    if let spine_stream::StreamPayload::Chunk { meta, data } = msg.payload {
                        if let Some(_) = receiver.process_chunk(meta, Bytes::from(data)).unwrap() {
                            break;
                        }
                    }
                }
            });
        });
    }

    group.finish();
}

fn benchmark_flow_control(c: &mut Criterion) {
    let mut group = c.benchmark_group("flow_control");

    group.bench_function("acquire_release", |b| {
        use spine_stream::{FlowConfig, FlowController};

        let fc = FlowController::new(FlowConfig {
            initial_window: 1_000_000,
            ..Default::default()
        });

        b.iter(|| {
            assert!(fc.try_acquire(1000));
            fc.release(1000);
        });
    });

    group.finish();
}

fn benchmark_latent_vectors(c: &mut Criterion) {
    let mut group = c.benchmark_group("latent_vectors");

    for dims in [128, 512, 1024].iter() {
        group.throughput(Throughput::Bytes((*dims * 4) as u64));
        group.bench_with_input(BenchmarkId::new("serialize", dims), dims, |b, &dims| {
            use spine_stream::latent::LatentVector;

            let vector = LatentVector::new(dims as u32, vec![0.5f32; dims]);

            b.iter(|| {
                let bytes = vector.to_bytes();
                let _restored = LatentVector::from_bytes(bytes).unwrap();
            });
        });

        group.bench_with_input(
            BenchmarkId::new("cosine_similarity", dims),
            dims,
            |b, &dims| {
                use spine_stream::latent::LatentVector;

                let v1 = LatentVector::new(dims as u32, vec![0.5f32; dims]);
                let v2 = LatentVector::new(dims as u32, vec![0.3f32; dims]);

                b.iter(|| v1.cosine_similarity(&v2));
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_backpressure_stream,
    benchmark_priority_queue,
    benchmark_chunking,
    benchmark_flow_control,
    benchmark_latent_vectors,
);

criterion_main!(benches);
