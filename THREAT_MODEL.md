# SPINE Threat Model

## Overview

SPINE is a **headless semantic browser with adaptive encryption** for AI agents. This document defines our security assumptions, adversary model, and mitigations.

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
- Latent-space encoding obscures semantic content
- Chameleon protocol varies packet patterns

### Tier 2: Active Network Attacker (MITM)
**Capabilities**: Packet injection, replay attacks, connection hijacking
**Cannot**: Compromise private keys, break authenticated encryption

**Mitigations**:
- X3DH key exchange with identity verification
- Forward secrecy via ratcheting
- Message authentication codes

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
- Optional RLWE post-quantum mode
- Hybrid X25519+RLWE for defense in depth
- **Explicit limitation**: RLWE security unproven

## Attack Vectors

### 1. Key Distribution Bootstrap
**Problem**: How do agents establish initial trust?
**Solution**: X3DH (Extended Triple Diffie-Hellman)
- Identity keys registered with directory service
- Ephemeral keys for forward secrecy
- **Assumption**: Directory service is honest

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
**Solution**: Chameleon protocol
- Rotating ciphersuites
- Variable padding
- **Limitation**: Cannot fully hide traffic volume

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

## Security Levels

| Level | Key Exchange | Encryption | Use Case |
|-------|-------------|------------|----------|
| Standard | X25519 | ChaCha20-Poly1305 | Most applications |
| Hardened | X25519 + RLWE | ChaCha20-Poly1305 | High-value targets |
| PostQuantum | RLWE only | ChaCha20-Poly1305 | Future-proofing |

## Explicit Assumptions

1. **Cryptographic hardness**: X25519, ChaCha20, RLWE assumptions hold
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

- Signal Protocol specification (X3DH, Double Ratchet)
- NIST Post-Quantum Cryptography standardization
- Sybil attack literature (Douceur 2002)
- BBR congestion control (Cardwell et al.)
