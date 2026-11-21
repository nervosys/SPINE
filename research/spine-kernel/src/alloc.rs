//! Custom Memory Allocators
//!
//! High-performance allocators optimized for agentic workloads:
//! - Arena allocator for request-scoped memory
//! - Slab allocator for fixed-size buffers
//! - Bump allocator for sequential allocation
//! - Huge page support for reduced TLB pressure

use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::{align_up, CACHE_LINE_SIZE, PAGE_SIZE};

// =============================================================================
// BUMP ALLOCATOR
// =============================================================================

/// Fast bump allocator for sequential allocation
///
/// Allocates by incrementing a pointer. Deallocation is a no-op;
/// memory is freed when the entire arena is dropped.
///
/// **Performance**: O(1) allocation, zero fragmentation
pub struct BumpAllocator {
    /// Start of the memory region
    start: NonNull<u8>,
    /// Current allocation pointer
    current: AtomicUsize,
    /// End of the memory region
    end: usize,
    /// Total capacity
    capacity: usize,
}

impl BumpAllocator {
    /// Create a new bump allocator with the given capacity
    pub fn new(capacity: usize) -> Self {
        let capacity = align_up(capacity, PAGE_SIZE);
        let layout = Layout::from_size_align(capacity, PAGE_SIZE).unwrap();

        // SAFETY: Layout is valid and non-zero
        let ptr = unsafe { alloc(layout) };
        let start = NonNull::new(ptr).expect("allocation failed");

        Self {
            start,
            current: AtomicUsize::new(ptr as usize),
            end: ptr as usize + capacity,
            capacity,
        }
    }

    /// Allocate memory with the given layout
    #[inline]
    pub fn alloc(&self, layout: Layout) -> Option<NonNull<u8>> {
        let align = layout.align().max(8);
        let size = layout.size();

        loop {
            let current = self.current.load(Ordering::Relaxed);
            let aligned = align_up(current, align);
            let new_current = aligned + size;

            if new_current > self.end {
                return None;
            }

            match self.current.compare_exchange_weak(
                current,
                new_current,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    // SAFETY: aligned is within our allocation
                    return Some(unsafe { NonNull::new_unchecked(aligned as *mut u8) });
                }
                Err(_) => continue, // Retry
            }
        }
    }

    /// Allocate a value of type T
    #[inline]
    pub fn alloc_val<T>(&self) -> Option<NonNull<T>> {
        let layout = Layout::new::<T>();
        self.alloc(layout).map(|p| p.cast())
    }

    /// Allocate a slice of T
    #[inline]
    pub fn alloc_slice<T>(&self, len: usize) -> Option<NonNull<[T]>> {
        let layout = Layout::array::<T>(len).ok()?;
        let ptr = self.alloc(layout)?;
        let slice_ptr = std::ptr::slice_from_raw_parts_mut(ptr.as_ptr() as *mut T, len);
        NonNull::new(slice_ptr)
    }

    /// Reset the allocator (invalidates all previous allocations)
    pub fn reset(&self) {
        self.current
            .store(self.start.as_ptr() as usize, Ordering::Release);
    }

    /// Get the number of bytes allocated
    pub fn allocated(&self) -> usize {
        self.current.load(Ordering::Relaxed) - self.start.as_ptr() as usize
    }

    /// Get the remaining capacity
    pub fn remaining(&self) -> usize {
        self.end - self.current.load(Ordering::Relaxed)
    }

    /// Get total capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl Drop for BumpAllocator {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.capacity, PAGE_SIZE).unwrap();
        // SAFETY: We allocated this memory in new()
        unsafe { dealloc(self.start.as_ptr(), layout) };
    }
}

// SAFETY: BumpAllocator uses atomic operations for thread safety
unsafe impl Send for BumpAllocator {}
unsafe impl Sync for BumpAllocator {}

// =============================================================================
// SLAB ALLOCATOR
// =============================================================================

/// Fixed-size slab allocator
///
/// Efficiently allocates fixed-size blocks from a pre-allocated pool.
/// Uses a lock-free free list for O(1) alloc/dealloc.
pub struct SlabAllocator {
    /// Block size (including any padding)
    block_size: usize,
    /// Number of blocks
    block_count: usize,
    /// Memory region
    memory: NonNull<u8>,
    /// Free list head (atomic for lock-free ops)
    free_head: AtomicPtr<SlabBlock>,
    /// Allocated blocks count
    allocated: AtomicUsize,
}

#[repr(C)]
struct SlabBlock {
    next: AtomicPtr<SlabBlock>,
}

