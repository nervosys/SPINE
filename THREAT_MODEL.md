# SPINE Threat Model

**Status of this document.** Rewritten 2026-11-23 from the source at `a4efd03` (branch `completion/agent-web`). It describes the default build of the daemon (`spine-core` without the `research` or `quic` features) and the stack crates under `src/`. Research crates under `research/` are not covered except where they are named as not deployed. Every statement about what is enforced cites a file and function; line numbers are as of `a4efd03` and drift. A statement that could not be checked against the code is marked unverified or left out. Earlier text in this file that described stake-weighted voting, reputation, working-memory isolation, proof-of-work, a sandboxed legacy bridge and an "emergency upgrade mechanism" as mitigations has been removed: none of it is in the default daemon, and none was verified.

## The largest residual risks, stated first

These are the points most easy to overstate. Read them before the rest.

1. **The authority gate is not enforced.** `spine_core::authority` (`src/spine-core/src/authority.rs`: `classify`, `AuthorityGate::authorize`, the durable ledger in `authority_ledger.rs`) and the `spine-authority` crate exist as library code. The daemon never calls the gate. spine-wire v1 has no field that can carry a grant chain or an invocation (`docs/spec/00-wire.md`, G8), and the handshake carries no client key, so there is nothing to bind a grant to. `[authority] mode = "required"` is refused at start-up (`config.rs` `AuthorityConfig::validate`) so that no operator believes enforcement is on. **Any client that can connect to the native port has operator-level access to all 19 commands**, including `navigate` (the node makes an HTTP request to a URL the client chose), `execute_binary` (runs a client-supplied program), `mesh_call` (the node signs a call with its own key and sends it to a peer), `publish_name` and `publish_resource` (the node announces signed records and stores bytes). **Nothing issues an authority receipt**, because the gate is not called. The only access control on the native path is who can reach the port and, if configured, TLS with a client certificate.
2. **The mesh handshake has had no external cryptographic review, and the Noise-style session layer (Stage 8 of `COMPLETION_PLAN.md`) is not built.** `spine-session` `handshake.rs` (ML-KEM-768 and Ed25519, `spine-mesh-hs-v1`) is the only SPINE-level channel, and it is used by the mesh only. The native client wire does not run over it (`docs/spec/00-wire.md` Appendix C, G1). Layer 1 of the seven-layer design (`docs/spec/01-session.md`) defines no session value that a grant could be bound to.
3. **A resolve answer on the client wire carries no succession log.** `NameResolution::Resolved { record, provenance }` (`src/spine-protocol/src/lib.rs:498-506`) is a closed type with a JSON record and nothing else (`docs/spec/00-wire.md` section 12.4; `docs/spec/02-names.md` 10.10 decision 46). A client that resolves a rotated name therefore cannot verify it through the daemon: the agent client (`src/spine-agent/src/resources.rs`) refuses a record that carries a `spine.succession` head claim unless the caller supplies the log some other way (`*_with_log`, `--log`), and a client that holds no log still accepts a record signed by a superseded key that carries no claim. The client's only alternative is to trust the daemon's judgement, which makes the daemon the root of trust.
4. **Publisher keys are free to mint, so per-publisher quotas bound a key, not a party.** A `did:` name is a fresh Ed25519 key. The resource store's fair share (`src/spine-core/src/resources.rs:46-49`, 64 MiB of 256 MiB per publisher) and the name store's capacity rules limit what one key can hold; an adversary with many keys is limited only by the global bounds.
5. **Replica records are in memory only, an evicted tombstone allows resurrection, and equivocation proofs spread only when asked for.** A restarted mesh node holds no replica until announcements or lookups bring them back; its succession logs and proofs are persisted when `namespace.records_path` is set (`spine-mesh/src/logbook_store.rs`; `spine-core/src/namespace.rs` `BackendLogBookStore`). A tombstone evicted under store pressure no longer blocks the older record it revoked, so the name can be resurrected for at most 7 days (`docs/spec/02-names.md` 10.10 decision 52); only a `retire` entry of a succession log is permanent, and only for verifiers that hold it. A node that forms an equivocation proof sends it to nobody unasked: peers learn it only when they look up the name or fetch its log (decision 51).
6. **One host call in `execute_binary` is not interruptible, and compile time is outside the 2 s deadline.** The deadline is checked at host calls (`src/spine-wasm/src/lib.rs`, `DEFAULT_EXECUTION_DEADLINE`, the charge function near line 446). A single host call is bounded by its byte and value budgets, not by time, and building the WAT and compiling the module happen before the guest starts and are bounded by `MAX_INSTRUCTIONS` (65,536) and `MAX_STRING_DATA_BYTES` (64 KiB), not by the deadline.
7. **QUIC receive windows are library defaults, and the mesh QUIC and WebSocket transports are not used by the daemon.** `spine-core` builds only `TcpNameTransport` (`src/spine-core/src/namespace.rs:42`, `:231`, always `Security::Encrypted`). The QUIC and WebSocket mesh transports in `spine-mesh` are exercised by tests only. The listeners now take connection limits, but the QUIC flow-control windows are whatever `spine-protocol` leaves at quinn's defaults, and no test covers the 64-stream cap or the QUIC handshake timeout (verification log, `057b2c2`).

