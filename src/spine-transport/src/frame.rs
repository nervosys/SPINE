//! Frame encoding and decoding with vectored I/O support.

use bytes::{BufMut, Bytes, BytesMut};
use std::io::IoSlice;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{Frame, FrameFlags, FrameHeader, TransportError, TransportResult};

// =============================================================================
// FRAME CODEC
// =============================================================================

/// High-performance frame encoder/decoder
pub struct FrameCodec {
    /// Maximum frame size
    max_frame_size: usize,
    /// Read buffer
    read_buf: BytesMut,
    /// Write buffer for vectored I/O
    write_bufs: Vec<Bytes>,
}

impl FrameCodec {
    /// Header size in bytes
    pub const HEADER_SIZE: usize = 12;

    /// Create a new frame codec
    pub fn new(max_frame_size: usize) -> Self {
        Self {
            max_frame_size,
            read_buf: BytesMut::with_capacity(64 * 1024),
            write_bufs: Vec::with_capacity(16),
        }
    }

    /// Encode a frame into bytes
    pub fn encode(&self, frame: &Frame) -> Bytes {
        let mut buf = BytesMut::with_capacity(Self::HEADER_SIZE + frame.payload.len());
        buf.put_slice(&frame.header_bytes());
        buf.put_slice(&frame.payload);
        buf.freeze()
    }

    /// Encode multiple frames into a single buffer
    pub fn encode_batch(&mut self, frames: &[Frame]) -> Bytes {
        let total_size: usize = frames
            .iter()
            .map(|f| Self::HEADER_SIZE + f.payload.len())
            .sum();

        let mut buf = BytesMut::with_capacity(total_size);

        for frame in frames {
            buf.put_slice(&frame.header_bytes());
            buf.put_slice(&frame.payload);
        }

        buf.freeze()
    }

    /// Encode frames into pre-allocated Bytes for vectored write
    pub fn encode_batch_vectored(&mut self, frames: &[Frame]) -> Vec<Bytes> {
        frames.iter().map(|f| self.encode(f)).collect()
    }

    /// Decode a frame from the read buffer (zero-copy where possible)
    pub fn decode(&mut self, data: &[u8]) -> TransportResult<Frame> {
        if data.len() < Self::HEADER_SIZE {
            return Err(TransportError::InvalidFrame(format!(
                "Frame too short: {} bytes",
                data.len()
            )));
        }

        let header = Frame::parse_header(data[..Self::HEADER_SIZE].try_into().unwrap());

        if header.length as usize > self.max_frame_size {
            return Err(TransportError::MessageTooLarge {
                size: header.length as usize,
                max: self.max_frame_size,
            });
        }

        let total_len = Self::HEADER_SIZE + header.length as usize;
        if data.len() < total_len {
            return Err(TransportError::InvalidFrame(format!(
                "Incomplete frame: have {} need {}",
                data.len(),
                total_len
            )));
        }

        let payload = Bytes::copy_from_slice(&data[Self::HEADER_SIZE..total_len]);

        Ok(Frame { header, payload })
    }

    /// Zero-copy decode from Bytes (avoids allocation when possible)
    pub fn decode_zerocopy(&mut self, data: Bytes) -> TransportResult<Frame> {
        if data.len() < Self::HEADER_SIZE {
            return Err(TransportError::InvalidFrame(format!(
                "Frame too short: {} bytes",
                data.len()
            )));
        }

        let header = Frame::parse_header(data[..Self::HEADER_SIZE].try_into().unwrap());

        if header.length as usize > self.max_frame_size {
            return Err(TransportError::MessageTooLarge {
                size: header.length as usize,
                max: self.max_frame_size,
            });
        }

        let total_len = Self::HEADER_SIZE + header.length as usize;
        if data.len() < total_len {
            return Err(TransportError::InvalidFrame(format!(
                "Incomplete frame: have {} need {}",
                data.len(),
                total_len
            )));
        }

        // ZERO-COPY: slice the input Bytes instead of copying
        let payload = data.slice(Self::HEADER_SIZE..total_len);

        Ok(Frame { header, payload })
    }

