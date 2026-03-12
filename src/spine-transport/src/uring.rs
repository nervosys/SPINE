//! io_uring-based I/O for Linux kernel bypass.
//!
//! This module provides high-performance I/O using Linux's io_uring interface,
//! enabling efficient batched syscalls and true async I/O.

use std::collections::VecDeque;
use std::io::{self, IoSlice, IoSliceMut};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::{BufMut, Bytes, BytesMut};

use crate::{Frame, TransportError, TransportResult};

// =============================================================================
// IO_URING CONFIG
// =============================================================================

/// Configuration for io_uring
#[derive(Clone, Debug)]
pub struct UringConfig {
    /// Submission queue depth
    pub sq_depth: u32,
    /// Completion queue depth (usually 2x sq_depth)
    pub cq_depth: u32,
    /// Enable SQPOLL (kernel-side polling)
    pub sqpoll: bool,
    /// SQPOLL idle timeout in milliseconds
    pub sqpoll_idle_ms: u32,
    /// Enable IOPOLL (polling-based completion)
    pub iopoll: bool,
    /// Enable registered buffers
    pub registered_buffers: bool,
    /// Number of registered buffers
    pub num_buffers: usize,
    /// Size of each buffer
    pub buffer_size: usize,
    /// Enable fixed file descriptors
    pub fixed_files: bool,
    /// Maximum fixed file descriptors
    pub max_fixed_files: usize,
}

impl Default for UringConfig {
    fn default() -> Self {
        Self {
            sq_depth: 256,
            cq_depth: 512,
            sqpoll: false, // Requires CAP_SYS_NICE
            sqpoll_idle_ms: 1000,
            iopoll: false, // Only for O_DIRECT files
            registered_buffers: true,
            num_buffers: 1024,
            buffer_size: 8192,
            fixed_files: true,
            max_fixed_files: 64,
        }
    }
}

impl UringConfig {
    /// High-throughput configuration
    pub fn high_throughput() -> Self {
        Self {
            sq_depth: 1024,
            cq_depth: 2048,
            sqpoll: true,
            sqpoll_idle_ms: 2000,
            iopoll: false,
            registered_buffers: true,
            num_buffers: 4096,
            buffer_size: 16384,
            fixed_files: true,
            max_fixed_files: 256,
        }
    }

    /// Low-latency configuration
    pub fn low_latency() -> Self {
        Self {
            sq_depth: 128,
            cq_depth: 256,
            sqpoll: true,
            sqpoll_idle_ms: 100,
            iopoll: false,
            registered_buffers: true,
            num_buffers: 512,
            buffer_size: 4096,
            fixed_files: true,
            max_fixed_files: 32,
        }
    }
}

// =============================================================================
// OPERATION TYPES
// =============================================================================

/// Types of io_uring operations
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpType {
    Read,
    Write,
    Send,
    Recv,
    Accept,
    Connect,
    Close,
    Timeout,
    Cancel,
    LinkTimeout,
    Poll,
    Fsync,
    Nop,
}

/// An I/O operation for submission
pub struct IoOp {
    /// Operation type
    pub op_type: OpType,
    /// File descriptor (or -1 for fixed file index)
    pub fd: RawFd,
    /// Fixed file index (if using fixed files)
    pub fixed_file: Option<u32>,
    /// Buffer for the operation
    pub buffer: Option<BytesMut>,
    /// Buffer index (for registered buffers)
    pub buffer_idx: Option<u16>,
    /// Offset for read/write operations
    pub offset: u64,
    /// Length of the operation
    pub len: usize,
    /// User data for completion identification
    pub user_data: u64,
    /// Flags for the operation
    pub flags: u32,
    /// Socket address (for accept/connect)
    pub addr: Option<std::net::SocketAddr>,
    /// Timeout duration
    pub timeout: Option<Duration>,
    /// Link to next operation
    pub linked: bool,
}

impl IoOp {
    /// Create a new read operation
    pub fn read(fd: RawFd, buffer: BytesMut, offset: u64, user_data: u64) -> Self {
        let len = buffer.capacity();
        Self {
            op_type: OpType::Read,
            fd,
            fixed_file: None,
            buffer: Some(buffer),
            buffer_idx: None,
            offset,
            len,
            user_data,
            flags: 0,
            addr: None,
            timeout: None,
            linked: false,
        }
    }

    /// Create a new write operation
    pub fn write(fd: RawFd, buffer: BytesMut, offset: u64, user_data: u64) -> Self {
        let len = buffer.len();
        Self {
            op_type: OpType::Write,
            fd,
            fixed_file: None,
            buffer: Some(buffer),
            buffer_idx: None,
            offset,
            len,
            user_data,
            flags: 0,
            addr: None,
            timeout: None,
            linked: false,
        }
    }

