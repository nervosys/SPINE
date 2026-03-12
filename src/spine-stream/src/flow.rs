//! Flow control for stream management.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::{StreamError, StreamResult};

// =============================================================================
// FLOW CONTROLLER
// =============================================================================

/// Flow controller implementing sliding window with AIMD congestion control.
///
/// Features:
/// - Sliding window flow control
/// - AIMD (Additive Increase Multiplicative Decrease) congestion control
/// - Per-stream and global rate limiting
/// - Automatic window adjustment based on RTT
#[derive(Debug)]
pub struct FlowController {
    /// Configuration
    config: FlowConfig,
    /// Current send window size
    send_window: AtomicU32,
    /// Current receive window size
    recv_window: AtomicU32,
    /// Bytes currently in flight
    in_flight: AtomicU64,
    /// Congestion window (for AIMD)
    cwnd: AtomicU32,
    /// Slow start threshold
    ssthresh: AtomicU32,
    /// RTT estimator
    rtt: Arc<RttEstimator>,
    /// Flow state
    state: RwLock<FlowState>,
    /// Notification for window updates
    window_notify: Arc<Notify>,
    /// Statistics
    stats: Arc<FlowStats>,
}

/// Flow control configuration
#[derive(Clone, Debug)]
pub struct FlowConfig {
    /// Initial window size
    pub initial_window: u32,
    /// Minimum window size
    pub min_window: u32,
    /// Maximum window size
    pub max_window: u32,
    /// Initial slow start threshold
    pub initial_ssthresh: u32,
    /// Window update threshold (send update when this much space available)
    pub update_threshold: f32,
    /// Enable slow start
    pub slow_start_enabled: bool,
    /// AIMD increase factor
    pub aimd_increase: u32,
    /// AIMD decrease factor (multiply by this on congestion)
    pub aimd_decrease: f32,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            initial_window: 256 * 1024,    // 256KB
            min_window: 16 * 1024,         // 16KB
            max_window: 16 * 1024 * 1024,  // 16MB
            initial_ssthresh: 1024 * 1024, // 1MB
            update_threshold: 0.5,         // Update at 50% capacity
            slow_start_enabled: true,
            aimd_increase: 1024, // Increase by 1KB per RTT
            aimd_decrease: 0.5,  // Halve on congestion
        }
    }
}

/// Flow state
#[derive(Clone, Debug, Default)]
pub struct FlowState {
    /// Is flow paused?
    pub paused: bool,
    /// Is in slow start phase?
    pub in_slow_start: bool,
    /// Last congestion event time
    pub last_congestion: Option<Instant>,
    /// Consecutive ACKs without loss
    pub ack_streak: u32,
}

/// Flow statistics
#[derive(Debug, Default)]
pub struct FlowStats {
    pub window_updates_sent: AtomicU64,
    pub window_updates_received: AtomicU64,
    pub congestion_events: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_acked: AtomicU64,
    pub packets_lost: AtomicU64,
}

impl FlowController {
    /// Create a new flow controller
    pub fn new(config: FlowConfig) -> Self {
        let in_slow_start = config.slow_start_enabled;

        Self {
            send_window: AtomicU32::new(config.initial_window),
            recv_window: AtomicU32::new(config.initial_window),
            in_flight: AtomicU64::new(0),
            cwnd: AtomicU32::new(config.initial_window),
            ssthresh: AtomicU32::new(config.initial_ssthresh),
            rtt: Arc::new(RttEstimator::new()),
            state: RwLock::new(FlowState {
                in_slow_start,
                ..Default::default()
            }),
            window_notify: Arc::new(Notify::new()),
            stats: Arc::new(FlowStats::default()),
            config,
        }
    }

