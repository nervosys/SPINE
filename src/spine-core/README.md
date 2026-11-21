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

- `research` (off by default): cluster swarm and its event listener, the
  latent, morph and neural-transmit commands (`GetLatentUR`, `Morph` in the
  dispatcher, `NeuralTransmit`), `TransferSession`, `GetAgenticState`, the
  autonomous agent loop and the human transpile on `Navigate`. Without it the
  daemon answers those commands with `unsupported_without_research`, `Search`
  is local only (`"distributed": false`, `"status": "local_only"`), and
  `spine-agentic`, `spine-cluster`, `spine-human` and `spine-neural` are not
  built. Build with `cargo build -p spine-core --features research`.
- `quic`: QUIC listener support.

## Workspace dependencies

- `spine-agent`
- `spine-agentic` (with `research`)
- `spine-cluster` (with `research`)
- `spine-compiler`
- `spine-human` (with `research`)
- `spine-name`
- `spine-neural` (with `research`)
- `spine-parser`
- `spine-protocol`
- `spine-transport`
- `spine-wasm`

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
