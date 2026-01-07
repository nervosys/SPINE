// Allow dead code for streaming APIs designed for future use
#![allow(dead_code)]

//! # SPINE Stream
//!
//! High-performance streaming layer for the SPINE Agentic web stack.
//!
//! This crate provides streaming abstractions that connect the low-level transport
//! layer with higher-level agent communication patterns.
//!
//! ## Features
//!
//! - **Reactive Streams**: Async streams with backpressure support
//! - **Multiplexed Channels**: Multiple logical streams over single connection
//! - **Priority Queuing**: Urgent messages bypass normal queuing
//! - **Flow Control**: Adaptive rate limiting based on receiver capacity
//! - **Latent Streaming**: Native support for embedding/tensor streams
//! - **Chunked Transfer**: Efficient large payload streaming
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      Application Layer                          │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
//! │  │ AgentStream │  │ EventStream │  │ LatentVectorStream      │ │
//! │  └──────┬──────┘  └──────┬──────┘  └────────────┬────────────┘ │
//! │         │                │                      │               │
//! │  ┌──────┴────────────────┴──────────────────────┴──────────┐   │
//! │  │                   StreamMultiplexer                      │   │
//! │  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────────┐ │   │
//! │  │  │ Stream0 │  │ Stream1 │  │ Stream2 │  │ Priority Q  │ │   │
//! │  │  └────┬────┘  └────┬────┘  └────┬────┘  └──────┬──────┘ │   │
//! │  └───────┴────────────┴────────────┴─────────────┬──────────┘   │
//! │                                                   │              │
//! │  ┌────────────────────────────────────────────────┴──────────┐  │
//! │  │                    FlowController                          │  │
//! │  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐  │  │
//! │  │  │ Backpressure  │  │ Rate Limiter  │  │ Window Mgmt   │  │  │
//! │  │  └───────────────┘  └───────────────┘  └───────────────┘  │  │
//! │  └────────────────────────────────────────────────┬──────────┘  │
//! │                                                   │              │
//! │  ┌────────────────────────────────────────────────┴──────────┐  │
//! │  │                   ChunkedTransfer                          │  │
//! │  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐  │  │
//! │  │  │   Chunker     │  │   Assembler   │  │   Retransmit  │  │  │
//! │  │  └───────────────┘  └───────────────┘  └───────────────┘  │  │
//! │  └────────────────────────────────────────────────┬──────────┘  │
//! │                                                   │              │
//! │  ┌────────────────────────────────────────────────┴──────────┐  │
//! │  │                 spine-transport                       │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

pub mod channel;
pub mod chunk;
pub mod flow;
pub mod latent;
pub mod multiplex;
pub mod priority;
pub mod reactive;

pub use channel::*;
pub use chunk::*;
pub use flow::*;
pub use latent::*;
pub use multiplex::*;
pub use priority::*;
pub use reactive::*;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

// =============================================================================
// STREAM ERROR TYPES
// =============================================================================

/// Stream layer errors
#[derive(Debug, Error)]
pub enum StreamError {
    #[error("Stream closed")]
    Closed,

    #[error("Stream timeout")]
    Timeout,

    #[error("Backpressure limit reached")]
    BackpressureLimitReached,

    #[error("Flow control violation: {0}")]
    FlowControlViolation(String),

    #[error("Invalid stream ID: {0}")]
    InvalidStreamId(u32),

    #[error("Chunk assembly error: {0}")]
    ChunkAssemblyError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Transport error: {0}")]
    Transport(#[from] spine_transport::TransportError),

    #[error("Channel send error")]
    ChannelSendError,

    #[error("Channel receive error")]
    ChannelRecvError,

    #[error("Stream not found: {0}")]
    StreamNotFound(u32),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Channel full")]
    ChannelFull,

    #[error("Request timeout")]
    RequestTimeout,

    #[error("Flow paused")]
    FlowPaused,

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: u32, actual: u32 },

    #[error("Transfer timeout")]
    TransferTimeout,

    #[error("Missing chunk {index} for transfer {transfer_id}")]
    MissingChunk { transfer_id: Uuid, index: u32 },

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("Decompression error: {0}")]
    DecompressionError(String),

    #[error("Invalid message: {0}")]
    InvalidMessage(String),
}

pub type StreamResult<T> = Result<T, StreamError>;

// =============================================================================
// STREAM CONFIGURATION
// =============================================================================

/// Configuration for stream behavior
#[derive(Clone, Debug)]
pub struct StreamConfig {
    /// Maximum in-flight bytes before backpressure kicks in
    pub max_in_flight_bytes: usize,
    /// Maximum pending items in stream buffer
    pub max_pending_items: usize,
    /// Default chunk size for large payloads
    pub chunk_size: usize,
    /// Flow control window size
    pub window_size: usize,
    /// Enable automatic compression
    pub auto_compress: bool,
    /// Compression threshold (compress if larger)
    pub compression_threshold: usize,
    /// Idle timeout before stream cleanup
    pub idle_timeout: Duration,
    /// Enable priority queuing
    pub enable_priority: bool,
    /// Number of priority levels
    pub priority_levels: u8,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_in_flight_bytes: 16 * 1024 * 1024, // 16MB
            max_pending_items: 1024,
            chunk_size: 64 * 1024,   // 64KB chunks
            window_size: 256 * 1024, // 256KB window
            auto_compress: true,
            compression_threshold: 1024,            // Compress > 1KB
            idle_timeout: Duration::from_secs(300), // 5 minutes
            enable_priority: true,
            priority_levels: 8,
        }
    }
}

