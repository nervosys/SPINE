# SPINE Security Audit (internal review, refreshed 2026-11-23)

**Status of this document.** Refreshed 2026-11-23 from the source at `a4efd03` (branch `completion/agent-web`), for the seven-layer design and the verified behaviour of the code. This is an internal review by the project, not an independent audit. It certifies nothing: no FIPS 140-3, CMMC, SOC or similar claim is made. The previous text carried the title "SPINE v1.2.1 Security Audit", a v1.3.0 "closes every BLOCKER" statement, a CMMC practice table and test counts of 1,072; those were written against code that has since changed (the native wire, the Chameleon layer, the gateway, the crate set) and are replaced or removed here. Findings about code that no longer exists are kept only as short history. Nothing in this document was run: figures marked "log" come from the dated verification log in `COMPLETION_PLAN.md`; everything else was read in the source. Companion document: `THREAT_MODEL.md`, which states the adversary model and the per-threat residuals in full.

## 0. What is open, stated first

1. **Nothing authorises a native command.** The authority gate (`src/spine-core/src/authority.rs`), its durable ledger (`authority_ledger.rs`) and the command classification are library code that the daemon does not call. spine-wire v1 has no carrier for a grant chain or invocation (`docs/spec/00-wire.md`, G8); the handshake's optional client key (`IDENTIFY`, 6.6) is recorded and logged and grants nothing, and it is not bound to the channel, so a relay can carry it. `[authority] mode = "required"` is refused at start-up (`config.rs` `AuthorityConfig::validate`). **Any client that can connect has operator-level access to all 19 commands**, including `navigate`, `execute_binary` and `mesh_call`. No receipt is issued. Reachability and, optionally, a TLS client certificate are the only controls (`SpineConfig::validate`, `config.rs:562-598`).
2. **No external cryptographic review of the handshake, and the session layer (Stage 8) is not built.** The mesh handshake (`spine-session` `handshake.rs`) is the only SPINE-level encrypted channel; the native client wire does not use it.
3. **The log in a resolve answer is only as long as the node holds it** (`NameResolution::Resolved { record, provenance, log }`, with `retired`, `compromised` and `revoked`; `docs/spec/00-wire.md` 12.5). Clients verify the evidence from the name's inception key (`spine-agent` `names.rs`). Open: a node that is behind (`02-names.md` 10.8), and a mesh-only compromise answered as `unavailable`.
4. **Publisher keys are free to mint**, so per-publisher quotas bound a key, not a party.
5. **Replica records are in memory only; an evicted tombstone allows resurrection; equivocation proofs spread only when asked for.**
6. **One host call in `execute_binary` is not interruptible, and compile time is outside the 2 s deadline.**
7. **QUIC receive windows are library defaults, and the daemon does not use the mesh QUIC or WebSocket transports.**
8. `UringRing` (behind the non-default `io-uring` feature) is a queue with no kernel ring; its `submit_and_wait` returns an `Unsupported` error and no longer reports I/O that never happened (section 8). It is not reachable from the daemon.

## 1. Dependency pass

The last recorded results are in the verification log, not from a run for this document.

| Date and commit | Result |
| --- | --- |
| 2026-11-20, `0a2b5de` | `cargo audit`: zero vulnerabilities, 14 unmaintained-crate warnings that do not fail the job. A `rustix` unsoundness advisory (RUSTSEC-2026-0486) was cleared by the lockfile bump. |
| 2026-11-20, `7ea7eb8` | `cargo deny` failed on one unsound advisory in `tokio`; addressed. |
| 2026-11-21 to 2026-11-23, `fd1b31a` onward | `deny` passed on every later commit read. The `audit` job failed intermittently when posting its check result to the GitHub API ("Not Found") after reporting no vulnerabilities; cause unidentified. |
| 2026-11-23, `fce11b1` | CI `release_gate.py` runs the same `cargo deny` checks as CI, including for `research/`; the MSRV job uses `--locked`. |

Open: the 14 unmaintained warnings are accepted and not enumerated here (the list in the earlier version named crates that have since been removed, for example `tabled`); `rustls-pemfile = "2"` (`Cargo.toml:101`) and `bincode = "1.3"` (`spine-transport/Cargo.toml:90`, a dev-dependency block) were tracked as deprecated in the earlier version; whether either is still flagged was not checked. Dependencies of `research/` have their own `Cargo.lock`.

