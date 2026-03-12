//! Zero-copy buffer management with ring buffers and slab allocators.
//!
//! This module provides high-performance buffer management designed to minimize
//! memory allocations and copies in the hot path.

use bytes::{BufMut, Bytes, BytesMut};
use crossbeam_queue::ArrayQueue;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::{BufferAllocator, BufferStats};

// =============================================================================
// RING BUFFER
// =============================================================================

/// Lock-free ring buffer for zero-copy I/O
///
/// Uses a pre-allocated circular buffer with atomic head/tail pointers.
/// Suitable for single-producer single-consumer scenarios.
pub struct RingBuffer {
    /// Backing storage
    data: Box<[u8]>,
    /// Capacity (power of 2 for fast modulo)
    capacity: usize,
    /// Mask for fast modulo (capacity - 1)
    mask: usize,
    /// Read position
    read_pos: AtomicUsize,
    /// Write position  
    write_pos: AtomicUsize,
}

impl RingBuffer {
    /// Create a new ring buffer with the given capacity (rounded up to power of 2)
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.next_power_of_two();
        Self {
            data: vec![0u8; capacity].into_boxed_slice(),
            capacity,
            mask: capacity - 1,
            read_pos: AtomicUsize::new(0),
            write_pos: AtomicUsize::new(0),
        }
    }

    /// Get available space for writing
    pub fn write_available(&self) -> usize {
        let read = self.read_pos.load(Ordering::Acquire);
        let write = self.write_pos.load(Ordering::Acquire);
        self.capacity - (write.wrapping_sub(read))
    }

    /// Get available data for reading
    pub fn read_available(&self) -> usize {
        let read = self.read_pos.load(Ordering::Acquire);
        let write = self.write_pos.load(Ordering::Acquire);
        write.wrapping_sub(read)
    }

    /// Write data to the buffer (returns bytes written)
    pub fn write(&self, data: &[u8]) -> usize {
        let available = self.write_available();
        let to_write = data.len().min(available);

        if to_write == 0 {
            return 0;
        }

        let write = self.write_pos.load(Ordering::Acquire);
        let start = write & self.mask;

        // Handle wrap-around
        if start + to_write <= self.capacity {
            // Contiguous write
            // SAFETY: We have exclusive write access to [start, start+to_write)
            unsafe {
                let dst = self.data.as_ptr().add(start) as *mut u8;
                std::ptr::copy_nonoverlapping(data.as_ptr(), dst, to_write);
            }
        } else {
            // Split write at boundary
            let first_part = self.capacity - start;
            unsafe {
                let dst1 = self.data.as_ptr().add(start) as *mut u8;
                std::ptr::copy_nonoverlapping(data.as_ptr(), dst1, first_part);

                let dst2 = self.data.as_ptr() as *mut u8;
                std::ptr::copy_nonoverlapping(
                    data.as_ptr().add(first_part),
                    dst2,
                    to_write - first_part,
                );
            }
        }

        self.write_pos
            .store(write.wrapping_add(to_write), Ordering::Release);
        to_write
    }

    /// Read data from the buffer (returns bytes read)
    pub fn read(&self, buf: &mut [u8]) -> usize {
        let available = self.read_available();
        let to_read = buf.len().min(available);

        if to_read == 0 {
            return 0;
        }

        let read = self.read_pos.load(Ordering::Acquire);
        let start = read & self.mask;

        // Handle wrap-around
        if start + to_read <= self.capacity {
            // Contiguous read
            buf[..to_read].copy_from_slice(&self.data[start..start + to_read]);
        } else {
            // Split read at boundary
            let first_part = self.capacity - start;
            buf[..first_part].copy_from_slice(&self.data[start..]);
            buf[first_part..to_read].copy_from_slice(&self.data[..to_read - first_part]);
        }

        self.read_pos
            .store(read.wrapping_add(to_read), Ordering::Release);
        to_read
    }

    /// Peek at data without consuming it
    pub fn peek(&self, buf: &mut [u8]) -> usize {
        let available = self.read_available();
        let to_read = buf.len().min(available);

        if to_read == 0 {
            return 0;
        }

        let read = self.read_pos.load(Ordering::Acquire);
        let start = read & self.mask;

        if start + to_read <= self.capacity {
            buf[..to_read].copy_from_slice(&self.data[start..start + to_read]);
        } else {
            let first_part = self.capacity - start;
            buf[..first_part].copy_from_slice(&self.data[start..]);
            buf[first_part..to_read].copy_from_slice(&self.data[..to_read - first_part]);
        }

        to_read
    }

    /// Skip n bytes in the read buffer
    pub fn skip(&self, n: usize) -> usize {
        let available = self.read_available();
        let to_skip = n.min(available);

        let read = self.read_pos.load(Ordering::Acquire);
        self.read_pos
            .store(read.wrapping_add(to_skip), Ordering::Release);

        to_skip
    }

    /// Clear the buffer
    pub fn clear(&self) {
        let write = self.write_pos.load(Ordering::Acquire);
        self.read_pos.store(write, Ordering::Release);
    }

    /// Get capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.read_available() == 0
    }

    /// Check if full
    pub fn is_full(&self) -> bool {
        self.write_available() == 0
    }
}

