# Kernel Primitives

`spine-kernel` provides ultra-low-level hardware primitives for maximum performance.

## SIMD Intrinsics

AVX2 (x86_64) and NEON (aarch64) accelerated operations:

```rust
use spine_kernel::simd::{dot_product_avx2, softmax_avx2, matvec_avx2};

let a = vec![1.0f32; 256];
let b = vec![2.0f32; 256];
let dot = dot_product_avx2(&a, &b);  // 57 GiB/s
```

### Performance

| Operation         | Throughput   |
| ----------------- | ------------ |
| Dot Product (256) | 57 GiB/s     |
| MatVec (256×256)  | 15.5 Gelem/s |
| Softmax (256)     | ~10 GiB/s    |

## Custom Allocators

### Bump Allocator (505 ps per allocation)

```rust
use spine_kernel::alloc::BumpAllocator;

let mut bump = BumpAllocator::new(4096);
let ptr = bump.alloc(64);  // 505 picoseconds
bump.reset();               // Free everything at once
```

### Slab Allocator

Fixed-size object pools for zero-fragmentation allocation.

### Arena Allocator

Region-based allocation with batch deallocation.

## Lock-Free Atomics

### SPSC Ring Buffer (1.36 ns per op, 700M ops/sec)

```rust
use spine_kernel::ring::SpscRing;

let (mut tx, mut rx) = SpscRing::<u64>::new(1024);
tx.push(42);
let val = rx.pop();
```

### SeqLock (4.4 ns reads)

Read-optimized lock for rarely-written data.

## RDTSC Timing

Sub-nanosecond measurement (2.6× faster than `Instant::now()`):

```rust
use spine_kernel::timing::rdtsc;

let start = rdtsc();
// ... work ...
let cycles = rdtsc() - start;
```
