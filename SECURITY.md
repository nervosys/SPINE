# Security Policy

> **Status, 2026-11-23 (source at `a4efd03`).** The threat-model table, hardening boundaries, gateway and key-memory sections below were corrected against the code. Any client that can connect to the native port has operator-level access to all 21 commands; the authority gate runs only in mode `audit`, for an `AUTH_REQUEST`, and enforces nothing (the command runs whatever the verdict). The mesh handshake has had no external review. See [`THREAT_MODEL.md`](THREAT_MODEL.md) and [`SECURITY_AUDIT.md`](SECURITY_AUDIT.md).

## Local implementation verification

The 2026-10-02 continuation patches Rustls and Wasmtime advisories, refuses missing certificates when native TCP TLS is enabled, redacts model-routing tokens and native request payloads, bounds resource transfer, and reports replica sends separately from acknowledged durability. Native and WASM HTTP navigation now share DNS-pinned public-address checks, exact internal-origin exceptions, redirect refusal, and response/concurrency/deadline limits; see [native operations](docs/NATIVE_OPERATIONS.md). See [the evidence report](docs/COMPLETION_STATUS.md). These checks do not establish an independent review of the custom handshake, multi-tenant isolation, cryptographic module certification, or verified hardware erasure. The quickstart uses trusted local clients; public multi-user deployment remains gated on authorization and policy across the remaining bridge/mesh paths.

The development task service additionally requires encrypted TCP, pins the recipient's key in the handshake before sending task data, checks pooled connection identity, and matches the signed caller key to the authenticated connection. Workers authorize explicit coordinator keys before accessing owner-scoped tasks or immutable knowledge. Unpinned bootstrap and other application transport modes are refused. Native clients of a coordinator share its identity; these controls do not provide multi-user origin tenancy. Application requests are never transparently replayed after an uncertain write. Read-only task recovery and duplicate semantics are documented in [native operations](docs/NATIVE_OPERATIONS.md).

## Supported Versions

| Version | Status        | Receives security fixes |
|---------|---------------|-------------------------|
| 2.0.x   | Current       | Yes (2.0.1 is the published release) |
| 1.9.x   | Previous      | Yes (critical only)     |
| < 1.9   | EOL           | No (please upgrade)     |

2.0.0 carries fixes for four cryptographic defects found during an internal
audit — see [`SECURITY_AUDIT.md`](SECURITY_AUDIT.md). Three of them (ephemeral
key generation, AES-GCM nonce reuse, and record-store admission) affect anyone
running a 1.x mesh. **Upgrading is a wire-breaking change:** 2.0.0 peers cannot
speak to 1.x peers over the Chameleon layer, so a deployment upgrades together
or not at all. (Historical: the Chameleon layer is no longer on the native wire; as of
2026-11-21 the native wire is spine-wire v1 and has no such layer.)

The native origin TLS configuration now applies to TCP and WSS, including explicitly required client certificates and revocation lists. Transport admission, complete-message deadlines, bounded decompression, and cancelled mesh-write retirement reduce resource growth; they do not establish a process-wide memory budget. The legacy cluster and diagnostics remain trusted-network interfaces. Native origin clients have operator access; neither mTLS nor the HTTP gateway token defines per-user roles. See [the connection policy](docs/NATIVE_OPERATIONS.md#connection-and-tls-policy) for concrete limits and unsupported modes.

Mesh authentication (2026-11-21): the sender of a mesh envelope is established by a signature check against a key this node recorded for it, and by the connection's authenticated key where there is one, for name-resolution responses, hellos, acks and application calls; `envelope.from` alone is never trusted for those. `AgentId` is not derived from the key, and no variant other than name resolution and application RPC is handled at all. Pinning (2026-11-21): every authenticated dial expects the key its introduction named (a node id is the peer's Ed25519 key) and a mismatch fails the dial; a bare seed address is trusted on first contact only, then pinned in memory for the life of the process, and `namespace.seed_keys` configures a seed's key from the first dial. QUIC: the client verifies the server certificate against an explicit trusted set, the first-stream mesh handshake's key is kept for every stream of the connection, and `Application` RPC works over it; the handshake is not bound to the TLS channel (`spine-session` has no input for channel context), so a party able to terminate TLS with a certificate the client trusts can relay a genuine peer's handshake. The full table is in [the `spine-mesh` README](src/spine-mesh/README.md#trust-model).