// SAFETY: RingBuffer uses atomic operations for synchronization
unsafe impl Send for RingBuffer {}
unsafe impl Sync for RingBuffer {}

// =============================================================================
// SLAB ALLOCATOR
// =============================================================================

/// Fixed-size buffer pool for efficient allocation
///
/// Pre-allocates a pool of fixed-size buffers that can be quickly borrowed
/// and returned without heap allocation.
pub struct SlabAllocator {
    /// Size of each buffer in the slab
    buffer_size: usize,
    /// Pool of available buffers
    pool: ArrayQueue<BytesMut>,
    /// Statistics
    allocated: AtomicU64,
    deallocated: AtomicU64,
    pool_hits: AtomicU64,
    pool_misses: AtomicU64,
}

impl SlabAllocator {
    /// Create a new slab allocator
    pub fn new(buffer_size: usize, pool_capacity: usize) -> Self {
        let pool = ArrayQueue::new(pool_capacity);

        // Pre-populate the pool
        for _ in 0..pool_capacity {
            let buf = BytesMut::with_capacity(buffer_size);
            let _ = pool.push(buf);
        }

        Self {
            buffer_size,
            pool,
            allocated: AtomicU64::new(pool_capacity as u64),
            deallocated: AtomicU64::new(0),
            pool_hits: AtomicU64::new(0),
            pool_misses: AtomicU64::new(0),
        }
    }

    /// Borrow a buffer from the pool
    pub fn borrow(&self) -> BytesMut {
        if let Some(mut buf) = self.pool.pop() {
            self.pool_hits.fetch_add(1, Ordering::Relaxed);
            buf.clear();
            buf
        } else {
            self.pool_misses.fetch_add(1, Ordering::Relaxed);
            self.allocated.fetch_add(1, Ordering::Relaxed);
            BytesMut::with_capacity(self.buffer_size)
        }
    }

    /// Return a buffer to the pool
    pub fn return_buffer(&self, mut buf: BytesMut) {
        self.deallocated.fetch_add(1, Ordering::Relaxed);

        // Only return to pool if it's the right size and pool isn't full
        if buf.capacity() >= self.buffer_size {
            buf.clear();
            buf.reserve(self.buffer_size.saturating_sub(buf.capacity()));
            let _ = self.pool.push(buf);
        }
        // Otherwise just drop it
    }

    /// Get buffer size
    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    /// Get pool statistics
    pub fn stats(&self) -> SlabStats {
        SlabStats {
            buffer_size: self.buffer_size,
            pool_capacity: self.pool.capacity(),
            pool_available: self.pool.len(),
            allocated: self.allocated.load(Ordering::Relaxed),
            deallocated: self.deallocated.load(Ordering::Relaxed),
            pool_hits: self.pool_hits.load(Ordering::Relaxed),
            pool_misses: self.pool_misses.load(Ordering::Relaxed),
        }
    }
}

