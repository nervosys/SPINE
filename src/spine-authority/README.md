# spine-authority

Capability grants for SPINE: layer 3 (`docs/spec/03-authority.md`): grants and
chains, signed revocation records with a signed feed head, signed receipts, and
an atomic in-memory usage ledger.

A signed `Grant` says that a party may act on some resources, with a ceiling on
effect, a validity window and optional limits. A `Chain` of grants delegates
from a root the verifier trusts, and each link can only narrow its parent. An
`Invocation` is a signed request; `verify` runs the eleven steps of the spec in
order and returns either a `Decision` or the first `Denial`.

The crate is pure: no I/O, no tokio, no clock (time is a parameter), no
randomness. Cryptography is Ed25519 (`verify_strict`) and SHA-256 only.
Revocation and usage state come through the traits `RevocationView` and
`UsageLedger`; the defaults (`NoRevocationData`, `NoLedger`) refuse.
`spine-core` has a node-side gate built on it (`spine_core::authority`), not yet
enforced because the wire has no carrier for a grant chain (see "Node
enforcement" in `docs/spec/03-authority.md`). The mesh, SDK and CLI do not use
it, and it has had no external review.

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

## Revocation, receipts and the ledger

All three follow the grant conventions: deterministic CBOR, `{body, sig}`,
byte strings for keys, ids and digests, strict Ed25519, a domain string ending
in NUL.

| Object | Domain | Body keys |
| --- | --- | --- |
| `Revocation` | `spine-authority-revocation-v1` | `v`, `revoker` (bytes 32), `grant` (bytes 32), `revoked_at` (uint), `nonce` (bytes 16) |
| `FeedHead` | `spine-authority-feed-head-v1` | `v`, `publisher` (bytes 32), `seq`, `as_of`, `count` (uint), `digest` (bytes 32) |
| `Receipt` | `spine-authority-receipt-v1` | `v`, `recipient`, `invocation`, `leaf`, `allowed` (bool), `effect` (text or null), `step` (uint or null), `calls`, `cost`, `bytes`, `result` (bytes 32), `time` |

Ids: a revocation's id is SHA-256 over `spine-authority-revocation-id-v1` and
its encoding; an invocation's id (`Invocation::id`, cited by receipts) over
`spine-authority-invocation-id-v1` and its encoding. A head's digest is SHA-256
over `spine-authority-feed-digest-v1` and the sorted revocation ids.

* **Who may revoke.** The issuer of the grant, or of any grant above it in the
  chain. The record names only the grant id and the revoker, so the right is
  checked at step 8 against the chain being verified
  (`Revocation::is_authorized_by`); `RevocationView::chain_revoked` is the hook,
  and `RevocationFeed` implements it. Records by anyone else are ignored for
  that chain.
* **Feed.** `RevocationFeed::load(records, head, check)` takes the trusted
  publisher, the last accepted sequence and the clock in `FeedCheck`. It accepts
  only a set that matches the head's count and digest, from the trusted
  publisher, with a sequence above the last seen and an `as_of` not in the
  future. A feed is immutable; one that has not loaded is no data, and step 8
  refuses it. Freshness (`as_of`) is compared with the per-effect-class
  `RevocationBounds`, default 0. The head detects truncation, padding and a
  mismatched or replayed head (given the last-seen sequence). It does not detect
  a publisher that omits a record from every head, split views, or a rollback
  when the caller has no last-seen sequence. Nothing here distributes feeds.
* **Receipt.** `Receipt::issue(signer, invocation, verdict, result, time)`
  records a verdict of `verify`: allowed (effect, one call, cost, bytes, result
  digest) or denied (the step number only; amounts and result are zero).
  `verify(expected_recipient)` checks form, recipient and signature;
  `matches(invocation, verdict)` checks the receipt is for that invocation and
  records exactly that outcome.
* **Ledger.** `MemoryLedger::charge(&Decision)` is atomic: it checks the nonce
  and the limits of every limited grant of the chain first and then records the
  nonce and charges them all, or changes nothing. The rate window is per second
  over the 60 seconds before the decision's `now`. Bounds (`LedgerConfig`):
  65536 nonces, 16384 grant records. A nonce is kept until the decision's
  `nonce_horizon` (`now + skew + max invocation age`) and only then forgotten, so
  eviction never admits a replay step 9 would accept; when the table is full of
  live nonces the charge is refused (`LedgerFull`). A usage record is dropped
  once its grant has expired. Grants without limits are not tracked.

## Bounds

Chain of at most 8 grants; encoded grant and invocation at most 4096 bytes; at
most 16 resource patterns, 4 actions, 8 caveats; caveat name 64 bytes, caveat
argument 256 bytes, role 16 bytes; at most 16 path segments of at most 128
bytes; at most 16 resources per invocation; clock skew clamped to 300 seconds;
revocation, feed head and receipt at most 512 bytes each; a feed at most 16384
revocations.

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
`scripts/authority_v1.py` (standard-library Python with its own Ed25519, a
reference implementation of the steps, of revocation, feed loading, receipts and
a model of the ledger) and checked against this crate by `tests/vectors.rs`.

## Provisional decisions

Choices the draft left open are listed, with alternatives, in
`docs/spec/03-authority.md`. None of the spec's open questions is resolved.
