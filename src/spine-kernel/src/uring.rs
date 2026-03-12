//! io_uring Kernel Bypass I/O (Linux only)
//!
//! Ultra-low-latency async I/O using Linux io_uring:
//! - Zero-copy buffer registration
//! - Batched submission/completion
//! - Kernel polling for sub-microsecond latency
//!
//! This module provides a safe abstraction over io_uring for
//! high-performance network and file I/O.

#![cfg(all(target_os = "linux", feature = "io-uring"))]

use std::io::{self, Error, ErrorKind, Result};
use std::os::unix::io::RawFd;
use std::ptr::NonNull;

use io_uring::{opcode, types, IoUring, Submitter};

// =============================================================================
// IO URING WRAPPER
// =============================================================================

/// High-performance io_uring wrapper
///
/// Provides batched, zero-copy async I/O operations.
pub struct UringIo {
    ring: IoUring,
    registered_buffers: Vec<Vec<u8>>,
}

impl UringIo {
    /// Create a new io_uring instance
    ///
    /// # Arguments
    /// * `entries` - Number of submission queue entries (power of 2)
    pub fn new(entries: u32) -> Result<Self> {
        let ring = IoUring::builder()
            .setup_sqpoll(1000) // Kernel polling with 1ms idle timeout
            .build(entries)
            .map_err(|e| Error::new(ErrorKind::Other, e))?;

        Ok(Self {
            ring,
            registered_buffers: Vec::new(),
        })
    }

    /// Create without kernel polling (lower CPU usage)
    pub fn new_standard(entries: u32) -> Result<Self> {
        let ring = IoUring::new(entries).map_err(|e| Error::new(ErrorKind::Other, e))?;

        Ok(Self {
            ring,
            registered_buffers: Vec::new(),
        })
    }

    /// Register buffers for zero-copy I/O
    ///
    /// Registered buffers avoid kernel copies during I/O.
    pub fn register_buffers(&mut self, buffers: Vec<Vec<u8>>) -> Result<()> {
        let iovecs: Vec<libc::iovec> = buffers
            .iter()
            .map(|buf| libc::iovec {
                iov_base: buf.as_ptr() as *mut _,
                iov_len: buf.len(),
            })
            .collect();

        self.ring
            .submitter()
            .register_buffers(&iovecs)
            .map_err(|e| Error::new(ErrorKind::Other, e))?;

        self.registered_buffers = buffers;
        Ok(())
    }