    /// Create a new send operation (for sockets)
    pub fn send(fd: RawFd, buffer: BytesMut, user_data: u64) -> Self {
        let len = buffer.len();
        Self {
            op_type: OpType::Send,
            fd,
            fixed_file: None,
            buffer: Some(buffer),
            buffer_idx: None,
            offset: 0,
            len,
            user_data,
            flags: 0,
            addr: None,
            timeout: None,
            linked: false,
        }
    }

    /// Create a new recv operation (for sockets)
    pub fn recv(fd: RawFd, buffer: BytesMut, user_data: u64) -> Self {
        let len = buffer.capacity();
        Self {
            op_type: OpType::Recv,
            fd,
            fixed_file: None,
            buffer: Some(buffer),
            buffer_idx: None,
            offset: 0,
            len,
            user_data,
            flags: 0,
            addr: None,
            timeout: None,
            linked: false,
        }
    }

    /// Create a timeout operation
    pub fn timeout(duration: Duration, user_data: u64) -> Self {
        Self {
            op_type: OpType::Timeout,
            fd: -1,
            fixed_file: None,
            buffer: None,
            buffer_idx: None,
            offset: 0,
            len: 0,
            user_data,
            flags: 0,
            addr: None,
            timeout: Some(duration),
            linked: false,
        }
    }

    /// Use a fixed file descriptor
    pub fn with_fixed_file(mut self, idx: u32) -> Self {
        self.fixed_file = Some(idx);
        self.fd = -1;
        self
    }

    /// Use a registered buffer
    pub fn with_buffer_idx(mut self, idx: u16) -> Self {
        self.buffer_idx = Some(idx);
        self
    }

    /// Link to next operation
    pub fn linked(mut self) -> Self {
        self.linked = true;
        self
    }
}

/// Completion of an I/O operation
#[derive(Debug)]
pub struct IoCompletion {
    /// User data from the original operation
    pub user_data: u64,
    /// Result code (bytes transferred or negative errno)
    pub result: i32,
    /// Flags from completion
    pub flags: u32,
    /// Buffer returned from the operation
    pub buffer: Option<BytesMut>,
}

impl IoCompletion {
    /// Check if operation succeeded
    pub fn is_success(&self) -> bool {
        self.result >= 0
    }

    /// Get bytes transferred
    pub fn bytes_transferred(&self) -> usize {
        if self.result >= 0 {
            self.result as usize
        } else {
            0
        }
    }

    /// Convert to io::Result
    pub fn to_result(&self) -> io::Result<usize> {
        if self.result >= 0 {
            Ok(self.result as usize)
        } else {
            Err(io::Error::from_raw_os_error(-self.result))
        }
    }
}

// =============================================================================
// BUFFER POOL
// =============================================================================

/// Pool of registered buffers for io_uring
pub struct RegisteredBufferPool {
    /// Buffer storage
    buffers: Vec<BytesMut>,
    /// Free buffer indices
    free: VecDeque<u16>,
    /// Buffer size
    buffer_size: usize,
    /// Total buffers
    total: usize,
}

impl RegisteredBufferPool {
    /// Create a new buffer pool
    pub fn new(count: usize, size: usize) -> Self {
        let mut buffers = Vec::with_capacity(count);
        let mut free = VecDeque::with_capacity(count);

        for i in 0..count {
            buffers.push(BytesMut::with_capacity(size));
            free.push_back(i as u16);
        }

        Self {
            buffers,
            free,
            buffer_size: size,
            total: count,
        }
    }

    /// Get the raw buffer pointers for registration
    pub fn get_iovecs(&self) -> Vec<IoSlice> {
        self.buffers.iter().map(|b| IoSlice::new(b)).collect()
    }

    /// Allocate a buffer
    pub fn alloc(&mut self) -> Option<(u16, &mut BytesMut)> {
        let idx = self.free.pop_front()?;
        self.buffers[idx as usize].clear();
        Some((idx, &mut self.buffers[idx as usize]))
    }

    /// Free a buffer
    pub fn free(&mut self, idx: u16) {
        if (idx as usize) < self.total {
            self.free.push_back(idx);
        }
    }

    /// Get a buffer by index
    pub fn get(&self, idx: u16) -> Option<&BytesMut> {
        self.buffers.get(idx as usize)
    }

    /// Get a mutable buffer by index
    pub fn get_mut(&mut self, idx: u16) -> Option<&mut BytesMut> {
        self.buffers.get_mut(idx as usize)
    }

    /// Available buffers
    pub fn available(&self) -> usize {
        self.free.len()
    }
}

// =============================================================================
// IO_URING STUB (requires io-uring crate)
// =============================================================================

