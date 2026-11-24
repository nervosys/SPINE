# Formal Verification

SPINE keeps several formal models and audit documents under `formal/`. Most of them model research code or an earlier design, not the deployed path. The deployed constructions are the native wire (spine-wire v1, TLS only for confidentiality; no model here covers it) and the mesh handshake in `spine-session`, which is custom cryptography without external review. These artifacts were not updated when the wire changed on 2026-11-21.

## TLA+ — Protocol Model Checking

**Mesh routing**: `formal/tla/MeshRouting.tla` models TTL, shortest-path routing, gossip discovery and deduplication in the mesh.

**Chameleon (research)**: `formal/tla/ChameleonProtocol.tla` models the Chameleon Protocol state machine (experimental code in `research/spine-protocol-research`, not on the native wire) with:
- Epoch monotonicity invariant
- Synchronized evolution between peers
- Morphology abstraction for latent-space transformations
- Decoy message generation

Verified with TLC model checker (`ChameleonProtocol_MC.tla` configuration).

## Tamarin Prover — Key Exchange (research design)

**File**: `formal/tamarin/SpineKeyExchange.spthy`

Models an X3DH + RLWE hybrid key exchange, which is not the deployed mesh handshake (that is ML-KEM-768 with Ed25519 in `spine-session`). The model has 10 lemmas:
- Session key secrecy
- Perfect forward secrecy (PFS)
- Key compromise impersonation (KCI) resistance
- Three security levels: Standard, Hardened, PostQuantum

## Kani — Bounded Model Checking

**File**: `research/spine-kernel/src/kani_harnesses.rs` (research crate)

15 verification harnesses for unsafe Rust code:
- **Allocators**: BumpAllocator, SlabAllocator, ArenaAllocator bounds
- **Lock-free structures**: SeqLock, LockFreeStack, AtomicFlags correctness
- **Ring buffers**: SPSC/MPSC wraparound, capacity limits
- **SIMD**: Dot product numerical bounds

## Cryptographic Audit (historical, internal)

**File**: `formal/audit/CRYPTO_AUDIT.md`

An internal audit dated 2026-02-16 of the crypto code as it then stood, which included the PSK AES-GCM, `ChameleonKey` and `ProtocolMorphology` code that has since been removed from `spine-protocol`. It is not an external review and does not cover `spine-session`. 13 findings across severity levels:
- 2 Critical (RLWE KEM, XOR cipher) — **remediated**
- 4 High (key evolution, SeqLock, ABA, mmap) — **remediated**
- 4 Medium (morphology HMAC, PoW, NIST params, nonces) — **remediated**
- 3 Low (compression oracle, constant-time, docs) — **remediated**

## MISRA Compliance

**File**: `formal/misra/MISRA_COMPLIANCE.md`

16 MISRA C:2012 rules mapped to Rust unsafe code with 8 documented deviations and Kani verification links.
