//! SPINE binary wire codec.
//!
//! The SPINE message *body* is serialized with a compact, self-describing
//! binary format (CBOR, [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949))
//! instead of UTF-8 JSON. CBOR is the right choice for SPINE because it is the
//! only serde-compatible binary format that round-trips
//! [`serde_json::Value`](serde_json::Value) without restructuring every message
//! field — `Value` deserializes via `deserialize_any`, which the non-self-
//! describing formats (bincode 1.x, postcard) reject. CBOR encodes integers,
//! floats, and byte strings in their native binary widths and drops JSON's
//! quotes, key repetition, and decimal-string float blowup, so a typical
//! agent frame (tool calls, stream tokens, encoded latents, capability ads)
//! lands well under half its JSON size before any compression.
//!
//! # Frame layout
//!
//! Every encoded body is prefixed with an 8-byte [`SpineWireHeader`]:
//!
//! ```text
//! ┌────────┬────────┬─────────┬────────┬───────────────────────┐
//! │ 'S'    │ 'P'    │ version │ format │ payload_len (u32 BE)  │
//! │ 1 byte │ 1 byte │ 1 byte  │ 1 byte │ 4 bytes               │
//! └────────┴────────┴─────────┴────────┴───────────────────────┘
//! ```
//!
//! The `format` byte lets a peer auto-detect the payload codec without
//! out-of-band negotiation:
//!
//! | code   | meaning                                            |
//! |--------|----------------------------------------------------|
//! | `0x01` | JSON (legacy / debug)                              |
//! | `0x02` | CBOR                                               |
//! | `0x03` | CBOR + zstd (payload `>= ZSTD_THRESHOLD` bytes)    |
//!
//! [`encode`] picks CBOR for small bodies and CBOR+zstd once the CBOR payload
//! crosses [`ZSTD_THRESHOLD`], so large token streams and JSON-shaped tool
//! arguments compress without the caller choosing. [`decode`] dispatches on the
//! header's `format` byte.
//!
//! # Backward compatibility
//!
//! A SPINE v1.3.x peer framed bodies as raw `serde_json` with no
//! [`SpineWireHeader`]. [`decode`] detects this: if the first two bytes are not
//! the `SP` magic, it falls back to parsing the whole buffer as JSON. This lets
//! a v1.4.0 node read v1.3.x bodies. The reverse (old node reading new CBOR) is
//! out of scope — interoperability with older peers is a later concern.

use crate::Message;

/// Wire magic: ASCII `"SP"` (SPine). First two bytes of every framed body.
pub const WIRE_MAGIC: [u8; 2] = *b"SP";

/// Current wire-format version.
pub const WIRE_VERSION: u8 = 1;

/// Size of [`SpineWireHeader`] in bytes.
pub const HEADER_LEN: usize = 8;

/// Payload codec: JSON text (legacy / debugging).
pub const FORMAT_JSON: u8 = 0x01;
/// Payload codec: CBOR (RFC 8949).
pub const FORMAT_CBOR: u8 = 0x02;
/// Payload codec: CBOR compressed with zstd.
pub const FORMAT_CBOR_ZSTD: u8 = 0x03;

/// CBOR payloads at least this large are zstd-compressed by [`encode`]. Below
/// this, zstd's ~13-byte frame overhead tends to outweigh the gain (and
/// [`encode`] keeps the compressed form only when it is actually smaller, so a
/// payload that doesn't compress falls back to plain CBOR regardless).
pub const ZSTD_THRESHOLD: usize = 128;

/// zstd compression level used for [`FORMAT_CBOR_ZSTD`] bodies. Level 3 is the
/// zstd default — a good size/CPU tradeoff for hot-path agent traffic.
pub const ZSTD_LEVEL: i32 = 3;

/// Errors produced while encoding or decoding a SPINE wire frame.
#[derive(Debug, thiserror::Error)]
pub enum WireError {
    /// The buffer was shorter than an 8-byte [`SpineWireHeader`].
    #[error("wire frame too short: {0} bytes (need >= {HEADER_LEN})")]
    TooShort(usize),
    /// The `format` byte was not one of the known codec codes.
    #[error("unknown wire format byte: {0:#04x}")]
    UnknownFormat(u8),
    /// The header's `payload_len` did not match the bytes that followed it.
    #[error("wire length mismatch: header says {expected}, have {actual}")]
    LengthMismatch {
        /// Length declared by the header.
        expected: usize,
        /// Bytes actually present after the header.
        actual: usize,
    },
    /// CBOR serialization failed.
    #[error("cbor encode: {0}")]
    CborEncode(String),
    /// CBOR deserialization failed.
    #[error("cbor decode: {0}")]
    CborDecode(String),
    /// JSON (de)serialization failed.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// zstd (de)compression failed.
    #[error("zstd: {0}")]
    Zstd(#[from] std::io::Error),
}

/// 8-byte fixed header prefixing every encoded SPINE body.
///
/// See the [module docs](self) for the on-the-wire layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpineWireHeader {
    /// Magic bytes; always [`WIRE_MAGIC`] (`"SP"`).
    pub magic: [u8; 2],
    /// Wire-format version; [`WIRE_VERSION`] for frames this build writes.
    pub version: u8,
    /// Payload codec (`FORMAT_*`).
    pub format: u8,
    /// Length of the payload that follows this header, in bytes.
    pub payload_len: u32,
}

impl SpineWireHeader {
    /// Build a header for a `format` payload of `payload_len` bytes.
    pub fn new(format: u8, payload_len: u32) -> Self {
        Self {
            magic: WIRE_MAGIC,
            version: WIRE_VERSION,
            format,
            payload_len,
        }
    }

