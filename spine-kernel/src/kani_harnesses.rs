//! Kani model checking harnesses for spine-kernel unsafe code.
//!
//! These harnesses verify the safety of low-level primitives using
//! bounded model checking via the kani verifier.
//!
//! # Running
//!
//! ```bash
//! # Install kani
//! cargo install --locked kani-verifier
//! kani setup
//!
//! # Run all harnesses
//! cd spine-kernel
//! cargo kani --harness-timeout 300
//!
//! # Run specific harness
//! cargo kani --harness bump_alloc_never_overlaps
//! ```
//!
//! # Coverage
//!
//! Each harness targets a specific unsafe invariant identified in the
//! formal audit (see formal/audit/CRYPTO_AUDIT.md §3).
//!
//! | Harness | Unsafe Code | Property Verified |
//! |---------|------------|-------------------|
//! | `bump_alloc_never_overlaps` | BumpAllocator::alloc | No overlapping allocations |
//! | `bump_alloc_alignment` | BumpAllocator::alloc | Returned pointers are aligned |
//! | `bump_alloc_within_bounds` | BumpAllocator::alloc | Pointers within [start, end) |
//! | `slab_alloc_dealloc_roundtrip` | SlabAllocator::alloc/dealloc | Alloc-dealloc-realloc cycle |
//! | `slab_free_list_integrity` | SlabAllocator | Free list tracks all blocks |
//! | `seqlock_read_consistency` | SeqLock::read/write | Reads see consistent values |
//! | `seqlock_write_atomicity` | SeqLock::write | Writer increments sequence correctly |
//! | `lock_free_stack_push_pop` | LockFreeStack | LIFO ordering preserved |
//! | `lock_free_stack_no_leak` | LockFreeStack | Every push has matching pop |
//! | `spsc_ring_no_data_loss` | SpscRing::try_push/try_pop | Every pushed item is popped exactly once |
//! | `spsc_ring_capacity` | SpscRing | Never exceeds capacity N-1 |
//! | `mpsc_ring_no_data_loss` | MpscRing | Multiple producers, single consumer, no loss |
//! | `tagged_ptr_roundtrip` | TaggedPtr | Tag/pointer packing is invertible |
//! | `atomic_flags_set_clear` | AtomicFlags | Set/clear/test are consistent |
//! | `dot_product_correctness` | SIMD dot_product | SIMD matches scalar computation |

#[cfg(kani)]
mod kani_harnesses {
    use std::alloc::Layout;

    // =========================================================================
    // BUMP ALLOCATOR HARNESSES
    // =========================================================================

    /// Verify that two allocations from a BumpAllocator never overlap.
    ///
    /// Targets: BumpAllocator::alloc (unsafe pointer arithmetic, CAS)
    /// Invariant: If alloc returns Some for two different calls, the memory
    /// regions [ptr1, ptr1+size1) and [ptr2, ptr2+size2) do not overlap.
    #[kani::proof]
    #[kani::unwind(2)]
    fn bump_alloc_never_overlaps() {
        let capacity: usize = kani::any();
        kani::assume(capacity >= 128 && capacity <= 4096);

        let bump = crate::alloc::BumpAllocator::new(capacity);

        let size1: usize = kani::any();
        let size2: usize = kani::any();
        kani::assume(size1 >= 1 && size1 <= 64);
        kani::assume(size2 >= 1 && size2 <= 64);

        let layout1 = Layout::from_size_align(size1, 8).unwrap();
        let layout2 = Layout::from_size_align(size2, 8).unwrap();

        if let (Some(ptr1), Some(ptr2)) = (bump.alloc(layout1), bump.alloc(layout2)) {
            let start1 = ptr1.as_ptr() as usize;
            let end1 = start1 + size1;
            let start2 = ptr2.as_ptr() as usize;
            let end2 = start2 + size2;

            // No overlap: end1 <= start2 OR end2 <= start1
            assert!(
                end1 <= start2 || end2 <= start1,
                "Allocations overlap: [{start1}, {end1}) and [{start2}, {end2})"
            );
        }
    }

