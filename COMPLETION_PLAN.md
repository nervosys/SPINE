# SPINE Project Completion Plan

Revised 2026-11-19. This replaces the 2026-10-02 plan and carries every item that was still open in it. Nothing has changed in the repository since that date; what changed is the world around it: the toolchain, the advisory database, and the disk.

"Complete" means: independent agents can discover capabilities, resolve signed names, retrieve verified resources, invoke tools, stream model output, and coordinate work across nodes; the security boundaries are documented and reviewed; validation is reproducible on every supported platform; and a release installs from published packages and archives.

## Where the project stands

**History recovered.** Claude Code built SPINE through Phase 48 under the earlier project name Hyperlight; those transcripts are no longer on disk, and their record is `HANDOFF.md`. A Codex session on 2026-10-01/02 wrote the first version of this plan and then implemented against it in five stages. It stopped after the session-retention stage with the reply "Independent security review, native tenancy, and cross-platform validation remain open."

**What that session left working** (Windows x64 only, verified 2026-10-02 on Rust 1.98.1): 1,478 tests across 70 suites, Clippy, formatting, Rust 1.88 compilation, and a clean-install archive passing 19 acceptance checks. The supported subset is seed discovery, persistent signed names with owner renewal, verified resource transfer, two read-only native tools (HTML parsing, SHA-256), OpenAI-compatible model forwarding with IronGate routing, signed durable read-only mesh tasks, TCP/WSS mTLS, a native MCP stdio adapter, bounded outbound navigation, readiness diagnostics, and bounded session retention. Detail and limits are in [the status report](docs/COMPLETION_STATUS.md).

**What is true today and was not then:**

| Finding (checked 2026-11-19) | Consequence |
| --- | --- |
| All of the October work was uncommitted: 46 modified and 27 untracked files on `master` at `38e6ad5` | Resolved the same day: committed to `completion/agent-web` and pushed as draft pull request #49. |
| `cargo audit` reports 34 vulnerabilities across 14 locked crate versions; it reported zero on 2026-10-02. `cargo deny check advisories` fails. | The security gate that was green is red. Stage 2. |
| The dependency exception register was due for review on 2026-11-02 | Overdue; unmaintained warnings grew from 11 to 17, plus 20 unsound and 2 yanked. |
| Toolchain is now Rust 1.100.0; all evidence is from 1.98.1. Cargo 1.100 emits about 50 new manifest warnings (unused and redundant dependencies). | The baseline must be re-established before new work. Stage 1. |
| `v2.0.2` is already tagged and released on GitHub with binaries, from commit `d8b105a` | The working tree also says 2.0.2 and the local candidate is named `spine-2.0.2-dev`. This work cannot ship as 2.0.2. |
| No CI run has ever seen this work. Remote CI is green for `38e6ad5` only. | Linux and macOS behaviour of the new code is unknown. |
| Drive E: has 0 bytes free. Cargo now builds into a shared target directory on C: (218 GB free); the status report still says `E:/cargo-target`. | Evidence paths are stale; disk has stopped builds on this machine before. |
| 15 Dependabot pull requests are open; #10, #12 and #15 are already superseded on `master` | Triage belongs with Stage 2. |
| The crates.io publish token pasted into a transcript on 2026-08-24 has no recorded revocation | Three months live. Account holder action, listed first under external items. |

**Verified today on Rust 1.100.0:** all 1,478 tests pass and formatting is clean, but Clippy fails on a lint the new toolchain added, so `scripts/verify.sh` exits 1. Details are in the *Verification log* at the end.

## Order of work

Stages 0 to 2 restore a trustworthy starting point and are short. Stages 3 to 5 are the remaining implementation. Stages 6 to 8 are evidence and release. External items run in parallel from day one because they have the longest lead time.

Check an item only when its stated result has been demonstrated on the commit in question. Record the command and log beside it in the status report.

## External items (start now, owner: account holder)

These cannot be closed from inside the repository.

- [ ] Revoke the crates.io publish token identified in `HANDOFF.md` and record the confirmation without copying its value.
- [ ] Commission the independent handshake review using `HANDSHAKE-REVIEW.md` and [the review packet](docs/CRYPTO_REVIEW_PACKET.md). Until findings are resolved, the custom handshake is described as unreviewed.
- [ ] Decide the release scope: which of Python, JavaScript/WASM, Go, C, GUI, browser extension, embedded, GPU, Kubernetes, RDMA and io_uring are supported, experimental, or deferred for this release. Stage 6 only does the work this decision calls for.
- [ ] Decide the release version (see Stage 8) and whether the result is published to crates.io.
- [ ] Supply real GPU/RDMA hardware evidence, or confirm those backends stay experimental.
- [ ] Decide whether benchmark history may be served from a public `gh-pages` branch.

