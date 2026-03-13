// Allow dead code for transport layer APIs designed for future use
#![allow(dead_code)]

//! SPINE Transport Layer
//!
//! High-performance, zero-copy transport protocols optimized for agentic AI workloads.
//!
//! # Features
//!
//! - **Zero-Copy I/O**: Ring buffers and memory pools eliminate copying
//! - **Vectored I/O**: Scatter-gather reduces syscalls
//! - **Connection Pooling**: Smart reuse with health monitoring
//! - **Adaptive Congestion Control**: BBR-inspired algorithms
//! - **Batch Coalescing**: Combine small messages for efficiency
//! - **io_uring Support**: Linux kernel bypass for extreme performance
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Application Layer                            │
//! │              (spine-protocol Messages)                     │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                    Coalescing Layer                             │
//! │         (Batch small messages, Nagle-like buffering)           │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                    Framing Layer                                │
//! │           (Length-prefixed, zero-copy frames)                   │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                    Connection Pool                              │
//! │        (Health monitoring, adaptive load balancing)            │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                    Transport Layer                              │
//! │              (TCP/QUIC/io_uring backends)                       │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                    Buffer Management                            │
//! │           (Ring buffers, slab allocator, mmap)                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

pub mod buffer;
pub mod coalesce;
pub mod congestion;
pub mod connection;
pub mod frame;
pub mod metrics;
pub mod marketplace;
pub mod plugin;
pub mod pool;
pub mod websocket;

#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub mod uring;

#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub mod uring_backend;

pub use buffer::*;
pub use coalesce::*;
pub use congestion::*;
pub use connection::*;
pub use frame::*;
pub use metrics::*;
pub use plugin::*;
pub use pool::*;
pub use websocket::*;

#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub use uring::*;

use bytes::{Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

// =============================================================================
// TRANSPORT CONFIGURATION
// =============================================================================

/// Transport layer configuration
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Maximum message size (default: 16MB)
    pub max_message_size: usize,

    /// Ring buffer size for zero-copy I/O (default: 4MB)
    pub ring_buffer_size: usize,

    /// Number of buffers in the slab allocator (default: 1024)
    pub slab_buffer_count: usize,

    /// Size of each slab buffer (default: 64KB)
    pub slab_buffer_size: usize,

    /// Connection pool size per endpoint (default: 16)
    pub pool_size: usize,

    /// Idle connection timeout (default: 60s)
    pub idle_timeout: Duration,

    /// Health check interval (default: 5s)
    pub health_check_interval: Duration,

    /// Enable Nagle-like coalescing (default: true)
    pub enable_coalescing: bool,

    /// Coalescing delay (default: 1ms)
    pub coalesce_delay: Duration,

    /// Maximum batch size for coalescing (default: 64)
    pub max_batch_size: usize,

    /// Enable BBR congestion control (default: true)
    pub enable_bbr: bool,

    /// Initial congestion window (default: 10 MSS)
    pub initial_cwnd: u32,

    /// Enable TCP_NODELAY when coalescing is disabled
    pub tcp_nodelay: bool,

    /// Send buffer size (default: 2MB)
    pub send_buffer_size: usize,

    /// Receive buffer size (default: 2MB)
    pub recv_buffer_size: usize,

    /// Enable zero-copy send (requires Linux 4.14+)
    pub zero_copy_send: bool,

    /// Compression level (0 = disabled, 1-22 for zstd)
    pub compression_level: i32,

    /// Use LZ4 for faster compression (vs zstd)
    pub use_lz4: bool,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            max_message_size: 16 * 1024 * 1024, // 16MB
            ring_buffer_size: 4 * 1024 * 1024,  // 4MB
            slab_buffer_count: 1024,
            slab_buffer_size: 64 * 1024, // 64KB
            pool_size: 16,
            idle_timeout: Duration::from_secs(60),
            health_check_interval: Duration::from_secs(5),
            enable_coalescing: true,
            coalesce_delay: Duration::from_millis(1),
            max_batch_size: 64,
            enable_bbr: true,
            initial_cwnd: 10,
            tcp_nodelay: true,
            send_buffer_size: 2 * 1024 * 1024, // 2MB
            recv_buffer_size: 2 * 1024 * 1024, // 2MB
            zero_copy_send: true,
            compression_level: 3,
            use_lz4: false, // zstd by default for better ratio
        }
    }
}

