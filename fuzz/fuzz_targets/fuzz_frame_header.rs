#![no_main]
use libfuzzer_sys::fuzz_target;

/// Fuzz the transport crate's `Frame::parse_header` with arbitrary 12-byte
/// inputs. That is the transport layer's own header, not the spine-wire v1
/// frame header (see fuzz_frame_decode for the latter).
/// Also tests with arbitrary-length inputs to ensure bounds checking.
fuzz_target!(|data: &[u8]| {
    if data.len() >= 12 {
        let mut arr = [0u8; 12];
        arr.copy_from_slice(&data[..12]);
        let _ = spine_transport::Frame::parse_header(&arr);
    }
});
