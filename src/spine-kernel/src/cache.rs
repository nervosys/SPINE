//! Cache-Optimized Data Structures
//!
//! Utilities for cache-efficient memory access:
//! - Cache line wrappers
//! - Prefetch hints
//! - Struct-of-arrays patterns
//! - False sharing prevention

use crate::CACHE_LINE_SIZE;

// =============================================================================
// CACHE LINE WRAPPER
// =============================================================================

/// Wrapper that ensures a value occupies exactly one cache line
///
/// Use this to prevent false sharing between frequently-accessed values.
#[repr(C, align(64))]
pub struct CacheLine<T> {
    value: T,
    _pad: [u8; 0], // Compiler handles padding
}

impl<T> CacheLine<T> {
    /// Create a new cache-line aligned value
    pub const fn new(value: T) -> Self {
        Self { value, _pad: [] }
    }

    /// Get a reference to the inner value
    #[inline]
    pub fn get(&self) -> &T {
        &self.value
    }

    /// Get a mutable reference to the inner value
    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.value
    }

    /// Consume and return the inner value
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: Default> Default for CacheLine<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Clone> Clone for CacheLine<T> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone())
    }
}

impl<T: Copy> Copy for CacheLine<T> {}

impl<T> std::ops::Deref for CacheLine<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> std::ops::DerefMut for CacheLine<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

// =============================================================================
// PREFETCH HINTS
// =============================================================================

/// Prefetch locality hint
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locality {
    /// Non-temporal data (use once and discard)
    NonTemporal = 0,
    /// Low locality (unlikely to be reused)
    Low = 1,
    /// Moderate locality
    Moderate = 2,
    /// High locality (likely to be reused)
    High = 3,
}

/// Prefetch data for reading
///
/// Hints to the CPU to bring data into cache before it's needed.
///
/// # Arguments
/// * `ptr` - Pointer to prefetch
/// * `locality` - How likely the data is to be reused
#[inline]
pub fn prefetch_read<T>(ptr: *const T, locality: Locality) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        match locality {
            Locality::NonTemporal => {
                std::arch::x86_64::_mm_prefetch(ptr as *const i8, std::arch::x86_64::_MM_HINT_NTA)
            }
            Locality::Low => {
                std::arch::x86_64::_mm_prefetch(ptr as *const i8, std::arch::x86_64::_MM_HINT_T2)
            }
            Locality::Moderate => {
                std::arch::x86_64::_mm_prefetch(ptr as *const i8, std::arch::x86_64::_MM_HINT_T1)
            }
            Locality::High => {
                std::arch::x86_64::_mm_prefetch(ptr as *const i8, std::arch::x86_64::_MM_HINT_T0)
            }
        }
    }

    #[cfg(target_arch = "aarch64")]
    unsafe {
        // ARM prefetch
        std::arch::aarch64::_prefetch(
            ptr as *const i8,
            std::arch::aarch64::_PREFETCH_READ,
            locality as i32,
        );
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (ptr, locality); // No-op on other architectures
    }
}

/// Prefetch data for writing
#[inline]
pub fn prefetch_write<T>(ptr: *mut T, locality: Locality) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        // x86 doesn't have write-specific prefetch, use regular
        match locality {
            Locality::NonTemporal => {
                std::arch::x86_64::_mm_prefetch(ptr as *const i8, std::arch::x86_64::_MM_HINT_NTA)
            }
            Locality::Low => {
                std::arch::x86_64::_mm_prefetch(ptr as *const i8, std::arch::x86_64::_MM_HINT_T2)
            }
            Locality::Moderate => {
                std::arch::x86_64::_mm_prefetch(ptr as *const i8, std::arch::x86_64::_MM_HINT_T1)
            }
            Locality::High => {
                std::arch::x86_64::_mm_prefetch(ptr as *const i8, std::arch::x86_64::_MM_HINT_T0)
            }
        }
    }

    #[cfg(target_arch = "aarch64")]
    unsafe {
        std::arch::aarch64::_prefetch(
            ptr as *const i8,
            std::arch::aarch64::_PREFETCH_WRITE,
            locality as i32,
        );
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (ptr, locality);
    }
}

/// Prefetch multiple cache lines starting from ptr
#[inline]
pub fn prefetch_range<T>(ptr: *const T, count: usize, locality: Locality) {
    let byte_ptr = ptr as *const u8;
    let byte_count = count * std::mem::size_of::<T>();
    let cache_lines = byte_count.div_ceil(CACHE_LINE_SIZE);

    for i in 0..cache_lines.min(16) {
        // Limit to avoid flooding
        prefetch_read(unsafe { byte_ptr.add(i * CACHE_LINE_SIZE) }, locality);
    }
}

// =============================================================================
// STRUCT-OF-ARRAYS HELPER
// =============================================================================

