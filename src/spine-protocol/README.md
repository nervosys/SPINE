# spine-protocol

Low-latency communication protocols for SPINE

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

## Documentation

API documentation: <https://docs.rs/spine-protocol>

## Features

- `quic` (default): the QUIC transport (`QuicEndpointBuilder`, `QuicTransport`).
- `research` (off by default): research code that needs `spine-neural` and
  `spine-crypto`: `ChameleonKey` (and `ProtocolHandler::enable_chameleon` and
  `enable_chameleon_aead`), `SpineConnection`,
  `connect_auto`, the protocol evolution framework and `TitansLatentCodec`.
  Enable it with `spine-protocol = { version = "2", features = ["research"] }`.
  Without it you get the wire types, the wire codec (`wire`), `Message`,
  `BrowserCommand`, the task types and `ProtocolHandler` with neither research
  crate in the tree. Through 2.0.2 this feature was on by default.

## Workspace dependencies

- `spine-crypto` and `spine-neural`, only with the `research` feature

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
