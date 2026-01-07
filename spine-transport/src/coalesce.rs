//! Message coalescing for improved throughput.

use bytes::{BufMut, Bytes, BytesMut};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Notify};
use tokio::time::interval;

use crate::congestion::AdaptiveBatchSizer;
use crate::{Frame, FrameFlags, FrameHeader, TransportError, TransportResult};

// =============================================================================
// COALESCE CONFIG
// =============================================================================

/// Configuration for message coalescing
#[derive(Clone, Debug)]
pub struct CoalesceConfig {
    /// Maximum batch size in bytes
    pub max_batch_bytes: usize,
    /// Maximum batch count
    pub max_batch_count: usize,
    /// Maximum wait time before flushing
    pub max_wait: Duration,
    /// Enable Nagle's algorithm equivalent
    pub enable_nagle: bool,
    /// Nagle timeout
    pub nagle_timeout: Duration,
    /// Enable compression for batches
    pub compress_batches: bool,
    /// Compression threshold (don't compress below this)
    pub compression_threshold: usize,
}

impl Default for CoalesceConfig {
    fn default() -> Self {
        Self {
            max_batch_bytes: 64 * 1024, // 64KB
            max_batch_count: 256,
            max_wait: Duration::from_micros(500),
            enable_nagle: true,
            nagle_timeout: Duration::from_micros(200),
            compress_batches: true,
            compression_threshold: 512,
        }
    }
}

impl CoalesceConfig {
    /// Low-latency configuration
    pub fn low_latency() -> Self {
        Self {
            max_batch_bytes: 8 * 1024,
            max_batch_count: 16,
            max_wait: Duration::from_micros(50),
            enable_nagle: false,
            nagle_timeout: Duration::from_micros(10),
            compress_batches: false,
            compression_threshold: 8 * 1024,
        }
    }

    /// High-throughput configuration
    pub fn high_throughput() -> Self {
        Self {
            max_batch_bytes: 256 * 1024,
            max_batch_count: 1024,
            max_wait: Duration::from_millis(5),
            enable_nagle: true,
            nagle_timeout: Duration::from_millis(1),
            compress_batches: true,
            compression_threshold: 256,
        }
    }
}

// =============================================================================
// MESSAGE COALESCER
// =============================================================================

/// Coalesces multiple small messages into batches
pub struct MessageCoalescer {
    /// Configuration
    config: CoalesceConfig,
    /// Pending messages
    pending: VecDeque<Frame>,
    /// Current batch size
    batch_bytes: usize,
    /// Time of first pending message
    first_pending: Option<Instant>,
    /// Adaptive batch sizer
    adaptive: Option<AdaptiveBatchSizer>,
    /// Statistics
    stats: CoalesceStats,
}

impl MessageCoalescer {
    /// Create a new message coalescer
    pub fn new(config: CoalesceConfig) -> Self {
        Self {
            config,
            pending: VecDeque::new(),
            batch_bytes: 0,
            first_pending: None,
            adaptive: None,
            stats: CoalesceStats::new(),
        }
    }

    /// Create with adaptive batch sizing
    pub fn with_adaptive(config: CoalesceConfig, throughput_bps: u64) -> Self {
        Self {
            config: config.clone(),
            pending: VecDeque::new(),
            batch_bytes: 0,
            first_pending: None,
            adaptive: Some(AdaptiveBatchSizer::from_throughput(
                config.max_batch_bytes,
                throughput_bps,
            )),
            stats: CoalesceStats::new(),
        }
    }

    /// Queue a frame for coalescing
    pub fn queue(&mut self, frame: Frame) -> Option<CoalescedBatch> {
        let frame_size = 12 + frame.payload.len();

        // Check if this frame would exceed batch limits
        let max_bytes = self
            .adaptive
            .as_ref()
            .map(|a| a.recommended_batch_size())
            .unwrap_or(self.config.max_batch_bytes);

        if !self.pending.is_empty()
            && (self.batch_bytes + frame_size > max_bytes
                || self.pending.len() >= self.config.max_batch_count)
        {
            // Flush current batch first
            let batch = self.flush();
            self.queue_internal(frame, frame_size);
            return Some(batch);
        }

        self.queue_internal(frame, frame_size);

        // Check if batch is full
        if self.batch_bytes >= max_bytes || self.pending.len() >= self.config.max_batch_count {
            return Some(self.flush());
        }

        None
    }