## Stage 0. Secure the work

- [ ] Confirm no other session is writing to this working tree or moving refs. Seven unrelated Cargo builds were running on this machine during today's check.
- [x] Create a branch for the October work and commit it in reviewable groups. Done 2026-11-19 as five commits on `completion/agent-web`, grouped by layer (protocol and names; mesh and SDK; core, gateway and CLI; scripts and examples; documentation) because the features share files and could not be separated cleanly.
- [x] Before committing, re-run `python scripts/scan_secrets.py` and confirm `artifacts/`, `build_completion*` logs, generated identities and `*.pem`/`*.key` files stay ignored.
- [x] Push the branch and open a pull request so CI runs on Linux, macOS and Windows. Done 2026-11-19: draft pull request #49.
- [ ] Free space on E: or remove it from any build path; keep at least 30 GB free on the drive holding the Cargo target directory before a full workspace build.
- [x] Correct the stale environment facts in `docs/COMPLETION_STATUS.md` (target directory, free space, toolchain). Added as a dated update; the 2026-10-02 record is kept as it was.

**Done when:** the work exists in commits on a pushed branch, and a CI run has reported on all three platforms.

## Stage 1. Re-establish the baseline on the current toolchain

- [x] Fix the Clippy failure Rust 1.100 introduces at `src/spine-crypto/src/lib.rs:939` (`needless_range_loop`), then any further lints it was hiding in dependent crates. Done 2026-11-19; the CI toolchain also rejected the deprecated `fetch_update`, replaced with compare-exchange loops so 1.88 still compiles. CI lint, docs, coverage and tests on all three platforms passed on `2801d13`.
- [ ] `scripts/verify.sh 1478` passes on Rust 1.100.0, run unpiped on a quiet machine: 1,478 tests, 70 suites, 5 ignored, Clippy silent, formatting clean. Tests and formatting already pass as of 2026-11-19; Clippy does not. Clippy passes locally since `2801d13`. The full run has not been repeated after the Stage 2 lockfile change: the 2026-11-19 attempt was stopped by the system for low memory before any test ran.
- [ ] Resolve the new Cargo manifest warnings (about 20 unused dependencies and 30 redundant declarations) by removing what is unused; do not silence the lint. Partly done 2026-11-19: 32 unused dependencies and the redundant `homepage` fields removed. 13 unused-dependency warnings remain where the crate name still appears in cfg-gated code, examples or benches, plus the unused `readme` workspace field; each needs a per-target check.
- [ ] `cargo +1.88 check --workspace --exclude spine-browser --locked` passes.
- [ ] `python scripts/release_gate.py` passes end to end, including the 19 clean-install checks, after Stage 2 changes the lockfile.
- [ ] Fix every CI failure from the Stage 0 pull request. Expect platform differences in process tests, file permissions (Unix 0600 key creation is untested on a real Unix host), and path handling. On `2801d13` only `audit` and `deny` still fail; both are Stage 2.
- [ ] Update `EXPECTED_SUITES_MIN`, the expected total, and the README test badge only to match a complete run.

**Done when:** local gates and CI are green on the same commit.

## Stage 2. Close the dependency regression

All figures are from `cargo audit` on 2026-11-19 against the current lockfile. Re-run it first; the minimum fixed versions below are the lowest the advisories name and later advisories may raise them.

