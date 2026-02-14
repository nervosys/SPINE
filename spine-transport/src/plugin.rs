//! Plugin system for composable protocol layers.
//!
//! Plugins intercept frames at send/receive time, enabling modular
//! composition of compression, encryption, metrics, rate-limiting,
//! and custom protocol extensions without modifying the transport core.
//!
//! # Architecture
//!
//! ```text
//! Application
//!     ↓ send_frame()
//! ┌──────────────────────┐
//! │  Plugin 1 (metrics)  │  on_send → record stats
//! ├──────────────────────┤
//! │  Plugin 2 (compress) │  on_send → compress payload
//! ├──────────────────────┤
//! │  Plugin 3 (encrypt)  │  on_send → encrypt frame
//! └──────────────────────┘
//!     ↓
//! TransportBackend::send_frame()
//! ```
//!
//! On receive, plugins execute in reverse order (encrypt → decompress → metrics).

use crate::{Frame, FrameFlags, TransportResult};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// =============================================================================
// PLUGIN TRAIT
// =============================================================================

/// A composable protocol layer that intercepts frames.
///
/// Plugins are applied in order on send (first plugin sees the frame first)
/// and in reverse order on receive (last plugin applied on send is first to
/// see the frame on receive).
pub trait TransportPlugin: Send + Sync + fmt::Debug {
    /// Plugin name for logging and diagnostics
    fn name(&self) -> &str;

    /// Called before a frame is sent to the transport backend.
    /// Plugins can modify the frame in place (e.g., compress, encrypt, tag).
    fn on_send(&self, frame: &mut Frame) -> TransportResult<()> {
        let _ = frame;
        Ok(())
    }

    /// Called after a frame is received from the transport backend.
    /// Plugins can modify the frame in place (e.g., decrypt, decompress, validate).
    fn on_recv(&self, frame: &mut Frame) -> TransportResult<()> {
        let _ = frame;
        Ok(())
    }

    /// Called when a new connection is established.
    fn on_connect(&self) -> TransportResult<()> {
        Ok(())
    }

    /// Called when a connection is closed.
    fn on_disconnect(&self) -> TransportResult<()> {
        Ok(())
    }

    /// Whether this plugin is currently enabled.
    fn is_enabled(&self) -> bool {
        true
    }
}

// =============================================================================
// PLUGIN PIPELINE
// =============================================================================

/// An ordered pipeline of transport plugins.
///
/// Plugins execute in insertion order on send, and reverse order on receive.
/// This ensures symmetric operations (e.g., compress-then-encrypt on send,
/// decrypt-then-decompress on receive).
#[derive(Debug)]
pub struct PluginPipeline {
    plugins: Vec<Box<dyn TransportPlugin>>,
}

impl PluginPipeline {
    /// Create an empty pipeline.
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Add a plugin to the end of the pipeline.
    pub fn add<P: TransportPlugin + 'static>(&mut self, plugin: P) {
        self.plugins.push(Box::new(plugin));
    }

    /// Insert a plugin at a specific position.
    pub fn insert<P: TransportPlugin + 'static>(&mut self, index: usize, plugin: P) {
        self.plugins.insert(index, Box::new(plugin));
    }

    /// Remove a plugin by name. Returns true if found.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.plugins.len();
        self.plugins.retain(|p| p.name() != name);
        self.plugins.len() < before
    }

    /// Process a frame through the pipeline before sending (forward order).
    pub fn process_send(&self, frame: &mut Frame) -> TransportResult<()> {
        for plugin in &self.plugins {
            if plugin.is_enabled() {
                plugin.on_send(frame)?;
            }
        }
        Ok(())
    }

    /// Process a frame through the pipeline after receiving (reverse order).
    pub fn process_recv(&self, frame: &mut Frame) -> TransportResult<()> {
        for plugin in self.plugins.iter().rev() {
            if plugin.is_enabled() {
                plugin.on_recv(frame)?;
            }
        }
        Ok(())
    }

    /// Notify all plugins of a new connection.
    pub fn notify_connect(&self) -> TransportResult<()> {
        for plugin in &self.plugins {
            plugin.on_connect()?;
        }
        Ok(())
    }

    /// Notify all plugins of a disconnection.
    pub fn notify_disconnect(&self) -> TransportResult<()> {
        for plugin in &self.plugins {
            plugin.on_disconnect()?;
        }
        Ok(())
    }

    /// Get the number of plugins in the pipeline.
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Check if the pipeline is empty.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// List plugin names in pipeline order.
    pub fn names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }
}

