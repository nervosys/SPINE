//! Latent vector streaming for neural embeddings.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{StreamError, StreamMessage, StreamPayload, StreamResult};

// =============================================================================
// LATENT VECTOR TYPES
// =============================================================================

/// A latent vector (neural embedding).
#[derive(Clone, Debug)]
pub struct LatentVector {
    /// Unique identifier
    pub id: Uuid,
    /// Vector dimensions
    pub dimensions: u32,
    /// Vector data (f32 values)
    pub data: Vec<f32>,
    /// Optional metadata
    pub metadata: Option<LatentMetadata>,
}

/// Metadata for latent vectors.
#[derive(Clone, Debug, Default)]
pub struct LatentMetadata {
    /// Source model identifier
    pub model_id: Option<String>,
    /// Layer from which embedding was extracted
    pub layer: Option<u32>,
    /// Quantization level (bits)
    pub quantization: Option<u8>,
    /// Timestamp when generated
    pub timestamp: Option<u64>,
    /// Custom tags
    pub tags: Vec<String>,
}

/// A batch of latent vectors for efficient transmission.
#[derive(Clone, Debug)]
pub struct LatentBatch {
    /// Batch identifier
    pub id: Uuid,
    /// Number of vectors in batch
    pub count: usize,
    /// Common dimensions (all vectors must have same dimensions)
    pub dimensions: u32,
    /// Packed vector data
    pub data: Vec<f32>,
    /// Optional per-vector metadata
    pub metadata: Vec<LatentMetadata>,
}

impl LatentVector {
    /// Create a new latent vector
    pub fn new(dimensions: u32, data: Vec<f32>) -> Self {
        assert_eq!(data.len(), dimensions as usize);
        Self {
            id: Uuid::new_v4(),
            dimensions,
            data,
            metadata: None,
        }
    }

    /// Create with metadata
    pub fn with_metadata(dimensions: u32, data: Vec<f32>, metadata: LatentMetadata) -> Self {
        let mut v = Self::new(dimensions, data);
        v.metadata = Some(metadata);
        v
    }

    /// Compute L2 norm
    pub fn l2_norm(&self) -> f32 {
        self.data.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    /// Normalize to unit vector
    pub fn normalize(&mut self) {
        let norm = self.l2_norm();
        if norm > 0.0 {
            for x in &mut self.data {
                *x /= norm;
            }
        }
    }

    /// Compute cosine similarity with another vector
    #[inline]
    pub fn cosine_similarity(&self, other: &LatentVector) -> f32 {
        assert_eq!(self.dimensions, other.dimensions);

        // Single-pass computation of dot product and norms
        let (dot, norm_a_sq, norm_b_sq) = self
            .data
            .iter()
            .zip(other.data.iter())
            .fold((0.0f32, 0.0f32, 0.0f32), |(d, na, nb), (&a, &b)| {
                (d + a * b, na + a * a, nb + b * b)
            });

        let denom = (norm_a_sq * norm_b_sq).sqrt();
        if denom > 0.0 {
            dot / denom
        } else {
            0.0
        }
    }

    /// Compute Euclidean distance to another vector
    pub fn euclidean_distance(&self, other: &LatentVector) -> f32 {
        assert_eq!(self.dimensions, other.dimensions);

        self.data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Serialize to bytes
    #[inline]
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(4 + self.data.len() * 4);
        buf.put_u32_le(self.dimensions);
        // Bulk copy using unsafe transmute for zero-cost serialization
        // Safety: f32 and [u8; 4] have same size and alignment is handled by put_slice
        let slice = unsafe {
            std::slice::from_raw_parts(self.data.as_ptr() as *const u8, self.data.len() * 4)
        };
        buf.put_slice(slice);
        buf.freeze()
    }

    /// Deserialize from bytes
    #[inline]
    pub fn from_bytes(mut data: Bytes) -> StreamResult<Self> {
        if data.len() < 4 {
            return Err(StreamError::InvalidMessage(
                "latent vector too short".into(),
            ));
        }

        let dimensions = data.get_u32_le();
        let expected_len = dimensions as usize * 4;

        if data.len() < expected_len {
            return Err(StreamError::InvalidMessage(
                "latent vector data truncated".into(),
            ));
        }

        // Bulk copy using unsafe transmute for zero-cost deserialization
        let mut vector_data = vec![0.0f32; dimensions as usize];
        // Safety: f32 and [u8; 4] have same size
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                vector_data.as_mut_ptr() as *mut u8,
                expected_len,
            );
        }
        data.advance(expected_len);

