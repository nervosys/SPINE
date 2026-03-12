//! Core data types for no_std environments.

/// Agent identifier as raw 16 bytes (UUID-compatible).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AgentIdBytes(pub [u8; 16]);

impl AgentIdBytes {
    /// Create a zeroed agent ID.
    pub const fn zero() -> Self {
        Self([0u8; 16])
    }

    /// Create from raw bytes.
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Get the raw bytes.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl core::fmt::Debug for AgentIdBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "AgentId(")?;
        for (i, b) in self.0.iter().enumerate() {
            if i == 4 || i == 6 || i == 8 || i == 10 {
                write!(f, "-")?;
            }
            write!(f, "{b:02x}")?;
        }
        write!(f, ")")
    }
}

impl core::fmt::Display for AgentIdBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if i == 4 || i == 6 || i == 8 || i == 10 {
                write!(f, "-")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// Fixed-size latent vector for neural encoding (no heap allocation).
///
/// Uses `i16` fixed-point representation (Q8.8 format) instead of `f32`
/// to avoid floating-point dependency on constrained targets.
#[derive(Clone, Copy)]
pub struct LatentVectorFixed<const N: usize> {
    /// Fixed-point components (Q8.8: value = raw / 256.0).
    pub data: [i16; N],
    /// Number of valid components.
    pub len: usize,
}

impl<const N: usize> LatentVectorFixed<N> {
    /// Create a zeroed latent vector.
    pub const fn zero() -> Self {
        Self {
            data: [0i16; N],
            len: 0,
        }
    }

    /// Create from a slice (copies up to N elements).
    pub fn from_slice(slice: &[i16]) -> Self {
        let mut data = [0i16; N];
        let len = slice.len().min(N);
        let (dst, _) = data.split_at_mut(len);
        dst.copy_from_slice(&slice[..len]);
        Self { data, len }
    }

    /// Convert an f32 to Q8.8 fixed-point.
    pub fn from_f32(val: f32) -> i16 {
        (val * 256.0) as i16
    }

    /// Convert Q8.8 fixed-point back to f32.
    pub fn to_f32(val: i16) -> f32 {
        val as f32 / 256.0
    }

    /// Get the valid portion of the vector.
    pub fn as_slice(&self) -> &[i16] {
        &self.data[..self.len]
    }
}

impl<const N: usize> core::fmt::Debug for LatentVectorFixed<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "LatentVector({} dims)", self.len)
    }
}

/// Wire-format frame header (12 bytes, no heap allocation).
///
/// Layout:
/// ```text
/// [0..4]   payload_len: u32 (big-endian)
/// [4..5]   flags: u8
/// [5..6]   frame_type: u8
/// [6..8]   sequence: u16 (big-endian)
/// [8..12]  checksum: u32 (big-endian)
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameHeader {
    pub payload_len: u32,
    pub flags: u8,
    pub frame_type: u8,
    pub sequence: u16,
    pub checksum: u32,
}

impl FrameHeader {
    /// Size of the header on the wire.
    pub const SIZE: usize = 12;

    /// Create a new frame header.
    pub const fn new(payload_len: u32, frame_type: u8, sequence: u16) -> Self {
        Self {
            payload_len,
            flags: 0,
            frame_type,
            sequence,
            checksum: 0,
        }
    }

    /// Set the compressed flag.
    pub const fn with_compressed(mut self) -> Self {
        self.flags |= 0x01;
        self
    }

    /// Set the encrypted flag.
    pub const fn with_encrypted(mut self) -> Self {
        self.flags |= 0x02;
        self
    }

    /// Check if compressed.
    pub const fn is_compressed(&self) -> bool {
        self.flags & 0x01 != 0
    }

    /// Check if encrypted.
    pub const fn is_encrypted(&self) -> bool {
        self.flags & 0x02 != 0
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    extern crate std;
    use std::format;
    use super::*;

    #[test]
    fn test_agent_id_zero() {
        let id = AgentIdBytes::zero();
        assert_eq!(id.as_bytes(), &[0u8; 16]);
    }

    #[test]
    fn test_agent_id_roundtrip() {
        let bytes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let id = AgentIdBytes::from_bytes(bytes);
        assert_eq!(*id.as_bytes(), bytes);
    }

    #[test]
    fn test_agent_id_display() {
        let bytes = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
                     0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10];
        let id = AgentIdBytes::from_bytes(bytes);
        let s = format!("{id}");
        assert_eq!(s, "01020304-0506-0708-090a-0b0c0d0e0f10");
    }

    #[test]
    fn test_latent_vector_zero() {
        let v = LatentVectorFixed::<64>::zero();
        assert_eq!(v.len, 0);
        assert_eq!(v.data[0], 0);
    }

    #[test]
    fn test_latent_vector_from_slice() {
        let data = [100i16, 200, -300, 400];
        let v = LatentVectorFixed::<8>::from_slice(&data);
        assert_eq!(v.len, 4);
        assert_eq!(v.as_slice(), &[100, 200, -300, 400]);
    }

    #[test]
    fn test_fixed_point_conversion() {
        let fp = LatentVectorFixed::<1>::from_f32(1.5);
        assert_eq!(fp, 384); // 1.5 * 256 = 384
        let back = LatentVectorFixed::<1>::to_f32(384);
        assert!((back - 1.5).abs() < 0.01);
    }

    #[test]
    fn test_frame_header_new() {
        let h = FrameHeader::new(1024, 1, 42);
        assert_eq!(h.payload_len, 1024);
        assert_eq!(h.frame_type, 1);
        assert_eq!(h.sequence, 42);
        assert!(!h.is_compressed());
        assert!(!h.is_encrypted());
    }

    #[test]
    fn test_frame_header_flags() {
        let h = FrameHeader::new(0, 0, 0).with_compressed().with_encrypted();
        assert!(h.is_compressed());
        assert!(h.is_encrypted());
    }
}