impl TransportConfig {
    /// Create config optimized for low latency
    pub fn low_latency() -> Self {
        Self {
            enable_coalescing: false,
            coalesce_delay: Duration::ZERO,
            tcp_nodelay: true,
            compression_level: 1,
            use_lz4: true,
            ..Default::default()
        }
    }

    /// Create config optimized for high throughput
    pub fn high_throughput() -> Self {
        Self {
            ring_buffer_size: 16 * 1024 * 1024, // 16MB
            slab_buffer_count: 4096,
            enable_coalescing: true,
            coalesce_delay: Duration::from_millis(5),
            max_batch_size: 256,
            compression_level: 6,
            ..Default::default()
        }
    }

    /// Create config optimized for many connections
    pub fn many_connections() -> Self {
        Self {
            pool_size: 64,
            ring_buffer_size: 1024 * 1024, // 1MB per connection
            slab_buffer_size: 16 * 1024,   // 16KB buffers
            ..Default::default()
        }
    }
}

// =============================================================================
// TRANSPORT STATISTICS
// =============================================================================

/// Runtime statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct TransportStats {
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Bytes saved by compression
    pub compression_savings: u64,
    /// Bytes saved by coalescing
    pub coalescing_savings: u64,
    /// Current connections in pool
    pub pool_connections: u32,
    /// Pool hits (reused connection)
    pub pool_hits: u64,
    /// Pool misses (new connection)
    pub pool_misses: u64,
    /// Current RTT estimate (microseconds)
    pub rtt_us: u64,
    /// Current bandwidth estimate (bytes/sec)
    pub bandwidth_bps: u64,
    /// Congestion events
    pub congestion_events: u64,
    /// Zero-copy sends
    pub zero_copy_sends: u64,
    /// Buffer allocations
    pub buffer_allocs: u64,
    /// Buffer reuses from pool
    pub buffer_reuses: u64,
}

// =============================================================================
// TRANSPORT ERRORS
// =============================================================================

/// Transport layer errors
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Connection timeout")]
    Timeout,

    #[error("Message too large: {size} > {max}")]
    MessageTooLarge { size: usize, max: usize },

    #[error("Invalid frame: {0}")]
    InvalidFrame(String),

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Pool exhausted")]
    PoolExhausted,

    #[error("Resource exhausted: {resource}")]
    ResourceExhausted { resource: String },

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("TLS error: {0}")]
    Tls(String),

    #[cfg(feature = "quic")]
    #[error("QUIC error: {0}")]
    Quic(#[from] quinn::ConnectionError),
}

pub type TransportResult<T> = Result<T, TransportError>;

// =============================================================================
// CORE TRAITS
// =============================================================================

/// Trait for transport backends
pub trait TransportBackend: Send + Sync {
    /// Send a frame
    fn send_frame(
        &mut self,
        frame: Frame,
    ) -> impl std::future::Future<Output = TransportResult<()>> + Send;

    /// Receive a frame
    fn recv_frame(&mut self) -> impl std::future::Future<Output = TransportResult<Frame>> + Send;

    /// Flush pending data
    fn flush(&mut self) -> impl std::future::Future<Output = TransportResult<()>> + Send;

    /// Get current RTT estimate
    fn rtt(&self) -> Duration;

    /// Check if connection is healthy
    fn is_healthy(&self) -> bool;

    /// Close the connection
    fn close(&mut self) -> impl std::future::Future<Output = TransportResult<()>> + Send;
}

