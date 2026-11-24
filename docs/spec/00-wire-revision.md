# spine-wire v1 revision: a draft proposal

Status: DRAFT, NOT NORMATIVE. A proposal for the project owner to accept, amend or reject. Nothing in it is implemented, and nothing in `00-wire.md`, its vectors, the code or `COMPLETION_PLAN.md` was changed by writing it. If it is accepted, its proposed texts replace or extend the sections of `00-wire.md` named in A.2, and this file is then deleted or reduced to a history note. Written against `completion/agent-web` at `b8911fb`. Nothing here was compiled or run. Every claim about the current protocol or code cites a file and a function or section that was read; a claim that was not confirmed by reading is marked "unverified". Citations are `file` plus `function`, relative to `src/spine-protocol/src` unless a crate or a document is named; line numbers are not used.

## A. Status and scope

### A.1 What the revision is for

`00-wire.md` 1.4 says: "There is no field for a signature, a capability or a nonce, and adding one is a new version." Consequences recorded elsewhere:

- Authority. `03-authority.md` specifies grants, invocations, revocation and receipts. `src/spine-authority` implements them. `src/spine-core/src/authority.rs` (`AuthorityGate::authorize`, `classify`) is a node gate that the daemon does not call, because a request cannot carry a chain or an invocation, the handshake has no client key and there is no per-connection value to bind an invocation to (`03-authority.md`, "Node enforcement"; `config.rs` `AuthorityConfig::validate` refuses `mode = "required"`).
- Names. `NameResolution::Resolved { record, provenance }` (`lib.rs`) carries no succession log, so a client cannot verify a rotated name through the daemon, and there is no outcome for a retired, a compromised (equivocated) or a revoked name (`02-names.md` 10.11, "Remains", items 2 and 7).
- Objects. `ResourceStore::delete(owner, hash)` exists (`spine-core` `resources.rs`) and `04-objects.md` section 4.4 says it "is not on the wire". Whether a knowledge delete exists in the store API is unverified (A.5).
- Smaller items: `store_knowledge` refusals reuse `execution_failed` (`00-wire.md` 13); idempotency keys, dry-run, cancel and per-client quota are recorded in `COMPLETION_PLAN.md` Stage 9 ("Idempotency keys, dry-run, and resume after disconnect") and Stage 8.

### A.2 What the revision would change in 00-wire.md

If accepted, these places of `00-wire.md` change. Each rule that is weakened or removed is quoted in the section that changes it.

| Section of 00-wire.md | Change | Where proposed |
| --- | --- | --- |
| 1.4, first paragraph | the sentence quoted in A.1 is replaced | C1, C3 |
| 3 (frame types) | three new types: `IDENTIFY` 0x06, `AUTH_REQUEST` 0x12, `RECEIPT` 0x13; the unallocated list shrinks | C1, C3, C5 |
| 5.2 rule 9, 6.3 | `HELLO_ACK` keeps its open-map rule but gains three keys every revised peer MUST send and MUST require | C1, C2 |
| 6.1 | one frame, `IDENTIFY`, may follow `HELLO_ACK` | C1 |
| 7.2 last paragraph | "`capability_query` is not limited" changes | C3.6 |
| 7.3 (`protocol_violation`), 7.4 | new state rules, new error codes | C1, C4 |
| 8.2, 8.3 | direction table, receipt ordering | C3, C5 |
| 12.1, 12.2, 12.4 | 21 commands instead of 19; the `AUTH_REQUEST` body; `NameResolution` | C3, C6, C7 |
| 13 | 64 codes instead of 47; the `execution_failed` row loses three conditions | C4 |
| 14, 15.4, Appendix C (G8), D.1 | limits, vectors, the G8 answer | D |

While reading `00-wire.md` for this proposal one inconsistency was found that is outside the revision: section 14's row "Requests in flight | 1 | `native.rs` `local_config`; not configurable | `overloaded` (unreachable)" contradicts 6.4, 8.3 and section 16 item 9, which say 8 (`native.rs` `DAEMON_MAX_INFLIGHT`). It should be corrected whatever is decided here.

### A.3 In place or version 2

The question: revise v1 in place (the version byte stays 1, `HELLO.versions` stays `[1]`), or open version 2.

Premise: nothing has been released. `00-wire.md` states "Normative for the reference implementation" and describes itself as not externally reviewed; `COMPLETION_PLAN.md` Stage 13 treats the release version as undecided. Whether any third party has built a peer from `00-wire.md` and the vectors is unknown to the author of this draft.

Option 1, revise in place.

- Spec: sections change as in A.2; the history stays in git. No second text to maintain.
- Vectors: `00-wire.vectors.json` changes. Every existing `handshakes` entry changes, because `HELLO_ACK` gains keys (the acknowledgement bytes differ; `scripts/wire_v1.py` `hello_ack_body`, `negotiate`, `make_handshakes` change and `_local` gains the new per-connection values as inputs so the vectors stay deterministic). The frame, body and CBOR vectors are unchanged.
- Version negotiation in HELLO: unchanged. A revised client against an unrevised daemon sees an acknowledgement without `binding` and fails with `protocol_violation` (C2 makes the keys required), which is fail-closed and not a typed `unsupported_version`. An unrevised client against a revised daemon works in `off` and `audit` (it ignores the new acknowledgement keys, 5.2 rule 9) and is refused with `authority_required` in `required`; it cannot be confused into believing it is authorised.
- Code: one codec, one `VERSION` (`frame.rs`), one `FrameType` enum; `connection.rs` `Tracker` gains states.

Option 2, version 2.

- Spec: a second document or a second half of this one, with v1 frozen. Section 2.2 ("`version` ... MUST be 1") becomes version-dependent.
- Vectors: v1 vectors stay valid unchanged and act as a regression suite; a v2 file is added; the checker gains a version switch.
- Version negotiation in HELLO: `versions` becomes `[1, 2]`; a v2 server selects 2; an old client gets `unsupported_version` with `supported` (`hello.rs` `negotiate`), a typed answer. The frame header's version byte differs per connection, so `frame.rs` `parse_header`, `Connection`, the SDK, the Go, FFI and Python bindings (unverified: not read) and the gateway all become two-version.
- Code: two codecs or a parameterised one, for a protocol that so far has one implementation and a Python reference.

Option 3, authority as an extension (considered and rejected). Section 11 of `00-wire.md` provides negotiated extensions with `EXT` frames. It says "The core command set and the frame types are closed; an extension cannot add a variant to them." So an extension could not add the 2 commands of C7 or the outcomes of C6 to the core, and an authority carrier made of `EXT` frames would need a pairing rule between an `EXT` frame and the `REQUEST` it authorises that `Connection` (`connection.rs` `Tracker`, `ext_index`, `check_ext`) does not have, since it does not track `EXT` frames at all. The extension route is kept for later optimisations (C8).

Recommendation: revise in place.

Reasoning. The cost of version 2 is two codecs, two vector sets and a typed downgrade path for a population that, on the stated premise, is empty. The benefit of a typed `unsupported_version` is real but small, since an unrevised daemon already closes a revised client at the handshake. Every change here is either a closed-set growth (frame types, commands, codes, outcome variants, which an old peer already refuses fail-closed because the sets are closed, 5.2 rule 8, 7.2 step 3, 7.4) or a new required key in an open map.

Strongest argument against: "v1" would then name two incompatible behaviours with the same version byte and the same `HELLO`, and nothing on the wire says which one a peer speaks, except by failing. Any copy of today's `00-wire.md` that someone built against (the document is cited by source comments and other documents, Appendix D) becomes wrong without a signal. In-place revision also gives up the clean statement "1.4 promises a new version".

What would make v2 the better choice:

- any peer, binding or document built from the current `00-wire.md` exists outside this repository (including the Go, FFI and Python bindings if they were published);
- the owner also wants to fix a deviation of section 16 that is a breaking change, for example item 3 (hashes as integer arrays inside JSON-shaped records) or item 8; bundling those with this revision under one version number is then clean and mixing them into v1 would not be;
- the owner wants a frozen v1 as the conformance baseline for independent implementers;
- the project gets an external review of v1 as it stands, which makes the reviewed text worth freezing.

If v2 is chosen, everything in section C applies unchanged except that the new keys of `HELLO_ACK` can be required by the version rather than by a rule, and section D gains a codec stage. The recommendation costs nothing to reverse before a release.

### A.4 What this document is not

It specifies no cryptography beyond Ed25519 signatures and SHA-256 that `src/spine-authority` and `spine-name` already use. It does not depend on the planned Noise session layer (`COMPLETION_PLAN.md` Stage 8, not built, not reviewed). It does not change layer 3 (`03-authority.md`) or layer 2 (`02-names.md`): it carries their objects.

### A.5 Things this draft did not read

Marked unverified wherever they matter: the Go, FFI and Python bindings, the gateway and the browser extension (they use the SDK according to `00-wire.md` 1.3); whether the store API has a per-entry knowledge delete (`spine-core` `knowledge.rs` has none; `main.rs` `BrowserState` releases knowledge only when a session ends, `knowledge_base.remove(id)` with `knowledge_budget.release`); whether the daemon has a node signing key when no `[namespace]` is configured (C3.7).

## B. Design principles

These are derived from rules `00-wire.md` already states.