## 2. Cryptographic surface

What the stack crates under `src/` use, from their manifests and code. "Validated module" is a per-binary certification process; none of these crates is one.

| Primitive | Where | Crate and version (manifest) | Notes |
| --- | --- | --- | --- |
| ML-KEM-768 (FIPS 203) | mesh handshake (`spine-session`) | `ml-kem 0.2` | Ephemeral per connection |
| Ed25519 | handshake, name records, mesh envelopes, authority grants, invocations, receipts, revocations | `ed25519-dalek 2` | `verify_strict` at `handshake.rs:168`, in `spine-name` records, `spine-mesh` `identity.rs` and throughout `spine-authority` (`grant.rs`, `invocation.rs`, `receipt.rs`, `revocation.rs`) |
| AES-256-GCM | mesh frames | `aes-gcm 0.10` | Counter nonce per direction, counter as AAD (`handshake.rs:483`, `:509`) |
| HKDF-SHA-256 | handshake key derivation | `hkdf 0.12` | Salt = hash of both handshake messages |
| SHA-256 | names, resources, authority ids, operation hashes | `sha2 0.10` | |
| TLS 1.3 | native TCP and WebSocket, gateway and metrics when fronted | `rustls 0.23` (Ring provider chosen explicitly by core and SDK, `docs/NATIVE_OPERATIONS.md`) | |
| OS randomness | ephemeral KEM keys | `rand 0.8` `OsRng` (`handshake.rs:80`, `:202`, `:335`) | The `_with_rng` seams require `CryptoRng` and exist for tests |

Not on the daemon path and not reviewed here: the RLWE lattice code, X3DH, the Chameleon encoder and the Titans codecs (`research/`). The earlier table's rows for HMAC, ECDSA P-256 and the SP 800-90A comparison were not re-verified and are removed.

**FIPS posture.** No crate is a FIPS 140-3 validated module. The `spine-gateway` crate has a `fips` feature (`Cargo.toml:73`, `fips = ["rustls/aws_lc_rs"]`); when enabled the gateway installs the `aws-lc-rs` provider as the rustls process default at start-up (`spine-gateway/src/main.rs:566-570`, `install_fips_provider`); that is an integration point, not a validated module, and the deployer must still build `aws-lc-rs` in FIPS mode. `spine-core` has no such feature and selects the Ring provider explicitly, so the daemon's native TLS is not affected by it. Treat any FIPS statement in older documents as unverified.

**Key memory.** Handshake state and ephemeral secrets derive `Zeroize` and `ZeroizeOnDrop` (`handshake.rs:178`, `:413`), and `ed25519_dalek::SigningKey` zeroizes on drop (`identity.rs`). The daemon's node key is read from a file (created with mode 0600 on Unix; an existing wider file draws a start-up warning, `namespace.rs` `load_or_create_key`) and held as a `SigningKey`, which wipes itself on drop. The hex text read from the file, the seed array (decoded or generated), the hex text written to a new file and the `to_bytes()` copy passed to `SigningIdentity::from_seed` are `zeroize::Zeroizing` and wiped on drop (best effort: copies made by the operating system (page cache, swap, core dumps), the allocator (a string buffer that grew) or the compiler (registers, stack slots), and the seed copy `spine-mesh` makes inside its own identity type, are out of reach). The earlier "CLOSED in v1.3.0: zeroize on `RingElement`, `QuantumKeyPair`, `MlKemKeyPair`, `Ed25519Keypair` ..." refers to research crates; nothing under `src/` uses them, and `SECURITY.md` carried the same list.

### 2.1 Closed findings, kept as history

The full write-ups are in the git history of this file. Their current status:

- **Chameleon layer keyed with 64 bits (Phases 47 and 48), and the latent AEAD path's 32-bit session nonce (Phase 45).** Concern code that is no longer on the native wire (removed 2026-11-21; `research/spine-protocol-research`). Nothing to fix in the daemon; nothing to rely on either.
- **Ephemeral mesh keys from a counter-seeded generator (Phase 42).** The handshake now draws them from `OsRng`. This is the one finding that mattered to the deployed channel; see `HANDSHAKE-REVIEW.md`.
- **Unbounded record store (Phase 44).** `RecordStore` is bounded (`DEFAULT_CAPACITY`, furthest-first eviction, a refusal for a record further than everything held, tombstone share, lapsed-first sweep; decisions 47, 48, 52 of `docs/spec/02-names.md`). The mesh `NameAnnounce` path remains open by design: any peer can announce a valid record, and a key costs nothing (risk 4).

