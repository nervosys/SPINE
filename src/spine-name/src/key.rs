//! The 256-bit keyspace names and nodes share.
//!
//! Names and nodes are hashed into one space so "which node should hold this
//! record" is answerable by arithmetic instead of by a directory. XOR distance
//! gives that space the property Kademlia relies on: it is a metric (symmetric,
//! and it obeys the triangle inequality), so every hop toward a target makes
//! monotonic progress and lookups converge in O(log n) hops.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

use crate::base32;

/// A 256-bit point in the shared keyspace — either a name's routing key or a
/// node's identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NameKey(#[serde(with = "key_bytes")] pub [u8; 32]);

impl NameKey {
    /// The key of some bytes.
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        NameKey(hasher.finalize().into())
    }

    /// Wrap raw bytes that are already a key (e.g. an Ed25519 public key used
    /// directly as a node id).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        NameKey(bytes)
    }

    /// The raw bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// XOR distance to another key.
    pub fn distance(&self, other: &NameKey) -> Distance {
        let mut out = [0u8; 32];
        for (o, (a, b)) in out.iter_mut().zip(self.0.iter().zip(other.0.iter())) {
            *o = a ^ b;
        }
        Distance(out)
    }

    /// The k-bucket index for `other` relative to `self`: the position of the
    /// highest set bit of the XOR distance, i.e. the length of the shared
    /// prefix. `None` when the keys are identical (a node never buckets itself).
    pub fn bucket_index(&self, other: &NameKey) -> Option<usize> {
        let d = self.distance(other);
        d.leading_zeros().map(|lz| 255 - lz)
    }

    /// Base32 spelling, matching how the key appears inside a `spine://` name.
    pub fn to_base32(&self) -> String {
        base32::encode(&self.0)
    }
}

impl fmt::Display for NameKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_base32())
    }
}

/// XOR distance between two keys. Ordered big-endian, so the derived `Ord` is
/// numeric order and `sort()` ranks nearest-first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Distance(pub [u8; 32]);

impl Distance {
    /// Number of leading zero bits; `None` if the distance is zero (same key).
    pub fn leading_zeros(&self) -> Option<usize> {
        for (i, byte) in self.0.iter().enumerate() {
            if *byte != 0 {
                return Some(i * 8 + byte.leading_zeros() as usize);
            }
        }
        None
    }

    /// Whether this is the zero distance.
    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }
}

/// Serde helper: keys travel as base32 strings in JSON (readable in mesh traces)
/// but stay a fixed 32-byte array in memory.
mod key_bytes {
    use super::base32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base32::encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let raw = String::deserialize(d)?;
        base32::decode_key(&raw)
            .ok_or_else(|| serde::de::Error::custom("expected 32-byte base32 key"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_is_deterministic_and_distinguishing() {
        assert_eq!(NameKey::of(b"a"), NameKey::of(b"a"));
        assert_ne!(NameKey::of(b"a"), NameKey::of(b"b"));
    }

    #[test]
    fn distance_to_self_is_zero() {
        let k = NameKey::of(b"node");
        assert!(k.distance(&k).is_zero());
        assert_eq!(k.bucket_index(&k), None);
    }

    #[test]
    fn distance_is_symmetric() {
        let a = NameKey::of(b"a");
        let b = NameKey::of(b"b");
        assert_eq!(a.distance(&b), b.distance(&a));
    }

    #[test]
    fn distance_obeys_the_triangle_inequality() {
        // For XOR, d(a,c) <= d(a,b) | d(b,c) bitwise, which implies the metric
        // property the routing argument depends on.
        let a = NameKey::of(b"alpha");
        let b = NameKey::of(b"beta");
        let c = NameKey::of(b"gamma");
        let ac = a.distance(&c);
        let ab = a.distance(&b);
        let bc = b.distance(&c);
        for i in 0..32 {
            assert_eq!(ac.0[i], ab.0[i] ^ bc.0[i]);
        }
    }

    #[test]
    fn bucket_index_tracks_shared_prefix_length() {
        let base = NameKey::from_bytes([0u8; 32]);

        // Differ in the top bit -> longest distance -> highest bucket.
        let mut top = [0u8; 32];
        top[0] = 0b1000_0000;
        assert_eq!(base.bucket_index(&NameKey::from_bytes(top)), Some(255));

        // Differ only in the last bit -> nearest -> bucket 0.
        let mut bottom = [0u8; 32];
        bottom[31] = 1;
        assert_eq!(base.bucket_index(&NameKey::from_bytes(bottom)), Some(0));
    }

    #[test]
    fn ordering_ranks_nearer_keys_first() {
        let target = NameKey::from_bytes([0u8; 32]);
        let near = NameKey::from_bytes({
            let mut b = [0u8; 32];
            b[31] = 1;
            b
        });
        let far = NameKey::from_bytes({
            let mut b = [0u8; 32];
            b[0] = 0xff;
            b
        });
        assert!(target.distance(&near) < target.distance(&far));
    }

    #[test]
    fn serde_roundtrips_as_base32() {
        let k = NameKey::of(b"payload");
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, format!("\"{}\"", k.to_base32()));
        assert_eq!(serde_json::from_str::<NameKey>(&json).unwrap(), k);
    }

    #[test]
    fn serde_rejects_a_key_of_the_wrong_length() {
        assert!(serde_json::from_str::<NameKey>("\"mzxw6\"").is_err());
    }
}