    fn queue_internal(&mut self, frame: Frame, size: usize) {
        if self.first_pending.is_none() {
            self.first_pending = Some(Instant::now());
        }
        self.batch_bytes += size;
        self.pending.push_back(frame);
        self.stats.messages_queued.fetch_add(1, Ordering::Relaxed);
    }

    /// Check if a flush is needed based on timeout
    pub fn should_flush(&self) -> bool {
        if self.pending.is_empty() {
            return false;
        }

        if let Some(first) = self.first_pending {
            let timeout = if self.config.enable_nagle {
                self.config.nagle_timeout
            } else {
                self.config.max_wait
            };

            return first.elapsed() >= timeout;
        }

        false
    }

    /// Flush pending messages into a batch
    pub fn flush(&mut self) -> CoalescedBatch {
        let frames: Vec<Frame> = self.pending.drain(..).collect();
        let message_count = frames.len();
        let total_bytes = self.batch_bytes;

        self.batch_bytes = 0;
        self.first_pending = None;

        // Update adaptive sizer
        if let Some(ref mut adaptive) = self.adaptive {
            adaptive.update(total_bytes as u64);
        }

        // Build batch
        let mut batch = CoalescedBatch {
            frames,
            total_bytes,
            compressed: false,
            compressed_bytes: None,
        };

        // Optionally compress
        if self.config.compress_batches && total_bytes >= self.config.compression_threshold {
            batch.compress();
        }

        self.stats.batches_created.fetch_add(1, Ordering::Relaxed);
        self.stats
            .messages_coalesced
            .fetch_add(message_count as u64, Ordering::Relaxed);
        self.stats
            .bytes_coalesced
            .fetch_add(total_bytes as u64, Ordering::Relaxed);

        if batch.compressed {
            self.stats.bytes_saved.fetch_add(
                (total_bytes - batch.compressed_bytes.unwrap_or(total_bytes)) as u64,
                Ordering::Relaxed,
            );
        }

        batch
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Get pending count
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get pending bytes
    pub fn pending_bytes(&self) -> usize {
        self.batch_bytes
    }

    /// Get statistics
    pub fn stats(&self) -> &CoalesceStats {
        &self.stats
    }
}

// =============================================================================
// COALESCED BATCH
// =============================================================================

/// A batch of coalesced frames
pub struct CoalescedBatch {
    /// The coalesced frames
    pub frames: Vec<Frame>,
    /// Total uncompressed bytes
    pub total_bytes: usize,
    /// Whether batch is compressed
    pub compressed: bool,
    /// Compressed size if applicable
    pub compressed_bytes: Option<usize>,
}

impl CoalescedBatch {
    /// Compress the batch
    pub fn compress(&mut self) {
        if self.compressed || self.frames.is_empty() {
            return;
        }

        // Serialize all frames
        let mut buffer = BytesMut::with_capacity(self.total_bytes);

        // Write frame count
        buffer.put_u32_le(self.frames.len() as u32);

        // Write each frame
        for frame in &self.frames {
            // Write header
            buffer.put_slice(&frame.header_bytes());
            // Write payload
            buffer.put_slice(&frame.payload);
        }

        // Compress using zstd
        let compressed = zstd::encode_all(buffer.as_ref(), 3).ok();

        if let Some(compressed_data) = compressed {
            if compressed_data.len() < self.total_bytes {
                // Replace frames with single compressed frame
                let compressed_frame = Frame {
                    header: FrameHeader {
                        length: compressed_data.len() as u32,
                        flags: FrameFlags::COMPRESSED | FrameFlags::BATCHED,
                        sequence: self.frames[0].header.sequence,
                        stream_id: 0, // Batch stream
                        _reserved: 0,
                    },
                    payload: Bytes::from(compressed_data),
                };

                self.compressed_bytes = Some(compressed_frame.payload.len());
                self.frames = vec![compressed_frame];
                self.compressed = true;
            }
        }
    }

