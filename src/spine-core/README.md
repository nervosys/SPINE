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

## Modules

The library half exports `config`, `connections`, `operations`, `outbound`,
`resources`, `session_store`, `tasks`, `tls`, `tools` and `authority`.

`authority` is the node-side enforcement of layer 3
(`docs/spec/03-authority.md`, "Node enforcement"): a classification of every
command, a gate that verifies a grant chain and an invocation against the
command being run and charges a usage ledger, and the single-user root
grant. It is unit-tested and not connected: spine-wire v1 cannot carry a
grant chain, the daemon does not call the gate, and
`authority.mode = "required"` is refused at startup.

## Workspace dependencies

- `spine-authority`
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