/// Macro to create struct-of-arrays from array-of-structs
///
/// SoA layout is more cache-friendly for SIMD operations.
#[macro_export]
macro_rules! soa {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                $field_vis:vis $field:ident : $ty:ty
            ),* $(,)?
        }
    ) => {
        paste::paste! {
            $(#[$meta])*
            $vis struct $name {
                len: usize,
                capacity: usize,
                $(
                    $(#[$field_meta])*
                    $field_vis $field: Vec<$ty>,
                )*
            }

            impl $name {
                /// Create a new SoA with the given capacity
                pub fn with_capacity(capacity: usize) -> Self {
                    Self {
                        len: 0,
                        capacity,
                        $(
                            $field: Vec::with_capacity(capacity),
                        )*
                    }
                }

                /// Get the number of elements
                pub fn len(&self) -> usize {
                    self.len
                }

                /// Check if empty
                pub fn is_empty(&self) -> bool {
                    self.len == 0
                }

                /// Get capacity
                pub fn capacity(&self) -> usize {
                    self.capacity
                }

                /// Push a new element
                pub fn push(&mut self, $($field: $ty),*) {
                    $(
                        self.$field.push($field);
                    )*
                    self.len += 1;
                }

                /// Clear all elements
                pub fn clear(&mut self) {
                    $(
                        self.$field.clear();
                    )*
                    self.len = 0;
                }
            }
        }
    };
}

// =============================================================================
// HOT/COLD SPLITTING
// =============================================================================

/// Hot data that's accessed frequently
///
/// Keep this small to fit in L1 cache.
#[repr(C)]
pub struct HotData<H, C> {
    pub hot: H,
    cold_ptr: *const C,
}

impl<H, C> HotData<H, C> {
    /// Create new hot/cold split data
    pub fn new(hot: H, cold: C) -> (Self, Box<C>) {
        let cold_box = Box::new(cold);
        let cold_ptr = &*cold_box as *const C;
        (Self { hot, cold_ptr }, cold_box)
    }

    /// Access cold data
    ///
    /// # Safety
    /// The cold box must still be alive
    #[inline]
    pub unsafe fn cold(&self) -> &C {
        &*self.cold_ptr
    }
}

// =============================================================================
// CACHE-AWARE ITERATION
// =============================================================================

/// Iterator that prefetches ahead
pub struct PrefetchIter<'a, T> {
    slice: &'a [T],
    index: usize,
    prefetch_distance: usize,
}

impl<'a, T> PrefetchIter<'a, T> {
    /// Create a new prefetching iterator
    ///
    /// # Arguments
    /// * `slice` - The slice to iterate
    /// * `prefetch_distance` - How many elements ahead to prefetch
    pub fn new(slice: &'a [T], prefetch_distance: usize) -> Self {
        // Initial prefetch
        let prefetch_distance = prefetch_distance.max(1);
        let end = slice.len().min(prefetch_distance);
        for item in slice.iter().take(end) {
            prefetch_read(item, Locality::High);
        }

        Self {
            slice,
            index: 0,
            prefetch_distance,
        }
    }
}

impl<'a, T> Iterator for PrefetchIter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.slice.len() {
            return None;
        }

        // Prefetch ahead
        let prefetch_idx = self.index + self.prefetch_distance;
        if prefetch_idx < self.slice.len() {
            prefetch_read(&self.slice[prefetch_idx], Locality::High);
        }

        let item = &self.slice[self.index];
        self.index += 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.slice.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a, T> ExactSizeIterator for PrefetchIter<'a, T> {}

// =============================================================================
// CACHE-LINE ARRAY
// =============================================================================

/// Array where each element is on its own cache line
///
/// Use for frequently-modified arrays accessed by multiple threads.
pub struct CacheLineArray<T, const N: usize> {
    data: [CacheLine<T>; N],
}

impl<T: Default + Copy, const N: usize> CacheLineArray<T, N> {
    /// Create a new array with default values
    pub fn new() -> Self {
        Self {
            data: [CacheLine::new(T::default()); N],
        }
    }
}

impl<T, const N: usize> CacheLineArray<T, N> {
    /// Get element at index
    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        self.data.get(index).map(|cl| cl.get())
    }

    /// Get mutable element at index
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.data.get_mut(index).map(|cl| cl.get_mut())
    }

    /// Number of elements
    pub const fn len(&self) -> usize {
        N
    }

    /// Check if empty
    pub const fn is_empty(&self) -> bool {
        N == 0
    }
}

impl<T, const N: usize> std::ops::Index<usize> for CacheLineArray<T, N> {
    type Output = T;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        self.data[index].get()
    }
}

impl<T, const N: usize> std::ops::IndexMut<usize> for CacheLineArray<T, N> {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.data[index].get_mut()
    }
}

impl<T: Default + Copy, const N: usize> Default for CacheLineArray<T, N> {
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
    fn test_cache_line_size() {
        assert_eq!(std::mem::size_of::<CacheLine<u8>>(), CACHE_LINE_SIZE);
        assert_eq!(std::mem::size_of::<CacheLine<u64>>(), CACHE_LINE_SIZE);
    }

    #[test]
    fn test_cache_line_alignment() {
        let cl = CacheLine::new(42u64);
        let ptr = &cl as *const _ as usize;
        assert_eq!(ptr % CACHE_LINE_SIZE, 0);
    }

    #[test]
    fn test_prefetch_iter() {
        let data: Vec<u64> = (0..1000).collect();
        let iter = PrefetchIter::new(&data, 8);

        let sum: u64 = iter.copied().sum();
        assert_eq!(sum, (0..1000u64).sum());
    }

    #[test]
    fn test_cache_line_array() {
        let mut arr: CacheLineArray<u64, 16> = CacheLineArray::new();
        arr[0] = 42;
        arr[15] = 100;

        assert_eq!(arr[0], 42);
        assert_eq!(arr[15], 100);
    }
}
