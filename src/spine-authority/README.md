# spine-authority

Capability grants for SPINE: part one of layer 3 (`docs/spec/03-authority.md`).

A signed `Grant` says that a party may act on some resources, with a ceiling on
effect, a validity window and optional limits. A `Chain` of grants delegates
from a root the verifier trusts, and each link can only narrow its parent. An
`Invocation` is a signed request; `verify` runs the eleven steps of the spec in
order and returns either a `Decision` or the first `Denial`.

The crate is pure: no I/O, no tokio, no clock (time is a parameter), no
randomness. Cryptography is Ed25519 (`verify_strict`) and SHA-256 only.
Revocation and usage state come through the traits `RevocationView` and
`UsageLedger`; the defaults (`NoRevocationData`, `NoLedger`) refuse. It is not
wired into the node, mesh, SDK or CLI, and it has had no external review.

## Encoding

Deterministic CBOR from `spine-cbor`. Keys, hashes, ids, nonces and signatures
are byte strings. Signed bytes are `"spine-authority-grant-v1\0"` followed by
the CBOR of the body; the grant id is SHA-256 over
`"spine-authority-grant-id-v1\0"` and the whole encoded grant
(`{"body", "sig"}`). Invocations use `"spine-authority-invocation-v1\0"`.
Decoding rejects non-deterministic input, unknown or missing fields, wrong
types and lengths, and anything that does not re-encode to the same bytes.

Grant body: `v` (uint, 1), `iss` (bytes 32), `aud` (bytes 32), `parent` (null
or bytes 32), `role` (text), `res` (array of `{owner: bytes 32, path: [text],
prefix: bool}`), `act` (array of text, strictly ascending), `effect` (text),
`delegable` (uint), `nbf`, `exp` (uint), `limits` (`{calls, cost, bytes, rate}`,
each uint or null), `caveats` (array of `{name: text, arg: bytes}`), `nonce`
(bytes 16).

## Bounds

Chain of at most 8 grants; encoded grant and invocation at most 4096 bytes; at
most 16 resource patterns, 4 actions, 8 caveats; caveat name 64 bytes, caveat
argument 256 bytes, role 16 bytes; at most 16 path segments of at most 128
bytes; at most 16 resources per invocation; clock skew clamped to 300 seconds.

## Scope rule

A resource is `(owner key, path segments)`. A pattern is exact or a prefix.
Containment is segment by segment, so `/data` contains `/data` and `/data/x`
but never `/database`. A child pattern is covered by a parent pattern when the
owners match and the parent is a prefix leading the child's segments, or both
are the same exact pattern.

## Verification steps

1. Bounds on chain and invocation. 2. Shape, version, caveats. 3. Root is
trusted and owns the resources. 4. Chain links. 5. Grant signatures. 6.
Attenuation. 7. Validity windows. 8. Revocation and its staleness. 9.
Invocation checks, signature, nonce. 10. Resource, action, effect. 11. Limits.

`verify` only reads state; the caller records the nonce and charges from the
`Decision` atomically with performing the operation. `Grant::issue_root` and
`Grant::delegate` refuse to sign a grant that would fail step 6.

## Vectors

`docs/spec/03-authority.vectors.json` is generated and checked by
`scripts/authority_v1.py` (standard-library Python with its own Ed25519 and a
reference implementation of the steps) and checked against this crate by
`tests/vectors.rs`.

## Provisional decisions

Choices the draft left open are listed, with alternatives, in
`docs/spec/03-authority.md`. None of the spec's open questions is resolved.
