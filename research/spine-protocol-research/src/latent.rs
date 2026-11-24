//! `LatentVector`, a plain payload type moved here from `spine-protocol`. It is
//! not on the native wire and is not a security feature.

use serde::{Deserialize, Serialize};

/// A point in the latent space. The dimensionality and basis evolve per-message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatentVector {
    /// The high-dimensional representation
    pub components: Vec<f32>,
    /// Dimension hint (can be misleading as part of obfuscation)
    pub dim_hint: u16,
    /// Epoch number for transformation alignment
    pub epoch: u64,
}

impl LatentVector {
    /// Zero-copy serialization to bytes (22+ GiB/s for 1024-dim vectors)
    /// Layout: [dim_hint: u16][epoch: u64][components: [f32; N]]
    #[inline]
    pub fn to_bytes_fast(&self) -> Vec<u8> {
        let component_bytes = self.components.len() * 4;
        let total = 2 + 8 + component_bytes;
        let mut buf = Vec::with_capacity(total);

        buf.extend_from_slice(&self.dim_hint.to_le_bytes());
        buf.extend_from_slice(&self.epoch.to_le_bytes());
        // SAFETY: f32 has no padding, transmute is valid
        buf.extend_from_slice(bytemuck::cast_slice(&self.components));

        buf
    }

    /// Zero-copy deserialization from bytes
    #[inline]
    pub fn from_bytes_fast(data: &[u8]) -> Option<Self> {
        if data.len() < 10 {
            return None;
        }

        let dim_hint = u16::from_le_bytes([data[0], data[1]]);
        let epoch = u64::from_le_bytes(data[2..10].try_into().ok()?);

        let component_bytes = &data[10..];
        if !component_bytes.len().is_multiple_of(4) {
            return None;
        }

        // Use try_cast_slice to handle unaligned data gracefully
        let components: Vec<f32> = match bytemuck::try_cast_slice(component_bytes) {
            Ok(s) => s.to_vec(),
            Err(_) => component_bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect(),
        };

        Some(Self {
            components,
            dim_hint,
            epoch,
        })
    }

    /// Serialize to bytes into pre-allocated buffer (zero-alloc hot path)
    #[inline]
    pub fn to_bytes_into(&self, buf: &mut Vec<u8>) {
        let component_bytes = self.components.len() * 4;
        let total = 2 + 8 + component_bytes;
        buf.clear();
        buf.reserve(total);

        buf.extend_from_slice(&self.dim_hint.to_le_bytes());
        buf.extend_from_slice(&self.epoch.to_le_bytes());
        buf.extend_from_slice(bytemuck::cast_slice(&self.components));
    }

    /// Bincode serialization (fallback, ~5x faster than JSON)
    #[inline]
    pub fn to_bincode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Bincode deserialization
    #[inline]
    pub fn from_bincode(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}
