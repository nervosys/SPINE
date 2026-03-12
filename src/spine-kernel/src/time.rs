//! Sub-Nanosecond Timing
//!
//! Ultra-precise timing using hardware counters:
//! - RDTSC/RDTSCP on x86_64
//! - CNTVCT_EL0 on ARM64
//! - Monotonic clocks for portability
//!
//! Provides cycle-accurate measurements for profiling hot paths.

use std::time::{Duration, Instant};

// =============================================================================
// HARDWARE TIMESTAMP COUNTER
// =============================================================================

/// Read the CPU timestamp counter
///
/// Returns a raw cycle count. To convert to nanoseconds, divide by
/// CPU frequency (GHz). E.g., 3 GHz ≈ 3 cycles/ns.
///
/// **Note**: TSC frequency may vary with CPU frequency scaling.
/// Use `calibrate_tsc()` for accurate timing.
#[inline]
pub fn rdtsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::x86_64::_rdtsc()
    }

    #[cfg(target_arch = "aarch64")]
    unsafe {
        let val: u64;
        std::arch::asm!("mrs {}, cntvct_el0", out(reg) val);
        val
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        // Fallback: use std Instant (less precise)
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(Instant::now);
        start.elapsed().as_nanos() as u64
    }
}

/// Read timestamp counter with serialization (x86_64)
///
/// RDTSCP ensures all previous instructions complete before reading.
/// More accurate for benchmarking but ~10 cycles slower.
#[inline]
pub fn rdtscp() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let mut _aux: u32 = 0;
        std::arch::x86_64::__rdtscp(&mut _aux)
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        rdtsc()
    }
}

// =============================================================================
// TSC CALIBRATION
// =============================================================================

/// TSC calibration result
#[derive(Debug, Clone, Copy)]
pub struct TscCalibration {
    /// Cycles per nanosecond (floating point for precision)
    pub cycles_per_ns: f64,
    /// Nanoseconds per cycle
    pub ns_per_cycle: f64,
    /// Estimated CPU frequency in MHz
    pub freq_mhz: u64,
}

/// Calibrate the TSC against system clock
///
/// Runs a calibration loop to determine cycles/nanosecond.
/// Call once at startup and cache the result.
pub fn calibrate_tsc() -> TscCalibration {
    const CALIBRATION_MS: u64 = 10;

    // Warmup
    for _ in 0..1000 {
        let _ = rdtsc();
    }

    let start_instant = Instant::now();
    let start_tsc = rdtsc();

    std::thread::sleep(Duration::from_millis(CALIBRATION_MS));

    let end_tsc = rdtsc();
    let elapsed = start_instant.elapsed();

    let cycles = end_tsc.saturating_sub(start_tsc);
    let nanos = elapsed.as_nanos() as f64;

    let cycles_per_ns = cycles as f64 / nanos;
    let ns_per_cycle = nanos / cycles as f64;
    let freq_mhz = (cycles_per_ns * 1000.0) as u64;

    TscCalibration {
        cycles_per_ns,
        ns_per_cycle,
        freq_mhz,
    }
}

/// Convert TSC cycles to nanoseconds
#[inline]
pub fn cycles_to_nanos(cycles: u64, calibration: &TscCalibration) -> u64 {
    (cycles as f64 * calibration.ns_per_cycle) as u64
}

/// Convert nanoseconds to TSC cycles
#[inline]
pub fn nanos_to_cycles(nanos: u64, calibration: &TscCalibration) -> u64 {
    (nanos as f64 * calibration.cycles_per_ns) as u64
}

// =============================================================================
// HIGH-PRECISION TIMER
// =============================================================================

/// High-precision timer using TSC
pub struct TscTimer {
    start: u64,
    calibration: TscCalibration,
}

impl TscTimer {
    /// Create and start a new timer
    pub fn start(calibration: TscCalibration) -> Self {
        Self {
            start: rdtsc(),
            calibration,
        }
    }