    /// Try to acquire send capacity
    pub fn try_acquire(&self, bytes: u64) -> bool {
        if self.state.read().paused {
            return false;
        }

        let in_flight = self.in_flight.load(Ordering::Relaxed);
        let send_window = self.send_window.load(Ordering::Relaxed) as u64;
        let cwnd = self.cwnd.load(Ordering::Relaxed) as u64;

        // Use minimum of send window and congestion window
        let effective_window = send_window.min(cwnd);

        if in_flight + bytes <= effective_window {
            self.in_flight.fetch_add(bytes, Ordering::Relaxed);
            self.stats.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Wait for send capacity
    pub async fn acquire(&self, bytes: u64) -> StreamResult<()> {
        loop {
            if self.try_acquire(bytes) {
                return Ok(());
            }

            // Wait for window update
            tokio::select! {
                _ = self.window_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }

            if self.state.read().paused {
                return Err(StreamError::FlowPaused);
            }
        }
    }

    /// Release send capacity (on ACK)
    pub fn release(&self, bytes: u64) {
        let prev = self.in_flight.fetch_sub(bytes, Ordering::Relaxed);
        self.stats.bytes_acked.fetch_add(bytes, Ordering::Relaxed);

        // Update congestion control
        self.on_ack(bytes);

        // Notify waiters
        if prev >= bytes {
            self.window_notify.notify_waiters();
        }
    }

    /// Handle ACK for congestion control
    fn on_ack(&self, bytes: u64) {
        let mut state = self.state.write();
        state.ack_streak += 1;

        let cwnd = self.cwnd.load(Ordering::Relaxed);
        let ssthresh = self.ssthresh.load(Ordering::Relaxed);

        if state.in_slow_start && cwnd < ssthresh {
            // Slow start: exponential increase
            let new_cwnd = cwnd.saturating_add(bytes as u32);
            self.cwnd
                .store(new_cwnd.min(self.config.max_window), Ordering::Relaxed);

            if new_cwnd >= ssthresh {
                state.in_slow_start = false;
            }
        } else {
            // Congestion avoidance: linear increase
            // Increase by aimd_increase per RTT worth of acks
            let rtt_bytes = cwnd; // Approximate RTT worth of bytes
            if state.ack_streak * bytes as u32 >= rtt_bytes {
                let new_cwnd = cwnd.saturating_add(self.config.aimd_increase);
                self.cwnd
                    .store(new_cwnd.min(self.config.max_window), Ordering::Relaxed);
                state.ack_streak = 0;
            }
        }
    }

    /// Handle packet loss (congestion signal)
    pub fn on_loss(&self) {
        self.stats.congestion_events.fetch_add(1, Ordering::Relaxed);
        self.stats.packets_lost.fetch_add(1, Ordering::Relaxed);

        let mut state = self.state.write();

        // Avoid reacting to multiple losses in quick succession
        if let Some(last) = state.last_congestion {
            if last.elapsed() < Duration::from_millis(100) {
                return;
            }
        }

        state.last_congestion = Some(Instant::now());
        state.in_slow_start = false;
        state.ack_streak = 0;

        // AIMD decrease
        let cwnd = self.cwnd.load(Ordering::Relaxed);
        let new_cwnd = (cwnd as f32 * self.config.aimd_decrease) as u32;
        self.cwnd
            .store(new_cwnd.max(self.config.min_window), Ordering::Relaxed);

        // Update slow start threshold
        self.ssthresh.store(new_cwnd, Ordering::Relaxed);
    }

    /// Update receive window (from peer)
    pub fn update_send_window(&self, new_window: u32) {
        self.send_window.store(new_window, Ordering::Relaxed);
        self.stats
            .window_updates_received
            .fetch_add(1, Ordering::Relaxed);
        self.window_notify.notify_waiters();
    }

    /// Consume receive buffer space
    pub fn consume_recv(&self, bytes: u32) {
        let _ = self
            .recv_window
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |w| {
                Some(w.saturating_sub(bytes))
            });
    }

    /// Free receive buffer space and optionally generate update
    pub fn free_recv(&self, bytes: u32) -> Option<u32> {
        let prev_window = self.recv_window.fetch_add(bytes, Ordering::Relaxed);
        let new_window = prev_window + bytes;

        // Send update if significant space available
        let threshold = (self.config.initial_window as f32 * self.config.update_threshold) as u32;
        if prev_window < threshold && new_window >= threshold {
            self.stats
                .window_updates_sent
                .fetch_add(1, Ordering::Relaxed);
            Some(new_window)
        } else {
            None
        }
    }

    /// Pause flow
    pub fn pause(&self) {
        self.state.write().paused = true;
    }

    /// Resume flow
    pub fn resume(&self) {
        self.state.write().paused = false;
        self.window_notify.notify_waiters();
    }

    /// Check if paused
    pub fn is_paused(&self) -> bool {
        self.state.read().paused
    }

    /// Get current send window
    pub fn send_window(&self) -> u32 {
        self.send_window.load(Ordering::Relaxed)
    }

    /// Get current receive window
    pub fn recv_window(&self) -> u32 {
        self.recv_window.load(Ordering::Relaxed)
    }

    /// Get congestion window
    pub fn cwnd(&self) -> u32 {
        self.cwnd.load(Ordering::Relaxed)
    }

    /// Get bytes in flight
    pub fn in_flight(&self) -> u64 {
        self.in_flight.load(Ordering::Relaxed)
    }

    /// Get available send capacity
    pub fn available(&self) -> u64 {
        let in_flight = self.in_flight.load(Ordering::Relaxed);
        let send_window = self.send_window.load(Ordering::Relaxed) as u64;
        let cwnd = self.cwnd.load(Ordering::Relaxed) as u64;

        let effective = send_window.min(cwnd);
        effective.saturating_sub(in_flight)
    }

    /// Get RTT estimator
    pub fn rtt(&self) -> &RttEstimator {
        &self.rtt
    }

    /// Get statistics
    pub fn stats(&self) -> &FlowStats {
        &self.stats
    }
}

// =============================================================================
// RTT ESTIMATOR
// =============================================================================

/// Estimates round-trip time using exponential moving average.
#[derive(Debug)]
pub struct RttEstimator {
    /// Smoothed RTT
    srtt: AtomicU64,
    /// RTT variance
    rttvar: AtomicU64,
    /// Minimum RTT observed
    min_rtt: AtomicU64,
    /// Maximum RTT observed
    max_rtt: AtomicU64,
    /// Sample count
    samples: AtomicU64,
}

impl RttEstimator {
    /// Create a new RTT estimator
    pub fn new() -> Self {
        Self {
            srtt: AtomicU64::new(100_000),  // 100ms initial guess
            rttvar: AtomicU64::new(50_000), // 50ms variance
            min_rtt: AtomicU64::new(u64::MAX),
            max_rtt: AtomicU64::new(0),
            samples: AtomicU64::new(0),
        }
    }

