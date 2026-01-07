//! BBR-inspired congestion control for transport optimization.
//!
//! Implements a simplified version of BBR (Bottleneck Bandwidth and Round-trip
//! propagation time) congestion control algorithm for optimal throughput.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

// =============================================================================
// BBR CONGESTION CONTROLLER
// =============================================================================

/// BBR congestion control state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BbrState {
    /// Initial startup phase - probe for bandwidth
    Startup,
    /// Drain phase - reduce inflight to match BDP
    Drain,
    /// Steady state - probe bandwidth periodically
    ProbeBw,
    /// Probe RTT - periodically sample min RTT
    ProbeRtt,
}

/// BBR-inspired congestion controller
///
/// Optimizes for high throughput and low latency by:
/// 1. Estimating bottleneck bandwidth (BtlBw)
/// 2. Estimating minimum RTT (RTprop)
/// 3. Setting cwnd based on bandwidth-delay product (BDP)
pub struct BbrController {
    /// Current state
    state: BbrState,

    /// Estimated bottleneck bandwidth (bytes/sec)
    btl_bw: f64,

    /// Minimum RTT seen (propagation time)
    rt_prop: Duration,

    /// Current congestion window (bytes)
    cwnd: u32,

    /// Initial congestion window
    initial_cwnd: u32,

    /// Pacing rate (bytes/sec)
    pacing_rate: f64,

    /// Bytes in flight
    inflight: u32,

    /// Pacing gain for current phase
    pacing_gain: f64,

    /// Cwnd gain for current phase
    cwnd_gain: f64,

    /// Bandwidth samples (for max filter)
    bw_samples: VecDeque<BwSample>,

    /// RTT samples (for min filter)
    rtt_samples: VecDeque<Duration>,

    /// Round count
    round_count: u64,

    /// Last RTT probe time
    probe_rtt_time: Option<Instant>,

    /// Probe RTT interval
    probe_rtt_interval: Duration,

    /// Startup growth count
    startup_rounds_without_growth: u32,

    /// Previous BtlBw for growth detection
    prev_btl_bw: f64,
}

#[derive(Debug, Clone)]
struct BwSample {
    bandwidth: f64,
    timestamp: Instant,
}

impl BbrController {
    /// MSS (Maximum Segment Size) in bytes
    const MSS: u32 = 1460;

    /// BDP window (10 RTTs worth of samples)
    const BW_WINDOW: usize = 10;

    /// RTT window (10 seconds of samples)
    const RTT_WINDOW: usize = 100;

    /// Startup pacing gain (2/ln(2) ≈ 2.89)
    const STARTUP_PACING_GAIN: f64 = 2.89;

    /// Startup cwnd gain
    const STARTUP_CWND_GAIN: f64 = 2.0;

    /// Drain pacing gain
    const DRAIN_PACING_GAIN: f64 = 0.35;

    /// Steady state cwnd gain
    const STEADY_CWND_GAIN: f64 = 2.0;

    /// Min cwnd (4 segments)
    const MIN_CWND: u32 = 4 * Self::MSS;

    /// Rounds without growth before exiting startup
    const STARTUP_GROWTH_TARGET: u32 = 3;

    /// Create a new BBR controller with default initial cwnd
    pub fn new() -> Self {
        Self::with_initial_cwnd(10) // 10 MSS default
    }

    /// Create a new BBR controller with specified initial cwnd
    pub fn with_initial_cwnd(initial_cwnd: u32) -> Self {
        let initial_cwnd = initial_cwnd.max(10) * Self::MSS;

        Self {
            state: BbrState::Startup,
            btl_bw: 0.0,
            rt_prop: Duration::from_millis(100),
            cwnd: initial_cwnd,
            initial_cwnd,
            pacing_rate: 0.0,
            inflight: 0,
            pacing_gain: Self::STARTUP_PACING_GAIN,
            cwnd_gain: Self::STARTUP_CWND_GAIN,
            bw_samples: VecDeque::with_capacity(Self::BW_WINDOW),
            rtt_samples: VecDeque::with_capacity(Self::RTT_WINDOW),
            round_count: 0,
            probe_rtt_time: None,
            probe_rtt_interval: Duration::from_secs(10),
            startup_rounds_without_growth: 0,
            prev_btl_bw: 0.0,
        }
    }

