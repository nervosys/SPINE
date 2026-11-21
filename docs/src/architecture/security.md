# Security

See [THREAT_MODEL.md](https://github.com/nervosys/SPINE/blob/master/THREAT_MODEL.md) for the full threat model.

## Security Levels

| Level       | Key Exchange  | Encryption            | Use Case            |
| ----------- | ------------- | --------------------- | ------------------- |
| Standard    | X25519        | AES-256-GCM           | General use         |
| Hardened    | X25519 + RLWE | AES-256-GCM + lattice | High security       |
| PostQuantum | RLWE-only     | Lattice-based         | Quantum adversaries |

## X3DH Key Exchange

Initial trust establishment uses the Extended Triple Diffie-Hellman protocol:
1. Identity keys (long-term)
2. Signed pre-keys (medium-term)
3. One-time pre-keys (single-use)

No pre-shared secrets required.

## Chameleon Protocol (research)

An experimental, unreviewed latent-space encoding with per-message key evolution. It lives in `research/spine-protocol-research`, not in `spine-protocol`, and is not on the native wire. It is not encryption and gives no forward secrecy; use TLS or the session layer for confidentiality.

## Sybil Resistance

Decentralized trust without central authority:
- **Stake-weighted voting** — Influence proportional to commitment
- **Node reputation** — Earned through honest behavior
- **Proof-of-work for identity** — Computational cost prevents mass identity creation

## RLWE Lattice Cryptography

Post-quantum key exchange using Ring Learning With Errors:
- Based on hardness of shortest vector problem
- Forward-secure key evolution
- 12 comprehensive security tests
