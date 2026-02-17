//! Kernel Primitives Benchmarks
//!
//! Benchmarks for ultra-low-level operations.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use spine_kernel::*;

fn bench_simd_dot_product(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_dot_product");

    for size in [64, 256, 1024, 4096].iter() {
        let a: Vec<f32> = (0..*size).map(|i| i as f32 * 0.001).collect();
        let b: Vec<f32> = (0..*size).map(|i| (size - i) as f32 * 0.001).collect();

        group.throughput(Throughput::Bytes((*size * 8) as u64)); // 2 vectors
        group.bench_function(format!("size_{}", size), |bench| {
            bench.iter(|| black_box(dot_product(black_box(&a), black_box(&b))));
        });
    }

    group.finish();
}

fn bench_simd_softmax(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_softmax");

    for size in [64, 256, 1024].iter() {
        let input: Vec<f32> = (0..*size)
            .map(|i| (i as f32 - *size as f32 / 2.0) * 0.1)
            .collect();

        group.throughput(Throughput::Bytes((*size * 4) as u64));
        group.bench_function(format!("size_{}", size), |bench| {
            bench.iter(|| {
                let mut data = input.clone();
                softmax(black_box(&mut data))
            });
        });
    }

    group.finish();
}

fn bench_simd_matmul(c: &mut Criterion) {
    let mut group = c.benchmark_group("simd_matmul");

    for size in [64, 128, 256].iter() {
        let rows = *size;
        let cols = *size;

        // Create weights as Vec<Vec<f32>> then convert to &[&[f32]]
        let weights_vec: Vec<Vec<f32>> = (0..rows)
            .map(|r| (0..cols).map(|c| ((r * cols + c) as f32) * 0.001).collect())
            .collect();
        let weights: Vec<&[f32]> = weights_vec.iter().map(|v| v.as_slice()).collect();
        let input: Vec<f32> = (0..cols).map(|i| i as f32 * 0.001).collect();
        let mut output = vec![0.0f32; rows];

        let flops = 2 * rows * cols; // 2 ops per multiply-add per element
        group.throughput(Throughput::Elements(flops as u64));
        group.bench_function(format!("{}x{}", rows, cols), |bench| {
            bench.iter(|| {
                matmul(
                    black_box(&weights),
                    black_box(&input),
                    black_box(&mut output),
                )
            });
        });
    }

    group.finish();
}

fn bench_spsc_ring(c: &mut Criterion) {
    let mut group = c.benchmark_group("spsc_ring");

    let ring: SpscRing<u64, 1024> = SpscRing::new();

    group.throughput(Throughput::Elements(1));
    group.bench_function("push_pop", |bench| {
        bench.iter(|| {
            ring.try_push(black_box(42)).ok();
            black_box(ring.try_pop())
        });
    });

    // Batch throughput
    group.throughput(Throughput::Elements(100));
    group.bench_function("batch_100", |bench| {
        bench.iter(|| {
            for i in 0..100 {
                ring.try_push(black_box(i)).ok();
            }
            for _ in 0..100 {
                black_box(ring.try_pop());
            }
        });
    });

    group.finish();
}

fn bench_bump_allocator(c: &mut Criterion) {
    let mut group = c.benchmark_group("bump_allocator");

    let alloc = BumpAllocator::new(1024 * 1024); // 1MB

    group.throughput(Throughput::Elements(1));
    group.bench_function("alloc_64_bytes", |bench| {
        bench.iter(|| {
            let layout = std::alloc::Layout::from_size_align(64, 8).unwrap();
            black_box(alloc.alloc(layout))
        });
        alloc.reset();
    });

    group.bench_function("alloc_1024_bytes", |bench| {
        bench.iter(|| {
            let layout = std::alloc::Layout::from_size_align(1024, 8).unwrap();
            black_box(alloc.alloc(layout))
        });
        alloc.reset();
    });

    group.finish();
}

fn bench_slab_allocator(c: &mut Criterion) {
    let mut group = c.benchmark_group("slab_allocator");

    let slab = SlabAllocator::new(64, 10000);

    group.throughput(Throughput::Elements(1));
    group.bench_function("alloc_dealloc", |bench| {
        bench.iter(|| {
            let ptr = slab.alloc().unwrap();
            unsafe { slab.dealloc(ptr) };
        });
    });

    group.finish();
}

fn bench_rdtsc(c: &mut Criterion) {
    let mut group = c.benchmark_group("timing");

    group.bench_function("rdtsc", |bench| {
        bench.iter(|| black_box(rdtsc()));
    });

    group.bench_function("instant_now", |bench| {
        bench.iter(|| black_box(std::time::Instant::now()));
    });

    group.finish();
}

fn bench_cache_line(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_line");

    // Array of regular u64
    let regular: Vec<u64> = vec![0; 1024];

    // Array of cache-line padded u64
    let padded: Vec<CacheLine<u64>> = (0..1024).map(|_| CacheLine::new(0)).collect();

    group.throughput(Throughput::Elements(1024));

    group.bench_function("regular_sum", |bench| {
        bench.iter(|| black_box(regular.iter().copied().sum::<u64>()));
    });

    group.bench_function("padded_sum", |bench| {
        bench.iter(|| black_box(padded.iter().map(|c| *c.get()).sum::<u64>()));
    });

    group.finish();
}

fn bench_atomic_flags(c: &mut Criterion) {
    let mut group = c.benchmark_group("atomic_flags");

    let flags = AtomicFlags::new();

    group.bench_function("set_clear", |bench| {
        bench.iter(|| {
            flags.set(black_box(32));
            flags.clear(black_box(32));
        });
    });

    group.bench_function("test_and_set", |bench| {
        bench.iter(|| black_box(flags.test_and_set(black_box(32))));
    });

    group.finish();
}

fn bench_prefetch_iter(c: &mut Criterion) {
    let mut group = c.benchmark_group("prefetch_iter");

    let data: Vec<u64> = (0..10000).collect();

    group.throughput(Throughput::Elements(10000));

    group.bench_function("regular_iter", |bench| {
        bench.iter(|| black_box(data.iter().copied().sum::<u64>()));
    });

    group.bench_function("prefetch_iter", |bench| {
        bench.iter(|| black_box(PrefetchIter::new(&data, 16).copied().sum::<u64>()));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_simd_dot_product,
    bench_simd_softmax,
    bench_simd_matmul,
    bench_spsc_ring,
    bench_bump_allocator,
    bench_slab_allocator,
    bench_rdtsc,
    bench_cache_line,
    bench_atomic_flags,
    bench_prefetch_iter,
);

criterion_main!(benches);
