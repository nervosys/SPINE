#![no_main]
use libfuzzer_sys::fuzz_target;
use bytes::Bytes;

/// Fuzz FrameCodec::decode with arbitrary byte slices.
/// Ensures it never panics on malformed frames.
fuzz_target!(|data: &[u8]| {
    let mut codec = spine_transport::FrameCodec::new();
    let _ = codec.decode(data);
    // Also test zero-copy path
    let mut codec2 = spine_transport::FrameCodec::new();
    let _ = codec2.decode_zerocopy(Bytes::copy_from_slice(data));
});
