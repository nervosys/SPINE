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
- `research` (default): research code that needs `spine-neural` and
  `spine-crypto`: `ChameleonKey`, the speculative predictor, protocol
  morphing, `ProtocolHandler`, `SpineConnection`, the protocol evolution
  framework and `TitansLatentCodec`. Build with `--no-default-features` to get
  the wire types, the wire codec (`wire`), `Message`, `BrowserCommand` and the
  task types without either research crate.

## Workspace dependencies

- `spine-crypto` and `spine-neural`, only with the `research` feature

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