/// Stub for io_uring ring (actual implementation would use io-uring crate)
pub struct UringRing {
    config: UringConfig,
    /// Buffer pool
    buffers: RegisteredBufferPool,
    /// Fixed file descriptors
    fixed_fds: Vec<Option<RawFd>>,
    /// Pending operations
    pending: VecDeque<IoOp>,
    /// User data counter
    user_data_counter: AtomicU64,
    /// Shutdown flag
    shutdown: AtomicBool,
}

impl UringRing {
    /// Create a new io_uring ring
    ///
    /// Note: This is a stub. Real implementation would call io_uring_setup().
    pub fn new(config: UringConfig) -> TransportResult<Self> {
        let buffers = RegisteredBufferPool::new(config.num_buffers, config.buffer_size);

        let fixed_fds = vec![None; config.max_fixed_files];

        Ok(Self {
            config,
            buffers,
            fixed_fds,
            pending: VecDeque::new(),
            user_data_counter: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        })
    }

    /// Get next user data value
    pub fn next_user_data(&self) -> u64 {
        self.user_data_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Submit an operation
    pub fn submit(&mut self, op: IoOp) -> TransportResult<u64> {
        let user_data = op.user_data;
        self.pending.push_back(op);
        Ok(user_data)
    }

    /// Submit multiple operations (batched)
    pub fn submit_batch(&mut self, ops: Vec<IoOp>) -> TransportResult<Vec<u64>> {
        let user_datas: Vec<u64> = ops.iter().map(|op| op.user_data).collect();
        for op in ops {
            self.pending.push_back(op);
        }
        Ok(user_datas)
    }

    /// Submit and wait for completions
    pub fn submit_and_wait(&mut self, wait_nr: u32) -> TransportResult<Vec<IoCompletion>> {
        // Stub: In real implementation, this would:
        // 1. Copy pending ops to submission queue
        // 2. Call io_uring_enter()
        // 3. Process completion queue

        let mut completions = Vec::new();

        // Simulate processing
        while let Some(op) = self.pending.pop_front() {
            completions.push(IoCompletion {
                user_data: op.user_data,
                result: op.len as i32, // Simulate success
                flags: 0,
                buffer: op.buffer,
            });

            if completions.len() >= wait_nr as usize {
                break;
            }
        }

        Ok(completions)
    }

    /// Get completions without blocking
    pub fn peek_completions(&mut self) -> Vec<IoCompletion> {
        // Non-blocking completion peek
        Vec::new()
    }

    /// Register a file descriptor
    pub fn register_fd(&mut self, fd: RawFd) -> TransportResult<u32> {
        for (i, slot) in self.fixed_fds.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(fd);
                return Ok(i as u32);
            }
        }

        Err(TransportError::ResourceExhausted {
            resource: "fixed file descriptors".into(),
        })
    }

    /// Unregister a file descriptor
    pub fn unregister_fd(&mut self, idx: u32) -> TransportResult<()> {
        if let Some(slot) = self.fixed_fds.get_mut(idx as usize) {
            *slot = None;
            Ok(())
        } else {
            Err(TransportError::InvalidFrame("Invalid fd index".into()))
        }
    }

    /// Get buffer pool
    pub fn buffers(&mut self) -> &mut RegisteredBufferPool {
        &mut self.buffers
    }

    /// Shutdown the ring
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    /// Check if shutdown
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }
}

// =============================================================================
// URING TRANSPORT
// =============================================================================

/// High-performance transport using io_uring
pub struct UringTransport {
    /// The io_uring ring
    ring: UringRing,
    /// Configuration
    config: UringConfig,
    /// Statistics
    stats: UringStats,
}

impl UringTransport {
    /// Create a new io_uring transport
    pub fn new(config: UringConfig) -> TransportResult<Self> {
        let ring = UringRing::new(config.clone())?;

        Ok(Self {
            ring,
            config,
            stats: UringStats::new(),
        })
    }

    /// Send data using io_uring
    pub fn send(&mut self, fd: RawFd, data: &[u8]) -> TransportResult<u64> {
        let mut buffer = BytesMut::with_capacity(data.len());
        buffer.put_slice(data);

        let user_data = self.ring.next_user_data();
        let op = IoOp::send(fd, buffer, user_data);

        self.ring.submit(op)?;
        self.stats.ops_submitted.fetch_add(1, Ordering::Relaxed);
        self.stats
            .bytes_submitted
            .fetch_add(data.len() as u64, Ordering::Relaxed);

        Ok(user_data)
    }

    /// Receive data using io_uring
    pub fn recv(&mut self, fd: RawFd, len: usize) -> TransportResult<u64> {
        let buffer = BytesMut::with_capacity(len);
        let user_data = self.ring.next_user_data();
        let op = IoOp::recv(fd, buffer, user_data);

        self.ring.submit(op)?;
        self.stats.ops_submitted.fetch_add(1, Ordering::Relaxed);

        Ok(user_data)
    }

