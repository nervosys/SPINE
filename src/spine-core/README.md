# spine-core

Core engine for the SPINE bioinspired agentic browser

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

## Documentation

API documentation: <https://docs.rs/spine-core>

## Features

- `research` (off by default): the cluster node and its event listener
  (session data, search, knowledge sync) and the human transpile on
  `Navigate`. Without it the daemon has no cluster and `Navigate` gives the
  session an empty binary; `spine-cluster` and `spine-human` are not built.
  The research-only commands (latent encoding, session transfer, neural
  transmit, agentic state, autonomous mode) are not part of the core command
  set; they are recorded in `research/spine-protocol-research/README.md`.
  Build with `cargo build -p spine-core --features research`.
- `quic`: QUIC listener support.

## Workspace dependencies

- `spine-cluster` (with `research`)
- `spine-human` (with `research`)
- `spine-mesh`
- `spine-name`
- `spine-parser`
- `spine-protocol`
- `spine-storage`
- `spine-transport`
- `spine-wasm`

Dev-dependency: `spine-agent`.

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
