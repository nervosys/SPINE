#![no_main]
use libfuzzer_sys::fuzz_target;

/// Fuzz the spine-wire v1 handshake path with arbitrary bytes: the
/// deterministic-CBOR check, the frame decoder, and `HELLO` decoding with
/// its validation and negotiation. None of them may panic.
fuzz_target!(|data: &[u8]| {
    let _ = spine_protocol::cbor::check_deterministic(data);
    if let Ok((frame, _used)) =
        spine_protocol::frame::decode_frame(data, spine_protocol::frame::PRE_HELLO_MAX_FRAME)
    {
        if let Ok(hello) = spine_protocol::hello::Hello::from_frame(&frame) {
            let local = spine_protocol::hello::LocalConfig::default();
            let _ = spine_protocol::hello::negotiate(&hello, &local);
        }
    }
    // A bare body is also a HELLO candidate: skip the frame header.
    let frame = spine_protocol::frame::Frame {
        frame_type: spine_protocol::frame::FrameType::Hello,
        request_id: 0,
        body: data.to_vec(),
    };
    let _ = spine_protocol::hello::Hello::from_frame(&frame);
});