## Reporting a Vulnerability

If you believe you've found a security vulnerability in SPINE, please **do
not open a public GitHub issue**. Instead, report it privately via one of
the following channels:

1. **GitHub Security Advisories** (preferred):
   <https://github.com/nervosys/SPINE/security/advisories/new>
2. **Email**: `opensource@nervosys.ai` with the subject prefix
   `[SPINE-SECURITY]`.

When reporting, include:

- A short description of the issue and its impact.
- Steps to reproduce (a minimal Rust snippet or HTTP request is ideal).
- The version(s) of SPINE affected (`git rev-parse HEAD` or a tag).
- Your assessment of severity (low / medium / high / critical).
- Whether you'd like public credit in the advisory.

We aim to acknowledge the report within **72 hours** and to ship a fix
or mitigation within **30 days** for high/critical issues. For dual-use
findings (e.g. a side channel in the Chameleon protocol or a fingerprint
issue in the RDMA scaffold) we will coordinate disclosure with you.

## Threat Model

SPINE is designed for **agent-to-agent** traffic that may carry private
content (prompts, embeddings, tool arguments, cached model state). The
shipped components target these properties:

| Asset                            | Property protected             | Mechanism                                  |
|----------------------------------|--------------------------------|--------------------------------------------|
| Message payloads on the wire     | Confidentiality                | Native wire: rustls 0.23 TLS, off by default; the native QUIC listener (non-default `quic` feature) refuses to start with TLS configured. Mesh: AES-256-GCM over the handshake, always. The native frame itself does no sealing |
| Long-lived agent identity        | Authenticity                   | Ed25519 in `spine-mesh` (`identity.rs`) and `spine-name` |
| Cross-tenant embeddings          | Isolation                      | Only in a gateway `research` build: stateless `TitansLatentCodec`. The default gateway answers `/v1/embeddings` with 503 |
| Certificate Transparency proofs  | Integrity                      | None. `spine-core/src/ct.rs` is compiled but unused (`#[allow(dead_code)]`); no connection is checked against it |
| Protocol fingerprint             | Resistance to passive analysis | None on the native wire. The Chameleon key (research, `research/spine-protocol-research`, unreviewed) is not a mitigation |
| Tool calls and their results     | Correlation                    | Frame request id (spine-wire v1) + W3C `traceparent` |

### What is **not** in scope

- **Confidentiality of metadata**: timing, message sizes, peer
  identities, and codec advertisements are intentionally visible to
  network observers. SPINE is not a metadata-private overlay.
- **GPU memory side channels**: `spine-gpu` shaders use the standard
  wgpu/Vulkan/DX12 path; we assume the GPU and its driver are trusted.
- **Hardware backends with HardwareUnavailable stubs**
  (`IbVerbsRdma`, `GpuDirectRdma`): the typed surface exists but no
  signed-release binary speaks to real InfiniBand hardware. If you wire
  these to production NICs you are responsible for the resulting
  privilege boundary.
- **Third-party LLM providers** plugged behind the gateway's
  `/v1/chat/completions` or `/v1/embeddings` endpoint: their privacy
  policy applies once a request leaves SPINE.

## Cryptographic Choices

- **TLS**: rustls 0.23 with the ring backend; webpki via rustls-webpki
  ≥ 0.103.13 (clears the 2026 CRL / name-constraint advisories).
- **AEAD**: AES-256-GCM via `aes-gcm = "0.10"`. Nonces are 96-bit
  random; never reuse a (key, nonce) pair.
- **Signing**: Ed25519 via `ed25519-dalek = "2"` with `rand_core`.
- **Hashing**: SHA-256 via `sha2 = "0.10"`. `source_hash` in
  `EncodedMetadata` is optional — turn it off if your input contents
  must not be guessable by hash collision.
- **Post-quantum**: lattice primitives in `spine-crypto` are research
  quality, **not** production. Do not deploy `SecurityLevel::PostQuantum`
  against a real adversary without an independent review.

## Known Hardening Boundaries

