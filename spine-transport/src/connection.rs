//! Connection management with health monitoring.

use bytes::{Bytes, BytesMut};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::{
    BbrController, Frame, FrameFlags, TransportBackend, TransportConfig, TransportError,
    TransportResult,
};

// =============================================================================
// TCP CONNECTION
// =============================================================================

/// High-performance TCP connection with zero-copy support
pub struct TcpConnection {
    /// Underlying TCP stream
    stream: TcpStream,
    /// Read buffer
    read_buf: BytesMut,
    /// Write buffer for coalescing
    write_buf: BytesMut,
    /// Congestion controller
    congestion: Option<BbrController>,
    /// Last activity time
    last_activity: Instant,
    /// Connection creation time
    created_at: Instant,
    /// Remote address
    peer_addr: SocketAddr,
    /// RTT estimate
    rtt_estimate: Duration,
    /// Health status
    healthy: bool,
    /// Bytes sent
    bytes_sent: u64,
    /// Bytes received
    bytes_received: u64,
}

impl TcpConnection {
    /// Create a new TCP connection
    pub async fn connect(addr: SocketAddr, config: &TransportConfig) -> TransportResult<Self> {
        let stream = TcpStream::connect(addr).await?;
        Self::from_stream(stream, config)
    }

    /// Create from an existing TCP stream
    pub fn from_stream(stream: TcpStream, config: &TransportConfig) -> TransportResult<Self> {
        // Configure socket options using socket2
        let std_stream = stream.into_std()?;
        let socket = socket2::Socket::from(std_stream);

        // Buffer sizes
        socket.set_send_buffer_size(config.send_buffer_size)?;
        socket.set_recv_buffer_size(config.recv_buffer_size)?;

        // TCP_NODELAY
        if config.tcp_nodelay && !config.enable_coalescing {
            socket.set_nodelay(true)?;
        }

        // Keep-alive
        if config.health_check_interval.as_secs() > 0 {
            let keepalive = socket2::TcpKeepalive::new()
                .with_time(config.health_check_interval)
                .with_interval(Duration::from_secs(1));
            socket.set_tcp_keepalive(&keepalive)?;
        }

        // Convert back to tokio
        let std_stream: std::net::TcpStream = socket.into();
        std_stream.set_nonblocking(true)?;
        let stream = TcpStream::from_std(std_stream)?;

        let peer_addr = stream.peer_addr()?;

        let congestion = if config.enable_bbr {
            Some(BbrController::new())
        } else {
            None
        };

        Ok(Self {
            stream,
            read_buf: BytesMut::with_capacity(64 * 1024),
            write_buf: BytesMut::with_capacity(64 * 1024),
            congestion,
            last_activity: Instant::now(),
            created_at: Instant::now(),
            peer_addr,
            rtt_estimate: Duration::from_millis(50),
            healthy: true,
            bytes_sent: 0,
            bytes_received: 0,
        })
    }

    /// Get peer address
    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    /// Get connection age
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get idle time
    pub fn idle_time(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// Get total bytes sent
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent
    }