All four earlier findings shared one shape (vetted primitives, standard composition, inadequate entropy or bounds fed in). It is worth continuing to look for it.

## 3. Attack-surface map (ATT&CK-shaped, replaces the v15 table)

| Technique | Surface | Status at `a4efd03` |
| --- | --- | --- |
| T1190 Exploit public-facing application | Native port; HTTP gateway; metrics listener | Native: no authorisation (section 0, item 1); loopback bind by default and a refusal to bind wide without mutual TLS or an explicit flag (`config.rs:592-598`). Gateway: refuses to start without `SPINE_GATEWAY_BEARER_TOKEN` or `SPINE_GATEWAY_ALLOW_UNAUTH=1` (`auth.rs` `AuthMode::resolve`); one shared secret, no roles. Metrics and `/ready`: unauthenticated, loopback by default |
| T1499 Endpoint denial of service | Connections, frames, sessions, mesh listeners, programs | Bounded by component limits listed in `THREAT_MODEL.md` N6, N7, N11; no process-wide memory budget; no sustained-load measurement |
| T1059 Command and scripting | `execute_binary` | Fuel, host budgets, memory limit, instruction cap, 2 s deadline checked at host calls, capability check on actions, 4 permits (`spine-wasm`, `main.rs:1062`); the capability list is declared by the program and checked against what it did, so it is a consistency check, not a grant; any client may run programs |
| T1090 / T1046 Outbound reach, scanning | `navigate` and program navigation | Single resolution, every answer checked, connection pinned, no redirects, ports 80 and 443 by default, per-chunk read timeout, per-caller cap, link-local exception guard (`outbound.rs`); see `THREAT_MODEL.md` N8 |
| T1040 Network sniffing | Native wire; metrics; gateway | Native: TLS if enabled, plaintext otherwise (default on loopback). Mesh: encrypted always. Metrics and gateway: plain HTTP unless fronted |
| T1557 Adversary in the middle | Mesh dial; QUIC mesh | TCP mesh: pinned keys, signed ephemeral exchange, trust on first use for an unconfigured seed once per process. Mesh QUIC and WebSocket: library only; handshake not bound to TLS |
| T1565 Data manipulation | Name records, resources | Strict Ed25519 signatures, 7-day TTL, post-dating refusal, resource ids that commit to type and bytes, succession logs; see N4, N5, N10 |
| T1098 Account manipulation | Node key, name keys | Plain file at rest, 0600 on Unix, plain array in memory; rotation exists as library, CLI and daemon publish path, not as a daemon command |
| T1036 Masquerading, protocol fingerprint | Wire format | **No mitigation.** The earlier "MITIGATED: Chameleon" row is withdrawn: the layer is not on the native wire. |
| T1518 Software discovery | `capability_query`, `namespace_status`, `/ready` | By design; the metrics labels carry no name, key, peer, session or address |

Residual ATT&CK risk: T1190 with the native port reachable by an untrusted client, which is full control of the node; T1557 on first contact with an unconfigured seed.

## 4. CMMC 2.0 and NIST SP 800-171

**Not assessed.** The earlier table marked practices MET on the strength of a bearer token that was opt-in (since closed), a single-codec `source_hash` that is in a research crate, "anomaly signal via Titans surprise scores" in a research crate, and TLS that is off by default. A practice-level claim cannot be made from this review. Practices that depend on facts this document establishes, for whoever does assess:

- AC.L1-3.1.1 / 3.1.2 (authorised users and functions): the native wire has no per-user authorisation (section 0, item 1); the gateway has one shared secret.
- AU.L2-3.3.1 / 3.3.2 (audit records): `tracing` output exists; no retention sink and no per-user identity (a proven `IDENTIFY` key is logged at debug level; it is not authorisation).
- IA.L2-3.5.3 (multi-factor): not addressed by the stack.
- SC.L2-3.13.8 (cryptographic protection in transit): TLS for the native wire only when enabled; mesh always encrypted.
- SC.L2-3.13.11 (FIPS-validated cryptography): no validated module.
- SC.L2-3.13.10, 3.13.16: not established.

## 5. Controls confirmed present at `a4efd03`