    /// Add an RTT sample (in microseconds)
    pub fn update(&self, rtt_us: u64) {
        self.samples.fetch_add(1, Ordering::Relaxed);

        // Update min/max
        self.min_rtt.fetch_min(rtt_us, Ordering::Relaxed);
        self.max_rtt.fetch_max(rtt_us, Ordering::Relaxed);

        // RFC 6298 algorithm
        let srtt = self.srtt.load(Ordering::Relaxed);
        let rttvar = self.rttvar.load(Ordering::Relaxed);

        if self.samples.load(Ordering::Relaxed) == 1 {
            // First sample
            self.srtt.store(rtt_us, Ordering::Relaxed);
            self.rttvar.store(rtt_us / 2, Ordering::Relaxed);
        } else {
            // Update variance: RTTVAR = (1 - beta) * RTTVAR + beta * |SRTT - R|
            // beta = 1/4
            let diff = rtt_us.abs_diff(srtt);
            let new_rttvar = (3 * rttvar + diff) / 4;
            self.rttvar.store(new_rttvar, Ordering::Relaxed);

            // Update SRTT: SRTT = (1 - alpha) * SRTT + alpha * R
            // alpha = 1/8
            let new_srtt = (7 * srtt + rtt_us) / 8;
            self.srtt.store(new_srtt, Ordering::Relaxed);
        }
    }

