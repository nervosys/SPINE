#![no_main]
use libfuzzer_sys::fuzz_target;

/// Fuzz the HLS compiler with arbitrary source strings.
/// The compiler should never panic on malformed input.
fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        let _ = spine_compiler::Compiler::compile(source);
    }
});