## Overview

SPINE is a headless semantic browser and name layer for software agents. The daemon (`spine-core`) listens on a native client port (TCP, plus WebSocket on port + 1, plus QUIC on port + 2 only in a build with `--features quic`), on a mesh port, and on a metrics port. The native client wire is spine-wire v1 (`docs/spec/00-wire.md`): frames and deterministic-CBOR bodies, 19 commands, no encryption or authentication of its own. Confidentiality and server authentication on that path come from TLS only. There is no SPINE-level encryption, obfuscation, morphing, padding or traffic-analysis resistance on it. This document states what each part defends against, what it does not, and where the code that does the defending lives.

## What SPINE is not

- **Not an access-control system today.** See risk 1.
- **Not a replacement for TLS.** The native wire relies on it.
- **Not post-quantum end to end.** The mesh handshake's key exchange is ML-KEM-768; its signatures are Ed25519, so authentication is classical. The native wire's TLS is whatever rustls negotiates.
- **Not a trustless system.** First contact with an unconfigured mesh seed is trust on first use (below).
- **Not anonymous.** Peers see IP addresses; observers see sizes and timing.

## How the native port is exposed (TLS and who may connect)

Rule, from `SpineConfig::validate` (`src/spine-core/src/config.rs:562-598`):

- `tls.enabled` defaults to false and `server.host` to `127.0.0.1`. With defaults, the native wire is plaintext on loopback.
- `tls.mutual_tls` requires TLS enabled and an explicit CA path; a client revocation list requires mutual TLS.
- A `server.host` that is not loopback is **refused at start-up** unless `tls.mutual_tls` is on or `server.allow_unauthenticated_remote = true`. Server-only TLS does not satisfy the rule. Setting the flag is the operator stating that the network is trusted: with the flag, TLS off and a wide bind, the native port is plaintext and open to anyone who can reach it.
- The mesh listener and the metrics listener bind the same `server.host` (`config.rs` namespace and metrics settings; `main.rs` metrics bind), so the rule above covers them too.
- TLS applies to native TCP and to the WebSocket port. A build with `--features quic` refuses to start when `tls.enabled` is true (`main.rs` `anyhow::ensure!(!config.tls.enabled, ...)`), because the QUIC listener cannot enforce the TLS policy; it uses an ephemeral certificate that clients cannot obtain, and there is no QUIC client in the SDK. QUIC is not built by default and no CI job exercises it.
- With mutual TLS, authentication is an X.509 client certificate. There is no SPINE identity on this path and no per-client role.
- Certificate rotation requires a restart (`docs/NATIVE_OPERATIONS.md`).

## Adversary tiers

### Tier 1: passive network observer

**Capabilities**: packet capture, timing analysis, traffic correlation.
**Enforced today**: native wire, only if TLS is enabled; mesh, always (the daemon uses the encrypted TCP transport, `namespace.rs:232`). Metrics, `/ready` and the gateway use plain HTTP unless fronted by something else.
**Residual**: message sizes and timing are visible on both paths; the mesh handshake sends both identities and signatures in clear (no identity hiding). The Chameleon encoder and latent codec exist only in `research/spine-protocol-research` and are not a mitigation.

