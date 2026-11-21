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
There is no `research` feature. The crate has no dependency on any crate under
`research/`. `ProtocolHandler` writes the `SP` body, a compression flag byte and
the fixed 5-byte frame; it has no sealing (confidentiality comes from TLS or the
session layer). The Chameleon latent-space key, `SpineConnection`, `connect_auto`,
the protocol evolution framework, `TitansLatentCodec` and the pre-shared-key AES
layer are not part of this crate: the first four are in the experimental,
unreviewed `research/spine-protocol-research` crate and the AES layer was
removed. `LatentVector` and the neural-codec types (`EncodedFrame`,
`NeuralCodec`, `CodecRegistry`, `EmbeddingRequest`, ...) moved to the same research
crate with the `Message` variants that carried them.

## Workspace dependencies

None. The `research/` crates `spine-nostd` and `spine-embedded` were dev
dependencies of the integration tests; those tests moved to
`research/spine-protocol-research`.

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