1. Closed sets. Frame types (3), commands (12), error codes (13), close codes (6.5) and every struct (5.2 rule 8) are closed; a revision grows a set by listing the new member and its failure behaviour. No new member is open-ended.
2. Deterministic encoding. Every new body is one deterministic CBOR item (5.1). Every new body is decoded with `native.rs` `decode_strict` (decode, re-encode, compare) so there is one encoding per value.
3. Fail closed. An unknown frame type is `unknown_frame_type` (7.2 step 3); an unknown command is `unknown_command` (7.4); an unknown field in a closed struct is a schema error. A new feature that a peer lacks is therefore refused, not ignored.
4. Bounded everything. Each new field has a type, a length or range, and one failure code (the tables in C).
5. No field whose absence changes meaning silently. Where a field may be absent, the text says what absence means and the answer to "does a client need to know" is yes or the field is not optional. C6 makes the log in a resolve answer always present (an empty list says "this node holds none") instead of optional.
6. Opaque carriage of layer-3 objects. The carrier treats a grant and an invocation as byte strings whose canonical encodings, versions and bounds belong to layer 3 (`spine-authority` `Grant::decode`, `Invocation::decode`). A new grant or invocation version (they carry `v`: `GRANT_VERSION`, `INVOCATION_VERSION`) needs no wire change. This is the structural half of "data cannot grant" (`03-authority.md`): grants travel in their own frame type, in a field no layer-4 object has.
7. No second encoding. A grant, invocation or receipt is carried as `spine-authority` encodes it; a succession log is carried in the JSON form that `PublishName` already accepts (`main.rs` `publication_log`, `02-names.md` 10.1).

How a revised v1 stays closed, concretely:

- The unknown-key rule is unchanged where it stands today. `HELLO` and `HELLO_ACK` are open (5.2 rule 9; `hello.rs` `Hello`, `HelloAck` have no `deny_unknown_fields`); `CLOSE`, `RESPONSE.err`, every command and every type of 12.3 are closed. The three new bodies are closed: `IDENTIFY` (C1.3), `AUTH_REQUEST` and its `auth` member (C3.2) and the outcome payloads of C6. `RECEIPT` is the `spine-authority` `Receipt` object, which is itself closed (`03-authority.md`, "part two": "closed maps (an unknown or missing key is refused)").
- `HELLO_ACK` stays open, so a later revision can add keys, but the three keys of this revision are required of revised peers, and a revised client that does not find them closes with `protocol_violation`. `HELLO` gets no new key at all, so it stays exactly as validated today.
- The new required keys are exact: name, type, size and range are in C1.2.

## C. The changes

Notation: "proposed text" is meant to be pasted into `00-wire.md` with section numbers adjusted. Key order in examples is the deterministic order of 5.1 rule 3 (encoded keys ascending bytewise, so shorter text keys first, then bytewise).

### C1. Client identity in the handshake

#### C1.1 What is proposed

A client MAY present an Ed25519 public key and a proof that it holds the matching private key, once, right after `HELLO_ACK`, in a new client-to-server frame `IDENTIFY` (type `0x06`, request id 0). The signed message contains the exact bytes of the two handshake bodies, a server-chosen value (`binding`, C2) and a channel-binding input that is empty today.

A one-round handshake cannot carry the proof, because the client signs over a value the server chooses; a third frame is the smallest change. `HELLO` is left untouched.

#### C1.2 New HELLO_ACK keys (shared with C2)

Proposed text for 6.3, rows added:

```
| `binding`   | bytes, exactly 32 | no | A value chosen by the server from a cryptographic random source for this connection (C2). |
| `authority` | text              | no | The node's authority mode: `"off"`, `"audit"` or `"required"`. Any other text is `protocol_violation` at the client. |
| `time`      | uint (u64)        | no | The server's clock, Unix seconds, when it built the acknowledgement. Not authenticated; see C2.3. |
```

A client MUST treat as `protocol_violation`: a missing key, a mistyped key, a `binding` not of 32 bytes, an `authority` not one of the three texts (`HelloAck::validate`). Deterministic order of the acknowledgement's keys becomes: `time`, `agent`, `binding`, `version`, `authority`, `max_frame`, `extensions`, `max_inflight`.

Why `authority` is disclosed to an unauthenticated peer: a client that holds a chain must know before its first request whether the node will check it (C3.4), and an operator's mode is not a secret worth a round trip of ambiguity. Why `time`: an invocation carries a `time` that the node checks against its own clock (`verification.rs` `check_invocation`, step 9, `max_invocation_age_secs` default 60, `GateSettings`), and a client with a clock error larger than that would fail with `authority_step_9` and could not tell why, since the refusal names only the step (P17, P31).

#### C1.3 IDENTIFY

Proposed text for section 3, one row: `0x06`, `IDENTIFY`, sender: client, request id `0`, body: `{key, sig, channel}`.

Body, a closed map; keys in deterministic order:

| Key | Type | Rule |
| --- | --- | --- |
| `key` | bytes, exactly 32 | The client's Ed25519 public key. |
| `sig` | bytes, exactly 64 | Ed25519 signature over the message `M` below, by `key`. |
| `channel` | bytes, 0 or 32 | The channel-binding input. In this revision it MUST be empty (zero length) and a server MUST refuse a non-empty one. Section C1.5 says what it will mean. |

The signed message `M` (all concatenated; `||` is concatenation; `u8(n)` is one byte):

```
M = "spine-wire-identify-v1" || 0x00
 || SHA-256(H)               ; H = the body bytes of the HELLO frame, exactly as sent
 || SHA-256(A)               ; A = the body bytes of the HELLO_ACK frame, exactly as sent
 || u8(len(channel)) || channel
 || key
```

`H` and `A` are deterministic CBOR (5.1), so both sides hold identical bytes without agreeing on anything further; a compressed frame cannot occur here (6.1: both frames are never compressed). `A` contains `binding` and `time`, so `M` is unique to this connection. The signature is Ed25519 as `spine-authority` and `spine-name` use it: strict verification (RFC 8032, rejecting S >= L and a small-order key or R, `02-names.md` 10.3 step 7; `ed25519_dalek` `verify_strict`, as in `invocation.rs` `Invocation::verify_signature`). The domain string has a NUL terminator like every layer-3 domain and is distinct from every domain in `03-authority.md` and `02-names.md`.

Size: a frame carrying an `IDENTIFY` with an empty `channel` has a 118-byte body (map head 1, `key` 4 plus 34, `sig` 4 plus 66, `channel` 8 plus 1) and a total of 133 bytes (`length` field 0x81).

Rules, proposed text for 6.1 and 8.2:

1. A client MAY send one `IDENTIFY`, as the first frame after it has read `HELLO_ACK`, and not later: a server that has received a `REQUEST`, `AUTH_REQUEST`, `STREAM_CANCEL`, `EXT` or an earlier `IDENTIFY` on the connection treats an `IDENTIFY` as `protocol_violation`. A `PING` or `PONG` does not close the window. The server processes frames in arrival order, so a client may pipeline `IDENTIFY` and the first request.
2. The server verifies the frame before it processes any later frame. Any failure (body not a closed map of the three keys, a length rule broken, `channel` not equal to what the server expects, the signature does not verify) is `CLOSE(protocol_violation)`. No new close code: a bad proof is a malformed handshake step, and 7.3's `protocol_violation` row already covers "a `HELLO`, `HELLO_ACK` or `CLOSE` body that is deterministic CBOR but does not fit its type". The row is extended to say "or an `IDENTIFY`".
3. On success the connection has an identity: the 32-byte `key`, fixed for the life of the connection. There is no answer frame. An `IDENTIFY` is accepted in every authority mode.
4. A server that has a connection identity and receives an `AUTH_REQUEST` whose invocation names another `invoker` refuses it with `authority_step_9` (C3.5).
5. `IDENTIFY` carries request id 0 (`frame.rs` `check_request_id`: the new type is added to the set that MUST carry 0). Client to server only (8.2).

#### C1.4 Anonymous clients and the authority modes

A client that sends no `IDENTIFY` is anonymous. What a node does maps to the configuration key `[authority] mode`, which today is `off` or `required` with `required` refused at startup (`config.rs` `AuthorityMode`, `AuthorityConfig::validate`). This revision adds `audit`. `HELLO_ACK.authority` reports it.

| Mode | A plain `REQUEST` | An `AUTH_REQUEST` | Identity |
| --- | --- | --- | --- |
| `off` | Runs, as today. | Answered `authority_not_enabled`; the command does not run. | Accepted and recorded in the node's log only. |
| `audit` | Runs. The node logs the command, the effect class (`authority.rs` `classify`), and that no authority was presented. | The gate runs (`AuthorityGate::authorize`, C3.6). The command runs whether or not the gate authorised it. The node logs the verdict. A refusal does not change the response and does not reveal the verdict to the client. The charge and the nonce are recorded exactly as in `required`, so limits and replay behave as they will. | Accepted, and the node attributes commands to it in its log. |
| `required` | Answered `authority_required`; the command does not run. No command is exempt, `capability_query` included (`03-authority.md`, "Command classification": the gate classifies all 19 commands, `capability_query` as `read` on `node/tools`; there is no "unauthenticated discovery"). | The gate runs; a refusal is `authority_step_N` (C4); the command runs only if the gate authorised it. | Accepted. |

Why `AUTH_REQUEST` is refused in `off` and not ignored: a client that attached a chain to a request would otherwise believe its limits and its identity applied. The client can see the mode before it sends anything (C1.2), so this is a refusal of a mistake, not of a normal case.