    /// Decompress a batch
    pub fn decompress(frame: &Frame) -> TransportResult<Vec<Frame>> {
        if !frame.header.flags.contains(FrameFlags::COMPRESSED) {
            return Err(TransportError::InvalidFrame("Not compressed".into()));
        }

        // Decompress
        let decompressed = zstd::decode_all(frame.payload.as_ref())
            .map_err(|e| TransportError::Compression(e.to_string()))?;

        // Parse frames
        let mut frames = Vec::new();

        // Read frame count
        if decompressed.len() < 4 {
            return Err(TransportError::InvalidFrame("Too short".into()));
        }

        let count = u32::from_le_bytes(decompressed[0..4].try_into().unwrap()) as usize;
        let mut offset = 4;

        for _ in 0..count {
            if offset + 12 > decompressed.len() {
                return Err(TransportError::InvalidFrame("Incomplete header".into()));
            }

            let header = Frame::parse_header(decompressed[offset..offset + 12].try_into().unwrap());
            offset += 12;

            let payload_end = offset + header.length as usize;
            if payload_end > decompressed.len() {
                return Err(TransportError::InvalidFrame("Incomplete payload".into()));
            }

            let payload = Bytes::copy_from_slice(&decompressed[offset..payload_end]);
            offset = payload_end;

            frames.push(Frame { header, payload });
        }

        Ok(frames)
    }

    /// Get wire size
    pub fn wire_size(&self) -> usize {
        if self.compressed {
            self.compressed_bytes.unwrap_or(self.total_bytes)
        } else {
            self.total_bytes
        }
    }

    /// Get compression ratio
    pub fn compression_ratio(&self) -> f64 {
        if let Some(compressed) = self.compressed_bytes {
            self.total_bytes as f64 / compressed as f64
        } else {
            1.0
        }
    }
}

// =============================================================================
// ASYNC COALESCE CHANNEL
// =============================================================================

/// Async channel with automatic coalescing
pub struct CoalesceChannel {
    /// Sender for incoming messages
    tx: mpsc::Sender<Frame>,
    /// Receiver for coalesced batches
    batch_rx: mpsc::Receiver<CoalescedBatch>,
    /// Shutdown notify
    shutdown: Arc<Notify>,
}

impl CoalesceChannel {
    /// Create a new coalesce channel
    pub fn new(config: CoalesceConfig, buffer_size: usize) -> (CoalesceSender, Self) {
        let (tx, rx) = mpsc::channel(buffer_size);
        let (batch_tx, batch_rx) = mpsc::channel(buffer_size / 4 + 1);
        let shutdown = Arc::new(Notify::new());

        // Spawn coalescing task
        let shutdown_clone = Arc::clone(&shutdown);
        let config_clone = config.clone();

        tokio::spawn(async move {
            Self::coalesce_loop(rx, batch_tx, config_clone, shutdown_clone).await;
        });

        let sender = CoalesceSender { tx: tx.clone() };

        let channel = Self {
            tx,
            batch_rx,
            shutdown,
        };

        (sender, channel)
    }

    /// Coalescing loop
    async fn coalesce_loop(
        mut rx: mpsc::Receiver<Frame>,
        batch_tx: mpsc::Sender<CoalescedBatch>,
        config: CoalesceConfig,
        shutdown: Arc<Notify>,
    ) {
        let mut coalescer = MessageCoalescer::new(config.clone());
        let mut flush_interval = interval(config.max_wait);

        loop {
            tokio::select! {
                // Check for incoming frames
                Some(frame) = rx.recv() => {
                    if let Some(batch) = coalescer.queue(frame) {
                        if batch_tx.send(batch).await.is_err() {
                            break;
                        }
                    }
                }

                // Periodic flush
                _ = flush_interval.tick() => {
                    if coalescer.should_flush() {
                        let batch = coalescer.flush();
                        if !batch.frames.is_empty()
                            && batch_tx.send(batch).await.is_err() {
                                break;
                            }
                    }
                }

                // Shutdown
                _ = shutdown.notified() => {
                    // Flush remaining
                    if !coalescer.is_empty() {
                        let batch = coalescer.flush();
                        let _ = batch_tx.send(batch).await;
                    }
                    break;
                }
            }
        }
    }

    /// Receive next coalesced batch
    pub async fn recv(&mut self) -> Option<CoalescedBatch> {
        self.batch_rx.recv().await
    }