/// Trait for buffer allocation
pub trait BufferAllocator: Send + Sync {
    /// Allocate a buffer of at least the given size
    fn allocate(&self, size: usize) -> BytesMut;

    /// Return a buffer to the pool
    fn deallocate(&self, buffer: BytesMut);

    /// Get allocator statistics
    fn stats(&self) -> BufferStats;
}

/// Buffer allocator statistics
#[derive(Debug, Clone, Default)]
pub struct BufferStats {
    pub allocated: u64,
    pub deallocated: u64,
    pub in_use: u64,
    pub pool_size: u64,
}

// =============================================================================
// FRAME TYPES
// =============================================================================

/// Wire frame format for zero-copy transmission
#[derive(Debug, Clone)]
pub struct Frame {
    /// Frame header
    pub header: FrameHeader,
    /// Frame payload (zero-copy bytes)
    pub payload: Bytes,
}

/// Frame header (12 bytes, cache-line aligned)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[repr(C, align(8))]
pub struct FrameHeader {
    /// Payload length
    pub length: u32,
    /// Frame flags
    pub flags: FrameFlags,
    /// Sequence number for ordering
    pub sequence: u32,
    /// Stream ID for multiplexing
    pub stream_id: u16,
    /// Reserved for future use
    pub _reserved: u16,
}

bitflags::bitflags! {
    /// Frame flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FrameFlags: u8 {
        /// Frame is compressed
        const COMPRESSED = 0b0000_0001;
        /// Frame is encrypted
        const ENCRYPTED = 0b0000_0010;
        /// Frame is part of a batch
        const BATCHED = 0b0000_0100;
        /// Frame requires acknowledgement
        const ACK_REQUIRED = 0b0000_1000;
        /// Frame is a control frame
        const CONTROL = 0b0001_0000;
        /// Frame is final in stream
        const FIN = 0b0010_0000;
        /// Frame uses zero-copy send
        const ZERO_COPY = 0b0100_0000;
        /// Frame is priority
        const PRIORITY = 0b1000_0000;
    }
}

impl Serialize for FrameFlags {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(self.bits())
    }
}

impl<'de> Deserialize<'de> for FrameFlags {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bits = u8::deserialize(deserializer)?;
        FrameFlags::from_bits(bits)
            .ok_or_else(|| serde::de::Error::custom(format!("Invalid FrameFlags bits: {}", bits)))
    }
}

impl Default for FrameFlags {
    fn default() -> Self {
        FrameFlags::empty()
    }
}

impl Frame {
    /// Create a new frame
    pub fn new(payload: Bytes) -> Self {
        Self {
            header: FrameHeader {
                length: payload.len() as u32,
                flags: FrameFlags::empty(),
                sequence: 0,
                stream_id: 0,
                _reserved: 0,
            },
            payload,
        }
    }

    /// Create a compressed frame
    pub fn compressed(payload: Bytes, level: i32, use_lz4: bool) -> TransportResult<Self> {
        let compressed = if use_lz4 {
            lz4_flex::compress_prepend_size(&payload)
        } else {
            zstd::encode_all(&payload[..], level)
                .map_err(|e| TransportError::Compression(e.to_string()))?
        };

        let mut frame = Self::new(Bytes::from(compressed));
        frame.header.flags |= FrameFlags::COMPRESSED;
        Ok(frame)
    }

    /// Decompress frame payload
    pub fn decompress(&self, use_lz4: bool) -> TransportResult<Bytes> {
        if !self.header.flags.contains(FrameFlags::COMPRESSED) {
            return Ok(self.payload.clone());
        }

        let decompressed = if use_lz4 {
            lz4_flex::decompress_size_prepended(&self.payload)
                .map_err(|e| TransportError::Compression(e.to_string()))?
        } else {
            zstd::decode_all(&self.payload[..])
                .map_err(|e| TransportError::Compression(e.to_string()))?
        };

        Ok(Bytes::from(decompressed))
    }

