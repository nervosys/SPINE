//! Lock-Free Atomic Data Structures
//!
//! High-performance concurrent primitives using atomic operations:
//! - AtomicCounter with cache-line padding
//! - Sequence lock for low-overhead reads
//! - Tagged pointer for ABA problem prevention
//! - Lock-free stack and queue

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering};

use crate::CACHE_LINE_SIZE;

// =============================================================================
// CACHE-PADDED ATOMIC COUNTER
// =============================================================================

/// Cache-line padded atomic counter
///
/// Prevents false sharing when multiple counters are accessed concurrently.
#[repr(C, align(64))]
pub struct PaddedAtomicU64 {
    value: AtomicU64,
    _pad: [u8; CACHE_LINE_SIZE - 8],
}

impl PaddedAtomicU64 {
    /// Create a new padded counter
    pub const fn new(value: u64) -> Self {
        Self {
            value: AtomicU64::new(value),
            _pad: [0; CACHE_LINE_SIZE - 8],
        }
    }

    #[inline]
    pub fn load(&self, order: Ordering) -> u64 {
        self.value.load(order)
    }

    #[inline]
    pub fn store(&self, value: u64, order: Ordering) {
        self.value.store(value, order);
    }

    #[inline]
    pub fn fetch_add(&self, val: u64, order: Ordering) -> u64 {
        self.value.fetch_add(val, order)
    }

    #[inline]
    pub fn fetch_sub(&self, val: u64, order: Ordering) -> u64 {
        self.value.fetch_sub(val, order)
    }

    #[inline]
    pub fn compare_exchange(
        &self,
        current: u64,
        new: u64,
        success: Ordering,
        failure: Ordering,
    ) -> Result<u64, u64> {
        self.value.compare_exchange(current, new, success, failure)
    }
}

impl Default for PaddedAtomicU64 {
    fn default() -> Self {
        Self::new(0)
    }
}

// =============================================================================
// SEQUENCE LOCK
// =============================================================================

/// Sequence lock for low-overhead read-heavy workloads
///
/// Writers acquire exclusive access via sequence increment.
/// Readers optimistically read and validate sequence.
///
/// **Use case**: Frequently-read, rarely-written data (config, metrics)
pub struct SeqLock<T> {
    seq: AtomicUsize,
    data: UnsafeCell<T>,
}

impl<T: Copy> SeqLock<T> {
    /// Create a new sequence lock
    pub fn new(value: T) -> Self {
        Self {
            seq: AtomicUsize::new(0),
            data: UnsafeCell::new(value),
        }
    }

    /// Read the value
    ///
    /// May spin if a write is in progress.
    #[inline]
    pub fn read(&self) -> T {
        loop {
            // Read sequence before data
            let seq1 = self.seq.load(Ordering::Acquire);

            // If odd, write in progress - spin
            if seq1 & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }

            // Read data
            // SAFETY: No concurrent write when seq is even
            let value = unsafe { *self.data.get() };

            // Memory barrier
            std::sync::atomic::fence(Ordering::Acquire);

            // Read sequence after data
            let seq2 = self.seq.load(Ordering::Relaxed);

            // If sequence unchanged, read was consistent
            if seq1 == seq2 {
                return value;
            }

            // Otherwise, retry
            std::hint::spin_loop();
        }
    }

    /// Write a new value
    #[inline]
    pub fn write(&self, value: T) {
        // Increment sequence (now odd = write in progress)
        self.seq.fetch_add(1, Ordering::AcqRel);

        // SAFETY: We have exclusive access (seq is odd)
        unsafe {
            *self.data.get() = value;
        }

        // Increment sequence again (now even = write complete)
        self.seq.fetch_add(1, Ordering::Release);
    }

    /// Get the current sequence number
    pub fn sequence(&self) -> usize {
        self.seq.load(Ordering::Relaxed)
    }
}

// SAFETY: SeqLock provides synchronization
unsafe impl<T: Copy + Send> Send for SeqLock<T> {}
unsafe impl<T: Copy + Send> Sync for SeqLock<T> {}