Each was read in the source; behaviour under load was not measured.

1. Body limits on the gateway: 8 MiB on the router and 40 MiB on `/v1/resources` (`spine-gateway/src/main.rs:640`, `:653`).
2. Gateway bearer authentication with constant-time comparison and a refusal to start with an ambiguous or missing choice (`auth.rs`).
3. Wasm fuel metering, host budgets, memory limit, wall-clock deadline at host calls, instruction cap and four executor permits (`spine-wasm`, `main.rs`).
4. Outbound fetch policy (`outbound.rs`), including `allowed_ports` before DNS, per-caller cap and the link-local exception guard.
5. Native admission and timeouts (`THREAT_MODEL.md` N6); `CLOSE(shutting_down)` on drain; a failed program answers `execution_failed` instead of aborting the daemon (the earlier `ExecuteBinary` abort under `panic = "abort"` is fixed).
6. Mesh authentication: derived agent ids, pins with a hard mismatch failure, response and hello checks against the session key, forged-sender tests on real TCP connections.
7. Name lifetime, tombstones, succession logs, equivocation proofs and their persistence, as in `THREAT_MODEL.md` N4 and N5.
8. Resource store: content-committing ids, fair share, post-dated refusal, owner-checked delete in the store API (not on the wire).
9. Knowledge and session bounds (1,024 entries, 64 KiB per entry, 128 query results, 256 state variables, and 32 MiB of knowledge for the whole daemon).
10. `/metrics` renders every family with HELP and TYPE from fixed label sets; `/ready` has fixed reasons and warnings (`docs/NATIVE_OPERATIONS.md`).

## 6. Known residuals

Ratings are the reviewer's, not a standard score. "Open" means no change in the code addresses it.

| Item | Rating | Status | Evidence |
| --- | --- | --- | --- |
| No authorisation on the native wire | High | Open. Gate built, not enforced, no carrier, `required` refused at start-up | `config.rs` `AuthorityConfig::validate`; `docs/spec/03-authority.md` "What is not done" |
| Handshake not externally reviewed; session layer not built | High | Open | `HANDSHAKE-REVIEW.md`; `COMPLETION_PLAN.md` Stage 8 |
| Native wire plaintext by default (loopback); wide bind allowed with an explicit flag | Medium | By design, documented | `config.rs:584-598` |
| Resolve answer log only as long as the node holds it; mesh-only compromise answered `unavailable` | Low | Open (the log and the outcomes are on the wire since 2026-11-24) | `spine-protocol/src/names.rs`; `spine-agent/src/names.rs`; `02-names.md` decision 53 |
| Key succession weak window before entry 1; lost current key unrecoverable | Medium | By design, documented | `02-names.md` 10.4, 10.9 |
| Evicted tombstone allows resurrection (at most 7 days); proofs not pushed | Medium | Open | `02-names.md` decisions 51, 52 |
| Per-publisher quotas bound a key, not a party | Medium | Open | `resources.rs:46-49` |
| Mesh replica records in memory only | Low | Open | `02-names.md` 10.11 item 5 |
| `execute_binary`: one host call and compile time outside the deadline | Low | Open | `spine-wasm` `DEFAULT_EXECUTION_DEADLINE` docs |
| Aggregate knowledge memory had no global cap (about 1,000 sessions x 1,024 entries x 64 KiB) | Medium | Fixed: 32 MiB daemon-wide byte budget (`KnowledgeBudget`), reserved before every store and released on reaping; unit-tested including concurrent reservations | `knowledge.rs`, `main.rs` `append_knowledge` |
| Mesh QUIC receive windows are library defaults; 64-stream cap and QUIC handshake timeout untested | Low | Open; transport unused by the daemon | log, `057b2c2` |
| Mesh QUIC handshake not bound to the TLS channel | Medium for users of the library | Open; transport unused by the daemon | `mesh_quic.rs` module docs |
| Trust on first use for an unconfigured seed; pins forgotten on restart; pin tables stop at 1,024 | Medium | Open | `mesh_tcp.rs:499`, `:69` |
| Handshake: no key confirmation, no identity hiding, no rekeying, replayable first message | Low to medium | By design, documented | `handshake.rs` module docs |
| Node key file plain on disk; seed copies made by the OS, allocator, compiler and `spine-mesh`'s identity type not wiped | Low | Partly fixed: held as a self-wiping `SigningKey`, the seed array, hex text and `to_bytes()` copy in `spine-core` are `Zeroizing`, wider file permissions warned; the file stays plain text | `namespace.rs` `load_or_create_key` |
| An unknown host name and an internal one answer with different outbound codes | Low | Open | log, `94371e7` |
| Delete of resources not on the wire | Low | Open | log, `53f32bd` |
| Metrics, `/ready`, dashboard and gateway have no per-user control; example configs showed wide binds | Low to medium | Partly fixed (templates), docs pages open | log, `9ecfac7`, `0009bf9` |
| Documents out of step with the code | Low | See section 9 | |