Why `audit` runs the command regardless: its purpose is to show the operator which commands `required` would refuse before it is turned on. An `audit` that blocks is `required`.

#### C1.5 The channel-binding input

What `channel` means is defined when a security layer under the wire defines a value both ends can compute. Today there is none the wire can use: TLS is optional and the SDK and the daemon handle the stream generically (`Connection<S: AsyncRead + AsyncWrite>`); the planned layer-1 session handshake (`01-session.md`, `COMPLETION_PLAN.md` Stage 8) has not been built or reviewed, and `01-session.md` 3 records that "Nothing in the repository derives one from the other (no exporter, no channel binding...)". Therefore:

- This revision defines `channel` as a field of the signed message and of the body whose only legal value is the empty byte string.
- A later document that defines a security layer MUST define, for that layer, a 32-byte value that both ends derive, and MUST state that an `IDENTIFY` on a connection over that layer carries it (32 bytes) and that a server over that layer refuses an empty `channel`. The rule in C1.3 then reads "equal to what the server expects", which is empty when no layer defines a value and that value when one does. Adding the layer needs a specification of the value and a new check in the server and no change to the frame, the body, the signed message layout or any vector other than new ones.
- Until then a connection without channel binding gets the weaker property of C1.6.

#### C1.6 What the proof does and does not prove

Proves: at some moment after the server sent this exact acknowledgement (which contains 32 random bytes the server chose and, before that, the exact client `HELLO`), some party that holds the private key of `key` signed `M`. Because `M` covers `SHA-256(A)` and `A` holds `binding`, a signature made for another connection does not verify on this one. The proof is not replayable across connections.

Does not prove:

- That the party that signed is the party that is sending the frames on this stream. Without channel binding, an active relay that sits between the client and the node can forward the client's `HELLO`, the node's acknowledgement and the client's `IDENTIFY` unchanged, and then holds a connection to the node that the node attributes to the client's key. The relay cannot make an `AUTH_REQUEST` that the node accepts, because the invocation is signed by the invoker's key over `binding`, the recipient and a hash of the command (C3.5), but it can have commands run in audit mode attributed to the client, relay the client's own authorised requests selectively, delay or drop them, and see everything in plaintext. Therefore a node MUST NOT treat the connection identity as authority: authority comes only from a verified invocation. C1.5 is what closes this, for a layer that provides the input.
- Who the node is. The acknowledgement carries no node key. A client learns the node's key from its chain (the root grant's issuer, `03-authority.md` P5), from a layer-2 name or by configuration, and the invocation's `recipient` field makes an invocation useless at any other node. A relay that is a different node gets a client `IDENTIFY` for its own acknowledgement and cannot use it at the real node.
- Anything about the key: that it is unique, human, fresh, or not minted a moment ago (key minting is free, F).

Why bother, given that the invocation signature also proves possession of the invoker's key on every request: connection identity is what lets the node (a) attribute a plain request in `audit` mode, (b) apply a per-identity quota and rate limit to the connection (C8, which today keys on a random per-connection lease id, `main.rs` `RateLease`, `check_rate_limit`), and (c) refuse an invocation signed by a key other than the connection's, which closes a confused-deputy path where a connection forwards another party's invocations (`03-authority.md` open question 2). If the owner decides (a) to (c) are not worth a third frame, C1 can be dropped and C2 to C8 stand; open question 3 in E asks.

#### C1.7 Alternatives considered and rejected

- Signing a client-chosen nonce in `HELLO`. Needs no third frame but proves nothing about this connection, since a relay can replay a captured `HELLO`; the server's challenge is the point.
- Putting `key` and `sig` into `HELLO` and the challenge into a first `RESPONSE`. Changes the open `HELLO` that is validated and fixed in size (6.2) and mixes a handshake step into the request path.
- An identity extension (section 11). It would need new frames, which an extension cannot add (11).
- A MAC keyed by a shared secret, or a Diffie-Hellman exchange in the wire. New cryptography, not allowed by A.4.
- Making the signature cover the TLS or QUIC transcript now. Needs an exporter from rustls inside code that is generic over the stream (`Connection<S>`); not verified to be available there, and it still leaves WebSocket and plain TCP without a value. Open question 14.

#### C1.8 Security analysis

Replay across connections: prevented by `binding` in `A`. Replay within a connection: the frame is accepted at most once (rule 1). Downgrade: a peer that does not know `IDENTIFY` closes with `unknown_frame_type`; there is no silent fallback. Key substitution: the signature covers `key`. Cross-protocol signature reuse: the domain string is unique and `M` has fixed-position hashes, so no layer-3 or layer-2 object parses as `M`. Resource use: one signature verification per connection, inside the handshake deadline's neighbourhood; the frame is at most 133 bytes; the server must not do the verification before it has sent the acknowledgement. Timing: nothing about the key is secret on the server side. Weak keys: strict verification refuses small-order keys.

### C2. The session value

#### C2.1 What it is

`HELLO_ACK.binding`: 32 bytes from the server's cryptographic random source, generated per connection after a valid `HELLO` has been read and before the acknowledgement is written. Unpredictable to a peer that has not read the acknowledgement; unique per connection with probability 1 - 2^-128 over 2^64 connections; never reused or derived from anything the client sent (the `HELLO` is not an input, so a client cannot steer it). It appears exactly once on the wire, in the acknowledgement, and in the signed bytes of `IDENTIFY` through `A`.

It is the value that `03-authority.md` P16 calls `session`: "`session` is an opaque 32-byte value supplied by the verifier and signed in the invocation. Layer 1 may define a handshake hash or channel id; the field type may change." It is also the value passed to `AuthorityGate::authorize(..., session: [u8; 32], ...)` (`authority.rs`), where the gate puts it in `VerifyContext::new(now, recipient, session, trusted_roots)`, and which `verification.rs` `check_invocation` compares with `invocation.body.session` (`WrongSession`, step 9).

The name `binding` on the wire avoids a clash: "session" already means the daemon's browser session (`navigate` replies `session_id`) and the layer-1 mesh session (`01-session.md`).

#### C2.2 Rules

1. The server stores `binding` for the life of the connection and passes it to the gate as `session` for every `AUTH_REQUEST` on that connection.
2. A client uses the value it read in this connection's acknowledgement as `body.session` of every invocation it sends on this connection. An invocation made for another connection fails at step 9.
3. Nothing else uses it. In particular it is not a session token; possession of it grants nothing.
4. A reconnect gets a new value. An in-flight request whose connection was lost cannot be resent with the same invocation (its `session` and its single-use nonce are spent or wrong); the client must sign a new invocation, and for a non-idempotent command it cannot know whether the first ran. C5 (receipts) and C8 (idempotency) concern that.

#### C2.3 Clock

The `time` key is the server's clock. A client SHOULD set `body.time` of an invocation to `time` plus the monotonic time elapsed since it read the acknowledgement, not to its own wall clock. This does not remove clock trust: a grant's `nbf` and `exp` are absolute times checked against the node's clock (`verification.rs` step 7), and `time` is unauthenticated, so an active attacker can shift it and cause refusals (a denial of service, not a forgery, because an invocation `time` outside `[now - max_invocation_age_secs, now + skew]` is refused, step 9). It narrows `03-authority.md` open question 7 to grants only.

#### C2.4 Alternatives rejected

- The hash of the handshake bytes (a transcript hash) as the session value: deterministic and client-influenced, so a client can choose its `HELLO` to reuse a value, and a node that has a recorded value could be asked to accept it twice. A server-chosen random value has neither problem. The transcript is bound by `IDENTIFY` instead.
- A counter. Predictable and not unique across restarts.
- No session value, relying on the nonce ledger. The ledger forgets a nonce after `now + skew + max_invocation_age` (`03-authority.md` "Usage ledger"), and a ledger on another node, or one that was lost, does not know the nonce; `binding` makes an invocation single-connection independent of the ledger.

#### C2.5 Security analysis

An eavesdropper on a plaintext connection learns `binding` but cannot sign an invocation. A relay that forwards the acknowledgement to the client makes the client bind invocations to the relayed connection, which is the connection the relay holds to the node; this is the relay of C1.6, not a new one. An attacker who can guess `binding` could pre-sign nothing, since it also lacks the invoker's key. Reuse of an acknowledgement is impossible: the client reads it from the stream it is using.

### C3. The authority carrier

#### C3.1 Decision

Per request. A request that needs authority is a new frame, `AUTH_REQUEST` (type `0x12`), whose body holds the chain, the invocation and the command. The chain is not established once per connection and referenced by a handle.

Reasons:

- State. A handle needs per-connection storage of up to `MAX_CHAIN_BYTES` per handle (8 grants of at most 4096 bytes plus 8: `grant.rs`), a register and a release command or frame, a bound on handles, and a rule for what happens to a handle whose grant is revoked or expires. The gate re-verifies every grant on every invocation anyway (steps 5 to 8 cannot be cached: expiry, revocation and the ledger change), so a handle saves bytes and no cryptography.
- Size. Chain size is bounded by layer 3 at 32,776 bytes, an invocation at 4,096 (`MAX_INVOCATION_BYTES`); so an `AUTH_REQUEST` adds at most about 37 KB. The frame maximum is 16 MiB and `max_inflight` is 8 (`native.rs` `DAEMON_MAX_INFLIGHT`), so the worst case is about 300 KB queued per connection, inside bounds that already exist (a `publish_resource` is up to 8 MiB).
- Closedness. A handle is a second way to reach the same authority and a place for a stale-handle bug. Per request has one path.
- Privacy. `03-authority.md` open question 3: a chain reveals the delegation path to the recipient. Per request discloses the whole chain on every request to the node. This revision accepts that, as `03-authority.md` P10 does ("the full chain is supplied"); a node-side cache of confirmed grants is the alternative P10 names, and it is the optimisation to add later (C8).