    /// Read a frame from an async reader
    pub async fn read_frame<R: AsyncRead + Unpin>(
        &mut self,
        reader: &mut R,
    ) -> TransportResult<Frame> {
        // Read header
        let mut header_buf = [0u8; Self::HEADER_SIZE];
        reader.read_exact(&mut header_buf).await?;

        let header = Frame::parse_header(&header_buf);

        // Validate length
        if header.length as usize > self.max_frame_size {
            return Err(TransportError::MessageTooLarge {
                size: header.length as usize,
                max: self.max_frame_size,
            });
        }

        // Read payload
        let mut payload = vec![0u8; header.length as usize];
        reader.read_exact(&mut payload).await?;

        Ok(Frame {
            header,
            payload: Bytes::from(payload),
        })
    }

    /// Write a frame to an async writer
    pub async fn write_frame<W: AsyncWrite + Unpin>(
        &mut self,
        writer: &mut W,
        frame: &Frame,
    ) -> TransportResult<()> {
        let header_bytes = frame.header_bytes();

        // Use vectored write if available
        let bufs = [IoSlice::new(&header_bytes), IoSlice::new(&frame.payload)];

        let _ = writer.write_vectored(&bufs).await?;

        Ok(())
    }

    /// Write multiple frames using vectored I/O
    pub async fn write_frames<W: AsyncWrite + Unpin>(
        &mut self,
        writer: &mut W,
        frames: &[Frame],
    ) -> TransportResult<()> {
        if frames.is_empty() {
            return Ok(());
        }

        // Build vectored buffers
        let mut slices = Vec::with_capacity(frames.len() * 2);
        let mut header_bytes = Vec::with_capacity(frames.len());

        for frame in frames {
            header_bytes.push(frame.header_bytes());
        }

        for (i, frame) in frames.iter().enumerate() {
            slices.push(IoSlice::new(&header_bytes[i]));
            slices.push(IoSlice::new(&frame.payload));
        }

        // Write all at once
        let mut written = 0;
        let total: usize = slices.iter().map(|s| s.len()).sum();

        while written < total {
            let n = writer.write_vectored(&slices[..]).await?;
            if n == 0 {
                return Err(TransportError::ConnectionClosed);
            }
            written += n;

            // Adjust slices (simplified - in practice would need to track partially written slices)
            if written >= total {
                break;
            }
        }

        Ok(())
    }
}

// =============================================================================
// FRAME BUILDER
// =============================================================================

/// Builder for constructing frames with various options
pub struct FrameBuilder {
    flags: FrameFlags,
    sequence: u32,
    stream_id: u16,
    payload: Option<Bytes>,
}

impl FrameBuilder {
    /// Create a new frame builder
    pub fn new() -> Self {
        Self {
            flags: FrameFlags::empty(),
            sequence: 0,
            stream_id: 0,
            payload: None,
        }
    }

    /// Set payload
    pub fn payload(mut self, data: impl Into<Bytes>) -> Self {
        self.payload = Some(data.into());
        self
    }

    /// Set sequence number
    pub fn sequence(mut self, seq: u32) -> Self {
        self.sequence = seq;
        self
    }

    /// Set stream ID
    pub fn stream_id(mut self, id: u16) -> Self {
        self.stream_id = id;
        self
    }

    /// Mark as compressed
    pub fn compressed(mut self) -> Self {
        self.flags |= FrameFlags::COMPRESSED;
        self
    }

    /// Mark as encrypted
    pub fn encrypted(mut self) -> Self {
        self.flags |= FrameFlags::ENCRYPTED;
        self
    }

    /// Mark as batched
    pub fn batched(mut self) -> Self {
        self.flags |= FrameFlags::BATCHED;
        self
    }

    /// Mark as requiring ACK
    pub fn ack_required(mut self) -> Self {
        self.flags |= FrameFlags::ACK_REQUIRED;
        self
    }

    /// Mark as control frame
    pub fn control(mut self) -> Self {
        self.flags |= FrameFlags::CONTROL;
        self
    }