        Ok(Self {
            id: Uuid::new_v4(),
            dimensions,
            data: vector_data,
            metadata: None,
        })
    }
}

impl LatentBatch {
    /// Create a new batch from vectors
    pub fn from_vectors(vectors: Vec<LatentVector>) -> StreamResult<Self> {
        if vectors.is_empty() {
            return Err(StreamError::InvalidMessage("empty batch".into()));
        }

        let dimensions = vectors[0].dimensions;

        // Verify all vectors have same dimensions
        for v in &vectors {
            if v.dimensions != dimensions {
                return Err(StreamError::InvalidMessage(
                    "mismatched dimensions in batch".into(),
                ));
            }
        }

        let count = vectors.len();
        let mut data = Vec::with_capacity(count * dimensions as usize);
        let mut metadata = Vec::with_capacity(count);

        for v in vectors {
            data.extend(v.data);
            metadata.push(v.metadata.unwrap_or_default());
        }

        Ok(Self {
            id: Uuid::new_v4(),
            count,
            dimensions,
            data,
            metadata,
        })
    }

    /// Get a vector from the batch by index
    pub fn get(&self, index: usize) -> Option<LatentVector> {
        if index >= self.count {
            return None;
        }

        let start = index * self.dimensions as usize;
        let end = start + self.dimensions as usize;

        Some(LatentVector {
            id: Uuid::new_v4(),
            dimensions: self.dimensions,
            data: self.data[start..end].to_vec(),
            metadata: self.metadata.get(index).cloned(),
        })
    }

    /// Iterate over vectors in the batch
    pub fn iter(&self) -> impl Iterator<Item = LatentVector> + '_ {
        (0..self.count).filter_map(move |i| self.get(i))
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(8 + self.data.len() * 4);
        buf.put_u32_le(self.count as u32);
        buf.put_u32_le(self.dimensions);
        for &v in &self.data {
            buf.put_f32_le(v);
        }
        buf.freeze()
    }

    /// Deserialize from bytes
    pub fn from_bytes(mut data: Bytes) -> StreamResult<Self> {
        if data.len() < 8 {
            return Err(StreamError::InvalidMessage("batch header too short".into()));
        }

        let count = data.get_u32_le() as usize;
        let dimensions = data.get_u32_le();
        let expected_len = count * dimensions as usize * 4;

        if data.len() < expected_len {
            return Err(StreamError::InvalidMessage("batch data truncated".into()));
        }

        let mut vector_data = Vec::with_capacity(count * dimensions as usize);
        for _ in 0..(count * dimensions as usize) {
            vector_data.push(data.get_f32_le());
        }

        Ok(Self {
            id: Uuid::new_v4(),
            count,
            dimensions,
            data: vector_data,
            metadata: vec![LatentMetadata::default(); count],
        })
    }
}

// =============================================================================
// LATENT STREAM
// =============================================================================

/// Statistics for latent streaming.
#[derive(Debug, Default)]
pub struct LatentStats {
    pub vectors_sent: AtomicU64,
    pub vectors_received: AtomicU64,
    pub batches_sent: AtomicU64,
    pub batches_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
}

/// Streams latent vectors efficiently.
pub struct LatentStreamer {
    tx: mpsc::Sender<StreamMessage>,
    stream_id: u32,
    sequence: AtomicU64,
    stats: Arc<LatentStats>,
    batch_size: usize,
    batch_buffer: parking_lot::Mutex<Vec<LatentVector>>,
}

impl LatentStreamer {
    /// Create a new latent streamer
    pub fn new(tx: mpsc::Sender<StreamMessage>, stream_id: u32) -> Self {
        Self {
            tx,
            stream_id,
            sequence: AtomicU64::new(0),
            stats: Arc::new(LatentStats::default()),
            batch_size: 32, // Default batch size
            batch_buffer: parking_lot::Mutex::new(Vec::with_capacity(32)),
        }
    }

