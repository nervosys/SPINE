# Installation

## Prerequisites

- **Rust 1.89+** (install via [rustup](https://rustup.rs/))
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

The GUI browser (`spine-browser`) is now in the separate `research/` workspace.
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
# Native QUIC listener in the daemon (off by default)
cargo build --release -p spine-core --features quic

# io_uring in the transport crate (Linux only)
cargo build --release -p spine-transport --features io-uring

# Research hooks in the daemon (cluster node, human transpiler)
cargo build --release -p spine-core --features research

# The experimental crates are a separate workspace
cd research && cargo build
```
