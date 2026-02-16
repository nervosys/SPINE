#![no_main]
use libfuzzer_sys::fuzz_target;

/// Fuzz Frame::parse_header with arbitrary 12-byte inputs.
/// Also tests with arbitrary-length inputs to ensure bounds checking.
fuzz_target!(|data: &[u8]| {
    if data.len() >= 12 {
        let mut arr = [0u8; 12];
        arr.copy_from_slice(&data[..12]);
        let _ = spine_transport::Frame::parse_header(&arr);
    }
});
