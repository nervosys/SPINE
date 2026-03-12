//! Chunked transfer protocol for efficient large payload handling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use parking_lot::RwLock;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{ChunkMeta, StreamError, StreamMessage, StreamPayload, StreamResult};

// =============================================================================
// CHUNK CONFIGURATION
// =============================================================================

/// Configuration for chunked transfers
#[derive(Clone, Debug)]
pub struct ChunkConfig {
    /// Maximum chunk size in bytes
    pub max_chunk_size: usize,
    /// Timeout for incomplete transfers
    pub transfer_timeout: Duration,
    /// Maximum concurrent transfers
    pub max_concurrent_transfers: usize,
    /// Enable compression for chunks
    pub compress_chunks: bool,
    /// Compression threshold (only compress if larger)
    pub compression_threshold: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            max_chunk_size: 64 * 1024, // 64KB
            transfer_timeout: Duration::from_secs(30),
            max_concurrent_transfers: 100,
            compress_chunks: true,
            compression_threshold: 1024, // 1KB
        }
    }
}

// =============================================================================
// CHUNKED SENDER
// =============================================================================

/// Sends large payloads as a series of chunks.
pub struct ChunkedSender {
    config: ChunkConfig,
    tx: mpsc::Sender<StreamMessage>,
    stream_id: u32,
    stats: Arc<ChunkStats>,
}

/// Chunking statistics
#[derive(Debug, Default)]
pub struct ChunkStats {
    pub total_transfers: AtomicU64,
    pub completed_transfers: AtomicU64,
    pub failed_transfers: AtomicU64,
    pub chunks_sent: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub chunks_received: AtomicU64,
    pub bytes_received: AtomicU64,
}

impl ChunkedSender {
    /// Create a new chunked sender
    pub fn new(config: ChunkConfig, tx: mpsc::Sender<StreamMessage>, stream_id: u32) -> Self {
        Self {
            config,
            tx,
            stream_id,
            stats: Arc::new(ChunkStats::default()),
        }
    }

    /// Send a large payload as chunks
    pub async fn send(&self, data: Bytes) -> StreamResult<Uuid> {
        let transfer_id = Uuid::new_v4();
        let total_size = data.len();
        let chunk_count = total_size.div_ceil(self.config.max_chunk_size);

        self.stats.total_transfers.fetch_add(1, Ordering::Relaxed);

        for (index, chunk_start) in (0..total_size)
            .step_by(self.config.max_chunk_size)
            .enumerate()
        {
            let chunk_end = (chunk_start + self.config.max_chunk_size).min(total_size);
            let chunk_data = data.slice(chunk_start..chunk_end);

            // Optionally compress
            let (final_data, compressed) = if self.config.compress_chunks
                && chunk_data.len() > self.config.compression_threshold
            {
                match compress_chunk(&chunk_data) {
                    Ok(compressed_bytes) if compressed_bytes.len() < chunk_data.len() => {
                        (compressed_bytes.to_vec(), true)
                    }
                    _ => (chunk_data.to_vec(), false),
                }
            } else {
                (chunk_data.to_vec(), false)
            };

            let is_last = index == chunk_count - 1;

            let meta = ChunkMeta {
                transfer_id,
                index: index as u32,
                total_chunks: chunk_count as u32,
                total_size: total_size as u64,
                is_last,
                checksum: Some(crc32fast::hash(&final_data)),
                compressed,
            };

            let final_len = final_data.len();
            let msg = StreamMessage {
                id: Uuid::new_v4(),
                stream_id: self.stream_id,
                sequence: index as u64,
                payload: StreamPayload::Chunk {
                    meta,
                    data: final_data,
                },
                priority: 4, // Normal priority
                timestamp_ns: crate::timestamp_now(),
                correlation_id: Some(transfer_id),
            };

            self.tx
                .send(msg)
                .await
                .map_err(|_| StreamError::ChannelSendError)?;

            self.stats.chunks_sent.fetch_add(1, Ordering::Relaxed);
            self.stats
                .bytes_sent
                .fetch_add(final_len as u64, Ordering::Relaxed);
        }

        self.stats
            .completed_transfers
            .fetch_add(1, Ordering::Relaxed);
        Ok(transfer_id)
    }

