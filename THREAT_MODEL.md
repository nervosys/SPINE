# SPINE Threat Model

Corrected 2026-11-20 against the source at 3d58059; file paths updated after the handshake moved to `spine-session` and the mesh to `spine-mesh`. Line numbers are as of 3d58059 and may have shifted. Constructions that are not in the deployed mesh handshake (X3DH, ratcheting, X25519, ChaCha20-Poly1305, the "security levels" table) are now under "Not deployed / research" below.

## Overview

SPINE is a **headless semantic browser** for AI agents. On the native client to daemon wire (spine-wire v1, `docs/spec/00-wire.md`) confidentiality and server authentication come from TLS only; there is no SPINE-level encryption, obfuscation, morphing, padding or traffic-analysis resistance on that path. This document defines our security assumptions, adversary model, and mitigations.

## What SPINE Is NOT

- **Not a new internet protocol** - runs over standard TCP/IP
- **Not a replacement for TLS** - uses TLS as baseline, adds optional layers
- **Not guaranteed quantum-safe** - RLWE parameters are conservative estimates
- **Not a trustless system** - requires some initial trust establishment

## Adversary Tiers

### Tier 1: Passive Network Observer
**Capabilities**: Packet capture, timing analysis, traffic correlation
**Cannot**: Modify packets, compromise endpoints, break standard crypto

**Mitigations**:
- TLS 1.3 baseline encryption
- Experimental latent-space encoding and a Chameleon key exist only in `research/spine-protocol-research`; they are not on the native wire and are not a mitigation

### Tier 2: Active Network Attacker (MITM)
**Capabilities**: Packet injection, replay attacks, connection hijacking
**Cannot**: Compromise private keys, break authenticated encryption

**Mitigations** (deployed mesh handshake; see "Deployed mesh handshake" below):
- Signed ephemeral ML-KEM-768 exchange with Ed25519 identity signatures
- Forward secrecy from a per-connection ephemeral KEM key (no ratcheting, no rekeying)
- AES-256-GCM frames with strict counter ordering; the counter is also the AAD
  (`src/spine-session/src/handshake.rs:487`, `:517`)

### Tier 3: Compromised Node
**Capabilities**: Full access to one node's memory/keys
**Cannot**: Compromise other nodes, forge identities of uncompromised nodes

**Mitigations**:
- Sybil resistance via stake-weighted voting
- Node reputation tracking
- Key rotation limits blast radius

### Tier 4: Nation-State (Quantum Computer)
**Capabilities**: Store-now-decrypt-later, quantum factoring
**Cannot**: Break lattice assumptions (conjectured)

**Mitigations**:
- Deployed mesh handshake: ML-KEM-768 (FIPS 203) for confidentiality. Identity
  signatures are Ed25519, so authentication is not post-quantum.
- The custom RLWE KEM and RLWE+ML-KEM hybrid in `research/spine-crypto/src/lib.rs` are
  not used by the mesh handshake (see "Not deployed / research")
- **Explicit limitation**: RLWE security unproven

## Attack Vectors

### 1. Key Distribution Bootstrap
**Problem**: How do agents establish initial trust?
**Solution (deployed)**: the peer's Ed25519 mesh key doubles as its DHT keyspace position and name-record authority; a dialer may pin the expected key (`Initiator::finish`, `handshake.rs:265`).
- Updated 2026-11-21: every authenticated dial now carries an expected key. A pooled dial to an agent id expects the key named by the hello, ack or referral that introduced the peer (`NameTransport::learn_peer`; the first claim stands), or the signed `recipient_key` of an application call. A bare seed address with no configured key is accepted on first contact only, its key is recorded and required on every later dial in this process, and `namespace.seed_keys` configures a seed's key from the first dial. A mismatch is a hard failure (`NameMeshError::PinMismatch`, logged and counted). Pins are in memory and are forgotten on restart, so first contact with an unconfigured seed is trust on first use once per process. (Before this date every DHT dial made by spine-core was unpinned.)
- A pin from a referral is the referrer's unsigned claim; a malicious referrer can make dials to an id it names first fail, not reach an impostor.
- QUIC mesh (2026-11-21): TLS 1.3 protects the bytes and the certificate set given to `trust_certificate` says which endpoint answered; the first-stream mesh handshake says which key is on the connection, and that key is kept for every stream of the connection. The handshake is not bound to the TLS channel, so a party able to terminate TLS with a certificate the client trusts can relay a genuine peer's handshake and sit in the middle; the mitigations are trusting only given certificates and pinning the expected key. Closing it needs a channel-context input in the handshake, which is under review.
- Added 2026-11-21: handlers no longer trust `envelope.from`. A `NameResolveResponse` is absorbed only when its signature verifies under a key recorded for that agent id and, on an authenticated connection, under the session key; hellos and acks must carry the session key. `AgentId` is still not derived from the key, so the label remains the sender's claim. Trust table: `src/spine-mesh/README.md`.
- Added 2026-11-21: `MeshNode::process_envelope`, which verified a signature only for senders in a trust map and let every other sender through, had no caller and was removed. The QUIC client no longer accepts any certificate; `QuicNameTransport` trusts only certificates it was given.
- X3DH is not used (see "Not deployed / research").
- **Assumption**: first contact with a bootstrap address is trusted

