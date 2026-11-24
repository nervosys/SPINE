//! Serde helpers that make byte strings travel as CBOR byte strings.
//!
//! serde writes `[u8; N]` and `Vec<u8>` as arrays of integers unless told
//! otherwise, which would make every key, hash and signature several times
//! larger and ambiguous with a list of numbers. These modules are used through
//! `#[serde(with = "...")]` on the fields that hold such values. On input only
//! a CBOR byte string of the right length is accepted; an array of integers is
//! refused.

/// A byte string of exactly `N` bytes.
pub mod fixed {
    use serde::de::{self, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    /// Writes `value` as one byte string.
    pub fn serialize<S: Serializer, const N: usize>(
        value: &[u8; N],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(value.as_slice())
    }

    struct FixedVisitor<const N: usize>;

    impl<'de, const N: usize> Visitor<'de> for FixedVisitor<N> {
        type Value = [u8; N];

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "a byte string of exactly {} bytes", N)
        }

        fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<[u8; N], E> {
            match <[u8; N]>::try_from(v) {
                Ok(array) => Ok(array),
                Err(_) => Err(E::invalid_length(v.len(), &self)),
            }
        }

        fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<[u8; N], E> {
            self.visit_bytes(v.as_slice())
        }
    }

    /// Reads one byte string of exactly `N` bytes.
    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        deserializer: D,
    ) -> Result<[u8; N], D::Error> {
        deserializer.deserialize_bytes(FixedVisitor::<N>)
    }
}

/// A byte string of any length.
pub mod variable {
    use serde::de::{self, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    /// Writes `value` as one byte string.
    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(value)
    }

    struct VariableVisitor;

    impl<'de> Visitor<'de> for VariableVisitor {
        type Value = Vec<u8>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a byte string")
        }

        fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
            Ok(v.to_vec())
        }

        fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
            Ok(v)
        }
    }

    /// Reads one byte string.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        deserializer.deserialize_bytes(VariableVisitor)
    }
}

/// An optional 32-byte string: CBOR `null` or a byte string of 32 bytes.
pub mod optional_id {
    use serde::de::{self, Visitor};
    use serde::{Deserializer, Serializer};
    use std::fmt;

    /// Writes `null` or one byte string.
    pub fn serialize<S: Serializer>(
        value: &Option<[u8; 32]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(bytes) => serializer.serialize_bytes(bytes.as_slice()),
            None => serializer.serialize_none(),
        }
    }

    struct OptionalIdVisitor;

    impl<'de> Visitor<'de> for OptionalIdVisitor {
        type Value = Option<[u8; 32]>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("null or a byte string of exactly 32 bytes")
        }

        fn visit_none<E: de::Error>(self) -> Result<Option<[u8; 32]>, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Option<[u8; 32]>, E> {
            Ok(None)
        }

        fn visit_some<D: Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Option<[u8; 32]>, D::Error> {
            let bytes: [u8; 32] = super::fixed::deserialize(deserializer)?;
            Ok(Some(bytes))
        }
    }

    /// Reads `null` or one byte string of 32 bytes.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<[u8; 32]>, D::Error> {
        deserializer.deserialize_option(OptionalIdVisitor)
    }
}
