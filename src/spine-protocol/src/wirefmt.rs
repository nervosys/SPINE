//! Serde helpers that give the v1 bodies their strict shape (C.5 rules 7 and 8
//! of `docs/spec/00-wire.md`).
//!
//! * [`reject_null`] and [`reject_null_value`]: an optional field is omitted
//!   when absent, and a present `null` is an error rather than `None`.
//! * [`bytes`], [`byte_array`] and [`opt_byte_array`]: byte fields are CBOR
//!   byte strings and nothing else. `serde_bytes` is not used for this
//!   because its deserialiser also accepts an array of integers, which would
//!   give one value two encodings. In a human-readable format (JSON, used for
//!   trace files and the HTTP gateway) the helpers accept an array of integers
//!   too, because JSON has no byte strings; CBOR is never human-readable.
//!
//! Use the byte helpers as `#[serde(with = "crate::wirefmt::bytes")]` on a
//! `Vec<u8>`, `#[serde(with = "crate::wirefmt::byte_array")]` on a `[u8; N]`
//! and `#[serde(default, with = "crate::wirefmt::opt_byte_array",
//! skip_serializing_if = "Option::is_none")]` on an `Option<[u8; N]>`.

use std::fmt;

use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// Deserialises an optional field so that a present `null` is rejected
/// (C.5 rule 7). A missing key is handled by `#[serde(default)]`.
///
/// For `T = serde_json::Value`, whose own `null` would be accepted, use
/// [`reject_null_value`].
pub fn reject_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Like [`reject_null`] for an optional [`serde_json::Value`]: a present
/// `null` is an error, every other value is `Some`.
pub fn reject_null_value<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Err(de::Error::custom(
            "a present null is not allowed for an optional field",
        ));
    }
    Ok(Some(value))
}

/// Deserialises a list that is omitted when empty: a present empty list is an
/// error, so a value has exactly one encoding. Pair it with `default` and
/// `skip_serializing_if = "Vec::is_empty"`.
pub fn non_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let list = Vec::<T>::deserialize(deserializer)?;
    if list.is_empty() {
        return Err(de::Error::custom("an empty list must be omitted, not sent"));
    }
    Ok(list)
}

/// Accepts a byte string; accepts an array of integers only when the format
/// is human-readable.
struct ByteVisitor {
    lenient: bool,
}

impl<'de> Visitor<'de> for ByteVisitor {
    type Value = Vec<u8>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a byte string")
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Vec<u8>, E> {
        Ok(v.to_vec())
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
        Ok(v)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
        if !self.lenient {
            return Err(de::Error::invalid_type(de::Unexpected::Seq, &self));
        }
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1 << 16));
        while let Some(byte) = seq.next_element::<u8>()? {
            out.push(byte);
        }
        Ok(out)
    }
}

fn read_bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    let lenient = deserializer.is_human_readable();
    // `deserialize_byte_buf`, not `deserialize_bytes`: ciborium's
    // `deserialize_bytes` refuses a byte string longer than its scratch buffer.
    deserializer.deserialize_byte_buf(ByteVisitor { lenient })
}

/// `with` module for a `Vec<u8>`: a CBOR byte string.
pub mod bytes {
    use super::*;

    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        read_bytes(deserializer)
    }
}

fn to_array<const N: usize, E: de::Error>(bytes: Vec<u8>) -> Result<[u8; N], E> {
    let len = bytes.len();
    <[u8; N]>::try_from(bytes)
        .map_err(|_| E::custom(format!("expected a byte string of {N} bytes, got {len}")))
}

/// `with` module for a `[u8; N]`: a CBOR byte string of exactly `N` bytes.
pub mod byte_array {
    use super::*;

    pub fn serialize<S: Serializer, const N: usize>(
        value: &[u8; N],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        deserializer: D,
    ) -> Result<[u8; N], D::Error> {
        to_array(read_bytes(deserializer)?)
    }
}

/// `with` module for an `Option<[u8; N]>`. Pair it with
/// `skip_serializing_if = "Option::is_none"` and `default`; a present `null`
/// is not a byte string and is rejected.
pub mod opt_byte_array {
    use super::*;

    pub fn serialize<S: Serializer, const N: usize>(
        value: &Option<[u8; N]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(bytes) => serializer.serialize_bytes(bytes),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        deserializer: D,
    ) -> Result<Option<[u8; N]>, D::Error> {
        to_array(read_bytes(deserializer)?).map(Some)
    }
}