    /// Set batch size
    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self.batch_buffer = parking_lot::Mutex::new(Vec::with_capacity(size));
        self
    }

    /// Send a single latent vector
    pub async fn send(&self, vector: LatentVector) -> StreamResult<()> {
        let dimensions = vector.dimensions;
        let data = vector.data.clone();

        let msg = StreamMessage {
            id: Uuid::new_v4(),
            stream_id: self.stream_id,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            payload: StreamPayload::LatentVector {
                dimensions,
                vector: data.clone(),
            },
            priority: 4,
            timestamp_ns: crate::timestamp_now(),
            correlation_id: None,
        };

        self.tx
            .send(msg)
            .await
            .map_err(|_| StreamError::ChannelSendError)?;

        self.stats.vectors_sent.fetch_add(1, Ordering::Relaxed);
        self.stats
            .bytes_sent
            .fetch_add((dimensions as u64) * 4, Ordering::Relaxed);

        Ok(())
    }

    /// Buffer a vector for batching
    pub fn buffer(&self, vector: LatentVector) {
        let mut buffer = self.batch_buffer.lock();
        buffer.push(vector);
    }

    /// Flush buffered vectors as a batch
    pub async fn flush(&self) -> StreamResult<()> {
        let vectors: Vec<_> = {
            let mut buffer = self.batch_buffer.lock();
            std::mem::take(&mut *buffer)
        };

        if vectors.is_empty() {
            return Ok(());
        }

        self.send_batch(vectors).await
    }

    /// Buffer and auto-flush when batch is full
    pub async fn buffer_auto_flush(&self, vector: LatentVector) -> StreamResult<()> {
        let should_flush = {
            let mut buffer = self.batch_buffer.lock();
            buffer.push(vector);
            buffer.len() >= self.batch_size
        };

        if should_flush {
            self.flush().await?;
        }

        Ok(())
    }

    /// Send a batch of vectors
    pub async fn send_batch(&self, vectors: Vec<LatentVector>) -> StreamResult<()> {
        let batch = LatentBatch::from_vectors(vectors)?;

        let msg = StreamMessage {
            id: Uuid::new_v4(),
            stream_id: self.stream_id,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            payload: StreamPayload::LatentBatch {
                count: batch.count as u32,
                dimensions: batch.dimensions,
                vectors: batch.data.clone(),
            },
            priority: 4,
            timestamp_ns: crate::timestamp_now(),
            correlation_id: None,
        };

        let bytes = (batch.count * batch.dimensions as usize * 4) as u64;

        self.tx
            .send(msg)
            .await
            .map_err(|_| StreamError::ChannelSendError)?;

        self.stats.batches_sent.fetch_add(1, Ordering::Relaxed);
        self.stats
            .vectors_sent
            .fetch_add(batch.count as u64, Ordering::Relaxed);
        self.stats.bytes_sent.fetch_add(bytes, Ordering::Relaxed);

        Ok(())
    }

    /// Get statistics
    pub fn stats(&self) -> &LatentStats {
        &self.stats
    }
}

// =============================================================================
// LATENT RECEIVER
// =============================================================================

/// Receives and processes latent vectors.
pub struct LatentReceiver {
    stats: Arc<LatentStats>,
}

impl LatentReceiver {
    /// Create a new latent receiver
    pub fn new() -> Self {
        Self {
            stats: Arc::new(LatentStats::default()),
        }
    }

    /// Process a received message
    pub fn process(&self, payload: StreamPayload) -> StreamResult<Vec<LatentVector>> {
        match payload {
            StreamPayload::LatentVector { dimensions, vector } => {
                self.stats.vectors_received.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .bytes_received
                    .fetch_add((dimensions as u64) * 4, Ordering::Relaxed);

                Ok(vec![LatentVector::new(dimensions, vector)])
            }
            StreamPayload::LatentBatch {
                count,
                dimensions,
                vectors,
            } => {
                self.stats.batches_received.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .vectors_received
                    .fetch_add(count as u64, Ordering::Relaxed);
                self.stats
                    .bytes_received
                    .fetch_add((count as u64) * (dimensions as u64) * 4, Ordering::Relaxed);

                let batch = LatentBatch {
                    id: Uuid::new_v4(),
                    count: count as usize,
                    dimensions,
                    data: vectors,
                    metadata: vec![LatentMetadata::default(); count as usize],
                };

                Ok(batch.iter().collect())
            }
            _ => Err(StreamError::InvalidMessage(
                "expected latent payload".into(),
            )),
        }
    }

    /// Get statistics
    pub fn stats(&self) -> &LatentStats {
        &self.stats
    }
}

