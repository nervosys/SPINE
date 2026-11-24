# Changelog

All notable changes to SPINE are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Namespace and storage state in `/metrics` and `/ready` (daemon only, no wire change).** `/metrics` now describes every resolver counter (lookups, replication, refusals, succession logs and fetches, equivocations, cache results), peers, records held, resource store bytes against its limit, `execute_binary` permits and session persistence state, with `# HELP` lines and fixed label sets; the renderer is a pure function in `spine_core::operations`. `/ready` gains a `warnings` array (`namespace_no_peers`, `namespace_seeds_unreachable`) that never fails it, and two reasons that do: `persistence_failing` and `storage_unavailable`. Existing fields and metric names are unchanged. See `docs/NATIVE_OPERATIONS.md`.
- **Durable authority ledger (library only, not on the wire).** `spine-core` gains `authority_ledger::DurableLedger`, a `MemoryLedger` mirrored in a `spine_storage::StorageBackend` (namespaces `spine-authority-nonces-v1` and `spine-authority-usage-v1`, fixed-layout versioned rows). A charge writes its rows first and only then changes memory, so a failed write refuses the call (step 11) and a crash errs on the side of having charged; on open, rows past their horizon or expiry are dropped and everything else is restored exactly, including totals and the rate window; an unreadable usage row restores its grant as exhausted, never as unspent. `AuthorityGate::new_durable` builds a gate over it with the same checks in the same order. `spine-authority` gains pure `MemoryLedger::plan_charge`, `commit`, `expire`, `restore_nonce`, `restore_usage` and `usage_record` (`charge` is unchanged in behaviour). Spec 03 records decisions P37 to P43. The daemon does not build a gate, so no node uses it yet.
- **Succession-aware resource clients (spec 02, 3.5, 10.11, decision 46).** `AgentClient::publish_resource_with_log`, `read_resource_with_log` and `fetch_resource_with_log` take the name's succession log from the caller and judge the binding record against it, and `spine name upload --log` and `fetch --log` pass it; `spine_agent::resources::authorise_with_log` and `authorise_without_log` are the checks. With no log a record that carries a `spine.succession` claim is refused with `NeedSuccessionLog` (the gateway resource routes, the existing client methods and the CLI included) instead of a bad-signature error, and a record without a claim is judged as before. A resolving client still cannot obtain the log from the daemon, so a rotated name's resource is fetched only by a caller that holds it.
- **Explicit log fetch on `NeedLog` (spec 02, 10.11 item 3, decision 33).** A mesh node that refuses an announced record because it claims a succession head beyond the log held now asks the announcer for the log: one ordinary `NameResolveRequest` for the name (whose answer already carries the log, also for a retired name with no record), to the announcer alone, only if the envelope verifies under a key recorded for it, and tracked as a pending single-peer lookup so the answer is accepted by the same rule as any response (asked peer, once, referrals ignored). No new payload and no layout change. Bounds in `spine_mesh::naming`: one fetch per name per 60 s (`LOG_FETCH_INTERVAL_SECS`), 64 names tracked for at most 300 s (`MAX_WANTED_LOGS`, `WANTED_LOG_HORIZON_SECS`), 8 per peer per minute (`MAX_LOG_FETCHES_PER_PEER`), unanswered fetches dropped after 30 s, all on the service clock. New `ResolverMetrics::log_fetches_sent`, `log_fetches_suppressed` and `logs_fetched`; new `NameService::want_log`, `is_log_fetch` and `wanted_logs`. A lookup that meets a record ahead of its log already carries on to the other peers and is unchanged.
- **Authority follow-ups.** `spine_authority::verify_encoded_with_invocation` returns the decoded `Invocation` with the `Decision` (`verify_encoded` is now a wrapper), and `AuthorityGate::authorize` uses it, so the invocation is decoded once. `parse_key_hex` in the node configuration accepts exactly 64 ASCII hex digits of either case, so a pair such as `+f` is refused in `[namespace.seed_keys]` and `[authority] trusted_roots` alike.
- **Key succession reaches the daemon's resource store and owner renewal.** `ResourceStore` gains `with_record_authority`, a function that replaces the bare `NameRecord::verify` in `put_at`; without it the store behaves as before. The daemon supplies the new read-only `LocalResolver::authorise` (`LogBook::authorise` on the local resolver's book: exactly `verify` for a name with no succession log), so a resource of a rotated name is stored when its binding record is signed by the current key and refused when it is signed by a superseded key, a retired or compromised name stores nothing, and a name with no log is unaffected. `publish_resource` accepts the succession log beside the record's fields, as `publish_name` does (`succession_log`), offers it to the local resolver before the store judges the record, and replicates the record with the log; a log for another name is `record_rejected`, and a record the store's check refuses is `publish_failed`, as a bad signature there always was. The record is judged once, at storage: loading and reading check the resource id only, so bytes stored before a name rotated stay readable by id, and a blob is charged to the name's inception key whichever key signed. Owner renewal now renews a rotated name when the node key is the name's current key (`renewed_current`, with the head claim, published with the log); a name whose inception key is the node key and that has rotated is still reported and not signed. Not done: the gateway resource routes, the agent client (`publish_resource`, `read_resource`, `fetch_resource_with`) and the CLI `upload` and `fetch` still verify under the key in the name, so the SDK cannot publish or fetch a rotated name's resource; the acceptance test sends the commands raw (`docs/spec/02-names.md` sections 3.5 and 10.11, decisions 37 and 40 to 44; `docs/spec/04-objects.md` section 4.4).
- **Node-side authority gate (library only, not on the wire).** `spine-core` gains an `authority` module built on `spine-authority`: `classify` maps each of the 19 commands to an action, an effect class and a resource in the node's own tree (owner the node's key, path `["node", <area>]`, nine areas) with an exhaustive match, so a new command does not compile until it is classified; `AuthorityGate::authorize` runs the eleven verification steps on an encoded chain and invocation, then requires the invocation to name the command's action, its effect class or a higher one, exactly its resource and the SHA-256 of the command body, and charges a shared in-memory ledger, all under one lock, so concurrent requests cannot reuse a nonce or pass a limit; a refusal exposes a step number only; `AuthorityGate::issue_self_root` issues the single-user root grant and `receipt` signs a receipt. New `[authority]` configuration section (`mode`, `trusted_roots`, `skew_secs`, `max_invocation_age_secs`); `mode = "required"` is refused at startup. Not connected: spine-wire v1 cannot carry a grant chain, so the daemon does not call the gate and every connected client keeps full control of the node. Also not done: session binding, `did:`-rooted resources named by `publish_name` and `publish_resource`, a durable ledger, revocation feeds, receipt distribution (`docs/spec/03-authority.md`, "Node enforcement (not yet on the wire)", P28 to P36).
- **Key succession wired into storage, admission, the mesh and the CLI.** Every `RecordStore` now holds a `LogBook` (`spine-name`, new `logbook` module): per name the best verified succession log, replaced only by a valid strict extension, and an `Equivocation` proof when a valid conflicting log is offered, after which the name fails closed for good. Bounds: 1024 logs and 256 proofs; a log is evicted only if no record of its name is held (live before retired, oldest first) and proofs are never evicted. `LocalResolver` persists logs and proofs beside the records (`spine-name-successions-v1`, `spine-name-equivocations-v1`) and validates them at load. Admission (`RecordStore::put`, `LocalResolver::publish`, `cache_record`, restore, `NameService::publish` and `accept_announcement`, `Lookup`) is `LogBook::authorise`: exactly `NameRecord::verify` for a name with no log, and `record_authorised` against the held log otherwise, with the new `NameError` variants `SupersededKey`, `Retired`, `Equivocated`, `NeedSuccessionLog`, `SuccessionRefused`, `InvalidSuccessionLog` and `SuccessionCapacity`. Records a newly adopted log no longer authorises are dropped at once; a retired or compromised name resolves as that, not as not found. The mesh carries the log with the record in optional fields (`AnnouncedRecord.log`, `ResolveResponse.logs`) that are absent for names without a log, so existing encodings and envelope signatures are unchanged; receivers bound a log before verifying it and count what they drop (six new `ResolverMetrics` counters). `NameRecord::sign_current` and `renewed_current` sign with the current key and add the `spine.succession` head claim (`sign` is unchanged), `SuccessionLog::rotated` and `retired` build the next entry, and owner renewal reports a rotated name instead of signing with a superseded key. New offline commands `spine name rotate`, `spine name retire` and `spine name verify`, and `--log` on `create`, `renew`, `revoke` and `publish`. Not yet connected: the daemon ignores the log `publish --log` sends and so refuses a rotated name's record, the resource paths still verify against the inception key, there is no fetch on `NeedSuccessionLog` and no gossip of entries or proofs (`docs/spec/02-names.md` section 10.11, decisions 26 to 39).
- **Key succession for names (library only, not wired into resolution).** A new `succession` module in `spine-name` defines identity that survives a key change: a name's authority stays its inception key, and a signed succession log of at most 64 entries (`Succession`, `SuccessionLog`) says which key currently signs. Each `rotate` entry reveals the new key, commits (SHA-256 under a domain string) to the next one and is signed by the key current before it; a `retire` entry revokes the name permanently. `SuccessionLog::verify` replays a log under strict Ed25519 and returns the state (current key or retired, index, head hash); `detect_equivocation` returns a checkable `Equivocation` proof when two valid logs fork; `record_authorised` is the pure rule that a record must be signed by the log's current key (a superseded key is refused whatever the record's remaining lifetime), with the optional `spine.succession` meta head claim and a distinct `NeedLog` outcome. `NameRecord::verify_with(key)` is added and `NameRecord::verify` is unchanged. Names with an empty log behave exactly as before. `docs/spec/02-names.md` section 10 specifies the layouts, the inception weakness (entry 1 is authorised by the inception key alone), the threat analysis, 16 provisional decisions and the wiring that remains; `scripts/names_v1.py` and `docs/spec/02-names.vectors.json` carry independent vectors (9 logs, 26 refused logs, 9 equivocation cases, 20 record cases), checked by `src/spine-name/tests/vectors.rs`.

- **`spine-authority`, signed revocation records, signed receipts and an atomic usage ledger (layer 3, part two).** Pure library additions, no I/O and no new dependencies. `Revocation` (domain `spine-authority-revocation-v1`) is a signed statement by the issuer of a grant, or of any grant above it in the chain, revoking one grant id; step 8 checks the revoker against the chain being verified, so a record by anyone else is ignored. `FeedHead` (signed by one trusted publisher: sequence, `as_of`, count, digest over the sorted record ids) and `RevocationFeed` load a set only if it matches the head, the head is from the trusted publisher, its sequence advances past the caller's last-seen value and it is not dated in the future; a feed that does not load is no data and step 8 refuses it, and freshness is compared with the per-effect-class staleness bounds (default 0). `Receipt` (domain `spine-authority-receipt-v1`) is signed by the party that performed the invocation and binds the invocation id (`Invocation::id`), the last grant, the decision, the amounts charged and a result digest; a denial receipt carries the step number only. `MemoryLedger::charge` is now atomic across the chain and the nonce (it returns `Result<(), ChargeError>`), implements the rolling rate window from the time passed in, and is bounded: a nonce is kept until `Decision::nonce_horizon`, so eviction never admits a replay inside the freshness window, and a full table refuses instead of forgetting. `RevocationView` gains `chain_revoked` (default: ask `is_revoked` per id) and `Decision` gains `chain_limits`, `chain_expiries` and `nonce_horizon`. The spec adds normative sections and provisional decisions P19 to P27 (no open question is resolved); `scripts/authority_v1.py` and `docs/spec/03-authority.vectors.json` gain revocations, feed heads, feed-loading cases, step-8 cases against a feed, receipts with verify and match cases, and ledger cases.
- **Record lifetime bound and name revocation (tombstones).** A record's `ttl_secs` is at most `MAX_RECORD_TTL_SECS` (604800, 7 days; it bounds how long a withdrawn or compromised record can live on a replica that never hears the revocation), enforced by `NameRecord::sign` and by `verify`, and a verifier with a clock refuses a record whose `published_at` is more than `MAX_CLOCK_SKEW_SECS` (300) ahead of it, so a signer cannot extend a record's life by post-dating it. `NameRecord::validate(now)` is the full admission check (size, lifetime, clock) and is applied at the record store, the local resolver (publish, cache, restore from disk), the mesh service (publish, announcements, lookup responses), the gateway resource routes, the CLI and the agent client. An owner withdraws a name with a tombstone: an ordinary `NameRecord` for the same name with a higher `seq`, no endpoints, content or links, the reserved meta key `spine.revoked` = `1` and a `ttl_secs` of exactly the maximum (`NameRecord::tombstone`, `is_tombstone`). The signing layout is unchanged (still `spine-name-record-v2`), so existing records and signatures are unaffected. A tombstone supersedes lower-`seq` records (and beats a same-`seq` ordinary record), replicates through the ordinary announce path, is answered by `LocalResolver::resolve` as `NameError::Revoked` (distinct from `NotFound`), and blocks a record of the same or lower `seq`; owner renewal never revives it. Revocation lasts until the tombstone expires and is not permanent (see `docs/spec/02-names.md` sections 3.7 and 9). New command `spine name revoke <record> --key <key> --output <file>`. Vectors: tombstones, a maximum-lifetime record, over-long and malformed tombstones in `invalid_records` (now with a `reason`), and a new `admission` section for the clock bound.

- **`spine-authority`, capability grants (layer 3, part one).** A new pure library crate (`src/spine-authority`: no I/O, no clock, no tokio; depends only on `spine-name`, `spine-cbor`, `serde`, `ed25519-dalek`, `sha2`, `thiserror`) implementing the draft design in `docs/spec/03-authority.md`: signed `Grant`s (Ed25519 `verify_strict`, SHA-256 ids, one canonical deterministic-CBOR encoding with domain-separated signed bytes, closed structs and explicit size bounds), `Chain`, `Invocation`, attenuation with a segment-wise resource containment rule and `is_attenuation_of` naming what was broadened, issuing that refuses to sign a broadening grant, and `verify`, the eleven verification steps in spec order returning the first failing step as a `Denial`. Revocation and usage are traits (`RevocationView`, `UsageLedger`) whose defaults fail closed. Tests: a negative test for every denial, a seeded property test that nothing that verifies exceeds its root, encoding and bounds tests, and independent vectors (`scripts/authority_v1.py` generates and checks `docs/spec/03-authority.vectors.json`; `src/spine-authority/tests/vectors.rs` checks the crate; the CI `wire-vectors` job checks the script). Choices the draft left open are listed under "Provisional decisions" in the spec; none of its open questions is resolved. Not wired into the node, mesh, SDK or CLI, and not externally reviewed. The root workspace has 20 crates.
- **Independent test vectors for the name layer.** `scripts/names_v1.py` (standard-library Python, with its own Ed25519 checked against RFC 8032 section 7.1) generates and checks `docs/spec/02-names.vectors.json`: keys, name canonical forms and routing keys, resource ids, and signed name records with 43 rejected variants; `src/spine-name/tests/vectors.rs` checks the crate against them and the CI `wire-vectors` job checks the script. Two fixes come with them: base32 decoding now refuses non-zero trailing bits, and `Rel::Other` with a built-in relation's name deserializes to that built-in.
- **`spine-cbor`, a leaf crate for deterministic CBOR.** The module that was `spine_protocol::cbor` (RFC 8949 section 4.2, no tags, depth at most 32) moved verbatim, with its tests, into `src/spine-cbor`, which depends only on `ciborium`, `serde` and `thiserror`, so crates that must stay light (`spine-name`, `spine-session`) can use the project's one canonical encoding without the protocol stack. `spine-protocol` re-exports it with `pub use spine_cbor as cbor;`, so every `spine_protocol::cbor::*` path is unchanged. The root workspace has 19 crates.

- **Conformance vectors for spine-wire v1 (wire step 14).** `docs/spec/00-wire.vectors.json` holds 88 valid and 106 invalid CBOR bodies, 54 valid and 50 invalid frames (plus 6 incomplete ones) and 50 handshake cases, with exact hex and, for every rejection, the `CborError` or `FrameError` variant and the close code. `scripts/wire_v1.py` is a standard-library-only Python implementation written from the specification text that generates the file and re-checks every vector (`generate`, `check`, `decode-body`, `decode-frame`); it reproduces the worked examples of spec C.12 and the three hand-checked bodies of `tests/bodies_v1.rs` byte for byte. `tests/conformance_vectors.rs` runs the vectors against `spine-protocol`, and the new `wire-vectors` CI job runs the Python check and fails if the committed file drifts from the generator. A `.gitattributes` entry keeps the file LF on every platform. Spec text and implementation differ in a few places (C.7 against C.4 for the close code of an oversize declared length, the silence of C.6 on invalid HELLO codes and extension order); the vectors follow the implementation and say so in a `note`.
- **Bindings, scripts and documentation brought in line with spine-wire v1 (wire step 13, `bindings-scripts-docs`).** `src/spine-ffi/include/spine.h` already matched the 15 exported functions one for one and only got clearer comments; `src/spine-go` (which wraps the C ABI, not the wire) lost its stale description and hard-coded version assertion and now says that `Connect` is plain TCP and that the stable error code is not exposed; `src/spine-python` documentation states its error format; `fuzz/` targets use `FrameCodec::new(limit)`, fuzz the v1 frame decoder and have `[[bin]]` entries for every target; `extension/README.md` states that nothing listens on `ws://localhost:9800/ws`. `scripts/release_gate.py` no longer passes the pre-v1 total `1478` to `verify.sh` (set `SPINE_EXPECTED_TESTS` after a fresh run), `scripts/deploy.sh` and `demo.ps1` were corrected, `test_list.txt` and `build_output.txt` are marked stale. README, ARCHITECTURE, SECURITY, THREAT_MODEL, ROADMAP, HANDOFF, PUBLISHING, OPTIMIZATIONS, BENCHMARK_REPORT, SECURITY_AUDIT, MATHEMATICAL_PROOFS, HLS_SPEC, `.github/copilot-instructions.md`, `docs/`, the mdBook sources, the spec notes and crate READMEs now describe the v1 wire, the 19 commands and the error codes, and state that confidentiality on the native path is TLS only. Benchmark figures, test counts and sizes measured on the earlier wire are kept only with a note that they predate v1. The generated `docs/book/` was not rebuilt and is stale. Documentation, scripts and non-compiled bindings only; no behaviour of workspace code changed.

- **Native wire v1 modules in `spine-protocol`, not yet used by the daemon or the SDK.** `cbor` (deterministic CBOR scanner, encoder and decoder), `frame` (the frame codec and `CloseCode`) and `hello` (`Hello`, `HelloAck` and `Close` bodies, their validation, `negotiate` and conversions to and from `Frame`), following `docs/spec/00-wire.md`. All three are additive: no existing path or wire encoding changes. Removed: the `negotiation` module (JSON `VersionOffer`, `VersionResponse`, `ProtocolFeature`), which had no caller on any wire; its only user was the `fuzz_negotiation` fuzz target, now retargeted at the new handshake code. (Superseded by the bullet "Breaking: the native wire is now spine-wire v1" below: the daemon and the SDK use these modules now.)

### Changed

- **Breaking: records with `ttl_secs` above 604800 (7 days) are refused, and `published_at` more than 300 s ahead of the verifier's clock is refused.** `NameRecord::sign` fails with `TtlTooLong`, `verify` fails for such a record even if its signature is valid, and persisted records written with a longer lifetime are skipped on restore. API: `RecordStore::put`, `NameService::publish` and `NameService::accept_announcement` take `now`; `NameError` gains `TtlTooLong`, `PostDated`, `MalformedTombstone` and `Revoked`; `NameRecord::supersedes` orders a tombstone above an ordinary record of equal `seq`; `NameRecord::renewed` refuses a tombstone; `LocalResolver::publish` and `cache_record` now also apply the size limits. `spine name create` and `renew` refuse a `--ttl` above the maximum. The numeric-extremes test vector now uses the maximum ttl.

- **Breaking: mesh agent ids are derived from the public key (2026-11-21).** `AgentId::from_public_key(key)` is the first 16 bytes of SHA-256(`"spine-agent-id-v1\0"` || Ed25519 key). `SigningIdentity::new` and `from_seed` now set it, so `envelope.from` is a function of the signing key; before, `from_seed` hashed the secret seed and `new` was random. `MeshNameResolver` refuses and counts (`ResolverMetrics::agent_id_mismatches`, plus `greetings_rejected` for hellos and acks, `envelopes_unauthenticated` for `Application`) any hello, ack or `Application` envelope whose `from` is not derived from its key, drops referrals that pair an id with a key it does not derive, and refuses `register_peer` for such a pair; `authenticate_sender` ignores recorded pairs that do not derive. The TCP, WebSocket and QUIC transports drop such pairs in `learn_peer` and fail a dial whose peer proves a key that does not derive the agent id dialed (`id_mismatches()`). `SigningIdentity::from_agent_id` still attaches an arbitrary id to a fresh key but its result is not self-certifying and the hardened handlers refuse it: use it only to build an impostor in tests. `AgentId::new()` is unchanged for keyless agents outside the mesh. The node key file holds the seed only, so every existing node's agent id changes on upgrade, and peers that recorded the old id for it must re-learn the pairing; a mixed fleet of old and new nodes cannot greet each other.
- **Breaking: the resource store keeps bytes, verifies once, and accounts per publisher (2026-11-21).** Blobs are stored as raw bytes (version byte, big-endian u32 content type length, content type, data) in namespace `spine-resources-v3`, with a 57-byte accounting row (publisher key, size, stored-at, referenced-until) in `spine-resources-meta-v3`; the JSON and integer-array forms are gone. There is no migration: resources stored by earlier builds in `spine-resources-v2` are not read, and must be published again. A read hashes a blob the first time the process serves it rather than on every 64 KiB chunk; the client check of the assembled object against the signed id is unchanged. The 256 MiB quota now counts accounted bytes and is shared fairly: a publisher at 64 MiB can evict only its own unreferenced blobs, a smaller one displaces the largest holder oldest unreferenced blob, and a blob stored with an unexpired record, or listed by the daemon reference source, is never evicted. `ResourceStore` gains `put_as`, `put_at`, owner-checked `delete`, `held_bytes` and `total_bytes`; `put` and `put_tracked` store for a fixed local owner. Startup drops any blob row that does not hash to its key. See `docs/spec/04-objects.md` section 4.4.

- **Mesh identity: QUIC keeps the authenticated peer, dials are pinned, the replay guard is keyed by the verified key (2026-11-21).** On an authenticated `QuicNameTransport` the first-stream handshake's key is kept for the life of the connection and passed to `handle_envelope_from_peer` for every later stream, a connection that fails the handshake is closed, and `Application` RPC now works over QUIC when the handshake proved the call's signed `recipient_key`; the handshake is still not bound to the TLS channel (`spine-session` has no input for it and is unchanged). Every authenticated dial now carries an expected key: hellos, acks and referrals record the node id (the peer's Ed25519 key) with `NameTransport::learn_peer`, a bare seed address is accepted on first contact and its key recorded and required afterwards (in memory), and `namespace.seed_keys` configures a seed's key from the first dial. A pin mismatch is `NameMeshError::PinMismatch`, logged and counted, never a fallback to unpinned. `ReplayGuard` is keyed by the signing key and bounded per key. `ReplyPath::Plain` can carry an authenticated peer key. Trust table: `src/spine-mesh/README.md`.
- **Daemon wire defects from section 16 of the specification.** Error codes no longer come from matching text: the fetcher returns a typed `FetchError` and `ResourceStore::read` a `ReadError`, and the daemon maps the variant. New listed codes `outbound_url_limit`, `outbound_dns_limit` and `outbound_http_status` (the status is in `details.status`) replace the old collapse into `navigation_failed`, which is retired (47 codes). `get_ur` on a page that fails to parse answers `parse_error`, not `no_page_loaded`; `tool_call` checks the tool name before its arguments. The daemon sends `CLOSE(shutting_down)` to idle connections on shutdown and after a handshake that completes while draining; `session_capacity` no longer ends the connection; a write that times out abandons the connection without a `CLOSE` (`Connection::abandon`). The session rate limit is per connection and applies before a session is created. HELLO, HELLO_ACK and CLOSE refuse a non-text map key (a byte-string key used to be accepted as a field name), with three new handshake vectors, and `hello.rs` shares `wirefmt::reject_null`.
- **Fixed: mesh handlers trusted `envelope.from`, an unauthenticated claim.** A `NameResolveResponse` was absorbed on the strength of `from` alone, and its unsigned `closer` referrals steer a lookup, so anyone able to deliver an envelope claiming to be an asked peer could redirect it. It is now absorbed only if the envelope signature verifies under a key this node recorded for that agent id (`register_peer`, hello or ack) and, on a session-authenticated connection, under the session key; otherwise it is dropped and counted in the new `ResolverMetrics::envelopes_unauthenticated`. A `NameHello` or `NameHelloAck` on an authenticated connection must now carry the session key (counted in `greetings_rejected` otherwise), and an `Application` request answered by agent id (no connection to reply on) now requires `from` bound to the signing key. `NameAnnounce` is unchanged by design (records are self-certifying; `from` is never read). New API: `MeshNameResolver::handle_envelope_from_peer`, `NameService::{keys_for_agent, on_response_from_keys}`; `pump` passes the connection's authenticated key. **Behaviour change:** in-process drivers that deliver a response or an addressed application request from a sender they never registered now see it dropped. `NameService::on_response_from` still takes an agent id at its word and is documented as unsafe for wire input. The mesh trust model (every payload variant against every entry point, what holds and what does not) is in `src/spine-mesh/README.md`.
- **Removed: `MeshNode::process_envelope`, `MeshAction`, `MeshNode::{trust_key, take_inbox}`.** `process_envelope` verified a signature only for senders in `trusted_keys` and passed every other sender through unverified, and it had no caller outside its own tests (searched `src/` and `research/`). The `trusted_keys` map and the local inbox went with it. Nothing dispatched the other payload variants (gossip, `PeerAnnounce`, `KnowledgeSync`, swarm, route) before or after.
- **Fixed: the QUIC client accepted any server certificate.** `SkipServerVerification` is removed from `spine-protocol`. `QuicEndpointBuilder::build_client` now verifies against the web PKI roots (new optional dependency `webpki-roots`, part of the `quic` feature), and `build_client_with_roots`, `build_client_trusting`, `client_config_with_roots`, `build_server_with_cert` and `build_server_self_signed` are added, so a client can pin a server certificate the way the TLS client takes a `RootCertStore`. **Behaviour change:** `QuicNameTransport` trusts no certificate until `trust_certificate` is called, and `serve_quic_node` now returns `(SocketAddr, CertificateDer)` so the listener certificate can be handed to peers; `connect_auto` over QUIC refuses a self-signed server (and `Auto` falls back to TCP). The mesh handshake still is not bound to the TLS channel.
- **`docs/spec/00-wire.md` is now the normative spine-wire v1 specification.** It is rewritten from a design document to the rules the code enforces (each citing file and function), with the receive check order, the 19 commands and their fields, all 45 error codes, limits and config keys, and a list of known deviations; the pre-v1 description, the migration record and the open questions are appendices. `scripts/wire_v1.py` notes no longer record disagreements with the text, and six vectors were added (valid frames 56, handshakes 54). `docs/src/architecture/protocol.md` error table corrected (`overloaded`, `invalid_request`, `navigation_failed`, `capability_denied`, `no_page_loaded`, session codes). Documents and the Python reference only.
- **Breaking: mesh envelope signature v3.** `MeshNode::signature_data` no longer hashes `serde_json::to_vec` output. The signed bytes are the domain `spine-mesh-envelope-v3\0` followed by the deterministic CBOR of one array `[id, from, to, timestamp_secs, timestamp_nanos, payload]` (`id` and `from` as 16-byte byte strings), so they are a function of the values and not of JSON text. There is no v2 fallback: an envelope signed under v2 fails verification, and a node running this version cannot exchange verified envelopes with an older one. An envelope that cannot be encoded (nesting deeper than 32) is not signed and does not verify. `serde_json` is now built with `float_roundtrip` (workspace dependency, both workspaces), so a float in a `serde_json::Value` re-parses on the receiver to the sender's exact `f64`; the cost is slower float parsing. `ttl` and `hops` remain unsigned. The wire form of an envelope is still JSON.

- **Removed: `SigningIdentity::sign_message` and `SignedEnvelope`.** They signed raw `serde_json::to_vec` output with no domain string and had no caller outside their own tests. `SigningIdentity::sign`/`verify` and `PublicIdentity` are unchanged.

- **Breaking: spine-wire v1 bodies (wire step 11, `bodies-v1`).** The frame header's request id is now the only correlation id: `Request`, `Response`, `ToolCall.id`, `CapabilityQuery.id`, `CapabilityAdvertisement.id` and the `id` of every stream body are gone, as are the `ToolResult` type and the SDK's `request_counter` ids. A `REQUEST` body is one command from a closed set of 19: `navigate`, `get_ur`, `get_raw_html`, `execute_binary`, `store_knowledge`, `query_knowledge`, `get_binary_capabilities`, `resolve_name`, `resolve_names`, `find_providers`, `publish_name`, `fetch_name`, `crawl_names`, `publish_resource`, `read_resource`, `namespace_status`, `mesh_call`, `tool_call` and `capability_query` (a text item for a command without fields, otherwise a single-entry map `{name: fields}`). A `RESPONSE` body is `{ok: result}` or `{err: {code, message[, details]}}`. Every name is an explicit `#[serde(rename)]` snake_case string, and `spine_protocol::COMMAND_NAMES` lists them. Every body struct and struct variant is `deny_unknown_fields`; an optional field is omitted when absent and a present `null` is rejected. Byte fields (`read_resource.hash`, `mesh_call.peer_key`, `resolve_name` and `fetch_name` `if_none_match`, `publish_resource.data`, `SpineBinary.data`, `ResourceChunk.hash` and `data`, `TraceContext` ids) are CBOR byte strings, and a JSON array of integers is refused in their place. `SpineBinary.exported_functions` is a `BTreeMap` (was a `HashMap`), so equal binaries encode to equal bytes. `serde_json::Value` fields stay values and encode as CBOR; their integer and float widths follow docs/spec/00-wire.md C.5. `Message` is no longer a wire type: it is a Rust-side enum for recording and tests (`Command`, `Reply`, `Ping`, `Pong` and the four stream bodies).
- **Breaking: errors carry a stable code.** Every daemon failure is `{err: {code, message}}` with a code from `spine_protocol::body` (`ERR_*`, `ALL_ERROR_CODES`): `overloaded`, `unknown_command` and `invalid_request` from the protocol layer, and `internal`, `rate_limit`, `session_capacity`, `session_not_found`, `session_expired`, `no_page_loaded`, `navigation_failed`, `capability_denied`, `execution_failed`, `namespace_unavailable`, `invalid_peer_id`, `mesh_call_failed`, `batch_too_large`, `lookup_failed`, `lookup_unavailable`, `record_expired`, `malformed_record`, `record_rejected`, `publish_failed`, `resource_not_found`, `invalid_seed`, `invalid_args`, `unsupported_tool`, `parse_error` and the `outbound_*` family from the daemon. A body that does not decode answers `unknown_command` when it names a command outside the set and `invalid_request` for any other defect, and the connection stays open. The SDK returns a `spine_protocol::native::NativeError::Remote { code, message, details }` inside `anyhow::Error`; read it with `native::remote_code(&err)` and never match the text. `AgentClient::call_tool` returns `ToolOutcome`, `capabilities` returns `CapabilityAdvertisement`, and `mesh_call` surfaces a peer application's `{error: {code, message}}` as `Remote` too. `spine-grpc`'s `tool_result_to_proto` now takes the gRPC request id and a `ToolOutcome`, and its executor returns `ToolOutcome`. `maximum_upload_fits_the_native_frame_limit` is recomputed on the CBOR encoding, where the upload is a byte string and costs its length plus an envelope.
- **Breaking: the native wire is now spine-wire v1 (wire steps 10 and 12).** The daemon and every client switch in one change from the pre-v1 transport (a 5-byte frame header, a compression flag byte and an `SP`-headed body) to the v1 frame: a 15-byte header (length, version, type, flags, request id) after a `HELLO`/`HELLO_ACK` handshake, with deterministic-CBOR bodies. Peers on the old wire cannot talk to this daemon, and there is no fallback. Bodies are still the current `Message` types; `spine_protocol::native` is the one place that maps them onto frames: `Request`, `ToolCall` and `CapabilityQuery` travel as `REQUEST`, the daemon's `Response`, `ToolResult` and `CapabilityAd` as `RESPONSE` with the same frame id, and `Ping`/`Pong` are the `PING`/`PONG` frames carrying an unsigned-integer token (the SDK's millisecond timestamp). The `id` fields inside bodies are carried but not cross-checked; the frame id correlates. Nothing produces the stream messages, so they are not mapped. The daemon negotiates version 1, a 16 MiB maximum frame and `max_inflight` of 1, answers a request it cannot decode with an error `Response` instead of dropping the connection, and closes a connection that sends no `HELLO` within the new `server.hello_timeout_secs` (default 10, `SPINE_HELLO_TIMEOUT`) or no frame within `idle_timeout_secs` with `CLOSE(timeout)`; a `PING` now restarts the idle deadline, and `Connection::recv_timeout` is the new idle-deadline receive. `AgentClient` holds a `Connection` in its public field `conn` (was `handler`), its constructors perform the handshake and report a server `CLOSE` with its code, and `AgentClient::over` runs the handshake on any stream. Frame compression is off by default (the old transport compressed every payload of 64 bytes or more). Removed: `ProtocolHandler` and `handler.rs`, the `wire` module (`encode`, `encode_compressed`, `encode_json`, `decode`, `SpineWireHeader`, the `FORMAT_*` codes and the headerless-JSON fallback), `replay::ReplayVerifier::replay_sends` (no callers) and the unused `spine-mesh` `signed_frame` module with its re-export from `spine-agentic`. `research/spine-protocol-research` keeps its own private size limits and encodes bodies with `cbor::to_vec`. The `chaos`, `e2e_protocol` and `integration` tests, the wire-size example and the hot-path bench now drive `Connection`; `wire_encoding` measures `cbor::to_vec` and the frame layer's compression.

- **Removed: 8 `Message` variants, the daemon latent stream, and the neural-codec types from `spine-protocol` (wire step 8).** `Message` is now the twelve variants `Request`, `Response`, `Ping`, `Pong`, `ToolCall`, `ToolResult`, `StreamStart`, `StreamToken`, `StreamEnd`, `StreamCancel`, `CapabilityQuery` and `CapabilityAd`. Deleted: `Event` (and the `Event` struct) and `BinaryProgram`, which nothing produced; `Sync`, `Speculative`, `PreComputed` and `MorphRequest` were already gone. Moved out of core into `research/spine-protocol-research` and recorded there as extension candidates: `LatentMessage`, `Encoded`, `CodecAd`, `CodecNegotiation`, `EmbeddingRequest` and `EmbeddingResponse`, together with `StreamData::Encoded` and `StreamStart::stream_codec`. The module `spine_protocol::agentic_codec` is gone: `LatentVector`, `EncodedFrame`, `EncodedMetadata`, `Modality`, `DType`, `CodecDescriptor`, `CodecDirection`, `CodecAdvertisement`, `CodecNegotiation`, `EmbeddingRequest`, `EmbeddingInput`, `EmbeddingResponse`, `CodecError`, `NeuralCodec` and `CodecRegistry` now live in the research crate, which also took the `LatentVector` property tests. `DecodeHints` stayed in core (`agentic.rs`). The daemon no longer streams latent vectors after an `ExecuteBinary` response (`handle_command` returns a `Response`), and the SDK lost `start_listener`, `subscribe_latent` and the `LatentMessage`/`Event` forwarding. `spine-protocol` dropped the dependencies `bincode`, `bytemuck` and `parking_lot`; the research crate gained `bincode`, `bytemuck` and `thiserror`. The `fuzz_latent_vector` target and the `docs/src/sdk/neural.md` page are deleted, the `hot_path_bench` latent group is gone, and `wire_encoding`, `wire_sizes` and `hot_path_bench` now use a `StreamToken` carrying `StreamData::Bytes` as the tensor-heavy frame. In `spine-gateway` the codec registry, `POST /v1/embeddings` and `GET /v1/agentic/codecs` are compiled only with the `research` feature; a gateway built without it keeps both routes and answers each with `503 Service Unavailable` and a message to rebuild with the feature (before: `/v1/embeddings` answered 503 `no codecs registered` and the codec list was empty). (Partly superseded by the "Breaking: spine-wire v1 bodies" bullet above: `Request`, `Response` and `ToolResult` were removed afterwards, so `Message` is smaller than the twelve variants listed here.)
- **Removed: 17 `BrowserCommand` variants, and the SDK, daemon, CLI and binding code that only served them (wire step 7).** `Click`, `Type`, `HandleEvent`, `Search`, `DeleteKnowledge`, `GetSessionHistory`, `SwarmSearch`, `DelegateTask`, `ProposeKnowledge`, `CreateSwarmPlan`, `ExecutePlanTask` and `SendSpeechAct` are deleted (stubs, no sender, or refused in every build). `GetLatentUR`, `TransferSession`, `SetAutonomousMode`, `NeuralTransmit` and `GetAgenticState` leave the core command set for the future research extension `nervosys.org/research-session/1`; they have no carrier until the wire cut-over, so they are removed from the daemon too and recorded, with their old payloads, in `research/spine-protocol-research/README.md`. `GetCapabilities` is renamed `GetBinaryCapabilities` (SDK `AgentClient::get_binary_capabilities`). `BrowserCommand` now has 17 variants. `AgentClient` loses `click`, `type_text`, `handle_event`, `search`, `delete_knowledge`, `get_history`, `get_latent_ur`, `transfer_session`, `set_autonomous_mode`, `swarm_search`, `delegate_task`, `propose_knowledge`, `create_swarm_plan`, `execute_plan_task`, `transmit_neural`, `get_agentic_state`, `send_speech_act`, `enable_human_mode` and the public `neural_protocol` field; the daemon no longer answers `unsupported_without_research`, `unsupported_legacy_swarm` or `unsupported_speech_act`, drops the autonomous loop, the per-session command history, `last_command` and `autonomous_mode`, the `HlbRuntime` interpreter in `vdom.rs` (its only caller was `HandleEvent`), and the dashboard's Autonomous column. `spine-core` no longer depends on `spine-neural` or `spine-agentic` (its `research` feature is now `spine-cluster` and `spine-human`); `spine-agent`'s `research` feature no longer pulls `spine-human`. The cluster node's `supports_chameleon` capability is now `false`. The CLI loses `spine query search`, the REPL `search` command and the `swarm.rs` scaffold example, and `spine benchmark` its latent-vector measurement; the gateway loses `POST /api/sessions/{id}/search`; `spine_search` leaves the C library and header, `Client.Search` the Go binding, and `click`, `type_text`, `get_latent_ur` and `search` the Python module. The examples `latent_analysis` and `session_transfer` are deleted. `research/spine-browser` (not built by CI) loses the search, swarm, consensus, planning, neural, comms, latent and history panels, the Auto toggle and its dependencies on `spine-agentic` and `uuid`. The SDK, FFI, Go and Python names `get_capabilities` / `spine_get_capabilities` / `GetCapabilities` are unchanged for now.
- **Removed: every research dependency of `spine-protocol`, and the pre-shared-key AES layer.** `spine-protocol` no longer has a `research` feature and depends on no crate under `research/` (not even as a dev-dependency). `src/research.rs`, `src/research/evolution.rs` (`ChameleonKey`, `SpineConnection`, `connect_auto`, the protocol evolution framework), `TitansLatentCodec`, the `neural_codec_bench` bench, the `chameleon_demo` example, the chameleon property tests and the `spine-nostd`/`spine-embedded` frame-interop tests moved to the new, experimental and unreviewed `research/spine-protocol-research` crate, which uses only public `spine-protocol` items (two small private helpers, `fresh_nonce` and `decode_zstd_limited`, are copied there). Deleted outright: `ProtocolHandler::enable_chameleon`, `enable_chameleon_aead`, `enable_encryption`, `send_decoy` and the handler's cipher, nonce, latent-AEAD and moving-target fields, with the tests of the PSK and chameleon paths (`protocol_encrypted_*`, `e2e_encrypted_*`, `chaos_encrypted_connection_drop`, `chaos_mismatched_encryption_keys`, `protocol_chameleon_encoding`, `e2e_chameleon_tcp_send_path`) and the hot-path bench cases for them; the handler now writes the `SP` body, a flag byte (zstd above 64 bytes) and the fixed 5-byte frame, with no sealing. Unused `spine-protocol` dependencies dropped: `aes-gcm`, `hkdf`, `hmac`, `sha2`, `rand`, `zeroize`, `bytes`, `log`, `tracing`, `tokio-test`. `spine-core`, `spine-agent` and `spine-web` no longer forward `spine-protocol/research`; `spine-gateway`'s `research` feature now enables the optional `spine-protocol-research` dependency for `TitansLatentCodec`. In `spine-agent`, the `enable_chameleon*` calls left the examples `simple_agent`, `encrypted_agent` (now a TLS demo), `agent_autonomous`, `research_swarm` and `offline_demo`, and `web_intelligence_demo` lost its moving-target benchmark. The browser GUI's Decoy button is gone. CI drops the `spine-protocol --features research` clippy and test steps and the redundant `--no-default-features --features quic` clippy run.
- **Removed: the speculative layer of the native wire.** Every default build and every `research` build already sent a full frame and never used a prediction, so the layer was dead code (docs/spec/00-wire.md A.5). `send_message` no longer wraps the `SP` body in a JSON `SpeculativeFrame`: it now emits the `SP`-header body directly, then the existing padding, compression flag, optional encryption and morphed header (what `send_message_raw` always did; `send_message_raw` is now a one-line call to `send_message`). `receive_message` no longer has the JSON-wrapper branch. Removed: `SpeculativeFrame`, `SpeculativePayload`, `Message::{Speculative, PreComputed, Sync}`, `PreComputedResponse`, `SyncPayload`, `SpeculationStats`, `MessageType`, the stand-in and the research `SpeculativePredictor`, `ProtocolHandler::{enable_speculation, precompute_response, speculate_responses, get_speculation_stats}` and the public `speculation_stats` field, `AgentClient::get_speculation_stats`, the daemon's per-request pre-computation, the `stats` command of `spine connect` and the speculation sections of `spine benchmark` and the `spine init` example. This changes the wire (the outer JSON array-of-integers wrapper is gone); no compatibility is kept. (Superseded by the "Breaking: the native wire is now spine-wire v1" bullet above: the `SP` body, the padding, the compression flag byte, the morphed header and `send_message` itself are gone with `ProtocolHandler`.)
- **Removed: protocol morphology and padding.** The frame header is now fixed at five bytes, `u32` big-endian length then `0x00` (before: `ProtocolMorphology` chose a 5 to 16 byte header, a byte order, a version byte and one of four padding modes, and changed them mid-connection with `Morph`/`MorphRequest`, under an all-zero evolution key unless a chameleon was set; the default bytes were already `length_be || 0x00`). Removes `ProtocolMorphology`, `PaddingMode`, `apply_padding`, `remove_padding`, `ProtocolHandler::morph_now`, `Message::MorphRequest`, `BrowserCommand::Morph`, `AgentClient::morph`, the daemon's 30-second monitor task, `needs_morph` and proactive `MorphRequest`, the `spine_protocol_morphs_total` metric and its Grafana panel, `spine_morph` in the C header and FFI, `Client.Morph` in Go, `morph` in the Python binding, and the `morph` command of `spine connect`. The research `SpineConnection` uses the same fixed header. Wire and API compatibility with earlier builds is not preserved. (Superseded by the "Breaking: the native wire is now spine-wire v1" bullet above: the five-byte header is gone; the frame header is 15 bytes.)
- **Fixed: task-store eviction was not fair across owners and ignored completion time.** When the 256-task store is full, the worker now evicts from the owner holding the most entries (ties to the smaller owner key), choosing that owner's terminal task with the earliest completion time, never one a pending or running task depends on. An owner already holding 64 or more entries can evict only its own finished tasks and is otherwise refused, so it cannot displace another owner's results. `TaskRecord` gains `completed_at` (optional, stamped when a task first becomes terminal, default `None` for older records). Knowledge entries are still never evicted; four free identities can still exhaust the 256 total, recorded as a Stage 6 admission-cost item.
- **Fixed: a resource's content type was not authenticated.** The type travelled beside the bytes under no hash or signature, so a serving peer could relabel verified bytes. A resource is now identified by `spine_name::resource_id(content_type, bytes)`: SHA-256 over `"spine/resource-id/v1\0" || u32_be(len(type)) || type || u64_be(len(bytes)) || bytes`, and a resource record's signed `content_hash` is that id. One function computes it for every publisher (SDK, CLI, gateway, origin) and `verify_resource` checks it on origin put, origin read, SDK `read_resource` / `fetch_resource` and gateway upload and download; failures are `NameError::InvalidContentType` or `NameError::ResourceIdMismatch`. Content types are exact-match canonical (1-256 printable ASCII bytes, no uppercase, no edge spaces). Breaking: ids of existing resources change, so records must be re-signed; the origin store namespace is now `spine-resources-v2` (old blobs are not read); `spine name create --resource` takes `--content-type` (default `application/octet-stream`) and `upload` must use the same one. Plain `content_hash` stays for `blob:` names and `spine.content/hash`.
- **The default root build is now free of research crates.** `spine-protocol` no longer enables its `research` feature by default (`default = ["quic"]`), and `spine-web` no longer enables `agentic` by default (`default = ["protocol", "transport", "agent", "name"]`). `cargo tree -e normal --workspace -i <crate>` finds none of the 14 research crates. To get the previous behaviour opt in: `spine-protocol = { version = "2", features = ["research"] }` restores `ChameleonKey`, `SpeculativePredictor`, `SpineConnection`, `connect_auto`, the protocol evolution framework, `TitansLatentCodec` and `ProtocolHandler::enable_chameleon*`; `spine-web` gains a `research` feature (all research crates except `gpu` and `k8s`, plus `spine-protocol/research`, `spine-agent/research` and `spine-core/research` where those are enabled) and `agentic` is now opt-in (`features = ["agentic"]` or `"research"`); `full` still enables everything it did. No research-workspace crate used a gated item, so none needed the feature. CI clippy-checks `spine-protocol` and `spine-web` with `research`, and the root `test` job runs `cargo test -p spine-protocol --features research --lib --tests` on Linux. `scripts/verify.sh` needs no new floor: no test target left the default run. (Partly superseded: `spine-protocol` has no `research` feature any more, so the opt-in to restore the previous behaviour described here no longer exists; see "Removed: every research dependency of `spine-protocol`".)
- **The research crates moved to a separate workspace.** `spine-agentic`, `spine-browser`, `spine-cluster`, `spine-crypto`, `spine-embedded`, `spine-gpu`, `spine-human`, `spine-k8s`, `spine-kernel`, `spine-knowledge`, `spine-mechgen`, `spine-neural`, `spine-nostd` and `spine-recursive` are now under `research/` (own `Cargo.toml`, `Cargo.lock`; build with `cd research && cargo build`). The root workspace keeps the 18 stack crates and excludes `research`. Stack crates still reach research crates only through opt-in `research` features and `spine-web` optional features, now by `../../research/...` paths. CI gains a `research` job; `scripts/verify.sh` covers the root workspace only (expected suite floor and ignored count lowered, not re-measured). `research/Cargo.toml` duplicates the root `[workspace.package]`, `[workspace.dependencies]` and profiles, which must be kept in sync.
- **The default `spine-gateway` is now research-free** (`default = []`; new opt-in `research` feature). It takes `spine-protocol` with default features off, so `spine-neural`, `spine-crypto`, `spine-agentic` and `spine-human` are not in its dependency tree. This changes the default gateway: the process-wide codec registry starts empty, so `GET /v1/agentic/codecs` returns an empty list, and `POST /v1/embeddings` answers `503 no codecs registered` (an omitted `model` now defaults to empty rather than the Titans codec id) until a deployment registers a codec on `agentic_sse::registry()`. Build with `--features research` to restore the pre-registered `TitansLatentCodec` and the Titans default model. CI clippy-checks the research build of `spine-gateway`.
- **The default `spine-agent` SDK is now research-free** (`default = []`; new opt-in `research` feature). `spine-agentic` and `spine-human` are optional dependencies and `spine-protocol` is taken with default features off, so no research crate (`spine-agentic`, `spine-human`, `spine-neural`, `spine-crypto`) is in `spine-agent`'s dependency tree. Without `research`, these are compiled out: `AgentClient::enable_human_mode` and the human-paced delays in `click` / `type_text`, the public `AgentClient::neural_protocol` field, `AgentClient::transmit_neural`, and `handler.enable_chameleon*` (the speculative predictor never runs, see the `spine-protocol` entry). Build with `--features research` to restore them. The examples `agent_autonomous`, `encrypted_agent`, `latent_analysis`, `llm_workflow_demo`, `offline_demo`, `research_swarm`, `simple_agent` and `web_intelligence_demo` now carry `required-features = ["research"]`. `spine-browser` enables `spine-agent/research`; `spine-compiler` and `spine-cli` take `spine-protocol` with default features off. CI clippy-checks the research build of `spine-agent`. (Partly superseded: the SDK methods named here for the removed commands, such as `morph` and `search`, no longer exist; `research` now only pulls in `spine-agentic` for the examples.)
- **Fixed: `BrowserCommand::Morph` desynchronised the framing.** The daemon morphed before sending the response and the SDK's `AgentClient::morph` never morphed its own handler, so the client read the response, and every later frame, with the old layout. The daemon now sends the response under the old framing and morphs after the send (closing the connection if the send fails); `AgentClient::morph` reads the seed from the response and morphs locally, and errors if the response carries none. (Superseded: the `Morph` command, `AgentClient::morph` and morphing itself were removed; see the morphology bullet above and the 17-variant removal bullet.)
- **The default `spine-core` daemon is now research-free** (`default = []`; `research` and `quic` stay opt-in features). This changes the behaviour of the default daemon: `GetLatentUR`, `Morph` (in the command dispatcher; the connection loop still morphs the framing), `TransferSession`, `NeuralTransmit` and `GetAgenticState` answer an error beginning `unsupported_without_research`; `Search` answers `"distributed": false, "status": "local_only"` instead of `true` and `"searching_cluster"`; `Navigate` stores an empty binary instead of the human transpile; and cluster swarm, the cluster event listener and the autonomous agent loop are not built. Build with `cargo build -p spine-core --features research` to restore them. `spine-cli` gains a `research` feature that forwards `spine-core/research` (off by default). `spine benchmark` skips its latent-vector measurement when the daemon does not support it, the SDK methods that send these commands document the requirement, `scripts/deploy.sh` builds with `--features research`, and CI now clippy-checks the research build of `spine-core`. (Partly superseded: `GetLatentUR`, `Morph`, `TransferSession`, `NeuralTransmit` and `GetAgenticState` no longer exist as commands, and an unknown command now gets `{err: {code: "unknown_command"}}`, not `unsupported_without_research`; `Search` was removed.)
- `ProtocolHandler`, `ProtocolMorphology`, `PaddingMode`, `SpeculationStats` and `MessageType` no longer need the `spine-protocol` `research` feature. The handler lives in a new ungated `handler` module and compiles with `--no-default-features`; the chameleon key and the speculative predictor plug in only when `research` is on (without it the handler never predicts, so it sends `Full` frames with `next_prediction_hash` 0, which a default peer reads unchanged). `ReplayVerifier::replay_sends` and the `chaos`, `e2e_protocol` and `integration` tests are ungated, and `spine-core` now takes `spine-protocol` with default features off, enabling its `research` and `quic` features through its own. With default features nothing changes: public paths and the wire encoding are the same. (Superseded: all of these were deleted; `ProtocolHandler` was replaced by `spine_protocol::connection::Connection`.)
- `mesh_quic` and `signed_frame` moved from `spine-agentic` into `spine-mesh`, which now depends on `spine-protocol` with default features off (only `quic`), so `spine-neural` and `spine-crypto` are still absent from `spine-mesh`'s dependency tree. `spine-agentic` re-exports both modules, so `spine_agentic::mesh_quic::..` and `spine_agentic::signed_frame::..` keep working; it drops its direct `quinn`, `rustls`, `spine-protocol` and `spine-transport` dependencies. No behaviour change.
- `spine-core` gets a `research` cargo feature (off by default, see above) that gates cluster swarm, the latent, morph and neural-transmit commands, agentic state, the autonomous agent loop and the human transpile on Navigate. Built without it, those commands answer `unsupported_without_research`, search is local only, and the daemon no longer depends on `spine-cluster`, `spine-human` or `spine-agentic`. It still reaches `spine-neural` and `spine-crypto`, now only through `spine-wasm`, which depends on `spine-protocol` with default features. (Partly superseded: the latent, morph and neural-transmit commands and `unsupported_without_research` errors are gone with those commands; the feature itself still gates the cluster node and the human transpile.)
- `spine-protocol` gets a `research` cargo feature, on by default, that gates its research code: `ChameleonKey`, the speculative predictor, `SpineConnection`, `connect_auto`, the protocol evolution framework and `TitansLatentCodec`. `spine-neural` and `spine-crypto` are now optional dependencies enabled by it, so `--no-default-features` (plus `quic` for `QuicEndpointBuilder`) builds the wire types, the wire codec, `Message`, `BrowserCommand` and the task types with no research crate in the tree. With default features nothing changes: public paths and the wire encoding are the same. This unblocks moving `mesh_quic` and `signed_frame` into `spine-mesh`. CI now clippy-checks the feature-off build. (Superseded by "Removed: every research dependency of `spine-protocol`" above: the feature no longer exists.)
- The production mesh stack (`identity`, `mesh`, `naming`, `naming_mesh`, `mesh_tcp`, `mesh_ws`, and `AgentId`) moved from `spine-agentic` into a new `spine-mesh` crate that depends only on `spine-name`, `spine-session` and `spine-transport`. `spine-agentic` re-exports all of it, so existing `spine_agentic::mesh::..` and `spine_agentic::AgentId` paths keep working. No behaviour change. `spine-agentic` drops its direct `spine-session` and `zeroize` dependencies.
- The session handshake moved from `spine-crypto` into a new `spine-session` crate that depends on no other workspace crate, so the mesh (`spine-agentic`) no longer pulls in the research crate for it. `spine_crypto::handshake` remains as a re-export. No behaviour change. `spine-crypto` drops its now-unused `thiserror` and `ed25519-dalek` dependencies.
- **A non-loopback `server.host` is refused at startup** unless `tls.mutual_tls` is enabled or `server.allow_unauthenticated_remote = true` is set. The native server has no per-client authorization, so any client that can connect controls the node. `spine init --tls` now writes the override explicitly with a warning.
- Name records, agent identities and the mesh handshake use strict Ed25519 verification. Conflicting records with equal sequence and publication time now have one deterministic winner, and the resolver cache no longer replaces a newer record with an older one.
- **MSRV raised from 1.88 to 1.89.** `rusqlite` 0.43 (bundling a `libsqlite3-sys` without RUSTSEC-2026-0459) requires it. Dependency upgrades also clear the other 33 advisories reported on 2026-11-19: Wasmtime 36.0.23, Rustls 0.23.46, Quinn 0.11.19, Zstd 0.13.5, and the gateway on Axum 0.8 with utoipa 6.

### Fixed

- **A full name store no longer reports a record published that it does not hold, and no longer keeps lapsed records against live ones (spec 02, 4.5, 5.1, decisions 47 and 48).** `NameService::publish` returned `PutOutcome::Rejected` as a success when the node's own `RecordStore` was full and the record was the furthest from home, so `MeshNameResolver::publish` replicated a record it did not hold and `publish_local` reported it stored; both now fail with `NameMeshError::Name(NameError::Capacity)`, as `LocalResolver::publish` already did, before any envelope is sent. `RecordStore::put` counted expired records that no sweep had removed against capacity, so a store full of lapsed records refused a live one; when full and a new name arrives it now first drops the records expired at the `now` it is given (an unexpired tombstone is never dropped by this step). `RecordStore::with_capacity(cap)` builds a small store with no home key (oldest-published evicted first). Tests: `spine-name` store, `spine-mesh` `naming` and `naming_mesh`.
- **Outbound fetcher hardening.** `navigate` could reach any port on any public address (a port scanner from the node's address), a slow-drip body held an admission permit for the whole request timeout, one session could take every permit, and `internal_origins` silently accepted a link-local origin such as the cloud metadata address. `[outbound]` gains `allowed_ports` (default `[80, 443]`; other ports answer `outbound_invalid_port` before DNS, except exact `internal_origins` exceptions), `read_timeout_secs` (default 10, per body chunk, answers `outbound_timeout`), `max_concurrent_per_caller` (default 4; the daemon keys it by session id and answers `outbound_busy`) and `allow_link_local_exceptions` (default false; configuration naming a literal link-local or unspecified exception is refused without it). The daemon logs a warning per configured exception. `Fetcher::fetch_text_for` is the per-caller entry point; `fetch_text` is unchanged. Operators who navigate to public hosts on other ports must list them in `allowed_ports`.
- **`execute_binary` is confined to what it declares and to bounded work.** A program's host values were unbounded (a string doubled per four instructions until the process aborted), `call` looped over a guest-supplied argument count of up to 2^31, host work was not metered, `DEFAULT_MAX_MEMORY_PAGES` was unused, and nothing consulted the declared capabilities. `spine-wasm` now keeps a host budget (`MAX_INSTRUCTIONS` 65,536, `MAX_STACK_DEPTH` 1,024, `MAX_VALUE_BYTES` 64 KiB, `MAX_LIVE_BYTES` 1 MiB, `MAX_ELEMENTS` 4,096, `MAX_EVENTS` 4,096, `MAX_ACTIONS` 64, `MAX_CALL_ARGS` 32, latent vectors 256 of at most 16,384 dimensions), checked before each allocation; exceeding one traps the guest and `execute` returns an error naming the budget. Every host call draws fuel (`HOST_CALL_FUEL` plus one unit per byte), a wasmtime `StoreLimits` limiter enforces `DEFAULT_MAX_MEMORY_PAGES`, and `WasmStats::memory_used_bytes` is the real linear-memory size. The daemon refuses an action whose capability the program did not declare (`navigate` needs `network`, `search` needs `search`, a knowledge write needs `storage`) with `capability_denied` before any effect; stores a program's knowledge write under the session's own id, as `store_knowledge` does (the program's key stays inside the entry, so a program can no longer write another session's list); caps a session at 256 state variables and 1,024 knowledge entries and a call at 4 navigations (`execution_failed`); and runs the program on a blocking thread behind four process-wide permits, answering `overloaded` when none frees within two seconds. A refused or failed program no longer replaces the session's current binary. `call("print")` logs at debug level instead of writing to the daemon's stdout. There is no wall-clock timeout: run time is bounded by fuel and the host budgets.

### Agent-web completion work

- Persist local publisher records and signed sequence state through the storage abstraction, restore fresh records after restart, and support opt-in node-owner renewal.
- Add bounded DNS bootstrap, aggregate local and remote capability providers, resolve batches with bounded concurrency, and return disconnected resolution as `Unavailable`.
- Add content-addressed origin storage, native resource upload/chunk reads, signed hash verification in the Rust SDK, namespace CLI operations, and HTTP resource access.
- Dispatch correlated native read-only HTML parsing and SHA-256 tools with input limits and typed failures.
- Advertise native tool schemas and add signed, recipient-bound mesh application RPC with key pinning and explicit worker allowlists. Persist owner-scoped read-only tasks, dependencies, cancellation, deadlines, duplicates, and immutable knowledge; add SDK and task CLI operations.
- Remove legacy timer-based swarm completion and simulated confidence votes. Unsupported planning/delegation and log-only speech delivery now return explicit errors; an unmeasured surprise metric is null. Legacy WASM and cluster logs omit private queries, values, goals, and reasoning payloads.
- Validate explicit configuration without silent fallback, pass full deployment configuration to managed children, drain on parent shutdown, restrict diagnostics to identifiers/counts, and bind gateway/metrics to the configured host.
- Add local native candidate archives with checksums, dependency inventory, source provenance, clean installation smoke tests, and ordered release gates.
- Replace implicit HTTP model echo with a configurable OpenAI-compatible upstream, preserving JSON requests and SSE bytes; apply IronGate routing and propagate consumer cancellation. Echo requires explicit demo configuration.
- Refuse missing TLS files, create identity keys exclusively with restrictive Unix permissions, and redact command payloads and routing tokens.
- Patch Rustls to 0.23.45 and Wasmtime to 36.0.16; fix verification exit handling and add regressions.
- Neural physical-layer transmission now reports unsupported rather than simulated delivery. Replica sends no longer imply durable remote storage.

- Bound TCP mesh admission, contact registries, frame/queue/write deadlines, and cancellation cleanup; make limits configurable and close evicted sockets.
- Share native admission across transports, enforce complete-message idle deadlines, bound native writes and decompression, reject unsupported wire versions, and bound both native frame readers.
- Apply explicit mTLS and CRL settings to native TCP and WSS; fail on missing required CA/CRL files. Generate development certificates with validity relative to creation time. Optional native QUIC refuses configured TLS until it can enforce that policy.

- Add `spine mcp` stdio forwarding to native core tool schemas and execution, including TLS/mTLS options, bounded messages/calls, JSON-RPC validation, ping, and unknown-tool errors. A failed native connection is discarded rather than reused or silently retried.
- Select the Ring TLS provider explicitly in core and SDK to avoid dependency-feature ambiguity; bound SDK connection and TLS setup and correct certificate CLI filenames.

- Make native readiness reflect global connection/session capacity and draining while keeping liveness healthy under capacity pressure. Export aggregate admission and outbound outcomes, including cancellation cleanup, without user-controlled labels; derive the session gauge from the actual map on scrape.

- Add configurable detached-session retention, connection leases, serialized session admission/imports, bounded atomic session/knowledge snapshots and restart expiry. Stop fabricating scout sessions for unsupported legacy swarm search.

- Make the daemon read while a command runs. Each native connection is split (`Connection::split`) into a reader, a command loop and a writer: a `PING` sent during a slow command is answered at once, a request beyond `max_inflight` is refused with `overloaded`, and a closed connection stops the running command. The daemon now advertises `max_inflight` 8; commands of one connection still run one at a time, in arrival order. Shutdown during a command answers it, then sends `CLOSE(shutting_down)`. `STREAM_CANCEL` is read and dropped, since v1 defines cancel only for streams and nothing streams.

### Compatibility notes

- Core migrates legacy per-session JSON files once into `spine-sessions-v1.json`, then treats that manifest as authoritative. Detached sessions expire after one hour by default; connected sessions are protected. Corrupt or over-capacity state refuses startup. Back up the manifest and configure retention for the deployment.

- Native and WASM HTTP navigation now deny internal addresses by default, refuse redirects and compressed responses, and enforce bounded UTF-8 responses and request deadlines. Use exact `[outbound].internal_origins` exceptions for intentionally internal resources. Operator-configured model and mesh endpoints retain their separate trust boundaries.

- Set `mutual_tls = true` explicitly when client certificates are required; a CA file alone no longer selects client authentication. TLS-enabled origin WebSocket clients must use WSS. Optional native QUIC cannot run with that configured TLS policy until equivalent certificate enforcement is integrated.

`PublishResource`, `ReadResource`, `NamespaceStatus`, and `MeshCall` are appended native commands; `Unavailable` is a new resolution outcome; application RPC adds a signed mesh payload variant. Upgrade clients and nodes together. Older peers cannot use the new wire operations; exhaustive Rust matches need updates. `FetchName` retains its endpoint-discovery contract. New namespace configuration is optional and task execution defaults off. Task and immutable knowledge storage use separate v1 namespaces; publisher/resource namespaces retain their existing versioning. Cache and peer state remain ephemeral. `replicas` reports directed sends, `replication_acknowledged` remains zero, and `ReplicationReport::is_durable()` returns false until a storage-acknowledgement protocol exists. Source remains 2.0.2 for this development candidate; public release version selection and publication remain pending.

### Added

- **An opt-in route through IronGate for the model dispatchers.** With
  `IRONGATE_URL` set (and `IRONGATE_TOKEN` when the gateway requires one),
  `OpenAiDispatcher` and `AnthropicDispatcher` send their calls to IronGate's
  OpenAI-compatible `/v1/chat/completions`, passing the model name through and
  no provider key -- the Anthropic request translated into the chat shape, with
  the same message and limit. Unset, calls go direct exactly as before;
  `SPINE_REQUIRE_GATEWAY=1` refuses a direct call with an error naming both
  variables; a dispatcher pointed at a loopback model server stays direct. The
  same contract as Rodeo's and IronBridge's (`spine_recursive::gateway_route`).
  The manifest declares the edge to `irongate`.

## [2.0.2] — 2026-08-25 — The release that builds

No library code changed behaviour on Linux or Windows. This release exists
because 2.0.1 could not be built by its own CI, and a tag that cannot be built
cannot produce binaries.

CI had never been green on `master` — of the 100 runs before today, 85 failed,
7 were cancelled and none passed, going back to 2025-12-21. Every verification
recorded in the repo was a local one, and the local tree really was green; the
failures were in things the local script did not run.

### Fixed

- **`cargo fmt --all`** across 85 files in 20 crates. The `lint` job is first in
  the dependency graph, so a formatting diff was stopping every job downstream
  of it — including the one that uploads release binaries.
- **`protoc` is now installed in CI.** `spine-grpc` builds its service
  definitions with `tonic-build`, which shells out to `protoc`; no runner ships
  one, so `prost-build` failed before compiling anything. Added to all seven
  compiling jobs and to the Dockerfile builder, which had the same hole.
- **`spine-kernel` compiles on aarch64**, which it never has — it was published
  to crates.io at 2.0.1 in that state. Its `_prefetch` call could not have
  compiled anywhere: the intrinsic is unstable and takes a const locality where
  a runtime value was passed. Six syscall wrappers had Linux and Windows arms
  and nothing in between, so `lib.rs` could not re-export them.
- **`cargo-deny`'s license allow-list**, which rejected the workspace against
  itself. SPINE relicensed to AGPL-3.0-or-later and the allow-list was not moved
  with it, so all 30 of its own crates were rejected; a comment in `deny.toml`
  still claimed Apache-2.0 long after `LICENSE` said otherwise.
- **The `bench` job's history step**, which had never worked. It fetched a
  `gh-pages` branch this repository does not have, and its auto-push condition
  tested `refs/heads/main` when the default branch is `master`, so nothing would
  ever have been stored even if the branch existed. Benchmarks still run; their
  output is uploaded as an artifact.
- **Two tests that encoded an x86 assumption.** `test_calibration` asserted a
  500 MHz–10 GHz counter; on Apple Silicon `rdtsc` reads `cntvct_el0`, a fixed
  24 MHz timer unrelated to the core clock. Bounds are per-architecture now.

### Security

- **Nine of the eleven open advisories are closed** by a `cargo update` inside
  existing semver ranges — no manifest changes were needed:
  `crossbeam-epoch` 0.9.18→0.9.20 (RUSTSEC-2026-0204), `quick-xml`
  0.39.4→0.41.0 (2026-0194, 2026-0195), `quinn-proto` 0.11.14→0.11.17
  (2026-0185, unbounded out-of-order buffering), `wasmtime` 36.0.10→36.0.14
  (2026-0222), `memmap2` 0.9.10→0.9.11 (2026-0186), `anyhow` 1.0.102→1.0.104
  (2026-0190), `event-listener` 5.4.1→5.4.2 (2026-0221), `webbrowser`
  1.2.1→1.2.4 (2026-0257), and `h2` 0.4.14→0.4.19 (2026-0258).
- `deny.toml` now also ignores the `backoff` and `ttf-parser` unmaintained
  notices, which had been left out of a list that already carried eight others.

- **RUSTSEC-2026-0258 is closed on the `h2` 0.3 line too**, which no update
  could do: 0.3.27 is the newest 0.3.x published, so the chain pulling it had
  to go. Two workspace declarations fed it, and both are migrated:

  - `spine-core` moves from **axum 0.6 to 0.7**, matching `spine-gateway` — the
    workspace had been carrying two axum majors at once. axum 0.7 dropped
    `Server` in favour of binding a listener and passing it to `axum::serve`,
    so bind failures are now reported separately from serve failures.
  - **OpenTelemetry 0.21 → 0.31** (`opentelemetry`, `opentelemetry_sdk`,
    `opentelemetry-otlp`, and `tracing-opentelemetry` 0.22 → 0.32). The
    `new_pipeline().install_batch()` builder is gone; the exporter and provider
    are constructed explicitly. The provider must outlive the tracer — dropping
    it shuts the batch processor down and silently stops exporting.

  `hyper` 0.14 and `h2` 0.3 are now absent from the lock file entirely.

### Removed

- **Containers, entirely.** `Dockerfile`, `docker-compose.yml`, the Helm chart
  under `deploy/kubernetes/`, and the `docker` and `helm` CI jobs are gone, and
  `release` no longer depends on `docker`. `scripts/deploy.sh` brings up the
  same 3-node cluster natively and is unaffected. `deploy/grafana` and
  `deploy/prometheus` stay — they are scrape and dashboard configs, not
  container assets, and the observability docs reference them.

  What prompted it: the `docker` job had sat behind a failing `lint` for eight
  months, and the first time it actually ran it failed too — its builder image
  was pinned at `rust:1.82`, which cannot parse `cranelift-assembler-x64`'s
  manifest in the current lock file. Rather than carry an image nobody had
  built successfully since 2025, the container story is withdrawn.

### Changed

- **MSRV is 1.88, not 1.75.** The 1.75 figure was never true and the `msrv` job
  had never once passed. `Cargo.lock` is lockfile v4, which cargo could not
  parse before 1.78, so the job died on the lock file; behind that sat ten call
  sites needing 1.82 or 1.87; behind those, `time@0.3.47` requires 1.88, which
  is the binding constraint. The README badge, installation docs and ROADMAP
  are corrected to match.
- **`clippy.toml` added** carrying that MSRV. Without it Clippy assumes the
  runner's toolchain and `clippy::incompatible_msrv` cannot fire at all.
- `chunks_exact(N)` → `as_chunks::<N>()` at four sites, which 1.88 makes
  available. Two of them drop a manual `[c[0], c[1], c[2], c[3]]`
  reconstruction.

### Notes

- **Not published to crates.io.** The 29 crates remain at 2.0.1 (`spine-embedded`
  at 0.2.0). This tag exists so the release workflow has something it can build.
- `deny` and `audit` are still red — a `cargo-deny` license allowlist that has
  fallen behind the tree, and 20 open RUSTSEC advisories. Neither gates the
  release job.

## [2.0.1] — 2026-08-19 — The release that ships

Same code as 2.0.0 plus the packaging it needed. 2.0.0 was tagged before the
crates were actually ready to publish, and a published version is immutable —
so rather than move a tag that was already pushed, this is a new patch release
whose tag points at what is genuinely being published.

### Added

- A `README.md` for each of the 29 publishable crates. Every crate but one had
  none, so each would have arrived on crates.io showing a single line of
  description. Generated from each crate's own metadata rather than by pointing
  all of them at the 78 KiB workspace README, which cargo copies wholesale into
  every package.
- `scripts/publish.sh`, which computes the publish order from `cargo metadata`
  instead of maintaining it by hand. The previous helper was gitignored, kept a
  hand-written list, and had silently dropped `spine-name` — which three crates
  depend on, so a run would have failed partway with earlier crates already
  permanently published.

### Changed

- `spine-embedded` moves to 0.2.0, reconciling a repo/registry divergence under
  a single version number; see the 2.0.0 notes below.
- `PUBLISHING.md` corrected: 29 publishable crates, four of which ship a
  binary. It had claimed 28 and 27.

### Notes

- Nothing was published from 2.0.0. It exists as a tag only.


## [2.0.0] — 2026-08-18 — The `spine://` namespace, and four cryptographic fixes

The namespace work (Phases 38-48) plus the security findings that came out of
preparing the mesh handshake for external review.

**This release breaks the wire without breaking a single signature.** A 2.0.0
peer cannot talk to a 1.x peer over the Chameleon layer. Cargo's semver rules
cannot see this — no public API changed — which is why it is stated here and in
`PUBLISHING.md`. Peers must be upgraded together.

### Added

- **`spine-name`** — the `spine://` namespace: URIs with four authority kinds
  (`did:`, `blob:`, `cap:`, `host:`), signed records, a 256-bit Kademlia
  keyspace, a capability index alongside the keyspace index, and a crawl
  frontier.
- **Federated resolution** over TCP, WebSocket and QUIC, with bootstrap from a
  seed address (`NameHello`/`NameHelloAck`), replication to the K closest
  nodes, and a bounded, resumable maintenance pass.
- **The namespace over plain HTTP** — `/v1/names/{resolve,providers,publish,
  endpoints,crawl}` on `spine-gateway`, mapping namespace outcomes onto the
  HTTP status codes that already mean them, and reporting provenance so a
  `host:` name's unattested address is never dressed up as a signed one.
- **`scripts/verify.sh`** — asserts the *shape* of a workspace run (suite
  count, ignored count, expected total, cargo's own exit status), because
  summing "test result" lines cannot tell a passing run from a truncated one.

### Security

- **The mesh handshake's ephemeral ML-KEM keypair came from a counter.**
  `StdRng::seed_from_u64` was fed a per-connection counter starting at zero, so
  the Nth connection a node accepted always used the same keypair and forward
  secrecy was nominal. Now drawn from the OS CSPRNG.
- **AES-GCM nonce reuse across sessions.** The nonce was a counter restarting at
  zero behind 32 random bits, under a key that is a pure function of the shared
  secret — two sessions collided with probability near 39% after 65,536 of them,
  costing both confidentiality and unforgeability. Nonces are now 96 random bits.
- **The record store was unbounded and accepted anything signed.** Minting a
  `did:` name costs one keygen, so verification bounded nothing. Now capped,
  evicting the key furthest from the node's own position, and refusing
  announcements for keys it is not responsible for.
- **The Chameleon layer keyed itself with 64 bits** derived by `DefaultHasher`.
  Now HKDF-SHA-256, seeding the encoder from all 32 bytes. `DefaultHasher` was
  also unspecified across Rust releases, so peers built with different compilers
  could have disagreed about the same secret.

### Fixed

- **Referrals carried no mesh identity**, so a peer learned mid-lookup could be
  shortlisted and never addressed — multi-hop lookups did not work over any real
  transport. The in-process test harness pre-registered every node, which is why
  the suite passed throughout.
- **`PublishName` never left the node**, making a record resolvable on one
  machine and findable nowhere.
- **Two `spine-ffi` tests asserted the version literal** rather than
  `CARGO_PKG_VERSION`, so they failed on any release.

### Notes

- **`spine-embedded` moves to 0.2.0.** It versions independently of the
  workspace, and its published 0.1.0 had drifted from the repo's: 0.1.0 went to
  crates.io when the crate was much smaller, after which it gained code and
  moved its `spine-nostd` dependency to a new major. A published version is
  immutable, so the two could only be reconciled by a new version. The publish
  script had been skipping it as "already published", which hid the divergence
  rather than causing it.
- **The workspace manifest had said `1.0.0` since v1.0.0**, unchanged through
  every tag to v1.9.1 — so those thirteen tagged releases all declared the same
  crate version, and none after the first could have been published to
  crates.io, which rejects a duplicate version. This release corrects the
  manifest as well as bumping it.

### Tests

- Workspace: 1,418 passing / 0 failing / 5 ignored, across 68 suites.

## [1.9.1] — 2026-06-08 — Harden the gRPC model backend against secret leaks

Release hardening for the v1.9.0 `OpenAiChatModel`, ensuring the bearer token
it holds can never escape via logs.

### Security

- **`OpenAiChatModel`'s API key is now private and redacted in `Debug`.** The
  field was `pub` with a derived `Debug`, so an accidental `{:?}` or a tracing
  span would have printed the bearer token. It is now a private field with a
  hand-written `Debug` that reports only presence (`api_key: Some("<set>")`),
  matching `spine_gateway`'s `BearerConfig` contract; added `has_api_key()` for
  a safe presence check and a regression test asserting the secret never
  appears in `Debug` output.
- **Upstream error bodies are capped at 256 chars** before being surfaced in
  `ChatError::Status`, so a large or sensitive upstream error response can't
  flow through unbounded.

### Notes

- No accidental data egress by default: with no `SPINE_GRPC_MODEL_URL`
  configured, `StreamChat` uses the in-process `EchoModel` and makes no network
  calls. The example never logs the key or the model URL.

## [1.9.0] — 2026-06-08 — Model-backed gRPC streaming + reflection

Matures the v1.8.0 gRPC bridge from a demo into a production server.

### Added

- **Pluggable `ChatModel` backend for `StreamChat` (`spine_grpc::model`).**
  `StreamChat` no longer hard-codes a word-splitter; it delegates to a
  `ChatModel`, mapped **lazily** so the response stream pulls from the model
  only as the gRPC client pulls — cancelling the gRPC stream stops generation
  (proven by `stream_chat_is_lazy_so_cancellation_stops_generation`). Two
  implementations ship:
  - `OpenAiChatModel` — backs `StreamChat` with **any OpenAI-compatible
    `/v1/chat/completions` endpoint** (OpenAI, Together, Groq, vLLM, or SPINE's
    own gateway), streaming `chat.completion.chunk` SSE deltas. The decode path
    (`SseDecoder`, `deltas_from_byte_stream`) is a pure state machine unit-
    tested with canned, mid-token-split bytes — no network in tests.
  - `EchoModel` — hermetic reference/default (streams the prompt back).
  `SpineAgent::with_model(...)` selects the backend.
- **gRPC server reflection.** `build.rs` emits a file-descriptor set and
  `spine_grpc::FILE_DESCRIPTOR_SET` exposes it, so a server can register
  `tonic-reflection` and let `grpcurl` (and other tooling) introspect and call
  the service with no `.proto`.
- **`examples/serve.rs`** — a runnable, reflection-enabled `AgentService` on
  `0.0.0.0:50051`. `EchoModel` by default; set `SPINE_GRPC_MODEL_URL` (+
  `SPINE_GRPC_MODEL_KEY`) to stream from a real model. Includes the `grpcurl`
  invocations.

## [1.8.0] — 2026-06-08 — gRPC / protobuf bridge

SPINE's third ecosystem bridge, after the MCP server and the OpenAI-compatible
gateway — each reaches a different installed base. This one opens the entire
protobuf/gRPC world.

### Added

- **`spine-grpc` crate — a tonic `AgentService` exposing SPINE's agentic
  surface over gRPC.** A `proto/spine_agentic.proto` defines `Capability`,
  `ListCapabilities`, `CallTool`, and a server-streaming `StreamChat`; the crate
  bridges them to the native `spine_protocol` types
  (`CapabilityAdvertisement` ↔ `ListCapabilities`, `ToolCall`/`ToolResult` ↔
  `CallTool`, `StreamStart/Token/End` ↔ `StreamChunk`). JSON-shaped fields
  (args, schemas, results) travel as serialized JSON strings — lossless and
  simple. `SpineAgent::new(advertisement, executor)` is a drop-in
  `AgentServiceServer`, so every gRPC client (tonic, grpc-go, grpc-java),
  `grpc-web`, and reflection tooling (`grpcurl`) can call a SPINE agent.
- **End-to-end test over real TCP/HTTP-2** (`tests/grpc_e2e.rs`): boots the
  service on a localhost socket and drives `ListCapabilities`, `CallTool`
  (`2 + 40 → 42`), and the `StreamChat` server stream with the generated
  client — verifying the actual wire path, not just the service methods.
- Pure, tested type mappings (`capability_to_proto`, `advertisement_to_proto`,
  `request_to_tool_call`, `tool_result_to_proto`) for callers that want the
  bridge without the transport.

### Notes

- Requires `protoc` at build time (codegen via `tonic-build`). Added
  `src/spine-grpc` to the workspace.

## [1.7.0] — 2026-06-08 — Wire codec: fast by default (no hot-path zstd)

A benchmark (`benches/hot_path_bench.rs::wire_codec`, added here) exposed a
serious performance bug in the v1.4.0–v1.6.0 wire codec: `wire::encode`
zstd-compressed every frame past 128 bytes, and zstd's ~hundreds-of-µs fixed
per-call cost dominated. Measured on this machine: encoding a 1 KiB embedding
took **252 µs** and a 247-byte tool call **334 µs** — 56× and ~1300× slower
than the JSON it replaced. The transport envelope *also* compresses, so the
live path was double-zstd-ing.

### Fixed

- **`wire::encode` is now plain CBOR — fast, and still dense.** It no longer
  compresses. Re-measured: encoding the 1 KiB embedding dropped **252 µs →
  588 ns** (~430× faster) — now ~10× *faster* than JSON encode, because writing
  a CBOR byte string beats formatting a JSON number array. Plain CBOR remains
  far smaller than JSON: the embedding frame is 3975 → **1263 B (68% smaller)**
  at full speed.

### Added

- **`wire::encode_compressed`** — explicit opt-in CBOR+zstd for bandwidth-bound
  or cold paths (bulk tensor transfer, archival, slow links), where zstd's CPU
  cost is worth the bytes (embedding frame → 446 B, 89% smaller). `decode` reads
  plain or compressed frames transparently, so peers need no configuration.
- **`benches/hot_path_bench.rs::wire_codec`** — head-to-head CBOR-vs-JSON
  encode/decode throughput for an embedding and a tool call. This is what
  caught the regression; it now guards against its return.

### Measured (via `examples/wire_sizes.rs`, header included)

| frame                          | JSON   | CBOR (default) | CBOR+zstd (opt-in) |
|--------------------------------|-------:|---------------:|-------------------:|
| EncodedFrame (1 KiB embedding) | 3975 B | 1263 B (68%)   | 446 B (89%)        |
| CapabilityAd (2 caps + schema) | 806 B  | 649 B (19%)    | 322 B (60%)        |
| ToolCall (URL + headers args)  | 323 B  | 284 B (12%)    | 255 B (21%)        |
| StreamToken (text)             | 132 B  | 123 B (7%)     | 123 B (7%)         |
| Ping (control)                 | 33 B   | 30 B (9%)      | 30 B (9%)          |

### Compatibility

- Wire-compatible: `decode` still reads `FORMAT_CBOR_ZSTD` frames, so frames
  written by v1.4.0–v1.6.0 (or by `encode_compressed`) decode unchanged. Only
  the *default* `encode` output changed (now always `FORMAT_CBOR`).

## [1.6.0] — 2026-06-08 — Runnable MCP stdio server

Turns the v1.5.0 MCP bridge from a library into a deployable server.

### Added

- **`mcp::serve_stdio` + `examples/mcp_stdio_server.rs` — a runnable MCP server
  over the stdio transport.** `serve_stdio` reads newline-delimited JSON-RPC
  from any `BufRead`, dispatches through `McpServer`, and writes responses to a
  `Write` — the exact loop an MCP host (Claude Desktop, Claude Code, an
  MCP-capable IDE) drives when it spawns the server process. The example wires
  it to stdin/stdout with a two-tool demo agent, so

  ```jsonc
  { "mcpServers": { "spine": { "command": "cargo",
    "args": ["run","-p","spine-protocol","--example","mcp_stdio_server"] } } }
  ```

  is a working host config. This closes the gap between *having* the MCP
  mapping (v1.5.0) and *being* a server a host can use with no SPINE-specific
  code. Covered by an end-to-end test driving initialize → notification →
  tools/list → tools/call over byte buffers.

## [1.5.0] — 2026-06-08 — Interop + completeness across all five agentic axes

A push to improve every axis the `agentic-eval` web benchmark scores —
streaming, tool-discoverability, encoding, interop, and security — with real,
tested capabilities rather than score tweaks. Interop gets the centerpiece (an
MCP bridge); the already-strong axes get genuine completeness work.

### Added

- **MCP bridge (`spine_protocol::mcp`) — interop.** Maps SPINE's native agentic
  primitives onto the Model Context Protocol JSON-RPC 2.0 surface so any MCP
  host (Claude Desktop, Claude Code, MCP-capable IDEs) can drive a SPINE agent
  with no SPINE-specific code: `CapabilityAdvertisement` → `tools/list`,
  `tools/call` ↔ `ToolCall`/`ToolResult`. `McpServer` is a transport-agnostic
  JSON-RPC dispatcher (`initialize` / `tools/list` / `tools/call`); the reverse
  direction lets a SPINE agent act as an MCP *client*. This is a concrete bridge
  to the existing ecosystem alongside the gateway's OpenAI-compatible routes.
- **Per-message signed frames (`spine_agentic::signed_frame`) — security.** An
  optional Ed25519 detached signature over the exact `wire::encode` bytes,
  wrapped in a 100-byte `"SPS1"` envelope (magic + 32-byte pubkey + 64-byte
  sig). Gives inline, offline-verifiable **integrity, authenticity, and
  non-repudiation** at the *message* level — a property channel auth (TLS/mTLS)
  does not provide once a message leaves the socket. Verifies the signature
  before handing any bytes to the decoder (fail-closed).
- **`Message::StreamCancel { id, reason }` — streaming.** Cooperatively cancels
  one in-flight stream by id. SPINE multiplexes many streams over one
  connection, so closing the transport (the SSE/HTTP cancel mechanism) is too
  blunt; this targets a single stream and asks the producer to emit a final
  `StreamEnd { reason: Cancelled }`.
- **`StreamToken.usage` — streaming.** Optional cumulative usage as of each
  chunk, the multiplexed analogue of OpenAI's `stream_options.include_usage`,
  so a consumer can enforce a token budget mid-stream.

### Changed

- **Tensor/byte payloads serialize as CBOR byte strings — encoding.**
  `EncodedFrame.data` and `StreamData::Bytes` now use `serde_bytes`, so on the
  wire they are a single CBOR byte string (1-byte overhead) rather than an
  integer-per-byte array. Measured: the 1 KiB embedding frame drops 546 → 446 B
  (86% → 89% smaller than JSON); the win is larger still for
  incompressible (real, random) embeddings where zstd cannot help. JSON output
  is unchanged (JSON has no byte type), so the legacy path and round-trips are
  preserved.

### Measured (via `examples/wire_sizes.rs`, header included)

| frame                          | JSON   | SPINE  | codec     | saved |
|--------------------------------|-------:|-------:|-----------|------:|
| EncodedFrame (1 KiB embedding) | 3975 B | 446 B  | cbor+zstd | 89%  |
| CapabilityAd (2 caps + schema) | 806 B  | 322 B  | cbor+zstd | 60%  |
| ToolCall (URL + headers args)  | 323 B  | 255 B  | cbor+zstd | 21%  |
| StreamToken (text)             | 132 B  | 123 B  | cbor      | 7%   |
| Ping (control)                 | 33 B   | 30 B   | cbor      | 9%   |

### Compatibility

- `StreamToken` gained an optional `usage` field and `Message` gained a
  `StreamCancel` variant. Both are additive; CBOR/JSON decoding of older frames
  is unaffected (the field is `skip_serializing_if = None`). The v1.4.0
  backward-compatible decode (legacy raw-JSON fallback) is unchanged.

## [1.4.0] — 2026-06-08 — Binary wire format (CBOR) — win on encoding

SPINE's message body was UTF-8 JSON behind a binary header. v1.4.0 replaces
it with a compact, self-describing binary wire codec so SPINE is competitive
on raw encoding efficiency — the axis it previously lost to gRPC/protobuf in
the `agentic-eval` web benchmark — without giving up `serde_json::Value`
ergonomics in tool args and schemas.

### Added

- **`spine_protocol::wire` — binary wire codec.** Every message body is now
  framed with an 8-byte `SpineWireHeader` (`"SP"` magic, version, format byte,
  `u32` big-endian payload length) followed by a **CBOR** ([RFC 8949](https://www.rfc-editor.org/rfc/rfc8949))
  payload. CBOR is the only serde-compatible binary format that round-trips
  `serde_json::Value` natively (it is self-describing, so `deserialize_any`
  works — unlike bincode 1.x / postcard, which reject it). The `format` byte
  auto-selects the codec on decode: `0x01` JSON (legacy/debug), `0x02` CBOR,
  `0x03` CBOR+zstd. Payloads ≥ 128 B are additionally zstd-compressed when that
  actually shrinks them (the encoder falls back to plain CBOR otherwise).
- `ciborium = "0.2"` dependency on `spine-protocol`.
- `examples/wire_sizes.rs` — prints JSON vs SPINE-wire sizes, codec, and
  savings per representative frame. Run with
  `cargo run -p spine-protocol --example wire_sizes`.
- `tests/wire_encoding.rs` — per-frame round-trip proofs (value-level equality)
  plus measured size assertions.

### Changed

- `ProtocolHandler` encode/decode paths (`send_message`, `send_message_raw`,
  `encode_message` / `decode_message`, and the speculative-frame payload) now
  serialize the message body through `wire::encode` / `wire::decode` instead of
  `serde_json`. The existing padding, adaptive-compression, Chameleon, and AEAD
  layers are unchanged and compose on top.

### Measured (via `examples/wire_sizes.rs`, header included)

| frame                | JSON   | SPINE  | codec     | saved |
|----------------------|-------:|-------:|-----------|------:|
| EncodedFrame (1 KiB embedding) | 3975 B | 546 B  | cbor+zstd | 86%  |
| CapabilityAd (2 caps + schema) | 806 B  | 322 B  | cbor+zstd | 60%  |
| ToolCall (URL + headers args)  | 323 B  | 255 B  | cbor+zstd | 21%  |
| StreamToken (text)             | 132 B  | 123 B  | cbor      | 7%   |
| Ping (control)                 | 33 B   | 30 B   | cbor      | 9%   |

The win is largest exactly where agent *data* traffic lives — embeddings,
latents, hidden states, and structured capability/schema frames — where CBOR's
native byte/number widths replace JSON's decimal-string-per-byte blowup.
High-entropy text (URLs, UUIDs, prose) can't be compressed below its own
content, so those frames shrink modestly; they still always beat JSON.

### Compatibility

- **Decode is backward-compatible.** `wire::decode` detects a missing `"SP"`
  magic and falls back to parsing the buffer as legacy v1.3.x raw-JSON, so a
  v1.4.0 node reads v1.3.x bodies. The reverse (a v1.3.x node reading v1.4.0
  CBOR) is **not** supported — broader cross-version interop is deliberately a
  later concern.

## [1.3.0] — 2026-06-04 — Close all v1.2.1 residuals

Addresses every remaining item from the v1.2.1 multi-framework audit.
No new feature surface; pure security hardening.

### Fixed (HIGH residuals from v1.2.1)

- **Gateway bearer auth is now secure by default** (CMMC AC.L1-3.1.1,
  MITRE T1190). The binary refuses to start unless the deployer makes
  an explicit choice via env var:
  - `SPINE_GATEWAY_BEARER_TOKEN=<secret>` — auth on (recommended).
  - `SPINE_GATEWAY_ALLOW_UNAUTH=1` — explicit opt-out for local dev
    or behind an authenticating proxy.
  Setting neither, setting both, or setting the token to an
  empty/whitespace string causes the gateway to print the deployer
  message to stderr and exit with code 2. Previous v1.2.1 behavior
  (silently running open) is no longer reachable. New type
  `AuthMode::resolve` carries the contract; legacy `BearerConfig::
  from_env` kept for backwards-compat with `#[allow(dead_code)]`.

### Fixed (MEDIUM residuals from v1.2.1)

- **Private-key memory is now zeroized on Drop** (NIST SP 800-171
  § 3.13.10). Added `zeroize = "1.8"` to `spine-crypto`,
  `spine-protocol`, and `spine-agentic`. The following structs derive
  or manually impl `Zeroize` / `ZeroizeOnDrop`:
  - `spine_crypto::RingElement` — RLWE coefficient vector.
  - `spine_crypto::QuantumKeyPair` — RLWE secret/public ring
    elements; `params` skipped (plaintext metadata).
  - `spine_crypto::MlKemKeyPair` — FIPS 203 decapsulation key bytes;
    `algorithm` skipped (enum tag, not secret).
  - `spine_crypto::QuantumKeyEvolution` — manual `Drop` that scrubs
    the rolling key-hash history; wrapped key structs zero themselves.
  - `spine_protocol::ProtocolMorphology` — manual `Drop` zeroing the
    32-byte session HMAC key.
  - `spine_agentic::Ed25519Keypair` — manual `Drop` scrubbing the
    cached public-key bytes; the inner `ed25519-dalek::SigningKey`
    already derives `ZeroizeOnDrop` upstream.

  Regression guards: 3 compile-time `assert_zeroize_on_drop` checks
  + 3 runtime "fill with secret, zeroize, observe zeros" tests in
  `spine-crypto`.

### Added (DEPLOYER residual from v1.2.1)

- **`fips` cargo feature on `spine-gateway`**. Enabling it
  (`cargo build -p spine-gateway --features fips`) pulls in
  `aws-lc-rs` as the rustls `CryptoProvider` and installs it as the
  process-wide rustls default at startup. The gateway emits an
  `INFO` line noting that FIPS mode is active.

  An end-to-end FIPS 140-3 *validated* module requires the deployer
  to also rebuild `aws-lc-rs` itself with `AWS_LC_FIPS=1` — the
  cargo feature wires SPINE's integration point; the validated
  binary is a deployer toolchain decision documented in
  `SECURITY_AUDIT.md § 2` and `SECURITY.md § FIPS 140-3 build`.

### Changed

- `spine-gateway` deps: added `rustls` (direct), `thiserror`, and the
  `fips` feature pass-through to `rustls/aws_lc_rs`.
- `spine-crypto` / `spine-protocol` / `spine-agentic` deps: added
  `zeroize = { version = "1.8", features = ["derive"] }`.
- `BearerConfig` now has a manual `Debug` impl that prints only the
  token length — never the secret bytes — so debug-log paths can't
  leak the configured bearer.
- `SECURITY.md` gains two new sections: "Gateway authentication
  (secure by default as of v1.3.0)" and "FIPS 140-3 build" and
  "Cryptographic key memory hygiene".

### Verification

- `cargo test --workspace --no-fail-fast` → **1,084 passed / 0
  failed / 5 ignored** (+12 since v1.2.1: 6 auth-resolve tests + 6
  zeroize regression guards).
- `cargo check -p spine-gateway --features fips` → clean compile;
  `aws-lc-rs 1.17.0` cross-builds on Windows.

### Breaking changes

- **`spine-gateway` startup behavior**. Existing deployments that
  relied on the v1.2.x "run open when env var unset" behavior will
  now exit with code 2 on launch. Migration: set
  `SPINE_GATEWAY_ALLOW_UNAUTH=1` to keep the previous open-public
  behavior (and immediately move to setting the bearer token for
  production).

## [1.2.1] — 2026-06-04 — Release hygiene + multi-framework audit

This release is gated by a formal security audit against CVE / RustSec,
NIST FIPS 140-3, MITRE ATT&CK v15, and CMMC 2.0. Full report in
`SECURITY_AUDIT.md`. Every BLOCKER is closed; HIGH and below are
documented and tracked.

### Fixed (BLOCKER)

- **T1565.002 Data Manipulation — Cross-request state leak in
  `/v1/embeddings`** (`spine-gateway` + `spine-protocol`). The default
  `TitansLatentCodec` is now **stateless**: every `encode()` call
  resets the wrapped `NeuralLatentEncoder`'s `message_history` buffer
  and re-seeds the PRNG before encoding, so request A's content can no
  longer influence request B's embedding via the moving-target morph.
  Added `NeuralLatentEncoder::reset_state(seed)` upstream in
  `spine-neural` to make this cheap. Opt-in stateful behavior is
  available via `TitansLatentCodec::stateful(...)`. Regression guards:
  `titans_stateless_codec_does_not_leak_across_calls`,
  `titans_stateful_codec_is_context_aware`.

- **T1499.002 Endpoint DoS — Service Exhaustion Flood**: gateway POST
  bodies were unbounded. A 10 GB POST to `/v1/embeddings` or `/api/parse`
  would deserialize and allocate without limit. Added
  `axum::extract::DefaultBodyLimit::max(8 MiB)` on the router. Per-route
  override remains available for trusted deployments.

- **T1499.004 Endpoint DoS / T1496 Resource Hijacking — CPU bound on
  HLS execution**: the WASM runtime had no fuel/timeout enforcement,
  so a `loop {}` inside an HLS program would hang the executor
  indefinitely. Added `wasmtime::Config::consume_fuel(true)` +
  per-execution fuel budget (default 1 B units ≈ seconds of CPU).
  Regression guards: `test_wasm_fuel_metering_is_active`,
  `test_default_fuel_budget_completes_normal_programs`.

- **T1190 Exploit Public-Facing App — bearer auth contract mismatch**:
  the OpenAPI schema declared a `bearer` security scheme since v1.0.0
  but no middleware enforced it. Added `auth::require_bearer` (in
  `src/spine-gateway/src/auth.rs`) with constant-time secret comparison
  via the `subtle` crate. Activated by setting
  `SPINE_GATEWAY_BEARER_TOKEN`; emits a `WARN` at startup when unset.
  Path allowlist for `/health`, `/ready`, `/swagger-ui`, `/api-docs`.

### Added

- `SECURITY.md` — supported-versions table, private reporting channels,
  threat model, cryptographic choices, known hardening boundaries.
- `SECURITY_AUDIT.md` — full v1.2.1 audit report across CVE / FIPS /
  ATT&CK / CMMC with per-finding severity and applied fixes.
- This `CHANGELOG.md`.
- `.gitignore` patterns for common secret-bearing file shapes
  (`.env*`, `*.pem`, `*.key`, `credentials*.json`,
  `secrets.{yml,yaml}`, `*.sqlite*`, backups).
- `spine-gateway` deps: `subtle = "2.5"` (constant-time comparison).

### Changed

- All four agentic gateway handlers (`chat_completions_stream`,
  `embeddings`, `capabilities`, `codecs`) now carry
  `#[tracing::instrument(skip_all, …)]` so request bodies never reach
  tracing spans. Span fields restricted to `model` and `stream` flags.
- `WasmRuntime::new()` now constructs a runtime with the default
  fuel budget; legacy callers see no API change. Trusted callers that
  need a larger budget can use `WasmRuntime::with_fuel(N)`.
- Gateway startup now logs whether bearer auth is enabled or disabled
  so deployers cannot accidentally run a production gateway with
  authentication off.

### Verification

- `cargo test --workspace --no-fail-fast` → **1,072 passed / 0 failed
  / 5 ignored** (+10 since v1.2.0: 2 WASM fuel guards + 8 auth tests).
- `cargo audit` → 0 vulnerabilities, 10 unmaintained warnings (all
  accepted with rationale in `SECURITY_AUDIT.md § 1`).

### Known residuals tracked for future release

- Bearer auth is opt-in by default. `SECURITY.md` documents the
  deployer obligation; will flip to opt-out in a future major version.
- Private-key memory not zeroized on drop. Targeted for v1.3.0
  (`zeroize` crate on `DecapsulationKey`, HKDF outputs, Ed25519
  secrets).
- FIPS 140-3 validated cryptographic module: algorithms are
  FIPS-approved by spec, but the Rust crates we use are not
  FIPS-validated. Federal deployments must swap `ring` for
  `aws-lc-rs` FIPS build; documented in `SECURITY_AUDIT.md § 2`.

## [1.2.0] — 2026-06-03 — Neural encoder-decoder protocols

### Added
- **Wire-level latent payloads** (`spine-protocol::agentic_codec`):
  - `EncodedFrame { codec, variant, data, metadata: { modality, shape,
    dtype, original_len, source_hash } }` — every latent is its own
    schema; `declared_size_consistent()` is a one-line check.
  - `Modality` enum: `Text` / `Image` / `Audio` / `Video` /
    `Embedding` / `HiddenState` / `Multimodal` / `Other(String)`.
  - `DType` enum: `F32` / `F16` / `BF16` / `I8` / `U8` / `I16` /
    `I32` / `Q4` / `Q8` with a `bytes_per_element()` table.
  - `CodecDescriptor` + `CodecAdvertisement` + `CodecNegotiation` for
    discovery and handshake.
  - `DecodeHints` (temperature, top_p, top_k, max_tokens,
    stop_sequences, repetition_penalty, presence_penalty,
    frequency_penalty, seed) — rides inline on `StreamStart`.
  - `EmbeddingRequest` / `EmbeddingResponse` (OpenAI shape at the wire
    level; `EmbeddingInput::Encoded` enables cross-codec transcoding).
- **Runtime**: `trait NeuralCodec { id, describe, encode, decode }` +
  `CodecRegistry` (`Arc<RwLock<HashMap>>`) + concrete `TitansLatentCodec`
  that wraps `spine_neural::NeuralLatentEncoder`.
- New `Message` variants: `Encoded`, `CodecAd`, `CodecNegotiation`,
  `EmbeddingRequest`, `EmbeddingResponse`.
- `StreamData::Encoded(EncodedFrame)` variant + `StreamStart.decode_hints`
  + `StreamStart.stream_codec` so token streams carry latents directly.
- Gateway routes: `POST /v1/embeddings`, `GET /v1/agentic/codecs`.

### Tests
- +21 tests (16 in `agentic_codec`, 5 in `agentic_sse`).
- Workspace: 1,060 passing / 0 failing / 5 ignored.

## [1.1.0] — 2026-06-03 — Agentic-first frame family

### Added
- **Tool calling** (MCP-shaped): `ToolCall { id, name, args }` →
  `ToolResult { id, outcome: Ok | Err }` with typed error codes.
- **Token streaming**: `StreamStart` → `StreamToken { seq, data }` →
  `StreamEnd { reason, usage }`. `StreamData::Text | Bytes | ToolCall`.
  `StreamEndReason` mirrors OpenAI/Anthropic finish-reason taxonomy.
- **Capability handshake**: `CapabilityQuery { selector: Exact | Prefix
  | Semantic { embedding, top_k } | All }` →
  `CapabilityAdvertisement { capabilities }` with JSON Schema +
  optional semantic embedding.
- **W3C trace context**: `TraceContext { trace_id, span_id, flags,
  state }` with `to_traceparent` / `from_traceparent` matching the
  W3C header format. Attached inline on tool calls, results, and
  stream starts.
- Gateway routes: `POST /v1/chat/completions` (OpenAI-compatible SSE),
  `GET /v1/agentic/capabilities`.

### Tests
- +15 tests (11 in `agentic`, 4 in `agentic_sse`).
- Workspace: 1,039 passing / 0 failing / 5 ignored.

## [1.0.1] — 2026-06-03 — Hardening release

### Fixed
- **Clippy clean** across the workspace (13 warnings → 0).
- **`cargo audit` 29 vulns → 0**. Wasmtime 27 → 36 (clears 15 CVEs in
  the wasmtime line); prometheus 0.13 → 0.14 (drops protobuf 2.x →
  3.x); transitive `rustls-webpki` / `quinn-proto` / `bytes` / `time`
  pulled to patched lines.
- **GPU matmul shader**: `WgpuBackend::mat_mul` was a CPU-fallback
  TODO; replaced with a real 16×16 tiled WGSL GEMM kernel. Verified
  on Windows via DX12 / Vulkan with `test_gpu_mat_mul_matches_cpu`.

### Added
- **CT log loader**: `CtPolicy::add_logs_from_json_v3()` parses
  Google's v3 schema and ingests usable / qualified / pending entries
  only. The previous placeholder 27-byte SPKIs are removed —
  `CtPolicy::default()` now ships with no trusted logs preloaded.
- **RDMA scaffold**: `IbVerbsRdma` / `GpuDirectRdma` replaced
  `PhantomData` stubs with the full QP state machine (RESET → INIT →
  RTR → RTS), `QpInfo` OOB tuple, and a `linux_verbs` FFI shim whose
  call sites name the exact rdma-core C functions a Linux operator
  with `rdma-sys` linked would invoke. Cross-platform compile
  preserved; `gpu-direct` now implies `rdma`.

### Tests
- Workspace: 1,024 passing / 0 failing / 5 ignored.

## [1.0.0] — 2026-05-21 — Initial public release

First tagged release at `nervosys/SPINE`. Core components:

- `spine-protocol` — Chameleon protocol (moving-target defense),
  speculative decoding with Titans predictor, AES-256-GCM AEAD, zstd
  compression, binary program execution.
- `spine-transport` — TCP / QUIC / WebSocket / shared-memory / RDMA
  trait (`LocalShmRdma` loopback impl), zero-copy frame I/O via
  vectored `writev`, batched server writes, pipelined client.
- `spine-neural` — Titans Neural Long-Term Memory with MIRAS
  variants (Tau, Kappa, Phi), VAE encoder, multi-head attention,
  surprise-gated writes.
- `spine-crypto` — quantum-resistant lattice primitives (research
  grade), X3DH key exchange, Titans-based speculative decoding.
- `spine-agentic` — agent identity (Ed25519 + DID), capability
  ontology (100+ hierarchical terms), DAG workflow engine,
  marketplace, replay debugger.
- `spine-gateway` — OpenAPI REST (axum + utoipa) with Swagger UI.
- `spine-gpu` — wgpu compute backend (mat-vec multiply, softmax, dot
  product) with SIMD-8 CPU fallback.
- 28 crates total; 1,016 tests passing.

### Honest performance posture (per 2026-05 audit)

The pre-1.0 README carried "514× lower latency / 610× higher
throughput" claims that originated in rigged comparison benches
(`tcp_comparison.rs` did pure in-memory encode vs real-loopback TCP;
`traditional_comparison.rs` substituted hand-rolled fakes for JSON,
AES-GCM, and Redis pub/sub). Those claims were **retracted across the
README, ROADMAP, and copilot-instructions**. The benchmarks were left
in tree with explicit "NOT A FAIR BENCHMARK" headers so the rigging is
inspectable.

Measured numbers from `network_realistic.rs`, `spine_vs_www.rs`,
`spine_vs_http2.rs`, `agentic_ai_workload.rs`, `llm_tok_per_sec.rs`,
and `llm_shm_ipc.rs` are recorded in `BENCHMARK_REPORT.md`. Headline
defensible wins: 35.9× faster than HTTP/2 on N=64 concurrent
multiplexed streams (1.42 M req/s), 20× faster than HTTP/2+JSON on
128-batch embedding transmission, 728 M tok/s peak via socket-tuned
async client, 1.33 G tok/s via shared-memory IPC. All on Windows 11
loopback (127.0.0.1); no real-network numbers.