/// Slab allocator statistics
#[derive(Debug, Clone)]
pub struct SlabStats {
    pub buffer_size: usize,
    pub pool_capacity: usize,
    pub pool_available: usize,
    pub allocated: u64,
    pub deallocated: u64,
    pub pool_hits: u64,
    pub pool_misses: u64,
}

// =============================================================================
// HIERARCHICAL ALLOCATOR
// =============================================================================

/// Hierarchical buffer allocator with multiple size classes
///
/// Uses different slab allocators for different size ranges to minimize
/// internal fragmentation while maintaining fast allocation.
pub struct HierarchicalAllocator {
    /// Small buffers (1KB)
    small: SlabAllocator,
    /// Medium buffers (8KB)
    medium: SlabAllocator,
    /// Large buffers (64KB)
    large: SlabAllocator,
    /// Huge buffers (512KB)
    huge: SlabAllocator,
    /// Fallback for very large allocations
    fallback_count: AtomicU64,
}

impl HierarchicalAllocator {
    /// Create a new hierarchical allocator
    pub fn new() -> Self {
        Self {
            small: SlabAllocator::new(1024, 4096),
            medium: SlabAllocator::new(8 * 1024, 1024),
            large: SlabAllocator::new(64 * 1024, 256),
            huge: SlabAllocator::new(512 * 1024, 64),
            fallback_count: AtomicU64::new(0),
        }
    }

    /// Create with custom sizes
    #[allow(clippy::too_many_arguments)]
    pub fn with_sizes(
        small_size: usize,
        small_count: usize,
        medium_size: usize,
        medium_count: usize,
        large_size: usize,
        large_count: usize,
        huge_size: usize,
        huge_count: usize,
    ) -> Self {
        Self {
            small: SlabAllocator::new(small_size, small_count),
            medium: SlabAllocator::new(medium_size, medium_count),
            large: SlabAllocator::new(large_size, large_count),
            huge: SlabAllocator::new(huge_size, huge_count),
            fallback_count: AtomicU64::new(0),
        }
    }

    /// Allocate a buffer
    fn alloc(&self, size: usize) -> BytesMut {
        if size <= 1024 {
            self.small.borrow()
        } else if size <= 8 * 1024 {
            self.medium.borrow()
        } else if size <= 64 * 1024 {
            self.large.borrow()
        } else if size <= 512 * 1024 {
            self.huge.borrow()
        } else {
            self.fallback_count.fetch_add(1, Ordering::Relaxed);
            BytesMut::with_capacity(size)
        }
    }

    /// Return a buffer to the appropriate pool
    fn dealloc(&self, buf: BytesMut) {
        let cap = buf.capacity();
        if cap <= 1024 {
            self.small.return_buffer(buf);
        } else if cap <= 8 * 1024 {
            self.medium.return_buffer(buf);
        } else if cap <= 64 * 1024 {
            self.large.return_buffer(buf);
        } else if cap <= 512 * 1024 {
            self.huge.return_buffer(buf);
        }
        // Very large buffers just get dropped
    }

    /// Get statistics for all pools
    pub fn all_stats(&self) -> HierarchicalStats {
        HierarchicalStats {
            small: self.small.stats(),
            medium: self.medium.stats(),
            large: self.large.stats(),
            huge: self.huge.stats(),
            fallback_allocations: self.fallback_count.load(Ordering::Relaxed),
        }
    }
}

impl Default for HierarchicalAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Hierarchical allocator statistics
#[derive(Debug, Clone)]
pub struct HierarchicalStats {
    pub small: SlabStats,
    pub medium: SlabStats,
    pub large: SlabStats,
    pub huge: SlabStats,
    pub fallback_allocations: u64,
}

impl BufferAllocator for HierarchicalAllocator {
    fn allocate(&self, size: usize) -> BytesMut {
        self.alloc(size)
    }

    fn deallocate(&self, buffer: BytesMut) {
        self.dealloc(buffer);
    }