impl Default for LatentReceiver {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// QUANTIZATION
// =============================================================================

/// Quantize f32 vectors to reduce bandwidth.
pub struct Quantizer {
    bits: u8,
}

impl Quantizer {
    /// Create a new quantizer
    pub fn new(bits: u8) -> Self {
        assert!((1..=16).contains(&bits));
        Self { bits }
    }

    /// Quantize a vector to u8 values
    pub fn quantize_u8(&self, vector: &[f32]) -> (Vec<u8>, f32, f32) {
        let (min, max) = vector.iter().fold((f32::MAX, f32::MIN), |(min, max), &v| {
            (min.min(v), max.max(v))
        });

        let range = max - min;
        let scale = if range > 0.0 { 255.0 / range } else { 1.0 };

        let quantized: Vec<u8> = vector
            .iter()
            .map(|&v| ((v - min) * scale).round() as u8)
            .collect();

        (quantized, min, max)
    }

    /// Dequantize u8 values back to f32
    pub fn dequantize_u8(&self, quantized: &[u8], min: f32, max: f32) -> Vec<f32> {
        let range = max - min;
        let scale = range / 255.0;

        quantized
            .iter()
            .map(|&v| min + (v as f32) * scale)
            .collect()
    }

    /// Quantize to u16 for higher precision
    pub fn quantize_u16(&self, vector: &[f32]) -> (Vec<u16>, f32, f32) {
        let (min, max) = vector.iter().fold((f32::MAX, f32::MIN), |(min, max), &v| {
            (min.min(v), max.max(v))
        });

        let range = max - min;
        let scale = if range > 0.0 { 65535.0 / range } else { 1.0 };

        let quantized: Vec<u16> = vector
            .iter()
            .map(|&v| ((v - min) * scale).round() as u16)
            .collect();

        (quantized, min, max)
    }

    /// Dequantize u16 values back to f32
    pub fn dequantize_u16(&self, quantized: &[u16], min: f32, max: f32) -> Vec<f32> {
        let range = max - min;
        let scale = range / 65535.0;

        quantized
            .iter()
            .map(|&v| min + (v as f32) * scale)
            .collect()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latent_vector_operations() {
        let v1 = LatentVector::new(3, vec![1.0, 0.0, 0.0]);
        let v2 = LatentVector::new(3, vec![0.0, 1.0, 0.0]);

        // Orthogonal vectors should have 0 cosine similarity
        let sim = v1.cosine_similarity(&v2);
        assert!((sim - 0.0).abs() < 0.001);

        // Same vector should have 1.0 similarity
        let sim = v1.cosine_similarity(&v1);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_latent_vector_serialization() {
        let original = LatentVector::new(4, vec![1.0, 2.0, 3.0, 4.0]);
        let bytes = original.to_bytes();
        let restored = LatentVector::from_bytes(bytes).unwrap();

        assert_eq!(original.dimensions, restored.dimensions);
        assert_eq!(original.data, restored.data);
    }

    #[test]
    fn test_latent_batch() {
        let vectors = vec![
            LatentVector::new(3, vec![1.0, 2.0, 3.0]),
            LatentVector::new(3, vec![4.0, 5.0, 6.0]),
            LatentVector::new(3, vec![7.0, 8.0, 9.0]),
        ];

        let batch = LatentBatch::from_vectors(vectors).unwrap();
        assert_eq!(batch.count, 3);
        assert_eq!(batch.dimensions, 3);

        let v0 = batch.get(0).unwrap();
        assert_eq!(v0.data, vec![1.0, 2.0, 3.0]);

        let v2 = batch.get(2).unwrap();
        assert_eq!(v2.data, vec![7.0, 8.0, 9.0]);
    }

    #[test]
    fn test_quantization() {
        let quantizer = Quantizer::new(8);
        let original = vec![-1.0, 0.0, 0.5, 1.0];

        let (quantized, min, max) = quantizer.quantize_u8(&original);
        let restored = quantizer.dequantize_u8(&quantized, min, max);

        // Should be close to original
        for (o, r) in original.iter().zip(restored.iter()) {
            assert!((o - r).abs() < 0.01);
        }
    }

    #[test]
    fn test_normalize() {
        let mut v = LatentVector::new(3, vec![3.0, 4.0, 0.0]);
        v.normalize();

        // L2 norm should be 1
        let norm = v.l2_norm();
        assert!((norm - 1.0).abs() < 0.001);
    }
}