- **Audit warnings (unmaintained transitive crates)**: `cargo audit`
  reports 0 vulnerabilities but ~10 unmaintained-crate warnings for
  transitives under `kube`, `eframe`, `wgpu`, `utoipa`, and
  `scraper`. None has a known exploit at the time of release; we track
  upstream and will bump when patched lines become available.
- **Certificate Transparency**: `CtPolicy` exists in `spine-core/src/ct.rs`
  and nothing calls it; no connection is checked against CT logs.
- **Stateful Titans codec**: `spine_protocol_research::TitansLatentCodec::stateful(...)` is
  context-aware by design — it leaks request history into subsequent
  encodings. Use only for a single-tenant session you control end to
  end. Never register in a shared `CodecRegistry`.
- **Echo `/v1/chat/completions`**: the gateway does not select the echo
  handler by default; without `SPINE_MODEL_URL` or `IRONGATE_URL` it
  answers that no model is configured, and echo needs
  `SPINE_GATEWAY_DEMO=1` (`spine-gateway/src/model.rs`).

## Gateway authentication (an explicit choice is required)

The `spine-gateway` binary will **refuse to start** unless the
deployer makes an explicit authentication choice via env vars:

| Variable                          | Effect                                                          |
|-----------------------------------|-----------------------------------------------------------------|
| `SPINE_GATEWAY_BEARER_TOKEN=<secret>` | Auth ON — every route (except `/health`, `/ready`, `/swagger-ui`, `/api-docs`) requires `Authorization: Bearer <secret>`. Constant-time compare via `subtle`. |
| `SPINE_GATEWAY_ALLOW_UNAUTH=1`    | Auth OFF — explicit opt-out. Use only for local dev or behind an authenticating proxy. Startup logs a `WARN`. |
| neither set                       | Gateway exits with code 2 and prints both options to stderr.    |
| both set                          | Same — ambiguous intent is rejected.                            |

This closes the v1.2.1 residual where bearer auth was opt-in by
default. It is one shared secret with no per-user roles, and the gateway
reaches the daemon as a single client; it does not create tenancy.

## FIPS 140-3 build

The `spine-gateway` crate exposes an opt-in `fips` cargo feature for
federal deployments. Enabling it:

```bash
cargo build -p spine-gateway --release --features fips
```

1. Pulls in `aws-lc-rs` as the rustls `CryptoProvider`.
2. At startup, installs `aws-lc-rs` as the process-wide rustls
   default of the gateway process (`install_fips_provider`,
   `spine-gateway/src/main.rs`). The daemon (`spine-core`) has no such
   feature and selects the Ring provider explicitly.

For an end-to-end FIPS-validated module (required by CMMC L2 / IRAP /
FedRAMP), the deployer must **also** rebuild `aws-lc-rs` itself in
FIPS mode (`AWS_LC_FIPS=1` plus the upstream toolchain instructions).
The cargo feature alone wires SPINE's integration point; AWS-LC's
validated module is a deployer-toolchain decision.

## Cryptographic key memory hygiene

Corrected 2026-11-23. Handshake state and ephemeral secrets in
`spine-session` derive `Zeroize` and `ZeroizeOnDrop`, and
`ed25519_dalek::SigningKey` zeroizes on drop (`spine-mesh/src/identity.rs`).
The daemon's node key is stored in a plain file, created with mode 0600 on
Unix (on Windows it takes the access list of its directory); an existing file
open to other users draws a start-up warning. It is held as an
`ed25519_dalek::SigningKey`, which wipes itself on drop. The hex text read from
the file, the decoded and the freshly generated seed array, the hex text
written to a new file and the `to_bytes()` copy made for
`SigningIdentity::from_seed` are held in `zeroize::Zeroizing` and wiped on
drop. This is best effort: copies made by the operating system (page cache,
swap, core dumps), the allocator (a string buffer that grew) or the compiler
(registers, stack slots), and the seed copy `spine-mesh` makes inside its own
identity type, are out of reach, and the key file is plain text on disk.
The earlier list of `spine_crypto` and `spine_agentic` structs that
zeroize on drop concerns research crates that the daemon does not use,
and is not repeated. Zeroization does not protect against a reader of the
state directory.

## Coordinated Disclosure

We will credit reporters in the security advisory by name or pseudonym
(your choice). Embargo windows are negotiated; the default is **public
disclosure 30 days after a fix ships**, sooner if mutual agreement.
