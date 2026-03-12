//! io_uring transport backend integration for spine-transport.
//!
//! Provides a `UringBackend` that wraps the low-level io_uring primitives
//! from spine-kernel and spine-transport's uring module into a unified
//! backend that can be used with the existing `ProtocolHandler`.
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────┐
//! │ ProtocolHandler│  (existing, works with AsyncRead+AsyncWrite)
//! └───────┬────────┘
//!         │
//! ┌───────▼────────┐
//! │ UringBackend   │  (this module)
//! │ ┌────────────┐ │
//! │ │ AcceptLoop │ │  io_uring accept → connection tracking
//! │ │ BufferPool │ │  Pre-registered kernel buffers
//! │ │ BatchSend  │ │  Coalesce + submit writes
//! │ │ Completion │ │  Process CQEs → wake futures
//! │ └────────────┘ │
//! └───────┬────────┘
//!         │
//! ┌───────▼────────┐
//! │ Linux Kernel   │  io_uring SQ/CQ (kernel bypass)
//! └────────────────┘
//! ```
//!
//! ## Usage
//!
//! The backend is feature-gated: `cargo build --features io-uring`
//!
//! On non-Linux or without the feature flag, this module is not compiled.

#![cfg(all(target_os = "linux", feature = "io-uring"))]

use crate::uring::{IoOp, OpType, UringConfig, UringRing};
use crate::{TransportError, TransportResult};

use bytes::BytesMut;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

// =============================================================================
// URING BACKEND
// =============================================================================

/// High-level io_uring backend for the SPINE transport layer.
///
/// Manages the io_uring ring, buffer pool, and connection tracking.
/// Designed to be used as the I/O backend for `ProtocolHandler` connections.
pub struct UringBackend {
    ring: UringRing,
    config: UringConfig,
    connections: HashMap<u64, ConnectionState>,
    next_conn_id: u64,
    stats: UringBackendStats,
    shutdown: Arc<AtomicBool>,
}

/// Per-connection state tracked by the backend.
#[derive(Debug)]
struct ConnectionState {
    fd: RawFd,
    addr: SocketAddr,
    bytes_sent: u64,
    bytes_recv: u64,
    pending_ops: u32,
}

/// Backend statistics.
#[derive(Debug, Default)]
pub struct UringBackendStats {
    pub connections_accepted: AtomicU64,
    pub connections_closed: AtomicU64,
    pub total_bytes_sent: AtomicU64,
    pub total_bytes_recv: AtomicU64,
    pub total_ops_submitted: AtomicU64,
    pub total_ops_completed: AtomicU64,
    pub batch_submissions: AtomicU64,
}

