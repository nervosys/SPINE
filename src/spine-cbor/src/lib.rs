//! Deterministic CBOR for the spine-wire v1 body (`docs/spec/00-wire.md`, C.5).
//!
//! A body is exactly one CBOR data item that satisfies RFC 8949 section 4.2.1
//! plus the v1 rules:
//!
//! 1. definite lengths only;
//! 2. integers and lengths in the shortest form, floats in the shortest of
//!    binary16, binary32 and binary64 that preserves the value, NaN as
//!    `f9 7e 00`;
//! 3. map keys in strictly ascending bytewise order of their encodings (which
//!    also rejects duplicates);
//! 4. no tags, simple values limited to `false`, `true` and `null`;
//! 5. text strings are valid UTF-8;
//! 6. nothing follows the item, and nesting is at most [`MAX_DEPTH`].
//!
//! [`check_deterministic`] is a hand-written scanner over the raw bytes. It
//! never allocates, never panics and checks every declared length against the
//! remaining input before using it. [`to_vec`] serialises through
//! [`ciborium::Value`] and writes the bytes with its own encoder, so the
//! output does not depend on how ciborium's writer orders or sizes anything.
//! [`from_slice`] validates first and only then hands the bytes to ciborium.

use ciborium::Value;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Maximum nesting depth of arrays and maps. Scalars do not count: a body of
/// 32 nested arrays is accepted, 33 is not.
pub const MAX_DEPTH: usize = 32;

/// Why a body was rejected, or why a value could not be encoded or decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CborError {
    /// An indefinite-length string, array or map.
    #[error("indefinite length is not allowed")]
    IndefiniteLength,
    /// An integer, string length, array length or map length that is not in its
    /// shortest head form.
    #[error("integer or length is not in shortest form")]
    NonShortestInteger,
    /// A float that fits a narrower width without loss.
    #[error("float is not in shortest form")]
    NonShortestFloat,
    /// A NaN other than `f9 7e 00`.
    #[error("NaN is not the canonical f9 7e 00")]
    NonCanonicalNan,
    /// A map key that is not strictly greater than the previous key, or an
    /// equal key.
    #[error("map keys are unsorted or duplicated")]
    UnsortedOrDuplicateKey,
    /// A tagged item.
    #[error("tags are not allowed")]
    Tag,
    /// A simple value other than `false`, `true` and `null` (including
    /// `undefined` and every `f8 xx` form).
    #[error("simple value is not allowed")]
    UnsupportedSimple,
    /// A text string that is not valid UTF-8.
    #[error("text string is not valid UTF-8")]
    InvalidUtf8,
    /// A break byte (`ff`) outside an indefinite-length item.
    #[error("unexpected break")]
    UnexpectedBreak,
    /// A head with an additional-information value of 28 to 31 where none is
    /// defined.
    #[error("reserved additional information value")]
    ReservedInfo,
    /// Bytes remain after the item.
    #[error("trailing bytes after the item")]
    TrailingBytes,
    /// The input ends inside an item, or a declared length exceeds the
    /// remaining input.
    #[error("input is truncated or a length exceeds the input")]
    Truncated,
    /// Arrays and maps are nested deeper than [`MAX_DEPTH`].
    #[error("nesting is deeper than the limit")]
    DepthExceeded,
    /// A value could not be encoded.
    #[error("encode error: {0}")]
    Encode(String),
    /// The bytes are well formed and deterministic but do not match the target
    /// type.
    #[error("decode error: {0}")]
    Decode(String),
}

// ---------------------------------------------------------------------------
// Scanner
// ---------------------------------------------------------------------------

/// Takes `n` bytes at `*pos`, or fails with `Truncated` without panicking.
fn take<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], CborError> {
    let end = pos.checked_add(n).ok_or(CborError::Truncated)?;
    let slice = data.get(*pos..end).ok_or(CborError::Truncated)?;
    *pos = end;
    Ok(slice)
}

/// Reads the argument of a head whose additional information is `info`
/// (0 to 30), enforcing the shortest form.
fn read_arg(data: &[u8], pos: &mut usize, info: u8) -> Result<u64, CborError> {
    match info {
        0..=23 => Ok(u64::from(info)),
        24 => {
            let b = take(data, pos, 1)?;
            let v = u64::from(b[0]);
            if v < 24 {
                Err(CborError::NonShortestInteger)
            } else {
                Ok(v)
            }
        }
        25 => {
            let b = take(data, pos, 2)?;
            let v = u64::from(u16::from_be_bytes([b[0], b[1]]));
            if v <= 0xff {
                Err(CborError::NonShortestInteger)
            } else {
                Ok(v)
            }
        }
        26 => {
            let b = take(data, pos, 4)?;
            let v = u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
            if v <= 0xffff {
                Err(CborError::NonShortestInteger)
            } else {
                Ok(v)
            }
        }
        27 => {
            let b = take(data, pos, 8)?;
            let v = u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
            if v <= 0xffff_ffff {
                Err(CborError::NonShortestInteger)
            } else {
                Ok(v)
            }
        }
        _ => Err(CborError::ReservedInfo),
    }
}

