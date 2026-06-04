# SPINE v1.2.1 Security Audit

| Field          | Value                                          |
|----------------|------------------------------------------------|
| Audit date     | 2026-06-04 (refreshed at v1.3.0)               |
| Target version | v1.3.0                                         |
| Frameworks     | CVE / RustSec, NIST FIPS 140-3 + SP 800-171, MITRE ATT&CK, CMMC 2.0 |
| Auditor        | Internal review, pre-public-release sweep      |
| Scope          | Entire `master` branch, all 28 workspace crates |

v1.3.0 closes every BLOCKER and HIGH/MEDIUM residual from the v1.2.1
audit. See `§ 6` for the closed-vs-open table.

This document captures the audit that gated the v1.2.1 release.
Findings are graded **BLOCKER / HIGH / MEDIUM / LOW / INFO**. Every
BLOCKER was fixed before tagging; HIGH and below are tracked in
`CHANGELOG.md` and `SECURITY.md § Known Hardening Boundaries`.

---

## 1. CVE / RustSec dependency pass

Tool: `cargo audit` against the RustSec advisory database (1,105
advisories at the time of audit).

### Findings

| ID                | Severity | Status     | Notes                                  |
|-------------------|----------|------------|----------------------------------------|
| RUSTSEC-2024-0437 | HIGH     | **CLEARED** in v1.0.1 via `prometheus` 0.13→0.14 (protobuf 2.x→3.x) |
| 15× wasmtime CVEs | HIGH     | **CLEARED** in v1.0.1 via `wasmtime` 27→36 |
| RUSTSEC-2025-0012 (`backoff`) | INFO | accepted — unmaintained transitive under `kube-runtime`; no exploit |
| RUSTSEC-2025-0141 (`bincode 1.x`) | INFO | accepted — `bincode 2.x` migration tracked separately |
| RUSTSEC-2024-0388 (`derivative`) | INFO | accepted — transitive under `zbus`/`kube-runtime` |
| RUSTSEC-2025-0057 (`fxhash`) | INFO | accepted — transitive under `selectors` + `wasmtime` |
| RUSTSEC-2024-0384 (`instant`) | INFO | accepted — transitive under `fastrand 1.x` |
| RUSTSEC-2025-0119 (`number_prefix`) | INFO | accepted — transitive under `indicatif` |
| RUSTSEC-2024-0436 (`paste`) | INFO | accepted — transitive under `metal` / `wgpu-hal` |
| RUSTSEC-2024-0370 (`proc-macro-error`) | INFO | accepted — transitive under `utoipa-gen` / `tabled_derive` |
| RUSTSEC-2025-0134 (`rustls-pemfile`) | INFO | accepted — transitive under `kube-client` and direct `rustls-pemfile = "2"` |

**Net status**: `cargo audit` returns **0 vulnerabilities**. All
remaining entries are `unmaintained` warnings, not exploitable
vulnerabilities. Each has no patched line available because the
upstream crate has been EOL'd without a replacement that the broader
Rust ecosystem agrees on. We track them and will bump when patched
upstream lines exist.

---

## 2. NIST FIPS 140-3 / SP 800-171 cryptographic surface

For each primitive: which FIPS standard names it (if any), and whether
the Rust crate we use is a **FIPS-validated cryptographic module**
(very few are — validation is a per-binary certification process).