    /// Serialize frame header to bytes
    pub fn header_bytes(&self) -> [u8; 12] {
        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&self.header.length.to_le_bytes());
        buf[4] = self.header.flags.bits();
        buf[5..8].copy_from_slice(&self.header.sequence.to_le_bytes()[0..3]);
        buf[8..10].copy_from_slice(&self.header.stream_id.to_le_bytes());
        buf[10..12].copy_from_slice(&self.header._reserved.to_le_bytes());
        buf
    }

    /// Parse frame header from bytes
    pub fn parse_header(buf: &[u8; 12]) -> FrameHeader {
        let mut seq_bytes = [0u8; 4];
        seq_bytes[0..3].copy_from_slice(&buf[5..8]);

        FrameHeader {
            length: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
            flags: FrameFlags::from_bits_truncate(buf[4]),
            sequence: u32::from_le_bytes(seq_bytes),
            stream_id: u16::from_le_bytes([buf[8], buf[9]]),
            _reserved: u16::from_le_bytes([buf[10], buf[11]]),
        }
    }
}

// =============================================================================
// HIGH-LEVEL TRANSPORT API
// =============================================================================

/// High-performance transport connection
pub struct HyperTransport<B: TransportBackend> {
    /// Transport backend
    backend: B,
    /// Configuration
    config: TransportConfig,
    /// Buffer allocator
    allocator: Arc<dyn BufferAllocator>,
    /// Message coalescer
    coalescer: Option<MessageCoalescer>,
    /// Congestion controller
    congestion: Option<BbrController>,
    /// Statistics
    stats: Arc<parking_lot::RwLock<TransportStats>>,
    /// Sequence counter
    sequence: u32,
}

impl<B: TransportBackend> HyperTransport<B> {
    /// Create a new transport with the given backend
    pub fn new(backend: B, config: TransportConfig, allocator: Arc<dyn BufferAllocator>) -> Self {
        let coalescer = if config.enable_coalescing {
            Some(MessageCoalescer::new(CoalesceConfig {
                max_batch_bytes: config.max_batch_size * 1024,
                max_batch_count: config.max_batch_size,
                max_wait: config.coalesce_delay,
                ..Default::default()
            }))
        } else {
            None
        };

        let congestion = if config.enable_bbr {
            Some(BbrController::new())
        } else {
            None
        };

        Self {
            backend,
            config,
            allocator,
            coalescer,
            congestion,
            stats: Arc::new(parking_lot::RwLock::new(TransportStats::default())),
            sequence: 0,
        }
    }

    /// Send a message
    pub async fn send(&mut self, data: &[u8]) -> TransportResult<()> {
        // Check size limit
        if data.len() > self.config.max_message_size {
            return Err(TransportError::MessageTooLarge {
                size: data.len(),
                max: self.config.max_message_size,
            });
        }

        // Apply compression if configured
        let payload = if self.config.compression_level > 0 && data.len() > 256 {
            let compressed = if self.config.use_lz4 {
                lz4_flex::compress_prepend_size(data)
            } else {
                zstd::encode_all(data, self.config.compression_level)
                    .map_err(|e| TransportError::Compression(e.to_string()))?
            };

            // Only use compressed if smaller
            if compressed.len() < data.len() {
                let mut stats = self.stats.write();
                stats.compression_savings += (data.len() - compressed.len()) as u64;
                (Bytes::from(compressed), true)
            } else {
                (Bytes::copy_from_slice(data), false)
            }
        } else {
            (Bytes::copy_from_slice(data), false)
        };

        // Create frame
        let mut frame = Frame::new(payload.0);
        frame.header.sequence = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);

        if payload.1 {
            frame.header.flags |= FrameFlags::COMPRESSED;
        }