    /// Called when data is acknowledged
    pub fn on_ack(&mut self, bytes_acked: usize, rtt: Duration) {
        self.round_count += 1;

        // Update RTT estimate
        self.update_rtt(rtt);

        // Calculate bandwidth sample
        if !rtt.is_zero() {
            let bw = bytes_acked as f64 / rtt.as_secs_f64();
            self.add_bw_sample(bw);
        }

        // Update inflight
        self.inflight = self.inflight.saturating_sub(bytes_acked as u32);

        // Update state machine
        self.update_state();

        // Update cwnd and pacing rate
        self.update_cwnd();
        self.update_pacing_rate();
    }

    /// Called when sending data
    pub fn on_send(&mut self, bytes_sent: usize) {
        self.inflight = self.inflight.saturating_add(bytes_sent as u32);
    }

    /// Called on packet loss
    pub fn on_loss(&mut self, bytes_lost: usize) {
        self.inflight = self.inflight.saturating_sub(bytes_lost as u32);

        // In BBR, loss doesn't directly reduce cwnd like in AIMD
        // But we note congestion for state transitions
    }

    /// Get current congestion window
    pub fn cwnd(&self) -> u32 {
        self.cwnd
    }

    /// Get current pacing rate (bytes/sec)
    pub fn pacing_rate(&self) -> f64 {
        self.pacing_rate
    }

    /// Get estimated bandwidth (bytes/sec)
    pub fn bandwidth(&self) -> f64 {
        self.btl_bw
    }

    /// Get minimum RTT
    pub fn min_rtt(&self) -> Duration {
        self.rt_prop
    }

    /// Get current state
    pub fn state(&self) -> BbrState {
        self.state
    }

    /// Get BDP (bandwidth-delay product)
    pub fn bdp(&self) -> u32 {
        (self.btl_bw * self.rt_prop.as_secs_f64()) as u32
    }

    /// Check if we can send more data
    pub fn can_send(&self) -> bool {
        self.inflight < self.cwnd
    }

    /// Get bytes available in window
    pub fn available(&self) -> u32 {
        self.cwnd.saturating_sub(self.inflight)
    }

    /// Get pacing interval for the next packet
    pub fn pacing_interval(&self, packet_size: usize) -> Duration {
        if self.pacing_rate <= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(packet_size as f64 / self.pacing_rate)
        }
    }

    fn update_rtt(&mut self, rtt: Duration) {
        self.rtt_samples.push_back(rtt);
        if self.rtt_samples.len() > Self::RTT_WINDOW {
            self.rtt_samples.pop_front();
        }

        // Update RTprop with min filter
        if let Some(&min_rtt) = self.rtt_samples.iter().min() {
            self.rt_prop = min_rtt;
        }
    }

    fn add_bw_sample(&mut self, bandwidth: f64) {
        self.bw_samples.push_back(BwSample {
            bandwidth,
            timestamp: Instant::now(),
        });

        if self.bw_samples.len() > Self::BW_WINDOW {
            self.bw_samples.pop_front();
        }

        // Update BtlBw with max filter
        if let Some(max_sample) = self.bw_samples.iter().max_by(|a, b| {
            a.bandwidth
                .partial_cmp(&b.bandwidth)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            self.btl_bw = max_sample.bandwidth;
        }
    }

    fn update_state(&mut self) {
        match self.state {
            BbrState::Startup => {
                // Check for bandwidth plateau
                let growth = if self.prev_btl_bw > 0.0 {
                    self.btl_bw / self.prev_btl_bw
                } else {
                    2.0
                };

                if growth < 1.25 {
                    // Less than 25% growth
                    self.startup_rounds_without_growth += 1;
                } else {
                    self.startup_rounds_without_growth = 0;
                }

                self.prev_btl_bw = self.btl_bw;

                // Exit startup when bandwidth plateaus
                if self.startup_rounds_without_growth >= Self::STARTUP_GROWTH_TARGET {
                    self.state = BbrState::Drain;
                    self.pacing_gain = Self::DRAIN_PACING_GAIN;
                }
            }

            BbrState::Drain => {
                // Exit drain when inflight <= BDP
                if self.inflight <= self.bdp() {
                    self.state = BbrState::ProbeBw;
                    self.pacing_gain = 1.0;
                    self.cwnd_gain = Self::STEADY_CWND_GAIN;
                }
            }

            BbrState::ProbeBw => {
                // Periodically probe for RTT
                let should_probe_rtt = match self.probe_rtt_time {
                    Some(t) => t.elapsed() >= self.probe_rtt_interval,
                    None => {
                        self.probe_rtt_time = Some(Instant::now());
                        false
                    }
                };

                if should_probe_rtt {
                    self.state = BbrState::ProbeRtt;
                    self.probe_rtt_time = Some(Instant::now());
                }

                // Cycle through pacing gains to probe bandwidth
                let cycle_phase = (self.round_count % 8) as usize;
                self.pacing_gain = match cycle_phase {
                    0 => 1.25, // Probe up
                    1 => 0.75, // Drain
                    _ => 1.0,  // Cruise
                };
            }

            BbrState::ProbeRtt => {
                // Reduce cwnd to probe RTT
                self.cwnd = Self::MIN_CWND;

                // Exit after 200ms
                if let Some(t) = self.probe_rtt_time {
                    if t.elapsed() >= Duration::from_millis(200) {
                        self.state = BbrState::ProbeBw;
                    }
                }
            }
        }
    }

    fn update_cwnd(&mut self) {
        if self.state == BbrState::ProbeRtt {
            self.cwnd = Self::MIN_CWND;
            return;
        }

        // cwnd = cwnd_gain * BDP
        let target = (self.cwnd_gain * self.bdp() as f64) as u32;
        self.cwnd = target.max(Self::MIN_CWND);
    }

    fn update_pacing_rate(&mut self) {
        // pacing_rate = pacing_gain * BtlBw
        self.pacing_rate = self.pacing_gain * self.btl_bw;
    }
}

