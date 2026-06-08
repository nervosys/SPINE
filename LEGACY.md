# Legacy & Un-validated Performance Tables

This file holds performance tables that were **removed from `README.md` because their
numbers could not be validated** on re-measurement (2026-06-08). They are preserved here
for historical context only.

**Do not cite any number in this file.** Each table below carries the reason it could not
be validated. Two distinct failure modes appear:

1. **Un-validatable by construction** — the comparison's baseline is a hand-rolled fake, a
   category error, or has no implementation in this repository. The 2026-05 audit
   (`BENCHMARK_REPORT.md`) already established these are not like-for-like measurements.
2. **Did not reproduce** — absolute micro-benchmark figures that were "validated January
   2026" on unknown hardware and do **not** reproduce when the backing bench is re-run on
   current hardware (often off by ~2×).

For the numbers that *are* current and reproducible, see the **Performance** section of
`README.md` (transport vs real HTTP/2, agentic embedding/token throughput, and a small
re-measured micro-benchmark table), all re-run 2026-06-08.

---

## 1. SPINE vs Traditional Web Stack

**Why moved:** from `src/spine-transport/benches/traditional_comparison.rs`, which the
2026-05 audit found compares hand-rolled fakes (XOR pretending to be AES-GCM, a 10-line
string split pretending to be `serde_json`, `Vec::clone` pretending to be Redis pub/sub)
against optimized SPINE code. The four- and five-digit ratios are **not supported by
like-for-like measurement** and cannot be validated.

### Serialization: JSON vs SPINE Zero-Copy

| Data Size   | JSON Roundtrip      | SPINE Zero-Copy     | **Speedup** |
| ----------- | ------------------- | ------------------- | ----------- |
| 10 fields   | 1.77 µs (195 MiB/s) | 4.3 ns (77 GiB/s)   | **411×**    |
| 100 fields  | 18.1 µs (199 MiB/s) | 20.4 ns (172 GiB/s) | **886×**    |
| 1000 fields | 203 µs (187 MiB/s)  | 320 ns (115 GiB/s)  | **634×**    |

### Header Parsing: HTTP vs SPINE Binary

| Protocol            | Time    | Throughput  | **Speedup** |
| ------------------- | ------- | ----------- | ----------- |
| HTTP Header Parse   | 1.41 µs | 708K elem/s | -           |
| SPINE Binary Header | 3.3 ns  | 299M elem/s | **427×**    |

### Context Processing: 128K Chunks vs SPINE RLM

| Context Size | Traditional (128K chunks) | SPINE RLM | **Speedup**  |
| ------------ | ------------------------- | --------- | ------------ |
| 100K chars   | 731 ns                    | 280 ps    | **2,610×**   |
| 1M chars     | 7.48 µs                   | 443 ps    | **16,883×**  |
| 10M chars    | 77.9 µs                   | 316 ps    | **246,500×** |

### Connection Handling: HTTP Keep-Alive vs SPINE Multiplexing

| Requests | HTTP Keep-Alive | SPINE Multiplexed | **Speedup** |
| -------- | --------------- | ----------------- | ----------- |
| 100      | 26.2 µs         | 11.2 ns           | **2,339×**  |
| 1,000    | 287 µs          | 140 ns            | **2,050×**  |
| 10,000   | 2.83 ms         | 1.0 µs            | **2,830×**  |

---

## 2. Real-World Application Benchmark

**Why moved:** the "Traditional Stack" column has **no corresponding implementation** in
this repository — the figures are illustrative estimates, not measured comparisons, and
cannot be validated.

Competitive Intelligence demo: 50 agents analyzing competitor websites, extracting
insights, building a knowledge graph.

| Metric                | Traditional Stack | SPINE         | **Advantage**       |
| --------------------- | ----------------- | ------------- | ------------------- |
| Cold Start            | ~5,000 ms         | 32 ms         | **156×**            |
| 50 Agent Swarm        | ~10,000 ms        | 40 ms         | **256×**            |
| Memory (50 agents)    | ~25 GB            | ~50 MB        | **500×**            |
| Max Context           | 128K tokens       | **UNLIMITED** | ∞                   |
| 10.7M char load       | FAILS             | 52 ms         | ✅                   |
| 10.7M char search     | FAILS             | 81 µs         | ✅                   |
| Knowledge Graph Build | External DB       | 407 µs        | **~1000×**          |
| Encryption            | Static TLS        | Moving-target | ✅ Quantum-resistant |
| **Total Processing**  | **~15 seconds**   | **127 ms**    | **118×**            |

---

## 3. Component Benchmarks

**Why moved:** absolute internal-operation throughput, "validated January 2026" on unknown
hardware. On a 2026-06-08 re-run of the backing benches (`transport_bench.rs`), the
flagship figures **did not reproduce** — frame-codec throughput is roughly **half** what
this table claims, and several rows (Latent Serialize, Cosine Similarity, Zero-Copy Buffer)
have no trustworthy backing bench (Zero-Copy Buffer appeared only in the retracted
`tcp_comparison.rs`).

