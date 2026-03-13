#![no_main]
use libfuzzer_sys::fuzz_target;

/// Fuzz SemVer parsing in the transport marketplace module.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = spine_transport::marketplace::SemVer::parse(s);
    }
});