    fn stats(&self) -> BufferStats {
        let all = self.all_stats();
        BufferStats {
            allocated: all.small.allocated
                + all.medium.allocated
                + all.large.allocated
                + all.huge.allocated
                + all.fallback_allocations,
            deallocated: all.small.deallocated
                + all.medium.deallocated
                + all.large.deallocated
                + all.huge.deallocated,
            in_use: (all.small.allocated
                + all.medium.allocated
                + all.large.allocated
                + all.huge.allocated)
                .saturating_sub(
                    all.small.deallocated
                        + all.medium.deallocated
                        + all.large.deallocated
                        + all.huge.deallocated,
                ),
            pool_size: (all.small.pool_capacity
                + all.medium.pool_capacity
                + all.large.pool_capacity
                + all.huge.pool_capacity) as u64,
        }
    }
}

// =============================================================================
// VECTORED BUFFER
// =============================================================================

/// Buffer that supports vectored (scatter-gather) I/O
///
/// Allows building a message from multiple non-contiguous chunks
/// that can be written with a single syscall using writev.
pub struct VectoredBuffer {
    /// List of buffer chunks
    chunks: Vec<Bytes>,
    /// Total length
    total_len: usize,
}

impl VectoredBuffer {
    /// Create a new empty vectored buffer
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            total_len: 0,
        }
    }

    /// Create with pre-allocated capacity
    pub fn with_capacity(chunk_count: usize) -> Self {
        Self {
            chunks: Vec::with_capacity(chunk_count),
            total_len: 0,
        }
    }

    /// Add a chunk to the buffer
    pub fn push(&mut self, chunk: Bytes) {
        self.total_len += chunk.len();
        self.chunks.push(chunk);
    }

    /// Add a chunk from a slice (copies data)
    pub fn push_slice(&mut self, data: &[u8]) {
        self.push(Bytes::copy_from_slice(data));
    }

    /// Get total length
    pub fn len(&self) -> usize {
        self.total_len
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.total_len == 0
    }

    /// Get number of chunks
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Get chunks as IO slices for vectored write
    pub fn as_io_slices(&self) -> Vec<std::io::IoSlice<'_>> {
        self.chunks
            .iter()
            .map(|c| std::io::IoSlice::new(c))
            .collect()
    }

    /// Consume and return chunks
    pub fn into_chunks(self) -> Vec<Bytes> {
        self.chunks
    }

    /// Flatten into a single contiguous buffer
    pub fn flatten(&self) -> Bytes {
        if self.chunks.len() == 1 {
            return self.chunks[0].clone();
        }

        let mut buf = BytesMut::with_capacity(self.total_len);
        for chunk in &self.chunks {
            buf.put_slice(chunk);
        }
        buf.freeze()
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.total_len = 0;
    }
}

impl Default for VectoredBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Bytes> for VectoredBuffer {
    fn from(bytes: Bytes) -> Self {
        let mut buf = Self::new();
        buf.push(bytes);
        buf
    }
}

impl From<Vec<Bytes>> for VectoredBuffer {
    fn from(chunks: Vec<Bytes>) -> Self {
        let total_len = chunks.iter().map(|c| c.len()).sum();
        Self { chunks, total_len }
    }
}

// =============================================================================
// MEMORY-MAPPED BUFFER
// =============================================================================

/// Memory-mapped buffer for large file transfers
///
/// Uses mmap to avoid copying large files through userspace.
#[cfg(unix)]
pub struct MmapBuffer {
    mmap: memmap2::Mmap,
}

#[cfg(unix)]
impl MmapBuffer {
    /// Create a read-only memory-mapped buffer from a file
    pub fn from_file(file: &std::fs::File) -> std::io::Result<Self> {
        // SAFETY: File must not be modified while mapped
        let mmap = unsafe { memmap2::Mmap::map(file)? };
        Ok(Self { mmap })
    }

    /// Get the buffer as bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.mmap
    }

    /// Get length
    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.mmap.is_empty()
    }

    /// Convert to Bytes (may copy if refcount > 0)
    pub fn to_bytes(&self) -> Bytes {
        Bytes::copy_from_slice(&self.mmap)
    }
}