| Component                    | Metric (claimed) | Throughput (claimed) | 2026-06-08 re-run        |
| ---------------------------- | ---------------- | -------------------- | ------------------------ |
| Latent Serialize (128-dim)   | 80 ns            | 6.0 GiB/s            | no backing bench         |
| Latent Serialize (512-dim)   | 108 ns           | 17.6 GiB/s           | no backing bench         |
| Latent Serialize (1024-dim)  | 143 ns           | 26.8 GiB/s           | no backing bench         |
| Cosine Similarity (128-dim)  | 47 ns            | 10.1 GiB/s           | no backing bench         |
| Cosine Similarity (1024-dim) | 373 ns           | 10.2 GiB/s           | no backing bench         |
| Frame Encode (8KB)           | 68 ns            | 110 GiB/s            | 149 ns / **51 GiB/s** ✗  |
| Frame Decode (8KB)           | 54 ns            | 141 GiB/s            | 123 ns / **62 GiB/s** ✗  |
| Zero-Copy Buffer (8KB)       | 131 ns           | 58 GiB/s             | only in retracted bench  |
| BBR Pacing Decision          | 275 ps           | -                    | 302 ps ✓ (close)         |
| Batch Encode (64 frames)     | 2.5 µs           | 25.8 Melem/s         | 3.31 µs / 19.3 Melem/s   |
| Backpressure Stream (10K)    | 2.1 ms           | 4.9 Melem/s          | not re-run               |
| Priority Queue (10K)         | 1.9 ms           | 5.4 Melem/s          | not re-run               |
| Ring Buffer (16KB)           | 300 ns           | 50.4 GiB/s           | 391 ns / 39 GiB/s ✗      |
| Context Chunking (10M)       | 2.4 ms           | 3.9 GiB/s            | not re-run               |

---

## 4. SPINE vs Standard TCP/IP Stack

**Why moved:** the speedup column derives from `tcp_comparison.rs`, which the audit calls a
**category error** — the "Standard TCP" side does real loopback socket I/O while the
"SPINE" side does pure in-memory frame encode/decode with no socket. The ratios are not a
like-for-like measurement and cannot be validated.

| Benchmark                | Standard TCP | SPINE    | Speedup   |
| ------------------------ | ------------ | -------- | --------- |
| **Latency (64 bytes)**   | 41.0 µs      | 60 ns    | **682×**  |
| **Latency (256 bytes)**  | 27.2 µs      | 82 ns    | **331×**  |
| **Latency (1024 bytes)** | 31.2 µs      | 99 ns    | **315×**  |
| **Latency (4096 bytes)** | 27.0 µs      | 102 ns   | **265×**  |
| **Throughput (1KB)**     | 34 MiB/s     | 21 GiB/s | **632×**  |
| **Throughput (8KB)**     | 359 MiB/s    | 58 GiB/s | **166×**  |
| **Frame Encode (8KB)**   | -            | 68 ns    | 110 GiB/s |
| **Frame Decode (8KB)**   | -            | 54 ns    | 141 GiB/s |
| **Ring Buffer (16KB)**   | -            | 300 ns   | 50 GiB/s  |
| **BBR Congestion Ctrl**  | N/A          | 109 ns   | -         |

(For an honest single-stream comparison against a *real* modern transport, see the
SPINE-vs-HTTP/2 numbers in `README.md` / `BENCHMARK_REPORT.md`.)

---

## 5. Kernel Primitives (spine-kernel)

**Why moved:** absolute figures that drift on re-measurement. The 2026-06-08 re-run of
`kernel_bench.rs` reproduces the *claims* directionally (this hardware is generally a bit
faster), but the **specific figures do not match**, so the table as written cannot be
validated. The re-measured values are kept in the README micro-benchmark table.

| Operation            | Size     | Time (claimed) | Throughput (claimed)          | 2026-06-08 re-run     |
| -------------------- | -------- | -------------- | ----------------------------- | --------------------- |
| **SIMD Dot Product** | 256      | 33 ns          | 57 GiB/s                      | 30.8 ns / 62.0 GiB/s  |
| **SIMD MatVec**      | 256×256  | 8.5 µs         | 15.5 Gelem/s                  | 8.24 µs / 15.9 Gelem/s |
| **SPSC Ring**        | push+pop | 1.36 ns        | 736 Melem/s                   | 1.21 ns / 829 Melem/s |
| **Bump Allocator**   | 64 bytes | 505 ps         | 1.98 Galloc/s                 | 349 ps / 2.87 Galloc/s |
| **RDTSC Timing**     | -        | 9.3 ns         | 2.6× faster than Instant::now | 7.14 ns / 3.3× faster |
| **Atomic Flags**     | test+set | 4.4 ns         | -                             | 3.84 ns               |

---

## 6. "Key Insights" and "Summary: Why SPINE Dominates"

**Why moved:** these aggregate the ratios from the tables above (all un-validatable), so the
roll-up factors inherit the same problems.

### Key Insights (as written)

- **265-682× lower latency** for messages (frame codec vs TCP roundtrip) — derives from the
  category-error TCP/IP table (§4).
- **166-632× higher throughput** using zero-copy ring buffers — same source.
- Frame codec achieves **110-141 GiB/s** encode/decode throughput — re-measured at
  **51-62 GiB/s** (§3), ~2× lower.
- BBR congestion control adds only **109 ns** overhead per decision — `on_ack` re-measured
  at ~130 ns (close).
- Pacing decisions take only **275 picoseconds** — re-measured ~302 ps (close).

### Summary: Why SPINE Dominates (as written)

| Category           | Traditional         | SPINE                   | Factor       |
| ------------------ | ------------------- | ----------------------- | ------------ |
| **Serialization**  | 187 MiB/s           | 115 GiB/s               | **630×**     |
| **Header Parsing** | 708K/s              | 299M/s                  | **422×**     |
| **Context Access** | O(n) chunking       | O(1) random             | **250,000×** |
| **Connections**    | Per-request parsing | Multiplexed streams     | **2,500×**   |
| **Latency (TCP)**  | 27-41 µs            | 60-102 ns               | **300-680×** |
| **Memory**         | 500 MB/browser      | 1 MB/agent              | **500×**     |
| **Context Limit**  | 128K tokens         | **UNLIMITED**           | ∞            |
| **Security**       | Static TLS          | Moving-target + Quantum | ✅            |
