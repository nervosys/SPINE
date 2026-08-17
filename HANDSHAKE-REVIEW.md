# SPINE mesh handshake — review package

A self-contained description of the `spine-mesh-hs-v1` handshake, written so a
cryptographer can review it without reading the codebase. Implementation:
`src/spine-crypto/src/handshake.rs`.

**What is being asked:** whether the construction below delivers the properties
claimed in §4, and whether the concessions in §5 are the only ones.

---

## 1. Setting and threat model

SPINE is an agent-to-agent mesh. Peers exchange **individually signed
envelopes** carrying DHT name-resolution traffic: "resolve `spine://did:…`",
"who provides `web.search`", and signed name records in reply.

Because every payload is independently signed and verifiable, the transport is
**not** the basis for payload integrity. What the transport must provide is
*confidentiality*, and the reason is specific: a passive observer of resolution
traffic learns which names an agent resolves and which capabilities it hunts
for, which is a direct read on what the agent is doing. Metadata is the asset.

Assumed adversary:

- **Passive**: records all traffic; may later compromise long-term keys.
- **Active**: injects, replays, reorders, and drops; may impersonate an address.
- **Not assumed**: compromise of a live endpoint's memory, or of the OS CSPRNG.

Each node holds one long-term Ed25519 keypair. That key is not only a transport
credential — the same public key is the node's position in the 256-bit DHT
keyspace and the authority half of every name it publishes. "The peer I dialed",
"the node responsible for this key", and "the peer whose records I trust" are
intended to be one fact rather than three.

## 2. Primitives

| Role | Primitive |
|---|---|
| KEM | ML-KEM-768 (FIPS 203), `ml-kem` 0.2 |
| Signatures | Ed25519, `ed25519-dalek` |
| KDF | HKDF-SHA-256 |
| AEAD | AES-256-GCM |
| Transcript hash | SHA-256 |
| Randomness | OS CSPRNG (`rand::rngs::OsRng`) |

No classical/PQ hybrid KEM: ML-KEM-768 is used alone. Authentication is
classical (Ed25519) and therefore not itself post-quantum; see §5.

## 3. The protocol

Two messages. `‖` is concatenation. Every wire field is `u16` big-endian
length-prefixed; messages are capped at 4096 bytes, checked before allocation.

```
Constants
  PROTOCOL_LABEL   = "spine-mesh-hs-v1"
  LABEL_INITIATOR  = "spine-mesh-hs-v1/initiator"
  LABEL_RESPONDER  = "spine-mesh-hs-v1/responder"
  INFO_I2R         = "spine-mesh-i2r"
  INFO_R2I         = "spine-mesh-r2i"

Initiator I (long-term signing key sk_I, public id_I)
Responder R (long-term signing key sk_R, public id_R)

I: (ek, dk) ← ML-KEM-768.KeyGen()                    [OS CSPRNG]
   sig_I    ← Sign(sk_I, LABEL_INITIATOR ‖ ek ‖ id_I)
   msg1     =  PROTOCOL_LABEL ‖ ek ‖ id_I ‖ sig_I     (field-framed)
   I → R: msg1

R: parse msg1; require |ek| = 1184, |id_I| = 32, |sig_I| = 64
   verify sig_I under id_I over (LABEL_INITIATOR ‖ ek ‖ id_I)
   (ct, ss) ← ML-KEM-768.Encapsulate(ek)             [OS CSPRNG]
   sig_R    ← Sign(sk_R, LABEL_RESPONDER ‖ SHA-256(msg1) ‖ ct ‖ id_R)
   msg2     =  PROTOCOL_LABEL ‖ ct ‖ id_R ‖ sig_R
   R → I: msg2

I: parse msg2; require |ct| = 1088, |id_R| = 32, |sig_R| = 64
   if a peer identity was pinned, require id_R = expected     (checked first)
   verify sig_R under id_R over (LABEL_RESPONDER ‖ SHA-256(msg1) ‖ ct ‖ id_R)
   ss ← ML-KEM-768.Decapsulate(dk, ct)

Both:
   transcript = SHA-256(msg1 ‖ msg2)
   HKDF-SHA-256(salt = transcript, ikm = ss)
     k_i2r = Expand(INFO_I2R, 32)
     k_r2i = Expand(INFO_R2I, 32)
```

### Record layer

Each direction has its own key and its own 64-bit counter. Nonce = the 8-byte
big-endian counter left-padded into the 12-byte GCM nonce. AAD is empty.
Separate keys per direction mean the two counters cannot collide, which is the
failure mode that makes GCM catastrophic rather than merely broken. `dk` is
zeroed on drop (`ZeroizeOnDrop`), as are session keys.

## 4. Properties claimed