    /// Submit a read operation
    pub fn submit_read(
        &mut self,
        fd: RawFd,
        buf_index: u16,
        offset: u64,
        user_data: u64,
    ) -> Result<()> {
        let buf = &self.registered_buffers[buf_index as usize];

        let entry = opcode::Read::new(types::Fd(fd), buf.as_ptr() as *mut _, buf.len() as _)
            .offset(offset)
            .build()
            .user_data(user_data);

        // SAFETY: Entry is valid
        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a write operation
    pub fn submit_write(
        &mut self,
        fd: RawFd,
        buf_index: u16,
        offset: u64,
        len: usize,
        user_data: u64,
    ) -> Result<()> {
        let buf = &self.registered_buffers[buf_index as usize];

        let entry = opcode::Write::new(types::Fd(fd), buf.as_ptr(), len.min(buf.len()) as _)
            .offset(offset)
            .build()
            .user_data(user_data);

        // SAFETY: Entry is valid
        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a vectored read (scatter)
    pub fn submit_readv(
        &mut self,
        fd: RawFd,
        iovecs: &[libc::iovec],
        offset: u64,
        user_data: u64,
    ) -> Result<()> {
        let entry = opcode::Readv::new(types::Fd(fd), iovecs.as_ptr(), iovecs.len() as _)
            .offset(offset)
            .build()
            .user_data(user_data);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a vectored write (gather)
    pub fn submit_writev(
        &mut self,
        fd: RawFd,
        iovecs: &[libc::iovec],
        offset: u64,
        user_data: u64,
    ) -> Result<()> {
        let entry = opcode::Writev::new(types::Fd(fd), iovecs.as_ptr(), iovecs.len() as _)
            .offset(offset)
            .build()
            .user_data(user_data);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a TCP accept operation
    pub fn submit_accept(&mut self, listen_fd: RawFd, user_data: u64) -> Result<()> {
        let entry = opcode::Accept::new(
            types::Fd(listen_fd),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
        .build()
        .user_data(user_data);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a TCP connect operation
    pub fn submit_connect(
        &mut self,
        fd: RawFd,
        addr: &libc::sockaddr_in,
        user_data: u64,
    ) -> Result<()> {
        let entry = opcode::Connect::new(
            types::Fd(fd),
            addr as *const _ as *const _,
            std::mem::size_of::<libc::sockaddr_in>() as _,
        )
        .build()
        .user_data(user_data);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a TCP send operation
    pub fn submit_send(&mut self, fd: RawFd, buf: &[u8], user_data: u64) -> Result<()> {
        let entry = opcode::Send::new(types::Fd(fd), buf.as_ptr(), buf.len() as _)
            .build()
            .user_data(user_data);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a TCP recv operation
    pub fn submit_recv(&mut self, fd: RawFd, buf: &mut [u8], user_data: u64) -> Result<()> {
        let entry = opcode::Recv::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
            .build()
            .user_data(user_data);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a close operation
    pub fn submit_close(&mut self, fd: RawFd, user_data: u64) -> Result<()> {
        let entry = opcode::Close::new(types::Fd(fd))
            .build()
            .user_data(user_data);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a timeout operation
    pub fn submit_timeout(&mut self, timespec: &types::Timespec, user_data: u64) -> Result<()> {
        let entry = opcode::Timeout::new(timespec).build().user_data(user_data);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit a no-op (useful for waking up the kernel poller)
    pub fn submit_nop(&mut self, user_data: u64) -> Result<()> {
        let entry = opcode::Nop::new().build().user_data(user_data);

        unsafe {
            self.ring
                .submission()
                .push(&entry)
                .map_err(|_| Error::new(ErrorKind::Other, "submission queue full"))?;
        }

        Ok(())
    }

    /// Submit all pending operations to the kernel
    pub fn submit(&mut self) -> Result<usize> {
        self.ring
            .submit()
            .map_err(|e| Error::new(ErrorKind::Other, e))
    }

    /// Submit and wait for at least one completion
    pub fn submit_and_wait(&mut self, want: usize) -> Result<usize> {
        self.ring
            .submit_and_wait(want)
            .map_err(|e| Error::new(ErrorKind::Other, e))
    }

    /// Process completions
    ///
    /// Calls the callback for each completed operation.
    pub fn process_completions<F>(&mut self, mut callback: F) -> usize
    where
        F: FnMut(u64, i32), // (user_data, result)
    {
        let mut count = 0;

        while let Some(cqe) = self.ring.completion().next() {
            callback(cqe.user_data(), cqe.result());
            count += 1;
        }

        count
    }

    /// Get the number of pending submissions
    pub fn pending_submissions(&self) -> usize {
        self.ring.submission().len()
    }

    /// Get the number of completions ready
    pub fn ready_completions(&self) -> usize {
        self.ring.completion().len()
    }
}

// =============================================================================
// COMPLETION HANDLER
// =============================================================================

/// Completion result
#[derive(Debug, Clone, Copy)]
pub struct Completion {
    /// User data from submission
    pub user_data: u64,
    /// Result (positive = success/bytes, negative = -errno)
    pub result: i32,
}

impl Completion {
    /// Check if the operation succeeded
    pub fn is_ok(&self) -> bool {
        self.result >= 0
    }

    /// Check if the operation failed
    pub fn is_err(&self) -> bool {
        self.result < 0
    }

    /// Get the error code (if failed)
    pub fn error(&self) -> Option<i32> {
        if self.result < 0 {
            Some(-self.result)
        } else {
            None
        }
    }

    /// Get bytes transferred (if success)
    pub fn bytes(&self) -> Option<usize> {
        if self.result >= 0 {
            Some(self.result as usize)
        } else {
            None
        }
    }

    /// Get the new file descriptor (for accept)
    pub fn fd(&self) -> Option<RawFd> {
        if self.result >= 0 {
            Some(self.result)
        } else {
            None
        }
    }
}

// =============================================================================
// BATCH OPERATIONS
// =============================================================================

/// Operation type for batching
#[derive(Debug, Clone)]
pub enum UringOp {
    Read {
        fd: RawFd,
        buf_index: u16,
        offset: u64,
    },
    Write {
        fd: RawFd,
        buf_index: u16,
        offset: u64,
        len: usize,
    },
    Send {
        fd: RawFd,
        data: Vec<u8>,
    },
    Recv {
        fd: RawFd,
        len: usize,
    },
    Accept {
        listen_fd: RawFd,
    },
    Connect {
        fd: RawFd,
        addr: libc::sockaddr_in,
    },
    Close {
        fd: RawFd,
    },
    Nop,
}

/// Batch submission builder
pub struct UringBatch {
    ops: Vec<(UringOp, u64)>, // (operation, user_data)
}

impl UringBatch {
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            ops: Vec::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, op: UringOp, user_data: u64) {
        self.ops.push((op, user_data));
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn clear(&mut self) {
        self.ops.clear();
    }
}

impl Default for UringBatch {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uring_creation() {
        if let Ok(ring) = UringIo::new_standard(32) {
            assert_eq!(ring.pending_submissions(), 0);
        }
        // May fail if io_uring not supported - that's OK
    }

    #[test]
    fn test_completion() {
        let success = Completion {
            user_data: 42,
            result: 100,
        };
        assert!(success.is_ok());
        assert_eq!(success.bytes(), Some(100));

        let failure = Completion {
            user_data: 43,
            result: -libc::EAGAIN,
        };
        assert!(failure.is_err());
        assert_eq!(failure.error(), Some(libc::EAGAIN));
    }
}