- [x] `wasmtime` 36.0.16 → at least 36.0.19 (7 advisories, two rated critical-impact miscompilation in Cranelift). Keep the matching Cranelift and internal packages aligned. Done: 36.0.23.
- [x] `rustls` 0.23.45 → at least 0.23.46 (4 advisories); `rustls-webpki` → at least 0.103.16; `webpki-roots` and `webpki-root-certs` → at least 1.0.10 (distrusted intermediates). Done: 0.23.46, 0.103.17, 1.0.12.
- [x] `quinn-proto` 0.11.17 → at least 0.11.18 (5 advisories, remote panics); `quinn-udp` → at least 0.5.16. Done: 0.11.19 and 0.5.16.
- [x] `zstd`: move the three crates still on `zstd = "0.12"` (`spine-protocol`, `spine-transport`, `spine-compiler`) and `spine-stream` to a fixed 0.13.x; this brings `zstd-safe` to at least 7.4 and `zstd-sys` to at least 2.1.2. Re-run the bounded-decompression and wire-compatibility tests, since this touches the native codec. Done: `zstd` 0.13.5, `zstd-safe` 7.4.1, `zstd-sys` 2.1.3; protocol, transport, stream and compiler tests passed.
- [ ] `libsqlite3-sys` 0.28 → at least 0.39 by upgrading `rusqlite` from 0.31 in `spine-storage`. This is a major upgrade under the namespace, resource, task and knowledge stores: re-run restart and backup/restore acceptance. **Open.** `rusqlite` moved to 0.40.2 (`libsqlite3-sys` 0.38.2), which is still affected by RUSTSEC-2026-0459. The fix needs `rusqlite` 0.41 or later, which requires rustc 1.89, one above the 1.88 MSRV. `rusqlite` 0.43 compiles with no source change. Decision needed: raise the MSRV to 1.89 (recommended) or make the SQLite backend optional.
- [x] `zip` 1.1.4 (6 advisories) arrives only as a build dependency of `utoipa-swagger-ui` 7.1.0 in `spine-gateway`. Upgrade `utoipa-swagger-ui` to a release on a fixed `zip`, or drop the bundled Swagger UI. Done: `utoipa` 6, `utoipa-swagger-ui` 10.0.2 and `axum` 0.8 in the gateway bring `zip` 9.7.3 (9.8.0 needs rustc 1.89). No source change was needed; the generated OpenAPI document has not been compared with the previous one.
- [x] `libz-sys` 1.1.29 → at least 1.1.33. Done: 1.1.33.
- [ ] Review the 20 unsound and 2 yanked warnings individually; replace yanked versions. Yanked versions replaced (0 remain). Two unsound warnings remain unreviewed: `tokio` RUSTSEC-2026-0386 and `rustix` RUSTSEC-2026-0486.
- [x] Review the exception register, due 2026-11-02: six new unmaintained packages (including `chrono`, `humantime`, `zbus_names`, `zvariant`, `zvariant_derive`) need a rationale and owner or a migration. Give `rustls-pemfile` priority; it parses certificates and keys. Reviewed 2026-11-19, next review 2026-12-19: `chrono`, `zbus_names`, `zvariant` and `zvariant_derive` added with rationale; `proc-macro-error` and `humantime` are gone from the lockfile. `rustls-pemfile` migration is still outstanding.
- [ ] Triage the 15 open Dependabot pull requests: close #10, #12 and #15 as superseded; fold #48 into this stage; decide #33 to #38 individually (several are major upgrades). #10, #12 and #15 closed 2026-11-19. #48 and #27 to #38 remain open.
- [ ] `cargo audit` reports zero vulnerabilities and `cargo deny check advisories licenses sources` passes locally and in CI. As of 2026-11-19: 1 vulnerability (down from 34), the SQLite advisory above; licenses and sources pass.

**Done when:** both tools are green on the Stage 1 commit and the register has a new review date.

## Stage 3. Finish the supported agent workflow

The remaining implementation in the namespace, resource, tool, model and coordination paths.

### Namespace lifecycle

- [ ] Add peer exchange to bounded bootstrap discovery (configured seeds and DNS seeds exist) and keep verifying identities learned through discovery.
- [ ] Audit every path that turns a remote lookup error into an empty result; distinguish local-only operation, partial connectivity and confirmed absence everywhere, not only in disconnected resolution.
- [ ] Exercise replica placement, maintenance, eviction, expiry, negative caching and conflicting sequences under node restart and churn.
- [ ] Verify capability discovery ranking, freshness, completeness and result limits, and document them.
- [ ] Add acknowledged replica storage and retry accounting, or state in the documentation that remote durability is not promised (`is_durable()` is false today).
- [ ] Multi-node acceptance: publish, resolve from a node with no local copy, restart the publisher, renew with the same identity, and stay available through a replica failure. The last step is not yet covered.

### Resources and tools

