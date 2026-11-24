# spine-crypto

Research cryptography for SPINE: Titans message prediction and NTRU-inspired key evolution

Research only and unreviewed: none of it is on the native wire (spine-wire v1),
which relies on TLS, and the mesh session handshake lives in `spine-session`.
Do not use it to protect data.

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

## Documentation

API documentation: <https://docs.rs/spine-crypto>

## Workspace dependencies

- `spine-neural`
- `spine-session`

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