impl Default for PluginPipeline {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// BUILT-IN PLUGINS
// =============================================================================

/// Metrics collection plugin — tracks frame counts, bytes, and latency.
#[derive(Debug)]
pub struct MetricsPlugin {
    frames_sent: AtomicU64,
    frames_recv: AtomicU64,
    bytes_sent: AtomicU64,
    bytes_recv: AtomicU64,
    connections: AtomicU64,
    disconnections: AtomicU64,
}

impl MetricsPlugin {
    pub fn new() -> Self {
        Self {
            frames_sent: AtomicU64::new(0),
            frames_recv: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_recv: AtomicU64::new(0),
            connections: AtomicU64::new(0),
            disconnections: AtomicU64::new(0),
        }
    }

    pub fn frames_sent(&self) -> u64 {
        self.frames_sent.load(Ordering::Relaxed)
    }

    pub fn frames_recv(&self) -> u64 {
        self.frames_recv.load(Ordering::Relaxed)
    }

    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent.load(Ordering::Relaxed)
    }

    pub fn bytes_recv(&self) -> u64 {
        self.bytes_recv.load(Ordering::Relaxed)
    }

    pub fn connections(&self) -> u64 {
        self.connections.load(Ordering::Relaxed)
    }

    /// Get a snapshot of all metrics.
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            frames_sent: self.frames_sent(),
            frames_recv: self.frames_recv(),
            bytes_sent: self.bytes_sent(),
            bytes_recv: self.bytes_recv(),
            connections: self.connections(),
            disconnections: self.disconnections.load(Ordering::Relaxed),
        }
    }
}

impl Default for MetricsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of metrics at a point in time.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub frames_sent: u64,
    pub frames_recv: u64,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub connections: u64,
    pub disconnections: u64,
}

impl TransportPlugin for MetricsPlugin {
    fn name(&self) -> &str {
        "metrics"
    }

