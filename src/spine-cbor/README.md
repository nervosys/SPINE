# spine-cbor

Deterministic CBOR (RFC 8949 section 4.2 core deterministic encoding, no tags,
nesting depth at most 32) for SPINE: a validating scanner, `to_vec` and
`from_slice`. It is the one canonical encoding used by the native wire and by
signed structures. It depends only on `ciborium`, `serde` and `thiserror`, so
crates that must stay light can use it without pulling in the protocol stack.

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme).

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
