# Security Policy

## Supported Versions

| Version | Status        | Receives security fixes |
|---------|---------------|-------------------------|
| 1.3.x   | Current       | Yes                     |
| 1.2.x   | Previous      | Yes (critical only)     |
| 1.1.x   | EOL           | No (please upgrade)     |
| 1.0.x   | EOL           | No (please upgrade)     |
| < 1.0   | Pre-release   | No                      |

## Reporting a Vulnerability

If you believe you've found a security vulnerability in SPINE, please **do
not open a public GitHub issue**. Instead, report it privately via one of
the following channels:

1. **GitHub Security Advisories** (preferred):
   <https://github.com/nervosys/SPINE/security/advisories/new>
2. **Email**: `adam.michael.erickson@gmail.com` with the subject prefix
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
| Message payloads on the wire     | Confidentiality                | AES-256-GCM + rustls 0.23 (or QUIC)        |
| Long-lived agent identity        | Authenticity, non-repudiation  | Ed25519 signing in `spine-agentic`         |
| Cross-tenant embeddings          | Isolation                      | Stateless `TitansLatentCodec` (default)    |
| Certificate Transparency proofs  | Integrity                      | RFC 6962 + Google v3 JSON log list         |
| Protocol fingerprint             | Resistance to passive analysis | Chameleon Protocol (moving-target)         |
| Tool calls and their results     | Correlation                    | Per-call `id` + W3C `traceparent`          |

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
- **Default config**: `CtPolicy::default()` ships with **no trusted
  CT logs** preloaded. Production deployments must call
  `add_logs_from_json_v3()` with the current Google v3 list.
- **Stateful Titans codec**: `TitansLatentCodec::stateful(...)` is
  context-aware by design — it leaks request history into subsequent
  encodings. Use only for a single-tenant session you control end to
  end. Never register in a shared `CodecRegistry`.
- **Echo `/v1/chat/completions`**: the gateway's default chat handler
  echoes the user's last message. Replace with a real LLM source before
  exposing to untrusted clients.

## Gateway authentication (secure by default as of v1.3.0)

The `spine-gateway` binary will **refuse to start** unless the
deployer makes an explicit authentication choice via env vars:

| Variable                          | Effect                                                          |
|-----------------------------------|-----------------------------------------------------------------|
| `SPINE_GATEWAY_BEARER_TOKEN=<secret>` | Auth ON — every route (except `/health`, `/ready`, `/swagger-ui`, `/api-docs`) requires `Authorization: Bearer <secret>`. Constant-time compare via `subtle`. |
| `SPINE_GATEWAY_ALLOW_UNAUTH=1`    | Auth OFF — explicit opt-out. Use only for local dev or behind an authenticating proxy. Startup logs a `WARN`. |
| neither set                       | Gateway exits with code 2 and prints both options to stderr.    |
| both set                          | Same — ambiguous intent is rejected.                            |

This closes the v1.2.1 residual where bearer auth was opt-in by
default. CMMC AC.L1-3.1.1, MITRE T1190.

## FIPS 140-3 build

The `spine-gateway` crate exposes an opt-in `fips` cargo feature for
federal deployments. Enabling it:

```bash
cargo build -p spine-gateway --release --features fips
```

1. Pulls in `aws-lc-rs` as the rustls `CryptoProvider`.
2. At startup, installs `aws-lc-rs` as the process-wide rustls
   default, so every TLS handshake uses AWS-LC primitives instead of
   `ring`.

For an end-to-end FIPS-validated module (required by CMMC L2 / IRAP /
FedRAMP), the deployer must **also** rebuild `aws-lc-rs` itself in
FIPS mode (`AWS_LC_FIPS=1` plus the upstream toolchain instructions).
The cargo feature alone wires SPINE's integration point; AWS-LC's
validated module is a deployer-toolchain decision.

## Cryptographic key memory hygiene (v1.3.0)

The following private-key-bearing structs now `Zeroize` on `Drop`
(NIST SP 800-171 § 3.13.10):

- `spine_crypto::RingElement` — RLWE coefficients
- `spine_crypto::QuantumKeyPair` — RLWE secret/public ring elements
- `spine_crypto::MlKemKeyPair` — FIPS 203 decapsulation key bytes
- `spine_crypto::QuantumKeyEvolution` — rolling key-hash history
- `spine_protocol::ProtocolMorphology` — session HMAC key (32 B)
- `spine_agentic::Ed25519Keypair` — wraps `ed25519-dalek::SigningKey`
  (which itself derives `ZeroizeOnDrop` upstream) plus the cached
  public-key bytes.

A dropped struct cannot leak its secret to a core-dump, swap-to-disk,
or hibernation-image attacker.

## Coordinated Disclosure

We will credit reporters in the security advisory by name or pseudonym
(your choice). Embargo windows are negotiated; the default is **public
disclosure 30 days after a fix ships**, sooner if mutual agreement.
