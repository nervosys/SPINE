//! RFC 4648 base32 (lowercase, unpadded) — the on-the-wire spelling of every
//! self-certifying SPINE authority.
//!
//! Why base32 and not base64 or hex: an authority is typed by humans into
//! configs, logged, and pasted between agents. Base32 is case-insensitive (so
//! normalization is a `to_ascii_lowercase`, never a semantic decision), contains
//! no characters that need percent-encoding in a URI, and is 20% denser than
//! hex. A 32-byte key encodes to exactly 52 characters with no padding.

/// RFC 4648 alphabet, lowercased.
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Encode bytes as lowercase unpadded base32.
pub fn encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(5) * 8);
    for chunk in input.chunks(5) {
        // Pack up to 5 bytes (40 bits) big-endian into a u64.
        let mut buf = [0u8; 5];
        buf[..chunk.len()].copy_from_slice(chunk);
        let n = u64::from(buf[0]) << 32
            | u64::from(buf[1]) << 24
            | u64::from(buf[2]) << 16
            | u64::from(buf[3]) << 8
            | u64::from(buf[4]);

        // 5 input bytes yield 8 output symbols; a short chunk yields only the
        // symbols its bits actually cover (unpadded).
        let symbols = match chunk.len() {
            1 => 2,
            2 => 4,
            3 => 5,
            4 => 7,
            _ => 8,
        };
        for i in 0..symbols {
            let shift = 35 - i * 5;
            out.push(ALPHABET[((n >> shift) & 0x1f) as usize] as char);
        }
    }
    out
}

/// Decode lowercase-or-uppercase unpadded base32. Returns `None` on any symbol
/// outside the alphabet or on a length that no byte string could produce.
pub fn decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 5 / 8);
    for chunk in input.as_bytes().chunks(8) {
        let mut n: u64 = 0;
        for (i, &c) in chunk.iter().enumerate() {
            let v = symbol_value(c)?;
            n |= u64::from(v) << (35 - i * 5);
        }
        // Inverse of the encode table: a chunk of 1, 3, or 6 symbols cannot be
        // produced by any input and is rejected rather than silently truncated.
        let bytes = match chunk.len() {
            2 => 1,
            4 => 2,
            5 => 3,
            7 => 4,
            8 => 5,
            _ => return None,
        };
        for i in 0..bytes {
            out.push(((n >> (32 - i * 8)) & 0xff) as u8);
        }
    }
    Some(out)
}

/// Decode into a fixed 32-byte array — the shape every SPINE key uses.
pub fn decode_key(input: &str) -> Option<[u8; 32]> {
    let bytes = decode(input)?;
    if bytes.len() != 32 {
        return None;
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Some(key)
}

fn symbol_value(c: u8) -> Option<u8> {
    match c {
        b'a'..=b'z' => Some(c - b'a'),
        b'A'..=b'Z' => Some(c - b'A'),
        b'2'..=b'7' => Some(c - b'2' + 26),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_all_lengths_up_to_two_chunks() {
        for len in 0..=16usize {
            let input: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(37)).collect();
            let encoded = encode(&input);
            assert_eq!(
                decode(&encoded).as_deref(),
                Some(input.as_slice()),
                "len {len}"
            );
        }
    }

    #[test]
    fn key_is_52_symbols_and_roundtrips() {
        let key = [0xABu8; 32];
        let encoded = encode(&key);
        assert_eq!(encoded.len(), 52);
        assert_eq!(decode_key(&encoded), Some(key));
    }

    #[test]
    fn decoding_is_case_insensitive() {
        let key = [7u8; 32];
        let lower = encode(&key);
        assert_eq!(decode_key(&lower.to_uppercase()), Some(key));
    }

    #[test]
    fn rejects_symbols_outside_alphabet() {
        // '1', '0', '8', '9' are deliberately absent from RFC 4648 base32.
        assert_eq!(decode("ab1c"), None);
        assert_eq!(decode("ab0c"), None);
        assert_eq!(decode("ab8c"), None);
        assert_eq!(decode("-bcd"), None);
    }

    #[test]
    fn rejects_impossible_chunk_lengths() {
        assert_eq!(decode("a"), None);
        assert_eq!(decode("abc"), None);
        assert_eq!(decode("abcdef"), None);
    }

    #[test]
    fn decode_key_rejects_wrong_length() {
        assert_eq!(decode_key(&encode(&[1u8; 16])), None);
        assert_eq!(decode_key(&encode(&[1u8; 33])), None);
    }

    #[test]
    fn matches_rfc4648_vectors() {
        // RFC 4648 §10 test vectors, lowercased and unpadded.
        assert_eq!(encode(b"f"), "my");
        assert_eq!(encode(b"fo"), "mzxq");
        assert_eq!(encode(b"foo"), "mzxw6");
        assert_eq!(encode(b"foob"), "mzxw6yq");
        assert_eq!(encode(b"fooba"), "mzxw6ytb");
        assert_eq!(encode(b"foobar"), "mzxw6ytboi");
    }
}