| Primitive          | FIPS spec     | Approved? | Rust crate           | Module validated? |
|--------------------|---------------|-----------|----------------------|-------------------|
| AES-256-GCM        | FIPS 197 + SP 800-38D | ✅ Approved | `aes-gcm = "0.10"`   | ❌ Not validated  |
| SHA-256 / SHA-512  | FIPS 180-4    | ✅ Approved | `sha2 = "0.10"`      | ❌ Not validated  |
| HMAC-SHA-256       | FIPS 198-1    | ✅ Approved | `hmac = "0.12"`      | ❌ Not validated  |
| HKDF-SHA-256       | SP 800-56C    | ✅ Approved | `hkdf = "0.12"`      | ❌ Not validated  |
| Ed25519            | FIPS 186-5 (Feb 2023) | ✅ Approved | `ed25519-dalek = "2"` | ❌ Not validated  |
| ECDSA P-256        | FIPS 186-5    | ✅ Approved | `rustls-webpki`      | ❌ Not validated (ring is, see below) |
| ML-KEM-768 / 1024  | FIPS 203 (Aug 2024) | ✅ Approved | `ml-kem` (RustCrypto) | ❌ Not validated  |
| TLS 1.3            | NIST SP 800-52 Rev. 2 | ✅ Approved | `rustls = "0.23"`    | ❌ rustls itself is not FIPS-validated; the underlying `ring` backend has separate FIPS submissions |
| **X3DH**           | —             | ❌ Not a NIST spec (Signal protocol) | custom  | n/a |
| **Custom RLWE lattice in `spine-crypto`** | — | ❌ Research-grade only | in-tree | n/a |
| **Titans speculative decoding** | — | ❌ Not crypto at all (ML technique) | in-tree | n/a |
| PRNG — message keys, nonces | SP 800-90A (HMAC_DRBG / CTR_DRBG / Hash_DRBG) | We use `rand::rngs::StdRng` (ChaCha12 — **not** SP 800-90A) | `rand = "0.8"` | ❌ Not validated |

### Posture

- **Algorithm choices are FIPS-approved by spec.** Anyone reading the
  list of crypto names will recognise them as approved primitives.
- **No crate in our tree is a FIPS 140-3 validated cryptographic
  module.** Federal deployments that require FIPS 140-3 must:
  1. Swap `ring` for `aws-lc-rs` in FIPS mode (rustls supports both).
  2. Replace `rand::StdRng` with a SP 800-90A-validated DRBG.
  3. Disable / remove the in-tree lattice + X3DH paths; they are
     research-grade and not approved for protection of CUI.

### Recommendation for federal deployment

Build the `spine-gateway` binary with `aws-lc-rs` as the rustls
provider and set `AWS_LC_FIPS=1` at compile time. Document the
unvalidated paths as "off in FIPS mode" in your SSP (System Security
Plan). This is a deployment-mode decision, not a code change.

---

## 3. MITRE ATT&CK v15 — attack-surface map

Tactics × techniques most relevant to a gateway-fronted agent stack.

| Technique                                            | Surface in SPINE                                            | Status                                                                 |
|------------------------------------------------------|-------------------------------------------------------------|------------------------------------------------------------------------|
| **T1190** Exploit Public-Facing App                  | All 17 gateway routes                                       | **MITIGATED (this release)**: optional bearer auth via `SPINE_GATEWAY_BEARER_TOKEN`. **HIGH residual**: auth is opt-in, not on by default — deployers must enable it. |
| **T1499.002** Endpoint DoS — Service Exhaustion Flood | POST bodies on `/v1/embeddings`, `/v1/chat/completions`, `/api/parse`, `/api/compile`, `/api/sessions/{id}/execute` | **MITIGATED (this release)**: `DefaultBodyLimit::max(8 MiB)` on the router. |
| **T1499.004** Endpoint DoS — Application Exhaustion  | HLS programs running inside the WASM runtime                | **MITIGATED (this release)**: wasmtime `consume_fuel(true)` + 1 B fuel/exec default. Tight `loop {}` inside HLS traps. |
| **T1059.011** Command & Scripting — HLS via `/execute` | Untrusted HLS reaches the WASM sandbox via the gateway      | **MITIGATED (pre-existing)**: no WASI / network / filesystem host functions; only 21 allowlisted DOM/state/event imports. |
| **T1565.002** Data Manipulation — In-Flight (cross-request leakage) | Stateful `TitansLatentCodec` in process-wide registry shipped in v1.2.0 | **MITIGATED in v1.2.1**: stateless codec by default + `reset_state` upstream + regression guard `titans_stateless_codec_does_not_leak_across_calls`. |
| **T1518.001** Software Discovery — Capability listing | `/v1/agentic/capabilities` and `/v1/agentic/codecs`         | **By design**: these endpoints exist to advertise. Risk is reconnaissance only, not exploit. Documented in `SECURITY.md`. |
| **T1119** Automated Collection                       | `EmbeddingRequest`s flowing through the gateway             | **No mitigation needed at protocol layer**: customers' privacy policies apply once requests leave SPINE. SPINE itself does not retain inputs. |
| **T1040** Network Sniffing                           | TLS-encrypted SPINE protocol on the wire                    | **MITIGATED**: AES-256-GCM + rustls 0.23. Plain HTTP is opt-in for the gateway; HTTPS via reverse proxy is the documented production pattern. |
| **T1098** Account Manipulation — Agent identity      | Ed25519 keypairs in `spine-agentic`                         | **PARTIAL**: keys are signed/verified correctly, but private-key memory is held in `Vec<u8>` without `zeroize`. **MEDIUM residual** — core-dump / swap risk. Tracked. |
| **T1496** Resource Hijacking — CPU                   | Long-running WASM programs                                  | **MITIGATED (this release)**: same fuel budget as T1499.004. |
| **T1036** Masquerading — protocol fingerprint        | Wire format                                                 | **MITIGATED (pre-existing)**: Chameleon Protocol moving-target keys + latent basis morph per message. |

