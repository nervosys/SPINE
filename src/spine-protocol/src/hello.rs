//! spine-wire v1 first frames: `HELLO`, `HELLO_ACK` and `CLOSE`
//! (`docs/spec/00-wire.md`, sections C.5 to C.7 and C.10).
//!
//! This module holds the three body types, the validation of every bound the
//! specification states, the server-side [`negotiate`] function and the
//! conversions to and from [`Frame`]. It owns no connection state: the caller
//! decides when a frame may be sent and what to do with a returned [`Close`].
//!
//! Errors are reported as a [`Close`], the body the caller should send in a
//! `CLOSE` frame before closing. The code is chosen as follows:
//!
//! - a body that is not well-formed, deterministic CBOR (C.5 rules 1 to 6)
//!   is `malformed_frame`;
//! - a body that is deterministic CBOR but does not fit its type, or breaks a
//!   stated bound, is `protocol_violation`;
//! - a frame of the wrong type is `protocol_violation`;
//! - no common version is `unsupported_version`, carrying `supported`.
//!
//! `HELLO` and `HELLO_ACK` are open: unknown keys are ignored (C.5 rule 9).
//! `CLOSE` is closed: an unknown key is a schema error (C.5 rule 8).
//!
//! `HELLO_ACK` carries three keys every peer must send and require
//! (`docs/spec/00-wire.md` 6.3): `binding`, a per-connection random value the
//! server chooses (6.6), `authority`, the node's [`AuthorityMode`], and
//! `time`, the server's clock.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::cbor::{self, CborError};
use crate::frame::{CloseCode, Frame, FrameType, MAX_FRAME};
use crate::wirefmt::reject_null;

/// Smallest `max_frame` a `HELLO` or `HELLO_ACK` may carry (C.6).
pub const MIN_MAX_FRAME: u32 = 1024;

/// Largest `agent` text in a `HELLO`, in bytes (C.6).
pub const MAX_AGENT_LEN: usize = 128;

/// Largest extension id, in bytes (C.10).
pub const MAX_EXTENSION_ID_LEN: usize = 128;

/// The `CLOSE` body for a body that failed to encode or decode.
fn body_error(error: &CborError) -> Close {
    match error {
        CborError::Decode(_) | CborError::Encode(_) => Close::new(
            CloseCode::ProtocolViolation,
            format!("body does not match its type: {error}"),
        ),
        other => Close::new(
            CloseCode::MalformedFrame,
            format!("body is not deterministic CBOR: {other}"),
        ),
    }
}

/// The body of a `CLOSE` frame (C.7). A closed type: unknown keys are
/// rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Close {
    /// The close code text, see [`CloseCode::as_str`].
    pub code: String,
    /// Human-readable detail.
    pub message: String,
    /// For `unsupported_version`, the versions the sender supports.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub supported: Option<Vec<u64>>,
}

impl Close {
    /// A `CLOSE` body with the given code and message and no `supported`.
    pub fn new(code: CloseCode, message: impl Into<String>) -> Close {
        Close {
            code: code.as_str().to_string(),
            message: message.into(),
            supported: None,
        }
    }

    /// An `unsupported_version` `CLOSE` body listing the sender's versions.
    pub fn unsupported_version(message: impl Into<String>, supported: Vec<u64>) -> Close {
        Close {
            code: CloseCode::UnsupportedVersion.as_str().to_string(),
            message: message.into(),
            supported: Some(supported),
        }
    }

    /// The code as an enum, or `None` when the text is not a code of v1.
    pub fn close_code(&self) -> Option<CloseCode> {
        const ALL: [CloseCode; 7] = [
            CloseCode::UnsupportedVersion,
            CloseCode::UnknownFrameType,
            CloseCode::MalformedFrame,
            CloseCode::FrameTooLarge,
            CloseCode::ProtocolViolation,
            CloseCode::Timeout,
            CloseCode::ShuttingDown,
        ];
        ALL.iter().copied().find(|c| c.as_str() == self.code)
    }

    /// Encodes this body as a `CLOSE` frame with request id 0.
    pub fn to_frame(&self) -> Result<Frame, CborError> {
        Ok(Frame {
            frame_type: FrameType::Close,
            request_id: 0,
            body: cbor::to_vec(self)?,
        })
    }

    /// Decodes a `CLOSE` frame. The error is the `CLOSE` to send in answer.
    pub fn from_frame(frame: &Frame) -> Result<Close, Close> {
        decode_body(frame, FrameType::Close)
    }
}