## 7. Verification

Most recent CI figures on Linux: **63 suites, 1,593 passed, 0 failed, 3 ignored at `fce11b1`, 2026-11-23** (log). Every job read passed at that commit: lint, docs, MSRV, coverage, `research`, `wire-vectors`, and the tests on Linux, macOS and Windows. These figures were not re-run for this document and are not a statement about `a4efd03`. The earlier "1,072 passed / 0 failed / 5 ignored" figure and the "Build clean on Rust stable" line are replaced by this. What is not established by any job: the Go, Python, JS, browser and fuzz code (nothing builds them), the QUIC listener of the daemon, a research-off peer interoperating with a research-on peer, the formal models (written, not executed as proofs), benchmarks with security modes, Miri or sanitizers on the `unsafe` paths.

## 8. Stub and simulation markers

Scope: `src/` (not `research/`), the patterns `todo!`, `unimplemented!`, `TODO`, `FIXME`, `stub`, `simulat`, `placeholder`, `mock` (case-insensitive on the first letter where noted), searched 2026-11-23 at `a4efd03`. **68 hits in 22 files. Zero are `todo!`, `unimplemented!`, `TODO` or `FIXME`.** Classified by where the hit sits:

| Class | Hits | Files |
| --- | --- | --- |
| Library or build code (not in a test module, bench or example) | 16 | compiler 4, ct 1, grpc 2, rdma 3, uring 5, parser 1 |
| Inside test modules or test helpers | 22 | pool 6, mesh_tcp 5, cache 3, parser 2, congestion 1, naming 1, tls 1, model 1, rdma 1, authority tests 1 |
| Benches | 18 | traditional_comparison 12, tcp_comparison 4, transport_bench 1, agentic_ai_workload 1 |
| Examples | 12 | swarm_demo 6, web_intelligence_demo 5, agent_autonomous 1 |

The 16 library hits and the behaviours that matter, with reachability from the daemon (`spine-core` default build: it depends on `spine-wasm`, `spine-transport`, `spine-mesh`, `spine-name`, `spine-protocol`, `spine-storage`; not on `spine-compiler` or `spine-grpc`):

| File:line | What it is | Verdict | Reachable from the daemon |
| --- | --- | --- | --- |
| `spine-compiler/src/lib.rs:476, 499, 509, 547` | `Jump(0)`, `JumpIf(0)`, `JumpIfNot(0)` operands pushed as placeholders and overwritten at `:488`, `:519-520`, `:556` | Real (back-patching) | No. `spine-compiler` is a dependency of the gateway, the SDK (`spine-agent`), `spine-ffi` and `spine-web`, not of `spine-core`; the daemon runs binaries the client compiled |
| `spine-core/src/ct.rs:67` | A comment recording that placeholder SPKIs were removed; the default policy is disabled with no trusted logs | Real, but the module is unused: `mod ct;` sits under `#[allow(dead_code)]` (`main.rs:36-37`) and nothing outside `ct.rs` refers to `CtPolicy` or `CtEnforcement` | No. Certificate Transparency enforcement is not applied to any connection, whatever `SECURITY.md` says about `CtPolicy::default()` |
| `spine-grpc/build.rs:1`, `src/lib.rs:53` | "client + server stubs" generated by tonic from the `.proto` | Real (generated code) | No (separate bridge crate) |
| `spine-transport/src/rdma.rs:382, 498, 669` | Typed stubs for InfiniBand verbs and GPU-Direct (`IbVerbsRdma`, `GpuDirectRdma`) and a placeholder comment for pointer types | Returns an explicit `RdmaError::HardwareUnavailable` (`:430`, `:465`, `:485`); the `rdma` feature is off by default; `LocalShmRdma` is a real in-process ring | No. The module is compiled in the default `spine-transport` build and nothing outside it calls it |
| `spine-transport/src/uring.rs:374, 392, 431, 438, 442` | `UringRing` is a "stub": `submit_and_wait` pops each pending op and reports it complete with `result = op.len`, with no kernel I/O (`:430-453`) | **Fixed after this review: `submit_and_wait` now returns an `Unsupported` I/O error and the docs say io_uring is not implemented. The text of this row describes the code at `a4efd03`: it returned fabricated success.** Compiled only on Linux with the non-default `io-uring` feature (`lib.rs:53-57`); no other crate enables the feature. `uring_backend.rs` builds on it, and the crate's top-level docs list "io_uring Support" as a feature | No. This is the one item the Stage 12 review should resolve: make `UringRing::new` or `submit_and_wait` return an unsupported error, or delete the module |
| `spine-parser/src/lib.rs:263` | Reads the HTML `placeholder` attribute of a form field | Not a marker (data) | Yes, as ordinary parsing |

