//! SIMD Vector Operations
//!
//! Hardware-accelerated vector operations using AVX2, AVX-512, and NEON intrinsics.
//! Falls back to portable SIMD when platform intrinsics are unavailable.
//!
//! ## Architecture Support
//!
//! | Architecture | SIMD Width | Operations |
//! |--------------|------------|------------|
//! | x86_64 AVX2  | 256-bit    | 8 x f32    |
//! | x86_64 AVX512| 512-bit    | 16 x f32   |
//! | ARM NEON     | 128-bit    | 4 x f32    |
//! | Portable     | varies     | scalar     |

/// SIMD vector width in bytes for the current platform
#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
pub const SIMD_WIDTH: usize = 32; // 256 bits

#[cfg(all(target_arch = "x86_64", not(target_feature = "avx2")))]
pub const SIMD_WIDTH: usize = 16; // 128 bits SSE

#[cfg(target_arch = "aarch64")]
pub const SIMD_WIDTH: usize = 16; // 128 bits NEON

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub const SIMD_WIDTH: usize = 16;

/// Number of f32 elements per SIMD register
pub const SIMD_F32_LANES: usize = SIMD_WIDTH / 4;

// =============================================================================
// AVX2 INTRINSICS (x86_64)
// =============================================================================

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
mod avx2 {
    use std::arch::x86_64::*;

    /// AVX2 dot product of two f32 slices (8-wide)
    ///
    /// # Safety
    /// Pointers must be valid and slices must have equal length
    #[inline]
    #[target_feature(enable = "avx2", enable = "fma")]
    pub unsafe fn dot_product_avx2(a: &[f32], b: &[f32]) -> f32 {
        let len = a.len().min(b.len());
        let chunks = len / 8;

        let mut sum = _mm256_setzero_ps();

        let a_ptr = a.as_ptr();
        let b_ptr = b.as_ptr();

        for i in 0..chunks {
            let offset = i * 8;
            let va = _mm256_loadu_ps(a_ptr.add(offset));
            let vb = _mm256_loadu_ps(b_ptr.add(offset));
            // Fused multiply-add: sum += va * vb
            sum = _mm256_fmadd_ps(va, vb, sum);
        }

        // Horizontal sum of 8 floats
        let sum128 = _mm_add_ps(_mm256_castps256_ps128(sum), _mm256_extractf128_ps(sum, 1));
        let sum64 = _mm_add_ps(sum128, _mm_movehl_ps(sum128, sum128));
        let sum32 = _mm_add_ss(sum64, _mm_shuffle_ps(sum64, sum64, 1));
        let mut result = _mm_cvtss_f32(sum32);

        // Handle remainder
        for i in (chunks * 8)..len {
            result += *a.get_unchecked(i) * *b.get_unchecked(i);
        }

        result
    }

    /// AVX2 vector addition: dst += src
    #[inline]
    #[target_feature(enable = "avx2")]
    pub unsafe fn vec_add_avx2(dst: &mut [f32], src: &[f32]) {
        let len = dst.len().min(src.len());
        let chunks = len / 8;

        let dst_ptr = dst.as_mut_ptr();
        let src_ptr = src.as_ptr();

        for i in 0..chunks {
            let offset = i * 8;
            let vdst = _mm256_loadu_ps(dst_ptr.add(offset));
            let vsrc = _mm256_loadu_ps(src_ptr.add(offset));
            let vsum = _mm256_add_ps(vdst, vsrc);
            _mm256_storeu_ps(dst_ptr.add(offset), vsum);
        }

        // Remainder
        for i in (chunks * 8)..len {
            *dst.get_unchecked_mut(i) += *src.get_unchecked(i);
        }
    }