        // Handle coalescing
        if let Some(ref mut coalescer) = self.coalescer {
            if let Some(batch) = coalescer.queue(frame) {
                for f in batch.frames {
                    self.backend.send_frame(f).await?;
                }
            }

            // Check if we should flush
            if coalescer.should_flush() {
                let batch = coalescer.flush();
                for f in batch.frames {
                    self.backend.send_frame(f).await?;
                }
            }
        } else {
            self.backend.send_frame(frame).await?;
        }

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.bytes_sent += data.len() as u64;
            stats.messages_sent += 1;
        }

        Ok(())
    }

    /// Receive a message
    pub async fn recv(&mut self) -> TransportResult<Bytes> {
        let frame = self.backend.recv_frame().await?;

        // Decompress if needed
        let data = if frame.header.flags.contains(FrameFlags::COMPRESSED) {
            frame.decompress(self.config.use_lz4)?
        } else {
            frame.payload
        };

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.bytes_received += data.len() as u64;
            stats.messages_received += 1;
        }

        // Update congestion controller
        if let Some(ref mut cc) = self.congestion {
            cc.on_ack(data.len(), self.backend.rtt());
        }

        Ok(data)
    }

    /// Flush pending data
    pub async fn flush(&mut self) -> TransportResult<()> {
        // Flush coalescer
        if let Some(ref mut coalescer) = self.coalescer {
            let batch = coalescer.flush();
            for frame in batch.frames {
                self.backend.send_frame(frame).await?;
            }
        }

        self.backend.flush().await
    }

    /// Get current statistics
    pub fn stats(&self) -> TransportStats {
        self.stats.read().clone()
    }

    /// Get current RTT
    pub fn rtt(&self) -> Duration {
        self.backend.rtt()
    }

    /// Check if connection is healthy
    pub fn is_healthy(&self) -> bool {
        self.backend.is_healthy()
    }

    /// Close the transport
    pub async fn close(mut self) -> TransportResult<()> {
        self.flush().await?;
        self.backend.close().await
    }
}

// Re-export for convenience
pub use bitflags;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_header_serialization() {
        let header = FrameHeader {
            length: 12345,
            flags: FrameFlags::COMPRESSED | FrameFlags::ENCRYPTED,
            sequence: 42,
            stream_id: 7,
            _reserved: 0,
        };

        let frame = Frame {
            header,
            payload: Bytes::from_static(b"test"),
        };

        let bytes = frame.header_bytes();
        let parsed = Frame::parse_header(&bytes);

        assert_eq!(parsed.length, 12345);
        assert!(parsed.flags.contains(FrameFlags::COMPRESSED));
        assert!(parsed.flags.contains(FrameFlags::ENCRYPTED));
        assert_eq!(parsed.sequence, 42);
        assert_eq!(parsed.stream_id, 7);
    }

    #[test]
    fn test_frame_compression_lz4() {
        let data = b"Hello World! ".repeat(100);
        let frame = Frame::compressed(Bytes::from(data.clone()), 0, true).unwrap();

        assert!(frame.header.flags.contains(FrameFlags::COMPRESSED));
        assert!(frame.payload.len() < data.len());

        let decompressed = frame.decompress(true).unwrap();
        assert_eq!(&decompressed[..], &data[..]);
    }

    #[test]
    fn test_frame_compression_zstd() {
        let data = b"Hello World! ".repeat(100);
        let frame = Frame::compressed(Bytes::from(data.clone()), 3, false).unwrap();

        assert!(frame.header.flags.contains(FrameFlags::COMPRESSED));
        assert!(frame.payload.len() < data.len());

        let decompressed = frame.decompress(false).unwrap();
        assert_eq!(&decompressed[..], &data[..]);
    }

    #[test]
    fn test_config_presets() {
        let low_lat = TransportConfig::low_latency();
        assert!(!low_lat.enable_coalescing);
        assert!(low_lat.use_lz4);

        let high_tp = TransportConfig::high_throughput();
        assert!(high_tp.enable_coalescing);
        assert_eq!(high_tp.max_batch_size, 256);

        let many_conn = TransportConfig::many_connections();
        assert_eq!(many_conn.pool_size, 64);
    }
}
