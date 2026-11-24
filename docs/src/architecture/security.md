# Security

See [THREAT_MODEL.md](https://github.com/nervosys/SPINE/blob/master/THREAT_MODEL.md) for the full threat model.

## Native Client Wire

The native wire between an agent and the `spine-core` daemon is spine-wire v1
([Protocol](./protocol.md)). It does no encryption of its own. **Confidentiality
and authentication on this path come from TLS only**, with optional mutual TLS
for client certificates. A plain TCP or WebSocket connection is plaintext.

There is no SPINE-level encryption, obfuscation, morphing, padding,
pre-shared-key AES-GCM, post-quantum protection or traffic-analysis resistance
on the native wire. The wire's contribution to security is structural: bounded
frames (64 KiB before `HELLO`, 16 MiB after), strictly deterministic CBOR bodies
that reject malformed input, and closed command and error sets.

The daemon applies an outbound fetch policy to `navigate` (the `outbound_*`
error codes), a per-IP connection limit, and a `HELLO` timeout.

## Mesh Path

Node-to-node mesh traffic is a different protocol. `spine-session` performs a
signed ephemeral ML-KEM-768 exchange with Ed25519 identity signatures and
derives AES-256-GCM session keys, giving forward secrecy per connection. Mesh
envelopes and name records are independently signed with Ed25519, so authorship
can be checked without trusting the transport. The session handshake is custom
cryptography that has not had external cryptographic review. Details and limits
are in the `spine-session` crate documentation and `THREAT_MODEL.md`.

## Research (not deployed)

The following exist only in the separate `research/` workspace and are not in the
native wire or the mesh handshake: the Chameleon latent-space key and latent
AEAD (`research/spine-protocol-research`), the custom RLWE key exchange
(`research/spine-crypto`), an X3DH example (`research/spine-agentic`), and
stake-weighted Sybil resistance (`research/spine-cluster`). They are
experimental and unreviewed. The earlier "security levels" table (Standard,
Hardened, PostQuantum) described this research code, not a deployed feature.
