//! Transport metrics and observability.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

// =============================================================================
// METRIC TYPES
// =============================================================================

/// A counter metric (monotonically increasing)
#[derive(Debug)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    /// Create a new counter
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Increment by 1
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment by amount
    pub fn inc_by(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    /// Get current value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Reset to zero
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

impl Default for Counter {
    fn default() -> Self {
        Self::new()
    }
}

/// A gauge metric (can go up or down)
#[derive(Debug)]
pub struct Gauge {
    value: AtomicU64,
}

impl Gauge {
    /// Create a new gauge
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Set value
    pub fn set(&self, v: u64) {
        self.value.store(v, Ordering::Relaxed);
    }

    /// Increment by 1
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement by 1
    pub fn dec(&self) {
        self.value.fetch_sub(1, Ordering::Relaxed);
    }

    /// Add to value
    pub fn add(&self, n: u64) {
        self.value.fetch_add(n, Ordering::Relaxed);
    }

    /// Subtract from value
    pub fn sub(&self, n: u64) {
        self.value.fetch_sub(n, Ordering::Relaxed);
    }

    /// Get current value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

impl Default for Gauge {
    fn default() -> Self {
        Self::new()
    }
}

/// A histogram for tracking distributions
pub struct Histogram {
    /// Count of observations
    count: AtomicU64,
    /// Sum of all observations
    sum: AtomicU64,
    /// Buckets (upper bound -> count)
    buckets: Vec<(u64, AtomicU64)>,
}

impl Histogram {
    /// Create a new histogram with given bucket boundaries
    pub fn new(boundaries: &[u64]) -> Self {
        let buckets = boundaries.iter().map(|&b| (b, AtomicU64::new(0))).collect();

        Self {
            count: AtomicU64::new(0),
            sum: AtomicU64::new(0),
            buckets,
        }
    }

    /// Create histogram for latency (microseconds)
    pub fn latency() -> Self {
        Self::new(&[
            10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000, 25000, 50000, 100000,
        ])
    }

    /// Create histogram for sizes (bytes)
    pub fn size() -> Self {
        Self::new(&[64, 256, 1024, 4096, 16384, 65536, 262144, 1048576, 4194304])
    }

    /// Observe a value
    pub fn observe(&self, value: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum.fetch_add(value, Ordering::Relaxed);

        for (bound, count) in &self.buckets {
            if value <= *bound {
                count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Get observation count
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Get sum of observations
    pub fn sum(&self) -> u64 {
        self.sum.load(Ordering::Relaxed)
    }

    /// Get average
    pub fn avg(&self) -> f64 {
        let count = self.count();
        if count == 0 {
            return 0.0;
        }
        self.sum() as f64 / count as f64
    }

    /// Get bucket values
    pub fn buckets(&self) -> Vec<(u64, u64)> {
        self.buckets
            .iter()
            .map(|(bound, count)| (*bound, count.load(Ordering::Relaxed)))
            .collect()
    }

    /// Get percentile (approximate)
    pub fn percentile(&self, p: f64) -> u64 {
        let count = self.count();
        if count == 0 {
            return 0;
        }

        let target = (count as f64 * p) as u64;

        for (bound, bucket_count) in &self.buckets {
            if bucket_count.load(Ordering::Relaxed) >= target {
                return *bound;
            }
        }

        self.buckets.last().map(|(b, _)| *b).unwrap_or(0)
    }
}

// =============================================================================
// TRANSPORT METRICS
// =============================================================================

/// Comprehensive transport metrics
pub struct TransportMetrics {
    // Byte counters
    /// Bytes sent
    pub bytes_sent: Counter,
    /// Bytes received
    pub bytes_recv: Counter,
    /// Bytes compressed (pre-compression size)
    pub bytes_compressed: Counter,
    /// Bytes after compression
    pub bytes_compressed_wire: Counter,

    // Frame counters
    /// Frames sent
    pub frames_sent: Counter,
    /// Frames received
    pub frames_recv: Counter,
    /// Frames dropped
    pub frames_dropped: Counter,
    /// Frames retransmitted
    pub frames_retransmit: Counter,

    // Connection metrics
    /// Active connections
    pub connections_active: Gauge,
    /// Total connections opened
    pub connections_opened: Counter,
    /// Total connections closed
    pub connections_closed: Counter,
    /// Connection errors
    pub connection_errors: Counter,

    // Latency histograms
    /// Round-trip time
    pub rtt_us: Histogram,
    /// Send latency
    pub send_latency_us: Histogram,
    /// Receive latency
    pub recv_latency_us: Histogram,

    // Size histograms
    /// Frame sizes
    pub frame_size: Histogram,
    /// Batch sizes
    pub batch_size: Histogram,

    // Congestion metrics
    /// Current congestion window
    pub cwnd: Gauge,
    /// Pacing rate (bytes/sec)
    pub pacing_rate: Gauge,
    /// Estimated bandwidth
    pub bandwidth_bps: Gauge,
    /// Minimum RTT observed
    pub min_rtt_us: Gauge,

    // Pool metrics
    /// Pool size
    pub pool_size: Gauge,
    /// Pool hits
    pub pool_hits: Counter,
    /// Pool misses
    pub pool_misses: Counter,

    // Error counters
    /// Timeout errors
    pub errors_timeout: Counter,
    /// Protocol errors
    pub errors_protocol: Counter,
    /// Compression errors
    pub errors_compression: Counter,
    /// Buffer full errors
    pub errors_buffer_full: Counter,

    // Timestamp
    /// When metrics collection started
    pub started_at: Instant,
}

impl TransportMetrics {
    /// Create new transport metrics
    pub fn new() -> Self {
        Self {
            bytes_sent: Counter::new(),
            bytes_recv: Counter::new(),
            bytes_compressed: Counter::new(),
            bytes_compressed_wire: Counter::new(),

            frames_sent: Counter::new(),
            frames_recv: Counter::new(),
            frames_dropped: Counter::new(),
            frames_retransmit: Counter::new(),

            connections_active: Gauge::new(),
            connections_opened: Counter::new(),
            connections_closed: Counter::new(),
            connection_errors: Counter::new(),

            rtt_us: Histogram::latency(),
            send_latency_us: Histogram::latency(),
            recv_latency_us: Histogram::latency(),

            frame_size: Histogram::size(),
            batch_size: Histogram::size(),

            cwnd: Gauge::new(),
            pacing_rate: Gauge::new(),
            bandwidth_bps: Gauge::new(),
            min_rtt_us: Gauge::new(),

            pool_size: Gauge::new(),
            pool_hits: Counter::new(),
            pool_misses: Counter::new(),

            errors_timeout: Counter::new(),
            errors_protocol: Counter::new(),
            errors_compression: Counter::new(),
            errors_buffer_full: Counter::new(),

            started_at: Instant::now(),
        }
    }

    /// Get uptime
    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Calculate throughput (bytes/sec)
    pub fn throughput_bps(&self) -> (f64, f64) {
        let elapsed = self.uptime().as_secs_f64();
        if elapsed < 0.001 {
            return (0.0, 0.0);
        }

        let sent = self.bytes_sent.get() as f64 / elapsed;
        let recv = self.bytes_recv.get() as f64 / elapsed;

        (sent, recv)
    }

    /// Calculate frame rate
    pub fn frame_rate(&self) -> (f64, f64) {
        let elapsed = self.uptime().as_secs_f64();
        if elapsed < 0.001 {
            return (0.0, 0.0);
        }

        let sent = self.frames_sent.get() as f64 / elapsed;
        let recv = self.frames_recv.get() as f64 / elapsed;

        (sent, recv)
    }

    /// Get compression ratio
    pub fn compression_ratio(&self) -> f64 {
        let original = self.bytes_compressed.get();
        let wire = self.bytes_compressed_wire.get();

        if wire == 0 {
            return 1.0;
        }

        original as f64 / wire as f64
    }

    /// Get error rate
    pub fn error_rate(&self) -> f64 {
        let total = self.frames_sent.get() + self.frames_recv.get();
        if total == 0 {
            return 0.0;
        }

        let errors =
            self.frames_dropped.get() + self.errors_timeout.get() + self.errors_protocol.get();

        errors as f64 / total as f64
    }

    /// Get pool hit rate
    pub fn pool_hit_rate(&self) -> f64 {
        let hits = self.pool_hits.get();
        let total = hits + self.pool_misses.get();

        if total == 0 {
            return 0.0;
        }

        hits as f64 / total as f64
    }

    /// Export as Prometheus format
    pub fn to_prometheus(&self) -> String {
        let mut output = String::new();

        // Bytes
        output.push_str(&format!(
            "spine_transport_bytes_sent_total {}\n",
            self.bytes_sent.get()
        ));
        output.push_str(&format!(
            "spine_transport_bytes_recv_total {}\n",
            self.bytes_recv.get()
        ));

        // Frames
        output.push_str(&format!(
            "spine_transport_frames_sent_total {}\n",
            self.frames_sent.get()
        ));
        output.push_str(&format!(
            "spine_transport_frames_recv_total {}\n",
            self.frames_recv.get()
        ));
        output.push_str(&format!(
            "spine_transport_frames_dropped_total {}\n",
            self.frames_dropped.get()
        ));

        // Connections
        output.push_str(&format!(
            "spine_transport_connections_active {}\n",
            self.connections_active.get()
        ));
        output.push_str(&format!(
            "spine_transport_connections_opened_total {}\n",
            self.connections_opened.get()
        ));

        // Latency
        output.push_str(&format!(
            "spine_transport_rtt_us_avg {:.2}\n",
            self.rtt_us.avg()
        ));
        output.push_str(&format!(
            "spine_transport_rtt_us_p50 {}\n",
            self.rtt_us.percentile(0.5)
        ));
        output.push_str(&format!(
            "spine_transport_rtt_us_p99 {}\n",
            self.rtt_us.percentile(0.99)
        ));

        // Congestion
        output.push_str(&format!("spine_transport_cwnd {}\n", self.cwnd.get()));
        output.push_str(&format!(
            "spine_transport_bandwidth_bps {}\n",
            self.bandwidth_bps.get()
        ));

        // Errors
        output.push_str(&format!(
            "spine_transport_errors_total {}\n",
            self.errors_timeout.get() + self.errors_protocol.get() + self.errors_compression.get()
        ));

        output
    }

    /// Export as JSON
    pub fn to_json(&self) -> String {
        let (send_bps, recv_bps) = self.throughput_bps();
        let (send_fps, recv_fps) = self.frame_rate();

        format!(
            r#"{{
    "bytes": {{
        "sent": {},
        "recv": {},
        "compressed": {},
        "compressed_wire": {}
    }},
    "frames": {{
        "sent": {},
        "recv": {},
        "dropped": {},
        "retransmit": {}
    }},
    "connections": {{
        "active": {},
        "opened": {},
        "closed": {},
        "errors": {}
    }},
    "latency_us": {{
        "rtt_avg": {:.2},
        "rtt_p50": {},
        "rtt_p99": {},
        "send_avg": {:.2},
        "recv_avg": {:.2}
    }},
    "congestion": {{
        "cwnd": {},
        "pacing_rate": {},
        "bandwidth_bps": {},
        "min_rtt_us": {}
    }},
    "pool": {{
        "size": {},
        "hits": {},
        "misses": {},
        "hit_rate": {:.4}
    }},
    "errors": {{
        "timeout": {},
        "protocol": {},
        "compression": {},
        "buffer_full": {}
    }},
    "derived": {{
        "uptime_secs": {:.2},
        "throughput_send_bps": {:.2},
        "throughput_recv_bps": {:.2},
        "frame_rate_send": {:.2},
        "frame_rate_recv": {:.2},
        "compression_ratio": {:.2},
        "error_rate": {:.6}
    }}
}}"#,
            self.bytes_sent.get(),
            self.bytes_recv.get(),
            self.bytes_compressed.get(),
            self.bytes_compressed_wire.get(),
            self.frames_sent.get(),
            self.frames_recv.get(),
            self.frames_dropped.get(),
            self.frames_retransmit.get(),
            self.connections_active.get(),
            self.connections_opened.get(),
            self.connections_closed.get(),
            self.connection_errors.get(),
            self.rtt_us.avg(),
            self.rtt_us.percentile(0.5),
            self.rtt_us.percentile(0.99),
            self.send_latency_us.avg(),
            self.recv_latency_us.avg(),
            self.cwnd.get(),
            self.pacing_rate.get(),
            self.bandwidth_bps.get(),
            self.min_rtt_us.get(),
            self.pool_size.get(),
            self.pool_hits.get(),
            self.pool_misses.get(),
            self.pool_hit_rate(),
            self.errors_timeout.get(),
            self.errors_protocol.get(),
            self.errors_compression.get(),
            self.errors_buffer_full.get(),
            self.uptime().as_secs_f64(),
            send_bps,
            recv_bps,
            send_fps,
            recv_fps,
            self.compression_ratio(),
            self.error_rate(),
        )
    }
}

impl Default for TransportMetrics {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// METRICS REGISTRY
// =============================================================================

/// Global metrics registry
pub struct MetricsRegistry {
    /// Named metrics
    metrics: RwLock<HashMap<String, Arc<TransportMetrics>>>,
    /// Global aggregated metrics
    global: TransportMetrics,
}

impl MetricsRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            metrics: RwLock::new(HashMap::new()),
            global: TransportMetrics::new(),
        }
    }

    /// Get or create metrics for a name
    pub async fn get_or_create(&self, name: &str) -> Arc<TransportMetrics> {
        // Check existing
        {
            let metrics = self.metrics.read().await;
            if let Some(m) = metrics.get(name) {
                return Arc::clone(m);
            }
        }

        // Create new
        let mut metrics = self.metrics.write().await;

        // Double-check
        if let Some(m) = metrics.get(name) {
            return Arc::clone(m);
        }

        let m = Arc::new(TransportMetrics::new());
        metrics.insert(name.to_string(), Arc::clone(&m));
        m
    }

    /// Get global metrics
    pub fn global(&self) -> &TransportMetrics {
        &self.global
    }

    /// List all metric names
    pub async fn names(&self) -> Vec<String> {
        let metrics = self.metrics.read().await;
        metrics.keys().cloned().collect()
    }

    /// Export all metrics as Prometheus format
    pub async fn to_prometheus(&self) -> String {
        let mut output = String::new();

        output.push_str("# Global metrics\n");
        output.push_str(&self.global.to_prometheus());
        output.push('\n');

        let metrics = self.metrics.read().await;
        for (name, m) in metrics.iter() {
            output.push_str(&format!("# Metrics for {}\n", name));
            // Would need to prefix each line with the name
            output.push_str(&m.to_prometheus());
            output.push('\n');
        }

        output
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// LATENCY TRACKER
// =============================================================================

/// Tracks operation latencies with automatic recording
pub struct LatencyTracker<'a> {
    histogram: &'a Histogram,
    start: Instant,
}

impl<'a> LatencyTracker<'a> {
    /// Start tracking
    pub fn start(histogram: &'a Histogram) -> Self {
        Self {
            histogram,
            start: Instant::now(),
        }
    }

    /// Stop tracking and record (called automatically on drop)
    pub fn stop(self) -> u64 {
        let elapsed = self.start.elapsed().as_micros() as u64;
        self.histogram.observe(elapsed);
        elapsed
    }
}

impl<'a> Drop for LatencyTracker<'a> {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_micros() as u64;
        self.histogram.observe(elapsed);
    }
}

// =============================================================================
// RATE TRACKER
// =============================================================================

/// Tracks rates over sliding windows
pub struct RateTracker {
    /// Window duration
    window: Duration,
    /// Samples within window
    samples: RwLock<Vec<(Instant, u64)>>,
}

impl RateTracker {
    /// Create a new rate tracker
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            samples: RwLock::new(Vec::new()),
        }
    }

    /// Record a sample
    pub async fn record(&self, value: u64) {
        let mut samples = self.samples.write().await;
        let now = Instant::now();

        // Remove old samples
        let cutoff = now - self.window;
        samples.retain(|(t, _)| *t >= cutoff);

        samples.push((now, value));
    }

    /// Get rate per second
    pub async fn rate(&self) -> f64 {
        let samples = self.samples.read().await;

        if samples.is_empty() {
            return 0.0;
        }

        let now = Instant::now();
        let cutoff = now - self.window;

        let sum: u64 = samples
            .iter()
            .filter(|(t, _)| *t >= cutoff)
            .map(|(_, v)| *v)
            .sum();

        sum as f64 / self.window.as_secs_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let counter = Counter::new();

        counter.inc();
        counter.inc_by(5);

        assert_eq!(counter.get(), 6);

        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn test_gauge() {
        let gauge = Gauge::new();

        gauge.set(10);
        assert_eq!(gauge.get(), 10);

        gauge.inc();
        assert_eq!(gauge.get(), 11);

        gauge.dec();
        assert_eq!(gauge.get(), 10);

        gauge.add(5);
        assert_eq!(gauge.get(), 15);
    }

    #[test]
    fn test_histogram() {
        let hist = Histogram::new(&[10, 50, 100, 500]);

        for i in 0..100 {
            hist.observe(i);
        }

        assert_eq!(hist.count(), 100);
        assert_eq!(hist.sum(), (0..100).sum::<u64>());

        // Approximate percentiles
        assert!(hist.percentile(0.1) <= 10);
        assert!(hist.percentile(0.5) <= 50);
    }

    #[test]
    fn test_transport_metrics() {
        let metrics = TransportMetrics::new();

        metrics.bytes_sent.inc_by(1000);
        metrics.bytes_recv.inc_by(500);
        metrics.frames_sent.inc_by(10);
        metrics.frames_recv.inc_by(5);

        metrics.rtt_us.observe(100);
        metrics.rtt_us.observe(200);
        metrics.rtt_us.observe(150);

        assert_eq!(metrics.bytes_sent.get(), 1000);
        assert!((metrics.rtt_us.avg() - 150.0).abs() < 0.1);
    }

    #[test]
    fn test_metrics_json() {
        let metrics = TransportMetrics::new();

        metrics.bytes_sent.inc_by(1000);
        metrics.connections_active.set(5);

        let json = metrics.to_json();
        assert!(json.contains("\"sent\": 1000"));
        assert!(json.contains("\"active\": 5"));
    }

    #[tokio::test]
    async fn test_metrics_registry() {
        let registry = MetricsRegistry::new();

        let m1 = registry.get_or_create("conn1").await;
        let m2 = registry.get_or_create("conn2").await;
        let m1_again = registry.get_or_create("conn1").await;

        // Same metrics should be returned
        assert!(Arc::ptr_eq(&m1, &m1_again));
        assert!(!Arc::ptr_eq(&m1, &m2));

        let names = registry.names().await;
        assert_eq!(names.len(), 2);
    }

    #[tokio::test]
    async fn test_rate_tracker() {
        let tracker = RateTracker::new(Duration::from_secs(1));

        for _ in 0..10 {
            tracker.record(100).await;
        }

        let rate = tracker.rate().await;
        assert!(rate > 0.0);
    }
}