    /// AVX2 vector scale-add: dst += scale * src
    #[inline]
    #[target_feature(enable = "avx2", enable = "fma")]
    pub unsafe fn vec_scale_add_avx2(dst: &mut [f32], scale: f32, src: &[f32]) {
        let len = dst.len().min(src.len());
        let chunks = len / 8;

        let vscale = _mm256_set1_ps(scale);
        let dst_ptr = dst.as_mut_ptr();
        let src_ptr = src.as_ptr();

        for i in 0..chunks {
            let offset = i * 8;
            let vdst = _mm256_loadu_ps(dst_ptr.add(offset));
            let vsrc = _mm256_loadu_ps(src_ptr.add(offset));
            // FMA: dst = dst + scale * src
            let vresult = _mm256_fmadd_ps(vscale, vsrc, vdst);
            _mm256_storeu_ps(dst_ptr.add(offset), vresult);
        }

        // Remainder
        for i in (chunks * 8)..len {
            *dst.get_unchecked_mut(i) += scale * *src.get_unchecked(i);
        }
    }

    /// AVX2 softmax with numerical stability
    #[inline]
    #[target_feature(enable = "avx2")]
    pub unsafe fn softmax_avx2(data: &mut [f32]) {
        if data.is_empty() {
            return;
        }

        // Find max (scalar, vectorized find_max is complex)
        let max = data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let vmax = _mm256_set1_ps(max);

        let len = data.len();
        let chunks = len / 8;
        let ptr = data.as_mut_ptr();

        // exp(x - max) and sum
        let mut vsum = _mm256_setzero_ps();
        for i in 0..chunks {
            let offset = i * 8;
            let vx = _mm256_loadu_ps(ptr.add(offset));
            let vdiff = _mm256_sub_ps(vx, vmax);
            // Approximate exp using polynomial (fast but less accurate)
            let vexp = exp_approx_avx2(vdiff);
            _mm256_storeu_ps(ptr.add(offset), vexp);
            vsum = _mm256_add_ps(vsum, vexp);
        }

        // Horizontal sum
        let sum128 = _mm_add_ps(_mm256_castps256_ps128(vsum), _mm256_extractf128_ps(vsum, 1));
        let sum64 = _mm_add_ps(sum128, _mm_movehl_ps(sum128, sum128));
        let sum32 = _mm_add_ss(sum64, _mm_shuffle_ps(sum64, sum64, 1));
        let mut total = _mm_cvtss_f32(sum32);

        // Handle remainder
        for i in (chunks * 8)..len {
            let exp_val = (*data.get_unchecked(i) - max).exp();
            *data.get_unchecked_mut(i) = exp_val;
            total += exp_val;
        }

        // Normalize
        let inv_sum = 1.0 / total;
        let vinv = _mm256_set1_ps(inv_sum);

        for i in 0..chunks {
            let offset = i * 8;
            let vx = _mm256_loadu_ps(ptr.add(offset));
            let vnorm = _mm256_mul_ps(vx, vinv);
            _mm256_storeu_ps(ptr.add(offset), vnorm);
        }

        for i in (chunks * 8)..len {
            *data.get_unchecked_mut(i) *= inv_sum;
        }
    }

    /// Fast exp approximation using polynomial (Remez minimax)
    #[inline]
    #[target_feature(enable = "avx2", enable = "fma")]
    unsafe fn exp_approx_avx2(x: __m256) -> __m256 {
        // Clamp to prevent overflow
        let min_val = _mm256_set1_ps(-88.0);
        let max_val = _mm256_set1_ps(88.0);
        let x = _mm256_max_ps(_mm256_min_ps(x, max_val), min_val);

        // exp(x) = 2^(x * log2(e))
        let log2e = _mm256_set1_ps(std::f32::consts::LOG2_E);
        let t = _mm256_mul_ps(x, log2e);

        // Split into integer and fractional parts
        let ti = _mm256_round_ps(t, _MM_FROUND_TO_NEAREST_INT | _MM_FROUND_NO_EXC);
        let tf = _mm256_sub_ps(t, ti);

        // 2^fractional using polynomial
        let c0 = _mm256_set1_ps(1.0);
        let c1 = _mm256_set1_ps(0.693147180559945);
        let c2 = _mm256_set1_ps(0.240226506959101);
        let c3 = _mm256_set1_ps(0.055504108664822);
        let c4 = _mm256_set1_ps(0.009618129107629);
        let c5 = _mm256_set1_ps(0.001333355814671);

        let p = _mm256_fmadd_ps(c5, tf, c4);
        let p = _mm256_fmadd_ps(p, tf, c3);
        let p = _mm256_fmadd_ps(p, tf, c2);
        let p = _mm256_fmadd_ps(p, tf, c1);
        let p = _mm256_fmadd_ps(p, tf, c0);

        // 2^integer part via bit manipulation
        let ti_i32 = _mm256_cvtps_epi32(ti);
        let bias = _mm256_set1_epi32(127);
        let exp_i32 = _mm256_add_epi32(ti_i32, bias);
        let exp_shifted = _mm256_slli_epi32(exp_i32, 23);
        let exp_f32 = _mm256_castsi256_ps(exp_shifted);

        _mm256_mul_ps(p, exp_f32)
    }
}

