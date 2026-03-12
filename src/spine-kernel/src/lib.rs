//! # SPINE Kernel - Ultra-Low-Level Primitives
//!
//! This crate provides hardware-level optimizations for the SPINE agentic web stack.
//! It operates at the boundary between software and hardware, extracting maximum
//! performance through direct hardware interaction.
//!
//! ## Design Philosophy
//!
//! The agentic web requires fundamentally different optimizations than human web browsing:
//!
//! | Human Web | Agentic Web |
//! |-----------|-------------|
//! | Rendering-bound | Compute-bound |
//! | Latency-tolerant (100ms+) | Latency-critical (<1ms) |
//! | Single-threaded DOM | Massively parallel inference |
//! | Memory-hungry (500MB+) | Memory-efficient (<1MB/agent) |
//! | Unpredictable access | Predictable vector patterns |
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                          SPINE Kernel                                   │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐   │
//! │  │   SIMD OPS   │ │  ALLOCATOR   │ │  PREFETCH    │ │   ATOMICS    │   │
//! │  │  AVX2/512    │ │  Arena/Slab  │ │  L1/L2/L3    │ │  Lock-free   │   │
//! │  │  NEON        │ │  Huge Pages  │ │  Software    │ │  SeqCst/Acq  │   │
//! │  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘   │
//! ├─────────────────────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐   │
//! │  │   io_uring   │ │  RING BUFFER │ │   TIMING     │ │   SYSCALL    │   │
//! │  │  Kernel BYP  │ │  SPSC/MPMC   │ │  RDTSC/CNT   │ │  Raw mmap    │   │
//! │  │  Batch SQ    │ │  Wait-free   │ │  Picosecond  │ │  Huge pages  │   │
//! │  └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Modules
//!
//! - [`simd`]: SIMD vector operations (AVX2/AVX-512/NEON)
//! - [`alloc`]: Custom allocators (arena, slab, huge pages)
//! - [`atomic`]: Lock-free data structures
//! - [`cache`]: Cache-optimized layouts and prefetching
//! - [`ring`]: Zero-copy ring buffers (SPSC/MPMC)
//! - [`time`]: Sub-nanosecond timing (RDTSC/CNTVCT)
//! - [`syscall`]: Direct system call interface
//! - [`uring`]: io_uring kernel bypass (Linux)

#![allow(dead_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(feature = "avx512", feature(stdarch_x86_avx512))]

pub mod alloc;
pub mod atomic;
pub mod cache;
pub mod ring;
pub mod simd;
pub mod syscall;
pub mod time;

#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub mod uring;

#[cfg(kani)]
mod kani_harnesses;

// Re-exports
pub use alloc::{ArenaAllocator, ArenaStats, BumpAllocator, SlabAllocator};
pub use atomic::{AtomicFlags, LockFreeStack, PaddedAtomicU64, SeqLock, TaggedPtr};
pub use cache::{CacheLine, CacheLineArray, Locality, PrefetchIter};
pub use ring::{MpscRing, RingBatch, SpscRing};
pub use simd::{dot_product, matmul, matmul_flat, softmax, vec_scale_add};
pub use syscall::{get_cpu, numa_info, set_cpu_affinity, set_thread_priority, Priority};
pub use time::{
    calibrate_tsc, measure, rdtsc, Deadline, RateLimiter, TimingStats, TscCalibration, TscTimer,
};

/// Cache line size for the target architecture
pub const CACHE_LINE_SIZE: usize = 64;

/// Page size for memory mapping
pub const PAGE_SIZE: usize = 4096;

/// Huge page size (2MB)
pub const HUGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// Align a value up to the given alignment
#[inline(always)]
pub const fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) & !(align - 1)
}

/// Align a value down to the given alignment
#[inline(always)]
pub const fn align_down(val: usize, align: usize) -> usize {
    val & !(align - 1)
}

/// Check if a value is aligned
#[inline(always)]
pub const fn is_aligned(val: usize, align: usize) -> bool {
    val & (align - 1) == 0
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0, 64), 0);
        assert_eq!(align_up(1, 64), 64);
        assert_eq!(align_up(63, 64), 64);
        assert_eq!(align_up(64, 64), 64);
        assert_eq!(align_up(65, 64), 128);
    }

    #[test]
    fn test_align_down() {
        assert_eq!(align_down(0, 64), 0);
        assert_eq!(align_down(1, 64), 0);
        assert_eq!(align_down(63, 64), 0);
        assert_eq!(align_down(64, 64), 64);
        assert_eq!(align_down(127, 64), 64);
    }

    #[test]
    fn test_is_aligned() {
        assert!(is_aligned(0, 64));
        assert!(!is_aligned(1, 64));
        assert!(is_aligned(64, 64));
        assert!(is_aligned(128, 64));
    }
}
