//! WebSocket bridge for browser-based agents and human interfaces.
//!
//! Allows JavaScript/TypeScript clients to connect to the SPINE protocol
//! over standard WebSocket connections, bridging the browser world into
//! the agent communication layer.
//!
//! # Architecture
//!
//! ```text
//! Browser/JS Agent
//!     ↕ WebSocket (ws:// or wss://)
//! ┌──────────────────────────────────┐
//! │  WebSocketBridge                 │
//! │  ├─ Frame serialization (binary) │
//! │  ├─ Health monitoring            │
//! │  └─ TransportBackend impl        │
//! └──────────────────────────────────┘
//!     ↕ Frame
//! TransportBackend / ProtocolHandler
//! ```
//!
//! # Usage
//!
//! Server-side (accepting WebSocket upgrades):
//! ```rust,ignore
//! let bridge = WebSocketBridge::accept(ws_stream).await?;
//! // bridge implements TransportBackend — use directly with HyperTransport
//! ```
//!
//! Client-side (connecting to a WebSocket server):
//! ```rust,ignore
//! let bridge = WebSocketBridge::connect("ws://localhost:8083/ws").await?;
//! ```

use bytes::{Bytes, BytesMut};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

use crate::{Frame, TransportBackend, TransportError, TransportResult};

#[cfg(test)]
use crate::{FrameFlags, FrameHeader};

// =============================================================================
// WEBSOCKET BRIDGE
// =============================================================================

/// A WebSocket connection that implements `TransportBackend`.
///
/// Frames are serialized to binary WebSocket messages using the same
/// 12-byte header + payload format as the TCP transport.
pub struct WebSocketBridge {
    /// Underlying WebSocket stream
    inner: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    /// Connection creation time
    created_at: Instant,
    /// Last activity time
    last_activity: Instant,
    /// Health status
    healthy: bool,
    /// RTT estimate (measured via ping/pong)
    rtt_estimate: Duration,
    /// Pending ping sent at
    pending_ping: Option<Instant>,
    /// Bytes sent
    bytes_sent: u64,
    /// Bytes received
    bytes_received: u64,
}

/// A server-side WebSocket connection accepted from a raw TCP upgrade.
pub struct WebSocketServerBridge {
    /// Underlying WebSocket stream (server-side, no TLS wrapper)
    inner: tokio_tungstenite::WebSocketStream<TcpStream>,
    /// Connection creation time
    created_at: Instant,
    /// Last activity time
    last_activity: Instant,
    /// Health status
    healthy: bool,
    /// RTT estimate
    rtt_estimate: Duration,
    /// Bytes sent
    bytes_sent: u64,
    /// Bytes received
    bytes_received: u64,
}

// =============================================================================
// CLIENT-SIDE BRIDGE
// =============================================================================

impl WebSocketBridge {
    /// Connect to a WebSocket server.
    pub async fn connect(url: &str) -> TransportResult<Self> {
        let (ws_stream, _response) = tokio_tungstenite::connect_async(url)
            .await
            .map_err(|e| TransportError::Protocol(format!("WebSocket connect failed: {}", e)))?;

        Ok(Self {
            inner: ws_stream,
            created_at: Instant::now(),
            last_activity: Instant::now(),
            healthy: true,
            rtt_estimate: Duration::from_millis(50),
            pending_ping: None,
            bytes_sent: 0,
            bytes_received: 0,
        })
    }

    /// Get connection age.
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get idle time.
    pub fn idle_time(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// Get total bytes sent.
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent
    }

    /// Get total bytes received.
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received
    }
}

impl TransportBackend for WebSocketBridge {
    async fn send_frame(&mut self, frame: Frame) -> TransportResult<()> {
        use futures::SinkExt;
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        // Serialize: 12-byte header + payload
        let header = frame.header_bytes();
        let mut buf = Vec::with_capacity(12 + frame.payload.len());
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&frame.payload);