    /// Send a batch of frames
    pub fn send_frames(&mut self, fd: RawFd, frames: &[Frame]) -> TransportResult<Vec<u64>> {
        let mut ops = Vec::with_capacity(frames.len());

        for frame in frames {
            let mut buffer = BytesMut::with_capacity(12 + frame.payload.len());
            buffer.put_slice(&frame.header_bytes());
            buffer.put_slice(&frame.payload);

            let user_data = self.ring.next_user_data();
            ops.push(IoOp::send(fd, buffer, user_data));
        }

        let user_datas = self.ring.submit_batch(ops)?;
        self.stats
            .ops_submitted
            .fetch_add(user_datas.len() as u64, Ordering::Relaxed);

        Ok(user_datas)
    }

    /// Wait for completions
    pub fn wait(&mut self, count: u32) -> TransportResult<Vec<IoCompletion>> {
        let completions = self.ring.submit_and_wait(count)?;
        self.stats
            .ops_completed
            .fetch_add(completions.len() as u64, Ordering::Relaxed);

        for comp in &completions {
            if comp.is_success() {
                self.stats
                    .bytes_completed
                    .fetch_add(comp.bytes_transferred() as u64, Ordering::Relaxed);
            } else {
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
            }
        }

        Ok(completions)
    }

    /// Get statistics
    pub fn stats(&self) -> &UringStats {
        &self.stats
    }

    /// Shutdown
    pub fn shutdown(&self) {
        self.ring.shutdown();
    }
}

/// io_uring statistics
pub struct UringStats {
    /// Operations submitted
    pub ops_submitted: AtomicU64,
    /// Operations completed
    pub ops_completed: AtomicU64,
    /// Bytes submitted
    pub bytes_submitted: AtomicU64,
    /// Bytes completed
    pub bytes_completed: AtomicU64,
    /// Errors
    pub errors: AtomicU64,
    /// SQ overflows
    pub sq_overflows: AtomicU64,
    /// CQ overflows
    pub cq_overflows: AtomicU64,
}

impl UringStats {
    /// Create new stats
    pub fn new() -> Self {
        Self {
            ops_submitted: AtomicU64::new(0),
            ops_completed: AtomicU64::new(0),
            bytes_submitted: AtomicU64::new(0),
            bytes_completed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            sq_overflows: AtomicU64::new(0),
            cq_overflows: AtomicU64::new(0),
        }
    }
}

impl Default for UringStats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uring_config() {
        let default = UringConfig::default();
        assert_eq!(default.sq_depth, 256);
        assert!(!default.sqpoll);

        let high_throughput = UringConfig::high_throughput();
        assert_eq!(high_throughput.sq_depth, 1024);
        assert!(high_throughput.sqpoll);
    }

    #[test]
    fn test_buffer_pool() {
        let mut pool = RegisteredBufferPool::new(4, 1024);

        assert_eq!(pool.available(), 4);

        let (idx1, _) = pool.alloc().unwrap();
        let (idx2, _) = pool.alloc().unwrap();

        assert_eq!(pool.available(), 2);

        pool.free(idx1);
        assert_eq!(pool.available(), 3);

        pool.free(idx2);
        assert_eq!(pool.available(), 4);
    }

    #[test]
    fn test_io_op_creation() {
        let buffer = BytesMut::with_capacity(1024);
        let op = IoOp::read(5, buffer, 0, 42);

        assert_eq!(op.op_type, OpType::Read);
        assert_eq!(op.fd, 5);
        assert_eq!(op.user_data, 42);
        assert_eq!(op.len, 1024);
    }

    #[test]
    fn test_io_op_with_fixed_file() {
        let buffer = BytesMut::with_capacity(1024);
        let op = IoOp::send(5, buffer, 42).with_fixed_file(7);

        assert_eq!(op.fixed_file, Some(7));
        assert_eq!(op.fd, -1);
    }

    #[test]
    fn test_uring_ring() {
        let config = UringConfig::default();
        let mut ring = UringRing::new(config).unwrap();

        let buffer = BytesMut::from(&b"test"[..]);
        let op = IoOp::send(5, buffer, ring.next_user_data());

        let user_data = ring.submit(op).unwrap();
        assert_eq!(user_data, 0);

        let completions = ring.submit_and_wait(1).unwrap();
        assert_eq!(completions.len(), 1);
    }

    #[test]
    fn test_io_completion() {
        let comp = IoCompletion {
            user_data: 42,
            result: 1024,
            flags: 0,
            buffer: None,
        };

        assert!(comp.is_success());
        assert_eq!(comp.bytes_transferred(), 1024);

        let comp_err = IoCompletion {
            user_data: 43,
            result: -11, // EAGAIN
            flags: 0,
            buffer: None,
        };

        assert!(!comp_err.is_success());
        assert_eq!(comp_err.bytes_transferred(), 0);
    }
}