    /// Elapsed cycles
    #[inline]
    pub fn elapsed_cycles(&self) -> u64 {
        rdtsc().saturating_sub(self.start)
    }

    /// Elapsed nanoseconds
    #[inline]
    pub fn elapsed_nanos(&self) -> u64 {
        cycles_to_nanos(self.elapsed_cycles(), &self.calibration)
    }

    /// Elapsed as Duration
    #[inline]
    pub fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.elapsed_nanos())
    }

    /// Reset the timer
    #[inline]
    pub fn reset(&mut self) {
        self.start = rdtsc();
    }

    /// Lap: return elapsed and reset
    #[inline]
    pub fn lap(&mut self) -> Duration {
        let elapsed = self.elapsed();
        self.reset();
        elapsed
    }
}

// =============================================================================
// SCOPED TIMER
// =============================================================================

/// RAII timer that measures scope duration
pub struct ScopedTimer<F: FnOnce(Duration)> {
    start: Instant,
    callback: Option<F>,
}

impl<F: FnOnce(Duration)> ScopedTimer<F> {
    /// Create a scoped timer with a callback
    pub fn new(callback: F) -> Self {
        Self {
            start: Instant::now(),
            callback: Some(callback),
        }
    }
}

impl<F: FnOnce(Duration)> Drop for ScopedTimer<F> {
    fn drop(&mut self) {
        if let Some(cb) = self.callback.take() {
            cb(self.start.elapsed());
        }
    }
}

/// Measure the execution time of a closure
#[inline]
pub fn measure<T, F: FnOnce() -> T>(f: F) -> (T, Duration) {
    let start = Instant::now();
    let result = f();
    (result, start.elapsed())
}

/// Measure with TSC (higher precision)
#[inline]
pub fn measure_tsc<T, F: FnOnce() -> T>(f: F, calibration: &TscCalibration) -> (T, Duration) {
    let start = rdtsc();
    let result = f();
    let cycles = rdtsc().saturating_sub(start);
    (
        result,
        Duration::from_nanos(cycles_to_nanos(cycles, calibration)),
    )
}

// =============================================================================
// DEADLINE CHECKING
// =============================================================================

/// Deadline for time-bounded operations
#[derive(Clone, Copy)]
pub struct Deadline {
    target: Instant,
}

impl Deadline {
    /// Create a deadline from now + duration
    pub fn from_now(duration: Duration) -> Self {
        Self {
            target: Instant::now() + duration,
        }
    }

    /// Check if the deadline has passed
    #[inline]
    pub fn has_passed(&self) -> bool {
        Instant::now() >= self.target
    }

    /// Get remaining time (zero if passed)
    #[inline]
    pub fn remaining(&self) -> Duration {
        self.target.saturating_duration_since(Instant::now())
    }

    /// Get the target instant
    pub fn target(&self) -> Instant {
        self.target
    }
}

// =============================================================================
// STATISTICS
// =============================================================================

/// Running statistics for timing measurements
pub struct TimingStats {
    count: u64,
    sum_ns: u64,
    sum_sq_ns: u128,
    min_ns: u64,
    max_ns: u64,
}

impl TimingStats {
    pub fn new() -> Self {
        Self {
            count: 0,
            sum_ns: 0,
            sum_sq_ns: 0,
            min_ns: u64::MAX,
            max_ns: 0,
        }
    }

    /// Record a measurement
    #[inline]
    pub fn record(&mut self, nanos: u64) {
        self.count += 1;
        self.sum_ns += nanos;
        self.sum_sq_ns += (nanos as u128) * (nanos as u128);
        self.min_ns = self.min_ns.min(nanos);
        self.max_ns = self.max_ns.max(nanos);
    }

    /// Record a Duration
    #[inline]
    pub fn record_duration(&mut self, d: Duration) {
        self.record(d.as_nanos() as u64);
    }

    /// Number of samples
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Mean in nanoseconds
    pub fn mean_ns(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        self.sum_ns as f64 / self.count as f64
    }