impl StreamConfig {
    /// Low-latency configuration
    pub fn low_latency() -> Self {
        Self {
            max_in_flight_bytes: 1024 * 1024,
            max_pending_items: 128,
            chunk_size: 8 * 1024,
            window_size: 32 * 1024,
            auto_compress: false,
            compression_threshold: 4096,
            idle_timeout: Duration::from_secs(60),
            enable_priority: true,
            priority_levels: 4,
        }
    }

    /// High-throughput configuration
    pub fn high_throughput() -> Self {
        Self {
            max_in_flight_bytes: 64 * 1024 * 1024,
            max_pending_items: 4096,
            chunk_size: 256 * 1024,
            window_size: 1024 * 1024,
            auto_compress: true,
            compression_threshold: 512,
            idle_timeout: Duration::from_secs(600),
            enable_priority: false,
            priority_levels: 2,
        }
    }

    /// Balanced configuration
    pub fn balanced() -> Self {
        Self::default()
    }
}

// =============================================================================
// STREAM MESSAGE TYPES
// =============================================================================

/// A stream message with metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamMessage {
    /// Unique message ID
    pub id: Uuid,
    /// Stream ID this message belongs to
    pub stream_id: u32,
    /// Sequence number within the stream
    pub sequence: u64,
    /// Message payload
    pub payload: StreamPayload,
    /// Priority level (0 = highest)
    pub priority: u8,
    /// Timestamp in nanoseconds since epoch
    pub timestamp_ns: u64,
    /// Optional correlation ID for request/response pairing
    pub correlation_id: Option<Uuid>,
}

/// Stream payload types
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StreamPayload {
    /// Raw bytes
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
    /// Latent vector (embedding)
    LatentVector { dimensions: u32, vector: Vec<f32> },
    /// Batched latent vectors
    LatentBatch {
        count: u32,
        dimensions: u32,
        vectors: Vec<f32>,
    },
    /// Chunked data (part of larger transfer)
    Chunk {
        meta: ChunkMeta,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
    /// Control message
    Control(StreamControl),
    /// Event notification
    Event(StreamEvent),
    /// Compressed payload
    Compressed {
        algorithm: CompressionAlg,
        original_size: usize,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    },
}

/// Chunk metadata for chunked transfers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChunkMeta {
    /// Transfer ID (groups chunks)
    pub transfer_id: Uuid,
    /// Chunk index
    pub index: u32,
    /// Total chunks in transfer
    pub total_chunks: u32,
    /// Total size of the complete transfer
    pub total_size: u64,
    /// Is this the final chunk?
    pub is_last: bool,
    /// Checksum of this chunk (CRC32)
    pub checksum: Option<u32>,
    /// Is chunk compressed?
    pub compressed: bool,
}

/// Stream control messages
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StreamControl {
    /// Open a new stream
    Open {
        stream_id: u32,
        config: Option<Vec<u8>>,
    },
    /// Close a stream
    Close {
        stream_id: u32,
        reason: Option<String>,
    },
    /// Flow control: window update
    WindowUpdate { stream_id: u32, increment: u32 },
    /// Flow control: pause stream
    Pause { stream_id: u32 },
    /// Flow control: resume stream
    Resume { stream_id: u32 },
    /// Ping for keepalive
    Ping { payload: u64 },
    /// Pong response
    Pong { payload: u64 },
    /// Acknowledge receipt
    Ack { stream_id: u32, sequence: u64 },
    /// Request retransmission
    Nack { stream_id: u32, sequence: u64 },
    /// Reset stream state
    Reset { stream_id: u32, error_code: u32 },
}

/// Stream events
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StreamEvent {
    /// Stream opened
    Opened { stream_id: u32 },
    /// Stream closed
    Closed {
        stream_id: u32,
        reason: Option<String>,
    },
    /// Flow control state changed
    FlowStateChanged { stream_id: u32, paused: bool },
    /// Error occurred
    Error { stream_id: u32, message: String },
    /// Latency measurement
    LatencyMeasurement { stream_id: u32, rtt_us: u64 },
    /// Throughput measurement
    ThroughputMeasurement { stream_id: u32, bytes_per_sec: u64 },
}

/// Compression algorithms
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompressionAlg {
    None,
    Zstd,
    Lz4,
}

// =============================================================================
// STREAM STATISTICS
// =============================================================================