1. **Confidentiality against a passive adversary**, including one who later
   compromises either long-term key. `ss` is protected by an ephemeral ML-KEM
   keypair that exists only for the connection; the long-term keys only ever
   sign.
2. **Forward secrecy**, on the same basis.
3. **Responder authentication**: `sig_R` covers `SHA-256(msg1)`, which commits
   to a freshly generated `ek`, so `msg2` cannot be replayed into another
   session. A dialer that pins `expected_peer` will not complete a handshake
   with any other identity.
4. **Initiator authentication**: `sig_I` covers `ek ‖ id_I`, so the responder
   learns which mesh identity opened the connection — used to bind the
   connection to a keyspace position.
5. **Domain separation**: distinct labels for each signature and each derived
   key; a signature from one role cannot be replayed into the other.
6. **Key independence per connection** (see the test
   `separate_connections_derive_independent_keys`, and §6).

## 5. Known concessions

**`InitiatorHello` is replayable.** A recorded `msg1` can be replayed to a
responder, which will complete a handshake with it. Judged to gain the attacker
nothing: without `dk` it cannot derive `k_i2r`/`k_r2i`, so it can neither read
nor send a frame. Eliminating it needs a third message or a responder-supplied
nonce. **Review question:** is the resulting responder-side work a meaningful
DoS amplification, given ML-KEM encapsulation plus one Ed25519 verification per
replayed hello?

**Authentication is classically secure only.** Confidentiality is
post-quantum (ML-KEM); the signatures are Ed25519. A future quantum adversary
could impersonate a node going forward, but could not decrypt recorded traffic.
This was a deliberate ordering — recorded traffic is the harvest-now-decrypt-
later asset — but it is worth confirming.

**No downgrade negotiation, by construction.** There is one ciphersuite. The
protocol label is compared for exact equality, so version skew is a refusal to
talk rather than a negotiation. **Review question:** does the absence of a
version field make a future migration harder than the downgrade risk it avoids?

**Unpinned dialing exists.** `expected_peer = None` accepts whoever answers.
This is used exactly once — dialing a bootstrap seed by address, where
establishing the identity is the purpose of the exchange. The identity proven is
then used for every subsequent connection to that peer. **Review question:** is
"trust the first answer at an address, pin thereafter" adequately bounded here?

**AAD is empty**, and the record layer carries no explicit length or type field
beyond what the framing above it provides.

**QUIC transport certificates authenticate nothing.** `QuicEndpointBuilder` uses
self-signed certificates with a permissive verifier; endpoint authentication
comes from this handshake, which must be enabled explicitly via
`QuicNameTransport::authenticated`.

## 6. A defect already found, and what it says about where to look

Until Phase 42 the ephemeral keypair was generated by
`StdRng::seed_from_u64(seed)`. The seed was **not** random: the TCP and
WebSocket listeners passed a counter starting at 0 and incremented per accepted
connection, and dialers passed a configured constant plus the current connection
pool size.

So the Nth connection a node accepted after start-up always used the same
ephemeral keypair. An attacker who could count connections — or simply try small
integers — could regenerate `dk`, recover `ss`, and decrypt the session. Forward
secrecy was nominal: the keypair was ephemeral in lifetime but not in value. The
arrangement of primitives was standard throughout; only what they were fed was
wrong.

The existing test suite did not catch it. `separate_connections_derive_independent_keys`
passed two different constants, and so only ever demonstrated that different
seeds produce different keys. It is now accompanied by
`every_handshake_generates_a_fresh_ephemeral_key` and
`a_responder_draws_fresh_randomness_for_every_connection`, which assert
unpredictability rather than mere difference.

The lesson for a reviewer: the composition here is conventional and the
primitives are vetted, so the likeliest remaining defects are of the same
species — a primitive fed something it does not require, or a property asserted
by a test that does not actually test it.

## 7. Where to look in the code

| Concern | Location |
|---|---|
| Message construction and parsing | `handshake.rs`, `Initiator::start`, `Responder::accept_with_rng` |
| Signature coverage | the `LABEL_*` byte strings and their `to_sign` buffers |
| Key schedule | `Session::derive` |
| Record layer, nonces, counters | `Session::seal` / `Session::open` |
| Identity pinning | `Initiator::finish`, `expected_peer` |
| Where pinning is skipped | `mesh_tcp.rs` / `mesh_ws.rs` / `mesh_quic.rs`, `provisional` |
| Test suite | `handshake.rs`, `mod tests` (25 tests) |

## 8. Status

Not externally reviewed. Appropriate for a mesh whose payloads are
independently signed. **Not** a substitute for TLS where the transport is the
only thing between an adversary and unauthenticated data.
