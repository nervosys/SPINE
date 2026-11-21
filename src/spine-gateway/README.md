# spine-gateway

OpenAPI REST gateway for SPINE — exposes agentic web APIs to non-Rust clients

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

## Documentation

API documentation: <https://docs.rs/spine-gateway>

## Features

- `research` (off by default): serves `POST /v1/embeddings` and
  `GET /v1/agentic/codecs` from a codec registry pre-populated with the Titans
  neural codec (`spine:codec/titans/v1@dim=256,dtype=f32`, 256-dim f32), which is
  the default `model` of `/v1/embeddings`. The codec comes from the optional
  `research/spine-protocol-research` dependency (which pulls in `spine-neural` and
  `spine-crypto`). Without it both routes stay registered and answer
  `503 Service Unavailable` with a message to rebuild with the `research` feature.
- `fips`: build against the FIPS-shaped rustls provider stack.

## Workspace dependencies

- `spine-agent`
- `spine-compiler`
- `spine-core`
- `spine-parser`
- `spine-protocol`
- `spine-transport`

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
