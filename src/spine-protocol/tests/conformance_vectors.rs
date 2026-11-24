//! spine-wire v1 conformance vectors (`docs/spec/00-wire.vectors.json`).
//!
//! The vector file is produced by `scripts/wire_v1.py`, an independent
//! reference implementation written from the specification text in Python.
//! This test feeds every vector to the Rust implementation, using only its
//! public API, and compares the result with what the vector says:
//!
//! * `valid_bodies`: `cbor::check_deterministic` accepts them;
//! * `invalid_bodies`: it rejects each with exactly the named `CborError`
//!   variant;
//! * `valid_frames`: `frame::decode_frame`, then (unless the body is stored
//!   compressed) `frame::encode_frame` gives back the same bytes, the body is
//!   deterministic CBOR, and the typed decoder named by `typed` reads it and
//!   re-encodes it to the same bytes;
//! * `invalid_frames` and `incomplete_frames`: `decode_frame` fails with the
//!   named `FrameError` variant and close code;
//! * `handshakes`: `Hello::from_frame` and `hello::negotiate` give the
//!   expected `HELLO_ACK` (byte for byte) or the expected `CLOSE` code;
//! * `hello_acks`: `HelloAck::from_frame` accepts or refuses each body as
//!   listed;
//! * `identify`: `Identify::from_frame` and `Identify::verify` accept the
//!   proof (and give the listed signed message and signature) or refuse it
//!   with the listed `CLOSE` code;
//! * `ed25519_strict`: `verify_strict` gives the listed strict verdict;
//! * `name_results`: `native::decode_strict::<NameResolution>` accepts or
//!   refuses each body as listed, and a body it accepts re-encodes to the same
//!   bytes and passes `NameResolution::validate`;
//! * `identify_sessions`: a server `Connection` that receives the listed
//!   frames after its handshake ends as listed.
//!
//! Every check returns a message that names the vector; a vector of a kind
//! this test does not understand fails with its name instead of panicking.
//! The file is read as UTF-8 text and parsed as JSON, so line endings do not
//! matter.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use spine_protocol::cbor;
use spine_protocol::connection::Connection;
use spine_protocol::frame::{self, Frame, FrameError, FrameType, MAX_FRAME, PRE_HELLO_MAX_FRAME};
use spine_protocol::hello::{self, AuthorityMode, Close, Hello, HelloAck, LocalConfig};
use spine_protocol::identify::{signed_message, Identify, SigningKey, Transcript};
use spine_protocol::native;
use spine_protocol::NameResolution;
use tokio::io::AsyncWriteExt;

const VECTORS: &str = "../../docs/spec/00-wire.vectors.json";

fn load() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(VECTORS);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()))
}

// ---- Small helpers: every one returns a message instead of panicking.

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err(format!("not hex: {text:?}"));
    }
    (0..text.len() / 2)
        .map(|i| {
            u8::from_str_radix(&text[2 * i..2 * i + 2], 16)
                .map_err(|_| format!("not hex: {text:?}"))
        })
        .collect()
}

fn get<'a>(v: &'a Value, key: &str) -> Result<&'a Value, String> {
    v.get(key).ok_or_else(|| format!("missing field {key:?}"))
}

fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    get(v, key)?
        .as_str()
        .ok_or_else(|| format!("field {key:?} is not a string"))
}

/// An unsigned integer: a JSON number, or `{"$int": "<decimal>"}` for one
/// beyond 2^53.
fn uint_value(v: &Value) -> Result<u64, String> {
    if let Some(n) = v.as_u64() {
        return Ok(n);
    }
    v.get("$int")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| format!("not an unsigned integer: {v}"))
}

fn uint(v: &Value, key: &str) -> Result<u64, String> {
    uint_value(get(v, key)?).map_err(|e| format!("field {key:?}: {e}"))
}

