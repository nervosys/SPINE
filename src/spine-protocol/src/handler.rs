//! The protocol handler: fixed framing over any `AsyncRead + AsyncWrite`
//! stream.
//!
//! A message goes out as `wire::encode(msg)` (the `SP` header and the body),
//! then the compression flag byte, then the fixed frame header:
//! `length: u32 big-endian || 0x00`. There is no sealing: confidentiality comes
//! from the session or TLS layer. The Chameleon latent-space key and the
//! pre-shared-key AES layer that used to plug in here are gone; the research
//! code is in `research/spine-protocol-research`.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use zstd::stream::encode_all;

use super::*;

/// Length of the fixed frame header: `u32` big-endian body length, then `0x00`.
const FRAME_HEADER_LEN: usize = 5;

pub struct ProtocolHandler<S> {
    stream: S,
    /// Reusable read buffer for frame payloads
    read_buf: Vec<u8>,
    /// Minimum payload size for zstd compression (below this, overhead exceeds savings)
    compression_threshold: usize,
}

impl<S> ProtocolHandler<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            read_buf: Vec::with_capacity(8192),
            compression_threshold: 64,
        }
    }

    /// Send a message.
    ///
    /// The body is `wire::encode(msg)`, one flag byte (`0x01` plus zstd level 3
    /// when the body is at least `compression_threshold` bytes, `0x00` plus the
    /// raw body otherwise), and the fixed 5-byte frame header.
    pub async fn send_message(&mut self, msg: &Message) -> anyhow::Result<()> {
        let mut data = wire::encode(msg)?;

        // Adaptive compression with flag
        if data.len() >= self.compression_threshold {
            let compressed = encode_all(&data[..], 3)?;
            let mut flagged = Vec::with_capacity(1 + compressed.len());
            flagged.push(0x01);
            flagged.extend_from_slice(&compressed);
            data = flagged;
        } else {
            data.insert(0, 0x00);
        }

        self.write_frame(&data).await?;
        Ok(())
    }

    /// Same as [`send_message`](Self::send_message). Kept as a name for the
    /// callers that predate the removal of the speculative wrapper.
    pub async fn send_message_raw(&mut self, msg: &Message) -> anyhow::Result<()> {
        self.send_message(msg).await
    }

    /// Receive a message
    pub async fn receive_message(&mut self) -> anyhow::Result<Message> {
        let mut data = self.read_frame().await?;

        // Adaptive decompression: check compression flag byte
        if data.is_empty() {
            return Err(anyhow::anyhow!("Empty message"));
        }
        data = if data[0] == 0x01 {
            wire::decode_zstd_limited(&data[1..], wire::MAX_DECODED_FRAME_BYTES)?
        } else {
            data[1..].to_vec()
        };

        Ok(wire::decode(&data)?)
    }

    /// Write a frame: `length: u32 big-endian || 0x00`, then the body. The
    /// header lives on the stack.
    #[inline]
    async fn write_frame(&mut self, data: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(
            data.len() <= wire::MAX_TRANSPORT_FRAME_BYTES,
            "native transport frame exceeds size limit"
        );
        let mut header = [0u8; FRAME_HEADER_LEN];
        header[..4].copy_from_slice(&(data.len() as u32).to_be_bytes());
        // header[4] stays 0x00.
        write_header_body(&mut self.stream, &header, data).await?;
        Ok(())
    }

    /// Read a frame (reuses the read buffer to minimize allocations). The
    /// fifth header byte is read and ignored.
    #[inline]
    async fn read_frame(&mut self) -> anyhow::Result<Vec<u8>> {
        let mut header = [0u8; FRAME_HEADER_LEN];
        self.stream.read_exact(&mut header).await?;
        let len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;

        // SECURITY: Limit maximum frame size to prevent OOM attacks (16 MiB)
        const MAX_FRAME_SIZE: usize = wire::MAX_TRANSPORT_FRAME_BYTES;
        if len > MAX_FRAME_SIZE {
            anyhow::bail!(
                "Frame size {} exceeds maximum allowed size {}",
                len,
                MAX_FRAME_SIZE
            );
        }

        // Reuse read buffer to avoid per-frame allocations
        self.read_buf.clear();
        self.read_buf.resize(len, 0);
        self.stream.read_exact(&mut self.read_buf).await?;
        Ok(std::mem::take(&mut self.read_buf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A message small enough that its frame may stay raw, and one large enough
    /// to take the zstd path.
    fn sample_messages() -> (Message, Message) {
        let small = Message::Ping { timestamp: 7 };
        let large = Message::Event(Event {
            name: "large".to_string(),
            data: serde_json::json!({ "payload": "x".repeat(300) }),
        });
        (small, large)
    }

    #[tokio::test]
    async fn handlers_exchange_small_and_large_messages_both_ways() {
        let (a_stream, b_stream) = tokio::io::duplex(64 * 1024);
        let mut a = ProtocolHandler::new(a_stream);
        let mut b = ProtocolHandler::new(b_stream);
        let (small, large) = sample_messages();

        for msg in [&small, &large] {
            let expected = wire::encode(msg).unwrap();

            a.send_message(msg).await.unwrap();
            let got = b.receive_message().await.unwrap();
            assert_eq!(wire::encode(&got).unwrap(), expected);

            b.send_message(msg).await.unwrap();
            let got = a.receive_message().await.unwrap();
            assert_eq!(wire::encode(&got).unwrap(), expected);
        }
    }
}