- [ ] Write the resource request/response contract: name, representation, content type, validator, size, streaming, cancellation, structured errors.
- [ ] Preserve which step failed (resolution, connection, authorization, transfer, validation) in errors surfaced through the SDK, CLI and gateway.
- [ ] Add cancellation, deadlines and caller authorization to `ToolCall`/`ToolResult` handling; today the tools are serial, read-only, and available to any connected client.
- [ ] Support bounded streaming and backpressure for large resources and encoded payloads (current ceiling: 8 MiB per representation, 64 KiB chunks).
- [ ] Connect typed links to traversal and fetching with budgets, deduplication, and visible skipped or failed resources.
- [ ] Extend automatic SDK endpoint selection beyond TCP, and let HTTP resource reads resolve beyond the configured origin, without hiding the provenance of unattested `host:` addresses.

### Models and coordination

- [ ] Consolidate the HTTP model backend with the gRPC model abstraction and the recursive dispatchers, or document why they stay separate.
- [ ] Verify upstream cancellation, bounded buffering, error propagation, timeouts and backpressure through native frames, SSE and gRPC. Only the SSE path has acceptance coverage.
- [ ] Apply the IronGate routing contract to every supported remote model path; test required-gateway refusal and local-model exceptions against mock servers.
- [ ] Test codec negotiation, shape and dtype validation, stateless defaults, and isolation for any session-specific stateful codec.
- [ ] Decide whether arbitrary task execution is in scope. If yes, design the executor, its sandbox and its authorization; if no, record the read-only boundary as final. Add task eviction either way (256 tasks and 256 knowledge keys per worker, no eviction).
- [ ] Decide whether model-issued tool calls are executed automatically; implement or document.
- [ ] Document the quality and context limits of recursive retrieval and neural representations on representative tasks; remove claims beyond the evidence.

**Done when:** a local model endpoint and an optional IronGate path both complete the workflow from a seed address alone, and cancelling a consumer terminates the corresponding upstream work on every supported protocol.

## Stage 4. Security and tenancy

- [ ] Add authorization for session ownership and privileged operations, with scopes for reading, execution, publication and administration. The native server currently gives every connected client operator-level access. Keep a documented single-user mode.
- [ ] Add a native reconnect/resume API consistent with that ownership model.
- [ ] Specify secure connection modes for TCP, WebSocket and QUIC: peer identity checks, downgrade refusal, and the difference between encryption and endpoint authentication. Bring optional QUIC to runtime and TLS-policy parity or mark it experimental; it has only been compiled.
- [ ] Define and enforce outbound fetch policy on the bridge and mesh paths; native and WASM navigation are done.
- [ ] Apply and test request, frame, decompression, lookup, connection and execution limits on the remaining paths (optional mesh transports, auxiliary cluster and diagnostics listeners), in an isolated harness with malformed and excessive input. Bound parser memory, which the fetched-byte cap does not.
- [ ] Verify identity-key creation, permissions (including Windows ACLs), redaction, backup, rotation and recovery. Define how rotation affects names whose authority is the old key.
- [ ] Verify that logs, traces, metrics, error bodies and shared codec or memory state never disclose another session's inputs.
- [ ] Keep research RLWE and Chameleon behaviour explicitly separate from standard AEAD, ML-KEM and TLS guarantees in code comments, configuration and documentation.
- [ ] Complete the secret review beyond the bounded format scan: full Git history and release inputs, findings kept local and redacted.
- [ ] Refresh `THREAT_MODEL.md` and `SECURITY_AUDIT.md` to verified behaviour. The audit is still titled "v1.2.1"; correct absolute claims about memory erasure and do not treat the `fips` feature flag as certification.
- [ ] Resolve independent review findings and add a regression for each.

**Done when:** no unresolved critical or high finding remains in the supported scope, secure configurations are tested, and the reviewer's evidence is on file.

## Stage 5. Operability

- [ ] Verify storage durability, migrations, backup and restore for names, sessions and knowledge against each component's persistence contract, including encrypted backups and power-loss behaviour.
- [ ] Expose namespace connectivity, lookup failures, replication outcomes, renewal failures, stream pressure, model failures and resource consumption through metrics and readiness.
- [ ] Make health and readiness reflect bridge, model and storage dependency state without requiring every optional dependency or seed.
- [ ] Preserve trace correlation across tools, mesh messages and model calls; keep payloads out of telemetry by default.
- [ ] Run sustained load and recovery tests. Measure the baseline first, then set limits for latency, resource growth, queue sizes and recovery time.
- [ ] Write the operator runbook and availability requirements for a public seed service. Operating one is outside repository completion.

**Done when:** an operator can install, configure, start, monitor, back up, restart and recover a small mesh from the documented commands on each supported platform.

## Stage 6. Interoperability and optional components