fn uint32(v: &Value, key: &str) -> Result<u32, String> {
    u32::try_from(uint(v, key)?).map_err(|_| format!("field {key:?} does not fit in 32 bits"))
}

fn list<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    get(v, key)?
        .as_array()
        .ok_or_else(|| format!("field {key:?} is not an array"))
}

fn texts(v: &Value, key: &str) -> Result<Vec<String>, String> {
    list(v, key)?
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("field {key:?} holds a non-string"))
        })
        .collect()
}

fn uints(v: &Value, key: &str) -> Result<Vec<u64>, String> {
    list(v, key)?
        .iter()
        .map(|x| uint_value(x).map_err(|e| format!("field {key:?}: {e}")))
        .collect()
}

/// A string field that may be JSON null.
fn opt_text(v: &Value, key: &str) -> Result<Option<String>, String> {
    match get(v, key)? {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        _ => Err(format!("field {key:?} is neither a string nor null")),
    }
}

fn frame_type_named(name: &str) -> Result<FrameType, String> {
    Ok(match name {
        "HELLO" => FrameType::Hello,
        "HELLO_ACK" => FrameType::HelloAck,
        "CLOSE" => FrameType::Close,
        "PING" => FrameType::Ping,
        "PONG" => FrameType::Pong,
        "IDENTIFY" => FrameType::Identify,
        "REQUEST" => FrameType::Request,
        "RESPONSE" => FrameType::Response,
        "STREAM_START" => FrameType::StreamStart,
        "STREAM_TOKEN" => FrameType::StreamToken,
        "STREAM_END" => FrameType::StreamEnd,
        "STREAM_CANCEL" => FrameType::StreamCancel,
        "EXT" => FrameType::Ext,
        other => return Err(format!("unknown frame type name {other:?}")),
    })
}

fn frame_error_name(error: &FrameError) -> &'static str {
    match error {
        FrameError::LengthTooSmall { .. } => "LengthTooSmall",
        FrameError::LengthTooLarge { .. } => "LengthTooLarge",
        FrameError::UnsupportedVersion(_) => "UnsupportedVersion",
        FrameError::UnknownFrameType(_) => "UnknownFrameType",
        FrameError::ReservedFlags(_) => "ReservedFlags",
        FrameError::BadRequestId { .. } => "BadRequestId",
        FrameError::CompressedBodyTooShort => "CompressedBodyTooShort",
        FrameError::UncompressedTooLarge { .. } => "UncompressedTooLarge",
        FrameError::UncompressedLengthMismatch { .. } => "UncompressedLengthMismatch",
        FrameError::Decompress(_) => "Decompress",
        FrameError::Truncated => "Truncated",
        FrameError::NeedMore { .. } => "NeedMore",
        FrameError::Io(_) => "Io",
    }
}