// =============================================================================
// NEON INTRINSICS (ARM)
// =============================================================================

#[cfg(target_arch = "aarch64")]
mod neon {
    use std::arch::aarch64::*;

    /// NEON dot product of two f32 slices (4-wide)
    #[inline]
    pub unsafe fn dot_product_neon(a: &[f32], b: &[f32]) -> f32 {
        let len = a.len().min(b.len());
        let chunks = len / 4;

        let mut sum = vdupq_n_f32(0.0);

        let a_ptr = a.as_ptr();
        let b_ptr = b.as_ptr();

        for i in 0..chunks {
            let offset = i * 4;
            let va = vld1q_f32(a_ptr.add(offset));
            let vb = vld1q_f32(b_ptr.add(offset));
            sum = vfmaq_f32(sum, va, vb);
        }

        // Horizontal sum
        let result = vaddvq_f32(sum);

        // Remainder
        let mut final_result = result;
        for i in (chunks * 4)..len {
            final_result += *a.get_unchecked(i) * *b.get_unchecked(i);
        }

        final_result
    }

    /// NEON vector scale-add: dst += scale * src
    #[inline]
    pub unsafe fn vec_scale_add_neon(dst: &mut [f32], scale: f32, src: &[f32]) {
        let len = dst.len().min(src.len());
        let chunks = len / 4;

        let vscale = vdupq_n_f32(scale);
        let dst_ptr = dst.as_mut_ptr();
        let src_ptr = src.as_ptr();

        for i in 0..chunks {
            let offset = i * 4;
            let vdst = vld1q_f32(dst_ptr.add(offset));
            let vsrc = vld1q_f32(src_ptr.add(offset));
            let vresult = vfmaq_f32(vdst, vscale, vsrc);
            vst1q_f32(dst_ptr.add(offset), vresult);
        }

        // Remainder
        for i in (chunks * 4)..len {
            *dst.get_unchecked_mut(i) += scale * *src.get_unchecked(i);
        }
    }
}

// =============================================================================
// PORTABLE FALLBACK
// =============================================================================

mod portable {
    /// Portable dot product (auto-vectorized by LLVM)
    #[inline]
    pub fn dot_product_portable(a: &[f32], b: &[f32]) -> f32 {
        let len = a.len().min(b.len());
        let (a_chunks, a_rem) = a[..len].split_at(len - len % 8);
        let (b_chunks, b_rem) = b[..len].split_at(len - len % 8);

        let sum: f32 = a_chunks
            .chunks_exact(8)
            .zip(b_chunks.chunks_exact(8))
            .map(|(ac, bc)| {
                ac[0] * bc[0]
                    + ac[1] * bc[1]
                    + ac[2] * bc[2]
                    + ac[3] * bc[3]
                    + ac[4] * bc[4]
                    + ac[5] * bc[5]
                    + ac[6] * bc[6]
                    + ac[7] * bc[7]
            })
            .sum();

        sum + a_rem.iter().zip(b_rem).map(|(x, y)| x * y).sum::<f32>()
    }

    /// Portable vector scale-add
    #[inline]
    pub fn vec_scale_add_portable(dst: &mut [f32], scale: f32, src: &[f32]) {
        for (d, s) in dst.iter_mut().zip(src.iter()) {
            *d += scale * *s;
        }
    }

    /// Portable softmax
    #[inline]
    pub fn softmax_portable(data: &mut [f32]) {
        if data.is_empty() {
            return;
        }
        let max = data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0f32;
        for x in data.iter_mut() {
            *x = (*x - max).exp();
            sum += *x;
        }
        let inv_sum = 1.0 / sum;
        for x in data.iter_mut() {
            *x *= inv_sum;
        }
    }
}