        self.inner
            .send(WsMessage::Binary(buf))
            .await
            .map_err(|e| TransportError::Protocol(format!("WebSocket send: {}", e)))?;

        self.bytes_sent += (12 + frame.payload.len()) as u64;
        self.last_activity = Instant::now();
        Ok(())
    }

    async fn recv_frame(&mut self) -> TransportResult<Frame> {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        loop {
            let msg = self
                .inner
                .next()
                .await
                .ok_or(TransportError::ConnectionClosed)?
                .map_err(|e| TransportError::Protocol(format!("WebSocket recv: {}", e)))?;

            match msg {
                WsMessage::Binary(data) => {
                    if data.len() < 12 {
                        return Err(TransportError::InvalidFrame(
                            "WebSocket message too short for frame header".into(),
                        ));
                    }

                    let header_bytes: [u8; 12] = data[..12]
                        .try_into()
                        .map_err(|_| TransportError::InvalidFrame("header slice error".into()))?;
                    let header = Frame::parse_header(&header_bytes);
                    let payload = Bytes::copy_from_slice(&data[12..]);

                    self.bytes_received += data.len() as u64;
                    self.last_activity = Instant::now();

                    return Ok(Frame { header, payload });
                }
                WsMessage::Ping(data) => {
                    let _ = self.inner.send(WsMessage::Pong(data)).await;
                }
                WsMessage::Pong(_) => {
                    if let Some(sent_at) = self.pending_ping.take() {
                        self.rtt_estimate = sent_at.elapsed();
                    }
                }
                WsMessage::Close(_) => {
                    self.healthy = false;
                    return Err(TransportError::ConnectionClosed);
                }
                _ => {
                    // Skip text frames, continuation, etc.
                    continue;
                }
            }
        }
    }

    async fn flush(&mut self) -> TransportResult<()> {
        use futures::SinkExt;
        self.inner
            .flush()
            .await
            .map_err(|e| TransportError::Protocol(format!("WebSocket flush: {}", e)))?;
        Ok(())
    }

    fn rtt(&self) -> Duration {
        self.rtt_estimate
    }

    fn is_healthy(&self) -> bool {
        self.healthy && self.idle_time() < Duration::from_secs(60)
    }

    async fn close(&mut self) -> TransportResult<()> {
        let _ = self.inner.close(None).await;
        self.healthy = false;
        Ok(())
    }
}

// =============================================================================
// SERVER-SIDE BRIDGE
// =============================================================================

impl WebSocketServerBridge {
    /// Accept a WebSocket connection from an already-upgraded TCP stream.
    pub async fn accept(stream: TcpStream) -> TransportResult<Self> {
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .map_err(|e| TransportError::Protocol(format!("WebSocket accept failed: {}", e)))?;

        Ok(Self {
            inner: ws_stream,
            created_at: Instant::now(),
            last_activity: Instant::now(),
            healthy: true,
            rtt_estimate: Duration::from_millis(10),
            bytes_sent: 0,
            bytes_received: 0,
        })
    }

    /// Get connection age.
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get idle time.
    pub fn idle_time(&self) -> Duration {
        self.last_activity.elapsed()
    }
}

impl TransportBackend for WebSocketServerBridge {
    async fn send_frame(&mut self, frame: Frame) -> TransportResult<()> {
        use futures::SinkExt;
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        let header = frame.header_bytes();
        let mut buf = Vec::with_capacity(12 + frame.payload.len());
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&frame.payload);

        self.inner
            .send(WsMessage::Binary(buf))
            .await
            .map_err(|e| TransportError::Protocol(format!("WebSocket send: {}", e)))?;