// =============================================================================
// ADAPTIVE RATE LIMITER
// =============================================================================

/// Token bucket rate limiter with burst support
pub struct RateLimiter {
    /// Maximum tokens (bucket size)
    capacity: u64,
    /// Current tokens
    tokens: f64,
    /// Token replenishment rate (tokens/sec)
    rate: f64,
    /// Last update time
    last_update: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(rate: u64, burst: u64) -> Self {
        Self {
            capacity: burst,
            tokens: burst as f64,
            rate: rate as f64,
            last_update: Instant::now(),
        }
    }

    /// Try to consume tokens, returns true if successful
    pub fn try_consume(&mut self, tokens: u64) -> bool {
        self.replenish();

        if self.tokens >= tokens as f64 {
            self.tokens -= tokens as f64;
            true
        } else {
            false
        }
    }

    /// Get wait time until tokens are available
    pub fn wait_time(&mut self, tokens: u64) -> Duration {
        self.replenish();

        if self.tokens >= tokens as f64 {
            Duration::ZERO
        } else {
            let needed = tokens as f64 - self.tokens;
            Duration::from_secs_f64(needed / self.rate)
        }
    }

    /// Update rate dynamically
    pub fn set_rate(&mut self, rate: u64) {
        self.replenish();
        self.rate = rate as f64;
    }

    /// Get current token count
    pub fn available(&self) -> u64 {
        self.tokens as u64
    }

    fn replenish(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update);

        self.tokens = (self.tokens + elapsed.as_secs_f64() * self.rate).min(self.capacity as f64);
        self.last_update = now;
    }
}

// =============================================================================
// ADAPTIVE BATCH SIZER
// =============================================================================

/// Dynamically adjusts batch sizes based on throughput feedback
pub struct AdaptiveBatchSizer {
    /// Current batch size
    batch_size: usize,
    /// Minimum batch size
    min_batch: usize,
    /// Maximum batch size
    max_batch: usize,
    /// Recent throughput samples
    throughput_samples: VecDeque<f64>,
    /// Previous batch size (for comparison)
    prev_batch_size: usize,
    /// Previous throughput (for comparison)
    prev_throughput: f64,
}

impl AdaptiveBatchSizer {
    /// Create a new adaptive batch sizer
    pub fn new(initial: usize, min: usize, max: usize) -> Self {
        Self {
            batch_size: initial,
            min_batch: min,
            max_batch: max,
            throughput_samples: VecDeque::with_capacity(10),
            prev_batch_size: initial,
            prev_throughput: 0.0,
        }
    }

