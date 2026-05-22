//! SPINE over shared-memory IPC — past the TCP loopback ceiling.
//!
//! The TCP path (`llm_tok_per_sec.rs`) saturated at ~728 M tok/s on Windows
//! loopback, limited by the kernel network stack (~2.9 GB/s on this host).
//! For same-host agent communication, TCP is unnecessary: agents can share
//! a ring buffer in memory and pass SPINE frames with zero syscalls and
//! zero kernel involvement.
//!
//! Pattern (the standard SPSC ring used by Aeron / Chronicle / LMAX):
//!
//! * Two SPSC rings: request (client→server) and response (server→client).
//! * Each ring is a power-of-two byte buffer with AtomicU64 head and tail.
//! * Producer writes data, increments head (Release).
//! * Consumer spin-reads head (Acquire), drains up to head, increments tail.
//! * No mutexes, no condvars, no syscalls on the hot path.
//!
//! For each iteration: client writes a SPINE-framed batch of N tokens to the
//! request ring, server thread spin-reads and echoes the frame to the
//! response ring, client spin-reads the response. Same semantics as the TCP
//! bench, but with no kernel transit.

use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use spine_transport::{Frame, FrameFlags, FrameHeader};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// =============================================================================
// SPSC byte ring buffer
// =============================================================================

#[repr(C, align(64))] // cacheline-align head & tail to avoid false sharing
struct RingInner {
    head: AtomicU64,            // bytes written (producer)
    _pad0: [u8; 56],            // pad head onto its own cacheline
    tail: AtomicU64,            // bytes read (consumer)
    _pad1: [u8; 56],
    capacity: usize,            // always power of 2
    mask: usize,                // capacity - 1
    buf: *mut u8,
}

unsafe impl Send for RingInner {}
unsafe impl Sync for RingInner {}

pub struct ShmRing {
    inner: Arc<RingInner>,
    _backing: Arc<Vec<u8>>, // keep backing buffer alive
}

impl ShmRing {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity.is_power_of_two(), "capacity must be power of 2");
        let mut backing = vec![0u8; capacity];
        let ptr = backing.as_mut_ptr();
        let backing = Arc::new(backing);
        let inner = Arc::new(RingInner {
            head: AtomicU64::new(0),
            _pad0: [0; 56],
            tail: AtomicU64::new(0),
            _pad1: [0; 56],
            capacity,
            mask: capacity - 1,
            buf: ptr,
        });
        Self { inner, _backing: backing }
    }

    pub fn clone_handle(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            _backing: Arc::clone(&self._backing),
        }
    }

    /// Write `data` into the ring, spinning while full. Returns once the
    /// last byte is in the buffer and head has been advanced.
    #[inline]
    pub fn write_all(&self, data: &[u8]) {
        let n = data.len();
        let inner = &*self.inner;
        let mut head = inner.head.load(Ordering::Relaxed);
        loop {
            let tail = inner.tail.load(Ordering::Acquire);
            let free = inner.capacity - (head - tail) as usize;
            if free >= n {
                break;
            }
            // Hybrid backoff: brief spin then yield. Pure spin-loop livelocks
            // on Windows when threads aren't core-pinned.
            for _ in 0..64 {
                std::hint::spin_loop();
            }
            std::thread::yield_now();
        }
        // Copy in two segments if wrap.
        let off = (head as usize) & inner.mask;
        let first = (inner.capacity - off).min(n);
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), inner.buf.add(off), first);
            if first < n {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr().add(first),
                    inner.buf,
                    n - first,
                );
            }
        }
        head += n as u64;
        inner.head.store(head, Ordering::Release);
    }

    /// Read exactly `n` bytes into `out`, spinning while empty.
    #[inline]
    pub fn read_exact(&self, out: &mut [u8]) {
        let n = out.len();
        let inner = &*self.inner;
        let mut tail = inner.tail.load(Ordering::Relaxed);
        loop {
            let head = inner.head.load(Ordering::Acquire);
            if (head - tail) as usize >= n {
                break;
            }
            // Hybrid backoff: brief spin then yield. Pure spin-loop livelocks
            // on Windows when threads aren't core-pinned.
            for _ in 0..64 {
                std::hint::spin_loop();
            }
            std::thread::yield_now();
        }
        let off = (tail as usize) & inner.mask;
        let first = (inner.capacity - off).min(n);
        unsafe {
            std::ptr::copy_nonoverlapping(inner.buf.add(off), out.as_mut_ptr(), first);
            if first < n {
                std::ptr::copy_nonoverlapping(
                    inner.buf,
                    out.as_mut_ptr().add(first),
                    n - first,
                );
            }
        }
        tail += n as u64;
        inner.tail.store(tail, Ordering::Release);
    }
}

// =============================================================================
// Bidirectional SPINE-over-shared-memory transport
// =============================================================================

pub struct ShmTransport {
    pub req: ShmRing,
    pub resp: ShmRing,
}

impl ShmTransport {
    pub fn new_pair(capacity: usize) -> (Self, Self) {
        // Two rings, each end gets the *opposite* directions of the two.
        let r1 = ShmRing::new(capacity);
        let r2 = ShmRing::new(capacity);
        let client = Self {
            req: r1.clone_handle(),
            resp: r2.clone_handle(),
        };
        let server = Self { req: r1, resp: r2 };
        (client, server)
    }
}

// =============================================================================
// Server: spin-poll request ring, echo each complete SPINE frame to response.
// =============================================================================