    /// Verify that BumpAllocator returns properly aligned pointers.
    ///
    /// Targets: BumpAllocator::alloc (alignment arithmetic)
    /// Invariant: Returned pointer address is a multiple of the requested alignment.
    #[kani::proof]
    #[kani::unwind(1)]
    fn bump_alloc_alignment() {
        let bump = crate::alloc::BumpAllocator::new(4096);

        let align_exp: u32 = kani::any();
        kani::assume(align_exp <= 6); // alignments: 1, 2, 4, 8, 16, 32, 64
        let align = 1usize << align_exp;

        let size: usize = kani::any();
        kani::assume(size >= 1 && size <= 256);

        let layout = Layout::from_size_align(size, align).unwrap();

        if let Some(ptr) = bump.alloc(layout) {
            let addr = ptr.as_ptr() as usize;
            assert!(
                addr % align == 0,
                "Pointer {addr:#x} not aligned to {align}"
            );
        }
    }

    /// Verify that BumpAllocator never returns a pointer outside its buffer.
    ///
    /// Targets: BumpAllocator::alloc (bounds checking)
    /// Invariant: start <= ptr && ptr + size <= end
    #[kani::proof]
    #[kani::unwind(1)]
    fn bump_alloc_within_bounds() {
        let capacity: usize = kani::any();
        kani::assume(capacity >= 64 && capacity <= 1024);

        let bump = crate::alloc::BumpAllocator::new(capacity);
        let bump_start = bump.start() as usize;
        let bump_end = bump_start + capacity;

        let size: usize = kani::any();
        kani::assume(size >= 1 && size <= 128);

        let layout = Layout::from_size_align(size, 8).unwrap();

        if let Some(ptr) = bump.alloc(layout) {
            let addr = ptr.as_ptr() as usize;
            assert!(addr >= bump_start, "Pointer below buffer start");
            assert!(addr + size <= bump_end, "Pointer exceeds buffer end");
        }
    }

    // =========================================================================
    // SLAB ALLOCATOR HARNESSES
    // =========================================================================

    /// Verify that slab alloc → dealloc → re-alloc cycle returns same block.
    ///
    /// Targets: SlabAllocator::alloc, SlabAllocator::dealloc (free list manipulation)
    /// Invariant: After dealloc, the block is returned to the free list and
    /// the next alloc returns it (LIFO free list).
    #[kani::proof]
    #[kani::unwind(1)]
    fn slab_alloc_dealloc_roundtrip() {
        let block_size: usize = kani::any();
        kani::assume(block_size >= 64 && block_size <= 256);
        kani::assume(block_size % 64 == 0); // SlabAllocator requires 64-byte alignment

        let slab = crate::alloc::SlabAllocator::new(block_size, 4);

        // Allocate a block
        if let Some(ptr1) = slab.alloc() {
            let addr1 = ptr1.as_ptr() as usize;

            // Deallocate it
            unsafe { slab.dealloc(ptr1) };

            // Re-allocate: should get the same block back (LIFO free list)
            if let Some(ptr2) = slab.alloc() {
                let addr2 = ptr2.as_ptr() as usize;
                assert!(
                    addr2 == addr1,
                    "Dealloc'd block not returned: expected {addr1:#x}, got {addr2:#x}"
                );
            }
        }
    }

    /// Verify slab free list integrity after multiple alloc/dealloc cycles.
    ///
    /// Targets: SlabAllocator free list (concurrent CAS on AtomicPtr)
    /// Invariant: Total allocated + total free = total blocks
    #[kani::proof]
    #[kani::unwind(5)]
    fn slab_free_list_integrity() {
        let slab = crate::alloc::SlabAllocator::new(64, 4);

        // Allocate all 4 blocks
        let mut ptrs = [None; 4];
        for i in 0..4 {
            ptrs[i] = slab.alloc();
        }

        // All 4 should have succeeded
        let alloc_count = ptrs.iter().filter(|p| p.is_some()).count();
        assert!(
            alloc_count == 4,
            "Expected 4 allocations, got {alloc_count}"
        );

        // 5th allocation should fail (no free blocks)
        assert!(slab.alloc().is_none(), "Slab should be exhausted");

        // Free all blocks
        for ptr in ptrs.iter().flatten() {
            unsafe { slab.dealloc(*ptr) };
        }

        // Now we should be able to allocate 4 again
        let mut count = 0;
        for _ in 0..4 {
            if slab.alloc().is_some() {
                count += 1;
            }
        }
        assert!(
            count == 4,
            "Expected 4 re-allocations after free, got {count}"
        );
    }

    // =========================================================================
    // SEQLOCK HARNESSES
    // =========================================================================