### Tier 2: active network attacker (man in the middle)

**Capabilities**: injection, replay, connection hijacking.
**Enforced today**:
- Native wire with TLS: rustls authenticates the server certificate; with mutual TLS it authenticates the client. Without TLS nothing authenticates anything.
- Mesh: signed ephemeral ML-KEM-768 exchange with Ed25519 identity signatures (`handshake.rs` `Initiator::start_with_rng`, `Initiator::finish`, `Responder::accept_with_rng`), HKDF-SHA-256 key derivation with a per-direction key, and AES-256-GCM frames with a strict counter that is also the AAD (`Session::seal` `:483`, `Session::open` `:509`). Signatures are verified with `verify_strict` (`handshake.rs:168`).
**Residual**: see "Deployed mesh handshake" for what the handshake does not give, and risk 2 for the lack of external review.

### Tier 3: compromised or malicious peer node

**Capabilities**: speaks the mesh protocol with its own valid key, announces records, answers lookups, sends referrals.
**Enforced today**: see threats M1 to M4 below. A peer cannot forge another node's identity (the agent id is derived from the key, `SigningIdentity::new`; a pair that does not derive is dropped and counted). It can mint unlimited identities.
**Residual**: no Sybil resistance exists in the stack crates. The earlier claims of stake-weighted voting, slashing and reputation describe `research/spine-cluster` and are not in the default daemon.

### Tier 4: store now, decrypt later

**Enforced today**: mesh confidentiality rests on ML-KEM-768 (FIPS 203) with a fresh ephemeral key per connection, so later compromise of a long-term Ed25519 key reveals nothing about past mesh traffic. The native wire's confidentiality is TLS and inherits whatever rustls negotiates.
**Residual**: no part of this is externally reviewed; authentication is classical; the research RLWE code (`research/spine-crypto`) is not used by the daemon and no claim is made for it. No crate is a FIPS 140-3 validated module.

## Threats, by asset

Each threat gives the asset, the adversary, what is enforced today and where, and the residual risk.

### N1. The node's authority over itself (the native command surface)

- **Asset**: the node's outbound network position, its signing key, its stores and its CPU.
- **Adversary**: any client that can open a native connection; any malicious page content that steers an agent holding such a connection.
- **Enforced**: reachability only (the TLS rule above); admission bounds (N6). Per connection: a token bucket of 100 commands refilled at 10 per second (`main.rs` `check_rate_limit`, applied in `handle_command` before a session is created); `max_inflight` 8 (`spine-protocol` `native.rs:127`, `DAEMON_MAX_INFLIGHT`), beyond which a request is answered `overloaded`.
- **Not enforced**: authorisation of any command. The classification exists (`authority.rs` `classify`, an exhaustive match over the 19 commands) and is not consulted.
- **Residual**: total, as in risk 1. Treat every client that can connect as the operator. The gateway bearer token (N9) and mutual TLS do not create per-user roles.

### N2. Identity pins and mesh peer authentication

- **Asset**: the binding between an agent id and the Ed25519 key that signs records and holds the connection.
- **Adversary**: a network attacker or a malicious referrer.
- **Enforced**: every authenticated mesh dial carries an expected key. A pooled dial expects the key named by the hello, ack or referral that introduced the peer, or the signed `recipient_key` of an application call; a bare seed address is accepted on first contact only, its key recorded and required for every later dial in the process; `[namespace.seed_keys]` configures a seed's key from the first dial. The writer is `pin_if_absent` (`src/spine-mesh/src/mesh_tcp.rs:499`): the first claim stands and later referrals cannot replace it. A mismatch is a hard failure (`NameMeshError::PinMismatch`, logged and counted). The agent id is derived from the public key (`SigningIdentity::new`, `from_seed`); a hello, ack, referral or application message whose id is not the derivation of its key is dropped and counted. A `NameResolveResponse` is absorbed only when its envelope signature verifies under a key recorded for the sender and, on an authenticated connection, that key equals the session key.
- **Residual**: pins are in memory and forgotten on restart; an unconfigured or DNS-discovered seed is trust on first use once per process; a pin from a referral is the referrer's unsigned claim, so a malicious referrer can make dials to an id it names first fail (it cannot make them reach an impostor); the pin tables stop recording at 1,024 entries (`MAX_KNOWN_PEERS`, `mesh_tcp.rs:69`); the replay guard for mesh RPC has a 120 s window, 4,096 entries and 256 per verified key (verification log; its persistence was not checked). A genuine resolve response can be replayed on an unauthenticated connection while its lookup is open (verification log, `faa5aaa`; not re-checked here).

