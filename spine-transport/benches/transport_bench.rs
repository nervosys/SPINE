//! Transport layer benchmarks

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Duration;

use spine_transport::{
    buffer::{HierarchicalAllocator, RingBuffer, SlabAllocator, VectoredBuffer},
    coalesce::{CoalesceConfig, MessageCoalescer},
    congestion::{BbrController, RateLimiter},
    frame::{FrameAggregator, FrameBuilder, FrameCodec, FrameFragmenter},
    BufferAllocator, Frame,
};

fn bench_ring_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("ring_buffer");

    for size in [64, 256, 1024, 4096, 16384].iter() {
        group.throughput(Throughput::Bytes(*size as u64));

        group.bench_with_input(BenchmarkId::new("write_read", size), size, |b, &size| {
            let buffer = RingBuffer::new(64 * 1024);
            let data = vec![42u8; size];

            b.iter(|| {
                buffer.write(&data);
                let mut out = vec![0u8; size];
                buffer.read(&mut out);
            });
        });
    }

    group.finish();
}

fn bench_slab_allocator(c: &mut Criterion) {
    let mut group = c.benchmark_group("slab_allocator");

    group.bench_function("borrow_return_cycle", |b| {
        let allocator = SlabAllocator::new(1024, 64);

        b.iter(|| {
            let buf = allocator.borrow();
            allocator.return_buffer(buf);
        });
    });

    group.bench_function("burst_borrow", |b| {
        let allocator = SlabAllocator::new(1024, 64);

        b.iter(|| {
            let mut bufs = Vec::with_capacity(32);
            for _ in 0..32 {
                bufs.push(allocator.borrow());
            }
            for buf in bufs {
                allocator.return_buffer(buf);
            }
        });
    });

    group.finish();
}

fn bench_hierarchical_allocator(c: &mut Criterion) {
    let mut group = c.benchmark_group("hierarchical_allocator");

    for size in [512, 4096, 32768, 262144].iter() {
        group.bench_with_input(BenchmarkId::new("alloc", size), size, |b, &size| {
            let allocator = HierarchicalAllocator::new();

            b.iter(|| allocator.allocate(size));
        });
    }

    group.finish();
}