        self.bytes_sent += (12 + frame.payload.len()) as u64;
        self.last_activity = Instant::now();
        Ok(())
    }

    async fn recv_frame(&mut self) -> TransportResult<Frame> {
        use futures::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        loop {
            let msg = self
                .inner
                .next()
                .await
                .ok_or(TransportError::ConnectionClosed)?
                .map_err(|e| TransportError::Protocol(format!("WebSocket recv: {}", e)))?;

            match msg {
                WsMessage::Binary(data) => {
                    if data.len() < 12 {
                        return Err(TransportError::InvalidFrame(
                            "WebSocket message too short for frame header".into(),
                        ));
                    }

                    let header_bytes: [u8; 12] = data[..12]
                        .try_into()
                        .map_err(|_| TransportError::InvalidFrame("header slice error".into()))?;
                    let header = Frame::parse_header(&header_bytes);
                    let payload = Bytes::copy_from_slice(&data[12..]);

                    self.bytes_received += data.len() as u64;
                    self.last_activity = Instant::now();

                    return Ok(Frame { header, payload });
                }
                WsMessage::Ping(data) => {
                    let _ = self.inner.send(WsMessage::Pong(data)).await;
                }
                WsMessage::Close(_) => {
                    self.healthy = false;
                    return Err(TransportError::ConnectionClosed);
                }
                _ => continue,
            }
        }
    }

    async fn flush(&mut self) -> TransportResult<()> {
        use futures::SinkExt;
        self.inner
            .flush()
            .await
            .map_err(|e| TransportError::Protocol(format!("WebSocket flush: {}", e)))?;
        Ok(())
    }

    fn rtt(&self) -> Duration {
        self.rtt_estimate
    }

    fn is_healthy(&self) -> bool {
        self.healthy && self.idle_time() < Duration::from_secs(60)
    }

    async fn close(&mut self) -> TransportResult<()> {
        let _ = self.inner.close(None).await;
        self.healthy = false;
        Ok(())
    }
}

// =============================================================================
// ASYNC READ/WRITE ADAPTER
// =============================================================================

/// Wraps a `WebSocketServerBridge` as an `AsyncRead + AsyncWrite` stream
/// for compatibility with `ProtocolHandler<S>`.
///
/// This adapts the message-oriented WebSocket protocol to the byte-stream
/// interface expected by the SPINE protocol handler. Binary WebSocket messages
/// are buffered and presented as a continuous byte stream.
pub struct WebSocketStream {
    bridge: WebSocketServerBridge,
    /// Read buffer for partially consumed messages
    read_buf: BytesMut,
}

impl WebSocketStream {
    pub fn new(bridge: WebSocketServerBridge) -> Self {
        Self {
            bridge,
            read_buf: BytesMut::with_capacity(8192),
        }
    }

    /// Consume and return the inner bridge.
    pub fn into_inner(self) -> WebSocketServerBridge {
        self.bridge
    }
}

