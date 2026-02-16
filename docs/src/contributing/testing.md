# Testing

## Test Categories

### Unit Tests

321+ tests across all crates:

```bash
cargo test --workspace
```

### Property-Based Tests

41 properties using `proptest` across 4 crates:

```bash
cargo test --workspace -- proptest
```

### Integration Tests

11 multi-session in-process tests:

```bash
cargo test -p spine-protocol --test '*'
```

### Chaos Tests

13 tests for fault tolerance:

```bash
cargo test -p spine-protocol -- chaos
```

### Fuzz Targets

5 `cargo-fuzz` targets:

| Target               | Input            | Tests                    |
| -------------------- | ---------------- | ------------------------ |
| `fuzz_parse_html`    | Arbitrary HTML   | Parser crash resistance  |
| `fuzz_frame_decode`  | Arbitrary bytes  | Frame decoder robustness |
| `fuzz_latent_vector` | Arbitrary floats | Latent vector handling   |
| `fuzz_message_deser` | Arbitrary JSON   | Message deserialization  |
| `fuzz_frame_header`  | Arbitrary bytes  | Header parsing           |

### Deterministic Replay

```rust
use spine_protocol::replay::{TraceLog, ReplayVerifier};

let mut log = TraceLog::new();
// ... record messages ...
let verifier = ReplayVerifier::new(&log);
verifier.verify()?;
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