### N3. Mesh over QUIC and WebSocket

- **Asset**: as N2.
- **Adversary**: a party able to terminate TLS.
- **Enforced** (library only; the daemon uses TCP): the QUIC client no longer accepts any certificate; it verifies against an empty set that a deployment fills with `QuicNameTransport::trust_certificate` (`mesh_quic.rs:252`). The key proved by the first-stream handshake is kept for the connection and passed to the handlers for every stream; application calls over QUIC require the connection key to equal the signed recipient key.
- **Residual**: the handshake is not bound to the TLS channel, so a party that can terminate TLS with a certificate the client trusts can relay a genuine peer's handshake and sit in the middle; closing it needs a channel-context input in `spine-session`, which is not implemented. Since the daemon does not use these transports, the exposure is to anyone who builds on the library.

### N4. Name records: forgery, staleness, revocation

- **Asset**: the binding of a name to endpoints and content.
- **Adversary**: a peer or a replayer.
- **Enforced** (`src/spine-name`): a record is signed by the name's own key and `verify` uses strict Ed25519 (`record.rs`); a TTL is at most 7 days (refused at signing and by `verify`); a record dated more than 300 s ahead of the verifier's clock is refused at admission (`validate_not_postdated`), including at the daemon's resource store; a tombstone is an ordinary record with the signed meta entry `spine.revoked`, the maximum TTL and nothing else, ordered above a same-`seq` record, and a store holding one refuses lower or equal `seq`; a full store drops lapsed records first, then evicts the furthest live record, and caps tombstones at half of capacity (`RecordStore::put`; decisions 47, 48, 52); a node's own publication into a full store answers `Capacity` and replicates nothing; republishing a record the node already outranks answers `StalePublication`.
- **Residual**: the wire has no revoked outcome, so the daemon reports a tombstoned name as `invalid` with a reason; revocation lasts at most as long as the tombstone and an evicted tombstone allows resurrection (risk 5); the replay argument assumes the owner assigns `seq` in publication order; `AgentId` is derived from a key but the label a peer attaches to a message is still the sender's claim until the signature is checked.

### N5. Names across key change (succession)

- **Asset**: a name that outlives a key.
- **Adversary**: a holder of a leaked or superseded key; a peer that forks a name's history.
- **Enforced** (`spine-name` `succession.rs`, `logbook.rs`; `spine-mesh` `naming.rs`, `naming_mesh.rs`; `spine-core` daemon): signed succession logs of at most 64 entries; each rotation is signed by the current key and commits to the next; a retire entry ends the name for verifiers that hold the log; a record signed by a superseded key is refused whatever its remaining lifetime; two valid, non-prefix logs are an equivocation proof and the name then fails closed for good at that verifier; every admission point checks against the held log, and each store keeps the best verified log per name, persisted beside the records; the log travels with the record on announce and in lookup responses; a node that refuses an announced record for want of its log sends one fetch to the announcer (bounded: one per name per 60 s, 64 names tracked, 8 per peer per 60 s); the daemon accepts the log on `publish_name` and `publish_resource`; the mesh service's logs and proofs are persisted when `namespace.records_path` is set, written before they are applied, and rows that do not verify are dropped and deleted at load; an equivocation proof is answered as its two logs when a peer asks.
- **Residual** (`docs/spec/02-names.md` 10.4, 10.8, 10.11): before entry 1 exists the inception key alone can take or retire a name; a lost current key cannot be recovered; a holder of a superseded key can still sign for verifiers that lack the superseding entry, with no 7-day bound on fresh records; a retire is permanent only where the log is held; a record that arrives ahead of its log is refused, not fetched, except for the single announcer fetch; a lookup whose peers all answer ahead of their log ends not found; a proof is not pushed (risk 5); the daemon has no commands to rotate or retire (the CLI commands are offline); the wire has no retired or compromised outcome; a blob stored before a rotation stays readable by id (decision 41); the gateway cannot serve a rotated name's resource at all; a rotated name whose inception key is the node key is not renewed by the node; and risk 3.