impl AsyncRead for WebSocketStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use futures::StreamExt;
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        // Drain read buffer first
        if !self.read_buf.is_empty() {
            let to_copy = std::cmp::min(buf.remaining(), self.read_buf.len());
            buf.put_slice(&self.read_buf.split_to(to_copy));
            return std::task::Poll::Ready(Ok(()));
        }

        // Poll for next WebSocket message
        let inner = &mut self.bridge.inner;
        match inner.poll_next_unpin(cx) {
            std::task::Poll::Ready(Some(Ok(WsMessage::Binary(data)))) => {
                let to_copy = std::cmp::min(buf.remaining(), data.len());
                buf.put_slice(&data[..to_copy]);
                if to_copy < data.len() {
                    self.read_buf.extend_from_slice(&data[to_copy..]);
                }
                self.bridge.last_activity = Instant::now();
                std::task::Poll::Ready(Ok(()))
            }
            std::task::Poll::Ready(Some(Ok(WsMessage::Close(_)))) => {
                self.bridge.healthy = false;
                std::task::Poll::Ready(Ok(())) // EOF
            }
            std::task::Poll::Ready(Some(Ok(_))) => {
                // Skip non-binary messages, re-poll
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
            std::task::Poll::Ready(Some(Err(e))) => {
                std::task::Poll::Ready(Err(std::io::Error::other(e)))
            }
            std::task::Poll::Ready(None) => {
                self.bridge.healthy = false;
                std::task::Poll::Ready(Ok(())) // EOF
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

impl AsyncWrite for WebSocketStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        use futures::SinkExt;
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        let msg = WsMessage::Binary(buf.to_vec());
        let inner = &mut self.bridge.inner;

        match inner.poll_ready_unpin(cx) {
            std::task::Poll::Ready(Ok(())) => match inner.start_send_unpin(msg) {
                Ok(()) => {
                    self.bridge.bytes_sent += buf.len() as u64;
                    self.bridge.last_activity = Instant::now();
                    std::task::Poll::Ready(Ok(buf.len()))
                }
                Err(e) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            },
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use futures::SinkExt;
        let inner = &mut self.bridge.inner;
        match inner.poll_flush_unpin(cx) {
            std::task::Poll::Ready(Ok(())) => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use futures::SinkExt;
        let inner = &mut self.bridge.inner;
        match inner.poll_close_unpin(cx) {
            std::task::Poll::Ready(Ok(())) => {
                self.bridge.healthy = false;
                std::task::Poll::Ready(Ok(()))
            }
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

// =============================================================================
// CLIENT-SIDE WEBSOCKET ASYNC READ/WRITE ADAPTER
// =============================================================================

/// Wraps a client-side `WebSocketBridge` as an `AsyncRead + AsyncWrite` stream
/// for compatibility with `ProtocolHandler<S>`.
///
/// Same concept as `WebSocketStream` (server-side) but works with the
/// `MaybeTlsStream` inner type of client connections.
pub struct WebSocketClientStream {
    bridge: WebSocketBridge,
    /// Read buffer for partially consumed messages
    read_buf: BytesMut,
}

impl WebSocketClientStream {
    pub fn new(bridge: WebSocketBridge) -> Self {
        Self {
            bridge,
            read_buf: BytesMut::with_capacity(8192),
        }
    }

    /// Consume and return the inner bridge.
    pub fn into_inner(self) -> WebSocketBridge {
        self.bridge
    }
}

impl AsyncRead for WebSocketClientStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use futures::StreamExt;
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        // Drain read buffer first
        if !self.read_buf.is_empty() {
            let to_copy = std::cmp::min(buf.remaining(), self.read_buf.len());
            buf.put_slice(&self.read_buf.split_to(to_copy));
            return std::task::Poll::Ready(Ok(()));
        }

        // Poll for next WebSocket message
        let inner = &mut self.bridge.inner;
        match inner.poll_next_unpin(cx) {
            std::task::Poll::Ready(Some(Ok(WsMessage::Binary(data)))) => {
                let to_copy = std::cmp::min(buf.remaining(), data.len());
                buf.put_slice(&data[..to_copy]);
                if to_copy < data.len() {
                    self.read_buf.extend_from_slice(&data[to_copy..]);
                }
                self.bridge.last_activity = Instant::now();
                std::task::Poll::Ready(Ok(()))
            }
            std::task::Poll::Ready(Some(Ok(WsMessage::Close(_)))) => {
                self.bridge.healthy = false;
                std::task::Poll::Ready(Ok(())) // EOF
            }
            std::task::Poll::Ready(Some(Ok(_))) => {
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
            std::task::Poll::Ready(Some(Err(e))) => {
                std::task::Poll::Ready(Err(std::io::Error::other(e)))
            }
            std::task::Poll::Ready(None) => {
                self.bridge.healthy = false;
                std::task::Poll::Ready(Ok(())) // EOF
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

impl AsyncWrite for WebSocketClientStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        use futures::SinkExt;
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        let msg = WsMessage::Binary(buf.to_vec());
        let inner = &mut self.bridge.inner;

        match inner.poll_ready_unpin(cx) {
            std::task::Poll::Ready(Ok(())) => match inner.start_send_unpin(msg) {
                Ok(()) => {
                    self.bridge.bytes_sent += buf.len() as u64;
                    self.bridge.last_activity = Instant::now();
                    std::task::Poll::Ready(Ok(buf.len()))
                }
                Err(e) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            },
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use futures::SinkExt;
        let inner = &mut self.bridge.inner;
        match inner.poll_flush_unpin(cx) {
            std::task::Poll::Ready(Ok(())) => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use futures::SinkExt;
        let inner = &mut self.bridge.inner;
        match inner.poll_close_unpin(cx) {
            std::task::Poll::Ready(Ok(())) => {
                self.bridge.healthy = false;
                std::task::Poll::Ready(Ok(()))
            }
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// Helper function to convert a client-side `WebSocketBridge` into an
/// `AsyncRead + AsyncWrite` stream for use with `ProtocolHandler`.
pub fn client_to_stream(bridge: WebSocketBridge) -> WebSocketClientStream {
    WebSocketClientStream::new(bridge)
}

// =============================================================================
// QUIC ASYNC READ/WRITE ADAPTER
// =============================================================================

/// Wraps QUIC send/recv streams as a unified `AsyncRead + AsyncWrite` stream
/// for compatibility with `ProtocolHandler<S>`.
///
/// This allows existing ProtocolHandler code to work over QUIC streams
/// without modification.
#[cfg(feature = "quic")]
pub struct QuicStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

#[cfg(feature = "quic")]
impl QuicStream {
    pub fn new(send: quinn::SendStream, recv: quinn::RecvStream) -> Self {
        Self { send, recv }
    }
}

#[cfg(feature = "quic")]
impl AsyncRead for QuicStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

#[cfg(feature = "quic")]
impl AsyncWrite for QuicStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match std::pin::Pin::new(&mut self.send).poll_write(cx, buf) {
            std::task::Poll::Ready(Ok(n)) => std::task::Poll::Ready(Ok(n)),
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match std::pin::Pin::new(&mut self.send).poll_flush(cx) {
            std::task::Poll::Ready(Ok(())) => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match std::pin::Pin::new(&mut self.send).poll_shutdown(cx) {
            std::task::Poll::Ready(Ok(())) => std::task::Poll::Ready(Ok(())),
            std::task::Poll::Ready(Err(e)) => std::task::Poll::Ready(Err(std::io::Error::other(e))),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_roundtrip_encoding() {
        // Verify that the 12-byte header + payload format is correct
        let frame = Frame {
            header: FrameHeader {
                length: 5,
                flags: FrameFlags::COMPRESSED,
                sequence: 42,
                stream_id: 7,
                _reserved: 0,
            },
            payload: Bytes::from_static(b"hello"),
        };

        let header = frame.header_bytes();
        assert_eq!(header.len(), 12);

        let parsed = Frame::parse_header(&header);
        assert_eq!(parsed.length, 5);
        assert!(parsed.flags.contains(FrameFlags::COMPRESSED));
        assert_eq!(parsed.sequence, 42);
        assert_eq!(parsed.stream_id, 7);
    }

    #[test]
    fn test_binary_message_format() {
        // Verify the binary wire format for WebSocket messages
        let frame = Frame::new(Bytes::from_static(b"test payload"));

        let header = frame.header_bytes();
        let mut buf = Vec::with_capacity(12 + frame.payload.len());
        buf.extend_from_slice(&header);
        buf.extend_from_slice(&frame.payload);

        assert_eq!(buf.len(), 12 + 12); // 12-byte header + "test payload"

        // Parse back
        let header_bytes: [u8; 12] = buf[..12].try_into().unwrap();
        let parsed = Frame::parse_header(&header_bytes);
        assert_eq!(parsed.length, 12);

        let payload = &buf[12..];
        assert_eq!(payload, b"test payload");
    }
}