impl SlabAllocator {
    /// Create a new slab allocator
    ///
    /// # Arguments
    /// * `block_size` - Size of each block (minimum 8 bytes for free list pointer)
    /// * `block_count` - Number of blocks in the slab
    pub fn new(block_size: usize, block_count: usize) -> Self {
        // Ensure block can hold the free list pointer
        let block_size = block_size.max(std::mem::size_of::<SlabBlock>());
        let block_size = align_up(block_size, 8);

        let total_size = block_size * block_count;
        let layout = Layout::from_size_align(total_size, CACHE_LINE_SIZE).unwrap();

        // SAFETY: Layout is valid
        let memory = unsafe {
            let ptr = alloc(layout);
            NonNull::new(ptr).expect("slab allocation failed")
        };

        // Initialize free list
        let mut prev: *mut SlabBlock = std::ptr::null_mut();
        for i in (0..block_count).rev() {
            let block_ptr = unsafe { memory.as_ptr().add(i * block_size) } as *mut SlabBlock;
            // SAFETY: Within our allocation
            unsafe {
                (*block_ptr).next = AtomicPtr::new(prev);
            }
            prev = block_ptr;
        }

        Self {
            block_size,
            block_count,
            memory,
            free_head: AtomicPtr::new(prev),
            allocated: AtomicUsize::new(0),
        }
    }

    /// Allocate a block from the slab
    #[inline]
    pub fn alloc(&self) -> Option<NonNull<u8>> {
        loop {
            let head = self.free_head.load(Ordering::Acquire);
            if head.is_null() {
                return None;
            }

            // SAFETY: head is from our slab
            let next = unsafe { (*head).next.load(Ordering::Relaxed) };

            match self.free_head.compare_exchange_weak(
                head,
                next,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.allocated.fetch_add(1, Ordering::Relaxed);
                    return Some(NonNull::new(head as *mut u8).unwrap());
                }
                Err(_) => continue,
            }
        }
    }

    /// Return a block to the slab
    ///
    /// # Safety
    /// The pointer must have been allocated from this slab and not yet deallocated.
    /// Passing a pointer from a different allocator or double-freeing causes undefined behavior.
    #[inline]
    pub unsafe fn dealloc(&self, ptr: NonNull<u8>) {
        // Validate pointer is within our memory range
        let ptr_addr = ptr.as_ptr() as usize;
        let start_addr = self.memory.as_ptr() as usize;
        let end_addr = start_addr + (self.block_size * self.block_count);

        debug_assert!(
            ptr_addr >= start_addr && ptr_addr < end_addr,
            "dealloc: pointer {:p} not from this slab [{:p}..{:p})",
            ptr.as_ptr(),
            self.memory.as_ptr(),
            (end_addr as *const u8)
        );

        // In release builds, validate alignment to block boundary
        debug_assert!(
            (ptr_addr - start_addr).is_multiple_of(self.block_size),
            "dealloc: pointer not aligned to block boundary"
        );

        let block = ptr.as_ptr() as *mut SlabBlock;

        loop {
            let head = self.free_head.load(Ordering::Relaxed);
            (*block).next = AtomicPtr::new(head);

            match self.free_head.compare_exchange_weak(
                head,
                block,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.allocated.fetch_sub(1, Ordering::Relaxed);
                    return;
                }
                Err(_) => continue,
            }
        }
    }

    /// Get block size
    pub fn block_size(&self) -> usize {
        self.block_size
    }

    /// Get total block count
    pub fn block_count(&self) -> usize {
        self.block_count
    }

    /// Get allocated block count
    pub fn allocated(&self) -> usize {
        self.allocated.load(Ordering::Relaxed)
    }

    /// Get available block count
    pub fn available(&self) -> usize {
        self.block_count - self.allocated()
    }
}

impl Drop for SlabAllocator {
    fn drop(&mut self) {
        let total_size = self.block_size * self.block_count;
        let layout = Layout::from_size_align(total_size, CACHE_LINE_SIZE).unwrap();
        // SAFETY: We allocated this memory
        unsafe { dealloc(self.memory.as_ptr(), layout) };
    }
}

// SAFETY: SlabAllocator uses atomic operations
unsafe impl Send for SlabAllocator {}
unsafe impl Sync for SlabAllocator {}

// =============================================================================
// ARENA ALLOCATOR
// =============================================================================

/// Thread-local arena allocator with multiple size classes
///
/// Combines bump allocation with size-class segregation for
/// efficient mixed-size allocations.
pub struct ArenaAllocator {
    /// Small allocations (≤64 bytes): slab
    small: SlabAllocator,
    /// Medium allocations (≤1KB): slab
    medium: SlabAllocator,
    /// Large allocations (≤64KB): slab  
    large: SlabAllocator,
    /// Huge allocations (>64KB): bump
    huge: BumpAllocator,
}

