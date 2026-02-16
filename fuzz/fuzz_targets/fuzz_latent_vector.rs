#![no_main]
use libfuzzer_sys::fuzz_target;

/// Fuzz LatentVector::from_bytes_fast with arbitrary byte slices.
/// Ensures it never panics on malformed input.
fuzz_target!(|data: &[u8]| {
    let _ = spine_protocol::LatentVector::from_bytes_fast(data);
});