    /// Serialize the header to its 8 wire bytes (`payload_len` big-endian).
    pub fn to_bytes(self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0] = self.magic[0];
        out[1] = self.magic[1];
        out[2] = self.version;
        out[3] = self.format;
        out[4..8].copy_from_slice(&self.payload_len.to_be_bytes());
        out
    }

    /// Parse an 8-byte header. Returns `None` if `buf` is too short or the
    /// magic does not match [`WIRE_MAGIC`] (e.g. a legacy headerless body).
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < HEADER_LEN {
            return None;
        }
        if buf[0] != WIRE_MAGIC[0] || buf[1] != WIRE_MAGIC[1] {
            return None;
        }
        Some(Self {
            magic: WIRE_MAGIC,
            version: buf[2],
            format: buf[3],
            payload_len: u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
        })
    }
}

/// Serialize a [`Message`] into a SPINE wire frame: an 8-byte
/// [`SpineWireHeader`] followed by the CBOR (or CBOR+zstd) payload.
///
/// The codec is chosen automatically: CBOR for small bodies, CBOR+zstd once the
/// CBOR payload reaches [`ZSTD_THRESHOLD`].
pub fn encode(msg: &Message) -> Result<Vec<u8>, WireError> {
    let mut cbor = Vec::new();
    ciborium::into_writer(msg, &mut cbor).map_err(|e| WireError::CborEncode(e.to_string()))?;

    let (format, payload) = if cbor.len() >= ZSTD_THRESHOLD {
        let compressed = zstd::stream::encode_all(&cbor[..], ZSTD_LEVEL)?;
        // Only keep the compressed form if it actually shrank the payload.
        if compressed.len() < cbor.len() {
            (FORMAT_CBOR_ZSTD, compressed)
        } else {
            (FORMAT_CBOR, cbor)
        }
    } else {
        (FORMAT_CBOR, cbor)
    };

    Ok(frame(format, &payload))
}

/// Serialize a [`Message`] as a JSON wire frame (`FORMAT_JSON`).
///
/// Provided for debugging and explicit legacy framing; [`encode`] is the
/// production path. Output is still wrapped in a [`SpineWireHeader`].
pub fn encode_json(msg: &Message) -> Result<Vec<u8>, WireError> {
    let payload = serde_json::to_vec(msg)?;
    Ok(frame(FORMAT_JSON, &payload))
}

/// Glue an 8-byte header onto a payload.
fn frame(format: u8, payload: &[u8]) -> Vec<u8> {
    let header = SpineWireHeader::new(format, payload.len() as u32);
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&header.to_bytes());
    out.extend_from_slice(payload);
    out
}

/// Deserialize a SPINE wire frame produced by [`encode`] / [`encode_json`].
///
/// Auto-detects the codec from the [`SpineWireHeader`]'s `format` byte. If the
/// buffer carries no SPINE magic, it is treated as a legacy headerless JSON
/// body and parsed directly — see the [module docs](self) on compatibility.
pub fn decode(buf: &[u8]) -> Result<Message, WireError> {
    let Some(header) = SpineWireHeader::from_bytes(buf) else {
        // Legacy v1.3.x body: raw serde_json, no SPINE header.
        return Ok(serde_json::from_slice(buf)?);
    };

    let payload = &buf[HEADER_LEN..];
    if payload.len() != header.payload_len as usize {
        return Err(WireError::LengthMismatch {
            expected: header.payload_len as usize,
            actual: payload.len(),
        });
    }

    match header.format {
        FORMAT_JSON => Ok(serde_json::from_slice(payload)?),
        FORMAT_CBOR => {
            ciborium::from_reader(payload).map_err(|e| WireError::CborDecode(e.to_string()))
        }
        FORMAT_CBOR_ZSTD => {
            let cbor = zstd::stream::decode_all(payload)?;
            ciborium::from_reader(&cbor[..]).map_err(|e| WireError::CborDecode(e.to_string()))
        }
        other => Err(WireError::UnknownFormat(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    #[test]
    fn header_roundtrips() {
        let h = SpineWireHeader::new(FORMAT_CBOR, 0x0001_0203);
        let bytes = h.to_bytes();
        assert_eq!(&bytes[0..2], b"SP");
        assert_eq!(bytes[2], WIRE_VERSION);
        assert_eq!(bytes[3], FORMAT_CBOR);
        // payload_len is big-endian.
        assert_eq!(&bytes[4..8], &[0x00, 0x01, 0x02, 0x03]);
        assert_eq!(SpineWireHeader::from_bytes(&bytes), Some(h));
    }

    #[test]
    fn from_bytes_rejects_non_spine() {
        assert!(SpineWireHeader::from_bytes(b"{\"x\":1}").is_none());
        assert!(SpineWireHeader::from_bytes(b"S").is_none());
    }

    #[test]
    fn ping_roundtrips_through_cbor() {
        let msg = Message::Ping { timestamp: 42 };
        let wire = encode(&msg).unwrap();
        assert_eq!(&wire[0..2], b"SP");
        assert_eq!(wire[3], FORMAT_CBOR);
        match decode(&wire).unwrap() {
            Message::Ping { timestamp } => assert_eq!(timestamp, 42),
            other => panic!("expected Ping, got {other:?}"),
        }
    }

    #[test]
    fn legacy_json_body_still_decodes() {
        // A v1.3.x peer would send a bare serde_json body with no SP header.
        let legacy = serde_json::to_vec(&Message::Ping { timestamp: 7 }).unwrap();
        match decode(&legacy).unwrap() {
            Message::Ping { timestamp } => assert_eq!(timestamp, 7),
            other => panic!("expected Ping from legacy body, got {other:?}"),
        }
    }
}