    /// Verify SeqLock read consistency: reads return values that were written.
    ///
    /// Targets: SeqLock::read, SeqLock::write (UnsafeCell dereference)
    /// Invariant: read() returns either the initial value or a value
    /// previously passed to write().
    #[kani::proof]
    #[kani::unwind(1)]
    fn seqlock_read_consistency() {
        let lock = crate::atomic::SeqLock::new(0u64);

        let val: u64 = kani::any();
        kani::assume(val <= 1000);

        lock.write(val);
        let read_val = lock.read();

        assert!(read_val == val, "SeqLock read {read_val} but wrote {val}");
    }

    /// Verify SeqLock write atomicity: sequence number is odd during write,
    /// even after write.
    ///
    /// Targets: SeqLock::write (sequence counter manipulation)
    /// Invariant: After write completes, sequence is even and incremented by 2.
    #[kani::proof]
    #[kani::unwind(1)]
    fn seqlock_write_atomicity() {
        let lock = crate::atomic::SeqLock::new(42u64);

        let seq_before = lock.sequence();
        assert!(seq_before % 2 == 0, "Initial sequence should be even");

        lock.write(99);

        let seq_after = lock.sequence();
        assert!(seq_after % 2 == 0, "Post-write sequence should be even");
        assert!(
            seq_after == seq_before + 2,
            "Sequence should increment by 2: {seq_before} → {seq_after}"
        );
    }

    // =========================================================================
    // LOCK-FREE STACK HARNESSES
    // =========================================================================

    /// Verify LockFreeStack preserves LIFO ordering.
    ///
    /// Targets: push (raw pointer write), pop (Box::from_raw)
    /// Invariant: Items come out in reverse order of insertion.
    #[kani::proof]
    #[kani::unwind(4)]
    fn lock_free_stack_push_pop() {
        let stack = crate::atomic::LockFreeStack::new();

        stack.push(1u32);
        stack.push(2u32);
        stack.push(3u32);

        assert!(stack.pop() == Some(3), "Third push should be first pop");
        assert!(stack.pop() == Some(2), "Second push should be second pop");
        assert!(stack.pop() == Some(1), "First push should be third pop");
        assert!(stack.pop() == None, "Stack should be empty");
    }

    /// Verify LockFreeStack: every push has a matching pop (no memory leak).
    ///
    /// Targets: LockFreeStack::push/pop (Box::into_raw / Box::from_raw balance)
    /// Invariant: After N pushes and N pops, stack is empty and all values recovered.
    #[kani::proof]
    #[kani::unwind(5)]
    fn lock_free_stack_no_leak() {
        let stack = crate::atomic::LockFreeStack::new();

        let n: usize = kani::any();
        kani::assume(n >= 1 && n <= 4);

        // Push n items
        for i in 0..n {
            stack.push(i as u64);
        }

        // Pop all n items — each must succeed
        let mut pop_count = 0;
        for _ in 0..n {
            if stack.pop().is_some() {
                pop_count += 1;
            }
        }

        assert!(pop_count == n, "Expected {n} pops, got {pop_count}");
        assert!(
            stack.pop().is_none(),
            "Stack should be empty after {n} pops"
        );
    }

    // =========================================================================
    // SPSC RING BUFFER HARNESSES
    // =========================================================================

    /// Verify SPSC ring: every pushed item is popped exactly once, in order.
    ///
    /// Targets: SpscRing::try_push/try_pop (UnsafeCell, MaybeUninit)
    /// Invariant: Items are consumed in FIFO order with no loss or duplication.
    #[kani::proof]
    #[kani::unwind(5)]
    fn spsc_ring_no_data_loss() {
        let ring = crate::ring::SpscRing::<u32, 4>::new();

        // Push 3 items (capacity is N-1 = 3 for power-of-2 ring)
        assert!(ring.try_push(10).is_ok());
        assert!(ring.try_push(20).is_ok());
        assert!(ring.try_push(30).is_ok());

        // Pop in FIFO order
        assert!(ring.try_pop() == Some(10), "Expected 10");
        assert!(ring.try_pop() == Some(20), "Expected 20");
        assert!(ring.try_pop() == Some(30), "Expected 30");
        assert!(ring.try_pop() == None, "Ring should be empty");
    }

    /// Verify SPSC ring never exceeds capacity.
    ///
    /// Targets: SpscRing::try_push (bounds checking)
    /// Invariant: After N-1 pushes (for ring of size N), push returns Err.
    #[kani::proof]
    #[kani::unwind(9)]
    fn spsc_ring_capacity() {
        let ring = crate::ring::SpscRing::<u32, 8>::new();

        // Fill to capacity (N-1 = 7 items)
        let mut pushed = 0;
        for i in 0..8 {
            if ring.try_push(i).is_ok() {
                pushed += 1;
            }
        }

        assert!(pushed == 7, "Ring<8> should hold 7 items, held {pushed}");

        // Pop one, push one should work
        assert!(ring.try_pop().is_some());
        assert!(ring.try_push(99).is_ok());
    }