Behaviours that are marker-like without containing the words, and are reachable (`main.rs`, `BrowserCommand::ExecuteBinary`):

| File:line | What it does | Reachable |
| --- | --- | --- |
| `spine-core/src/main.rs:1250-1259` | At `a4efd03` a program's `search` action was logged and, without the `research` feature, discarded: it reported `executed` and performed no search. Fixed: the program is refused up front with `execution_failed` naming the action (`actions.rs` `unsupported_action`) | Fixed |
| `spine-core/src/main.rs:1294-1304` | At `a4efd03` `query_knowledge` and `reason` actions from a program were only logged. Fixed the same way, in every build | Fixed |
| `spine-mesh/src/naming_mesh.rs:1324` | Application RPC with no handler answers `{"error":{"code":"unsupported","message":"application RPC is disabled"}}` | Yes (when tasks are not enabled): an explicit unsupported answer |
| `spine-core/src/main.rs:1891, 1899` | Legacy swarm search and task events from the cluster listener log "unsupported" and do nothing | Only in a `research` build (`#[cfg(feature = "research")]`, `main.rs:1786-1794`) |

Markers earlier reviews named that no longer exist: the `Search`, `Click` and `Type` command stubs (the 19-command set has none), the stale `supports_chameleon: true` flag (now `false`, `main.rs:1508`), and `signed_frame.rs` and `negotiation.rs` with no caller (removed). The remaining 22 test, 18 bench and 12 example hits are fixtures and demonstrations; the benches `traditional_comparison.rs` and `tcp_comparison.rs` compare against hand-rolled simulations and their own headers say they were audited as not like-for-like. The plan's Stage 12 item ("confirm each is real, test-only, or returns an explicit unsupported error") is therefore met for every library hit except `uring.rs`.

## 9. Documents found out of step with the code during this refresh

Not changed here unless this file or `THREAT_MODEL.md`, `README.md` or `SECURITY.md`:

- Corrected afterwards: `docs/NATIVE_OPERATIONS.md` said the daemon negotiates `max_inflight` of 1; `DAEMON_MAX_INFLIGHT` is 8 (`spine-protocol/src/native.rs:127`) and each connection runs a reader, command loop and writer. Commands of one connection still run one at a time. The same document now states the knowledge bounds (1,024 entries per session, 32 MiB daemon-wide).
- `docs/spec/00-wire.md` was re-checked: it does not name `put_tracked` and its 256 MiB resource limit is the accounted bytes of `resources.rs` `MAX_STORED_BYTES`, with the 64 MiB per-publisher share; nothing needed correcting there. `docs/AGENT_WEB_QUICKSTART.md` called the 256 MiB "serialized" and now says accounted.

## 10. Not covered

- Any third-party audit; the handshake, the authority crate and the succession design have none.
- The `research/` workspace and the `research` feature build of the daemon.
- Behaviour under load; memory use; performance claims.
- The Go, Python, JS, browser extension and fuzz code.
- `unsafe` code paths under Miri or sanitizers.
- Operating-system and deployment hardening; container and network policy.
- Compliance frameworks (CMMC, NIST SP 800-171, FIPS 140-3): not assessed.
- The task service beyond the statements in `docs/NATIVE_OPERATIONS.md`.
- The MCP and gRPC bridges, beyond their being outside the daemon.
- The exact list of 14 unmaintained dependencies.