impl UringBackend {
    /// Create a new io_uring backend.
    pub fn new(config: UringConfig) -> TransportResult<Self> {
        let ring = UringRing::new(config.clone())?;
        Ok(Self {
            ring,
            config,
            connections: HashMap::new(),
            next_conn_id: 0,
            stats: UringBackendStats::default(),
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create with the default high-throughput config.
    pub fn high_throughput() -> TransportResult<Self> {
        Self::new(UringConfig::high_throughput())
    }

    /// Create with the default low-latency config.
    pub fn low_latency() -> TransportResult<Self> {
        Self::new(UringConfig::low_latency())
    }

    /// Register a new connection.
    pub fn register_connection(&mut self, fd: RawFd, addr: SocketAddr) -> u64 {
        let id = self.next_conn_id;
        self.next_conn_id += 1;
        self.connections.insert(
            id,
            ConnectionState {
                fd,
                addr,
                bytes_sent: 0,
                bytes_recv: 0,
                pending_ops: 0,
            },
        );
        self.stats
            .connections_accepted
            .fetch_add(1, Ordering::Relaxed);
        id
    }

    /// Submit a batched send via io_uring.
    pub fn submit_send(&mut self, conn_id: u64, data: &[u8]) -> TransportResult<u64> {
        let conn = self
            .connections
            .get_mut(&conn_id)
            .ok_or_else(|| TransportError::ConnectionClosed)?;

        let mut buffer = BytesMut::with_capacity(data.len());
        buffer.extend_from_slice(data);

        let user_data = self.ring.next_user_data();
        let op = IoOp::send(conn.fd, buffer, user_data);
        self.ring.submit(op)?;

        conn.pending_ops += 1;
        conn.bytes_sent += data.len() as u64;
        self.stats.total_ops_submitted.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_bytes_sent
            .fetch_add(data.len() as u64, Ordering::Relaxed);

        Ok(user_data)
    }

    /// Submit a batched recv via io_uring.
    pub fn submit_recv(&mut self, conn_id: u64, len: usize) -> TransportResult<u64> {
        let conn = self
            .connections
            .get_mut(&conn_id)
            .ok_or_else(|| TransportError::ConnectionClosed)?;

        let buffer = BytesMut::with_capacity(len);
        let user_data = self.ring.next_user_data();
        let op = IoOp::recv(conn.fd, buffer, user_data);
        self.ring.submit(op)?;

        conn.pending_ops += 1;
        self.stats.total_ops_submitted.fetch_add(1, Ordering::Relaxed);

        Ok(user_data)
    }

    /// Submit multiple sends as a batch (single syscall for all).
    pub fn submit_send_batch(
        &mut self,
        conn_id: u64,
        chunks: &[&[u8]],
    ) -> TransportResult<Vec<u64>> {
        let conn = self
            .connections
            .get_mut(&conn_id)
            .ok_or_else(|| TransportError::ConnectionClosed)?;

        let mut ops = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let mut buffer = BytesMut::with_capacity(chunk.len());
            buffer.extend_from_slice(chunk);
            let user_data = self.ring.next_user_data();
            ops.push(IoOp::send(conn.fd, buffer, user_data));
            conn.bytes_sent += chunk.len() as u64;
            conn.pending_ops += 1;
        }

        let user_datas = self.ring.submit_batch(ops)?;
        self.stats
            .total_ops_submitted
            .fetch_add(user_datas.len() as u64, Ordering::Relaxed);
        self.stats.batch_submissions.fetch_add(1, Ordering::Relaxed);

        Ok(user_datas)
    }

    /// Process completions (non-blocking).
    pub fn process_completions(&mut self) -> Vec<crate::uring::IoCompletion> {
        let completions = self.ring.peek_completions();
        self.stats
            .total_ops_completed
            .fetch_add(completions.len() as u64, Ordering::Relaxed);

        for comp in &completions {
            if comp.is_success() {
                self.stats
                    .total_bytes_recv
                    .fetch_add(comp.bytes_transferred() as u64, Ordering::Relaxed);
            }
        }

        completions
    }

    /// Close a connection.
    pub fn close_connection(&mut self, conn_id: u64) -> TransportResult<()> {
        if let Some(conn) = self.connections.remove(&conn_id) {
            let user_data = self.ring.next_user_data();
            let op = IoOp {
                op_type: OpType::Close,
                fd: conn.fd,
                fixed_file: None,
                buffer: None,
                buffer_idx: None,
                offset: 0,
                len: 0,
                user_data,
                flags: 0,
                addr: None,
                timeout: None,
                linked: false,
            };
            self.ring.submit(op)?;
            self.stats
                .connections_closed
                .fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Number of active connections.
    pub fn active_connections(&self) -> usize {
        self.connections.len()
    }

    /// Get backend statistics.
    pub fn stats(&self) -> &UringBackendStats {
        &self.stats
    }

    /// Signal shutdown.
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.ring.shutdown();
    }

    /// Check if shutdown.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_creation() {
        let backend = UringBackend::new(UringConfig::default());
        assert!(backend.is_ok());
        let backend = backend.unwrap();
        assert_eq!(backend.active_connections(), 0);
    }

    #[test]
    fn test_register_connection() {
        let mut backend = UringBackend::new(UringConfig::default()).unwrap();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let id = backend.register_connection(42, addr);
        assert_eq!(id, 0);
        assert_eq!(backend.active_connections(), 1);
    }

    #[test]
    fn test_close_connection() {
        let mut backend = UringBackend::new(UringConfig::default()).unwrap();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let id = backend.register_connection(42, addr);
        backend.close_connection(id).unwrap();
        assert_eq!(backend.active_connections(), 0);
    }

    #[test]
    fn test_shutdown() {
        let backend = UringBackend::new(UringConfig::default()).unwrap();
        assert!(!backend.is_shutdown());
        backend.shutdown();
        assert!(backend.is_shutdown());
    }

    #[test]
    fn test_stats_initial() {
        let backend = UringBackend::new(UringConfig::default()).unwrap();
        assert_eq!(backend.stats().connections_accepted.load(Ordering::Relaxed), 0);
        assert_eq!(backend.stats().total_bytes_sent.load(Ordering::Relaxed), 0);
    }
}
