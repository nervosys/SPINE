# Custom mesh handshake review packet

Corrected 2026-11-20 against the source at 3d58059; file paths updated after the handshake moved to `spine-session` and the mesh to `spine-mesh`. Line numbers are as of 3d58059 and may have shifted.

This prepares independent review of the development checkout. It is not an audit certificate. The candidate's source digest, toolchain, base commit and dirty state are in `provenance.json`; match that digest to the reviewed snapshot. Implementation evidence is in [the completion status](COMPLETION_STATUS.md). Reviewing the linked sources requires the matching repository checkout; the native archive carries binaries and documentation, not a full source tree.

## Sources and deployed path

The custom construction in [handshake.rs](../src/spine-session/src/handshake.rs) combines ephemeral ML-KEM-768, Ed25519 identity signatures, a SHA-256 transcript, HKDF-SHA256 direction labels, and AES-256-GCM frames with ordered counters authenticated as AEAD associated data. Ed25519 verification uses `verify_strict`; the handshake has no key confirmation, no rekeying, and sends both identities in clear (see [HANDSHAKE-REVIEW.md](../HANDSHAKE-REVIEW.md)). Ed25519 authentication is classical; ML-KEM alone does not establish post-quantum authentication, certification, or a reviewed hybrid construction.

Mesh transport integration is in [mesh_tcp.rs](../src/spine-mesh/src/mesh_tcp.rs) and [namespace.rs](../src/spine-core/src/namespace.rs). Native origin Rustls policy is separate, in [tls.rs](../src/spine-core/src/tls.rs). The native client wire (spine-wire v1, `docs/spec/00-wire.md`) contains no SPINE cryptography: its confidentiality and authentication come from TLS alone, and the Rustls policy above is the only protection to review there. The Chameleon key, latent-space encoding, protocol morphing, padding and pre-shared-key AES-GCM that earlier versions carried on that wire were removed on 2026-11-21. What remains of the Chameleon key and latent codec is experimental code in `research/spine-protocol-research`, which is not on any wire and is not in scope for this review; it does not substitute for authenticated transport protection.

Application RPC is defined in [mesh.rs](../src/spine-mesh/src/mesh.rs), correlated in [naming_mesh.rs](../src/spine-mesh/src/naming_mesh.rs), and authorized/persisted in [tasks.rs](../src/spine-core/src/tasks.rs). The recipient key and agent ID are signed inside the application payload. Outgoing TCP pins the handshake key before application delivery, including reused connections. Incoming signed sender keys must match the authenticated connection. Worker allowlists apply before task/knowledge access. Non-application (DHT) dials are not pinned in this checkout: the only writer of the per-peer pin map, `set_identity` ([mesh_tcp.rs](../src/spine-mesh/src/mesh_tcp.rs) line 788), has no caller outside tests. Application messages refuse plaintext, unpinned bootstrap, and experimental mesh WebSocket/QUIC delivery.

## Trust and lifecycle

Bootstrap addresses supply reachability, not operator trust anchors. Self-signed peer identity does not establish publisher reputation, honest routing, Sybil resistance, or global availability. Signed records establish key ownership and sequence; signed hashes establish retrieved byte integrity, without certifying content meaning or wider provenance.

Rotation changes DID authority and requires authenticated operator updates to pins and allowlists. This checkout does not transfer old authority, automate encrypted key escrow, or establish complete secret erasure. Native clients of a coordinator share its mesh identity. The legacy cluster and diagnostics retain trusted-network boundaries; origin mTLS does not implement per-user scopes across those interfaces.

[Native operations](NATIVE_OPERATIONS.md#connection-and-tls-policy) lists socket, frame, queue and codec bounds. A cancelled or failed mesh write retires its socket. Application requests are never silently replayed; query the original task UUID after an uncertain outcome. Read-only restart recovery permits deterministic work under the original owner and deadline. Component limits are not measured availability or a process-wide memory guarantee.

## Independent review checklist

- [ ] Assess authentication/secrecy goals, classical identity signatures, and the absence of a reviewed hybrid exchange.
- [ ] Assess transcript/domain separation, direction and identity binding, key confirmation, bootstrap and recipient pins.
- [ ] Assess replay/order handling, counter exhaustion, nonce/key lifecycle, errors and shutdown.
- [ ] Assess randomness, stored key permissions, crash/restart behavior and realistic erasure claims.
- [ ] Assess malformed-input handling and resource budgets across handshake, frames, codecs, queues, discovery and RPC.
- [ ] Assess downgrade/transport selection, pooled identity changes, cancellation, redial and mixed-version behavior.
- [ ] Assess application ownership, reply correlation, idempotence, recovery and allowlist changes independently of channel authentication.
- [ ] Record reviewer, exact source digest, methodology, findings, residual risk and permitted deployment scope.

Automated regressions exercise signature/transcript failures, independent key establishment, authenticated frame ordering, incorrect pins, plaintext/bootstrap refusal, cancelled-write retirement, admission recovery and worker restart. They provide implementation evidence and do not replace independent review.
