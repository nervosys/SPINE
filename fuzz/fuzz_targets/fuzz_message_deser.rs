#![no_main]
use libfuzzer_sys::fuzz_target;
use spine_protocol::frame::FrameType;
use spine_protocol::{cbor, native, BrowserCommand};

/// Fuzz the spine-wire v1 body decoders with arbitrary bytes.
///
/// Nothing may panic, and a body that the strict decoders accept must be the
/// canonical encoding of its value (one encoding per value), so that
/// `to_vec(decode(b)) == b`.
fuzz_target!(|data: &[u8]| {
    // The plain CBOR decoder: determinism check plus serde. It may accept a
    // body that is not canonical (see `decode_strict`), so only no-panic.
    let _ = cbor::from_slice::<BrowserCommand>(data);
    let _ = cbor::check_deterministic(data);

    // The request path.
    if let Ok(command) = native::decode_command(data) {
        let again = cbor::to_vec(&command).expect("an accepted command encodes");
        assert_eq!(again, data, "an accepted command body is canonical");
    }

    // The reply path, with a JSON-shaped result.
    if let Ok(reply) = native::decode_reply::<serde_json::Value>(data) {
        let again = cbor::to_vec(&reply).expect("an accepted reply encodes");
        assert_eq!(again, data, "an accepted reply body is canonical");
    }

    // The stream bodies.
    for frame_type in [
        FrameType::StreamStart,
        FrameType::StreamToken,
        FrameType::StreamEnd,
        FrameType::StreamCancel,
    ] {
        if let Ok(message) = native::decode_stream_body(frame_type, data) {
            let (back_type, again) =
                native::encode_stream_body(&message).expect("an accepted stream body encodes");
            assert_eq!(back_type, frame_type);
            assert_eq!(again, data, "an accepted stream body is canonical");
        }
    }

    // PING and PONG tokens.
    let _ = native::ping_token(data);
});