/// Statistics for a single stream
#[derive(Clone, Debug, Default)]
pub struct StreamStats {
    /// Messages sent
    pub messages_sent: u64,
    /// Messages received
    pub messages_received: u64,
    /// Bytes sent (before compression)
    pub bytes_sent: u64,
    /// Bytes received (before decompression)
    pub bytes_received: u64,
    /// Bytes saved by compression
    pub compression_savings: u64,
    /// Retransmissions
    pub retransmissions: u64,
    /// Flow control pauses
    pub flow_pauses: u64,
    /// Average latency in microseconds
    pub avg_latency_us: u64,
    /// Peak latency in microseconds
    pub peak_latency_us: u64,
    /// Window size updates
    pub window_updates: u64,
}

/// Aggregate statistics across all streams
#[derive(Clone, Debug, Default)]
pub struct AggregateStats {
    /// Active stream count
    pub active_streams: u32,
    /// Total streams created
    pub total_streams_created: u64,
    /// Total streams closed
    pub total_streams_closed: u64,
    /// Total messages across all streams
    pub total_messages: u64,
    /// Total bytes across all streams
    pub total_bytes: u64,
    /// Current in-flight bytes
    pub in_flight_bytes: u64,
    /// Backpressure events
    pub backpressure_events: u64,
    /// Priority queue high-water mark
    pub priority_queue_hwm: u32,
}

// =============================================================================
// STREAM HANDLE
// =============================================================================

/// Handle to a stream for sending messages
#[derive(Clone)]
pub struct StreamHandle {
    stream_id: u32,
    tx: tokio::sync::mpsc::Sender<StreamMessage>,
    sequence: std::sync::Arc<std::sync::atomic::AtomicU64>,
    config: StreamConfig,
}

impl StreamHandle {
    /// Create a new stream handle
    pub fn new(
        stream_id: u32,
        tx: tokio::sync::mpsc::Sender<StreamMessage>,
        config: StreamConfig,
    ) -> Self {
        Self {
            stream_id,
            tx,
            sequence: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            config,
        }
    }

    /// Get the stream ID
    pub fn id(&self) -> u32 {
        self.stream_id
    }

    /// Send bytes on this stream
    pub async fn send_bytes(&self, data: impl Into<Bytes>) -> StreamResult<()> {
        let seq = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let data = data.into();

        let payload = if self.config.auto_compress && data.len() > self.config.compression_threshold
        {
            let compressed = zstd::encode_all(data.as_ref(), 3)
                .map_err(|e| StreamError::SerializationError(e.to_string()))?;
            StreamPayload::Compressed {
                algorithm: CompressionAlg::Zstd,
                original_size: data.len(),
                data: compressed,
            }
        } else {
            StreamPayload::Bytes(data.to_vec())
        };

        let msg = StreamMessage {
            id: Uuid::new_v4(),
            stream_id: self.stream_id,
            sequence: seq,
            payload,
            priority: 4, // Default priority
            timestamp_ns: timestamp_now(),
            correlation_id: None,
        };

        self.tx
            .send(msg)
            .await
            .map_err(|_| StreamError::ChannelSendError)
    }

    /// Send a latent vector (embedding)
    pub async fn send_latent(&self, dimensions: u32, vector: Vec<f32>) -> StreamResult<()> {
        let seq = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let msg = StreamMessage {
            id: Uuid::new_v4(),
            stream_id: self.stream_id,
            sequence: seq,
            payload: StreamPayload::LatentVector { dimensions, vector },
            priority: 2, // Higher priority for latent vectors
            timestamp_ns: timestamp_now(),
            correlation_id: None,
        };

        self.tx
            .send(msg)
            .await
            .map_err(|_| StreamError::ChannelSendError)
    }

    /// Send with custom priority (0 = highest)
    pub async fn send_priority(&self, data: impl Into<Bytes>, priority: u8) -> StreamResult<()> {
        let seq = self
            .sequence
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let msg = StreamMessage {
            id: Uuid::new_v4(),
            stream_id: self.stream_id,
            sequence: seq,
            payload: StreamPayload::Bytes(data.into().to_vec()),
            priority,
            timestamp_ns: timestamp_now(),
            correlation_id: None,
        };

        self.tx
            .send(msg)
            .await
            .map_err(|_| StreamError::ChannelSendError)
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Get current timestamp in nanoseconds
#[inline]
pub fn timestamp_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_config_presets() {
        let low_lat = StreamConfig::low_latency();
        assert!(low_lat.chunk_size < 16 * 1024);
        assert!(!low_lat.auto_compress);

        let high_tp = StreamConfig::high_throughput();
        assert!(high_tp.chunk_size >= 64 * 1024);
        assert!(high_tp.auto_compress);
    }

    #[tokio::test]
    async fn test_stream_handle_send() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let handle = StreamHandle::new(1, tx, StreamConfig::default());

        handle.send_bytes(b"hello".as_slice()).await.unwrap();

        let msg = rx.recv().await.unwrap();
        assert_eq!(msg.stream_id, 1);
        assert_eq!(msg.sequence, 0);
    }

    #[test]
    fn test_compression_algorithms() {
        assert_ne!(CompressionAlg::Zstd, CompressionAlg::Lz4);
        assert_eq!(CompressionAlg::None, CompressionAlg::None);
    }
}
