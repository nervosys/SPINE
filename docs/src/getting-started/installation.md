# Installation

## Prerequisites

- **Rust 1.75+** (install via [rustup](https://rustup.rs/))
- **OpenSSL** (for TLS features)
- **pkg-config** (Linux/macOS)

## From Source

```bash
git clone https://github.com/nervosys/SPINE.git
cd SPINE
cargo build --release
```

The following binaries are produced in `target/release/`:

| Binary          | Description                 |
| --------------- | --------------------------- |
| `spine`         | CLI tool for managing SPINE |
| `spine-core`    | Core server                 |
| `spine-gateway` | REST API gateway            |
| `spine-browser` | GUI browser (egui)          |

## Verify Installation

```bash
# Run tests
cargo test --workspace

# Check the CLI
cargo run -p spine-cli -- --help
```

## Optional Features

Enable additional features via Cargo feature flags:

```bash
# QUIC transport support
cargo build --release --features quic

# io_uring (Linux only)
cargo build --release --features io_uring
```
