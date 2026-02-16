#![no_main]
use libfuzzer_sys::fuzz_target;

/// Fuzz Message JSON deserialization with arbitrary bytes.
/// Ensures serde never panics on malformed JSON.
fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<spine_protocol::Message>(data);
    let _ = serde_json::from_slice::<spine_protocol::Request>(data);
    let _ = serde_json::from_slice::<spine_protocol::Response>(data);
    let _ = serde_json::from_slice::<spine_protocol::LatentVector>(data);
});
