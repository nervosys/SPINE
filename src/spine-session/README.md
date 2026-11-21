# spine-session

Authenticated, forward-secret session handshake for the SPINE mesh (ML-KEM-768 + Ed25519 + AES-256-GCM)

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

The construction has not had external cryptographic review; see the
`handshake` module documentation for what it does and does not claim.

## Documentation

API documentation: <https://docs.rs/spine-session>

## Workspace dependencies

None.

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
