#![no_main]
use bytes::Bytes;
use libfuzzer_sys::fuzz_target;

/// Fuzz both frame decoders with arbitrary byte slices; neither may panic.
///
/// - `spine_protocol::frame::decode_frame` is the spine-wire v1 frame decoder
///   (docs/spec/00-wire.md C.1), with the pre-HELLO and a large limit.
/// - `spine_transport::FrameCodec` is the transport crate's own 12-byte frame
///   format. It is not the native client-daemon wire.
fuzz_target!(|data: &[u8]| {
    for limit in [
        spine_protocol::frame::PRE_HELLO_MAX_FRAME,
        16 * 1024 * 1024,
    ] {
        let _ = spine_protocol::frame::decode_frame(data, limit);
    }

    let mut codec = spine_transport::FrameCodec::new(1 << 20);
    let _ = codec.decode(data);
    // Also test zero-copy path
    let mut codec2 = spine_transport::FrameCodec::new(1 << 20);
    let _ = codec2.decode_zerocopy(Bytes::copy_from_slice(data));
});
