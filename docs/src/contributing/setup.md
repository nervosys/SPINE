# Development Setup

## Prerequisites

```bash
rustup update stable
rustup component add llvm-tools-preview  # For coverage
cargo install cargo-llvm-cov             # Coverage tool
cargo install cargo-fuzz                  # Fuzz testing
cargo install mdbook                     # Documentation
```

## Building

```bash
cargo build --workspace
cargo build --workspace --release
```

## Running Tests

```bash
# All tests
cargo test --workspace

# Specific crate
cargo test -p spine-protocol

# With output
cargo test --workspace -- --nocapture
```

## Linting

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

## Code Coverage

```bash
# HTML report
./scripts/coverage.sh html

# JSON summary
./scripts/coverage.sh json

# LCOV format
./scripts/coverage.sh lcov
```

## Fuzz Testing

```bash
cd fuzz
cargo fuzz run fuzz_parse_html -- -max_total_time=60
cargo fuzz run fuzz_frame_decode -- -max_total_time=60
cargo fuzz run fuzz_latent_vector -- -max_total_time=60
cargo fuzz run fuzz_message_deser -- -max_total_time=60
cargo fuzz run fuzz_frame_header -- -max_total_time=60
```

## Documentation

```bash
# Build mdBook docs
cd docs && mdbook build

# Serve locally
cd docs && mdbook serve

# Rust API docs
cargo doc --workspace --no-deps --open
```