### N6. Resource exhaustion on the native and mesh ports

- **Asset**: availability.
- **Adversary**: a connecting client or peer.
- **Enforced**: `server.max_connections` 256 and `max_connections_per_ip` 50 across TCP, WebSocket and QUIC, including unfinished handshakes, refused immediately beyond the limit and released on close; 10 s for TLS and WebSocket upgrade and for QUIC connect and first stream; `server.hello_timeout_secs` (10) for `HELLO`, after which the daemon closes with `CLOSE(timeout)`; `idle_timeout_secs` (300) restarted by any frame; a frame is capped at the negotiated 16 MiB (64 KiB before `HELLO`) before any body is allocated; a compressed body declares its length, checked before decompression, with an 8 MiB zstd window; 30 s write deadline; `max_sessions` 1,000 and a retention timer for detached sessions. Mesh TCP listeners (inbound and outbound, 64 connections each by default, 1 to 1,024 configurable) have handshake, write, frame and queue timeouts, a 64-envelope inbound queue and a 4 MiB frame cap; the QUIC and WebSocket mesh listeners take the same `ConnectionLimits` (connection semaphore with immediate refusal, handshake and frame timeouts, at most 64 served streams per QUIC connection, the frame reader allocates as bytes arrive).
- **Residual**: these are component bounds, not a process-wide memory guarantee or a sustained-load result; QUIC receive windows are library defaults; a silent client holds one admission slot for up to the hello timeout; `server.max_connections` is global, so a single well-placed client population can consume it. Sustained resource measurements remain open.

### N7. `execute_binary`: client-supplied programs

- **Asset**: the daemon's CPU, memory and its outbound position.
- **Adversary**: any client (N1).
- **Enforced** (`src/spine-wasm/src/lib.rs`; `src/spine-core/src/main.rs` `BrowserCommand::ExecuteBinary`, line 1062): no WASI, network or filesystem import; wasmtime `consume_fuel` with a 1,000,000,000 default budget; host calls draw on fuel (`HOST_CALL_FUEL` 64) and are charged against byte and value budgets checked before allocating (`MAX_VALUE_BYTES` 64 KiB, `MAX_LIVE_BYTES` 1 MiB, `MAX_STRING_DATA_BYTES` 64 KiB, `MAX_ELEMENTS`, `MAX_EVENTS`, `MAX_ACTIONS` 64 and others); instruction cap 65,536; a store memory limiter (256 pages, 16 MiB); a 2 s wall-clock deadline checked at each host call; execution on `spawn_blocking` behind 4 process-wide permits, with `overloaded` after 2 s of waiting for a permit; the declared capability list is checked against every action the program produced (`navigate` needs `network`, `search` needs `search`, `store_knowledge` needs `storage`) before any is carried out; at most 4 navigations per call; at most 256 state variables and 1,024 knowledge entries per session; knowledge is written under the caller's session; a program that fails to compile or run is `execution_failed`, not a daemon abort.
- **Not enforced**: that the client may run programs at all (N1). A `navigate` action in a program uses the outbound policy (N8), not the client's authority.
- **Residual**: risk 6; the permits are process-wide, so one client can hold all four for the run time and make others see `overloaded`; navigations spawned by a program run detached and accumulate across calls up to the per-connection command rate; the `Search` action is discarded in a build without `research`, and `QueryKnowledge` and `Reason` actions are only logged (they do nothing).

### N8. Outbound fetch from `navigate` and program navigation (SSRF)