    /// Get smoothed RTT in microseconds
    pub fn srtt_us(&self) -> u64 {
        self.srtt.load(Ordering::Relaxed)
    }

    /// Get smoothed RTT as Duration
    pub fn srtt(&self) -> Duration {
        Duration::from_micros(self.srtt.load(Ordering::Relaxed))
    }

    /// Get RTT variance in microseconds
    pub fn rttvar_us(&self) -> u64 {
        self.rttvar.load(Ordering::Relaxed)
    }

    /// Get retransmission timeout (RTO) in microseconds
    /// RTO = SRTT + max(G, K*RTTVAR) where K=4
    pub fn rto_us(&self) -> u64 {
        let srtt = self.srtt.load(Ordering::Relaxed);
        let rttvar = self.rttvar.load(Ordering::Relaxed);

        // Minimum granularity of 1ms
        let k_rttvar = 4 * rttvar;
        let g = 1000; // 1ms granularity

        srtt + k_rttvar.max(g)
    }

    /// Get RTO as Duration
    pub fn rto(&self) -> Duration {
        Duration::from_micros(self.rto_us())
    }

    /// Get minimum RTT observed
    pub fn min_rtt(&self) -> Duration {
        let min = self.min_rtt.load(Ordering::Relaxed);
        if min == u64::MAX {
            self.srtt()
        } else {
            Duration::from_micros(min)
        }
    }