    /// Send with progress callback
    pub async fn send_with_progress<F>(&self, data: Bytes, mut on_progress: F) -> StreamResult<Uuid>
    where
        F: FnMut(usize, usize), // (bytes_sent, total_bytes)
    {
        let transfer_id = Uuid::new_v4();
        let total_size = data.len();
        let chunk_count = total_size.div_ceil(self.config.max_chunk_size);

        let mut bytes_sent = 0;

        for (index, chunk_start) in (0..total_size)
            .step_by(self.config.max_chunk_size)
            .enumerate()
        {
            let chunk_end = (chunk_start + self.config.max_chunk_size).min(total_size);
            let chunk_data = data.slice(chunk_start..chunk_end);
            let chunk_len = chunk_data.len();

            let is_last = index == chunk_count - 1;

            let meta = ChunkMeta {
                transfer_id,
                index: index as u32,
                total_chunks: chunk_count as u32,
                total_size: total_size as u64,
                is_last,
                checksum: Some(crc32fast::hash(&chunk_data)),
                compressed: false,
            };

            let msg = StreamMessage {
                id: Uuid::new_v4(),
                stream_id: self.stream_id,
                sequence: index as u64,
                payload: StreamPayload::Chunk {
                    meta,
                    data: chunk_data.to_vec(),
                },
                priority: 4,
                timestamp_ns: crate::timestamp_now(),
                correlation_id: Some(transfer_id),
            };

            self.tx
                .send(msg)
                .await
                .map_err(|_| StreamError::ChannelSendError)?;

            bytes_sent += chunk_len;
            on_progress(bytes_sent, total_size);
        }

        Ok(transfer_id)
    }

    /// Get statistics
    pub fn stats(&self) -> &ChunkStats {
        &self.stats
    }
}

// =============================================================================
// CHUNKED RECEIVER
// =============================================================================

/// Receives and reassembles chunked payloads.
pub struct ChunkedReceiver {
    config: ChunkConfig,
    transfers: Arc<RwLock<HashMap<Uuid, TransferState>>>,
    stats: Arc<ChunkStats>,
}

struct TransferState {
    chunks: HashMap<u32, Bytes>,
    total_chunks: u32,
    total_size: u64,
    received_size: u64,
    started_at: std::time::Instant,
}

impl ChunkedReceiver {
    /// Create a new chunked receiver
    pub fn new(config: ChunkConfig) -> Self {
        Self {
            config,
            transfers: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(ChunkStats::default()),
        }
    }

    /// Process an incoming chunk message
    pub fn process_chunk(&self, meta: ChunkMeta, data: Bytes) -> StreamResult<Option<Bytes>> {
        self.stats.chunks_received.fetch_add(1, Ordering::Relaxed);
        self.stats
            .bytes_received
            .fetch_add(data.len() as u64, Ordering::Relaxed);

        // Verify checksum if present
        if let Some(expected_checksum) = meta.checksum {
            let actual_checksum = crc32fast::hash(&data);
            if actual_checksum != expected_checksum {
                return Err(StreamError::ChecksumMismatch {
                    expected: expected_checksum,
                    actual: actual_checksum,
                });
            }
        }

        // Decompress if needed
        let decompressed = if meta.compressed {
            decompress_chunk(&data)?
        } else {
            data
        };

        let mut transfers = self.transfers.write();

        // Create or get transfer state
        let state = transfers.entry(meta.transfer_id).or_insert_with(|| {
            self.stats.total_transfers.fetch_add(1, Ordering::Relaxed);
            TransferState {
                chunks: HashMap::new(),
                total_chunks: meta.total_chunks,
                total_size: meta.total_size,
                received_size: 0,
                started_at: std::time::Instant::now(),
            }
        });

        // Check for timeout
        if state.started_at.elapsed() > self.config.transfer_timeout {
            transfers.remove(&meta.transfer_id);
            self.stats.failed_transfers.fetch_add(1, Ordering::Relaxed);
            return Err(StreamError::TransferTimeout);
        }

        // Store chunk
        state.chunks.insert(meta.index, decompressed.clone());
        state.received_size += decompressed.len() as u64;

        // Check if complete
        if state.chunks.len() as u32 == state.total_chunks {
            // Reassemble
            let mut result = BytesMut::with_capacity(state.total_size as usize);
            for i in 0..state.total_chunks {
                if let Some(chunk) = state.chunks.get(&i) {
                    result.extend_from_slice(chunk);
                } else {
                    return Err(StreamError::MissingChunk {
                        transfer_id: meta.transfer_id,
                        index: i,
                    });
                }
            }

            transfers.remove(&meta.transfer_id);
            self.stats
                .completed_transfers
                .fetch_add(1, Ordering::Relaxed);

            Ok(Some(result.freeze()))
        } else {
            Ok(None) // More chunks needed
        }
    }

