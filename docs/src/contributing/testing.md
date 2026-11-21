# Testing

Counts of tests and suites change often and are not recorded here. Earlier
figures (for example 897+ unit tests and 41 properties) were measured before the
native wire became spine-wire v1 (2026-11-21) and have not been re-measured; run
the commands below for current numbers.

## Test Categories

### Unit Tests

```bash
cargo test --workspace
```

The `research/` crates are a separate workspace; test them with
`cd research && cargo test`.

### Property-Based Tests

`proptest` properties, for example in `src/spine-protocol/tests/proptest_protocol.rs`:

```bash
cargo test --workspace -- proptest
```

### Wire and Integration Tests

The `spine-protocol` integration tests cover the v1 wire: `wire_encoding.rs`
(frame and CBOR encoding), `bodies_v1.rs` (command and reply bodies),
`e2e_protocol.rs` and `integration.rs`.

```bash
cargo test -p spine-protocol --test '*'
```

### Chaos Tests

Fault-tolerance tests in `src/spine-protocol/tests/chaos.rs`:

```bash
cargo test -p spine-protocol --test chaos
```

### Fuzz Targets

Seven `cargo-fuzz` targets in `fuzz/fuzz_targets/`:

| Target               | Input            | Tests                    |
| -------------------- | ---------------- | ------------------------ |
| `fuzz_parse_html`    | Arbitrary HTML   | Parser crash resistance  |
| `fuzz_frame_decode`  | Arbitrary bytes  | spine-wire v1 frame decoder and the transport crate's own `FrameCodec` |
| `fuzz_message_deser` | Arbitrary bytes  | v1 body decoders; accepted bodies must re-encode identically |
| `fuzz_frame_header`  | Arbitrary bytes  | The transport crate's 12-byte header (not the v1 header) |
| `fuzz_negotiation`   | Arbitrary bytes  | `HELLO` decoding, validation and negotiation |
| `fuzz_hls_compile`   | Arbitrary source | HLS compiler            |
| `fuzz_semver_parse`  | Arbitrary strings | SemVer parsing in the transport marketplace module |

### Deterministic Replay

```rust
use spine_protocol::replay::{TraceLog, ReplayVerifier};

let mut log = TraceLog::new();
// ... log.record_send(&msg, elapsed); log.record_recv(&msg, elapsed); ...
let verifier = ReplayVerifier::new(log);
let serialization = verifier.verify_serialization()?;
let ordering = verifier.verify_ordering();
```

## Test Organization

```
spine-*/
├── src/
│   └── lib.rs          # #[cfg(test)] mod tests { ... }
├── tests/
│   ├── proptest_*.rs   # Property-based tests
│   └── integration_*.rs # Integration tests
└── benches/
    └── *.rs            # Criterion benchmarks
```

## Coverage

Generate coverage reports:

```bash
./scripts/coverage.sh html
open target/coverage/html/index.html
```