fn bench_frame_codec(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_codec");

    for payload_size in [64, 256, 1024, 8192].iter() {
        group.throughput(Throughput::Bytes(*payload_size as u64));

        group.bench_with_input(
            BenchmarkId::new("encode", payload_size),
            payload_size,
            |b, &size| {
                let codec = FrameCodec::new(1024 * 1024);
                let frame = FrameBuilder::new()
                    .payload(vec![42u8; size])
                    .sequence(1)
                    .stream_id(1)
                    .build();

                b.iter(|| codec.encode(&frame));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("decode", payload_size),
            payload_size,
            |b, &size| {
                let mut codec = FrameCodec::new(1024 * 1024);
                let frame = FrameBuilder::new()
                    .payload(vec![42u8; size])
                    .sequence(1)
                    .stream_id(1)
                    .build();
                let encoded = codec.encode(&frame);

                b.iter(|| codec.decode(&encoded).unwrap());
            },
        );
    }

    group.finish();
}

fn bench_frame_aggregator(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_aggregator");

    group.bench_function("aggregate_16_frames", |b| {
        let mut agg = FrameAggregator::new(64 * 1024, 16);
        let frame = FrameBuilder::new().payload(vec![42u8; 256]).build();

        b.iter(|| {
            for _ in 0..16 {
                agg.add(frame.clone());
            }
            agg.take()
        });
    });

    group.finish();
}

fn bench_frame_fragmenter(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_fragmenter");

    for payload_size in [4096, 16384, 65536].iter() {
        group.throughput(Throughput::Bytes(*payload_size as u64));

        group.bench_with_input(
            BenchmarkId::new("fragment", payload_size),
            payload_size,
            |b, &size| {
                let mut fragmenter = FrameFragmenter::new(1024);
                let frame = FrameBuilder::new().payload(vec![42u8; size]).build();

                b.iter(|| fragmenter.fragment(frame.clone()));
            },
        );
    }

    group.finish();
}

fn bench_coalescer(c: &mut Criterion) {
    let mut group = c.benchmark_group("coalescer");

    group.bench_function("coalesce_batch", |b| {
        let config = CoalesceConfig {
            max_batch_bytes: 64 * 1024,
            max_batch_count: 64,
            ..Default::default()
        };
        let mut coalescer = MessageCoalescer::new(config);

        b.iter(|| {
            for i in 0..64 {
                let frame = FrameBuilder::new()
                    .payload(vec![i as u8; 128])
                    .sequence(i)
                    .build();
                coalescer.queue(frame);
            }
            coalescer.flush()
        });
    });

    group.bench_function("coalesce_with_compression", |b| {
        let config = CoalesceConfig {
            max_batch_bytes: 64 * 1024,
            max_batch_count: 64,
            compress_batches: true,
            compression_threshold: 256,
            ..Default::default()
        };
        let mut coalescer = MessageCoalescer::new(config);

        b.iter(|| {
            for i in 0..64 {
                let frame = FrameBuilder::new()
                    .payload(vec![i as u8; 128])
                    .sequence(i)
                    .build();
                coalescer.queue(frame);
            }
            coalescer.flush()
        });
    });

    group.finish();
}

fn bench_bbr_controller(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbr");

    group.bench_function("on_ack", |b| {
        let mut bbr = BbrController::new();

        b.iter(|| bbr.on_ack(1000, Duration::from_micros(100)));
    });

    group.bench_function("pacing_decisions", |b| {
        let mut bbr = BbrController::new();
        // Simulate steady state
        for _ in 0..100 {
            bbr.on_ack(10000, Duration::from_micros(100));
        }

        b.iter(|| {
            let pacing = bbr.pacing_rate();
            let cwnd = bbr.cwnd();
            (pacing, cwnd)
        });
    });

    group.finish();
}

fn bench_rate_limiter(c: &mut Criterion) {
    let mut group = c.benchmark_group("rate_limiter");

    group.bench_function("try_consume", |b| {
        let mut limiter = RateLimiter::new(1_000_000_000, 65536); // 1Gbps

        b.iter(|| limiter.try_consume(1500));
    });

    group.finish();
}

fn bench_vectored_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("vectored_buffer");

    group.bench_function("add_chunks", |b| {
        let mut buffer = VectoredBuffer::with_capacity(32);

        b.iter(|| {
            buffer.clear();
            for i in 0..16 {
                buffer.push(Bytes::from(vec![i as u8; 256]));
            }
            buffer.len()
        });
    });

    group.bench_function("flatten", |b| {
        let mut buffer = VectoredBuffer::with_capacity(32);
        for i in 0..16 {
            buffer.push(Bytes::from(vec![i as u8; 64]));
        }

        b.iter(|| buffer.flatten());
    });

    group.finish();
}

fn bench_frame_batch_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_encode");
    group.throughput(Throughput::Elements(64));

    group.bench_function("64_frames", |b| {
        let codec = FrameCodec::new(1024 * 1024);
        let frames: Vec<Frame> = (0..64)
            .map(|i| {
                FrameBuilder::new()
                    .payload(vec![i as u8; 256])
                    .sequence(i)
                    .stream_id(1)
                    .build()
            })
            .collect();

        b.iter(|| {
            let mut encoded = Vec::with_capacity(64);
            for frame in &frames {
                encoded.push(codec.encode(frame));
            }
            encoded
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_ring_buffer,
    bench_slab_allocator,
    bench_hierarchical_allocator,
    bench_frame_codec,
    bench_frame_aggregator,
    bench_frame_fragmenter,
    bench_coalescer,
    bench_bbr_controller,
    bench_rate_limiter,
    bench_vectored_buffer,
    bench_frame_batch_encode,
);

criterion_main!(benches);