    /// Get transfer progress
    pub fn transfer_progress(&self, transfer_id: Uuid) -> Option<(u64, u64)> {
        self.transfers
            .read()
            .get(&transfer_id)
            .map(|s| (s.received_size, s.total_size))
    }

    /// Cancel a transfer
    pub fn cancel_transfer(&self, transfer_id: Uuid) {
        self.transfers.write().remove(&transfer_id);
    }

    /// Clean up stale transfers
    pub fn cleanup_stale(&self) {
        let mut transfers = self.transfers.write();
        let timeout = self.config.transfer_timeout;

        transfers.retain(|_, state| {
            let stale = state.started_at.elapsed() > timeout;
            if stale {
                self.stats.failed_transfers.fetch_add(1, Ordering::Relaxed);
            }
            !stale
        });
    }

    /// Get statistics
    pub fn stats(&self) -> &ChunkStats {
        &self.stats
    }
}

// =============================================================================
// STREAMING CHUNKER
// =============================================================================

/// Streaming interface for sending chunks as they're generated.
pub struct StreamingChunker {
    config: ChunkConfig,
    tx: mpsc::Sender<StreamMessage>,
    stream_id: u32,
    transfer_id: Uuid,
    index: u32,
    buffer: BytesMut,
    max_chunk_size: usize,
}

impl StreamingChunker {
    /// Start a new streaming transfer
    pub fn new(config: ChunkConfig, tx: mpsc::Sender<StreamMessage>, stream_id: u32) -> Self {
        let max_chunk_size = config.max_chunk_size;
        Self {
            config,
            tx,
            stream_id,
            transfer_id: Uuid::new_v4(),
            index: 0,
            buffer: BytesMut::with_capacity(max_chunk_size),
            max_chunk_size,
        }
    }

    /// Write data (will be chunked automatically)
    pub async fn write(&mut self, data: &[u8]) -> StreamResult<()> {
        self.buffer.extend_from_slice(data);

        // Send complete chunks
        while self.buffer.len() >= self.max_chunk_size {
            let chunk_data = self.buffer.split_to(self.max_chunk_size).freeze();
            self.send_chunk(chunk_data, false).await?;
        }

        Ok(())
    }

    /// Finish the transfer (sends remaining data)
    pub async fn finish(mut self) -> StreamResult<Uuid> {
        // Send final chunk with remaining data
        if !self.buffer.is_empty() {
            let final_data = std::mem::take(&mut self.buffer).freeze();
            self.send_chunk(final_data, true).await?;
        } else {
            // Send empty final chunk to signal completion
            self.send_chunk(Bytes::new(), true).await?;
        }

        Ok(self.transfer_id)
    }

    async fn send_chunk(&mut self, data: Bytes, is_last: bool) -> StreamResult<()> {
        let meta = ChunkMeta {
            transfer_id: self.transfer_id,
            index: self.index,
            total_chunks: 0, // Unknown in streaming mode
            total_size: 0,   // Unknown in streaming mode
            is_last,
            checksum: Some(crc32fast::hash(&data)),
            compressed: false,
        };

        let msg = StreamMessage {
            id: Uuid::new_v4(),
            stream_id: self.stream_id,
            sequence: self.index as u64,
            payload: StreamPayload::Chunk {
                meta,
                data: data.to_vec(),
            },
            priority: 4,
            timestamp_ns: crate::timestamp_now(),
            correlation_id: Some(self.transfer_id),
        };

        self.tx
            .send(msg)
            .await
            .map_err(|_| StreamError::ChannelSendError)?;

        self.index += 1;
        Ok(())
    }

    /// Get the transfer ID
    pub fn transfer_id(&self) -> Uuid {
        self.transfer_id
    }
}

// =============================================================================
// STREAMING DECHUNKER
// =============================================================================

/// Streaming interface for receiving chunks as they arrive.
pub struct StreamingDechunker {
    config: ChunkConfig,
    pending: HashMap<Uuid, StreamingTransfer>,
    output_tx: mpsc::Sender<(Uuid, Bytes)>,
}

struct StreamingTransfer {
    next_index: u32,
    buffer: HashMap<u32, Bytes>,
    started_at: std::time::Instant,
}

impl StreamingDechunker {
    /// Create a new streaming dechunker
    pub fn new(config: ChunkConfig) -> (Self, mpsc::Receiver<(Uuid, Bytes)>) {
        let (output_tx, output_rx) = mpsc::channel(256);

        (
            Self {
                config,
                pending: HashMap::new(),
                output_tx,
            },
            output_rx,
        )
    }