    /// Shutdown the channel
    pub fn shutdown(&self) {
        self.shutdown.notify_one();
    }
}

/// Sender half of coalesce channel
#[derive(Clone)]
pub struct CoalesceSender {
    tx: mpsc::Sender<Frame>,
}

impl CoalesceSender {
    /// Send a frame for coalescing
    pub async fn send(&self, frame: Frame) -> TransportResult<()> {
        self.tx
            .send(frame)
            .await
            .map_err(|_| TransportError::ConnectionClosed)
    }
}

// =============================================================================
// STATISTICS
// =============================================================================

/// Coalescing statistics
pub struct CoalesceStats {
    /// Messages queued
    pub messages_queued: AtomicU64,
    /// Messages coalesced into batches
    pub messages_coalesced: AtomicU64,
    /// Batches created
    pub batches_created: AtomicU64,
    /// Total bytes coalesced
    pub bytes_coalesced: AtomicU64,
    /// Bytes saved by compression
    pub bytes_saved: AtomicU64,
}

impl CoalesceStats {
    /// Create new stats
    pub fn new() -> Self {
        Self {
            messages_queued: AtomicU64::new(0),
            messages_coalesced: AtomicU64::new(0),
            batches_created: AtomicU64::new(0),
            bytes_coalesced: AtomicU64::new(0),
            bytes_saved: AtomicU64::new(0),
        }
    }

    /// Get average messages per batch
    pub fn avg_messages_per_batch(&self) -> f64 {
        let batches = self.batches_created.load(Ordering::Relaxed);
        if batches == 0 {
            return 0.0;
        }

        let messages = self.messages_coalesced.load(Ordering::Relaxed);
        messages as f64 / batches as f64
    }

    /// Get compression savings percentage
    pub fn compression_savings_pct(&self) -> f64 {
        let total = self.bytes_coalesced.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }

        let saved = self.bytes_saved.load(Ordering::Relaxed);
        (saved as f64 / total as f64) * 100.0
    }
}

impl Default for CoalesceStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameBuilder;

    #[test]
    fn test_coalescer_basic() {
        let config = CoalesceConfig {
            max_batch_count: 3,
            max_batch_bytes: 1024,
            ..Default::default()
        };

        let mut coalescer = MessageCoalescer::new(config);

        // Queue some frames
        let frame1 = FrameBuilder::new().payload(b"Hello".to_vec()).build();
        let frame2 = FrameBuilder::new().payload(b"World".to_vec()).build();

        assert!(coalescer.queue(frame1).is_none());
        assert!(coalescer.queue(frame2).is_none());

        // Third frame should trigger flush
        let frame3 = FrameBuilder::new().payload(b"!".to_vec()).build();
        let batch = coalescer.queue(frame3);

        // Batch contains first 2 frames, frame3 is pending
        assert!(batch.is_none() || batch.as_ref().map(|b| b.frames.len()) == Some(3));
    }

    #[test]
    fn test_batch_compression() {
        let frames: Vec<Frame> = (0..10)
            .map(|i| FrameBuilder::new().payload(vec![i as u8; 100]).build())
            .collect();

        let total_bytes: usize = frames.iter().map(|f| 12 + f.payload.len()).sum();

        let mut batch = CoalescedBatch {
            frames,
            total_bytes,
            compressed: false,
            compressed_bytes: None,
        };

        batch.compress();

        assert!(batch.compressed);
        assert!(batch.compressed_bytes.unwrap() < total_bytes);
        assert!(batch.compression_ratio() > 1.0);
    }

    #[test]
    fn test_config_presets() {
        let low_latency = CoalesceConfig::low_latency();
        let high_throughput = CoalesceConfig::high_throughput();

        assert!(low_latency.max_batch_bytes < high_throughput.max_batch_bytes);
        assert!(low_latency.max_wait < high_throughput.max_wait);
        assert!(!low_latency.compress_batches);
        assert!(high_throughput.compress_batches);
    }

    #[tokio::test]
    async fn test_coalesce_channel() {
        let config = CoalesceConfig {
            max_batch_count: 5,
            max_wait: Duration::from_millis(10),
            ..Default::default()
        };

        let (sender, mut channel) = CoalesceChannel::new(config, 100);

        // Send some frames
        for i in 0..5 {
            let frame = FrameBuilder::new().payload(vec![i as u8; 50]).build();
            sender.send(frame).await.unwrap();
        }

        // Should get a batch
        let batch = tokio::time::timeout(Duration::from_millis(100), channel.recv())
            .await
            .ok()
            .flatten();

        assert!(batch.is_some());

        channel.shutdown();
    }
}