/// Checks the frame type and decodes the body.
pub(crate) fn decode_body<T: serde::de::DeserializeOwned>(
    frame: &Frame,
    expected: FrameType,
) -> Result<T, Close> {
    if frame.frame_type != expected {
        return Err(Close::new(
            CloseCode::ProtocolViolation,
            format!(
                "expected frame type 0x{:02x}, got 0x{:02x}",
                expected.as_u8(),
                frame.frame_type.as_u8()
            ),
        ));
    }
    require_text_keys(&frame.body)?;
    cbor::from_slice(&frame.body).map_err(|e| body_error(&e))
}

/// Rejects a body whose top-level map has a key that is not text. serde's
/// field identifiers accept a byte string as a name (ciborium
/// `deserialize_identifier`), so `{h'6d61785f6672616d65': ...}` would otherwise
/// decode as `max_frame`, and an open type would silently ignore any other
/// non-text key. A body that is not deterministic CBOR, or not a map, passes
/// here and fails in the typed decode with its own code.
fn require_text_keys(body: &[u8]) -> Result<(), Close> {
    if let Ok(ciborium::Value::Map(entries)) = cbor::from_slice::<ciborium::Value>(body) {
        if entries
            .iter()
            .any(|(key, _)| !matches!(key, ciborium::Value::Text(_)))
        {
            return Err(Close::new(
                CloseCode::ProtocolViolation,
                "a body key is not text",
            ));
        }
    }
    Ok(())
}

/// The body of a `HELLO` frame (C.6). An open type: unknown keys are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Frame versions the client can speak, strictly ascending, not empty.
    pub versions: Vec<u64>,
    /// The largest frame the client will accept, 1024 to 16,777,216.
    pub max_frame: u32,
    /// Extension ids the client offers (C.10), no duplicates, possibly empty.
    pub extensions: Vec<String>,
    /// Software name and version, at most 128 bytes, informational.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub agent: Option<String>,
}

/// A `Close` for a `max_frame` outside 1024 to 16,777,216.
fn check_max_frame(max_frame: u32) -> Result<(), Close> {
    if (MIN_MAX_FRAME..=MAX_FRAME).contains(&max_frame) {
        Ok(())
    } else {
        Err(Close::new(
            CloseCode::ProtocolViolation,
            format!("max_frame {max_frame} is outside {MIN_MAX_FRAME} to {MAX_FRAME}"),
        ))
    }
}

/// A `Close` when `extensions` holds a duplicate.
fn check_no_duplicates(extensions: &[String]) -> Result<(), Close> {
    let mut seen: HashSet<&str> = HashSet::new();
    for id in extensions {
        if !seen.insert(id.as_str()) {
            return Err(Close::new(
                CloseCode::ProtocolViolation,
                format!("duplicate extension id {id:?}"),
            ));
        }
    }
    Ok(())
}

impl Hello {
    /// Checks every bound of C.6 and C.10: versions non-empty and strictly
    /// ascending, `max_frame` in range, extension ids ASCII, at most 128
    /// bytes and without duplicates, `agent` at most 128 bytes.
    pub fn validate(&self) -> Result<(), Close> {
        if self.versions.is_empty() {
            return Err(Close::new(
                CloseCode::ProtocolViolation,
                "versions is empty",
            ));
        }
        if self.versions.windows(2).any(|w| w[0] >= w[1]) {
            return Err(Close::new(
                CloseCode::ProtocolViolation,
                "versions are not strictly ascending",
            ));
        }
        check_max_frame(self.max_frame)?;
        for id in &self.extensions {
            if !id.is_ascii() || id.len() > MAX_EXTENSION_ID_LEN {
                return Err(Close::new(
                    CloseCode::ProtocolViolation,
                    "extension id is not ASCII or is longer than 128 bytes",
                ));
            }
        }
        check_no_duplicates(&self.extensions)?;
        if let Some(agent) = &self.agent {
            if agent.len() > MAX_AGENT_LEN {
                return Err(Close::new(
                    CloseCode::ProtocolViolation,
                    "agent is longer than 128 bytes",
                ));
            }
        }
        Ok(())
    }

    /// Encodes this body as a `HELLO` frame with request id 0.
    pub fn to_frame(&self) -> Result<Frame, CborError> {
        Ok(Frame {
            frame_type: FrameType::Hello,
            request_id: 0,
            body: cbor::to_vec(self)?,
        })
    }

    /// Decodes and validates a `HELLO` frame. The error is the `CLOSE` to
    /// send in answer.
    pub fn from_frame(frame: &Frame) -> Result<Hello, Close> {
        let hello: Hello = decode_body(frame, FrameType::Hello)?;
        hello.validate()?;
        Ok(hello)
    }
}