fn spawn_shm_echo(server: ShmTransport, stop: Arc<AtomicU64>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        // One contiguous frame buffer; read header first (in place), then the
        // rest of the payload, then write the WHOLE thing back in ONE call.
        // The original 4-op (read hdr / read payload / write hdr / write
        // payload) pattern doubled the ring round-trips per request.
        let mut frame = vec![0u8; 1 << 20];
        loop {
            if stop.load(Ordering::Relaxed) != 0 {
                return;
            }
            server.req.read_exact(&mut frame[..12]);
            let length =
                u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
            let total = 12 + length;
            if total > frame.len() {
                frame.resize(total, 0);
            }
            server.req.read_exact(&mut frame[12..total]);
            server.resp.write_all(&frame[..total]);
        }
    })
}

// =============================================================================
// Token helpers (same as llm_tok_per_sec.rs)
// =============================================================================

fn make_token_ids(n: usize) -> Vec<u32> {
    (0..n as u32).map(|i| i.wrapping_mul(2654435761)).collect()
}

fn tokens_to_binary(token_ids: &[u32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(token_ids.len() * 4);
    for id in token_ids {
        v.extend_from_slice(&id.to_le_bytes());
    }
    v
}

fn tokens_to_spine_frame(token_ids: &[u32]) -> Vec<u8> {
    let payload_len = token_ids.len() * 4;
    let frame = Frame {
        header: FrameHeader {
            length: payload_len as u32,
            flags: FrameFlags::empty(),
            sequence: 1,
            stream_id: 1,
            _reserved: 0,
        },
        payload: Bytes::from(tokens_to_binary(token_ids)),
    };
    let mut v = Vec::with_capacity(12 + payload_len);
    v.extend_from_slice(&frame.header_bytes());
    v.extend_from_slice(&frame.payload);
    v
}

// =============================================================================
// SHM batch tok/s
// =============================================================================

fn bench_shm_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("llm_shm_tok_per_sec");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(4));

    for &n_tokens in [1024usize, 4096, 16384, 65536, 262_144].iter() {
        group.throughput(Throughput::Elements(n_tokens as u64));

        let token_ids = make_token_ids(n_tokens);
        let spine_frame = tokens_to_spine_frame(&token_ids);
        let frame_len = spine_frame.len();
        // Ring capacity must hold one full frame at minimum; round up to pow2.
        let ring_cap = ((frame_len * 4).next_power_of_two()).max(1 << 16);

        group.bench_with_input(BenchmarkId::new("spine_shm", n_tokens), &n_tokens, |b, _| {
            let (client, server) = ShmTransport::new_pair(ring_cap);
            let stop = Arc::new(AtomicU64::new(0));
            let stop_h = Arc::clone(&stop);
            let server_handle = spawn_shm_echo(server, stop_h);
            // Warm-up: do one roundtrip outside the bench to ensure the
            // server thread is in steady state.
            client.req.write_all(&spine_frame);
            let mut tmp = vec![0u8; frame_len];
            client.resp.read_exact(&mut tmp);

            let mut recv = vec![0u8; frame_len];
            b.iter(|| {
                client.req.write_all(&spine_frame);
                client.resp.read_exact(&mut recv);
                black_box(&recv);
            });

            // Abandon the server thread; the bench process tear-down ends it.
            // Storing stop is best-effort; the thread is spinning on a ring
            // read which we don't drain (would require another roundtrip).
            stop.store(1, Ordering::Relaxed);
            std::mem::forget(server_handle);
        });
    }

    group.finish();
}

// =============================================================================
// Pipelined SHM — multiple in-flight frames
// =============================================================================

fn bench_shm_pipelined(c: &mut Criterion) {
    let mut group = c.benchmark_group("llm_shm_pipelined_tok_per_sec");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(4));

    const TOKENS_PER_REQ: usize = 4096;

    for &k in [4usize, 16, 64].iter() {
        let total_tokens = (k * TOKENS_PER_REQ) as u64;
        group.throughput(Throughput::Elements(total_tokens));

        let token_ids = make_token_ids(TOKENS_PER_REQ);
        // K concatenated frames in one big send buffer.
        let mut send_buf = Vec::with_capacity(k * (12 + TOKENS_PER_REQ * 4));
        for i in 0..k {
            let frame = Frame {
                header: FrameHeader {
                    length: (TOKENS_PER_REQ * 4) as u32,
                    flags: FrameFlags::empty(),
                    sequence: 1,
                    stream_id: (i as u16) + 1,
                    _reserved: 0,
                },
                payload: Bytes::from(tokens_to_binary(&token_ids)),
            };
            send_buf.extend_from_slice(&frame.header_bytes());
            send_buf.extend_from_slice(&frame.payload);
        }
        let total_bytes = send_buf.len();
        let ring_cap = (total_bytes * 4).next_power_of_two().max(1 << 16);

        group.bench_with_input(BenchmarkId::new("spine_shm", k), &k, |b, _| {
            let (client, server) = ShmTransport::new_pair(ring_cap);
            let stop = Arc::new(AtomicU64::new(0));
            let stop_h = Arc::clone(&stop);
            let server_handle = spawn_shm_echo(server, stop_h);

            // Warm-up
            client.req.write_all(&send_buf);
            let mut warmup = vec![0u8; total_bytes];
            client.resp.read_exact(&mut warmup);

            let mut recv = vec![0u8; total_bytes];
            b.iter(|| {
                client.req.write_all(&send_buf);
                client.resp.read_exact(&mut recv);
                black_box(&recv);
            });

            stop.store(1, Ordering::Relaxed);
            std::mem::forget(server_handle);
        });
    }

    group.finish();
}

criterion_group!(benches, bench_shm_batch, bench_shm_pipelined);
criterion_main!(benches);