/// Mutable memory-mapped buffer
#[cfg(unix)]
pub struct MmapMutBuffer {
    mmap: memmap2::MmapMut,
}

#[cfg(unix)]
impl MmapMutBuffer {
    /// Create a new anonymous memory-mapped buffer
    pub fn anonymous(len: usize) -> std::io::Result<Self> {
        let mmap = memmap2::MmapMut::map_anon(len)?;
        Ok(Self { mmap })
    }

    /// Create a read-write memory-mapped buffer from a file
    pub fn from_file(file: &std::fs::File) -> std::io::Result<Self> {
        // SAFETY: Exclusive access to file required
        let mmap = unsafe { memmap2::MmapMut::map_mut(file)? };
        Ok(Self { mmap })
    }

    /// Get the buffer as bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.mmap
    }

    /// Get the buffer as mutable bytes
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.mmap
    }

    /// Get length
    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.mmap.is_empty()
    }

    /// Flush changes to disk
    pub fn flush(&self) -> std::io::Result<()> {
        self.mmap.flush()
    }

    /// Async flush
    pub fn flush_async(&self) -> std::io::Result<()> {
        self.mmap.flush_async()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_basic() {
        let ring = RingBuffer::new(1024);

        assert_eq!(ring.capacity(), 1024);
        assert!(ring.is_empty());
        assert_eq!(ring.write_available(), 1024);

        // Write some data
        let data = b"Hello, World!";
        let written = ring.write(data);
        assert_eq!(written, data.len());
        assert!(!ring.is_empty());

        // Read it back
        let mut buf = [0u8; 64];
        let read = ring.read(&mut buf);
        assert_eq!(read, data.len());
        assert_eq!(&buf[..read], data);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_ring_buffer_wrap_around() {
        let ring = RingBuffer::new(16); // Small buffer to force wrap

        // Write 12 bytes
        let written = ring.write(b"Hello World!");
        assert_eq!(written, 12);

        // Read 8 bytes
        let mut buf = [0u8; 8];
        ring.read(&mut buf);
        assert_eq!(&buf, b"Hello Wo");

        // Write 10 more bytes (wraps around)
        let written = ring.write(b"ABCDEFGHIJ");
        assert_eq!(written, 10);

        // Read all
        let mut buf = [0u8; 14];
        let read = ring.read(&mut buf);
        assert_eq!(read, 14);
        assert_eq!(&buf[..4], b"rld!");
        assert_eq!(&buf[4..], b"ABCDEFGHIJ");
    }

    #[test]
    fn test_slab_allocator() {
        let slab = SlabAllocator::new(4096, 16);

        // Borrow several buffers
        let buf1 = slab.borrow();
        let buf2 = slab.borrow();

        assert!(buf1.capacity() >= 4096);
        assert!(buf2.capacity() >= 4096);

        // Return them
        slab.return_buffer(buf1);
        slab.return_buffer(buf2);

        let stats = slab.stats();
        assert!(stats.pool_hits >= 2);
    }

    #[test]
    fn test_hierarchical_allocator() {
        let alloc = HierarchicalAllocator::new();

        // Allocate various sizes
        let small = alloc.allocate(100);
        let medium = alloc.allocate(2000);
        let large = alloc.allocate(20000);

        assert!(small.capacity() >= 100);
        assert!(medium.capacity() >= 2000);
        assert!(large.capacity() >= 20000);

        // Deallocate
        alloc.deallocate(small);
        alloc.deallocate(medium);
        alloc.deallocate(large);
    }

    #[test]
    fn test_vectored_buffer() {
        let mut vec_buf = VectoredBuffer::new();

        vec_buf.push(Bytes::from_static(b"Hello, "));
        vec_buf.push(Bytes::from_static(b"World!"));

        assert_eq!(vec_buf.len(), 13);
        assert_eq!(vec_buf.chunk_count(), 2);

        let flat = vec_buf.flatten();
        assert_eq!(&flat[..], b"Hello, World!");
    }
}