    /// Get total bytes received
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received
    }

    /// Send raw bytes
    pub async fn send_raw(&mut self, data: &[u8]) -> TransportResult<()> {
        self.stream.write_all(data).await?;
        self.bytes_sent += data.len() as u64;
        self.last_activity = Instant::now();

        if let Some(ref mut cc) = self.congestion {
            cc.on_send(data.len());
        }

        Ok(())
    }

    /// Read exact number of bytes
    pub async fn recv_exact(&mut self, buf: &mut [u8]) -> TransportResult<()> {
        self.stream.read_exact(buf).await?;
        self.bytes_received += buf.len() as u64;
        self.last_activity = Instant::now();
        Ok(())
    }

    /// Ping to check connection health and measure RTT
    pub async fn ping(&mut self) -> TransportResult<Duration> {
        let start = Instant::now();

        // Send a minimal ping frame
        let ping_frame = [
            0x00,
            0x00,
            0x00,
            0x00,
            FrameFlags::CONTROL.bits(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        self.send_raw(&ping_frame).await?;

        // Wait for pong
        let mut pong = [0u8; 12];
        tokio::time::timeout(Duration::from_secs(5), self.recv_exact(&mut pong))
            .await
            .map_err(|_| TransportError::Timeout)??;

        let rtt = start.elapsed();
        self.rtt_estimate = rtt;

        if let Some(ref mut cc) = self.congestion {
            cc.on_ack(12, rtt);
        }

        Ok(rtt)
    }
}

impl TransportBackend for TcpConnection {
    async fn send_frame(&mut self, frame: Frame) -> TransportResult<()> {
        // Send header
        let header_bytes = frame.header_bytes();
        self.send_raw(&header_bytes).await?;

        // Send payload
        self.send_raw(&frame.payload).await?;

        self.last_activity = Instant::now();

        // Update congestion controller
        if let Some(ref mut cc) = self.congestion {
            let total = header_bytes.len() + frame.payload.len();
            cc.on_send(total);
        }

        Ok(())
    }

    async fn recv_frame(&mut self) -> TransportResult<Frame> {
        // Read header
        let mut header_buf = [0u8; 12];
        self.recv_exact(&mut header_buf).await?;

        let header = Frame::parse_header(&header_buf);

        // Validate length
        if header.length > 16 * 1024 * 1024 {
            return Err(TransportError::InvalidFrame(format!(
                "Frame too large: {} bytes",
                header.length
            )));
        }

        // Read payload
        let mut payload = vec![0u8; header.length as usize];
        self.recv_exact(&mut payload).await?;

        self.last_activity = Instant::now();

        // Update congestion controller
        if let Some(ref mut cc) = self.congestion {
            cc.on_ack(12 + header.length as usize, self.rtt_estimate);
        }

        Ok(Frame {
            header,
            payload: Bytes::from(payload),
        })
    }

    async fn flush(&mut self) -> TransportResult<()> {
        self.stream.flush().await?;
        Ok(())
    }

    fn rtt(&self) -> Duration {
        self.rtt_estimate
    }

    fn is_healthy(&self) -> bool {
        self.healthy && self.idle_time() < Duration::from_secs(60)
    }

    async fn close(&mut self) -> TransportResult<()> {
        self.stream.shutdown().await?;
        self.healthy = false;
        Ok(())
    }
}

// =============================================================================
// QUIC CONNECTION
// =============================================================================

#[cfg(feature = "quic")]
pub mod quic {
    use super::*;
    use quinn::{Connection as QuinnConnection, RecvStream, SendStream};

    /// High-performance QUIC connection
    pub struct QuicConnection {
        /// QUIC connection handle
        connection: QuinnConnection,
        /// Current send stream
        send_stream: Option<SendStream>,
        /// Current receive stream
        recv_stream: Option<RecvStream>,
        /// Connection creation time
        created_at: Instant,
        /// Last activity time
        last_activity: Instant,
        /// Health status
        healthy: bool,
        /// RTT estimate
        rtt_estimate: Duration,
    }

    impl QuicConnection {
        /// Create from an existing QUIC connection
        pub fn new(connection: QuinnConnection) -> Self {
            Self {
                connection,
                send_stream: None,
                recv_stream: None,
                created_at: Instant::now(),
                last_activity: Instant::now(),
                healthy: true,
                rtt_estimate: Duration::from_millis(50),
            }
        }

        /// Open a bidirectional stream
        pub async fn open_bi(&mut self) -> TransportResult<()> {
            let (send, recv) = self
                .connection
                .open_bi()
                .await
                .map_err(TransportError::from)?;
            self.send_stream = Some(send);
            self.recv_stream = Some(recv);
            Ok(())
        }

        /// Get connection age
        pub fn age(&self) -> Duration {
            self.created_at.elapsed()
        }

        /// Get idle time
        pub fn idle_time(&self) -> Duration {
            self.last_activity.elapsed()
        }
    }

    impl TransportBackend for QuicConnection {
        async fn send_frame(&mut self, frame: Frame) -> TransportResult<()> {
            let stream = self
                .send_stream
                .as_mut()
                .ok_or_else(|| TransportError::ConnectionClosed)?;

            // Send header
            stream
                .write_all(&frame.header_bytes())
                .await
                .map_err(|e| TransportError::Protocol(e.to_string()))?;

            // Send payload
            stream
                .write_all(&frame.payload)
                .await
                .map_err(|e| TransportError::Protocol(e.to_string()))?;

            self.last_activity = Instant::now();
            Ok(())
        }

        async fn recv_frame(&mut self) -> TransportResult<Frame> {
            let stream = self
                .recv_stream
                .as_mut()
                .ok_or_else(|| TransportError::ConnectionClosed)?;

            // Read header
            let mut header_buf = [0u8; 12];
            stream
                .read_exact(&mut header_buf)
                .await
                .map_err(|e| TransportError::Protocol(e.to_string()))?;

            let header = Frame::parse_header(&header_buf);

            // Validate length
            if header.length > 16 * 1024 * 1024 {
                return Err(TransportError::InvalidFrame(format!(
                    "Frame too large: {} bytes",
                    header.length
                )));
            }

            // Read payload
            let mut payload = vec![0u8; header.length as usize];
            stream
                .read_exact(&mut payload)
                .await
                .map_err(|e| TransportError::Protocol(e.to_string()))?;

            self.last_activity = Instant::now();

            // Update RTT from QUIC stats
            self.rtt_estimate = self.connection.rtt();

            Ok(Frame {
                header,
                payload: Bytes::from(payload),
            })
        }

        async fn flush(&mut self) -> TransportResult<()> {
            // QUIC handles flushing automatically
            Ok(())
        }

        fn rtt(&self) -> Duration {
            self.connection.rtt()
        }

        fn is_healthy(&self) -> bool {
            self.healthy && self.connection.close_reason().is_none()
        }

        async fn close(&mut self) -> TransportResult<()> {
            if let Some(mut stream) = self.send_stream.take() {
                let _ = stream.finish();
            }
            self.connection.close(0u32.into(), b"close");
            self.healthy = false;
            Ok(())
        }
    }
}

// =============================================================================
// HEALTH MONITOR
// =============================================================================

/// Monitors connection health
pub struct HealthMonitor {
    /// Check interval
    check_interval: Duration,
    /// Failure threshold
    failure_threshold: u32,
    /// Current failures
    failures: u32,
    /// Last check time
    last_check: Instant,
}

impl HealthMonitor {
    /// Create a new health monitor
    pub fn new(check_interval: Duration, failure_threshold: u32) -> Self {
        Self {
            check_interval,
            failure_threshold,
            failures: 0,
            last_check: Instant::now(),
        }
    }

    /// Record a successful check
    pub fn success(&mut self) {
        self.failures = 0;
        self.last_check = Instant::now();
    }

    /// Record a failed check
    pub fn failure(&mut self) {
        self.failures += 1;
        self.last_check = Instant::now();
    }

    /// Check if connection should be considered unhealthy
    pub fn is_unhealthy(&self) -> bool {
        self.failures >= self.failure_threshold
    }

    /// Check if a health check is due
    pub fn should_check(&self) -> bool {
        self.last_check.elapsed() >= self.check_interval
    }

    /// Get current failure count
    pub fn failures(&self) -> u32 {
        self.failures
    }
}

// =============================================================================
// RECONNECTING CONNECTION
// =============================================================================

/// Connection wrapper that automatically reconnects
pub struct ReconnectingConnection<C: TransportBackend> {
    /// Current connection
    connection: Option<C>,
    /// Connection factory
    factory: Box<
        dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = TransportResult<C>> + Send>>
            + Send
            + Sync,
    >,
    /// Health monitor
    health: HealthMonitor,
    /// Reconnect delay
    reconnect_delay: Duration,
    /// Maximum reconnect attempts
    max_attempts: u32,
    /// Current attempt
    current_attempt: u32,
}

impl<C: TransportBackend> ReconnectingConnection<C> {
    /// Create a new reconnecting connection
    pub fn new<F, Fut>(
        factory: F,
        health_interval: Duration,
        reconnect_delay: Duration,
        max_attempts: u32,
    ) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = TransportResult<C>> + Send + 'static,
    {
        Self {
            connection: None,
            factory: Box::new(move || Box::pin(factory())),
            health: HealthMonitor::new(health_interval, 3),
            reconnect_delay,
            max_attempts,
            current_attempt: 0,
        }
    }

    /// Ensure connection is established
    pub async fn ensure_connected(&mut self) -> TransportResult<()> {
        if self
            .connection
            .as_ref()
            .map(|c| c.is_healthy())
            .unwrap_or(false)
        {
            return Ok(());
        }

        // Try to reconnect
        self.reconnect().await
    }

    /// Attempt to reconnect
    async fn reconnect(&mut self) -> TransportResult<()> {
        // Close existing connection
        if let Some(mut conn) = self.connection.take() {
            let _ = conn.close().await;
        }

        // Exponential backoff
        while self.current_attempt < self.max_attempts {
            self.current_attempt += 1;

            let delay = self.reconnect_delay * (1 << self.current_attempt.min(5));
            if self.current_attempt > 1 {
                tokio::time::sleep(delay).await;
            }

            match (self.factory)().await {
                Ok(conn) => {
                    self.connection = Some(conn);
                    self.current_attempt = 0;
                    self.health.success();
                    return Ok(());
                }
                Err(_e) => {
                    self.health.failure();
                }
            }
        }

        Err(TransportError::ConnectionClosed)
    }

    /// Get reference to inner connection
    pub fn inner(&self) -> Option<&C> {
        self.connection.as_ref()
    }

    /// Get mutable reference to inner connection
    pub fn inner_mut(&mut self) -> Option<&mut C> {
        self.connection.as_mut()
    }
}

// =============================================================================
// CONNECTION FACTORY
// =============================================================================

/// Factory for creating connections
pub struct ConnectionFactory {
    /// Default configuration
    config: TransportConfig,
}

impl ConnectionFactory {
    /// Create a new factory
    pub fn new(config: TransportConfig) -> Self {
        Self { config }
    }

    /// Create a TCP connection
    pub async fn create_tcp(&self, addr: SocketAddr) -> TransportResult<TcpConnection> {
        TcpConnection::connect(addr, &self.config).await
    }

    /// Create a QUIC connection
    #[cfg(feature = "quic")]
    pub async fn create_quic(
        &self,
        endpoint: &quinn::Endpoint,
        addr: SocketAddr,
        server_name: &str,
    ) -> TransportResult<quic::QuicConnection> {
        let connection = endpoint
            .connect(addr, server_name)
            .map_err(|e| TransportError::Protocol(e.to_string()))?
            .await?;

        let mut quic_conn = quic::QuicConnection::new(connection);
        quic_conn.open_bi().await?;

        Ok(quic_conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_monitor() {
        let mut monitor = HealthMonitor::new(Duration::from_secs(5), 3);

        assert!(!monitor.is_unhealthy());

        monitor.failure();
        monitor.failure();
        assert!(!monitor.is_unhealthy());

        monitor.failure();
        assert!(monitor.is_unhealthy());

        monitor.success();
        assert!(!monitor.is_unhealthy());
    }
}