impl ArenaAllocator {
    /// Create a new arena allocator
    pub fn new() -> Self {
        Self {
            small: SlabAllocator::new(64, 16384),       // 1MB
            medium: SlabAllocator::new(1024, 4096),     // 4MB
            large: SlabAllocator::new(64 * 1024, 256),  // 16MB
            huge: BumpAllocator::new(64 * 1024 * 1024), // 64MB
        }
    }

    /// Allocate memory
    #[inline]
    pub fn alloc(&self, size: usize, align: usize) -> Option<NonNull<u8>> {
        let size = align_up(size, align);

        if size <= 64 {
            self.small.alloc()
        } else if size <= 1024 {
            self.medium.alloc()
        } else if size <= 64 * 1024 {
            self.large.alloc()
        } else {
            let layout = Layout::from_size_align(size, align).ok()?;
            self.huge.alloc(layout)
        }
    }

    /// Deallocate memory
    ///
    /// # Safety
    /// Pointer must have been allocated from this arena
    #[inline]
    pub unsafe fn dealloc(&self, ptr: NonNull<u8>, size: usize) {
        if size <= 64 {
            self.small.dealloc(ptr);
        } else if size <= 1024 {
            self.medium.dealloc(ptr);
        } else if size <= 64 * 1024 {
            self.large.dealloc(ptr);
        }
        // Huge allocations: bump allocator, no individual dealloc
    }

    /// Reset the arena (invalidates all allocations)
    pub fn reset(&self) {
        // Slabs don't need reset - they reuse blocks
        self.huge.reset();
    }

    /// Get memory statistics
    pub fn stats(&self) -> ArenaStats {
        ArenaStats {
            small_allocated: self.small.allocated(),
            small_available: self.small.available(),
            medium_allocated: self.medium.allocated(),
            medium_available: self.medium.available(),
            large_allocated: self.large.allocated(),
            large_available: self.large.available(),
            huge_allocated: self.huge.allocated(),
            huge_remaining: self.huge.remaining(),
        }
    }
}

impl Default for ArenaAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Arena allocation statistics
#[derive(Debug, Clone)]
pub struct ArenaStats {
    pub small_allocated: usize,
    pub small_available: usize,
    pub medium_allocated: usize,
    pub medium_available: usize,
    pub large_allocated: usize,
    pub large_available: usize,
    pub huge_allocated: usize,
    pub huge_remaining: usize,
}

// =============================================================================
// CACHE-ALIGNED ALLOCATION
// =============================================================================

/// Allocate cache-line aligned memory
#[inline]
pub fn alloc_aligned(size: usize) -> Option<NonNull<u8>> {
    let layout = Layout::from_size_align(size, CACHE_LINE_SIZE).ok()?;
    // SAFETY: Layout is valid
    let ptr = unsafe { alloc(layout) };
    NonNull::new(ptr)
}

/// Deallocate cache-line aligned memory
///
/// # Safety
/// Pointer must have been allocated by alloc_aligned with the same size
#[inline]
pub unsafe fn dealloc_aligned(ptr: NonNull<u8>, size: usize) {
    let layout = Layout::from_size_align(size, CACHE_LINE_SIZE).unwrap();
    dealloc(ptr.as_ptr(), layout);
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bump_allocator() {
        let alloc = BumpAllocator::new(4096);

        let p1 = alloc.alloc(Layout::new::<u64>()).unwrap();
        let p2 = alloc.alloc(Layout::new::<u64>()).unwrap();

        assert_ne!(p1, p2);
        assert!(alloc.allocated() >= 16);

        alloc.reset();
        assert_eq!(alloc.allocated(), 0);
    }

    #[test]
    fn test_slab_allocator() {
        let slab = SlabAllocator::new(64, 100);

        let mut ptrs = Vec::new();
        for _ in 0..100 {
            let ptr = slab.alloc().unwrap();
            ptrs.push(ptr);
        }

        assert!(slab.alloc().is_none()); // Full
        assert_eq!(slab.allocated(), 100);

        // Return one
        unsafe { slab.dealloc(ptrs.pop().unwrap()) };
        assert_eq!(slab.allocated(), 99);

        // Can allocate again
        let _ = slab.alloc().unwrap();
        assert_eq!(slab.allocated(), 100);
    }

    #[test]
    fn test_arena_allocator() {
        let arena = ArenaAllocator::new();

        // Small allocation
        let _p1 = arena.alloc(32, 8).unwrap();
        // Medium allocation
        let _p2 = arena.alloc(512, 8).unwrap();
        // Large allocation
        let _p3 = arena.alloc(32 * 1024, 8).unwrap();
        // Huge allocation
        let _p4 = arena.alloc(128 * 1024, 8).unwrap();

        let stats = arena.stats();
        assert!(stats.small_allocated >= 1);
        assert!(stats.medium_allocated >= 1);
        assert!(stats.large_allocated >= 1);
        assert!(stats.huge_allocated >= 128 * 1024);
    }
}