#### C3.2 AUTH_REQUEST

Proposed text for section 3: `0x12`, `AUTH_REQUEST`, sender: client, request id: nonzero, not in flight, body: `{auth, command}`. Section 8.3 applies exactly as for `REQUEST` (in-flight table, `max_inflight`, `overloaded`).

Body, a closed map; deterministic key order `auth`, `command`:

| Key | Type | Rule |
| --- | --- | --- |
| `auth` | closed map `{chain, invocation}` | Below. |
| `command` | one command item, as in 12.1 | The command to run, in exactly the form a `REQUEST` body has. |

`auth` keys, in deterministic order: `chain`, `invocation`.

| Key | Type | Bounds | Failure |
| --- | --- | --- | --- |
| `chain` | array of bytes | 1 to 8 items (`MAX_CHAIN_DEPTH`), root first. Each item is the output of `spine_authority::Grant::encode` for one grant, at most 4096 bytes (`MAX_GRANT_BYTES`). The items are not re-encoded and not embedded as maps: this is the argument `chain_bytes: &[Vec<u8>]` of `verify_encoded_with_invocation`. | An empty array, more than 8 items, an item over 4096 bytes or a grant that does not decode canonically is `authority_step_1` (item count and size) or `authority_step_2` (a grant that is malformed or has an unknown version or caveat), as `verification.rs` `verify_encoded_with_invocation` maps them (`ChainLimits` is step 1; `Malformed` is step 2). |
| `invocation` | bytes | At most 4096 bytes (`MAX_INVOCATION_BYTES`), the output of `Invocation::encode`. | Over the bound: `authority_step_1` (`InvocationLimits`). Does not decode: `authority_step_9` (`InvocationMalformed`). |

A body that is not this map (a missing key, a key of another type, an array item that is not a byte string, an unknown key anywhere in the two closed maps, a `command` that does not decode as a command) is not an authority verdict: it is `invalid_request`, or `unknown_command` when `command` names a command outside the set, decided as 7.4 does today but reading the `command` member (`native.rs` `claimed_command_name` descends into it). Authority is not consulted for such a body and no nonce is recorded.

The length bounds are enforced by the gate, in the order `03-authority.md` verification gives, and not by the body decoder, so that each failure has one authority verdict. Memory is bounded by the frame limit exactly as for any request.

#### C3.3 What the invocation signature covers

By reference to `03-authority.md` and `spine-authority` `InvocationBody`, not redefined: the signature covers the domain `spine-authority-invocation-v1` and the deterministic CBOR of `{v, invoker, recipient, session, nonce, time, res, act, effect, op, leaf, cost, bytes}`. In wire terms:

- `session` MUST equal `HELLO_ACK.binding` of this connection (C2); it is the binding to the connection.
- `recipient` MUST be the node's key; `invoker` MUST be the last grant's audience, and the connection's identity key if there is one (C1.3 rule 4).
- `nonce` (16 bytes, single use per invoker) and `time` are the replay protection together with `session`.
- `op` MUST be `SHA-256(command_bytes)` where `command_bytes` is the encoding of the `command` member. By 5.1 and `decode_strict` (the daemon rejects a body that does not re-encode to the received bytes) the encoding of a command item is unique, so the node computes `command_bytes` as the canonical encoding of the decoded command and the client computes it from what it sends. This is `03-authority.md` P32 ("the SHA-256 of the exact command body bytes as received") with the command moved inside a map: the bytes hashed are those of the command item alone, not of the whole `AUTH_REQUEST` body, because the invocation lives in the same body and cannot hash itself. `authority.rs` `operation_hash` is unchanged.
- `res`, `act`, `effect`, `cost`, `bytes` are checked by the gate against `classify` (`authority.rs` `authorize` checks 2 to 5). `leaf` is the id of the last grant of the supplied chain.
- The request id is not covered (P32 notes the alternative). A relay cannot make a captured invocation run twice (the nonce) or run a different command (`op`) or on another connection (`session`) or at another node (`recipient`); it can attach the same pair to a different request id on the same connection before the first is processed, in which case exactly one of them is accepted.

#### C3.4 Plain REQUEST and AUTH_REQUEST

`REQUEST` (0x10) is unchanged: a body that is one command. Its meaning is "no authority presented". The mode table of C1.4 gives what the node does with it. There is no field whose absence changes meaning: the two frame types are the two meanings, and a client chooses by the type it sends.

#### C3.5 Verdicts and their order

For an `AUTH_REQUEST` the node does, in order, and stops at the first failure: (1) the frame-level checks of 7.2 and `Connection`'s in-flight rules, as for a `REQUEST`; (2) body decoding (C3.2), `invalid_request` or `unknown_command` on failure; (3) the connection's command rate limit (C3.6); (4) the identity rule: if the connection has an identity and `invocation.body.invoker` differs, `authority_step_9`; (5) `AuthorityGate::authorize(chain_bytes, invocation_bytes, &command, SHA-256(command_bytes), binding, now)`, whose steps 1 to 11 and the checks 2 to 5 of "The gate" in `03-authority.md` are unchanged; (6) on success, the session is created if needed and the command runs (`main.rs` `answer_job`, `handle_command`). The rule in (4) needs a new `RefusalCause` in `authority.rs` that reports as step 9 (`RefusalCause::step`).

A refusal records no nonce and charges nothing (`authorize`). A command refused at (3) did not reach the gate, so it consumes no nonce and may be sent again.

#### C3.6 Rate limit change

Quoted from 7.2, last paragraph: "`tool_call` and `capability_query` need no session; `tool_call` is limited by a separate per-connection bucket and `capability_query` is not limited (`spine-core` `main.rs` `handle_session`)." The unlimited `capability_query` becomes an unauthenticated-cost path once an `AUTH_REQUEST` makes the node verify up to 9 signatures per request (an Ed25519 strict verification for each grant of the chain and one for the invocation; a few tens of microseconds each, unverified on the target hardware), and `AuthorityGate::authorize` holds one lock across all of it (`03-authority.md` P33), so one connection could keep every other connection waiting. Proposed: every `AUTH_REQUEST`, whatever its command, is charged to the connection's command bucket (burst 100, refill 10 per second, `check_rate_limit`) before the gate runs, and `capability_query` and `tool_call` inside an `AUTH_REQUEST` are charged to that bucket as well as to their own. A plain `capability_query` stays unlimited. P33's own alternative (verify outside the lock, charge inside) should be done before `required` is enabled (D, stage 6).

#### C3.7 Node key

The gate's recipient and the receipt signer are the node's key. `config.rs` has a `[namespace] key_path`; whether the daemon holds a node signing key when no namespace is configured is unverified (`main.rs` has no direct `load_or_create_key` call by name; the function is `namespace.rs`). Modes `audit` and `required` require a node key and a daemon that cannot find one refuses to start in those modes. Open question 11.

#### C3.8 Alternatives rejected

- A chain handle per connection (C3.1). Not rejected for ever: an optimisation, as an extension (C8).
- A header field in every frame (`COMPLETION_PLAN.md` Stage 8, "a distinct message class"; `00-wire.md` Appendix C G8 "Capability chains in the frame header? Resolved for v1: no"). A header field costs bytes on every frame including `PING`, and it does not distinguish a request that carries authority from one that does not except by a flag, which is a field whose absence has meaning. G8 is superseded by this section; the reasoning of G8 ("a header field would be a new version") is what this document is.
- A flag bit in the frame header (`flags` bit 1). 2.2 requires bits 1 to 7 to be zero and `malformed_frame` otherwise; using one would make the meaning of the body depend on a flag a peer might ignore. A new type is refused by `unknown_frame_type` instead.
- Putting the invocation inside each command. Changes 19 types, and the command cannot be hashed with the signature inside it.
- Carrying a grant as a JSON-shaped map. 5.3 cannot hold byte strings, and a second encoding of a grant is what the brief forbids.

#### C3.9 Security analysis

Data cannot grant (`03-authority.md`): the grant bytes sit in `auth`, which is in no layer-4 object and which the daemon reads only in `AUTH_REQUEST` frames. A hostile `command` payload cannot contain an invocation the node will read, because the node reads `auth` from one place. Replay: `session`, `nonce`, `time`, `recipient`. Confusion between commands: `op` plus `classify`. Resource exhaustion: C3.6, the frame cap, the per-connection queue of 8 (`main.rs` `handle_session`). Information: the refusal names a step only (C4). Relay: C1.6. Side channels: the step number distinguishes where a chain failed, as `03-authority.md` P17 and P31 accept.

### C4. Error codes

Principle: a verdict of `03-authority.md` maps to one code; codes branch, messages do not (8.4). The codes are carried in `RESPONSE.err.code`; `details` is not used by any of them.

