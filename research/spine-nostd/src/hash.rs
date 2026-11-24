//! Lightweight hashing for no_std environments.

/// FNV-1a 32-bit hash.
pub fn fnv1a_32(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &byte in data {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// FNV-1a 64-bit hash.
pub fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// Simple hash combiner for multiple fields.
pub fn hash_combine(a: u64, b: u64) -> u64 {
    a.wrapping_mul(0x517c_c1b7_2722_0a95).wrapping_add(b)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fnv1a_32_known() {
        // FNV-1a of empty string
        assert_eq!(fnv1a_32(b""), 0x811c_9dc5);
    }

    #[test]
    fn test_fnv1a_32_deterministic() {
        let h1 = fnv1a_32(b"spine");
        let h2 = fnv1a_32(b"spine");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_fnv1a_32_differs() {
        assert_ne!(fnv1a_32(b"hello"), fnv1a_32(b"world"));
    }

    #[test]
    fn test_fnv1a_64_known() {
        assert_eq!(fnv1a_64(b""), 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn test_fnv1a_64_deterministic() {
        let h1 = fnv1a_64(b"agent");
        let h2 = fnv1a_64(b"agent");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_combine_order_matters() {
        let a = fnv1a_64(b"foo");
        let b = fnv1a_64(b"bar");
        assert_ne!(hash_combine(a, b), hash_combine(b, a));
    }
}