/// A newtype used by tests and helpers that need a standalone byte string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteString(#[serde(with = "bytes")] pub Vec<u8>);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cbor;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Probe {
        #[serde(with = "bytes")]
        data: Vec<u8>,
        #[serde(with = "byte_array")]
        key: [u8; 4],
        #[serde(
            default,
            with = "opt_byte_array",
            skip_serializing_if = "Option::is_none"
        )]
        tag: Option<[u8; 2]>,
        #[serde(
            default,
            deserialize_with = "reject_null",
            skip_serializing_if = "Option::is_none"
        )]
        note: Option<String>,
    }

    fn probe() -> Probe {
        Probe {
            data: vec![1, 2, 3],
            key: [9; 4],
            tag: Some([7, 8]),
            note: None,
        }
    }

    #[test]
    fn bytes_are_byte_strings_and_round_trip() {
        let body = cbor::to_vec(&probe()).unwrap();
        // Keys sorted by their encodings: shorter keys first, so "key" and
        // "tag" (in that order) before "data".
        let expected: Vec<u8> = [
            &[0xa3u8][..],
            &[0x63, b'k', b'e', b'y', 0x44, 9, 9, 9, 9],
            &[0x63, b't', b'a', b'g', 0x42, 7, 8],
            &[0x64, b'd', b'a', b't', b'a', 0x43, 1, 2, 3],
        ]
        .concat();
        assert_eq!(body, expected);
        assert_eq!(cbor::from_slice::<Probe>(&body).unwrap(), probe());
    }

    #[test]
    fn a_long_byte_string_survives() {
        let big = Probe {
            data: vec![0xab; 200_000],
            ..probe()
        };
        let body = cbor::to_vec(&big).unwrap();
        assert!(body.len() < 200_100);
        assert_eq!(cbor::from_slice::<Probe>(&body).unwrap(), big);
    }

    #[test]
    fn an_array_of_integers_is_not_a_byte_string() {
        let as_array = serde_json::json!({"data": [1, 2, 3], "key": [9, 9, 9, 9]});
        let body = cbor::to_vec(&as_array).unwrap();
        assert!(cbor::from_slice::<Probe>(&body).is_err());
    }

    #[test]
    fn a_byte_array_of_the_wrong_length_is_rejected() {
        #[derive(Serialize)]
        struct Short {
            #[serde(with = "bytes")]
            data: Vec<u8>,
            #[serde(with = "bytes")]
            key: Vec<u8>,
        }
        let body = cbor::to_vec(&Short {
            data: vec![],
            key: vec![1, 2, 3],
        })
        .unwrap();
        assert!(cbor::from_slice::<Probe>(&body).is_err());
    }

    #[test]
    fn a_present_null_is_rejected_for_optionals() {
        #[derive(Serialize)]
        struct WithNull {
            #[serde(with = "bytes")]
            data: Vec<u8>,
            #[serde(with = "byte_array")]
            key: [u8; 4],
            note: Option<String>,
        }
        let body = cbor::to_vec(&WithNull {
            data: vec![],
            key: [0; 4],
            note: None,
        })
        .unwrap();
        assert!(cbor::from_slice::<Probe>(&body).is_err());
        #[derive(Serialize)]
        struct TagNull {
            #[serde(with = "bytes")]
            data: Vec<u8>,
            #[serde(with = "byte_array")]
            key: [u8; 4],
            tag: Option<Vec<u8>>,
        }
        let body = cbor::to_vec(&TagNull {
            data: vec![],
            key: [0; 4],
            tag: None,
        })
        .unwrap();
        assert!(cbor::from_slice::<Probe>(&body).is_err());
    }

    #[test]
    fn json_accepts_an_array_of_integers() {
        let text = r#"{"data":[1,2,3],"key":[9,9,9,9]}"#;
        let back: Probe = serde_json::from_str(text).unwrap();
        assert_eq!(back.data, vec![1, 2, 3]);
        assert_eq!(back.key, [9; 4]);
    }

    #[test]
    fn reject_null_value_refuses_json_null_only() {
        #[derive(Debug, Deserialize)]
        struct V {
            #[serde(default, deserialize_with = "reject_null_value")]
            v: Option<serde_json::Value>,
        }
        assert!(serde_json::from_str::<V>(r#"{"v":null}"#).is_err());
        assert_eq!(
            serde_json::from_str::<V>(r#"{"v":0}"#).unwrap().v,
            Some(serde_json::json!(0))
        );
        assert!(serde_json::from_str::<V>("{}").unwrap().v.is_none());
    }
}