/// The body of a `HELLO_ACK` frame (C.6). An open type: unknown keys are
/// ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloAck {
    /// The selected version, one the client offered.
    pub version: u64,
    /// `min(client max_frame, server maximum)`; applies in both directions.
    pub max_frame: u32,
    /// Requests the server processes concurrently per connection, at least 1.
    pub max_inflight: u32,
    /// The accepted extensions, in the order that fixes the extension index.
    pub extensions: Vec<String>,
    /// Informational software name and version.
    #[serde(
        default,
        deserialize_with = "reject_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub agent: Option<String>,
    /// A value the server chose for this connection from a cryptographic
    /// random source (6.6). Exactly 32 bytes.
    #[serde(with = "crate::wirefmt::byte_array")]
    pub binding: [u8; 32],
    /// The node's authority mode (6.3).
    pub authority: AuthorityMode,
    /// The server's clock when it built the acknowledgement, Unix seconds.
    /// Not authenticated (6.6).
    pub time: u64,
}

/// The node's authority mode, as `HELLO_ACK.authority` reports it
/// (`docs/spec/00-wire.md` 6.3). The wire has three values; a daemon reports
/// the modes it implements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthorityMode {
    /// No grant is asked for. The default.
    #[default]
    Off,
    /// Reserved on the wire for a mode that records what `required` would
    /// refuse; no peer in this repository sends it yet.
    Audit,
    /// Every command needs authority.
    Required,
}

impl AuthorityMode {
    /// The text this mode has on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            AuthorityMode::Off => "off",
            AuthorityMode::Audit => "audit",
            AuthorityMode::Required => "required",
        }
    }
}

impl HelloAck {
    /// Checks the bounds that hold without knowing the `HELLO`: `max_frame`
    /// in range, `max_inflight` at least 1, no duplicate extension. A missing
    /// or mistyped `binding`, `authority` or `time`, a `binding` that is not
    /// 32 bytes and an unknown `authority` text are refused when the body is
    /// decoded ([`HelloAck::from_frame`]), before this runs.
    pub fn validate(&self) -> Result<(), Close> {
        check_max_frame(self.max_frame)?;
        if self.max_inflight < 1 {
            return Err(Close::new(
                CloseCode::ProtocolViolation,
                "max_inflight is 0",
            ));
        }
        check_no_duplicates(&self.extensions)
    }

    /// Client-side check of this acknowledgement against the `HELLO` that
    /// was sent: [`HelloAck::validate`], the version was offered, `max_frame`
    /// is not above the client's, and the extensions are a duplicate-free
    /// subset of those offered.
    pub fn validate_against(&self, hello: &Hello) -> Result<(), Close> {
        self.validate()?;
        if !hello.versions.contains(&self.version) {
            return Err(Close::new(
                CloseCode::ProtocolViolation,
                format!("selected version {} was not offered", self.version),
            ));
        }
        if self.max_frame > hello.max_frame {
            return Err(Close::new(
                CloseCode::ProtocolViolation,
                format!(
                    "max_frame {} is above the offered {}",
                    self.max_frame, hello.max_frame
                ),
            ));
        }
        for id in &self.extensions {
            if !hello.extensions.contains(id) {
                return Err(Close::new(
                    CloseCode::ProtocolViolation,
                    format!("extension {id:?} was not offered"),
                ));
            }
        }
        Ok(())
    }

    /// Encodes this body as a `HELLO_ACK` frame with request id 0.
    pub fn to_frame(&self) -> Result<Frame, CborError> {
        Ok(Frame {
            frame_type: FrameType::HelloAck,
            request_id: 0,
            body: cbor::to_vec(self)?,
        })
    }

    /// Decodes a `HELLO_ACK` frame and runs [`HelloAck::validate`]. The error
    /// is the `CLOSE` to send; C.7 makes a schema failure here a
    /// `protocol_violation`.
    pub fn from_frame(frame: &Frame) -> Result<HelloAck, Close> {
        let ack: HelloAck = decode_body(frame, FrameType::HelloAck)?;
        ack.validate()?;
        Ok(ack)
    }
}

/// What the local (server) side supports, input to [`negotiate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalConfig {
    /// Versions the server supports, in any order.
    pub versions: Vec<u64>,
    /// The largest frame the server accepts; clamped to 1024 to 16,777,216.
    pub max_frame: u32,
    /// Requests processed concurrently per connection; at least 1 is used.
    pub max_inflight: u32,
    /// Extension ids the server supports.
    pub extensions: Vec<String>,
    /// Informational software name and version for the `HELLO_ACK`.
    pub agent: Option<String>,
    /// The mode reported in `HELLO_ACK.authority`.
    pub authority: AuthorityMode,
    /// The `binding` to send. `None`: [`negotiate`] draws 32 bytes from the
    /// operating system's random source for every call. Set it only in tests
    /// and vectors, never in a server.
    pub binding: Option<[u8; 32]>,
    /// The `time` to send. `None`: the system clock. Injectable for tests and
    /// vectors.
    pub time: Option<u64>,
}