    fn on_send(&self, frame: &mut Frame) -> TransportResult<()> {
        self.frames_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent
            .fetch_add(frame.payload.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    fn on_recv(&self, frame: &mut Frame) -> TransportResult<()> {
        self.frames_recv.fetch_add(1, Ordering::Relaxed);
        self.bytes_recv
            .fetch_add(frame.payload.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    fn on_connect(&self) -> TransportResult<()> {
        self.connections.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn on_disconnect(&self) -> TransportResult<()> {
        self.disconnections.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

// ---------------------------------------------------------------------------

/// Rate limiter plugin — enforces send rate limits using token bucket.
#[derive(Debug)]
pub struct RateLimiterPlugin {
    /// Tokens per second
    rate: f64,
    /// Maximum burst size
    burst: u64,
    /// Current tokens (scaled by 1000 for precision without floats in atomic)
    tokens_x1000: AtomicU64,
    /// Last refill time (as nanos since an epoch)
    last_refill_nanos: AtomicU64,
    /// Whether the limiter is enabled
    enabled: bool,
}

impl RateLimiterPlugin {
    /// Create a rate limiter with the given rate (frames/sec) and burst capacity.
    pub fn new(rate: f64, burst: u64) -> Self {
        Self {
            rate,
            burst,
            tokens_x1000: AtomicU64::new(burst * 1000),
            last_refill_nanos: AtomicU64::new(0),
            enabled: true,
        }
    }

    fn refill(&self) {
        let now = Instant::now();
        let now_nanos = now.elapsed().as_nanos() as u64; // monotonic offset
        let last = self.last_refill_nanos.swap(now_nanos, Ordering::Relaxed);

        if last == 0 {
            return; // First call, no delta
        }

        let delta_nanos = now_nanos.saturating_sub(last);
        let delta_secs = delta_nanos as f64 / 1_000_000_000.0;
        let new_tokens = (self.rate * delta_secs * 1000.0) as u64;

        if new_tokens > 0 {
            let max = self.burst * 1000;
            let _ =
                self.tokens_x1000
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                        Some(current.saturating_add(new_tokens).min(max))
                    });
        }
    }
}

impl TransportPlugin for RateLimiterPlugin {
    fn name(&self) -> &str {
        "rate_limiter"
    }

    fn on_send(&self, _frame: &mut Frame) -> TransportResult<()> {
        self.refill();

        // Try to consume one token (1000 in scaled units)
        let result =
            self.tokens_x1000
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    if current >= 1000 {
                        Some(current - 1000)
                    } else {
                        None // No tokens available
                    }
                });

        match result {
            Ok(_) => Ok(()),
            Err(_) => Err(crate::TransportError::ResourceExhausted {
                resource: format!("rate limit exceeded ({} frames/sec)", self.rate),
            }),
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }
}

// ---------------------------------------------------------------------------

/// Frame tagging plugin — adds metadata tags to frames via reserved flags.
#[derive(Debug)]
pub struct TaggingPlugin {
    /// Default flags to apply to outgoing frames
    default_flags: FrameFlags,
    /// Stream ID to assign (0 = don't override)
    stream_id: u16,
}

impl TaggingPlugin {
    /// Create a tagging plugin that marks frames as priority.
    pub fn priority() -> Self {
        Self {
            default_flags: FrameFlags::PRIORITY,
            stream_id: 0,
        }
    }

    /// Create a tagging plugin that assigns a specific stream ID.
    pub fn with_stream(stream_id: u16) -> Self {
        Self {
            default_flags: FrameFlags::empty(),
            stream_id,
        }
    }

    /// Create a tagging plugin with custom flags and stream.
    pub fn new(flags: FrameFlags, stream_id: u16) -> Self {
        Self {
            default_flags: flags,
            stream_id,
        }
    }
}

impl TransportPlugin for TaggingPlugin {
    fn name(&self) -> &str {
        "tagging"
    }

    fn on_send(&self, frame: &mut Frame) -> TransportResult<()> {
        frame.header.flags |= self.default_flags;
        if self.stream_id != 0 {
            frame.header.stream_id = self.stream_id;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------

/// Logging plugin — logs frame metadata for debugging.
#[derive(Debug)]
pub struct LoggingPlugin {
    level: LogLevel,
}

#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    /// Log frame summaries (type, size)
    Summary,
    /// Log full frame headers
    Headers,
    /// Log everything including payload sizes
    Verbose,
}

impl LoggingPlugin {
    pub fn new(level: LogLevel) -> Self {
        Self { level }
    }

    pub fn summary() -> Self {
        Self::new(LogLevel::Summary)
    }

    pub fn verbose() -> Self {
        Self::new(LogLevel::Verbose)
    }
}

impl TransportPlugin for LoggingPlugin {
    fn name(&self) -> &str {
        "logging"
    }

    fn on_send(&self, frame: &mut Frame) -> TransportResult<()> {
        match self.level {
            LogLevel::Summary => {
                log::debug!(
                    "[plugin:logging] SEND seq={} len={}",
                    frame.header.sequence,
                    frame.payload.len()
                );
            }
            LogLevel::Headers => {
                log::debug!(
                    "[plugin:logging] SEND seq={} stream={} flags={:?} len={}",
                    frame.header.sequence,
                    frame.header.stream_id,
                    frame.header.flags,
                    frame.payload.len()
                );
            }
            LogLevel::Verbose => {
                log::info!(
                    "[plugin:logging] SEND seq={} stream={} flags={:?} len={} payload_hash={:x}",
                    frame.header.sequence,
                    frame.header.stream_id,
                    frame.header.flags,
                    frame.payload.len(),
                    simple_hash(&frame.payload)
                );
            }
        }
        Ok(())
    }

    fn on_recv(&self, frame: &mut Frame) -> TransportResult<()> {
        match self.level {
            LogLevel::Summary => {
                log::debug!(
                    "[plugin:logging] RECV seq={} len={}",
                    frame.header.sequence,
                    frame.payload.len()
                );
            }
            LogLevel::Headers => {
                log::debug!(
                    "[plugin:logging] RECV seq={} stream={} flags={:?} len={}",
                    frame.header.sequence,
                    frame.header.stream_id,
                    frame.header.flags,
                    frame.payload.len()
                );
            }
            LogLevel::Verbose => {
                log::info!(
                    "[plugin:logging] RECV seq={} stream={} flags={:?} len={} payload_hash={:x}",
                    frame.header.sequence,
                    frame.header.stream_id,
                    frame.header.flags,
                    frame.payload.len(),
                    simple_hash(&frame.payload)
                );
            }
        }
        Ok(())
    }

    fn on_connect(&self) -> TransportResult<()> {
        log::info!("[plugin:logging] CONNECTION ESTABLISHED");
        Ok(())
    }

    fn on_disconnect(&self) -> TransportResult<()> {
        log::info!("[plugin:logging] CONNECTION CLOSED");
        Ok(())
    }
}

/// Simple non-cryptographic hash for logging (FNV-1a variant).
fn simple_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ---------------------------------------------------------------------------

/// Payload size limiter plugin — rejects frames exceeding a size threshold.
#[derive(Debug)]
pub struct SizeLimiterPlugin {
    max_payload_bytes: usize,
}

impl SizeLimiterPlugin {
    pub fn new(max_payload_bytes: usize) -> Self {
        Self { max_payload_bytes }
    }
}

impl TransportPlugin for SizeLimiterPlugin {
    fn name(&self) -> &str {
        "size_limiter"
    }

    fn on_send(&self, frame: &mut Frame) -> TransportResult<()> {
        if frame.payload.len() > self.max_payload_bytes {
            return Err(crate::TransportError::MessageTooLarge {
                size: frame.payload.len(),
                max: self.max_payload_bytes,
            });
        }
        Ok(())
    }

    fn on_recv(&self, frame: &mut Frame) -> TransportResult<()> {
        if frame.payload.len() > self.max_payload_bytes {
            return Err(crate::TransportError::MessageTooLarge {
                size: frame.payload.len(),
                max: self.max_payload_bytes,
            });
        }
        Ok(())
    }
}

// =============================================================================
// BUILDER HELPERS
// =============================================================================

/// Convenience builder for common plugin pipeline configurations.
pub struct PluginPipelineBuilder {
    pipeline: PluginPipeline,
}

impl PluginPipelineBuilder {
    pub fn new() -> Self {
        Self {
            pipeline: PluginPipeline::new(),
        }
    }

    /// Add metrics collection.
    pub fn with_metrics(mut self) -> Self {
        self.pipeline.add(MetricsPlugin::new());
        self
    }

    /// Add rate limiting.
    pub fn with_rate_limit(mut self, rate: f64, burst: u64) -> Self {
        self.pipeline.add(RateLimiterPlugin::new(rate, burst));
        self
    }

    /// Add frame logging.
    pub fn with_logging(mut self, level: LogLevel) -> Self {
        self.pipeline.add(LoggingPlugin::new(level));
        self
    }

    /// Add payload size limits.
    pub fn with_size_limit(mut self, max_bytes: usize) -> Self {
        self.pipeline.add(SizeLimiterPlugin::new(max_bytes));
        self
    }

    /// Add frame tagging.
    pub fn with_tagging(mut self, flags: FrameFlags, stream_id: u16) -> Self {
        self.pipeline.add(TaggingPlugin::new(flags, stream_id));
        self
    }

    /// Build the pipeline.
    pub fn build(self) -> PluginPipeline {
        self.pipeline
    }
}

impl Default for PluginPipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FrameHeader;
    use bytes::Bytes;

    fn test_frame(payload: &[u8]) -> Frame {
        Frame {
            header: FrameHeader {
                length: payload.len() as u32,
                flags: FrameFlags::empty(),
                sequence: 1,
                stream_id: 0,
                _reserved: 0,
            },
            payload: Bytes::copy_from_slice(payload),
        }
    }

    #[test]
    fn test_empty_pipeline() {
        let pipeline = PluginPipeline::new();
        assert!(pipeline.is_empty());
        assert_eq!(pipeline.len(), 0);

        let mut frame = test_frame(b"hello");
        pipeline.process_send(&mut frame).unwrap();
        pipeline.process_recv(&mut frame).unwrap();
        assert_eq!(&frame.payload[..], b"hello");
    }

    #[test]
    fn test_metrics_plugin() {
        let metrics = MetricsPlugin::new();
        let mut frame = test_frame(b"test data");

        metrics.on_send(&mut frame).unwrap();
        metrics.on_send(&mut frame).unwrap();
        metrics.on_recv(&mut frame).unwrap();

        assert_eq!(metrics.frames_sent(), 2);
        assert_eq!(metrics.frames_recv(), 1);
        assert_eq!(metrics.bytes_sent(), 18); // 9 bytes * 2
        assert_eq!(metrics.bytes_recv(), 9);
    }

    #[test]
    fn test_metrics_snapshot() {
        let metrics = MetricsPlugin::new();
        let mut frame = test_frame(b"data");

        metrics.on_send(&mut frame).unwrap();
        metrics.on_connect().unwrap();
        metrics.on_disconnect().unwrap();

        let snap = metrics.snapshot();
        assert_eq!(snap.frames_sent, 1);
        assert_eq!(snap.connections, 1);
        assert_eq!(snap.disconnections, 1);
    }

    #[test]
    fn test_size_limiter() {
        let limiter = SizeLimiterPlugin::new(10);
        let mut small = test_frame(b"ok");
        let mut large = test_frame(b"this is way too large for the limit");

        assert!(limiter.on_send(&mut small).is_ok());
        assert!(limiter.on_send(&mut large).is_err());
        assert!(limiter.on_recv(&mut large).is_err());
    }

    #[test]
    fn test_tagging_plugin() {
        let tagger = TaggingPlugin::new(FrameFlags::PRIORITY | FrameFlags::ENCRYPTED, 42);
        let mut frame = test_frame(b"data");

        tagger.on_send(&mut frame).unwrap();

        assert!(frame.header.flags.contains(FrameFlags::PRIORITY));
        assert!(frame.header.flags.contains(FrameFlags::ENCRYPTED));
        assert_eq!(frame.header.stream_id, 42);
    }

    #[test]
    fn test_pipeline_ordering() {
        // Tagging runs first (adds PRIORITY), then size limiter checks
        let mut pipeline = PluginPipeline::new();
        pipeline.add(TaggingPlugin::priority());
        pipeline.add(SizeLimiterPlugin::new(1024));

        let mut frame = test_frame(b"hello");
        pipeline.process_send(&mut frame).unwrap();

        assert!(frame.header.flags.contains(FrameFlags::PRIORITY));
    }

    #[test]
    fn test_pipeline_names() {
        let pipeline = PluginPipelineBuilder::new()
            .with_metrics()
            .with_logging(LogLevel::Summary)
            .with_size_limit(1024)
            .build();

        let names = pipeline.names();
        assert_eq!(names, vec!["metrics", "logging", "size_limiter"]);
    }

    #[test]
    fn test_pipeline_remove() {
        let mut pipeline = PluginPipeline::new();
        pipeline.add(MetricsPlugin::new());
        pipeline.add(LoggingPlugin::summary());
        assert_eq!(pipeline.len(), 2);

        assert!(pipeline.remove("metrics"));
        assert_eq!(pipeline.len(), 1);
        assert_eq!(pipeline.names(), vec!["logging"]);

        assert!(!pipeline.remove("nonexistent"));
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiterPlugin::new(1000.0, 5);
        let mut frame = test_frame(b"data");

        // Should allow burst of 5
        for _ in 0..5 {
            assert!(limiter.on_send(&mut frame).is_ok());
        }

        // 6th should fail (no refill time has passed)
        assert!(limiter.on_send(&mut frame).is_err());
    }

    #[test]
    fn test_builder_pattern() {
        let pipeline = PluginPipelineBuilder::new()
            .with_metrics()
            .with_rate_limit(100.0, 10)
            .with_size_limit(1024 * 1024)
            .with_logging(LogLevel::Headers)
            .build();

        assert_eq!(pipeline.len(), 4);
        assert_eq!(
            pipeline.names(),
            vec!["metrics", "rate_limiter", "size_limiter", "logging"]
        );
    }

    #[test]
    fn test_connect_disconnect_notifications() {
        let pipeline = PluginPipelineBuilder::new().with_metrics().build();

        pipeline.notify_connect().unwrap();
        pipeline.notify_connect().unwrap();
        pipeline.notify_disconnect().unwrap();

        // Metrics plugin should have recorded these
        // (We can't easily access it here, but the test verifies no panics)
    }
}