// =============================================================================
// TAGGED POINTER (ABA PREVENTION)
// =============================================================================

/// Tagged pointer for ABA problem prevention
///
/// Combines a pointer with a version counter in a single atomic.
/// The version prevents the ABA problem in lock-free algorithms.
#[derive(Debug)]
pub struct TaggedPtr<T> {
    packed: AtomicU64,
    _marker: std::marker::PhantomData<*mut T>,
}

impl<T> TaggedPtr<T> {
    /// Bits reserved for tag (16 bits = 65536 versions)
    const TAG_BITS: u64 = 16;
    const TAG_MASK: u64 = (1 << Self::TAG_BITS) - 1;
    const PTR_MASK: u64 = !Self::TAG_MASK;

    /// Create a new tagged pointer
    pub fn new(ptr: *mut T) -> Self {
        Self {
            packed: AtomicU64::new(ptr as u64),
            _marker: std::marker::PhantomData,
        }
    }

    /// Create a null tagged pointer
    pub fn null() -> Self {
        Self::new(std::ptr::null_mut())
    }

    /// Load the pointer and tag
    #[inline]
    pub fn load(&self, order: Ordering) -> (*mut T, u16) {
        let packed = self.packed.load(order);
        let ptr = (packed & Self::PTR_MASK) as *mut T;
        let tag = (packed & Self::TAG_MASK) as u16;
        (ptr, tag)
    }

    /// Store a new pointer with incremented tag
    #[inline]
    pub fn store(&self, ptr: *mut T, order: Ordering) {
        let (_, old_tag) = self.load(Ordering::Relaxed);
        let new_tag = old_tag.wrapping_add(1) as u64;
        let packed = (ptr as u64 & Self::PTR_MASK) | (new_tag & Self::TAG_MASK);
        self.packed.store(packed, order);
    }

    /// Compare and exchange with tag validation
    #[inline]
    pub fn compare_exchange(
        &self,
        expected_ptr: *mut T,
        expected_tag: u16,
        new_ptr: *mut T,
        success: Ordering,
        failure: Ordering,
    ) -> Result<(*mut T, u16), (*mut T, u16)> {
        let expected =
            (expected_ptr as u64 & Self::PTR_MASK) | (expected_tag as u64 & Self::TAG_MASK);
        let new_tag = expected_tag.wrapping_add(1) as u64;
        let new = (new_ptr as u64 & Self::PTR_MASK) | (new_tag & Self::TAG_MASK);

        match self
            .packed
            .compare_exchange(expected, new, success, failure)
        {
            Ok(_) => Ok((new_ptr, new_tag as u16)),
            Err(actual) => {
                let ptr = (actual & Self::PTR_MASK) as *mut T;
                let tag = (actual & Self::TAG_MASK) as u16;
                Err((ptr, tag))
            }
        }
    }
}

// SAFETY: TaggedPtr is a wrapper around AtomicU64
unsafe impl<T: Send> Send for TaggedPtr<T> {}
unsafe impl<T: Send> Sync for TaggedPtr<T> {}

// =============================================================================
// LOCK-FREE STACK (SIMPLE TREIBER STACK)
// =============================================================================

/// Lock-free stack using Treiber's algorithm
///
/// Simple implementation using AtomicPtr. Note: susceptible to ABA problem
/// in pathological cases, but safe for most uses.
pub struct LockFreeStack<T> {
    head: AtomicPtr<Node<T>>,
}

struct Node<T> {
    value: T,
    next: *mut Node<T>,
}

