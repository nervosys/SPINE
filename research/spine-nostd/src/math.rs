//! Fixed-point and integer-only math for no_std neural operations.
//!
//! All operations use Q8.8 fixed-point (i16) or Q16.16 fixed-point (i32)
//! to avoid floating-point dependency.

/// Dot product in Q8.8 fixed-point.
///
/// Result is in Q16.16 (i32) to avoid overflow.
/// To convert back to Q8.8: `(result >> 8) as i16`
pub fn dot_product_fixed(a: &[i16], b: &[i16]) -> i32 {
    let len = a.len().min(b.len());
    let mut sum: i32 = 0;
    for i in 0..len {
        sum += a[i] as i32 * b[i] as i32;
    }
    sum
}

/// Cosine similarity in Q8.8 fixed-point.
///
/// Returns a Q8.8 value in [-256, 256] (representing [-1.0, 1.0]).
/// Returns 0 if either vector has zero magnitude.
pub fn cosine_similarity_fixed(a: &[i16], b: &[i16]) -> i16 {
    let dot = dot_product_fixed(a, b);
    let mag_a = dot_product_fixed(a, a);
    let mag_b = dot_product_fixed(b, b);

    if mag_a == 0 || mag_b == 0 {
        return 0;
    }

    // Approximate: dot / sqrt(mag_a * mag_b)
    // Use integer sqrt and scale to Q8.8
    let denom = isqrt(mag_a as u64) as i64 * isqrt(mag_b as u64) as i64;
    if denom == 0 {
        return 0;
    }

    // Scale dot to Q8.8 range
    ((dot as i64 * 256) / denom) as i16
}

/// Softmax approximation in Q8.8 fixed-point.
///
/// Uses a piecewise linear approximation of exp() suitable for
/// constrained devices. Writes results into `out`.
///
/// Returns the number of elements written.
pub fn softmax_fixed(input: &[i16], out: &mut [i16]) -> usize {
    let len = input.len().min(out.len());
    if len == 0 {
        return 0;
    }

    // Find max for numerical stability
    let mut max_val = input[0];
    for item in input.iter().take(len).skip(1) {
        if *item > max_val {
            max_val = *item;
        }
    }

    // Approximate exp(x - max) using piecewise linear:
    // exp(x) ≈ max(0, 256 + x) for small x (in Q8.8)
    let mut exp_vals = [0u32; 256]; // Max 256 elements in no_std
    let actual_len = len.min(256);
    let mut sum: u32 = 0;

    for i in 0..actual_len {
        let shifted = (input[i] as i32) - (max_val as i32);
        // Clamp to reasonable range
        let approx = if shifted >= 0 {
            256u32
        } else if shifted < -1024 {
            1u32
        } else {
            (256i32 + shifted / 4).max(1) as u32
        };
        exp_vals[i] = approx;
        sum += approx;
    }

    if sum == 0 {
        sum = 1;
    }

    // Normalize to Q8.8 (output sums to ~256)
    for i in 0..actual_len {
        out[i] = ((exp_vals[i] * 256) / sum) as i16;
    }

    actual_len
}

/// Integer square root (binary search).
pub fn isqrt(val: u64) -> u64 {
    if val == 0 {
        return 0;
    }
    let mut lo: u64 = 1;
    let mut hi: u64 = val.min(0xFFFF_FFFF); // Cap to avoid overflow
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        if let Some(sq) = mid.checked_mul(mid) {
            if sq == val {
                return mid;
            } else if sq < val {
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        } else {
            hi = mid - 1;
        }
    }
    hi
}

/// Absolute value for i16 without branching.
pub const fn abs_i16(x: i16) -> i16 {
    let mask = x >> 15;
    (x ^ mask) - mask
}

/// Clamp an i16 to a range.
pub const fn clamp_i16(val: i16, min: i16, max: i16) -> i16 {
    if val < min {
        min
    } else if val > max {
        max
    } else {
        val
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dot_product_fixed() {
        // [1.0, 2.0, 3.0] in Q8.8 = [256, 512, 768]
        let a = [256i16, 512, 768];
        let b = [256i16, 256, 256];
        // Expected: 1*1 + 2*1 + 3*1 = 6.0 in Q16.16 = 6 * 65536
        // Actually: 256*256 + 512*256 + 768*256 = 65536 + 131072 + 196608 = 393216
        let result = dot_product_fixed(&a, &b);
        assert_eq!(result, 393216);
        // Convert to float: 393216 / 65536 = 6.0
        let float_result = result as f64 / 65536.0;
        assert!((float_result - 6.0).abs() < 0.01);
    }

    #[test]
    fn test_dot_product_empty() {
        assert_eq!(dot_product_fixed(&[], &[]), 0);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let v = [256i16, 256, 256]; // [1.0, 1.0, 1.0]
        let sim = cosine_similarity_fixed(&v, &v);
        // Should be close to 256 (1.0 in Q8.8)
        assert!(sim > 200, "Expected ~256, got {sim}");
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = [256i16, 0];
        let b = [0i16, 256];
        let sim = cosine_similarity_fixed(&a, &b);
        assert_eq!(sim, 0);
    }

    #[test]
    fn test_cosine_similarity_zero_vector() {
        let a = [256i16, 256];
        let b = [0i16, 0];
        assert_eq!(cosine_similarity_fixed(&a, &b), 0);
    }

    #[test]
    fn test_softmax_fixed() {
        let input = [256i16, 512, 768]; // [1.0, 2.0, 3.0]
        let mut out = [0i16; 3];
        let n = softmax_fixed(&input, &mut out);
        assert_eq!(n, 3);
        // Sum should be approximately 256 (1.0 in Q8.8)
        let sum: i16 = out.iter().sum();
        assert!(sum > 200 && sum < 300, "Softmax sum {sum} not near 256");
        // Largest input should have largest output
        assert!(out[2] >= out[1]);
        assert!(out[1] >= out[0]);
    }

    #[test]
    fn test_softmax_empty() {
        let mut out = [0i16; 0];
        assert_eq!(softmax_fixed(&[], &mut out), 0);
    }

    #[test]
    fn test_isqrt() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(100), 10);
        assert_eq!(isqrt(65536), 256);
    }

    #[test]
    fn test_isqrt_non_perfect() {
        // isqrt(10) should be 3 (floor)
        assert_eq!(isqrt(10), 3);
        assert_eq!(isqrt(99), 9);
    }

    #[test]
    fn test_abs_i16() {
        assert_eq!(abs_i16(5), 5);
        assert_eq!(abs_i16(-5), 5);
        assert_eq!(abs_i16(0), 0);
    }

    #[test]
    fn test_clamp_i16() {
        assert_eq!(clamp_i16(50, 0, 100), 50);
        assert_eq!(clamp_i16(-10, 0, 100), 0);
        assert_eq!(clamp_i16(200, 0, 100), 100);
    }
}