    /// Get sample count
    pub fn samples(&self) -> u64 {
        self.samples.load(Ordering::Relaxed)
    }
}

impl Default for RttEstimator {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TOKEN BUCKET RATE LIMITER
// =============================================================================

/// Token bucket rate limiter for bandwidth limiting.
#[derive(Debug)]
pub struct TokenBucket {
    /// Bucket capacity (burst size)
    capacity: u64,
    /// Current tokens
    tokens: AtomicU64,
    /// Tokens per second
    rate: u64,
    /// Last refill time
    last_refill: Mutex<Instant>,
    /// Notification for token availability
    notify: Arc<Notify>,
}

impl TokenBucket {
    /// Create a new token bucket
    pub fn new(rate: u64, burst: u64) -> Self {
        Self {
            capacity: burst,
            tokens: AtomicU64::new(burst),
            rate,
            last_refill: Mutex::new(Instant::now()),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Try to consume tokens
    pub fn try_consume(&self, tokens: u64) -> bool {
        self.refill();

        let current = self.tokens.load(Ordering::Relaxed);
        if current >= tokens {
            self.tokens.fetch_sub(tokens, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Consume tokens, waiting if necessary
    pub async fn consume(&self, tokens: u64) {
        loop {
            if self.try_consume(tokens) {
                return;
            }

            // Calculate wait time
            let current = self.tokens.load(Ordering::Relaxed);
            let needed = tokens.saturating_sub(current);
            let wait_secs = needed as f64 / self.rate as f64;
            let wait = Duration::from_secs_f64(wait_secs);

            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = self.notify.notified() => {}
            }
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&self) {
        let mut last_refill = self.last_refill.lock();
        let now = Instant::now();
        let elapsed = now.duration_since(*last_refill);

        let new_tokens = (elapsed.as_secs_f64() * self.rate as f64) as u64;
        if new_tokens > 0 {
            let current = self.tokens.load(Ordering::Relaxed);
            let new_total = (current + new_tokens).min(self.capacity);
            self.tokens.store(new_total, Ordering::Relaxed);
            *last_refill = now;

            self.notify.notify_waiters();
        }
    }

    /// Get current token count
    pub fn available(&self) -> u64 {
        self.refill();
        self.tokens.load(Ordering::Relaxed)
    }

    /// Get rate (tokens per second)
    pub fn rate(&self) -> u64 {
        self.rate
    }
}

// =============================================================================
// LEAKY BUCKET RATE LIMITER
// =============================================================================

/// Leaky bucket for smooth rate limiting.
#[derive(Debug)]
pub struct LeakyBucket {
    /// Bucket capacity
    capacity: u64,
    /// Current fill level
    fill: AtomicU64,
    /// Leak rate (units per second)
    leak_rate: u64,
    /// Last leak time
    last_leak: Mutex<Instant>,
}

impl LeakyBucket {
    /// Create a new leaky bucket
    pub fn new(capacity: u64, leak_rate: u64) -> Self {
        Self {
            capacity,
            fill: AtomicU64::new(0),
            leak_rate,
            last_leak: Mutex::new(Instant::now()),
        }
    }

    /// Try to add to the bucket (returns false if would overflow)
    pub fn try_add(&self, amount: u64) -> bool {
        self.leak();

        let current = self.fill.load(Ordering::Relaxed);
        if current + amount <= self.capacity {
            self.fill.fetch_add(amount, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Leak water based on elapsed time
    fn leak(&self) {
        let mut last_leak = self.last_leak.lock();
        let now = Instant::now();
        let elapsed = now.duration_since(*last_leak);

        let leaked = (elapsed.as_secs_f64() * self.leak_rate as f64) as u64;
        if leaked > 0 {
            let current = self.fill.load(Ordering::Relaxed);
            let new_fill = current.saturating_sub(leaked);
            self.fill.store(new_fill, Ordering::Relaxed);
            *last_leak = now;
        }
    }

    /// Get current fill level
    pub fn fill_level(&self) -> u64 {
        self.leak();
        self.fill.load(Ordering::Relaxed)
    }

    /// Get available capacity
    pub fn available(&self) -> u64 {
        self.capacity.saturating_sub(self.fill_level())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_controller_acquire_release() {
        let fc = FlowController::new(FlowConfig {
            initial_window: 1000,
            ..Default::default()
        });

        assert!(fc.try_acquire(500));
        assert_eq!(fc.in_flight(), 500);

        assert!(fc.try_acquire(500));
        assert_eq!(fc.in_flight(), 1000);

        // Window full
        assert!(!fc.try_acquire(1));

        // Release and try again
        fc.release(500);
        assert_eq!(fc.in_flight(), 500);
        assert!(fc.try_acquire(500));
    }

    #[test]
    fn test_flow_controller_congestion() {
        let fc = FlowController::new(FlowConfig {
            initial_window: 10000,
            min_window: 1000, // Must be less than initial * aimd_decrease
            aimd_decrease: 0.5,
            ..Default::default()
        });

        let initial_cwnd = fc.cwnd();
        fc.on_loss();

        // CWND should be halved (to 5000)
        assert!(fc.cwnd() < initial_cwnd);
        assert_eq!(fc.cwnd(), 5000);
    }

    #[test]
    fn test_rtt_estimator() {
        let rtt = RttEstimator::new();

        rtt.update(100_000); // 100ms
        rtt.update(120_000); // 120ms
        rtt.update(90_000); // 90ms

        let srtt = rtt.srtt();
        assert!(srtt > Duration::from_millis(80));
        assert!(srtt < Duration::from_millis(150));
    }

    #[test]
    fn test_token_bucket() {
        let bucket = TokenBucket::new(100, 100);

        assert!(bucket.try_consume(50));
        assert!(bucket.try_consume(50));
        assert!(!bucket.try_consume(1)); // Empty
    }

    #[test]
    fn test_leaky_bucket() {
        let bucket = LeakyBucket::new(100, 1000);

        assert!(bucket.try_add(50));
        assert!(bucket.try_add(50));
        assert!(!bucket.try_add(1)); // Full
    }

    #[tokio::test]
    async fn test_flow_pause_resume() {
        let fc = FlowController::new(FlowConfig::default());

        fc.pause();
        assert!(fc.is_paused());
        assert!(!fc.try_acquire(100));

        fc.resume();
        assert!(!fc.is_paused());
        assert!(fc.try_acquire(100));
    }
}
