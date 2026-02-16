#![no_main]
use libfuzzer_sys::fuzz_target;

/// Fuzz the HTML parser with arbitrary strings.
/// Ensures parse_html never panics or causes UB.
fuzz_target!(|data: &str| {
    let _ = spine_parser::parse_html(data);
});