/// The binary16 bit pattern of `x` if it is exactly representable, else `None`.
/// NaN is never representable here (it has its own canonical form).
fn f64_to_half_bits(x: f64) -> Option<u16> {
    let bits = x.to_bits();
    let sign: u16 = if bits >> 63 == 1 { 0x8000 } else { 0 };
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let man = bits & 0x000f_ffff_ffff_ffff;
    if exp == 0x7ff {
        return if man == 0 { Some(sign | 0x7c00) } else { None };
    }
    if exp == 0 {
        return if man == 0 { Some(sign) } else { None };
    }
    let e = exp - 1023;
    if (-14..=15).contains(&e) {
        if man & ((1u64 << 42) - 1) != 0 {
            return None;
        }
        let half_exp = (e + 15) as u16;
        let half_man = (man >> 42) as u16;
        return Some(sign | (half_exp << 10) | half_man);
    }
    if (-24..=-15).contains(&e) {
        let shift = (28 - e) as u32;
        let sig = man | (1u64 << 52);
        if sig & ((1u64 << shift) - 1) != 0 {
            return None;
        }
        return Some(sign | ((sig >> shift) as u16));
    }
    None
}

/// True when `x` survives a round trip through binary32.
fn fits_f32(x: f64) -> bool {
    f64::from(x as f32).to_bits() == x.to_bits()
}

/// Scans one item starting at `*pos`, advancing `*pos` past it.
fn scan(data: &[u8], pos: &mut usize, depth: usize) -> Result<(), CborError> {
    let first = take(data, pos, 1)?;
    let initial = first[0];
    let major = initial >> 5;
    let info = initial & 0x1f;
    match major {
        0..=1 => {
            read_arg(data, pos, info)?;
            Ok(())
        }
        2..=3 => {
            if info == 31 {
                return Err(CborError::IndefiniteLength);
            }
            let n = read_arg(data, pos, info)?;
            let len = usize::try_from(n).map_err(|_| CborError::Truncated)?;
            let body = take(data, pos, len)?;
            if major == 3 && std::str::from_utf8(body).is_err() {
                return Err(CborError::InvalidUtf8);
            }
            Ok(())
        }
        4 => {
            if info == 31 {
                return Err(CborError::IndefiniteLength);
            }
            let n = read_arg(data, pos, info)?;
            let inner = depth + 1;
            if inner > MAX_DEPTH {
                return Err(CborError::DepthExceeded);
            }
            let count = usize::try_from(n).map_err(|_| CborError::Truncated)?;
            // Every element takes at least one byte.
            if count > data.len().saturating_sub(*pos) {
                return Err(CborError::Truncated);
            }
            for _ in 0..count {
                scan(data, pos, inner)?;
            }
            Ok(())
        }
        5 => {
            if info == 31 {
                return Err(CborError::IndefiniteLength);
            }
            let n = read_arg(data, pos, info)?;
            let inner = depth + 1;
            if inner > MAX_DEPTH {
                return Err(CborError::DepthExceeded);
            }
            let count = usize::try_from(n).map_err(|_| CborError::Truncated)?;
            // Every entry takes at least two bytes.
            if count > data.len().saturating_sub(*pos) / 2 {
                return Err(CborError::Truncated);
            }
            let mut previous: Option<&[u8]> = None;
            for _ in 0..count {
                let start = *pos;
                scan(data, pos, inner)?;
                let key = data.get(start..*pos).ok_or(CborError::Truncated)?;
                if let Some(prev) = previous {
                    if key <= prev {
                        return Err(CborError::UnsortedOrDuplicateKey);
                    }
                }
                previous = Some(key);
                scan(data, pos, inner)?;
            }
            Ok(())
        }
        6 => Err(CborError::Tag),
        _ => match info {
            20..=22 => Ok(()),
            25 => {
                let b = take(data, pos, 2)?;
                let bits = u16::from_be_bytes([b[0], b[1]]);
                let is_nan = bits & 0x7c00 == 0x7c00 && bits & 0x03ff != 0;
                if is_nan && bits != 0x7e00 {
                    return Err(CborError::NonCanonicalNan);
                }
                Ok(())
            }
            26 => {
                let b = take(data, pos, 4)?;
                let f = f32::from_bits(u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
                if f.is_nan() {
                    return Err(CborError::NonCanonicalNan);
                }
                if f64_to_half_bits(f64::from(f)).is_some() {
                    return Err(CborError::NonShortestFloat);
                }
                Ok(())
            }
            27 => {
                let b = take(data, pos, 8)?;
                let f = f64::from_bits(u64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ]));
                if f.is_nan() {
                    return Err(CborError::NonCanonicalNan);
                }
                if fits_f32(f) {
                    return Err(CborError::NonShortestFloat);
                }
                Ok(())
            }
            24 => {
                take(data, pos, 1)?;
                Err(CborError::UnsupportedSimple)
            }
            28..=30 => Err(CborError::ReservedInfo),
            31 => Err(CborError::UnexpectedBreak),
            _ => Err(CborError::UnsupportedSimple),
        },
    }
}

