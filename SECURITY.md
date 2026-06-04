# Security Policy

## Supported Versions

| Version | Status        | Receives security fixes |
|---------|---------------|-------------------------|
| 1.2.x   | Current       | Yes                     |
| 1.1.x   | Previous      | Yes (critical only)     |
| 1.0.x   | Initial       | No (please upgrade)     |
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

## Coordinated Disclosure

We will credit reporters in the security advisory by name or pseudonym
(your choice). Embargo windows are negotiated; the default is **public
disclosure 30 days after a fix ships**, sooner if mutual agreement.