    /// Process an incoming chunk
    pub async fn process(&mut self, meta: ChunkMeta, data: Bytes) -> StreamResult<bool> {
        // Verify checksum
        if let Some(expected) = meta.checksum {
            let actual = crc32fast::hash(&data);
            if actual != expected {
                return Err(StreamError::ChecksumMismatch { expected, actual });
            }
        }

        // Get or create transfer
        let transfer = self
            .pending
            .entry(meta.transfer_id)
            .or_insert_with(|| StreamingTransfer {
                next_index: 0,
                buffer: HashMap::new(),
                started_at: std::time::Instant::now(),
            });

        // Check timeout
        if transfer.started_at.elapsed() > self.config.transfer_timeout {
            self.pending.remove(&meta.transfer_id);
            return Err(StreamError::TransferTimeout);
        }

        // Store chunk
        transfer.buffer.insert(meta.index, data);

        // Output in-order chunks
        while let Some(chunk) = transfer.buffer.remove(&transfer.next_index) {
            self.output_tx
                .send((meta.transfer_id, chunk))
                .await
                .map_err(|_| StreamError::ChannelSendError)?;
            transfer.next_index += 1;
        }

        // Check if complete
        if meta.is_last && transfer.buffer.is_empty() {
            self.pending.remove(&meta.transfer_id);
            return Ok(true);
        }

        Ok(false)
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

fn compress_chunk(data: &[u8]) -> StreamResult<Bytes> {
    let compressed =
        zstd::encode_all(data, 3).map_err(|e| StreamError::CompressionError(e.to_string()))?;
    Ok(Bytes::from(compressed))
}

fn decompress_chunk(data: &[u8]) -> StreamResult<Bytes> {
    let decompressed =
        zstd::decode_all(data).map_err(|e| StreamError::DecompressionError(e.to_string()))?;
    Ok(Bytes::from(decompressed))
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chunk_send_receive() {
        let (tx, mut rx) = mpsc::channel(100);
        let config = ChunkConfig {
            max_chunk_size: 10,
            compress_chunks: false,
            ..Default::default()
        };

        let sender = ChunkedSender::new(config.clone(), tx, 1);
        let receiver = ChunkedReceiver::new(config);

        // Send 25 bytes -> 3 chunks
        let data = Bytes::from("Hello, this is test data!");
        let _transfer_id = sender.send(data.clone()).await.unwrap();

        // Receive and reassemble
        let mut result = None;
        while let Some(msg) = rx.recv().await {
            if let StreamPayload::Chunk { meta, data } = msg.payload {
                result = receiver.process_chunk(meta, Bytes::from(data)).unwrap();
                if result.is_some() {
                    break;
                }
            }
        }

        assert_eq!(result.unwrap(), data);
    }

    #[tokio::test]
    async fn test_streaming_chunker() {
        let (tx, mut rx) = mpsc::channel(100);
        let config = ChunkConfig {
            max_chunk_size: 5,
            compress_chunks: false,
            ..Default::default()
        };

        let mut chunker = StreamingChunker::new(config, tx, 1);

        chunker.write(b"Hello").await.unwrap();
        chunker.write(b"World").await.unwrap();
        let _transfer_id = chunker.finish().await.unwrap();

        // Should have received 3 chunks: "Hello", "World", and empty final
        let mut count = 0;
        while let Ok(_msg) = rx.try_recv() {
            count += 1;
        }
        assert!(count >= 2);
    }

    #[test]
    fn test_compression() {
        let data = b"This is some test data that should compress well. ".repeat(10);
        let compressed = compress_chunk(&data).unwrap();
        let decompressed = decompress_chunk(&compressed).unwrap();

        assert_eq!(&data[..], &decompressed[..]);
        assert!(compressed.len() < data.len()); // Should be smaller
    }

    #[tokio::test]
    async fn test_checksum_verification() {
        let config = ChunkConfig::default();
        let receiver = ChunkedReceiver::new(config);

        let meta = ChunkMeta {
            transfer_id: Uuid::new_v4(),
            index: 0,
            total_chunks: 1,
            total_size: 5,
            is_last: true,
            checksum: Some(12345), // Wrong checksum
            compressed: false,
        };

        let result = receiver.process_chunk(meta, Bytes::from("hello"));
        assert!(matches!(result, Err(StreamError::ChecksumMismatch { .. })));
    }
}