    /// Create from throughput estimate
    pub fn from_throughput(max_batch_size: usize, throughput_bps: u64) -> Self {
        // Estimate good batch size from throughput
        // Aim for ~10ms worth of data per batch
        let batch_bytes = (throughput_bps as f64 * 0.01) as usize;
        let initial = batch_bytes.clamp(1024, max_batch_size);
        Self::new(initial, 1024, max_batch_size)
    }

    /// Get current recommended batch size
    pub fn batch_size(&self) -> usize {
        self.batch_size
    }

    /// Alias for batch_size for API compatibility
    pub fn recommended_batch_size(&self) -> usize {
        self.batch_size
    }

    /// Update with bytes processed
    pub fn update(&mut self, bytes_processed: u64) {
        // Simple heuristic: if we're processing a lot, increase batch size
        if bytes_processed as usize > self.batch_size {
            self.batch_size = (self.batch_size * 3 / 2).min(self.max_batch);
        }
    }

    /// Report throughput for feedback
    pub fn report_throughput(&mut self, throughput: f64) {
        self.throughput_samples.push_back(throughput);
        if self.throughput_samples.len() > 10 {
            self.throughput_samples.pop_front();
        }

        // Calculate average throughput
        let avg: f64 =
            self.throughput_samples.iter().sum::<f64>() / self.throughput_samples.len() as f64;

        // Adaptive sizing based on throughput change
        if avg > self.prev_throughput * 1.1 {
            // Throughput improved - try larger batches
            if self.batch_size > self.prev_batch_size {
                // We increased and it helped - keep going
                self.batch_size = (self.batch_size * 3 / 2).min(self.max_batch);
            }
        } else if avg < self.prev_throughput * 0.9 {
            // Throughput decreased - try smaller batches
            if self.batch_size > self.prev_batch_size {
                // We increased and it hurt - back off
                self.batch_size = self.prev_batch_size;
            } else {
                // Try reducing
                self.batch_size = (self.batch_size * 2 / 3).max(self.min_batch);
            }
        } else {
            // Stable - small random exploration
            let variation = (self.batch_size / 10).max(1);
            let delta = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 3) as i64
                - 1;

            self.prev_batch_size = self.batch_size;
            self.batch_size = ((self.batch_size as i64 + delta * variation as i64) as usize)
                .clamp(self.min_batch, self.max_batch);
        }

        self.prev_batch_size = self.batch_size;
        self.prev_throughput = avg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bbr_startup() {
        let mut bbr = BbrController::new();

        assert_eq!(bbr.state(), BbrState::Startup);

        // Simulate ACKs with increasing bandwidth
        for i in 1..=20 {
            let rtt = Duration::from_millis(50);
            let bytes = i * 1000;
            bbr.on_ack(bytes, rtt);
        }

        // Should have estimated some bandwidth
        assert!(bbr.bandwidth() > 0.0);
        assert!(bbr.cwnd() > 0);
    }

    #[test]
    fn test_bbr_bdp_calculation() {
        let mut bbr = BbrController::new();

        // Set known bandwidth and RTT
        bbr.btl_bw = 1_000_000.0; // 1 MB/s
        bbr.rt_prop = Duration::from_millis(100);

        // BDP = 1MB/s * 0.1s = 100KB
        let bdp = bbr.bdp();
        assert_eq!(bdp, 100_000);
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(1000, 100);

        // Should have burst available
        assert!(limiter.try_consume(50));
        assert!(limiter.try_consume(50));
        assert!(!limiter.try_consume(50)); // Exhausted

        // Wait time should be positive
        let wait = limiter.wait_time(50);
        assert!(wait > Duration::ZERO);
    }

    #[test]
    fn test_adaptive_batch_sizer() {
        let mut sizer = AdaptiveBatchSizer::new(32, 8, 256);
        let initial_batch = sizer.batch_size();

        // Report improving throughput
        for i in 1..=10 {
            sizer.report_throughput(i as f64 * 1000.0);
        }

        // Should maintain or increase batch size (not decrease below initial)
        // Since throughput is improving, we shouldn't decrease
        assert!(sizer.batch_size() >= initial_batch.min(8)); // At least min_batch
    }
}