Do only what the scope decision under *External items* requires; label everything else experimental with a named acceptance criterion.

- [ ] Test Rust SDK, HTTP/SSE, gRPC and MCP clients against real local servers, including errors and cancellation. Test the MCP adapter in at least one third-party MCP host.
- [ ] Add schema and wire compatibility fixtures, explicit version negotiation, and useful rejection of incompatible peers. Required before the `bincode` 1.x migration.
- [ ] Build and smoke-test Python, JavaScript/WASM, Go and C bindings where supported; document installation and coverage. Python and JavaScript are outside the workspace and currently unverified.
- [ ] Verify C ABI ownership, buffer lifetimes, error handling and binding parity for advertised APIs.
- [ ] Verify embedded and `no_std` builds on a real target, not only on the host.
- [ ] Audit the Kubernetes crate against the removed deployment assets; validate the operator workflow or deprecate it.
- [ ] For GPU, RDMA, GPU-direct and io_uring, separate simulation from hardware execution. Unavailable backends fail explicitly or use a documented fallback; a no-op never counts as acceleration.
- [ ] Review the stub and simulation markers that remain in `spine-transport` (30), `spine-agentic` (24) and `spine-agent` (21) and confirm each is either real, test-only, or returns an explicit unsupported error.
- [ ] Promote the five ignored documentation examples to executable examples or delete them.

**Done when:** every distributed package builds and passes a smoke test on its advertised platform, or is excluded from the supported scope in writing.

## Stage 7. Evidence and documentation

- [ ] Extend the acceptance harness (`src/spine-core/tests/agent_web.rs`, seven process scenarios) to cover node loss, delayed replies, interrupted transfer, stale records, authorization failures, malformed input and consumer cancellation, and run it in CI on all three platforms.
- [ ] Run the supported platform and feature matrix, including MSRV with the GUI, optional features and release builds; record results and exceptions.
- [ ] Review unsafe kernel and FFI paths with Miri or sanitizers where applicable. Distinguish formal models that were written (`formal/tamarin`, `formal/tla`) from models executed and properties proved.
- [ ] Re-run representative codec and real-network benchmarks with comparable payloads, security modes, compression and concurrency; record hardware, toolchain, method and raw results. Report token transport separately from model generation speed, and distributions separately from peaks.
- [ ] Add performance regression checks with thresholds from measured variance; move unverifiable historical figures to `LEGACY.md`.
- [ ] Reconcile source behaviour with `README.md`, `ARCHITECTURE.md`, `ROADMAP.md`, the security documents, the SDK cookbook, the mdBook under `docs/`, and `paper/paper.typ`. Known stale items: the README "Test Summary (1,418 tests)" section and 2.0.1 version badge; ROADMAP phases marked complete whose features now return unsupported; the audit title.
- [ ] Replace stale version examples, crate counts, completion badges and security conclusions with verified current values.
- [ ] Rewrite `HANDOFF.md` for the current state. It still describes Phase 38 and predates all of the October work.

**Done when:** another developer reproduces the acceptance harness and benchmarks from the documented setup, and every public claim traces to recorded evidence.

## Stage 8. Release

- [ ] Choose the version. The October work changes wire behaviour (unsupported-version rejection, frame limits), configuration (strict validation, internal origins must be listed, redirects refused) and persistence (session manifest, SQLite task and name stores). That argues for 2.1.0 at minimum and 3.0.0 if any change breaks existing peers; it cannot be 2.0.2. Keep `spine-embedded` on its own 0.2.x line.
- [ ] Rename the candidate archive and update `package_candidate.py` to the chosen version.
- [ ] Reconcile workspace versions, internal dependency pins, changelog (move *Unreleased* under the version), compatibility notes and package metadata.
- [ ] Test package resolution without path dependencies in the order derived by `scripts/publish.sh`.
- [ ] Bring the release workflow in `.github/workflows/ci.yml` up to the local gate: checksums, configuration examples, license, lockfile, dependency inventory and provenance in every archive, built with `--locked`.
- [ ] Build Linux, macOS and Windows archives and run `scripts/smoke_candidate.py` against each on its own platform.
- [ ] Exercise upgrade from 2.0.1/2.0.2: persistence migration, incompatible-peer rejection, the documented recovery path, and where rollback is unsafe.
- [ ] Confirm all verification evidence applies to the exact commit being tagged.
- [ ] After explicit authorization: check auth with `cargo owner --list spine-protocol`, publish, verify installation from the registry and the release page, and update the handoff with actual outcomes.