### Residual ATT&CK risk

1. **HIGH — T1190 with auth off**: the gateway ships with bearer auth
   **disabled by default** (env var unset). The startup log emits a
   WARN. Production deployments **must** set
   `SPINE_GATEWAY_BEARER_TOKEN` or run behind an authenticating proxy.
2. **MEDIUM — T1098 via key memory disclosure**: private-key bytes
   not zeroized on drop. Tracked for a future minor release; a
   `zeroize`-shaped Drop impl can be added without breaking the API.

---

## 4. CMMC 2.0 — practice-level gap analysis

### Level 1 (Foundational, 17 practices, FCI safeguarding)

| Practice         | Description                          | Status                                  |
|------------------|--------------------------------------|-----------------------------------------|
| AC.L1-3.1.1      | Limit system access to authorized users | **MET (opt-in)** via `SPINE_GATEWAY_BEARER_TOKEN`. **HIGH residual**: must be enabled at deploy time. |
| AC.L1-3.1.2      | Limit transactions to authorized functions | **MET** — bearer middleware gates every authenticated path. |
| AC.L1-3.1.20     | External information system control  | Documented in `SECURITY.md § Threat Model`. |
| AC.L1-3.1.22     | Public information control           | `SECURITY.md` calls out what is and isn't publicly exposed. |
| IA.L1-3.5.1      | Identify users / processes           | Ed25519 DID in `spine-agentic` + bearer at gateway. |
| IA.L1-3.5.2      | Authenticate identity                | Same as above. Constant-time compare via `subtle::ConstantTimeEq`. |
| MP.L1-3.8.3      | Sanitize media before reuse          | N/A for in-process. Documented for keys (see § FIPS). |
| PE.L1            | Physical                             | N/A (software-only). |
| SC.L1-3.13.1     | Boundary monitoring                  | **MET**: `DefaultBodyLimit` + body-shape validation enforce the protocol boundary. |
| SC.L1-3.13.5     | Public-access systems                | Swagger UI and `/health` deliberately open. |
| SI.L1-3.14.1     | Identify, report, correct flaws      | This document is the report. `SECURITY.md` is the channel. |
| SI.L1-3.14.2     | Malicious-code protection            | HLS sandbox (no WASI, no network, no filesystem) + fuel cap. |
| SI.L1-3.14.4     | Update protections                   | `cargo audit` in CI (recommended); upgrade path via SemVer minor bumps. |
| SI.L1-3.14.5     | Periodic scan                        | `cargo audit` + this document. |

**L1 verdict**: Met when deployed correctly (bearer enabled, behind
TLS-terminating proxy or with direct `rustls` enabled). Documentation
calls out the required deployer actions.

### Level 2 (selected practices, NIST SP 800-171 Rev 2)

Focused on the families where SPINE has a concrete posture.

