# spine-mesh

The SPINE agent mesh: Ed25519 identities, signed envelopes, signed name records, and authenticated TCP/WebSocket transports

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

The QUIC mesh transport and the signed-frame codec live here too. They use
`spine-protocol` without its `research` feature, so `spine-neural` and
`spine-crypto` are not in this crate's dependency tree. `spine-agentic`
re-exports this crate's modules and `AgentId`.

## Documentation

API documentation: <https://docs.rs/spine-mesh>

## Workspace dependencies

- `spine-name`
- `spine-session`
- `spine-transport`

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