- **Asset**: internal network services and cloud metadata endpoints reachable from the node.
- **Adversary**: any client (N1) naming a URL; a hostile DNS answer.
- **Enforced** (`src/spine-core/src/outbound.rs`; defaults in `FetchConfig`): `http` and `https` only; no credentials or fragments in the URL; URLs at most 8 KiB; ports limited to `allowed_ports` (default 80 and 443) and checked before any DNS lookup, except for configured internal origins; one DNS resolution per fetch, at most 64 answers, and **every** answer must be a public unicast address (`public_address`, `:188`: loopback, private, link-local, shared, multicast, documentation, benchmarking and reserved ranges are refused; IPv6 only inside 2000::/3 minus documentation and transition ranges; IPv4-mapped IPv6 is judged as IPv4); the connection is pinned to those checked addresses (`resolve_to_addrs`, `:480`) on a fresh client per fetch, so a second resolution cannot differ; no redirects (a 3xx is `outbound_redirect_denied`), no proxies, no compressed responses; response bytes counted as they arrive against `max_response_bytes` (2 MiB default, 8 MiB maximum); a per-chunk read timeout (10 s) and a total timeout (30 s); at most 16 concurrent fetches (`outbound_busy` beyond) and 4 per caller session (`max_concurrent_per_caller`); an operator exception in `internal_origins` matches an exact scheme, host and port, and an exception naming a literal link-local (169.254.0.0/16, fe80::/10) or unspecified address is refused at configuration unless `allow_link_local_exceptions = true`, with a log line per exception at start-up. Failures return stable `outbound_*` codes without URL contents.
- **Residual**: an internal-origin exception is authority granted to every native client, because of N1; the policy does not govern operator-configured model endpoints or signed mesh resource endpoints; a body that keeps dripping bytes is cut only by the total timeout; an unknown hostname and an internal one answer with different codes, which leaks a little about internal DNS; the address policy can deny unusual public ranges; the fetched HTML then goes to the parser and, in a `research` build, a transpiler, whose total memory use this bound does not cover; network-level egress policy is still the right second control.

### N9. The HTTP gateway and the diagnostic listeners

- **Asset**: the gateway's routes (sessions, parse, compile, execute, names, resources, chat, embeddings) and the process metrics.
- **Adversary**: any HTTP client that can reach the listener.
- **Enforced**: `spine-gateway` refuses to start unless `SPINE_GATEWAY_BEARER_TOKEN` (non-empty) or `SPINE_GATEWAY_ALLOW_UNAUTH=1` is set, and refuses both (`auth.rs` `AuthMode::resolve`); with a token, `require_bearer` compares in constant time (`subtle::ConstantTimeEq`, `auth.rs:156`); bodies are limited to 8 MiB, and 40 MiB on `/v1/resources` (`main.rs:640`, `:653`); the gateway binds `server.host`. `/metrics`, `/health` and `/ready` on `metrics_port` have no authentication and bind `server.host` (loopback by default); `/metrics` labels come from fixed sets and carry no name, key, peer, session, URL or address (`docs/NATIVE_OPERATIONS.md`). The gateway's `/api/sessions/{id}/execute` forwards to the daemon through the agent client (`spine-gateway` `execute_hls`), so the `execute_binary` limits of N7 apply to it.
- **Residual**: one bearer secret, no per-user roles, and the gateway reaches the daemon as one client with its own identity, so the token does not create tenancy; the diagnostics, the dashboard and the research-feature cluster listener are inside a trusted-operator-network boundary; the example configuration shows a wide bind in places (the verification log notes this for the metrics example and for two docs pages); the `/ready` endpoint does not probe model or bridge dependencies.

### N10. Resource store and resource transfer

- **Asset**: stored blobs and the node's disk.
- **Adversary**: a client or peer publishing resources.
- **Enforced** (`src/spine-core/src/resources.rs`): a resource id commits to the content type and bytes (`spine_name::resource_id`, `verify_resource`), checked on every publish and fetch path; the store is 256 MiB with a 64 MiB per-publisher fair share (`MAX_STORED_BYTES`, `FAIR_SHARE_BYTES`); at capacity the oldest unreferenced blob of the most over-share publisher is evicted; a blob is charged to the name's inception key whichever key signed; a blob is hashed on write, at load and on the first read per process; blobs named by a currently published record are protected from eviction until that record expires; the binding record is judged by the local resolver's current key authority (`LocalResolver::authorise`) and refused first if post-dated.
- **Residual**: risk 4; delete is not on the wire, so an owner cannot withdraw bytes, only let them expire and be evicted; the load pass hashes every stored blob at start-up; a blob stored before a rotation stays readable by id; the first-read verified set is deliberately not seeded from load (corruption between load and first read is detected today).

### N11. Knowledge and session state