**Done when:** published packages and archives reproduce the supported workflow and the evidence names the shipped commit.

## Final completion checklist

- [x] Independent agents discover and use resources from a seed address without preconfigured provider endpoints (Windows, local).
- [ ] Names, identity, renewal, replicas and required persistent state survive the documented restart and failure scenarios.
- [ ] Real tools and models stream correctly, support cancellation, enforce authorization, and expose useful errors.
- [ ] Supported transports have verified trust behaviour and bounded resource use; external review findings are closed.
- [ ] Simulations, research cryptography and unavailable hardware are excluded from supported functionality and performance claims.
- [ ] Operators can install and recover the supported deployment through documented native workflows.
- [ ] All supported packages, platforms and bridges pass acceptance on the release commit.
- [ ] Security, dependency, license, privacy and performance evidence is current and reproducible.
- [ ] Documentation and release metadata agree with verified behaviour.
- [ ] Release candidates install and run; publication, if authorized, is verified afterwards.

## Already demonstrated on 2026-10-02

These were checked in the previous plan with evidence in the status report. They hold for Windows x64 on Rust 1.98.1 and must be re-confirmed in Stage 1; the dependency item among them has since regressed and is reopened in Stage 2.

- **Baseline:** environment recorded; workspace built and tested; `verify.sh` detects failures by exit code; ignored tests explained; component inventory; supported platforms and MSRV recorded.
- **Namespace:** publisher records and sequence state persist and restore; persistence policy for replicas, caches and contacts; owner-only renewal; publication outcome reporting; bounded batch resolution; CLI for publish, resolve, renew, identity, peers and health; DNS seed bootstrap.
- **Resources and tools:** origin interface for named and content-addressed data; SDK resolve-connect-fetch-verify; safe fallback without replaying side effects; versions, schemas and permissions carried from discovery to invocation.
- **Models and coordination:** configurable model source with echo as explicit demo; OpenAI-compatible subset specified; simulated neural transmit returns unsupported; two processes complete dependent tasks over authenticated connections; cancellation, duplicates, ownership and recovery defined for read-only tasks.
- **Operations:** validated configuration examples; graceful shutdown, draining and idle cleanup; bounded session retention with atomic snapshots; admission readiness and outbound diagnostics; native start, stop, upgrade and recovery instructions.
- **Security:** shared native/WASM outbound navigation policy; explicit TCP/WSS mTLS and CRL policy; bounded codecs and mesh admission.
- **Other:** GUI and extension scope labelled; experimental work documented with acceptance criteria; clean-install quickstart with demo and configured model; local evidence report and release notes drafted.

## Verification log

**2026-11-19, Rust 1.100.0, Windows x64, uncommitted tree.** `scripts/verify.sh 1478` exited 1.

| Stage | Result |
| --- | --- |
| `cargo test --workspace --no-fail-fast` | Passed: 1,478 tests, 70 suites, 0 failed, 5 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | **Failed.** `clippy::needless_range_loop` at `src/spine-crypto/src/lib.rs:939`, a lint Rust 1.100 applies and 1.98.1 did not. Clippy stopped at `spine-crypto`, so crates that depend on it were not linted; more findings may follow once this one is fixed. |
| `cargo fmt --all -- --check` | Passed |

The run shared the machine with several unrelated builds, so the passing test count is not weakened by that, but timings are not meaningful. An earlier attempt the same day was interrupted mid-build and is disregarded. The MSRV check, release build, packaging and clean-install checks were not run.

## References

- [Status and evidence, 2026-10-02](docs/COMPLETION_STATUS.md)
- [Agent-web quickstart](docs/AGENT_WEB_QUICKSTART.md) and [native operations](docs/NATIVE_OPERATIONS.md)
- [Handoff through Phase 48 and the 2.0.1/2.0.2 releases](HANDOFF.md)
- [Independent review brief](HANDSHAKE-REVIEW.md) and [review packet](docs/CRYPTO_REVIEW_PACKET.md)
- [Dependency exception register](docs/DEPENDENCY_EXCEPTIONS.md)
- [Security policy](SECURITY.md), [audit](SECURITY_AUDIT.md), [threat model](THREAT_MODEL.md)
- [Verification script](scripts/verify.sh) and [local release gate](scripts/release_gate.py)
- [Acceptance harness](src/spine-core/tests/agent_web.rs)