    // =========================================================================
    // MPSC RING BUFFER HARNESSES
    // =========================================================================

    /// Verify MPSC ring: push from multiple conceptual producers, pop recovers all.
    ///
    /// Targets: MpscRing::try_push/try_pop (CAS for writers, UnsafeCell)
    /// Invariant: All pushed items are eventually popped (no data loss).
    #[kani::proof]
    #[kani::unwind(5)]
    fn mpsc_ring_no_data_loss() {
        let ring = crate::ring::MpscRing::<u32, 4>::new();

        // Simulate 3 "producers" pushing
        assert!(ring.try_push(100).is_ok());
        assert!(ring.try_push(200).is_ok());
        assert!(ring.try_push(300).is_ok());

        // Single consumer pops all
        let mut values = Vec::new();
        while let Some(v) = ring.try_pop() {
            values.push(v);
        }

        assert!(values.len() == 3, "Expected 3 items, got {}", values.len());
        assert!(values.contains(&100));
        assert!(values.contains(&200));
        assert!(values.contains(&300));
    }

    // =========================================================================
    // TAGGED POINTER HARNESSES
    // =========================================================================

    /// Verify TaggedPtr roundtrip: store(ptr, tag) → load → same (ptr, tag).
    ///
    /// Targets: TaggedPtr::store/load (AtomicU64 bit packing)
    /// Invariant: The 48-bit pointer and 16-bit tag are correctly packed/unpacked.
    #[kani::proof]
    #[kani::unwind(1)]
    fn tagged_ptr_roundtrip() {
        let tagged = crate::atomic::TaggedPtr::<u64>::new();

        // Create a value on heap to get a valid pointer
        let boxed = Box::new(42u64);
        let ptr = Box::into_raw(boxed);
        let tag: u16 = kani::any();

        tagged.store(ptr, tag, std::sync::atomic::Ordering::SeqCst);

        let (loaded_ptr, loaded_tag) = tagged.load(std::sync::atomic::Ordering::SeqCst);

        assert!(loaded_ptr == ptr, "Pointer mismatch after roundtrip");
        assert!(loaded_tag == tag, "Tag mismatch after roundtrip");

        // Clean up
        unsafe { drop(Box::from_raw(ptr)) };
    }

    // =========================================================================
    // ATOMIC FLAGS HARNESSES
    // =========================================================================

    /// Verify AtomicFlags: set, test, clear are consistent.
    ///
    /// Targets: AtomicFlags (safe AtomicU64 wrapper — included for completeness)
    /// Invariant: After set(n), test(n) is true. After clear(n), test(n) is false.
    #[kani::proof]
    #[kani::unwind(1)]
    fn atomic_flags_set_clear() {
        let flags = crate::atomic::AtomicFlags::new();

        let bit: usize = kani::any();
        kani::assume(bit < 64);

        assert!(!flags.test(bit), "Flag should start unset");

        flags.set(bit);
        assert!(flags.test(bit), "Flag should be set after set()");

        flags.clear(bit);
        assert!(!flags.test(bit), "Flag should be cleared after clear()");
    }

    // =========================================================================
    // SIMD CORRECTNESS HARNESSES
    // =========================================================================

    /// Verify SIMD dot_product matches scalar computation.
    ///
    /// Targets: dot_product_avx2/neon (SIMD intrinsics, get_unchecked)
    /// Invariant: SIMD result ≈ scalar result (within floating-point tolerance).
    #[kani::proof]
    #[kani::unwind(17)]
    fn dot_product_correctness() {
        let n: usize = kani::any();
        kani::assume(n >= 1 && n <= 16);

        let a: Vec<f32> = (0..n).map(|i| (i as f32) * 0.5).collect();
        let b: Vec<f32> = (0..n).map(|i| (i as f32) * 0.25 + 1.0).collect();

        let simd_result = crate::simd::dot_product(&a, &b);

        // Scalar reference
        let scalar_result: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();

        let diff = (simd_result - scalar_result).abs();
        assert!(
            diff < 1e-3,
            "SIMD vs scalar mismatch: {simd_result} vs {scalar_result} (diff={diff})"
        );
    }
}