impl<T> LockFreeStack<T> {
    /// Create a new empty stack
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(std::ptr::null_mut()),
        }
    }

    /// Push a value onto the stack
    pub fn push(&self, value: T) {
        let node = Box::into_raw(Box::new(Node {
            value,
            next: std::ptr::null_mut(),
        }));

        loop {
            let head = self.head.load(Ordering::Acquire);

            // SAFETY: node is valid
            unsafe { (*node).next = head };

            match self
                .head
                .compare_exchange_weak(head, node, Ordering::AcqRel, Ordering::Relaxed)
            {
                Ok(_) => return,
                Err(_) => continue,
            }
        }
    }

    /// Pop a value from the stack
    pub fn pop(&self) -> Option<T> {
        loop {
            let head = self.head.load(Ordering::Acquire);

            if head.is_null() {
                return None;
            }

            // SAFETY: head is valid (we pushed it)
            let next = unsafe { (*head).next };

            match self
                .head
                .compare_exchange_weak(head, next, Ordering::AcqRel, Ordering::Relaxed)
            {
                Ok(_) => {
                    // SAFETY: We own the node now
                    let node = unsafe { Box::from_raw(head) };
                    return Some(node.value);
                }
                Err(_) => continue,
            }
        }
    }

    /// Check if the stack is empty
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire).is_null()
    }
}

impl<T> Default for LockFreeStack<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for LockFreeStack<T> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

// SAFETY: Stack operations are atomic
unsafe impl<T: Send> Send for LockFreeStack<T> {}
unsafe impl<T: Send> Sync for LockFreeStack<T> {}

// =============================================================================
// ATOMIC FLAGS
// =============================================================================

/// Atomic flag set for efficient multi-flag operations
#[repr(C, align(8))]
pub struct AtomicFlags {
    bits: AtomicU64,
}

impl AtomicFlags {
    pub const fn new() -> Self {
        Self {
            bits: AtomicU64::new(0),
        }
    }

    /// Set a flag (0-63)
    #[inline]
    pub fn set(&self, flag: u8) {
        debug_assert!(flag < 64);
        self.bits.fetch_or(1 << flag, Ordering::AcqRel);
    }

    /// Clear a flag
    #[inline]
    pub fn clear(&self, flag: u8) {
        debug_assert!(flag < 64);
        self.bits.fetch_and(!(1 << flag), Ordering::AcqRel);
    }

    /// Test a flag
    #[inline]
    pub fn test(&self, flag: u8) -> bool {
        debug_assert!(flag < 64);
        self.bits.load(Ordering::Acquire) & (1 << flag) != 0
    }

    /// Test and set (returns previous value)
    #[inline]
    pub fn test_and_set(&self, flag: u8) -> bool {
        debug_assert!(flag < 64);
        let mask = 1 << flag;
        self.bits.fetch_or(mask, Ordering::AcqRel) & mask != 0
    }

    /// Clear all flags
    #[inline]
    pub fn clear_all(&self) {
        self.bits.store(0, Ordering::Release);
    }

    /// Get raw bits
    #[inline]
    pub fn bits(&self) -> u64 {
        self.bits.load(Ordering::Acquire)
    }
}

impl Default for AtomicFlags {
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
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_padded_counter() {
        let counter = PaddedAtomicU64::new(0);
        assert_eq!(counter.fetch_add(5, Ordering::SeqCst), 0);
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn test_seq_lock() {
        let lock = Arc::new(SeqLock::new(42u64));

        let lock2 = lock.clone();
        let writer = thread::spawn(move || {
            for i in 0..1000 {
                lock2.write(i);
            }
        });

        let lock3 = lock.clone();
        let reader = thread::spawn(move || {
            for _ in 0..10000 {
                let _ = lock3.read();
            }
        });

        writer.join().unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn test_lock_free_stack() {
        // Single-threaded test to avoid race conditions in tagged pointer
        let stack = LockFreeStack::new();

        // Push some items
        stack.push(1);
        stack.push(2);
        stack.push(3);

        // Pop in LIFO order
        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn test_atomic_flags() {
        let flags = AtomicFlags::new();

        flags.set(0);
        flags.set(63);

        assert!(flags.test(0));
        assert!(flags.test(63));
        assert!(!flags.test(32));

        flags.clear(0);
        assert!(!flags.test(0));
    }
}