    /// Standard deviation in nanoseconds
    pub fn stddev_ns(&self) -> f64 {
        if self.count < 2 {
            return 0.0;
        }
        let mean = self.mean_ns();
        let variance = (self.sum_sq_ns as f64 / self.count as f64) - (mean * mean);
        variance.sqrt()
    }

    /// Minimum in nanoseconds
    pub fn min_ns(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.min_ns
        }
    }

    /// Maximum in nanoseconds
    pub fn max_ns(&self) -> u64 {
        self.max_ns
    }

    /// Reset statistics
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for TimingStats {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// RATE LIMITER
// =============================================================================

/// Token bucket rate limiter
pub struct RateLimiter {
    /// Tokens available
    tokens: f64,
    /// Maximum tokens (burst capacity)
    max_tokens: f64,
    /// Tokens added per nanosecond
    tokens_per_ns: f64,
    /// Last update timestamp
    last_update: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    ///
    /// # Arguments
    /// * `rate_per_sec` - Operations per second allowed
    /// * `burst` - Maximum burst capacity
    pub fn new(rate_per_sec: f64, burst: f64) -> Self {
        Self {
            tokens: burst,
            max_tokens: burst,
            tokens_per_ns: rate_per_sec / 1_000_000_000.0,
            last_update: Instant::now(),
        }
    }

    /// Try to acquire a token
    ///
    /// Returns true if allowed, false if rate-limited.
    #[inline]
    pub fn try_acquire(&mut self) -> bool {
        self.try_acquire_n(1.0)
    }

    /// Try to acquire N tokens
    #[inline]
    pub fn try_acquire_n(&mut self, n: f64) -> bool {
        self.refill();

        if self.tokens >= n {
            self.tokens -= n;
            true
        } else {
            false
        }
    }

    /// Get current token count
    pub fn available(&mut self) -> f64 {
        self.refill();
        self.tokens
    }

    #[inline]
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed_ns = now.duration_since(self.last_update).as_nanos() as f64;
        self.tokens = (self.tokens + elapsed_ns * self.tokens_per_ns).min(self.max_tokens);
        self.last_update = now;
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rdtsc() {
        let t1 = rdtsc();
        let t2 = rdtsc();
        assert!(t2 >= t1);
    }

    #[test]
    fn test_calibration() {
        let cal = calibrate_tsc();
        println!("TSC calibration: {:?}", cal);

        // Sanity check: should be somewhere between 1 and 10 GHz
        assert!(cal.freq_mhz > 500);
        assert!(cal.freq_mhz < 10_000);
    }

    #[test]
    fn test_tsc_timer() {
        let cal = calibrate_tsc();
        let timer = TscTimer::start(cal);

        std::thread::sleep(Duration::from_millis(1));

        let elapsed = timer.elapsed();
        // Should be at least 1ms, but allow some margin
        assert!(elapsed.as_micros() >= 500);
    }

    #[test]
    fn test_deadline() {
        let deadline = Deadline::from_now(Duration::from_millis(10));
        assert!(!deadline.has_passed());

        std::thread::sleep(Duration::from_millis(15));
        assert!(deadline.has_passed());
    }

    #[test]
    fn test_timing_stats() {
        let mut stats = TimingStats::new();

        stats.record(100);
        stats.record(200);
        stats.record(300);

        assert_eq!(stats.count(), 3);
        assert!((stats.mean_ns() - 200.0).abs() < 0.01);
        assert_eq!(stats.min_ns(), 100);
        assert_eq!(stats.max_ns(), 300);
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(1000.0, 10.0);

        // Should allow burst
        for _ in 0..10 {
            assert!(limiter.try_acquire());
        }

        // Should be rate limited
        assert!(!limiter.try_acquire());

        // Wait for refill
        std::thread::sleep(Duration::from_millis(5));
        assert!(limiter.try_acquire());
    }
}