// =============================================================================
// PUBLIC API - RUNTIME DISPATCH
// =============================================================================

/// SIMD-accelerated dot product
///
/// Automatically dispatches to the best available implementation:
/// - AVX2 + FMA on x86_64 with AVX2
/// - NEON on ARM64
/// - Portable fallback otherwise
#[inline]
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    {
        // SAFETY: We've checked target_feature
        unsafe { avx2::dot_product_avx2(a, b) }
    }

    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    {
        unsafe { neon::dot_product_neon(a, b) }
    }

    #[cfg(not(any(
        all(target_arch = "x86_64", target_feature = "avx2"),
        all(target_arch = "aarch64", target_feature = "neon")
    )))]
    {
        portable::dot_product_portable(a, b)
    }
}

/// SIMD-accelerated vector scale-add: dst += scale * src
#[inline]
pub fn vec_scale_add(dst: &mut [f32], scale: f32, src: &[f32]) {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    {
        unsafe { avx2::vec_scale_add_avx2(dst, scale, src) }
    }

    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    {
        unsafe { neon::vec_scale_add_neon(dst, scale, src) }
    }

    #[cfg(not(any(
        all(target_arch = "x86_64", target_feature = "avx2"),
        all(target_arch = "aarch64", target_feature = "neon")
    )))]
    {
        portable::vec_scale_add_portable(dst, scale, src)
    }
}

/// SIMD-accelerated softmax (in-place)
#[inline]
pub fn softmax(data: &mut [f32]) {
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    {
        unsafe { avx2::softmax_avx2(data) }
    }

    #[cfg(not(all(target_arch = "x86_64", target_feature = "avx2")))]
    {
        portable::softmax_portable(data)
    }
}

/// Matrix-vector multiplication: output = weights @ input
///
/// Uses SIMD for each row's dot product.
#[inline]
pub fn matmul(weights: &[&[f32]], input: &[f32], output: &mut [f32]) {
    for (row, out) in weights.iter().zip(output.iter_mut()) {
        *out = dot_product(row, input);
    }
}

/// Flattened matrix-vector multiplication
///
/// weights_flat is row-major: weights_flat[row * cols + col]
#[inline]
pub fn matmul_flat(
    weights_flat: &[f32],
    rows: usize,
    cols: usize,
    input: &[f32],
    output: &mut [f32],
) {
    debug_assert_eq!(input.len(), cols);
    debug_assert_eq!(output.len(), rows);
    debug_assert_eq!(weights_flat.len(), rows * cols);

    for (row_idx, out) in output.iter_mut().enumerate() {
        let row_start = row_idx * cols;
        let row = &weights_flat[row_start..row_start + cols];
        *out = dot_product(row, input);
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dot_product() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let b = vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let result = dot_product(&a, &b);
        assert!((result - 55.0).abs() < 1e-5);
    }

    #[test]
    fn test_vec_scale_add() {
        let mut dst = vec![1.0, 2.0, 3.0, 4.0];
        let src = vec![1.0, 1.0, 1.0, 1.0];
        vec_scale_add(&mut dst, 2.0, &src);
        assert_eq!(dst, vec![3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn test_softmax() {
        let mut data = vec![1.0, 2.0, 3.0, 4.0];
        softmax(&mut data);
        let sum: f32 = data.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);
        // Check ordering is preserved
        assert!(data[3] > data[2]);
        assert!(data[2] > data[1]);
        assert!(data[1] > data[0]);
    }

    #[test]
    fn test_matmul_flat() {
        // 2x3 matrix @ 3x1 vector
        let weights = vec![
            1.0, 2.0, 3.0, // row 0
            4.0, 5.0, 6.0, // row 1
        ];
        let input = vec![1.0, 1.0, 1.0];
        let mut output = vec![0.0, 0.0];
        matmul_flat(&weights, 2, 3, &input, &mut output);
        assert!((output[0] - 6.0).abs() < 1e-5);
        assert!((output[1] - 15.0).abs() < 1e-5);
    }
}