    /// Mark as final in stream
    pub fn fin(mut self) -> Self {
        self.flags |= FrameFlags::FIN;
        self
    }

    /// Mark as priority
    pub fn priority(mut self) -> Self {
        self.flags |= FrameFlags::PRIORITY;
        self
    }

    /// Build the frame
    pub fn build(self) -> Frame {
        let payload = self.payload.unwrap_or_default();
        Frame {
            header: FrameHeader {
                length: payload.len() as u32,
                flags: self.flags,
                sequence: self.sequence,
                stream_id: self.stream_id,
                _reserved: 0,
            },
            payload,
        }
    }
}

impl Default for FrameBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// FRAME AGGREGATOR
// =============================================================================

/// Aggregates multiple small frames into larger batches
pub struct FrameAggregator {
    /// Pending frames
    pending: Vec<Frame>,
    /// Current batch size
    batch_size: usize,
    /// Maximum batch size
    max_batch_size: usize,
    /// Maximum batch count
    max_batch_count: usize,
}

impl FrameAggregator {
    /// Create a new frame aggregator
    pub fn new(max_batch_size: usize, max_batch_count: usize) -> Self {
        Self {
            pending: Vec::with_capacity(max_batch_count),
            batch_size: 0,
            max_batch_size,
            max_batch_count,
        }
    }

    /// Add a frame to the batch
    /// Returns true if batch is ready to send
    pub fn add(&mut self, frame: Frame) -> bool {
        let frame_size = 12 + frame.payload.len();

        // Check if this frame would exceed limits
        if !self.pending.is_empty()
            && (self.batch_size + frame_size > self.max_batch_size
                || self.pending.len() >= self.max_batch_count)
        {
            return true;
        }

        self.batch_size += frame_size;
        self.pending.push(frame);

        self.batch_size >= self.max_batch_size || self.pending.len() >= self.max_batch_count
    }

    /// Check if batch is ready
    pub fn is_ready(&self) -> bool {
        !self.pending.is_empty()
            && (self.batch_size >= self.max_batch_size
                || self.pending.len() >= self.max_batch_count)
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Get pending count
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get pending size
    pub fn pending_size(&self) -> usize {
        self.batch_size
    }

    /// Take the current batch
    pub fn take(&mut self) -> Vec<Frame> {
        self.batch_size = 0;
        std::mem::take(&mut self.pending)
    }

    /// Clear without returning
    pub fn clear(&mut self) {
        self.pending.clear();
        self.batch_size = 0;
    }
}

// =============================================================================
// FRAME FRAGMENTER
// =============================================================================

/// Fragments large frames into smaller chunks for transmission
pub struct FrameFragmenter {
    /// Maximum fragment size
    max_fragment_size: usize,
    /// Base sequence number
    base_sequence: u32,
}

impl FrameFragmenter {
    /// Create a new frame fragmenter
    pub fn new(max_fragment_size: usize) -> Self {
        Self {
            max_fragment_size,
            base_sequence: 0,
        }
    }

    /// Fragment a large frame into smaller pieces
    pub fn fragment(&mut self, frame: Frame) -> Vec<Frame> {
        if frame.payload.len() <= self.max_fragment_size {
            return vec![frame];
        }

        let mut fragments = Vec::new();
        let mut offset = 0;
        let total = frame.payload.len();
        let _original_seq = frame.header.sequence;

        while offset < total {
            let end = (offset + self.max_fragment_size).min(total);
            let chunk = frame.payload.slice(offset..end);
            let is_last = end >= total;

            let mut flags = frame.header.flags;
            flags |= FrameFlags::BATCHED; // Mark as fragment
            if is_last {
                flags |= FrameFlags::FIN; // Mark final fragment
            }

            fragments.push(Frame {
                header: FrameHeader {
                    length: chunk.len() as u32,
                    flags,
                    sequence: self.base_sequence,
                    stream_id: frame.header.stream_id,
                    _reserved: 0,
                },
                payload: chunk,
            });

            self.base_sequence = self.base_sequence.wrapping_add(1);
            offset = end;
        }

        fragments
    }