fn same_bytes(what: &str, got: &[u8], want: &[u8]) -> Result<(), String> {
    if got == want {
        Ok(())
    } else {
        Err(format!(
            "{what}: got {}, want {}",
            to_hex(got),
            to_hex(want)
        ))
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let shown: String = bytes.iter().take(48).map(|b| format!("{b:02x}")).collect();
    if bytes.len() > 48 {
        format!("{shown}... ({} bytes)", bytes.len())
    } else {
        shown
    }
}

/// Runs `check` on every vector of `section` and fails with one message that
/// names each vector that did not pass. `minimum` guards against a file that
/// lost its vectors.
fn run_section(
    root: &Value,
    section: &str,
    minimum: usize,
    check: impl Fn(&Value) -> Result<(), String>,
) {
    let vectors = root
        .get(section)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("section {section:?} is missing from the vector file"));
    assert!(
        vectors.len() >= minimum,
        "section {section:?} has {} vectors, expected at least {minimum}",
        vectors.len()
    );
    let mut failures = Vec::new();
    for v in vectors {
        let name = v.get("name").and_then(Value::as_str).unwrap_or("<unnamed>");
        if let Err(message) = check(v) {
            failures.push(format!("  {section}/{name}: {message}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} vectors in {section:?} failed:\n{}",
        failures.len(),
        vectors.len(),
        failures.join("\n")
    );
}

// ---- Typed bodies.

fn check_typed(kind: &str, frame: &Frame) -> Result<(), String> {
    let body = &frame.body;
    match kind {
        "none" => Ok(()),
        "command" => {
            let command =
                native::decode_command(body).map_err(|e| format!("decode_command: {e}"))?;
            let again =
                native::encode_command(&command).map_err(|e| format!("encode_command: {e}"))?;
            same_bytes("re-encoded command", &again, body)
        }
        "reply" => {
            let reply =
                native::decode_reply::<Value>(body).map_err(|e| format!("decode_reply: {e}"))?;
            let again = cbor::to_vec(&reply).map_err(|e| format!("to_vec: {e}"))?;
            same_bytes("re-encoded reply", &again, body)
        }
        "ping" | "pong" => {
            let token = native::ping_token(body).map_err(|e| format!("ping_token: {e}"))?;
            let again = native::ping_body(token).map_err(|e| format!("ping_body: {e}"))?;
            same_bytes("re-encoded token", &again, body)
        }
        "stream" => {
            let message = native::decode_stream_body(frame.frame_type, body)
                .map_err(|e| format!("decode_stream_body: {e}"))?;
            let (frame_type, again) = native::encode_stream_body(&message)
                .map_err(|e| format!("encode_stream_body: {e}"))?;
            if frame_type != frame.frame_type {
                return Err(format!("re-encoded as {frame_type:?}"));
            }
            same_bytes("re-encoded stream body", &again, body)
        }
        "hello" => {
            let hello =
                Hello::from_frame(frame).map_err(|c| format!("Hello::from_frame: {c:?}"))?;
            let again = hello
                .to_frame()
                .map_err(|e| format!("Hello::to_frame: {e}"))?;
            same_bytes("re-encoded HELLO body", &again.body, body)
        }
        "hello_ack" => {
            let ack =
                HelloAck::from_frame(frame).map_err(|c| format!("HelloAck::from_frame: {c:?}"))?;
            let again = ack
                .to_frame()
                .map_err(|e| format!("HelloAck::to_frame: {e}"))?;
            same_bytes("re-encoded HELLO_ACK body", &again.body, body)
        }
        "identify" => {
            let identify =
                Identify::from_frame(frame).map_err(|c| format!("Identify::from_frame: {c:?}"))?;
            let again = identify
                .to_frame()
                .map_err(|e| format!("Identify::to_frame: {e}"))?;
            same_bytes("re-encoded IDENTIFY body", &again.body, body)
        }
        "close" => {
            let close =
                Close::from_frame(frame).map_err(|c| format!("Close::from_frame: {c:?}"))?;
            let again = close
                .to_frame()
                .map_err(|e| format!("Close::to_frame: {e}"))?;
            same_bytes("re-encoded CLOSE body", &again.body, body)
        }
        "ext" => {
            let value = cbor::from_slice::<ciborium::Value>(body)
                .map_err(|e| format!("from_slice: {e}"))?;
            match value {
                ciborium::Value::Array(items)
                    if items.len() == 3
                        && matches!(items[0], ciborium::Value::Integer(_))
                        && matches!(items[1], ciborium::Value::Integer(_)) =>
                {
                    Ok(())
                }
                other => Err(format!("EXT body is not [index, kind, payload]: {other:?}")),
            }
        }
        other => Err(format!("unknown typed kind {other:?}")),
    }
}

// ---- Sections.

fn check_valid_frame(v: &Value) -> Result<(), String> {
    let bytes = unhex(text(v, "hex")?)?;
    let (frame, used) =
        frame::decode_frame(&bytes, MAX_FRAME).map_err(|e| format!("decode_frame: {e}"))?;
    if used != bytes.len() {
        return Err(format!("decode_frame used {used} of {} bytes", bytes.len()));
    }
    let want_type = frame_type_named(text(v, "type")?)?;
    if frame.frame_type != want_type {
        return Err(format!("type {:?}, want {want_type:?}", frame.frame_type));
    }
    let want_id = uint_value(get(v, "request_id")?)?;
    if frame.request_id != want_id {
        return Err(format!("request id {}, want {want_id}", frame.request_id));
    }
    let want_flags = uint(v, "flags")?;
    if u64::from(bytes[6]) != want_flags {
        return Err(format!("flags byte {}, want {want_flags}", bytes[6]));
    }
    let want_body = unhex(text(v, "body_hex")?)?;
    same_bytes("decoded body", &frame.body, &want_body)?;
    cbor::check_deterministic(&frame.body).map_err(|e| format!("check_deterministic: {e:?}"))?;
    // A compressed body is stored in a raw-block zstd frame, which is not
    // what a compressing sender emits, so re-encoding is not compared.
    if want_flags & 1 == 0 {
        let again = frame::encode_frame(&frame, MAX_FRAME, false)
            .map_err(|e| format!("encode_frame: {e}"))?;
        same_bytes("re-encoded frame", &again, &bytes)?;
    } else {
        // The sender policy may compress it differently; whatever it emits must decode to the same body.
        let again = frame::encode_frame(&frame, MAX_FRAME, true)
            .map_err(|e| format!("encode_frame (compress): {e}"))?;
        let (back, _) = frame::decode_frame(&again, MAX_FRAME)
            .map_err(|e| format!("decode of the re-encoded frame: {e}"))?;
        same_bytes("body after re-encoding", &back.body, &frame.body)?;
    }
    check_typed(text(v, "typed")?, &frame)
}

fn check_valid_body(v: &Value) -> Result<(), String> {
    let bytes = unhex(text(v, "hex")?)?;
    cbor::check_deterministic(&bytes).map_err(|e| format!("rejected as {e:?}"))
}

fn check_invalid_body(v: &Value) -> Result<(), String> {
    let bytes = unhex(text(v, "hex")?)?;
    let want = text(v, "reason")?;
    match cbor::check_deterministic(&bytes) {
        Ok(()) => return Err(format!("accepted, want {want}")),
        Err(e) => {
            let got = format!("{e:?}");
            if got != want {
                return Err(format!("rejected as {got}, want {want}"));
            }
        }
    }
    // The decoders validate first, so they must name the same variant.
    match cbor::from_slice::<ciborium::Value>(&bytes) {
        Ok(_) => Err(format!("from_slice accepted it, want {want}")),
        Err(e) => {
            let got = format!("{e:?}");
            if got == want {
                Ok(())
            } else {
                Err(format!("from_slice rejected as {got}, want {want}"))
            }
        }
    }
}

fn check_invalid_frame(v: &Value) -> Result<(), String> {
    let bytes = unhex(text(v, "hex")?)?;
    let max_frame = uint32(v, "max_frame")?;
    let want_error = text(v, "error")?;
    let want_close = opt_text(v, "close_code")?;
    match frame::decode_frame(&bytes, max_frame) {
        Ok((frame, _)) => Err(format!(
            "decoded as {:?}, want {want_error}",
            frame.frame_type
        )),
        Err(e) => {
            let got = frame_error_name(&e);
            if got != want_error {
                return Err(format!("rejected as {got} ({e}), want {want_error}"));
            }
            let got_close = e.close_code().map(|c| c.as_str().to_string());
            if got_close != want_close {
                return Err(format!("close code {got_close:?}, want {want_close:?}"));
            }
            if let FrameError::NeedMore { needed } = e {
                let want_needed = uint(v, "needed")?;
                if needed as u64 != want_needed {
                    return Err(format!("needs {needed} more bytes, want {want_needed}"));
                }
            }
            Ok(())
        }
    }
}

fn local_config(v: &Value) -> Result<LocalConfig, String> {
    let local = get(v, "local")?;
    Ok(LocalConfig {
        versions: uints(local, "versions")?,
        max_frame: uint32(local, "max_frame")?,
        max_inflight: uint32(local, "max_inflight")?,
        extensions: texts(local, "extensions")?,
        agent: opt_text(local, "agent")?,
        authority: authority_named(text(local, "authority")?)?,
        binding: Some(bytes32(local, "binding")?),
        time: Some(uint(local, "time")?),
    })
}

fn check_handshake(v: &Value) -> Result<(), String> {
    let bytes = unhex(text(v, "hello_frame")?)?;
    let local = local_config(v)?;
    let expect = get(v, "expect")?;
    let (frame, _) = frame::decode_frame(&bytes, PRE_HELLO_MAX_FRAME)
        .map_err(|e| format!("decode_frame of the HELLO: {e}"))?;
    let outcome = Hello::from_frame(&frame).and_then(|h| hello::negotiate(&h, &local));
    match (text(expect, "result")?, outcome) {
        ("ack", Ok(ack)) => {
            let want = get(expect, "ack")?;
            let got_version = ack.version;
            if got_version != uint(want, "version")? {
                return Err(format!("version {got_version}"));
            }
            if u64::from(ack.max_frame) != uint(want, "max_frame")? {
                return Err(format!("max_frame {}", ack.max_frame));
            }
            if u64::from(ack.max_inflight) != uint(want, "max_inflight")? {
                return Err(format!("max_inflight {}", ack.max_inflight));
            }
            if ack.extensions != texts(want, "extensions")? {
                return Err(format!("extensions {:?}", ack.extensions));
            }
            if ack.agent != opt_text(want, "agent")? {
                return Err(format!("agent {:?}", ack.agent));
            }
            if full_hex(&ack.binding) != text(want, "binding")? {
                return Err(format!("binding {}", full_hex(&ack.binding)));
            }
            if ack.authority.as_str() != text(want, "authority")? {
                return Err(format!("authority {:?}", ack.authority));
            }
            if ack.time != uint(want, "time")? {
                return Err(format!("time {}", ack.time));
            }
            let ack_frame = ack
                .to_frame()
                .map_err(|e| format!("HelloAck::to_frame: {e}"))?;
            let encoded = frame::encode_frame(&ack_frame, MAX_FRAME, false)
                .map_err(|e| format!("encode_frame: {e}"))?;
            same_bytes(
                "HELLO_ACK frame",
                &encoded,
                &unhex(text(expect, "ack_frame")?)?,
            )
        }
        ("ack", Err(close)) => Err(format!("closed with {close:?}, want an ack")),
        ("close", Ok(ack)) => Err(format!(
            "negotiated {ack:?}, want close {}",
            text(expect, "code")?
        )),
        ("close", Err(close)) => {
            let want_code = text(expect, "code")?;
            if close.code != want_code {
                return Err(format!("close code {:?}, want {want_code:?}", close.code));
            }
            let want_supported = match get(expect, "supported")? {
                Value::Null => None,
                _ => Some(uints(expect, "supported")?),
            };
            if close.supported != want_supported {
                return Err(format!(
                    "supported {:?}, want {want_supported:?}",
                    close.supported
                ));
            }
            Ok(())
        }
        (other, _) => Err(format!("unknown expected result {other:?}")),
    }
}

fn full_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn bytes32(v: &Value, key: &str) -> Result<[u8; 32], String> {
    let bytes = unhex(text(v, key)?)?;
    <[u8; 32]>::try_from(bytes).map_err(|b| format!("field {key:?} has {} bytes, want 32", b.len()))
}

fn authority_named(name: &str) -> Result<AuthorityMode, String> {
    match name {
        "off" => Ok(AuthorityMode::Off),
        "audit" => Ok(AuthorityMode::Audit),
        "required" => Ok(AuthorityMode::Required),
        other => Err(format!("unknown authority mode {other:?}")),
    }
}

/// `HELLO_ACK` bodies: accepted ("ok", and re-encoded to the same bytes) or
/// refused with the named `CLOSE` code.
fn check_hello_ack(v: &Value) -> Result<(), String> {
    let body = unhex(text(v, "body_hex")?)?;
    let frame = Frame {
        frame_type: FrameType::HelloAck,
        request_id: 0,
        body: body.clone(),
    };
    let want = text(v, "expect")?;
    match (want, HelloAck::from_frame(&frame)) {
        ("ok", Ok(ack)) => {
            let again = ack
                .to_frame()
                .map_err(|e| format!("HelloAck::to_frame: {e}"))?;
            same_bytes("re-encoded HELLO_ACK body", &again.body, &body)
        }
        ("ok", Err(close)) => Err(format!("refused with {}, want it accepted", close.code)),
        (code, Ok(_)) => Err(format!("accepted, want {code}")),
        (code, Err(close)) if close.code == code => Ok(()),
        (code, Err(close)) => Err(format!("refused with {}, want {code}", close.code)),
    }
}

/// `name_results`: the `ok` value of a `resolve_name` answer is read strictly
/// (one encoding per value, unknown fields refused) and its succession logs must
/// have the sizes and shapes `docs/spec/00-wire.md` 12.5 gives.
fn check_name_result(v: &Value) -> Result<(), String> {
    let body = unhex(text(v, "body_hex")?)?;
    let want = text(v, "expect")?;
    match (want, native::decode_strict::<NameResolution>(&body)) {
        ("ok", Ok(resolution)) => {
            resolution
                .validate()
                .map_err(|e| format!("decoded, but validate says: {e}"))?;
            let again = cbor::to_vec(&resolution).map_err(|e| format!("re-encoding: {e}"))?;
            same_bytes("re-encoded name resolution", &again, &body)
        }
        ("ok", Err(e)) => Err(format!("refused ({e}), want it accepted")),
        ("invalid", Ok(_)) => Err("accepted, want it refused".to_string()),
        ("invalid", Err(_)) => Ok(()),
        (other, _) => Err(format!("unknown expected verdict {other:?}")),
    }
}

/// What a server makes of an `IDENTIFY` body: the `CLOSE` code and reason of
/// the refusal, or the identity and the decoded body. The deterministic-CBOR
/// check comes first, as in the connection.
fn judge_identify(
    body: &[u8],
    transcript: &Transcript,
) -> Result<(Identify, [u8; 32]), (String, String)> {
    if let Err(e) = cbor::check_deterministic(body) {
        return Err(("malformed_frame".to_string(), format!("{e:?}")));
    }
    let frame = Frame {
        frame_type: FrameType::Identify,
        request_id: 0,
        body: body.to_vec(),
    };
    let identify = Identify::from_frame(&frame).map_err(|close| (close.code, close.message))?;
    let key = identify
        .verify(transcript)
        .map_err(|reason| ("protocol_violation".to_string(), reason))?;
    Ok((identify, key))
}

fn check_identify(v: &Value) -> Result<(), String> {
    let hello_body = unhex(text(v, "hello_body")?)?;
    let ack_body = unhex(text(v, "ack_body")?)?;
    let body = unhex(text(v, "body_hex")?)?;
    let transcript = Transcript::of_bodies(&hello_body, &ack_body);
    let want = text(v, "expect")?;
    match (want, judge_identify(&body, &transcript)) {
        ("valid", Ok((identify, key))) => {
            if full_hex(&key) != text(v, "key")? {
                return Err(format!("key {}", full_hex(&key)));
            }
            let message = signed_message(&transcript, &identify.channel, &identify.key);
            same_bytes("signed message", &message, &unhex(text(v, "message")?)?)?;
            same_bytes("signature", &identify.sig, &unhex(text(v, "signature")?)?)?;
            // Signing again with the test seed gives the same body.
            let signing = SigningKey::from_bytes(&bytes32(v, "seed")?);
            let again = Identify::sign(&signing, &transcript)
                .to_frame()
                .map_err(|e| format!("Identify::to_frame: {e}"))?;
            same_bytes("body signed by the seed", &again.body, &body)
        }
        ("valid", Err((code, reason))) => Err(format!("refused with {code} ({reason})")),
        (code, Ok(_)) => Err(format!("accepted, want {code}")),
        (code, Err((got, _))) if got == code => Ok(()),
        (code, Err((got, reason))) => Err(format!("refused with {got} ({reason}), want {code}")),
    }
}

/// `verify_strict` of the crate in use gives the strict verdict of the vector.
/// The plain RFC 8032 verdict is the reference implementation's alone.
fn check_ed25519_strict(v: &Value) -> Result<(), String> {
    let public = bytes32(v, "public")?;
    let message = unhex(text(v, "message")?)?;
    let signature = <[u8; 64]>::try_from(unhex(text(v, "signature")?)?)
        .map_err(|b| format!("signature has {} bytes, want 64", b.len()))?;
    let want = get(v, "strict")?
        .as_bool()
        .ok_or_else(|| "field \"strict\" is not a boolean".to_string())?;
    let got = ed25519_dalek::VerifyingKey::from_bytes(&public)
        .ok()
        .is_some_and(|key| {
            key.verify_strict(&message, &ed25519_dalek::Signature::from_bytes(&signature))
                .is_ok()
        });
    if got == want {
        Ok(())
    } else {
        Err(format!("verify_strict gave {got}, want {want}"))
    }
}

/// Runs the server side of one `identify_sessions` vector: the handshake,
/// then the listed frames, then the end of the stream.
async fn run_session(v: &Value) -> Result<(), String> {
    let hello_frame = unhex(text(v, "hello_frame")?)?;
    let local = local_config(v)?;
    let expect = get(v, "expect")?;
    let (mut peer, ours) = tokio::io::duplex(1 << 20);
    peer.write_all(&hello_frame)
        .await
        .map_err(|e| format!("writing HELLO: {e}"))?;
    let mut conn = Connection::server_handshake(ours, &local, Duration::from_secs(10))
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    for frame in list(v, "frames")? {
        let hex = frame
            .as_str()
            .ok_or_else(|| "a frame is not a string".to_string())?;
        peer.write_all(&unhex(hex)?)
            .await
            .map_err(|e| format!("writing a frame: {e}"))?;
    }
    peer.shutdown()
        .await
        .map_err(|e| format!("shutting down the write side: {e}"))?;
    let ended = loop {
        match conn.recv().await {
            Ok(Some(_)) => {}
            Ok(None) => break Ok(()),
            Err(e) => break Err(e.close_code().map(|code| code.as_str())),
        }
    };
    match (text(expect, "result")?, ended) {
        ("ok", Ok(())) => {
            let got = conn.identity().map(|key| full_hex(&key));
            let want = opt_text(expect, "identity")?;
            if got == want {
                Ok(())
            } else {
                Err(format!("identity {got:?}, want {want:?}"))
            }
        }
        ("ok", Err(code)) => Err(format!("closed with {code:?}, want every frame accepted")),
        ("close", Ok(())) => Err(format!(
            "every frame was accepted, want close {}",
            text(expect, "code")?
        )),
        ("close", Err(code)) => {
            let want = text(expect, "code")?;
            if code == Some(want) {
                Ok(())
            } else {
                Err(format!("closed with {code:?}, want {want:?}"))
            }
        }
        (other, _) => Err(format!("unknown expected result {other:?}")),
    }
}

// ---- Tests.

#[test]
fn the_vector_file_is_the_expected_format() {
    let root = load();
    assert_eq!(
        root.get("format").and_then(Value::as_str),
        Some("spine-wire-vectors")
    );
    assert_eq!(root.get("version").and_then(Value::as_u64), Some(1));
}

#[test]
fn valid_bodies_are_deterministic_cbor() {
    run_section(&load(), "valid_bodies", 60, check_valid_body);
}

#[test]
fn invalid_bodies_are_rejected_with_the_named_variant() {
    let root = load();
    run_section(&root, "invalid_bodies", 60, check_invalid_body);
    // Every rejection variant that a body can cause has at least one vector.
    let reasons: Vec<&str> = root["invalid_bodies"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v["reason"].as_str()).collect())
        .unwrap_or_default();
    for variant in [
        "IndefiniteLength",
        "NonShortestInteger",
        "NonShortestFloat",
        "NonCanonicalNan",
        "UnsortedOrDuplicateKey",
        "Tag",
        "UnsupportedSimple",
        "InvalidUtf8",
        "UnexpectedBreak",
        "ReservedInfo",
        "TrailingBytes",
        "Truncated",
        "DepthExceeded",
    ] {
        assert!(reasons.contains(&variant), "no vector for {variant}");
    }
}

#[test]
fn valid_frames_decode_re_encode_and_read_as_their_type() {
    let root = load();
    run_section(&root, "valid_frames", 40, check_valid_frame);
    let types: Vec<&str> = root["valid_frames"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v["type"].as_str()).collect())
        .unwrap_or_default();
    for name in [
        "HELLO",
        "HELLO_ACK",
        "CLOSE",
        "PING",
        "PONG",
        "IDENTIFY",
        "REQUEST",
        "RESPONSE",
        "STREAM_START",
        "STREAM_TOKEN",
        "STREAM_END",
        "STREAM_CANCEL",
        "EXT",
    ] {
        assert!(types.contains(&name), "no valid frame vector for {name}");
    }
}

#[test]
fn invalid_frames_are_rejected_with_the_named_error_and_close_code() {
    let root = load();
    run_section(&root, "invalid_frames", 40, check_invalid_frame);
    run_section(&root, "incomplete_frames", 4, check_invalid_frame);
}

#[test]
fn handshakes_negotiate_as_listed() {
    run_section(&load(), "handshakes", 30, check_handshake);
}

#[test]
fn hello_acks_are_accepted_or_refused_as_listed() {
    run_section(&load(), "hello_acks", 25, check_hello_ack);
}

#[test]
fn identify_proofs_are_accepted_or_refused_as_listed() {
    run_section(&load(), "identify", 25, check_identify);
}

#[test]
fn name_results_are_accepted_or_refused_as_listed() {
    run_section(&load(), "name_results", 60, check_name_result);
}

#[test]
fn strict_verification_gives_the_listed_verdicts() {
    run_section(&load(), "ed25519_strict", 6, check_ed25519_strict);
}

#[tokio::test]
async fn identify_sessions_end_as_listed() {
    let root = load();
    let vectors = root
        .get("identify_sessions")
        .and_then(Value::as_array)
        .expect("section \"identify_sessions\" is missing from the vector file");
    assert!(
        vectors.len() >= 15,
        "section \"identify_sessions\" has {} vectors, expected at least 15",
        vectors.len()
    );
    let mut failures = Vec::new();
    for v in vectors {
        let name = v.get("name").and_then(Value::as_str).unwrap_or("<unnamed>");
        match tokio::time::timeout(Duration::from_secs(30), run_session(v)).await {
            Ok(Ok(())) => {}
            Ok(Err(message)) => failures.push(format!("  identify_sessions/{name}: {message}")),
            Err(_) => failures.push(format!("  identify_sessions/{name}: no end within 30 s")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} identify_sessions vectors failed:\n{}",
        failures.len(),
        vectors.len(),
        failures.join("\n")
    );
}
