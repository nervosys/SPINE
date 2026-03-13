#![no_main]
use libfuzzer_sys::fuzz_target;

/// Fuzz deserialization of protocol version negotiation messages.
/// Tests both VersionOffer and VersionResponse parsing with arbitrary bytes.
fuzz_target!(|data: &[u8]| {
    let _ = spine_protocol::negotiation::deserialize_offer(data);
    let _ = spine_protocol::negotiation::deserialize_response(data);
});