### 2. Sybil Attacks on Consensus
**Problem**: Attacker creates many fake nodes to control voting
**Solution**: Stake-weighted consensus
- Nodes must stake tokens to participate
- Voting power proportional to stake
- Malicious behavior results in stake slashing
- **Assumption**: Economic cost deters spam

### 3. Neural Encoder Adversarial Inputs
**Problem**: Crafted inputs that decode to different semantics
**Solution**: Semantic checksums
- UR hash included in encrypted payload
- Mismatch triggers re-parse from source
- **Limitation**: Adds latency

### 4. Traffic Analysis
**Problem**: Packet timing reveals communication patterns
**Solution**: None in the native wire. The frame header is fixed and there is no padding; the Chameleon key in `research/spine-protocol-research` is not on the native wire.
- **Limitation**: Message sizes and timing are visible to an observer

### 5. Memory Disclosure
**Problem**: Side-channels leak neural memory contents
**Solution**: Working memory isolation
- Sensitive data in separate memory region
- Automatic clearing after use
- **Limitation**: Cannot prevent all timing attacks

### 6. Swarm Coordination Attacks
**Problem**: Malicious agents disrupt collective behavior
**Solution**: Reputation-based filtering
- Low-reputation agents ignored
- Gradual trust accumulation
- **Limitation**: New agents have limited influence

### 7. Legacy Web Bridge Exploitation
**Problem**: spine-human exposes attack surface to traditional web
**Solution**: Sandboxing
- Separate process for legacy interactions
- Limited data sharing with main agent
- **Assumption**: OS process isolation holds

### 8. Denial of Service
**Problem**: Resource exhaustion attacks
**Solution**: Rate limiting + proof-of-work
- Connection limits per identity
- Computational puzzles for resource requests
- **Limitation**: Legitimate high-volume use affected

## Deployed mesh handshake

Source: `src/spine-session/src/handshake.rs` (`spine-mesh-hs-v1`); full description in `HANDSHAKE-REVIEW.md`. Not externally reviewed.

| Role | Primitive |
|---|---|
| Key exchange / KEM | Ephemeral ML-KEM-768 (no Diffie-Hellman; no hybrid) |
| Signature | Ed25519, verified with `verify_strict` (`handshake.rs:165`; name records `spine-name/src/record.rs:280`; agent identities `spine-mesh/src/identity.rs:126`). `spine-agentic/src/swe.rs` was switched later the same day; an audit found no other Ed25519 verification site. |
| KDF | HKDF-SHA-256, salt = SHA-256(msg1 and msg2) |
| AEAD | AES-256-GCM, per-direction key, counter nonce, counter as AAD |

Properties the code does not provide:
- No key confirmation: no message under the session keys is exchanged or required to finish the handshake.
- No identity hiding: both identities and signatures travel in clear (msg1, msg2 are unencrypted).
- No rekeying; a session uses its two keys for its lifetime.
- The initiator signature covers only its label, ephemeral key and identity (`handshake.rs:215-219`), not the responder, so a recorded first message is replayable. A replayed hello makes the responder complete the handshake and attach a session (`mesh_tcp.rs:449-458`, `:1139-1148`); the replayer cannot derive session keys.
- Post-quantum confidentiality only; authentication is classical.

## Not deployed / research

The following describe constructions that are NOT in the deployed mesh handshake. They are retained as design/research material.

### Security Levels (research; `research/spine-crypto/src/lib.rs` KEM options, example output only)

| Level | Key Exchange | Encryption | Use Case |
|-------|-------------|------------|----------|
| Standard | X25519 | ChaCha20-Poly1305 | Most applications |
| Hardened | X25519 + RLWE | ChaCha20-Poly1305 | High-value targets |
| PostQuantum | RLWE only | ChaCha20-Poly1305 | Future-proofing |

## Explicit Assumptions

1. **Cryptographic hardness**: ML-KEM-768, Ed25519, SHA-256, HKDF and AES-256-GCM assumptions hold (deployed handshake); RLWE only for the research KEM
2. **Random number generation**: OS CSPRNG is secure
3. **Memory safety**: Rust prevents buffer overflows
4. **Network availability**: TCP/IP routing functions
5. **Clock synchronization**: Nodes have roughly accurate time
6. **Economic rationality**: Attackers respond to incentives

## Known Limitations

1. **No anonymity guarantees** - IP addresses visible to peers
2. **Trust bootstrapping** - Requires directory service
3. **Quantum security unproven** - RLWE is best-effort
4. **Performance vs security tradeoff** - Higher security = slower
5. **Side-channel resistance incomplete** - Timing attacks possible

## Incident Response

1. **Key compromise**: Revoke via directory, rotate all keys
2. **Sybil attack detected**: Increase stake requirements
3. **Protocol vulnerability**: Emergency upgrade mechanism
4. **Memory leak**: Automatic restart with fresh state

## References

- Signal Protocol specification (X3DH, Double Ratchet) - design reference only, not deployed
- NIST Post-Quantum Cryptography standardization
- Sybil attack literature (Douceur 2002)
- BBR congestion control (Cardwell et al.)