- **Asset**: per-session state and the knowledge store.
- **Adversary**: a client filling memory.
- **Enforced** (`main.rs` constants at `:600-613`; `src/spine-core/src/knowledge.rs`): 1,024 knowledge entries per session and 64 KiB per entry for both `store_knowledge` and the entries a program stores; `query_knowledge` returns at most 128 entries without cloning the rest; 256 reactive state variables per session; knowledge is session-scoped and removed with the session; session snapshots are limited to 64 MiB (`session_store.rs:9`).
- **Residual** (computed from the constants, not tested): the bounds are per session and sessions are bounded only by `max_sessions` (1,000) and a retention timer (3,600 s default), so the worst-case aggregate is of the order of 1,000 x 1,024 x 64 KiB; there is no global knowledge cap. Reconnecting creates a new session. What the save does at the 64 MiB snapshot bound was not traced. The research-only shared knowledge bucket is uncapped (not in the default build). Native session ownership and reconnect or resume are open work: sessions are not tenanted.

### N12. Key material at rest and in memory

- **Asset**: the node's Ed25519 mesh and publication key.
- **Adversary**: a local reader of the state directory, a core dump, swap.
- **Enforced**: the key file is created with mode 0600 on Unix (`namespace.rs` `load_or_create_key`, `:613-636`); handshake secrets and ephemeral state zeroize on drop (`handshake.rs` derives `Zeroize` and `ZeroizeOnDrop`), and `ed25519_dalek::SigningKey` zeroizes on drop (`identity.rs`).
- **Residual**: the seed is stored as a plain file and returned from `load_or_create_key` as a plain `[u8; 32]`; no zeroizing wrapper was found on it. Windows access control is the operator's (`docs/NATIVE_OPERATIONS.md`). The earlier claim that every private-key struct in `spine-crypto` and `spine-agentic` zeroizes is out of scope here: those are research crates and no code under `src/` uses them. No automated encrypted backup or key escrow exists. Name publication keys are separate files owned by the publisher.

### N13. Dependencies and build

- **Asset**: the supply chain.
- **Enforced**: CI runs `cargo audit` and `cargo deny` (including for `research/`), the MSRV job uses `--locked`, and `release_gate.py` runs the same deny checks (verification log, `fce11b1`). At 2026-11-20 the audit job reported zero vulnerabilities and 14 unmaintained-crate warnings; the job has failed intermittently posting its result to the GitHub API with no finding behind it.
- **Residual**: the unmaintained warnings are accepted, not fixed; the figures above are from the log, not from a run for this document.

## Deployed mesh handshake

Source: `src/spine-session/src/handshake.rs` (`spine-mesh-hs-v1`); full description in `HANDSHAKE-REVIEW.md`. **Not externally reviewed.**

| Role | Primitive |
|---|---|
| Key exchange / KEM | Ephemeral ML-KEM-768 (no Diffie-Hellman; no hybrid) |
| Signature | Ed25519 verified with `verify_strict` (`handshake.rs:168`; name records `spine-name/src/record.rs`; agent identities `spine-mesh/src/identity.rs`; `spine-authority` grants, invocations, receipts and revocations). An audit found no other Ed25519 verification site in `src/` at the time; not repeated for this document beyond a search of `verify` and `verify_strict` calls. |
| KDF | HKDF-SHA-256, salt = SHA-256 of both handshake messages, one key per direction |
| AEAD | AES-256-GCM, counter nonce, counter as AAD |

Properties the code does not provide:
- No key confirmation: no message under the session keys is exchanged or required to finish the handshake.
- No identity hiding: both identities and signatures travel in clear.
- No rekeying; a session uses its two keys for its lifetime.
- The initiator signature covers only its label, ephemeral key and identity (`Initiator::start_with_rng`, `:218-222`), not the responder, so a recorded first message is replayable; a replayed hello makes the responder complete the handshake, and the replayer cannot derive session keys (module docs, `handshake.rs:52-58`).
- Authentication is classical (Ed25519); only the key exchange is post-quantum.
- No binding to an outer channel (N3).

## Not deployed / research

The following are not in the default daemon and are retained as design or research material. None is a mitigation.