/// Checks that `bytes` is exactly one CBOR item satisfying the v1 rules of
/// spec C.5 (rules 1 to 6). Allocation-free, depth-bounded by [`MAX_DEPTH`],
/// and free of panics on any input.
pub fn check_deterministic(bytes: &[u8]) -> Result<(), CborError> {
    let mut pos = 0usize;
    scan(bytes, &mut pos, 0)?;
    if pos != bytes.len() {
        return Err(CborError::TrailingBytes);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------------

/// Writes a head with the shortest form of `arg`.
fn write_head(out: &mut Vec<u8>, major: u8, arg: u64) {
    let m = major << 5;
    if arg < 24 {
        out.push(m | (arg as u8));
    } else if arg <= 0xff {
        out.push(m | 24);
        out.push(arg as u8);
    } else if arg <= 0xffff {
        out.push(m | 25);
        out.extend_from_slice(&(arg as u16).to_be_bytes());
    } else if arg <= 0xffff_ffff {
        out.push(m | 26);
        out.extend_from_slice(&(arg as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&arg.to_be_bytes());
    }
}

/// Writes a float in the shortest width that preserves it; NaN is `f9 7e 00`.
fn write_float(out: &mut Vec<u8>, f: f64) {
    if f.is_nan() {
        out.extend_from_slice(&[0xf9, 0x7e, 0x00]);
    } else if let Some(half) = f64_to_half_bits(f) {
        out.push(0xf9);
        out.extend_from_slice(&half.to_be_bytes());
    } else if fits_f32(f) {
        out.push(0xfa);
        out.extend_from_slice(&(f as f32).to_bits().to_be_bytes());
    } else {
        out.push(0xfb);
        out.extend_from_slice(&f.to_bits().to_be_bytes());
    }
}

/// Writes one value with deterministic rules.
fn encode_value(out: &mut Vec<u8>, value: &Value, depth: usize) -> Result<(), CborError> {
    match value {
        Value::Integer(i) => {
            let n = i128::from(*i);
            if n >= 0 {
                let arg = u64::try_from(n)
                    .map_err(|_| CborError::Encode("integer out of range".to_string()))?;
                write_head(out, 0, arg);
            } else {
                let arg = u64::try_from(!n)
                    .map_err(|_| CborError::Encode("integer out of range".to_string()))?;
                write_head(out, 1, arg);
            }
            Ok(())
        }
        Value::Bytes(b) => {
            write_head(out, 2, b.len() as u64);
            out.extend_from_slice(b);
            Ok(())
        }
        Value::Text(s) => {
            write_head(out, 3, s.len() as u64);
            out.extend_from_slice(s.as_bytes());
            Ok(())
        }
        Value::Float(f) => {
            write_float(out, *f);
            Ok(())
        }
        Value::Bool(b) => {
            out.push(if *b { 0xf5 } else { 0xf4 });
            Ok(())
        }
        Value::Null => {
            out.push(0xf6);
            Ok(())
        }
        Value::Tag(_, _) => Err(CborError::Tag),
        Value::Array(items) => {
            let inner = depth + 1;
            if inner > MAX_DEPTH {
                return Err(CborError::DepthExceeded);
            }
            write_head(out, 4, items.len() as u64);
            for item in items {
                encode_value(out, item, inner)?;
            }
            Ok(())
        }
        Value::Map(entries) => {
            let inner = depth + 1;
            if inner > MAX_DEPTH {
                return Err(CborError::DepthExceeded);
            }
            let mut encoded: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(entries.len());
            for (k, v) in entries {
                let mut key_bytes = Vec::new();
                encode_value(&mut key_bytes, k, inner)?;
                let mut value_bytes = Vec::new();
                encode_value(&mut value_bytes, v, inner)?;
                encoded.push((key_bytes, value_bytes));
            }
            encoded.sort();
            for pair in encoded.windows(2) {
                if pair[0].0 == pair[1].0 {
                    return Err(CborError::UnsortedOrDuplicateKey);
                }
            }
            write_head(out, 5, encoded.len() as u64);
            for (key_bytes, value_bytes) in &encoded {
                out.extend_from_slice(key_bytes);
                out.extend_from_slice(value_bytes);
            }
            Ok(())
        }
        _ => Err(CborError::Encode("unsupported value kind".to_string())),
    }
}

/// Serialises `value` to deterministic CBOR. The value is first serialised to
/// a [`ciborium::Value`]; map entries are then sorted by their encoded key
/// bytes and written with shortest heads and shortest floats. Duplicate map
/// keys and tags are errors. The output is always re-checked with
/// [`check_deterministic`].
pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>, CborError> {
    let tree = Value::serialized(value).map_err(|e| CborError::Encode(e.to_string()))?;
    let mut out = Vec::new();
    encode_value(&mut out, &tree, 0)?;
    check_deterministic(&out)?;
    Ok(out)
}

/// Decodes `bytes` into `T`. The bytes must first pass
/// [`check_deterministic`]; only then are they given to ciborium, so a body
/// that decodes always re-encodes to the identical bytes (for types whose
/// serialisation is itself canonical).
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CborError> {
    check_deterministic(bytes)?;
    ciborium::de::from_reader(bytes).map_err(|e| CborError::Decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::HashMap;

    fn hex(s: &str) -> Vec<u8> {
        let digits: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
        digits
            .chunks(2)
            .map(|pair| {
                let text: String = pair.iter().collect();
                u8::from_str_radix(&text, 16).unwrap()
            })
            .collect()
    }

    fn accepts(h: &str) {
        let bytes = hex(h);
        assert_eq!(check_deterministic(&bytes), Ok(()), "should accept {h}");
    }

    fn rejects(h: &str, expected: CborError) {
        let bytes = hex(h);
        assert_eq!(
            check_deterministic(&bytes),
            Err(expected),
            "wrong result for {h}"
        );
    }

    fn encodes<T: Serialize>(value: &T, h: &str) {
        let got = to_vec(value).unwrap();
        assert_eq!(got, hex(h), "encoding mismatch, expected {h}");
        assert_eq!(check_deterministic(&got), Ok(()));
    }

    fn int(n: i64) -> Value {
        Value::Integer(n.into())
    }

    fn text(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    #[test]
    fn unsigned_integers_at_every_head_boundary() {
        encodes(&0u64, "00");
        encodes(&23u64, "17");
        encodes(&24u64, "1818");
        encodes(&255u64, "18ff");
        encodes(&256u64, "190100");
        encodes(&65535u64, "19ffff");
        encodes(&65536u64, "1a00010000");
        encodes(&4294967295u64, "1affffffff");
        encodes(&4294967296u64, "1b0000000100000000");
        encodes(&u64::MAX, "1bffffffffffffffff");
        encodes(&1000000u64, "1a000f4240");
    }

    #[test]
    fn negative_integers_at_every_head_boundary() {
        encodes(&-1i64, "20");
        encodes(&-10i64, "29");
        encodes(&-24i64, "37");
        encodes(&-25i64, "3818");
        encodes(&-256i64, "38ff");
        encodes(&-257i64, "390100");
        encodes(&-65536i64, "39ffff");
        encodes(&-65537i64, "3a00010000");
        encodes(&-4294967296i64, "3affffffff");
        encodes(&-4294967297i64, "3b0000000100000000");
        encodes(&i64::MIN, "3b7fffffffffffffff");
        encodes(&-100i64, "3863");
        encodes(&-1000i64, "3903e7");
    }

    #[test]
    fn integers_decode_back() {
        assert_eq!(
            from_slice::<u64>(&hex("1bffffffffffffffff")).unwrap(),
            u64::MAX
        );
        assert_eq!(
            from_slice::<i64>(&hex("3b7fffffffffffffff")).unwrap(),
            i64::MIN
        );
        assert_eq!(from_slice::<i64>(&hex("3903e7")).unwrap(), -1000);
    }

    #[test]
    fn strings_and_byte_strings() {
        encodes(&"", "60");
        encodes(&"a", "6161");
        encodes(&"IETF", "6449455446");
        encodes(&"\"\\", "62225c");
        encodes(&"\u{00fc}", "62c3bc");
        encodes(&"\u{6c34}", "63e6b0b4");
        encodes(&"\u{10151}", "64f0908591");
        encodes(&serde_bytes::ByteBuf::from(Vec::<u8>::new()), "40");
        encodes(
            &serde_bytes::ByteBuf::from(vec![1u8, 2, 3, 4]),
            "4401020304",
        );
        // A 24 byte string needs the one byte length form.
        let long = "a".repeat(24);
        let mut expected = vec![0x78, 0x18];
        expected.extend_from_slice(long.as_bytes());
        assert_eq!(to_vec(&long).unwrap(), expected);
        assert_eq!(from_slice::<String>(&expected).unwrap(), long);
        assert_eq!(
            from_slice::<String>(&hex("64f0908591")).unwrap(),
            "\u{10151}"
        );
    }

    #[test]
    fn arrays() {
        encodes(&Vec::<u8>::new(), "80");
        encodes(&vec![1u8, 2, 3], "83010203");
        let nested = Value::Array(vec![
            int(1),
            Value::Array(vec![int(2), int(3)]),
            Value::Array(vec![int(4), int(5)]),
        ]);
        encodes(&nested, "8301820203820405");
        let twenty_five: Vec<u8> = (1..=25).collect();
        encodes(
            &twenty_five,
            "98190102030405060708090a0b0c0d0e0f101112131415161718181819",
        );
        assert_eq!(
            from_slice::<Vec<u8>>(&hex(
                "98190102030405060708090a0b0c0d0e0f101112131415161718181819"
            ))
            .unwrap(),
            twenty_five
        );
    }

    #[test]
    fn maps() {
        encodes(&Value::Map(Vec::new()), "a0");
        encodes(
            &Value::Map(vec![(int(1), int(2)), (int(3), int(4))]),
            "a201020304",
        );
        // Entries are sorted whatever the insertion order.
        encodes(
            &Value::Map(vec![(int(3), int(4)), (int(1), int(2))]),
            "a201020304",
        );
        let ab = Value::Map(vec![
            (text("a"), int(1)),
            (text("b"), Value::Array(vec![int(2), int(3)])),
        ]);
        encodes(&ab, "a26161016162820203");
        let ba = Value::Map(vec![
            (text("b"), Value::Array(vec![int(2), int(3)])),
            (text("a"), int(1)),
        ]);
        encodes(&ba, "a26161016162820203");
        let decoded: HashMap<String, Value> = from_slice(&hex("a26161016162820203")).unwrap();
        assert_eq!(decoded.len(), 2);
    }

    #[test]
    fn floats_in_shortest_form() {
        encodes(&0.0f64, "f90000");
        encodes(&-0.0f64, "f98000");
        encodes(&1.0f64, "f93c00");
        encodes(&1.5f64, "f93e00");
        encodes(&65504.0f64, "f97bff");
        encodes(&100000.0f64, "fa47c35000");
        encodes(&100000.0f32, "fa47c35000");
        encodes(&3.4028234663852886e38f64, "fa7f7fffff");
        encodes(&1.1f64, "fb3ff199999999999a");
        encodes(&1.0e300f64, "fb7e37e43c8800759c");
        encodes(&5.960464477539063e-8f64, "f90001");
        encodes(&0.00006103515625f64, "f90400");
        encodes(&-4.0f64, "f9c400");
        encodes(&-4.1f64, "fbc010666666666666");
        encodes(&f64::INFINITY, "f97c00");
        encodes(&f64::NEG_INFINITY, "f9fc00");
        encodes(&f64::NAN, "f97e00");
        encodes(&f64::from_bits(0x7ff8_0000_0000_0001), "f97e00");
        encodes(&f32::NAN, "f97e00");
    }

    #[test]
    fn floats_decode_back() {
        assert_eq!(from_slice::<f64>(&hex("f93e00")).unwrap(), 1.5);
        assert_eq!(from_slice::<f64>(&hex("fa47c35000")).unwrap(), 100000.0);
        assert_eq!(from_slice::<f64>(&hex("fb3ff199999999999a")).unwrap(), 1.1);
        assert_eq!(from_slice::<f64>(&hex("f97c00")).unwrap(), f64::INFINITY);
        assert!(from_slice::<f64>(&hex("f97e00")).unwrap().is_nan());
    }

    #[test]
    fn simple_values() {
        encodes(&false, "f4");
        encodes(&true, "f5");
        encodes(&(), "f6");
        encodes(&Option::<u8>::None, "f6");
        encodes(&Some(5u8), "05");
        assert!(from_slice::<bool>(&hex("f5")).unwrap());
        assert_eq!(from_slice::<Option<u8>>(&hex("f6")).unwrap(), None);
    }

    #[test]
    fn accepted_scanner_vectors() {
        accepts("00");
        accepts("1bffffffffffffffff");
        accepts("3b7fffffffffffffff");
        accepts("40");
        accepts("6449455446");
        accepts("a0");
        // "b" before "aa": shorter encoded key first.
        accepts("a2 6162 01 626161 02");
        accepts("f97e00");
        accepts("f9fc00");
        accepts("fa47c35000");
        accepts("fb3ff199999999999a");
    }

    #[test]
    fn rejects_indefinite_lengths() {
        rejects("5fff", CborError::IndefiniteLength);
        rejects("7fff", CborError::IndefiniteLength);
        rejects("9fff", CborError::IndefiniteLength);
        rejects("bfff", CborError::IndefiniteLength);
        rejects("9f01ff", CborError::IndefiniteLength);
        rejects("5f4101ff", CborError::IndefiniteLength);
    }

    #[test]
    fn rejects_break_and_reserved_heads() {
        rejects("ff", CborError::UnexpectedBreak);
        rejects("1c", CborError::ReservedInfo);
        rejects("1f", CborError::ReservedInfo);
        rejects("3e", CborError::ReservedInfo);
        rejects("fc", CborError::ReservedInfo);
    }

    #[test]
    fn rejects_non_shortest_integers_and_lengths() {
        rejects("1817", CborError::NonShortestInteger);
        rejects("1800", CborError::NonShortestInteger);
        rejects("1900ff", CborError::NonShortestInteger);
        rejects("1a0000ffff", CborError::NonShortestInteger);
        rejects("1b00000000ffffffff", CborError::NonShortestInteger);
        rejects("3817", CborError::NonShortestInteger);
        // String length 1 written with a one byte length.
        rejects("780161", CborError::NonShortestInteger);
        rejects("580141", CborError::NonShortestInteger);
        // Array and map lengths.
        rejects("980100", CborError::NonShortestInteger);
        rejects("b800", CborError::NonShortestInteger);
    }

    #[test]
    fn rejects_non_shortest_floats_and_non_canonical_nan() {
        // 1.0 as binary64 and as binary32.
        rejects("fb3ff0000000000000", CborError::NonShortestFloat);
        rejects("fa3f800000", CborError::NonShortestFloat);
        // Infinity as binary32 and binary64.
        rejects("fa7f800000", CborError::NonShortestFloat);
        rejects("fb7ff0000000000000", CborError::NonShortestFloat);
        // 1.5 as binary32.
        rejects("fa3fc00000", CborError::NonShortestFloat);
        // 0.0 and -0.0 as binary32 and binary64.
        rejects("fa00000000", CborError::NonShortestFloat);
        rejects("fb8000000000000000", CborError::NonShortestFloat);
        // A binary64 that fits binary32 but not binary16 (100000.0).
        rejects("fb40f86a0000000000", CborError::NonShortestFloat);
        // NaN forms.
        rejects("f97e01", CborError::NonCanonicalNan);
        rejects("f9fe00", CborError::NonCanonicalNan);
        rejects("f97c01", CborError::NonCanonicalNan);
        rejects("fa7fc00000", CborError::NonCanonicalNan);
        rejects("fb7ff8000000000000", CborError::NonCanonicalNan);
    }

    #[test]
    fn rejects_unsorted_and_duplicate_keys() {
        rejects("a2616202616101", CborError::UnsortedOrDuplicateKey);
        rejects("a2616101616102", CborError::UnsortedOrDuplicateKey);
        // "aa" (62 61 61) sorts after "b" (61 62).
        rejects("a2626161 01 616202", CborError::UnsortedOrDuplicateKey);
        // Integer keys.
        rejects("a2 03 04 01 02", CborError::UnsortedOrDuplicateKey);
    }

    #[test]
    fn rejects_tags_and_unsupported_simples() {
        rejects("c100", CborError::Tag);
        rejects("c11a514b67b0", CborError::Tag);
        rejects(
            "d82076687474703a2f2f7777772e6578616d706c652e636f6d",
            CborError::Tag,
        );
        rejects("f7", CborError::UnsupportedSimple);
        rejects("f820", CborError::UnsupportedSimple);
        rejects("f8ff", CborError::UnsupportedSimple);
        rejects("f0", CborError::UnsupportedSimple);
        rejects("e0", CborError::UnsupportedSimple);
        rejects("f8", CborError::Truncated);
    }

    #[test]
    fn rejects_invalid_utf8() {
        rejects("62c328", CborError::InvalidUtf8);
        rejects("61ff", CborError::InvalidUtf8);
        // A UTF-16 surrogate encoded as UTF-8.
        rejects("63eda080", CborError::InvalidUtf8);
        // Byte strings may hold anything.
        accepts("42c328");
    }

    #[test]
    fn rejects_trailing_bytes_and_truncation() {
        rejects("0000", CborError::TrailingBytes);
        rejects("80 00", CborError::TrailingBytes);
        rejects("f4f4", CborError::TrailingBytes);
        rejects("", CborError::Truncated);
        rejects("18", CborError::Truncated);
        rejects("19 01", CborError::Truncated);
        rejects("1a 00 01 00", CborError::Truncated);
        rejects("1b 00 00 00 01 00 00 00", CborError::Truncated);
        rejects("fa 47 c3", CborError::Truncated);
        rejects("63 61 61", CborError::Truncated);
        rejects("43 01", CborError::Truncated);
        rejects("82 01", CborError::Truncated);
        rejects("a1 61 61", CborError::Truncated);
    }

    #[test]
    fn rejects_lengths_beyond_the_input_without_allocating() {
        // The scanner never allocates; these must simply fail.
        rejects("9bffffffffffffffff", CborError::Truncated);
        rejects("5bffffffffffffffff", CborError::Truncated);
        rejects("7bffffffffffffffff", CborError::Truncated);
        rejects("bbffffffffffffffff", CborError::Truncated);
        rejects("9b7fffffffffffffff", CborError::Truncated);
        rejects("9affffffff", CborError::Truncated);
        rejects("5a7fffffff00", CborError::Truncated);
        // An array that declares more elements than there are bytes left.
        rejects("9a00010000 00", CborError::Truncated);
        // A map that declares more entries than there are byte pairs left.
        rejects("a3 01 02 03 04", CborError::Truncated);
    }

    #[test]
    fn depth_limit_is_32_containers() {
        let mut ok = vec![0x81u8; MAX_DEPTH - 1];
        ok.push(0x80);
        assert_eq!(check_deterministic(&ok), Ok(()));

        let mut too_deep = vec![0x81u8; MAX_DEPTH];
        too_deep.push(0x80);
        assert_eq!(
            check_deterministic(&too_deep),
            Err(CborError::DepthExceeded)
        );

        // A scalar inside 32 arrays is also accepted.
        let mut leaf = vec![0x81u8; MAX_DEPTH];
        leaf.push(0x00);
        assert_eq!(check_deterministic(&leaf), Ok(()));

        // Maps count too: {0: {0: ... }} 33 deep.
        let mut maps = Vec::new();
        for _ in 0..(MAX_DEPTH + 1) {
            maps.push(0xa1);
            maps.push(0x00);
        }
        maps.push(0x00);
        assert_eq!(check_deterministic(&maps), Err(CborError::DepthExceeded));

        // Very deep input stops at the limit instead of recursing.
        let deep = vec![0x81u8; 100_000];
        assert_eq!(check_deterministic(&deep), Err(CborError::DepthExceeded));
    }

    #[test]
    fn encoder_enforces_depth_tags_and_duplicates() {
        let mut v = Value::Array(Vec::new());
        for _ in 0..(MAX_DEPTH - 1) {
            v = Value::Array(vec![v]);
        }
        assert!(to_vec(&v).is_ok());
        let deeper = Value::Array(vec![v]);
        assert_eq!(to_vec(&deeper), Err(CborError::DepthExceeded));

        let tagged = Value::Tag(1, Box::new(int(1)));
        assert_eq!(to_vec(&tagged), Err(CborError::Tag));

        let dup = Value::Map(vec![(text("a"), int(1)), (text("a"), int(2))]);
        assert_eq!(to_vec(&dup), Err(CborError::UnsortedOrDuplicateKey));
    }

    #[test]
    fn from_slice_rejects_before_decoding() {
        assert_eq!(
            from_slice::<u64>(&hex("1817")),
            Err(CborError::NonShortestInteger)
        );
        assert_eq!(
            from_slice::<Vec<u8>>(&hex("9f01ff")),
            Err(CborError::IndefiniteLength)
        );
        assert_eq!(
            from_slice::<u8>(&hex("0100")),
            Err(CborError::TrailingBytes)
        );
        // Well formed but the wrong type is a decode error, not a scan error.
        match from_slice::<String>(&hex("01")) {
            Err(CborError::Decode(_)) => {}
            other => panic!("expected a decode error, got {other:?}"),
        }
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    enum Kind {
        Unit,
        Newtype(u32),
        Struct { x: i32, y: i32 },
        Tuple(u8, u8),
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Sample {
        zeta: u32,
        alpha: String,
        table: HashMap<String, u32>,
        kind: Kind,
        maybe: Option<u32>,
        mid: Vec<i64>,
        blob: serde_bytes::ByteBuf,
        ratio: f64,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Table {
        table: HashMap<String, u32>,
    }

    fn sample(order: &[(&str, u32)]) -> Sample {
        let mut table = HashMap::new();
        for (k, v) in order {
            table.insert((*k).to_string(), *v);
        }
        Sample {
            zeta: 9,
            alpha: "first".to_string(),
            table,
            kind: Kind::Struct { x: 1, y: -1 },
            maybe: Some(7),
            mid: vec![-1, 0, 1, 70000],
            blob: serde_bytes::ByteBuf::from(vec![0xde, 0xad]),
            ratio: 0.1,
        }
    }

    #[test]
    fn enum_shapes_are_serde_external_tagging() {
        encodes(&Kind::Unit, "64 556e6974");
        encodes(&Kind::Newtype(5), "a1 67 4e657774797065 05");
        encodes(
            &Kind::Struct { x: 1, y: -1 },
            "a1 66 537472756374 a2 6178 01 6179 20",
        );
        encodes(&Kind::Tuple(1, 2), "a1 65 5475706c65 82 01 02");
        for kind in [
            Kind::Unit,
            Kind::Newtype(5),
            Kind::Struct { x: 1, y: -1 },
            Kind::Tuple(1, 2),
        ] {
            let bytes = to_vec(&kind).unwrap();
            assert_eq!(from_slice::<Kind>(&bytes).unwrap(), kind);
        }
    }

    #[test]
    fn hash_map_keys_come_out_in_bytewise_order() {
        // Encoded key order: "a", "b", "aa", "bb", "ccc".
        let expected = hex("a1 65 7461626c65 a5 6161 01 6162 02 626161 03 626262 04 63636363 05");
        let orders: [[(&str, u32); 5]; 3] = [
            [("a", 1), ("b", 2), ("aa", 3), ("bb", 4), ("ccc", 5)],
            [("ccc", 5), ("bb", 4), ("aa", 3), ("b", 2), ("a", 1)],
            [("aa", 3), ("ccc", 5), ("a", 1), ("bb", 4), ("b", 2)],
        ];
        for order in orders.iter() {
            let mut table = HashMap::new();
            for (k, v) in order.iter() {
                table.insert((*k).to_string(), *v);
            }
            let bytes = to_vec(&Table { table }).unwrap();
            assert_eq!(bytes, expected);
        }
        let back: Table = from_slice(&expected).unwrap();
        assert_eq!(back.table.len(), 5);
        assert_eq!(back.table["bb"], 4);
    }

    #[test]
    fn struct_round_trip_is_independent_of_field_and_insertion_order() {
        let a = sample(&[("a", 1), ("b", 2), ("aa", 3), ("bb", 4), ("ccc", 5)]);
        let b = sample(&[("ccc", 5), ("aa", 3), ("b", 2), ("bb", 4), ("a", 1)]);
        let bytes_a = to_vec(&a).unwrap();
        let bytes_b = to_vec(&b).unwrap();
        assert_eq!(bytes_a, bytes_b);
        assert_eq!(check_deterministic(&bytes_a), Ok(()));

        let decoded: Sample = from_slice(&bytes_a).unwrap();
        assert_eq!(decoded, a);
        // Decode then encode gives the identical bytes.
        assert_eq!(to_vec(&decoded).unwrap(), bytes_a);

        // Top level keys are in encoded order: zeta (64..), alpha, table, kind
        // (64..), ratio, blob... check by decoding the map and comparing the
        // encoded key sequence with its sorted self.
        let tree: Value = from_slice(&bytes_a).unwrap();
        let entries = tree.into_map().unwrap();
        let keys: Vec<Vec<u8>> = entries.iter().map(|(k, _)| to_vec(k).unwrap()).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert_eq!(keys.len(), 8);
    }

    #[test]
    fn c12_example_1_hello_body() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Hello {
            extensions: Vec<String>,
            versions: Vec<u32>,
            max_frame: u32,
        }
        let frame = hex(
            "00 00 00 32 01 01 00 00 00 00 00 00 00 00 00 a3 68 76 65 72 73 69 6f 6e 73 81 01 \
             69 6d 61 78 5f 66 72 61 6d 65 1a 01 00 00 00 6a 65 78 74 65 6e 73 69 6f 6e 73 80",
        );
        assert_eq!(frame.len(), 54);
        let body = &frame[15..];
        assert_eq!(body.len(), 39);
        assert_eq!(check_deterministic(body), Ok(()));
        let hello: Hello = from_slice(body).unwrap();
        assert_eq!(
            hello,
            Hello {
                extensions: Vec::new(),
                versions: vec![1],
                max_frame: 16_777_216,
            }
        );
        assert_eq!(to_vec(&hello).unwrap(), body);
    }

    #[test]
    fn c12_example_2_request_body() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Args {
            text: String,
        }
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        enum Command {
            #[serde(rename = "tool_call")]
            ToolCall { name: String, args: Args },
        }
        let frame = hex(
            "00 00 00 3d 01 10 00 00 00 00 00 00 00 00 01 a1 69 74 6f 6f 6c 5f 63 61 6c 6c a2 \
             64 61 72 67 73 a1 64 74 65 78 74 62 68 69 64 6e 61 6d 65 72 73 70 69 6e 65 2e 63 \
             6f 6e 74 65 6e 74 2f 68 61 73 68",
        );
        assert_eq!(frame.len(), 65);
        let body = &frame[15..];
        assert_eq!(body.len(), 50);
        assert_eq!(check_deterministic(body), Ok(()));
        let command: Command = from_slice(body).unwrap();
        assert_eq!(
            command,
            Command::ToolCall {
                name: "spine.content/hash".to_string(),
                args: Args {
                    text: "hi".to_string()
                },
            }
        );
        assert_eq!(to_vec(&command).unwrap(), body);
    }

    #[test]
    fn c12_ping_body() {
        // `18 2a` is the integer 42, the body of a PING with token 42.
        encodes(&42u64, "182a");
        assert_eq!(from_slice::<u64>(&hex("182a")).unwrap(), 42);
    }

    #[test]
    fn to_vec_output_always_passes_the_scanner() {
        let values: Vec<Value> = vec![
            int(0),
            int(-1),
            Value::Float(0.1),
            Value::Float(f64::NAN),
            Value::Text("x".repeat(300)),
            Value::Bytes(vec![0u8; 70000]),
            Value::Array((0..300).map(int).collect()),
            Value::Map(
                (0..300)
                    .rev()
                    .map(|i| (Value::Text(format!("k{i}")), int(i)))
                    .collect(),
            ),
        ];
        for v in &values {
            let bytes = to_vec(v).unwrap();
            assert_eq!(check_deterministic(&bytes), Ok(()));
        }
    }
}