impl Default for LocalConfig {
    /// Version 1, the 16 MiB maximum frame, one request in flight, no
    /// extensions, no agent string.
    fn default() -> LocalConfig {
        LocalConfig {
            versions: vec![1],
            max_frame: MAX_FRAME,
            max_inflight: 1,
            extensions: Vec::new(),
            agent: None,
            authority: AuthorityMode::Off,
            binding: None,
            time: None,
        }
    }
}

/// 32 bytes from the operating system's random source. Panics only if that
/// source fails, which `rand` treats as unrecoverable.
fn fresh_binding() -> [u8; 32] {
    let mut value = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut value);
    value
}

/// The system clock in Unix seconds; 0 when it is before the epoch.
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Server side of C.6: validates `hello` and builds the `HELLO_ACK`.
///
/// The version is the highest one offered that the server supports; if there
/// is none the result is an `unsupported_version` [`Close`] carrying the
/// server's versions in `supported`. `max_frame` is the smaller of the two
/// limits. The accepted extensions are those offered that the server supports,
/// in the order the client offered them: C.10 says the position in
/// `HELLO_ACK.extensions` is the index but does not say whose order builds
/// it, so v1 here follows the client's.
///
/// The acknowledgement also gets its per-connection values: `binding` is the
/// one in `local` or, when `local` has none, fresh random bytes; `time` is the
/// one in `local` or the system clock; `authority` is copied.
pub fn negotiate(hello: &Hello, local: &LocalConfig) -> Result<HelloAck, Close> {
    hello.validate()?;
    let version = match hello
        .versions
        .iter()
        .rev()
        .copied()
        .find(|v| local.versions.contains(v))
    {
        Some(v) => v,
        None => {
            let mut supported = local.versions.clone();
            supported.sort_unstable();
            supported.dedup();
            return Err(Close::unsupported_version(
                "no offered version is supported",
                supported,
            ));
        }
    };
    let local_max = local.max_frame.clamp(MIN_MAX_FRAME, MAX_FRAME);
    let extensions: Vec<String> = hello
        .extensions
        .iter()
        .filter(|id| local.extensions.contains(id))
        .cloned()
        .collect();
    Ok(HelloAck {
        version,
        max_frame: hello.max_frame.min(local_max),
        max_inflight: local.max_inflight.max(1),
        extensions,
        agent: local.agent.clone(),
        binding: local.binding.unwrap_or_else(fresh_binding),
        authority: local.authority,
        time: local.time.unwrap_or_else(unix_now),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{decode_frame, encode_frame, PRE_HELLO_MAX_FRAME};

    const HELLO_HEX: &str = "00 00 00 32 01 01 00 00 00 00 00 00 00 00 00 a3 68 76 65 72 73 69 6f 6e 73 81 01 69 6d 61 78 5f 66 72 61 6d 65 1a 01 00 00 00 6a 65 78 74 65 6e 73 69 6f 6e 73 80";

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect()
    }

    /// CBOR text string with a short (under 24 bytes) length.
    fn text(s: &str) -> Vec<u8> {
        assert!(s.len() < 24);
        let mut out = vec![0x60 + s.len() as u8];
        out.extend_from_slice(s.as_bytes());
        out
    }

    /// A map of at most 23 entries, with the entries in the order given.
    fn map_in_order(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0xa0 + entries.len() as u8];
        for (key, value) in entries {
            out.extend(text(key));
            out.extend_from_slice(value);
        }
        out
    }

    /// A map with its entries sorted by the encoded key bytes.
    fn map_sorted(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut sorted: Vec<(&str, Vec<u8>)> = entries.to_vec();
        sorted.sort_by_key(|a| text(a.0));
        map_in_order(&sorted)
    }

    fn frame_of(frame_type: FrameType, body: Vec<u8>) -> Frame {
        Frame {
            frame_type,
            request_id: 0,
            body,
        }
    }

    fn hello(versions: &[u64], max_frame: u32, extensions: &[&str]) -> Hello {
        Hello {
            versions: versions.to_vec(),
            max_frame,
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            agent: None,
        }
    }

    fn local(versions: &[u64], max_frame: u32, extensions: &[&str]) -> LocalConfig {
        LocalConfig {
            versions: versions.to_vec(),
            max_frame,
            max_inflight: 4,
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            agent: Some("test/1".to_string()),
            authority: AuthorityMode::Off,
            binding: Some([0x42; 32]),
            time: Some(1_700_000_000),
        }
    }

    /// CBOR byte string of 32 copies of `fill`.
    fn bytes32(fill: u8) -> Vec<u8> {
        let mut out = vec![0x58, 0x20];
        out.extend_from_slice(&[fill; 32]);
        out
    }

    /// The entries of a valid `HELLO_ACK` body with `max_frame` 4096.
    fn ack_entries() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("version", vec![0x01]),
            ("max_frame", vec![0x19, 0x10, 0x00]),
            ("max_inflight", vec![0x01]),
            ("extensions", vec![0x80]),
            ("binding", bytes32(0x07)),
            ("authority", text("off")),
            ("time", vec![0x1a, 0x65, 0x53, 0xf1, 0x00]),
        ]
    }

    /// `ack_entries` with the entry `key` replaced by `value`.
    fn ack_entries_with(key: &str, value: Vec<u8>) -> Vec<(&'static str, Vec<u8>)> {
        ack_entries()
            .into_iter()
            .map(|(k, v)| if k == key { (k, value.clone()) } else { (k, v) })
            .collect()
    }

    fn ack_error(entries: &[(&str, Vec<u8>)]) -> Close {
        HelloAck::from_frame(&frame_of(FrameType::HelloAck, map_sorted(entries))).unwrap_err()
    }

    #[test]
    fn worked_example_decodes_and_reencodes_byte_for_byte() {
        let bytes = hex(HELLO_HEX);
        let (frame, used) = decode_frame(&bytes, PRE_HELLO_MAX_FRAME).unwrap();
        assert_eq!(used, bytes.len());
        let got = Hello::from_frame(&frame).unwrap();
        assert_eq!(got, hello(&[1], 16_777_216, &[]));
        let again = got.to_frame().unwrap();
        assert_eq!(again, frame);
        assert_eq!(
            encode_frame(&again, PRE_HELLO_MAX_FRAME, false).unwrap(),
            bytes
        );
    }

    #[test]
    fn hello_ack_and_close_round_trip() {
        let ack = HelloAck {
            version: 1,
            max_frame: 4096,
            max_inflight: 8,
            extensions: vec!["a/b/1".to_string()],
            agent: Some("srv".to_string()),
            binding: [9; 32],
            authority: AuthorityMode::Audit,
            time: 1,
        };
        let frame = ack.to_frame().unwrap();
        assert_eq!(frame.frame_type, FrameType::HelloAck);
        assert_eq!(frame.request_id, 0);
        assert_eq!(HelloAck::from_frame(&frame).unwrap(), ack);

        let close = Close::unsupported_version("no", vec![1, 2]);
        let frame = close.to_frame().unwrap();
        assert_eq!(frame.frame_type, FrameType::Close);
        let back = Close::from_frame(&frame).unwrap();
        assert_eq!(back, close);
        assert_eq!(back.close_code(), Some(CloseCode::UnsupportedVersion));
        assert_eq!(Close::new(CloseCode::Timeout, "t").supported, None);
    }

    #[test]
    fn absent_optional_fields_are_omitted() {
        let body = cbor::to_vec(&Close::new(CloseCode::Timeout, "x")).unwrap();
        let expected = map_sorted(&[("code", text("timeout")), ("message", text("x"))]);
        assert_eq!(body, expected);
    }

    #[test]
    fn no_shared_version_closes_with_supported() {
        let err = negotiate(&hello(&[1, 2], 4096, &[]), &local(&[5, 3], 4096, &[])).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::UnsupportedVersion));
        assert_eq!(err.supported, Some(vec![3, 5]));
    }

    #[test]
    fn highest_common_version_is_chosen() {
        let ack = negotiate(&hello(&[1, 2, 3], 4096, &[]), &local(&[3, 1, 9], 4096, &[])).unwrap();
        assert_eq!(ack.version, 3);
        let ack = negotiate(&hello(&[1, 2, 3], 4096, &[]), &local(&[2, 9], 4096, &[])).unwrap();
        assert_eq!(ack.version, 2);
    }

    #[test]
    fn accepted_extensions_are_the_supported_offers_in_client_order() {
        let h = hello(&[1], 4096, &["z/z/1", "a/a/1", "m/m/1", "q/q/1"]);
        let l = local(&[1], 4096, &["m/m/1", "z/z/1", "x/x/1"]);
        let ack = negotiate(&h, &l).unwrap();
        assert_eq!(
            ack.extensions,
            vec!["z/z/1".to_string(), "m/m/1".to_string()]
        );
        assert_eq!(ack.validate_against(&h), Ok(()));
    }

    #[test]
    fn max_frame_is_the_minimum_of_both_sides() {
        let ack = negotiate(&hello(&[1], 1_000_000, &[]), &local(&[1], 65_536, &[])).unwrap();
        assert_eq!(ack.max_frame, 65_536);
        let ack = negotiate(&hello(&[1], 8192, &[]), &local(&[1], 65_536, &[])).unwrap();
        assert_eq!(ack.max_frame, 8192);
        assert_eq!(ack.max_inflight, 4);
        assert_eq!(ack.agent.as_deref(), Some("test/1"));
    }

    #[test]
    fn local_limits_are_clamped_into_range() {
        let ack = negotiate(&hello(&[1], 4096, &[]), &local(&[1], 10, &[])).unwrap();
        assert_eq!(ack.max_frame, MIN_MAX_FRAME);
        let mut l = local(&[1], MAX_FRAME, &[]);
        l.max_inflight = 0;
        let ack = negotiate(&hello(&[1], 4096, &[]), &l).unwrap();
        assert_eq!(ack.max_inflight, 1);
    }

    #[test]
    fn max_frame_out_of_range_is_rejected() {
        for bad in [0, 1023, 16_777_217, u32::MAX] {
            let err = negotiate(&hello(&[1], bad, &[]), &LocalConfig::default()).unwrap_err();
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
        }
        for good in [1024, 16_777_216] {
            assert!(negotiate(&hello(&[1], good, &[]), &LocalConfig::default()).is_ok());
        }
    }

    #[test]
    fn versions_must_be_non_empty_and_strictly_ascending() {
        for bad in [&[][..], &[2, 1][..], &[1, 1][..], &[1, 3, 2][..]] {
            let err = hello(bad, 4096, &[]).validate().unwrap_err();
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
        }
        assert!(hello(&[1, 2, 7], 4096, &[]).validate().is_ok());
    }

    #[test]
    fn duplicate_extensions_are_rejected() {
        let err = hello(&[1], 4096, &["a/b/1", "c/d/1", "a/b/1"])
            .validate()
            .unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
    }

    #[test]
    fn extension_and_agent_bounds() {
        let long = "a".repeat(129);
        assert!(hello(&[1], 4096, &[long.as_str()]).validate().is_err());
        let ok = "a".repeat(128);
        assert!(hello(&[1], 4096, &[ok.as_str()]).validate().is_ok());
        assert!(hello(&[1], 4096, &["caf\u{e9}/x/1"]).validate().is_err());
        let mut h = hello(&[1], 4096, &[]);
        h.agent = Some(ok);
        assert!(h.validate().is_ok());
        h.agent = Some(long);
        assert!(h.validate().is_err());
    }

    #[test]
    fn unknown_hello_key_is_ignored() {
        let body = map_sorted(&[
            ("versions", vec![0x81, 0x01]),
            ("max_frame", vec![0x19, 0x10, 0x00]),
            ("extensions", vec![0x80]),
            ("future", vec![0x01]),
        ]);
        let got = Hello::from_frame(&frame_of(FrameType::Hello, body)).unwrap();
        assert_eq!(got, hello(&[1], 4096, &[]));
    }

    #[test]
    fn unknown_hello_ack_key_is_ignored() {
        let mut entries = ack_entries();
        entries.push(("future", vec![0x01]));
        let body = map_sorted(&entries);
        let got = HelloAck::from_frame(&frame_of(FrameType::HelloAck, body)).unwrap();
        assert_eq!(got.version, 1);
        assert_eq!(got.max_frame, 4096);
        assert_eq!(got.binding, [0x07; 32]);
        assert_eq!(got.authority, AuthorityMode::Off);
        assert_eq!(got.time, 1_700_000_000);
    }

    #[test]
    fn hello_ack_encodes_keys_in_deterministic_order() {
        let ack = HelloAck {
            version: 1,
            max_frame: 4096,
            max_inflight: 1,
            extensions: Vec::new(),
            agent: Some("a".to_string()),
            binding: [0x07; 32],
            authority: AuthorityMode::Off,
            time: 1_700_000_000,
        };
        let body = cbor::to_vec(&ack).unwrap();
        // time, agent, binding, version, authority, max_frame, extensions,
        // max_inflight: ascending by the bytes of the encoded key.
        let mut want = vec![0xa8];
        for (key, value) in [
            ("time", vec![0x1a, 0x65, 0x53, 0xf1, 0x00]),
            ("agent", text("a")),
            ("binding", bytes32(0x07)),
            ("version", vec![0x01]),
            ("authority", text("off")),
            ("max_frame", vec![0x19, 0x10, 0x00]),
            ("extensions", vec![0x80]),
            ("max_inflight", vec![0x01]),
        ] {
            want.extend(text(key));
            want.extend(value);
        }
        assert_eq!(body, want);
        assert_eq!(
            HelloAck::from_frame(&frame_of(FrameType::HelloAck, body)).unwrap(),
            ack
        );
    }

    #[test]
    fn hello_ack_refuses_a_missing_binding_authority_or_time() {
        for key in ["binding", "authority", "time"] {
            let entries: Vec<_> = ack_entries()
                .into_iter()
                .filter(|(k, _)| *k != key)
                .collect();
            let err = ack_error(&entries);
            assert_eq!(
                err.close_code(),
                Some(CloseCode::ProtocolViolation),
                "{key}"
            );
        }
    }

    #[test]
    fn hello_ack_refuses_a_bad_binding() {
        let mut thirty_one = vec![0x58, 0x1f];
        thirty_one.extend_from_slice(&[7; 31]);
        let mut thirty_three = vec![0x58, 0x21];
        thirty_three.extend_from_slice(&[7; 33]);
        let mut array = vec![0x98, 0x20];
        array.extend_from_slice(&[0x07; 32]);
        for bad in [
            vec![0x40],
            thirty_one,
            thirty_three,
            text("not bytes"),
            array,
            vec![0xf6],
        ] {
            let err = ack_error(&ack_entries_with("binding", bad));
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
        }
    }

    #[test]
    fn hello_ack_refuses_a_bad_authority_or_time() {
        for bad in [text("maybe"), text("Off"), text(""), vec![0x01], vec![0xf6]] {
            let err = ack_error(&ack_entries_with("authority", bad));
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
        }
        for bad in [text("0"), vec![0x20], vec![0xf9, 0x3c, 0x00], vec![0xf6]] {
            let err = ack_error(&ack_entries_with("time", bad));
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
        }
        for mode in ["off", "audit", "required"] {
            let body = map_sorted(&ack_entries_with("authority", text(mode)));
            let got = HelloAck::from_frame(&frame_of(FrameType::HelloAck, body)).unwrap();
            assert_eq!(got.authority.as_str(), mode);
        }
        // The largest u64 is a valid time.
        let max = vec![0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        let body = map_sorted(&ack_entries_with("time", max));
        assert_eq!(
            HelloAck::from_frame(&frame_of(FrameType::HelloAck, body))
                .unwrap()
                .time,
            u64::MAX
        );
    }

    #[test]
    fn negotiate_reports_the_injected_per_connection_values() {
        let mut l = local(&[1], 4096, &[]);
        l.authority = AuthorityMode::Required;
        let ack = negotiate(&hello(&[1], 4096, &[]), &l).unwrap();
        assert_eq!(ack.binding, [0x42; 32]);
        assert_eq!(ack.time, 1_700_000_000);
        assert_eq!(ack.authority, AuthorityMode::Required);
    }

    #[test]
    fn negotiate_draws_a_fresh_binding_and_reads_the_clock() {
        let mut l = local(&[1], 4096, &[]);
        l.binding = None;
        l.time = None;
        let a = negotiate(&hello(&[1], 4096, &[]), &l).unwrap();
        let b = negotiate(&hello(&[1], 4096, &[]), &l).unwrap();
        assert_ne!(a.binding, b.binding);
        assert_ne!(a.binding, [0u8; 32]);
        // After 2020-09-13 and not absurdly far in the future.
        assert!(a.time > 1_600_000_000 && a.time < 32_000_000_000);
    }

    #[test]
    fn unknown_close_key_is_rejected() {
        let body = map_sorted(&[
            ("code", text("timeout")),
            ("message", text("x")),
            ("extra", vec![0x01]),
        ]);
        let err = Close::from_frame(&frame_of(FrameType::Close, body)).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
    }

    /// `valid` is a map of three entries; this puts `key` and `value` first
    /// (a non-text key sorts before every text key) in a four-entry map.
    fn with_leading_entry(valid: &[u8], key: &[u8], value: &[u8]) -> Vec<u8> {
        let mut out = vec![valid[0] + 1];
        out.extend_from_slice(key);
        out.extend_from_slice(value);
        out.extend_from_slice(&valid[1..]);
        out
    }

    #[test]
    fn a_non_text_key_in_hello_hello_ack_or_close_is_a_protocol_violation() {
        let hello_body = map_sorted(&[
            ("versions", vec![0x81, 0x01]),
            ("max_frame", vec![0x19, 0x10, 0x00]),
            ("extensions", vec![0x80]),
        ]);
        let ack_body = map_sorted(&ack_entries());
        let close_body = map_sorted(&[("code", text("timeout")), ("message", text("x"))]);
        // An integer key and a byte-string key (h'78'), each with a value.
        for (key, value) in [(vec![0x00], vec![0x01]), (vec![0x41, 0x78], vec![0x01])] {
            let body = with_leading_entry(&hello_body, &key, &value);
            let err = Hello::from_frame(&frame_of(FrameType::Hello, body)).unwrap_err();
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
            let body = with_leading_entry(&ack_body, &key, &value);
            let err = HelloAck::from_frame(&frame_of(FrameType::HelloAck, body)).unwrap_err();
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
            let body = with_leading_entry(&close_body, &key, &value);
            let err = Close::from_frame(&frame_of(FrameType::Close, body)).unwrap_err();
            assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
        }
        // A byte-string key must not stand in for a field name: h'636f6465' is
        // "code".
        let mut body = vec![0xa2, 0x44, 0x63, 0x6f, 0x64, 0x65];
        body.extend(text("timeout"));
        body.extend(text("message"));
        body.extend(text("x"));
        let err = Close::from_frame(&frame_of(FrameType::Close, body)).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
    }

    #[test]
    fn null_for_an_optional_field_is_rejected() {
        let body = map_sorted(&[
            ("versions", vec![0x81, 0x01]),
            ("max_frame", vec![0x19, 0x10, 0x00]),
            ("extensions", vec![0x80]),
            ("agent", vec![0xf6]),
        ]);
        let err = Hello::from_frame(&frame_of(FrameType::Hello, body)).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
    }

    #[test]
    fn missing_required_hello_key_is_rejected() {
        let body = map_sorted(&[("versions", vec![0x81, 0x01]), ("extensions", vec![0x80])]);
        let err = Hello::from_frame(&frame_of(FrameType::Hello, body)).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
    }

    #[test]
    fn hello_from_frame_validates_bounds() {
        let body = map_sorted(&[
            ("versions", vec![0x82, 0x02, 0x01]),
            ("max_frame", vec![0x19, 0x10, 0x00]),
            ("extensions", vec![0x80]),
        ]);
        let err = Hello::from_frame(&frame_of(FrameType::Hello, body)).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
    }

    #[test]
    fn wrong_frame_type_is_a_protocol_violation() {
        let body = cbor::to_vec(&hello(&[1], 4096, &[])).unwrap();
        let err = Hello::from_frame(&frame_of(FrameType::HelloAck, body.clone())).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
        let err = HelloAck::from_frame(&frame_of(FrameType::Hello, body.clone())).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
        let err = Close::from_frame(&frame_of(FrameType::Ping, body)).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
    }

    #[test]
    fn non_deterministic_body_is_malformed_frame() {
        // max_frame sorts after versions; putting it first is unsorted.
        let body = map_in_order(&[
            ("max_frame", vec![0x19, 0x10, 0x00]),
            ("versions", vec![0x81, 0x01]),
            ("extensions", vec![0x80]),
        ]);
        let err = Hello::from_frame(&frame_of(FrameType::Hello, body)).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::MalformedFrame));

        // Trailing bytes after a valid body.
        let mut body = cbor::to_vec(&hello(&[1], 4096, &[])).unwrap();
        body.push(0x00);
        let err = Hello::from_frame(&frame_of(FrameType::Hello, body)).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::MalformedFrame));

        let err = Close::from_frame(&frame_of(FrameType::Close, vec![0xff])).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::MalformedFrame));
    }

    fn ack(version: u64, max_frame: u32, extensions: &[&str]) -> HelloAck {
        HelloAck {
            version,
            max_frame,
            max_inflight: 1,
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            agent: None,
            binding: [3; 32],
            authority: AuthorityMode::Off,
            time: 0,
        }
    }

    #[test]
    fn validate_against_accepts_a_good_ack() {
        let h = hello(&[1, 2], 8192, &["a/a/1", "b/b/1"]);
        assert_eq!(
            ack(2, 8192, &["b/b/1", "a/a/1"]).validate_against(&h),
            Ok(())
        );
        assert_eq!(ack(1, 4096, &[]).validate_against(&h), Ok(()));
    }

    #[test]
    fn validate_against_rejects_violations() {
        let h = hello(&[1, 2], 8192, &["a/a/1"]);
        // Version not offered.
        assert!(ack(3, 4096, &[]).validate_against(&h).is_err());
        // max_frame above the client's.
        assert!(ack(1, 8193, &[]).validate_against(&h).is_err());
        // Extension not offered.
        assert!(ack(1, 4096, &["z/z/1"]).validate_against(&h).is_err());
        // Duplicate extension.
        assert!(ack(1, 4096, &["a/a/1", "a/a/1"])
            .validate_against(&h)
            .is_err());
        // max_inflight of 0.
        let mut zero = ack(1, 4096, &[]);
        zero.max_inflight = 0;
        assert!(zero.validate_against(&h).is_err());
        // max_frame below the minimum.
        assert!(ack(1, 100, &[]).validate_against(&h).is_err());
        let err = ack(3, 4096, &[]).validate_against(&h).unwrap_err();
        assert_eq!(err.close_code(), Some(CloseCode::ProtocolViolation));
    }
}
