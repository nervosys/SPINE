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

- `research` (off by default): pre-registers the Titans neural codec
  (`spine:codec/titans/v1@dim=256,dtype=f32`, 256-dim f32) in the codec
  registry and makes it the default `model` of `POST /v1/embeddings`. It
  enables `spine-protocol/research`, which pulls in `spine-neural` and
  `spine-crypto`. Without it the registry starts empty:
  `GET /v1/agentic/codecs` returns an empty list and `POST /v1/embeddings`
  answers `503 no codecs registered` until the deployment registers a codec
  on `agentic_sse::registry()`.
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