| Code | Returned when | Must not reveal |
| --- | --- | --- |
| `authority_required` | Mode `required`, a plain `REQUEST` (any command). | Whether any chain would have sufficed; which resource or action the command needs. The message is fixed text. |
| `authority_not_enabled` | Mode `off`, any `AUTH_REQUEST`. | Nothing beyond the mode, which `HELLO_ACK.authority` already states. |
| `authority_step_1` | Gate step 1: no grants, more than 8, a grant or the invocation over its size bound. | Which grant or bound. |
| `authority_step_2` | Step 2: a grant is malformed, has an unknown version, or carries a caveat that is not recognised. | Which of the three. |
| `authority_step_3` | Step 3: the root's issuer does not own every resource touched or is not a trusted root. | Whether the issuer is trusted or merely not the owner; the trusted roots. |
| `authority_step_4` | Step 4: the chain is not linked. | Which link. |
| `authority_step_5` | Step 5: a grant signature does not verify. | Which grant. |
| `authority_step_6` | Step 6: a grant is broader than its parent. | Which field. |
| `authority_step_7` | Step 7: a grant is not yet valid or has expired. | Which, and which grant. |
| `authority_step_8` | Step 8: a grant is revoked, or revocation data is missing or older than the bound for the effect class (also `RevocationsUnavailable`). | Which grant; whether the cause is revocation or stale data; the revoker. |
| `authority_step_9` | Step 9: the invocation is malformed, cites another chain, has an invoker that is not the last audience or not the connection's identity, names another recipient or session, is stale, has a bad signature, or its nonce was used (also a charge refused for `NonceReplayed`). | Which of the eight; whether a nonce was seen. |
| `authority_step_10` | Step 10: the resource, action or effect class is not covered by the last grant, or the invocation names another action, a weaker effect, another resource or another operation than the command's (the four binding causes `authority.rs` `RefusalCause::step` reports as 10). | Which; the required class of the command. |
| `authority_step_11` | Step 11: a limit would be exceeded, usage is unknown for a limited grant, the ledger is full or could not be written (`LedgerUnavailable`, `Charge(_)`). | Which limit, how much is used, whether the ledger is durable. |

The eleven names are those of `authority.rs` `STEP_NAMES` and `Refusal::name`; `03-authority.md` P31 fixes them ("The caller is given `authority_step_N` and nothing else"). One code with `details.step` was considered and rejected: a client that branches on `code` (8.4) would need a second key, and `details` is JSON-shaped and absent from all but one existing code (`outbound_http_status`, 13). The cost is eleven entries in the closed set.

Further additions, none of them from layer 3:

| Code | Returned when | Must not reveal |
| --- | --- | --- |
| `knowledge_entry_too_large` | `store_knowledge`: the entry (key, compact JSON of the value, tags) exceeds 64 KiB (`knowledge.rs` `MAX_KNOWLEDGE_ENTRY_BYTES`, `admit_entry`). Today `execution_failed`. | Other sessions' usage. |
| `knowledge_capacity` | `store_knowledge`: the session holds 1,024 entries, or the daemon-wide 32 MiB budget is exhausted (`KnowledgeBudget::reserve`, `MAX_KNOWLEDGE_BYTES`). Today `execution_failed`. The text distinguishes the two causes; the code does not. | The daemon-wide total. |
| `action_unsupported` | `execute_binary`: the run requested an action the daemon does not perform (`search`, `query_knowledge`, `reason` in a default build, 13 `execution_failed` row). The program is refused whole before any action runs. Today `execution_failed`. | The list of actions that are performed. |
| `delete_refused` | `delete_resource`: the proof does not verify, or the blob's recorded publisher is not the proven key (C7). | Which of the two; whether the blob exists under another publisher. |

Changes to `00-wire.md` 13: the table grows by 17 rows (2 + 11 + 3 + 1) to 64 codes; the `execution_failed` row loses "`store_knowledge` is refused with the same code when ..." and the sentence about a program that requests an action the daemon does not perform; `body.rs` `ALL_ERROR_CODES` grows to 64 and `PROTOCOL_ERROR_CODES` is unchanged. A client MUST still tolerate a code it does not know (8.4), so an old client sees the new codes as unknown errors.

### C5. Receipts