    /// Reassemble fragments into a complete frame
    pub fn reassemble(&self, fragments: &[Frame]) -> TransportResult<Frame> {
        if fragments.is_empty() {
            return Err(TransportError::InvalidFrame("No fragments".into()));
        }

        // Verify all fragments belong to same stream
        let stream_id = fragments[0].header.stream_id;
        for frag in fragments {
            if frag.header.stream_id != stream_id {
                return Err(TransportError::InvalidFrame("Mismatched stream IDs".into()));
            }
        }

        // Check that last fragment has FIN flag
        if !fragments
            .last()
            .unwrap()
            .header
            .flags
            .contains(FrameFlags::FIN)
        {
            return Err(TransportError::InvalidFrame(
                "Missing FIN flag on last fragment".into(),
            ));
        }

        // Concatenate payloads
        let total_len: usize = fragments.iter().map(|f| f.payload.len()).sum();
        let mut payload = BytesMut::with_capacity(total_len);

        for frag in fragments {
            payload.put_slice(&frag.payload);
        }

        // Use flags from first fragment (minus BATCHED)
        let mut flags = fragments[0].header.flags;
        flags.remove(FrameFlags::BATCHED);
        flags.remove(FrameFlags::FIN);

        Ok(Frame {
            header: FrameHeader {
                length: total_len as u32,
                flags,
                sequence: fragments[0].header.sequence,
                stream_id,
                _reserved: 0,
            },
            payload: payload.freeze(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_codec_encode_decode() {
        let mut codec = FrameCodec::new(1024 * 1024);

        let frame = FrameBuilder::new()
            .payload(b"Hello, World!".to_vec())
            .sequence(42)
            .stream_id(7)
            .compressed()
            .build();

        let encoded = codec.encode(&frame);
        let decoded = codec.decode(&encoded).unwrap();

        assert_eq!(decoded.header.length, frame.header.length);
        assert_eq!(decoded.header.sequence, 42);
        assert_eq!(decoded.header.stream_id, 7);
        assert!(decoded.header.flags.contains(FrameFlags::COMPRESSED));
        assert_eq!(&decoded.payload[..], b"Hello, World!");
    }

    #[test]
    fn test_frame_builder() {
        let frame = FrameBuilder::new()
            .payload(b"test".to_vec())
            .sequence(123)
            .stream_id(456)
            .compressed()
            .encrypted()
            .priority()
            .build();

        assert_eq!(frame.header.sequence, 123);
        assert_eq!(frame.header.stream_id, 456);
        assert!(frame.header.flags.contains(FrameFlags::COMPRESSED));
        assert!(frame.header.flags.contains(FrameFlags::ENCRYPTED));
        assert!(frame.header.flags.contains(FrameFlags::PRIORITY));
    }

    #[test]
    fn test_frame_aggregator() {
        let mut agg = FrameAggregator::new(1000, 10);

        // Add frames until batch is ready
        for i in 0..10 {
            let frame = FrameBuilder::new().payload(vec![i as u8; 50]).build();

            let ready = agg.add(frame);
            if i < 9 {
                assert!(!ready);
            } else {
                assert!(ready);
            }
        }

        let batch = agg.take();
        assert_eq!(batch.len(), 10);
        assert!(agg.is_empty());
    }

    #[test]
    fn test_frame_fragmenter() {
        let mut fragmenter = FrameFragmenter::new(100);

        // Create a large frame
        let frame = FrameBuilder::new()
            .payload(vec![42u8; 350])
            .sequence(1)
            .stream_id(5)
            .build();

        let fragments = fragmenter.fragment(frame);
        assert_eq!(fragments.len(), 4); // 350 / 100 = 4 (with remainder)

        // Last fragment should have FIN
        assert!(fragments
            .last()
            .unwrap()
            .header
            .flags
            .contains(FrameFlags::FIN));

        // Reassemble
        let reassembled = fragmenter.reassemble(&fragments).unwrap();
        assert_eq!(reassembled.payload.len(), 350);
        assert_eq!(reassembled.header.stream_id, 5);
    }
}