- Chameleon key, latent-space encoding and morphing: `research/spine-protocol-research`. Removed from the native wire on 2026-11-21.
- X3DH, double ratchet, X25519 and ChaCha20-Poly1305 "security levels", the RLWE KEM and the RLWE plus ML-KEM hybrid: `research/spine-crypto`. The security-levels table that was here described example output of that crate.
- Stake-weighted consensus, reputation, swarm coordination, Sybil resistance by stake: `research/spine-cluster` and `research/spine-agentic`.
- The legacy web bridge `research/spine-human` and its claims of process isolation.

## Explicit assumptions

1. ML-KEM-768, Ed25519, SHA-256, HKDF and AES-256-GCM assumptions hold for the mesh handshake; rustls and the platform trust store behave for TLS.
2. The operating system's CSPRNG is sound (`OsRng` in `handshake.rs`).
3. Rust memory safety holds in the dependency tree; `unsafe` kernel and FFI paths have not been run under Miri or sanitizers (open item of Stage 12).
4. Clocks agree within the 300 s skew the name layer allows.
5. An operator who sets `allow_unauthenticated_remote` has a network that is trusted.
6. The host's file permissions protect the state directory.

## Known limitations

1. No anonymity: IP addresses are visible to peers and sizes and timing to observers.
2. No authorisation on the native wire (risk 1).
3. Trust on first use for unconfigured seeds, once per process.
4. Side-channel resistance has not been assessed.
5. Native sessions are not tenanted; the HTTP gateway token and mutual TLS define no per-user roles.
6. No external review of the handshake, the authority crate, the succession design or the wire.

## Incident response

What exists in the code, and nothing more:

1. **Name or key compromise**: publish a tombstone (`spine name revoke`), or rotate and then retire through a succession log (`spine name rotate`, `retire`). A tombstone lives at most 7 days and can be evicted; a retire is permanent only for verifiers that hold the log. Distribute the log to peers (it travels with later records) because nothing pushes it.
2. **Node key compromise**: the node key is also its DHT position and its agent id; changing it changes the node's identity and does not transfer ownership of old names (`docs/NATIVE_OPERATIONS.md`, back up and recover). Peers' pins for the old key must be updated by an operator channel.
3. **Equivocation detected**: the name fails closed at the node that holds the proof; there is no operator action that spreads it faster than lookups.
4. **Resource abuse**: no delete on the wire; restart does not clear the store.
5. **Vulnerability reports**: `SECURITY.md`.

Earlier text here promised revocation "via directory", stake-based Sybil response, an "emergency upgrade mechanism" and "automatic restart with fresh state". No such mechanism was found in the code, and they are not listed.

## References

- `docs/spec/00-wire.md`, `01-session.md`, `02-names.md` (especially 10.9 to 10.11), `03-authority.md` (Node enforcement, Durable ledger), `04-objects.md`
- `docs/NATIVE_OPERATIONS.md`, `HANDSHAKE-REVIEW.md`, `SECURITY_AUDIT.md`
- `COMPLETION_PLAN.md`, verification log
- Signal Protocol specification (X3DH, Double Ratchet): design reference only, not deployed
- Douceur 2002 (Sybil attack); NIST FIPS 203

## Not covered

- Per-user tenancy, multi-user native sessions and historical credential revocation.
- The task service (`namespace.tasks_enabled`, off by default) beyond what `docs/NATIVE_OPERATIONS.md` says: encrypted TCP, pinned recipient key, explicit coordinator keys, read-only HTML parse and hash tasks. Not reviewed here.
- The `research` feature build of the daemon (cluster listener, human transpiler, knowledge broadcast) and everything under `research/`.
- The Go, Python, JS, browser and fuzz code, which no CI job builds; `extension/`, which talks to a port nothing listens on.
- The MCP and gRPC bridges beyond the statement in `docs/NATIVE_OPERATIONS.md` that the MCP adapter is read-only.
- Memory safety of `unsafe` kernel and FFI paths; formal models in `formal/` (written, not executed as proofs).
- Hardware backends (`IbVerbsRdma`, `GpuDirectRdma`, io_uring), which are typed stubs or simulations (see the marker table in `SECURITY_AUDIT.md`).
- Timing and side channels; traffic analysis resistance; load and sustained-resource behaviour.
- Anything about deployments: operating system hardening, container isolation, network egress policy.