Decision: a receipt is returned to the client in a new server-to-client frame `RECEIPT` (type `0x13`) for each `AUTH_REQUEST` that the gate authorised and whose command then ran (whether the command answered `ok` or `err`), in modes `audit` and `required`. Never for a refusal, never for a plain `REQUEST`, never in `off`. The client cannot request or suppress it. The node also keeps it (`03-authority.md` "Receipts": the node-local copy is the node's audit record; where it is stored is not specified here).

Frame. Request id: the id of the request. Body: one deterministic CBOR item, the `spine-authority` receipt exactly as `Receipt::encode` writes it (a closed map `{body, sig}`, at most 512 bytes: `MAX_RECEIPT_BYTES`; fields in `03-authority.md` "Receipt"). The node issues it with `AuthorityGate::receipt(signer, authorized, result_hash, time)`.

Rules:

1. Order. The node sends `RECEIPT` before the `RESPONSE` of the same request, while the request id is in flight. A `RECEIPT` for an id that is not in flight, or already receipted, or streaming, is `protocol_violation` on receipt, and `ConnectionError::Usage` on send, like a second `RESPONSE` (8.4). Sending before the `RESPONSE` avoids the ambiguity of an id that the client reuses the moment it reads the answer (8.3: "a client that reuses the id the moment it has read the answer is not refused").
2. Result digest. `result` in the receipt is `SHA-256("spine-wire-result-v1" || 0x00 || body)` where `body` is the body of the `RESPONSE` frame the node sends for this request, exactly as sent. The client can therefore verify that the receipt is for the answer it got, with no extra data, and `03-authority.md` receives the opaque digest it asks for. An answer that the daemon replaces with `internal` because it was too large (8.4, `main.rs` `reply`) is hashed as replaced.
3. What the receipt proves and does not: the node, holding its key, says it authorised and performed the invocation with this id (the receipt's `invocation` is `Invocation::id`, which the client can compute) and charged these amounts. It is not proof that the effect persisted, only that the command ran; a command that returned `err` is a performed invocation with an error result (`allowed` true).
4. A client SHOULD verify it (`Receipt::verify` against the node's key, then `matches`) and MAY keep it. A client that does not understand `RECEIPT` is a revised-protocol violation: the type is in the closed set (it closes with `unknown_frame_type` on an old peer, which is why the node sends one only to a client that sent an `AUTH_REQUEST`).
5. Bound: one `RECEIPT` per request, at most 512 bytes of body; 8 requests in flight so at most 8 receipts outstanding.

Why returned and not node-local only: a client that loses its connection after sending a non-idempotent command cannot learn whether it ran (C2.2 rule 4). A receipt is a proof of performance the client holds, and this is the one place where the audit record of layer 3 benefits the requester. Why not on request: an opt-in is a field, and a request without it silently changes what the node does (principle 5); the size is small and the signing cost is one Ed25519 signature on a path that has just done at least 9 verifications.

Why never for a refusal: `03-authority.md` P25 notes that signing refusals lets anyone who can send an invocation demand a signature, and the refusal already reaches the client as a code. If the owner wants denial receipts, `Receipt` supports them (`allowed` false, the step) and only the node's choice to send them changes.

Alternatives rejected: a `receipt` key inside `RESPONSE` (breaks the single-entry `{ok}` / `{err}` shape of 8.4, which every decoder and binding relies on, `body.rs` `Reply`); a `RECEIPT` frame after the `RESPONSE` (the id-reuse ambiguity above); a new command to fetch a receipt by invocation id (requires the node to store receipts, a store with its own bound, and the client to make a second authorised request to fetch the proof of the first).

Security: the receipt is signed by the node key over a domain-separated body (`spine-authority-receipt-v1`); it discloses nothing the requester did not send except the node's `time` and the charged amounts. The signing key is the node key, so a node that signs receipts uses it for a second purpose (it already signs mesh calls and records: `03-authority.md` classification of `mesh_call`); a separate receipt key is possible and not proposed.

### C6. Names

#### C6.1 The log in a resolve answer

Current: `NameResolution` is externally tagged: `{"resolved": {record, provenance}}`, `{"unchanged": {ttl_secs}}`, `{"not_found": {name}}`, `{"invalid": {name, reason}}`, `{"unavailable": {name, reason}}` (`lib.rs` `NameResolution`; `00-wire.md` 12.4). The record is a JSON-shaped `spine_name::NameRecord` (5.3, section 16 item 3).

Proposed `resolved` variant: `{"resolved": {record, provenance, log}}` with

| Field | Type | Rule |
| --- | --- | --- |
| `log` | array of JSON objects, 0 to 64 items (`MAX_SUCCESSION_ENTRIES`) | Always present in a `resolved` answer. The succession entries the node holds for the name, in the JSON form of `02-names.md` 10.1 (each entry an object with the fields of that section, keys, hashes and the signature as unpadded lower-case base32, `prev` and `commitment` `null` when absent, `kind` the lower-case word). An empty array means exactly: "this node holds no log for this name". It does not mean the name has none (see below). |

The same JSON form is what `publish_name` and `publish_resource` already read from the publication object (`main.rs` `publication_log`, `PublishResource`), so no second encoding is introduced. Always present, not optional, so that a client cannot confuse "the node did not include the log" with "no log" (principle 5); the unavoidable residue is that the node may hold a shorter log than exists (`02-names.md` 10.8), which a client cannot detect and which the head claim in the record narrows (10.7).

Client rule, proposed text: a client that receives `resolved` for a `did:` name MUST NOT treat the record as authorised until it has verified `log` from the inception key in the name (`SuccessionLog::verify`) and judged the record against the resulting state (`record_authorised`, 10.6) and, if the record carries a head claim above the log it was given, treated the answer as unverified (`NeedLog`). The node is not trusted: a wrong or truncated log can only make the client refuse, never accept a record the key history forbids. The reference client function is `spine-agent` `authorise_with_log` / `read_resource_with_log` (`02-names.md` 10.11, "Client resource paths"); the SDK's `resolve_name` returns the log to it.

Bound: 64 entries of at most about 600 bytes of JSON each (an estimate, not measured), so about 40 KB per answer. One answer fits any frame. A `resolve_names` batch of 256 names with full logs could exceed 16 MiB; the existing rule then applies (an answer too large to send is replaced by `internal`, 8.4), and the node MAY instead answer `batch_too_large` when it can see in advance that the sum would exceed `max_frame`.

`unchanged`: `if_none_match` is the hash of the record the client holds. A node answers `unchanged` only when it would otherwise answer `resolved` with that record; if the name is now retired, compromised or revoked it answers that outcome instead. The client keeps its log. If a log grew while the record stayed valid, a fresh answer is not needed: a growth of the log that does not supersede the record's signing key does not exist (every rotation supersedes the previous key, 10.6).

#### C6.2 New outcomes

Three variants, each carrying the evidence so that the client verifies it and need not trust the node:

| Variant | Fields | Meaning and client rule |
| --- | --- | --- |
| `{"retired": {name, log}}` | `log`: 1 to 64 entries whose last entry is a `retire` | The name's succession log ends in a retirement (`02-names.md` 10.3, `State::Retired`; today `NameError::Retired`, "answers a retired name with `Retired`", 10.11). Permanent for verifiers that hold the log (10.8). The client MUST verify `log` from the inception key in the name and that it ends in `retire`; if it does not verify, the client MUST treat the answer as `unavailable` (the node is wrong or hostile, and "not found" would be a lie in the other direction). |
| `{"compromised": {name, logs}}` | `logs`: exactly 2 arrays of 1 to 64 entries each | The node holds an equivocation proof for the name (`02-names.md` 10.5, 10.10 decision 51): the proof's two logs (`Equivocation::logs`: the shared prefix with `first`, and the shared prefix with `second`). The client MUST verify both from the inception key and that neither is a prefix of the other (`detect_equivocation`), and then treat the name as compromised: authorise no record for it. If the check fails, the answer is `unavailable`. This mirrors decision 51 exactly: the proof travels as two logs, nothing else is added. |
| `{"revoked": {record, provenance, log}}` | `record`: the tombstone as a JSON `NameRecord`; `provenance` and `log` as in `resolved` | The node holds a fresh tombstone for the name (`02-names.md` 3.7, today `NameError::Revoked`, reported as `invalid` with a reason, 12.4 last paragraph). The client MUST verify the tombstone (signature, `is_tombstone`, shape of 3.7, under the key the verified `log` gives). A tombstone expires (7 days) and is not permanent; a client MUST NOT treat `revoked` as permanent. |

Quoted from 12.4 and changed: "A name whose record is a revocation (tombstone) is reported as `invalid` with a reason (`NameError::Revoked`'s text); there is no separate wire variant for it." The sentence is deleted. "`resolve_name` always answers `ok`; a bad or missing name is a variant, not an `err`" stays. The status of a retired or compromised name that is learned from the mesh and today surfaces as `unavailable` with the reason (`02-names.md` 10.11 item 1) becomes `retired` or `compromised` with evidence.

These variants are used by `resolve_name`, `resolve_names` (per item) and `fetch_name` (the `resolution` field). `find_providers` and `crawl_names` are not changed: providers are records without logs, and a client that wants a provider's key history resolves the name. This is a limit, listed in F.

Alternatives rejected: a single `status` field next to `record` (an optional field again); returning the proof as a new object (decision 51 considered that and chose to reuse the logs); a bare `retired`/`compromised` marker without logs (an unverifiable claim from an unauthenticated daemon, 1.4); keeping tombstones as `invalid` (a client cannot tell revocation from a malformed name and cannot verify it).

Security: a hostile node can withhold a log (the client then holds an unverified answer, which it handles as above), send a log that does not verify (refused), or claim `retired` or `compromised` to deny service to a name (it cannot make the claim verify without the proof, so the client falls back to `unavailable`, which already is the answer for an unreachable node). What the node cannot do is make a client accept a superseded key's record: that needs the client to skip the check, which the MUST forbids; the exposure left is 10.8 (a node that is behind) and 10.6 (a record with no head claim signed by a superseded key, which no log-holding verifier would accept and a client without the entry cannot know).

### C7. Objects: delete

Two new commands. The set grows from 19 to 21 (`lib.rs` `COMMAND_NAMES`, `BrowserCommand`).

#### C7.1 delete_resource

Fields (12.2 format):

| Field | Type | Optional | Bytes |
| --- | --- | --- | --- |
| `hash` | 32 bytes | no | yes |
| `owner` | 32 bytes | no | yes |
| `proof` | 64 bytes | no | yes |

`proof` is an Ed25519 signature by `owner` over the message `"spine-wire-delete-resource-v1" || 0x00 || hash || binding`, where `binding` is `HELLO_ACK.binding` of this connection. Binding it to the connection prevents a captured proof from deleting a blob that the owner re-publishes later through another connection; within one connection a replay deletes the same blob again, which the idempotency rule below makes harmless.

Behaviour (`resources.rs` `ResourceStore::delete(owner, hash)`; `DeleteError` has `NotFound`, `NotOwner`, `Internal`):

- The proof does not verify, or the blob exists and its recorded publisher is not `owner`: `delete_refused` (one code for both, so that a probe learns nothing it could not learn from `read_resource`, which already confirms existence of a hash).
- The blob does not exist: `ok` with `{deleted: false}`. A delete is idempotent: after a successful delete, a second delete of the same hash answers `{deleted: false}`. The cost of this is that a mistyped hash looks like success; the reply says which.
- The blob exists and `owner` is its publisher: it is removed and `{deleted: true}` is answered. A name that refers to it then stops serving bytes (`resources.rs` `delete` comment); the name record is not touched.
- An internal failure is `internal`.

Authority: classified `publish` on `node/resources`, effect class `irreversible` (`authority.rs` `classify` has no wildcard arm, so the two commands cannot be added without a row each; `03-authority.md` P29: "a publication ... [is] `irreversible`"). The grant says the caller may use the node's storage; it says nothing about whose blob, exactly as `publish_name` and `publish_resource` carry a record signed by the name's own key and layer 2 checks it (`03-authority.md`, "Resource model"). The two checks are independent and both must pass. Who may delete without a grant: in modes `off` and `audit` anyone who can produce the proof, because the owner check is the only check; this is as strong as `publish_resource` today, which also needs a signature by the publisher's key.

Why not authority alone: the node-level grant is one resource area (`node/resources`, P28), so a grant holder could delete another publisher's blobs; the owner check that `resources.rs` already implements is kept. Why not the owner check alone: `required` mode says no operation succeeds without a chain.

#### C7.2 delete_knowledge

| Field | Type | Optional |
| --- | --- | --- |
| `key` | text | no |

Removes every entry of this connection's session whose key equals `key` exactly (the same scope as `query_knowledge`: "from this connection's session only", 12.4), releases their bytes from the daemon-wide budget, and answers `{deleted: n}` with `n` the number removed (a uint, possibly 0). Absent: `{deleted: 0}`, success; idempotent. A connection that has no session answers `{deleted: 0}` and creates none (a delete creates no session; unlike other session commands). Authority: `invoke` on `node/knowledge`, effect class `reversible` (a later `store_knowledge` of the value the client still holds restores the state; this is the criterion of `03-authority.md` "Command classification"). The store API has no per-entry delete that was found (A.5), so this needs a new function on `BrowserState` that mirrors the release done when a session ends.

Alternatives: delete by index or timestamp (entries are an append-only vector with duplicates allowed and no identifier on the wire); delete everything (loses the selective case). Not proposed: a delete across sessions (knowledge is session-scoped).

#### C7.3 Security

A delete is a destructive command, so both are rate limited like other session commands. `delete_resource` costs one signature verification and a store write. The proof's domain is unique; its `binding` input is why a proof is not a bearer token.

### C8. Deferred, and what to reserve

| Item | Decision | Reserve now? |
| --- | --- | --- |
| Idempotency keys | Deferred. The need is real: a client that loses a connection cannot know whether a non-idempotent command ran (C2.2 rule 4); C5 gives a receipt when the answer is lost only if the receipt arrived. A key belongs in the signed invocation (it must be bound to the operation and the invoker), which has a version (`INVOCATION_VERSION`, `v`) and is opaque to the wire (C3.1, principle 6): a future invocation version can carry it with no wire change. For plain requests there is no invoker to scope a key to. | Nothing. The carrier already allows it. |
| Dry-run | Deferred. It is a property of a request, not of a grant, so it does not fit in the invocation. The likely shape is a command that asks the gate and the command checks without running ("would this be authorised, and would it be accepted"), which is a new command in the closed set. | Nothing: reserving a field in the closed `AUTH_REQUEST` map would be speculative. Adding a command later is a growth of a closed set the revision already does. |
| Cancel | Deferred. `STREAM_CANCEL` exists and is ignored by the daemon (16 item 9, 10); it is the natural frame for cancelling any in-flight request. | One sentence for section 10 to add now, because it removes an ambiguity at no cost: "A `STREAM_CANCEL` for an id whose request is not a stream is ignored; a sender MUST NOT rely on it having an effect." It states what the daemon does and leaves a later revision free to give it effect. |
| Per-client quota | Deferred. The keys needed (the identity of C1, and `limits` in grants, whose ledger is shared by all callers: `authority.rs` `AuthorityGate`) exist after this revision; the daemon's per-connection bucket keys on a random lease id (`RateLease`). The work is daemon-side and needs no wire field. | Nothing. |
| Chain cache | Deferred optimisation (C3.1): a node-side cache of confirmed grants keyed by id (P10), registered by an `EXT` frame under an extension such as `spine.authority/chain-cache/1`. Section 11 lets an extension carry it without a core change if the core `chain` member can then be absent, which is not allowed here (`chain` is required). So the optimisation would be another change to `AUTH_REQUEST`. | No: the cost of one future field is lower than a speculative optional one now. |

The assessment of a "third change": the only item that could force one is dry-run, and only if it is shaped as a request option. The shape as a command does not.

## D. Compatibility inside the project

### D.1 Every code location that would change

In dependency order (lower layers first).

`src/spine-protocol/src`:

- `frame.rs`: `FrameType` (add `Identify = 0x06`, `AuthRequest = 0x12`, `Receipt = 0x13`), `FrameType::from_u8` / `as_u8`, `check_request_id` (the `IDENTIFY` id is 0, `AUTH_REQUEST` follows `REQUEST`, `RECEIPT` follows `RESPONSE`), the test that lists unallocated types (`0x12` is in its list today).
- `hello.rs`: `HelloAck` (three fields), `HelloAck::validate`, `LocalConfig` (`binding`, `authority`, `time` supplied per connection), `negotiate` (copies them into the acknowledgement). `Hello` is unchanged. A new `Identify` struct with `to_frame`, `from_frame`, the signed-message builder and its check.
- `connection.rs`: `Tracker::check_outgoing`, `check_incoming` and `admit` (the three new types; the identity window; the one-receipt-per-request state; `Exchange` gains a "receipted" notion); `Connection::server_handshake` (accept per-connection values: today it takes `&LocalConfig` and builds the acknowledgement, so the daemon needs per-connection values: unverified how `serve_native` can pass them without cloning `local_config()`, which is a plain function now); `split` halves (`ConnectionReader`, `ConnectionWriter`) must learn the new types; the reader must hand an `IDENTIFY` to the caller rather than answer it.
- `native.rs`: `AuthRequest` and its `auth` struct, `encode_auth_request`, `decode_auth_request` (and `claimed_command_name` reading the `command` member), `send_auth_command` (client), `decode_message` (REQUEST/AUTH_REQUEST/RECEIPT), `local_config`, `client_hello` (unchanged), the client handshake path (`client_connect` keeps the acknowledgement's `binding`, `authority`, `time`).
- `body.rs`: 17 `ERR_*` constants, `ALL_ERROR_CODES` (47 to 64), the uniqueness test.
- `lib.rs`: `BrowserCommand` (two variants), `COMMAND_NAMES` (`[&str; 19]` to 21), `NameResolution` (`resolved.log`, three variants).
- `wirefmt.rs`: only if a helper is needed for the byte-string fields.

`src/spine-authority` and `src/spine-name`: no format change. `spine-authority` gains nothing. `spine-name` needs a public function that returns the log JSON for a name from the resolver's `LogBook` (unverified whether one exists; `logbook.rs` `LogBook::logs_to_send` is used for the mesh answer).

`src/spine-core/src`:

- `config.rs`: `AuthorityMode` gains `Audit` (`AuthorityMode::Off`, `Required` today); `AuthorityConfig::validate` stops refusing `required` only in the last stage (D.2 stage 6); a startup check that a node key exists in `audit` and `required`.
- `authority.rs`: `classify` (two rows), a `RefusalCause` for the identity rule (step 9), a wrapper that maps `Refusal::name` to the code (`STEP_NAMES` already is the codes), `GateSettings`/`from_config` (unchanged).
- `main.rs`: `serve_native` (per-connection `LocalConfig`: `binding` from the OS random source, `authority`, `time`), `read_requests` (types, the `IDENTIFY` step, `AUTH_REQUEST` to `Job`), `Job` (the request type and the auth bytes), `answer_job` (rate limit, identity rule, gate, then the existing path), `run_commands` and `response_frame` (the receipt before the response), `handle_command` (`DeleteResource`, `DeleteKnowledge`; `read_failure`-style mapping for `DeleteError`), `resolve_one` and the `fetch_name` path (the log and the three outcomes; unverified: not read), `ExecuteBinary` and `StoreKnowledge` arms (the three codes), `BrowserState::append_knowledge` (return a typed error instead of `String`; `knowledge.rs` `admit_entry`, `KnowledgeBudget::reserve` return `String` today) and a new `BrowserState` delete.
- `resources.rs`: no change (`ResourceStore::delete` exists).
- `authority_ledger.rs`: no change; its cost per authorised request (one durable write) is a performance matter (F).

`src/spine-agent/src/lib.rs`: `AgentClient::over` keeps `binding`, `authority`, `time`; an optional identity key (`with_identity`) that sends `IDENTIFY`; a per-call way to attach a chain and a signing key (an `Authorization` value); `navigate` and the other command methods route through one place that sends `REQUEST` or `AUTH_REQUEST`; receipt handling (collect, verify, expose); `resolve_name` returns the log; `delete_resource`, `delete_knowledge`; `read_resource_with_log` and `publish_resource_with_log` take the log from the answer. The Go, FFI and Python bindings, the gateway and `spine-cli` use the SDK: unverified how many of them construct requests directly.

`docs`: `00-wire.md` (A.2), `03-authority.md` ("Node enforcement", "What is not done", "Configuration", P36), `02-names.md` 10.11 (Remains 1, 2, 7), `04-objects.md` section 4.4 ("It is not on the wire"), `COMPLETION_PLAN.md` (Stage 6 to 9 items), `CHANGELOG.md`.

`scripts/wire_v1.py`, `docs/spec/00-wire.vectors.json`, `src/spine-protocol/tests/conformance_vectors.rs`, `.github/workflows` job `wire-vectors`.

### D.2 Staged implementation plan

Every stage keeps `cargo test` and the CI jobs green and is testable alone. The gate stays disconnected until stage 5.

1. Spec, vectors and reference first. Promote the text of C into `00-wire.md`; extend `scripts/wire_v1.py` and regenerate the vectors; extend `conformance_vectors.rs` so the Rust codec fails the new vectors; no behaviour change in the daemon. Gate: the `wire-vectors` job and the Rust conformance test, both of which fail until stage 2 and pass after it, so the order within the stage is vectors, then the encoders of stage 2.
2. Encoders and decoders. `frame.rs`, `hello.rs`, `connection.rs`, `native.rs`, `body.rs`, `lib.rs` as in D.1, with unit tests per rule. The daemon builds its acknowledgement with fresh per-connection values from this stage on (a random `binding`, `authority = "off"`, its clock), so that its handshake already conforms to the new vectors. Because `Connection` now knows the three new types, the daemon's reader must refuse `IDENTIFY`, `AUTH_REQUEST` and any `RECEIPT` with `protocol_violation` until stage 3 and stage 5 accept them; this guard is part of the stage and is tested.
3. Handshake: `binding`, `authority`, `time` and `IDENTIFY` in the daemon (`serve_native`, `read_requests`) and the SDK. `authority` is `off` for everyone. The SDK sends `IDENTIFY` when given a key. Test: an acceptance test through a spawned daemon (`src/spine-core/tests/agent_web.rs`) for a good proof, a bad proof (`CLOSE(protocol_violation)`), a second `IDENTIFY`, one after a request.
4. Names and objects (independent of authority; can run in parallel with 3): `NameResolution` log and variants, `delete_resource`, `delete_knowledge`, the three codes, the `classify` rows. Tests: the rotated-name acceptance tests (`rotated_name_publishes_and_reads_a_resource_through_the_agent_client` and its siblings) rewritten to use the log from the answer; a retired name, a compromised name and a revoked name through a running daemon; the delete tests.
5. Carrier in `audit`. `AuthorityMode::Audit`; build the gate in `main.rs` from `AuthorityGate::from_config` and the node key; `AUTH_REQUEST` through `answer_job`; the receipt frame; the rate-limit change; the identity rule. Commands always run. Revocation data: `refresh_revocations` called by a timer in the daemon (the gate refuses everything at step 8 without it, `03-authority.md` P35). Tests: every verdict reaches the log; a replay is logged as step 9 and still runs; receipts verify; an acceptance test that a node in `audit` runs a command whose chain is bad and logs it.
6. `required`. Remove the startup refusal in `AuthorityConfig::validate`; verify outside the lock (P33's alternative) with the charge inside it; a documented source for revocation data (open question 12); acceptance tests that every one of the 21 commands is refused without a chain and accepted with the right chain and refused for each wrong class. `COMPLETION_PLAN.md` Stage 6 items close as far as they are met.

### D.3 Vectors to add

In `00-wire.vectors.json`, in the existing sections:

- `valid_frames`: an `IDENTIFY` with an empty `channel`; an `AUTH_REQUEST` (the `auth` bytes copied from `03-authority.vectors.json` so a grant and an invocation of known bytes appear; the `op` of that invocation must equal the hash of the `command` member, so this vector requires generating a matching invocation: the authority script `scripts/authority_v1.py` is the generator and the wire script can import its encoder or the pair can be fixed constants); a `RECEIPT` (the `Receipt::encode` bytes of an authority vector); new commands and their replies.
- `invalid_frames`: `IDENTIFY` with a nonzero request id (`protocol_violation`); an `IDENTIFY` body with a 31-byte key, a 63-byte signature, a 1-byte `channel`, an unknown key, a non-map body; `AUTH_REQUEST` with request id 0; a `RECEIPT` with request id 0 (it carries the id of its request, which is never 0).
- `handshakes`: the `local` input gains `binding`, `authority`, `time`; every existing entry's expected acknowledgement changes; add cases for a client that receives an acknowledgement without `binding`, with a 31-byte `binding`, with an `authority` of `"maybe"`, with a mistyped `time` (all `protocol_violation`).
- A new section `identify` for the signed message: for given `H`, `A`, `channel`, `key`, the bytes of `M`, the signature of a fixed test key, and the verdict (valid, a changed `A`, a changed `channel`, a changed key). These need Ed25519.
- `invalid_bodies` or a new section for `AUTH_REQUEST` bodies and the C6 outcomes' JSON-shaped values, and `NameResolution` samples for `resolved` with and without a log.
- A section listing `ALL_ERROR_CODES` (64) so a drift between the spec table and `body.rs` is caught; there is none today (15.4 says section 13 is not covered).

What the checker needs: `scripts/wire_v1.py` is Python 3 with the standard library only (15.1) and the standard library has no Ed25519. The reference needs either a short pure-Python RFC 8032 implementation (signing and strict verification, about 100 lines, self-checked against the RFC's test vectors inside the script, slow but a handful of signatures is enough) or a dependency. Recommended: the pure-Python implementation, so the reference stays dependency-free and independent of the Rust code. It must implement the strict rule (S below the group order; small-order key or R rejected) that `ed25519_dalek::verify_strict` implements, with vectors for each. The checker also needs the new frame type table (`FRAME_TYPES`), `CLOSE` unchanged, `COMMAND_NAMES` and `COMMAND_FIELDS` for 21 commands, a `check_auth_request` for the closed `auth` map, and the cross-check function `cross_checks` extended for the new sections. The CI job needs no change beyond running the script; the authority vectors stay checked by their own job.

## E. Open questions for the owner

Each has a recommended answer and what depends on it.

1. In place or version 2 (A.3). Recommended: in place. Depends on: whether anything outside this repository speaks the current v1. If yes, version 2 and C stands unchanged (A.3 says what moves).
2. Chain per request or by handle (C3.1). Recommended: per request. Depends on: the measured cost of 37 KB per request, which this draft has not measured; if it is too high the optimisation is a later change to `AUTH_REQUEST`.
3. Keep `IDENTIFY` (C1), or drop it and rely on the invocation signature alone. Recommended: keep. Depends on: attribution of plain requests in `audit`, per-identity quota and the invoker equals identity rule. Without it C2 to C8 stand and the daemon has one fewer frame.
4. Should an invocation's invoker be required to equal the connection identity (C1.3 rule 4). Recommended: yes. Depends on: whether any proxy or service pattern needs one connection to carry several invokers; `03-authority.md` question 2 (confused deputy) says the service should use a delegated narrower chain under its own key, which this allows.
5. Does `audit` run the command whatever the verdict (C1.4). Recommended: yes. Depends on: nothing else; the alternative is a mode that blocks, which is `required`.
6. Is any command exempt from `required` (C1.4), for example `capability_query` for discovery. Recommended: none. Depends on: whether anonymous discovery is a product need; an exemption is a list in the node's configuration, and an ambient-authority hole.
7. Eleven codes or one code with `details` (C4). Recommended: eleven. Depends on: the owner's view of code-set size against client simplicity.
8. Classification of the new commands (C7): `delete_resource` as `publish`/`irreversible`, `delete_knowledge` as `invoke`/`reversible`, and whether the first use of action `admin` is wanted instead. Recommended: as proposed. Depends on: staleness bounds per class (P8): `irreversible` needs current revocation data.
9. The owner proof for `delete_resource` (C7.1) next to the grant. Recommended: both. Depends on: whether the node's grant should be allowed to delete any publisher's blob; if yes the proof can go.
10. The `log` field in a resolve answer always present, and the three outcomes with evidence (C6). Recommended: as proposed. Depends on: the 40 KB per answer and the `resolve_names` bound.
11. The node key when no `[namespace]` is configured (C3.7). Recommended: `audit` and `required` demand a key file and refuse to start without one. Depends on: how the daemon derives or stores its key; unverified.
12. Where revocation data comes from before a feed is fetched (D.2 stage 5, 6; P35). Recommended: a daemon timer that refreshes the node's local set, and a statement that an intermediate issuer's revocation (P19) is not seen until feeds exist. Depends on: Stage 6 of the plan; `required` is not usable for `irreversible` commands without it.
13. A pure-Python Ed25519 in the reference (D.3). Recommended: yes. Depends on: the rule that the reference is dependency-free.
14. A TLS exporter as the `channel` value today (C1.7). Recommended: not in this revision. Depends on: the rustls access in generic stream code, and a WebSocket and QUIC story.
15. Rate-limit `AUTH_REQUEST` under the connection bucket including `capability_query` (C3.6). Recommended: yes, with the change to 7.2 quoted there.
16. Receipts: always on an authorised request, none for refusals (C5). Recommended: as proposed. Depends on: the node key being acceptable as a receipt signer.

Which of `03-authority.md`'s open questions (Q1 to Q7) this revision forces and which it leaves:

- Q1, offline revocation: touched, not answered. `required` cannot work without revocation data (step 8 refuses everything with none) so question 12 above must be answered before `required`; the staleness defaults (P8) stay.
- Q2, confused deputy: touched. Rule 4 of C1.3 makes the connection's key the only invoker, which forces the service-holds-its-own-delegated-chain pattern; the API question (is the wrong thing hard) stays open.
- Q3, privacy: forced for the wire: the chain travels in full to the recipient on every request (C3.1). The hidden-principal option stays unanswered at layer 3.
- Q4, post-quantum signatures: left open. The carrier bounds nothing itself (C3.2: layer 3 bounds), so a larger signature needs `MAX_GRANT_BYTES` and the frame cap checked, no wire change; the 37 KB figure of C3.1 would grow.
- Q5, cost units: left open. The invocation carries declared `cost` and `bytes` (C3.3); the wire does not say what they mean.
- Q6, human approval: left open; the caveat registry is empty (P6) and the carrier is opaque to caveats.
- Q7, clock trust: forced in part: `HELLO_ACK.time` (C2.3) chooses the node's clock as the reference for invocation freshness; grants' windows and devices without a clock stay unspecified.

## F. Threats this does not address

- The handshake has had no external review, and neither has this revision. The frame, handshake and error layers stay "not externally reviewed" (`00-wire.md`, status line).
- Relay without channel binding (C1.6). Until a security layer defines the `channel` value, an active relay on an unprotected path can carry an authenticated connection and see and drop traffic; only invocations are protected from forgery.
- No confidentiality and no integrity of the frame layer (1.4): chains, commands and results are plaintext unless TLS is used. A passive observer on plaintext learns `binding`, the chain (the whole delegation path, `03-authority.md` Q3) and every result.
- The node is not authenticated by the wire. A client learns the node's key from elsewhere (C1.6).
- Key minting is free. A key proves possession, not uniqueness; a client can open connections as any number of identities, so per-identity quota (C8) bounds only identified, granted clients, and the daemon's connection and per-IP limits (`server.max_connections`, `server.max_connections_per_ip`) remain the bound on the rest. This is the residual `04-objects.md` records for per-publisher shares.
- Verification cost. Up to 9 signature verifications per `AUTH_REQUEST`, one lock in the gate (P33) and one durable write per authorised request (`authority_ledger.rs`), all under the connection rate bucket. Throughput is unmeasured. A flood of well-formed, cheap-to-reject requests is bounded by the bucket; a flood of connections is bounded by admission only.
- Revocation reach. Only the node's local set is consulted (P35); no feed is fetched. A grant revoked by an issuer the node does not know of is honoured until it expires.
- The log a node returns may be shorter than the true log (`02-names.md` 10.8). The revision lets a client verify what it is given and see a head claim that exceeds it; it does not make a client learn an entry nobody showed it. `find_providers` and `crawl_names` return records without logs.
- A name's weak window before its first rotation (`02-names.md` 10.4) is unchanged.
- Receipts prove the node's statement, not that the effect persisted or that the node was honest; they are signed by the node key, which also signs other things.
- Timing, traffic analysis, the size of a chain as a fingerprint, and denial of service by a client that holds a valid identity and a valid chain and sends legitimately large requests.
- Cross-layer: nothing here changes how a grant is issued, how a root is trusted (P5) or how the node's own root grant (single-user mode, P34) reaches the first client; that bootstrap is layer 3 and is not on the wire.
- The browser extension protocol (`00-wire.md` Appendix C G11) is untouched.