| Practice          | Status                                                    |
|-------------------|-----------------------------------------------------------|
| AC.L2-3.1.3       | **MET** — body-size limits + WASM fuel constrain control of CUI flow. |
| AU.L2-3.3.1       | **PARTIAL** — `tracing` spans exist (`skip_all` on agentic handlers); no out-of-the-box audit-log retention. Deploy `tracing-subscriber` with a persistent sink. |
| AU.L2-3.3.2       | **PARTIAL** — actions are traced; user identification ties only to bearer token (no per-user ID until SSO is wired). |
| CM.L2-3.4.1/2     | **MET** — config defaults captured in `spine-core::config::TlsConfig`, CT policy disabled by default, codec registry stateless by default. |
| CM.L2-3.4.6       | **MET** — workspace + `Cargo.lock` committed; reproducible builds. |
| IA.L2-3.5.3       | **DEPLOYER ACTION** — MFA is an upstream-proxy concern; SPINE supplies a single bearer secret. |
| IA.L2-3.5.7/8/9   | N/A — no password handling. |
| SC.L2-3.13.8      | **MET** — TLS in transit via rustls (0.23). |
| SC.L2-3.13.10     | **PARTIAL** — Ed25519 keys are signed correctly but not zeroized in memory. |
| SC.L2-3.13.11     | **PARTIAL FIPS** — algorithms approved by spec, modules not validated. See § 2. |
| SC.L2-3.13.16     | **MET** — `EncodedFrame.metadata.source_hash` lets the receiver verify content integrity end-to-end. |
| SI.L2-3.14.6/7    | **MET** — anomaly signal via Titans surprise scores in `spine-neural`. |

**L2 verdict**: Substantial coverage with two deployer-actionable
gaps (audit-log sink, key zeroization) and one architectural choice
(FIPS-validated modules). All documented.

### Level 3

Out of scope for a public OSS release. CMMC L3 (APT-grade) requires
deployment-specific controls (SIEM integration, threat hunting, etc.)
that are not protocol concerns.

---

## 5. Fixes applied in v1.2.1

The audit identified four BLOCKERS; all were closed in this release:

1. **Cross-request data leak in `/v1/embeddings`** (T1565.002):
   `TitansLatentCodec` now resets its `NeuralLatentEncoder` state via
   the new `NeuralLatentEncoder::reset_state(seed)` upstream call
   before every `encode()`. Regression guards:
   `titans_stateless_codec_does_not_leak_across_calls`,
   `titans_stateful_codec_is_context_aware`.
2. **Unbounded POST bodies** (T1499.002): `DefaultBodyLimit::max(8
   MiB)` on the gateway router.
3. **No CPU bound on HLS execution** (T1499.004 / T1496): wasmtime
   `consume_fuel(true)` + 1 B fuel/exec default. Regression guards:
   `test_wasm_fuel_metering_is_active`,
   `test_default_fuel_budget_completes_normal_programs`.
4. **OpenAPI-declared bearer auth had no middleware** (T1190): added
   `auth::require_bearer` with constant-time comparison via `subtle`,
   gated on `SPINE_GATEWAY_BEARER_TOKEN`. Tests cover constant-time
   match, length-mismatch rejection, env-var on/off, and path
   allowlist.

## 6. Known residuals — status after v1.3.0

| Item                                   | Severity | Status                                                                 |
|----------------------------------------|----------|------------------------------------------------------------------------|
| Bearer auth opt-in by default          | HIGH     | **CLOSED in v1.3.0** — `AuthMode::resolve` makes the gateway refuse to start without an explicit choice; the previous silent-open mode is no longer reachable. |
| Private-key memory not zeroized        | MEDIUM   | **CLOSED in v1.3.0** — `zeroize` derives or manual `Drop` on `RingElement`, `QuantumKeyPair`, `MlKemKeyPair`, `QuantumKeyEvolution`, `ProtocolMorphology`, `Ed25519Keypair`. Compile-time + runtime regression guards. |
| FIPS-validated crypto module           | DEPLOYER | **WIRED in v1.3.0** — `cargo build -p spine-gateway --features fips` swaps rustls's CryptoProvider to `aws-lc-rs`. End-to-end FIPS 140-3 validation still requires the deployer to rebuild `aws-lc-rs` with `AWS_LC_FIPS=1`. |
| `bincode 1.x` deprecation              | LOW      | Tracked. Migrate to `bincode 2.x` when ecosystem catches up. |
| `rustls-pemfile` unmaintained          | LOW      | Tracked. Move to `rustls-pki-types::CertificateDer` parsing. |

## 7. Verification

`cargo test --workspace --no-fail-fast` → **1,072 passed / 0 failed
/ 5 ignored** at the audit-completion commit. `cargo audit` → 0
vulnerabilities, 10 unmaintained warnings (all accepted in the table
above). Build clean on Rust stable.
