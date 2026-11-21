# SPINE layer 1: Session

Channel security, framing and streams, as built.

Status: descriptive, not normative. This document records what the code in the `completion/agent-web` checkout does. Where the code and another document disagree, the code is treated as authoritative and the disagreement is listed in section 10. No cargo command was run to produce this document; claims about test results rest on the log files named in section 6 and may not reflect HEAD. Statements marked "Undetermined" name the file that was searched.

Source paths are relative to the repository root. `path:line` citations refer to the checkout at commit `7f7d1fc`.

## 1. Scope and terminology

Layer 1 in `docs/FIRST_PRINCIPLES.md` is "Session": connect to a key, multiplexed streams, credit-based backpressure, cancel with reason, resumption and migration, and channel security from a published construction. This document describes four separate things that the repository places under that heading, because they are not one design:

| Name used here | What it is | Where |
| --- | --- | --- |
| **Mesh channel** | The encrypted, key-authenticated TCP channel that carries DHT name-resolution traffic and application RPC between mesh nodes. Uses the custom `spine-mesh-hs-v1` handshake. This is the only SPINE-specific channel security that is wired into a production binary. | `research/spine-crypto/src/handshake.rs`, `research/spine-agentic/src/mesh_tcp.rs`, wired by `src/spine-core/src/namespace.rs:56-83` |
| **Mesh alternates** | The same handshake over WebSocket and QUIC, used only by tests and library users. | `research/spine-agentic/src/mesh_ws.rs`, `mesh_quic.rs` |
| **Native wire** | The client-facing protocol served by `spine-core` on TCP, WebSocket and optional QUIC: a length-prefixed frame carrying a CBOR message body. Security is rustls TLS or nothing. | `src/spine-protocol/src/lib.rs`, `wire.rs`; served by `src/spine-core/src/main.rs:2152-2415` |
| **Transport crate and stream crate** | `Frame` with a 12-byte header, `FrameCodec`, congestion and pooling in `spine-transport`; a stream multiplexer, flow controller and priority queues in `spine-stream`. | `src/spine-transport/`, `src/spine-stream/` |

Terms:

- **Initiator (I)**, **Responder (R)**: the dialing and accepting side of the mesh handshake.
- **Identity**: a 32-byte Ed25519 public key. The same key is the node's DHT keyspace position and the authority of the name records it signs (`HANDSHAKE-REVIEW.md:30-34`).
- **Pin**: an expected 32-byte responder identity supplied to the dialer before the handshake.
- **Envelope**: `MeshEnvelope`, a JSON object carrying a payload and an Ed25519 signature by the originator (`research/spine-agentic/src/mesh.rs:149-166`). Envelopes are the plaintext that the mesh channel protects.
- **Record**: one AEAD-sealed unit on the mesh channel (section 2.9).
- **Frame**: used in two senses. "Mesh frame" is the u32-length-prefixed unit in `mesh_tcp.rs`. "Native frame" is the unit in `spine-protocol`. "Transport `Frame`" is the 12-byte-header struct in `spine-transport`. Section 4 keeps them apart.
- **`‖`**: byte concatenation.

## 2. The mesh handshake as deployed

### 2.1 Where it runs

`spine-core` joins the namespace in `join_with_application` (`src/spine-core/src/namespace.rs:46-83`). It reads or creates a 32-byte seed from `cfg.key_path`, derives one Ed25519 `SigningKey` from it, and builds `TcpNameTransport` with `Security::Encrypted` using that key (`namespace.rs:59-64`). The same key is passed to the listener (`namespace.rs:75-83`). Every mesh connection, dialed or accepted, therefore performs the handshake below, over TCP. Plaintext mode exists (`Security::Plaintext`, `mesh_tcp.rs:704-713`) but `namespace.rs` does not select it.

`mesh_ws.rs` and `mesh_quic.rs` are not constructed by any non-test code outside their own files (searched `src/` for `WsNameTransport::`, `QuicNameTransport::`; no caller). Section 3 and 6 describe them.

### 2.2 Keys

| Key | Type | Lifetime | Generation | Role |
| --- | --- | --- | --- | --- |
| Identity key `sk`, `id = pk` | Ed25519 | Long-term; file-backed seed | Seed read or created by `load_or_create_key` (`namespace.rs`, see lines near 287-326) | Signs handshake messages only. Never used for key agreement. |
| Initiator KEM key pair `(ek, dk)` | ML-KEM-768 | One connection | `MlKem768::generate(rng)` with `OsRng` by default (`handshake.rs:198-199, 211`) | `ek` (1184 bytes) is sent in clear. `dk` is held as bytes in `Initiator` and zeroed on drop (`handshake.rs:175-182`). |
| Encapsulation result `(ct, ss)` | ML-KEM-768 ciphertext 1088 bytes, shared secret 32 bytes | One connection | `ek.encapsulate(rng)` with `OsRng` (`handshake.rs:332-333, 367-369`) | `ss` is the only input keying material. |
| Direction keys `k_i2r`, `k_r2i` | 32 bytes each | One connection | HKDF-Expand (`handshake.rs:442-446`) | AES-256-GCM keys. Held in `Session`, zeroed on drop (`handshake.rs:410-420`). |

There is no static key-agreement key. There is no pre-shared key. There is no resumption ticket.

The RNG seam `start_with_rng` / `accept_with_rng` is bounded on `CryptoRng + RngCore` (`handshake.rs:207-210, 336-340`). The production entry points `start` and `accept` call it with `OsRng`. An earlier defect, where the ephemeral key was seeded from a connection counter, is described in `HANDSHAKE-REVIEW.md:152-177`; the current code has no seeded path in `mesh_tcp.rs`.

### 2.3 Primitives and parameters

| Role | Primitive | Crate and version | Parameters |
| --- | --- | --- | --- |
| KEM | ML-KEM-768 (FIPS 203) | `ml-kem` 0.2.3 (`Cargo.lock`) | ek 1184 B, ct 1088 B, ss 32 B (`handshake.rs:95-97`) |
| Signature | Ed25519, `Verifier::verify` | `ed25519-dalek` 2.2.0 | pk 32 B, sig 64 B (`handshake.rs:98-99`) |
| Hash | SHA-256 | `sha2` 0.10 | transcript and message hash (`handshake.rs:152-156`) |
| KDF | HKDF-SHA-256 | `hkdf` 0.12 | salt = transcript hash, ikm = `ss`, two Expand calls of 32 B (`handshake.rs:442-446`) |
| AEAD | AES-256-GCM | `aes-gcm` 0.10.3 | 96-bit nonce, 128-bit tag, 8-byte AAD (`handshake.rs:476-531`) |
| Randomness | OS CSPRNG | `rand::rngs::OsRng` | (`handshake.rs:80`) |

There is one suite. There is no suite identifier on the wire beyond the protocol label.

### 2.4 Framing around the handshake

The handshake messages are carried in the mesh frame format of `mesh_tcp.rs`, before any encryption:

```
mesh frame  = len (u32, big-endian) ‖ body[len]
              accepted range for handshake frames: 1 ..= 4096   (mesh_tcp.rs:464-501; MAX_HANDSHAKE_BYTES, handshake.rs:102)
```

Inside the body, `handshake.rs` uses its own field encoding: each field is `u16 big-endian length ‖ bytes` (`handshake.rs:132-150`). The whole message is capped at 4096 bytes before parsing (`handshake.rs:247, 341`). A length that overruns the buffer yields `Malformed`.

There are two length prefixes, one from `mesh_tcp.rs` (u32) and one per field from `handshake.rs` (u16). Both are significant for an implementer.

The handshake has a 10 s deadline by default, enforced around the whole exchange on both sides (`mesh_tcp.rs:84, 889-896, 1137-1141`).

### 2.5 Message flow

Constants (`handshake.rs:87-92`):

```
PROTOCOL_LABEL  = "spine-mesh-hs-v1"
LABEL_INITIATOR = "spine-mesh-hs-v1/initiator"
LABEL_RESPONDER = "spine-mesh-hs-v1/responder"
INFO_I2R        = "spine-mesh-i2r"
INFO_R2I        = "spine-mesh-r2i"
```

Wire format of each message (field-framed as above):

```
msg1 (I -> R) = field(PROTOCOL_LABEL) ‖ field(ek: 1184) ‖ field(id_I: 32) ‖ field(sig_I: 64)
msg2 (R -> I) = field(PROTOCOL_LABEL) ‖ field(ct: 1088) ‖ field(id_R: 32) ‖ field(sig_R: 64)
```

Total sizes: msg1 = 4 × 2 + 16 + 1184 + 32 + 64 = 1304 bytes; msg2 = 4 × 2 + 16 + 1088 + 32 + 64 = 1208 bytes. Both are inside the 4096 cap.

| Step | Actor | Action | Cite |
| --- | --- | --- | --- |
| 1 | I | Generate `(dk, ek)` with ML-KEM-768 using `OsRng`. | `handshake.rs:211` |
| 2 | I | `sig_I = Ed25519.Sign(sk_I, LABEL_INITIATOR ‖ ek ‖ id_I)`. | `handshake.rs:215-219` |
| 3 | I | Build `msg1`; keep `dk` bytes, `msg1` bytes and a clone of the signing key in the `Initiator`. | `handshake.rs:221-234` |
| 4 | I | Send `msg1` as one mesh frame. | `mesh_tcp.rs:426-427` |
| 5 | R | Read one mesh frame, 1..=4096 bytes. | `mesh_tcp.rs:453-455` |
| 6 | R | Reject if length > 4096, label differs from `PROTOCOL_LABEL` (`WrongProtocol`), any field is the wrong size (`Malformed`). | `handshake.rs:341-355` |
| 7 | R | Parse `id_I` as an Ed25519 verifying key; verify `sig_I` over `LABEL_INITIATOR ‖ ek ‖ id_I`. No check of `id_I` against any list at this layer. | `handshake.rs:357-362` |
| 8 | R | `(ct, ss) = ML-KEM-768.Encapsulate(ek)` using `OsRng`. | `handshake.rs:364-369` |
| 9 | R | `sig_R = Ed25519.Sign(sk_R, LABEL_RESPONDER ‖ SHA-256(msg1) ‖ ct ‖ id_R)`. | `handshake.rs:374-381` |
| 10 | R | Build `msg2`; derive the session from `(ss, msg1, msg2)` with role Responder. | `handshake.rs:383-394` |
| 11 | R | Send `msg2` as one mesh frame. The listener then calls `attach_secure_bounded`. | `mesh_tcp.rs:458`, `1143-1151` |
| 12 | I | Read one mesh frame, 1..=4096 bytes. Reject on size, label, field sizes. | `handshake.rs:247-261`, `mesh_tcp.rs:428-430` |
| 13 | I | If a pin was supplied, require `id_R == pin` before verifying the signature. | `handshake.rs:263-272` |
| 14 | I | Verify `sig_R` under `id_R` over `LABEL_RESPONDER ‖ SHA-256(msg1) ‖ ct ‖ id_R`. | `handshake.rs:274-280` |
| 15 | I | `ss = ML-KEM-768.Decapsulate(dk, ct)`. | `handshake.rs:282-293` |
| 16 | I | Derive the session from `(ss, msg1, msg2)` with role Initiator. | `handshake.rs:296-302` |
| 17 | I | Attach the session; the first sealed record may be sent immediately. | `mesh_tcp.rs:897-903` |

There is no third message. There is no explicit key-confirmation value. The first record that opens successfully in each direction is the only evidence that the peer derived the same keys.

### 2.6 Key schedule

`Session::derive(ss, msg1, msg2, peer, role)` (`handshake.rs:433-459`):

```
transcript = SHA-256( msg1 ‖ msg2 )          // msg1, msg2 exactly as sent, including field length prefixes and labels
PRK        = HKDF-Extract( salt = transcript, ikm = ss )
k_i2r      = HKDF-Expand( PRK, info = "spine-mesh-i2r", L = 32 )
k_r2i      = HKDF-Expand( PRK, info = "spine-mesh-r2i", L = 32 )
Initiator: tx = k_i2r, rx = k_r2i
Responder: tx = k_r2i, rx = k_i2r
tx_counter = rx_counter = 0
```

The info strings do not include the transcript, the identities or the role beyond the direction name. The identities enter the key schedule only through the transcript hash, because both `id_I` and `id_R` are fields of `msg1` and `msg2`. No handshake state other than `ss` is mixed into the IKM.

### 2.7 Transcript binding

| Value | Signed or hashed over | Not covered |
| --- | --- | --- |
| `sig_I` | `LABEL_INITIATOR ‖ ek ‖ id_I` (`handshake.rs:215-219`) | `ct`, `id_R`, any responder-supplied value, any nonce chosen by R. The initiator signature is not bound to the intended responder. |
| `sig_R` | `LABEL_RESPONDER ‖ SHA-256(msg1) ‖ ct ‖ id_R` (`handshake.rs:374-381`) | `PROTOCOL_LABEL` field of `msg2` as sent (the label is covered through `LABEL_RESPONDER`, which contains the protocol string, and through `msg1` hash). Covers all of `msg1`, `ct` and `id_R`. |
| Session keys | `SHA-256(msg1 ‖ msg2)` as HKDF salt (`handshake.rs:437-442`) | Not the MAC input; there is no handshake MAC. A disagreement about the transcript makes the keys differ, so the first record fails. |

The signed buffers are unambiguous without length prefixes because every variable component has a fixed length enforced before signing or verifying: label is a constant, `ek` is 1184, `id` is 32, `ct` is 1088, the hash is 32.

### 2.8 Authentication and identity pinning

How each side authenticates the other:

- **Responder to initiator.** The initiator verifies `sig_R`, which covers `H(msg1)` (and so the fresh `ek`), `ct` and `id_R`. Because `ct` is bound by the signature and only the holder of `dk` can decapsulate it, the initiator knows `msg2` was produced for this `msg1` by the holder of `sk_R`. Whether the initiator then *accepts* that identity depends on the pin (below).
- **Initiator to responder.** The responder verifies `sig_I` over `ek ‖ id_I`. This shows that the holder of `sk_I` signed this `ek` at some time. It does not show that the sender of `msg1` is live or that it holds `dk`. The responder learns `id_I` and exposes it as `Session::peer_identity()` (`handshake.rs:462-464`). No acceptance decision is made at this layer. Allowlisting happens in `spine-core` tasks (`CRYPTO_REVIEW_PACKET.md:11`).
- **Possession of `dk`.** Proven to R only when an AEAD record from I opens.

What is pinned, and when:

| Situation | Pin supplied to handshake | Source | Cite |
| --- | --- | --- | --- |
| Outgoing dial for a non-application envelope | `self.identities.get(agent)` | `set_identity`. **No caller of `set_identity` exists outside tests and the three `mesh_*.rs` files** (searched `src/`). In `spine-core` the map is therefore empty and this dial is unpinned. | `mesh_tcp.rs:786-797, 888, 913-915`; callers: none outside `#[cfg(test)]` |
| Outgoing dial for an `Application` envelope | `message.recipient_key`, taken from the signed application payload | The caller of the RPC | `mesh_tcp.rs:919-930`, `connection(.., required)` at 842-853, 888 |
| Reuse of a pooled connection | A pooled connection is reused only if it is open and, when a key is required, `authenticated_peer() == required` | | `mesh_tcp.rs:847-853` |
| Bootstrap dial (`send_to`) | None | By design: identity is the purpose of the dial | `mesh_tcp.rs:958-1015, 994-997` |
| Inbound connection | Not applicable; no responder-side acceptance list in the handshake | | `mesh_tcp.rs:1118-1167` |

After the handshake, for `Application` payloads only, `pump` drops an inbound envelope unless `reply.authenticated_peer() == Some(message.public_key)` (`mesh_tcp.rs:1173-1180`). A changed pin closes the pooled connection (`mesh_tcp.rs:794-796`). `deliver` never retries an `Application` envelope after a failed write (`mesh_tcp.rs:937-942`).

For non-application mesh traffic there is no comparison between `session.peer_identity()` and the key carried in the envelope or `NameHelloAck`. The ack's key is checked only by the envelope signature (`naming_mesh.rs:1011-1013`, `mesh.rs:734-737`).

> Update 2026-11-21: a comparison now exists. `pump` passes `ReplyPath::authenticated_peer()` to `MeshNameResolver::handle_envelope_from_peer`, which drops a `NameHello` or `NameHelloAck` whose carried key differs from it and requires a `NameResolveResponse` to be signed by it (counters `greetings_rejected` and `envelopes_unauthenticated`). Pins and bootstrap trust on first use are unchanged.

The mapping from a dialed target to a key goes through `AgentId`, a UUID. `SigningIdentity::from_seed` derives it as a hash of the seed, not of the public key, and `SigningIdentity::new` uses a random UUID (`research/spine-agentic/src/identity.rs:148, 167`; `lib.rs:233-251`). See observation O2.

### 2.9 AEAD record format after the handshake

Per-direction state: key (32 B) and a u64 counter, starting at 0 (`handshake.rs:410-420, 452-458`).

```
record on the mesh channel =
    len        u32 big-endian          // mesh frame length (mesh_tcp.rs:464-477)
    counter    u64 big-endian          // sent in clear, handshake.rs:492-494
    ciphertext ‖ tag(16 bytes)         // AES-256-GCM output

nonce (12 B) = 0x00000000 ‖ counter (u64, big-endian)       // handshake.rs:527-531
AAD          = counter (8 bytes, big-endian)                 // handshake.rs:487, 517
plaintext    = JSON-serialised MeshEnvelope                  // mesh_tcp.rs:531-532, 605
```

| Property | Value | Cite |
| --- | --- | --- |
| Record header | 8-byte counter, then ciphertext and tag | `handshake.rs:492-495` |
| Minimum record | 8 + 16 = 24 bytes; shorter is `BadFrame` | `handshake.rs:500-502` |
| Maximum plaintext | `MAX_FRAME_BYTES` = 4 MiB (4 194 304) envelope JSON | `mesh_tcp.rs:62, 533-538` |
| Maximum mesh frame accepted for a record | `MAX_FRAME_BYTES + 1024` | `mesh_tcp.rs:579` |
| Records per envelope | exactly one; there is no fragmentation | `mesh_tcp.rs:552-555` |
| Receive order | strict: counter must equal `rx_counter`, otherwise `BadFrame`; `rx_counter` advances only after a successful decrypt | `handshake.rs:503-509, 523` |
| Behaviour on failure | `secure_read_loop` returns, the connection closes; nothing is recoverable | `mesh_tcp.rs:592-603` |
| Send serialisation | sealing and the socket write happen under one lock so counters leave in order | `mesh_tcp.rs:539-561` |
| Partial write | a `WriteGuard` marks the connection closed unless the write completed, because the counter has advanced | `mesh_tcp.rs:136-148, 545-563` |
| Direction separation | independent keys per direction; the unit test shows a client cannot open its own record | `handshake.rs:660-667` |

Limits not present: no maximum number of records per key, no check for counter exhaustion (`tx_counter += 1`, `handshake.rs:478`), no rekey (section 2.11).

### 2.10 Replay and downgrade

Replay of handshake messages:

- A recorded `msg2` cannot complete a different handshake: `sig_R` covers `SHA-256(msg1)` and `ct`. Test: `a_responder_reply_cannot_be_replayed_into_another_session` (`handshake.rs:733-747`).
- A recorded `msg1` can be replayed to a responder. R verifies one Ed25519 signature, runs ML-KEM encapsulation, signs, and attaches a session that holds an admission permit until the 60 s frame timeout or close (`mesh_tcp.rs:1119-1151, 76`). The attacker cannot read or send records. This is acknowledged in `handshake.rs:52-58` and `HANDSHAKE-REVIEW.md:119-125`. There is no timestamp, nonce or cookie in `msg1`.

Replay of records: rejected by the strict counter and the AAD (`handshake.rs:503-509`; tests `a_replayed_frame_is_rejected`, `frames_delivered_out_of_order_are_rejected`).

Downgrade:

- One suite, no negotiation. The label is compared for exact equality in both directions (`handshake.rs:252-254, 346-348`). A different label yields `WrongProtocol` and the connection fails.
- There is no version field outside the label string `...-v1`. A later version must change the label and will not interoperate.
- Plaintext versus encrypted is a local configuration of each transport object, not a negotiated property. An encrypted listener will fail to parse a plaintext client's first frame as a handshake (test `a_plaintext_client_cannot_talk_to_an_encrypted_node`, `mesh_tcp.rs:2266`). A plaintext listener will fail to parse an encrypted client's `msg1` as an envelope. Neither side falls back automatically.
- For `Application` payloads, `deliver` refuses to send on a non-encrypted transport (`mesh_tcp.rs:919-929`).

### 2.11 Rekeying and lifetime

Undetermined whether any caller closes and re-dials connections on a schedule: searched `research/spine-agentic/src/mesh_tcp.rs` and `src/spine-core/src/namespace.rs`; found idle and frame timeouts only (`ConnectionLimits`, `mesh_tcp.rs:69-90`). The session itself has no rekey operation and no key update message. A connection ends on an error, an idle `frame_timeout` of 60 s without a complete frame (`mesh_tcp.rs:82-85, 578-580`), eviction, or process exit. Session keys are zeroed when `Session` drops (`handshake.rs:410-420`).

### 2.12 Concurrency and bounds around the channel

| Item | Value | Cite |
| --- | --- | --- |
| Max concurrent connections, each direction | 64 default, validated 1..=1024 | `mesh_tcp.rs:82-83, 93-110` |
| Handshake timeout | 10 s | `mesh_tcp.rs:84` |
| Frame timeout (time to receive a complete frame) | 60 s | `mesh_tcp.rs:85` |
| Inbound queue depth, then queue timeout | 64 items, 5 s | `mesh_tcp.rs:65, 86` |
| Write timeout | 10 s | `mesh_tcp.rs:87` |
| Registries (addresses, identities) | 1024 entries each | `mesh_tcp.rs:67, 786-820` |
| Zero-length frame | refused | `mesh_tcp.rs:206-208, 491-495` |

Backpressure on the mesh channel is TCP backpressure plus the bounded inbound queue: when the queue stays full for `queue_timeout`, the reader closes the connection (`mesh_tcp.rs:616-621`).

### 2.13 Tests that exercise the handshake

`handshake.rs` contains 25 `#[test]` functions (`handshake.rs:534-903`). They cover round trip, tamper, replay, reorder, truncation, wrong pin, unpinned dial, forged signature, swapped ephemeral key, reply replay, wrong label, malformed input, independent keys, and fresh randomness. `mesh_tcp.rs` has encrypted-connection tests including `the_query_is_not_readable_on_the_wire`, `a_dialer_refuses_a_peer_with_the_wrong_identity`, `a_tampered_frame_drops_the_connection`, `application_delivery_pins_the_handshake_even_with_a_pooled_socket` (`mesh_tcp.rs:2103-2400`). There are no known-answer test vectors. There is no fuzz target for the handshake (`fuzz/fuzz_targets/` lists frame, header, message, negotiation and parser targets only).

## 3. Where standard TLS is used, and how it relates

| Path | TLS | Mesh handshake | Relationship |
| --- | --- | --- | --- |
| Mesh TCP (production) | None. Raw TCP. | Yes, mandatory in `spine-core` | The handshake is the whole channel security. |
| Mesh WebSocket | `ws://` or `wss://` accepted by `tokio_tungstenite::connect_async` with the `native-tls` feature (`src/spine-transport/Cargo.toml:90`, `websocket.rs:96-103`). Server side in tests is plain `ws://` on a `TcpListener` (`mesh_ws.rs:265-309`). | Optional (`WsNameTransport::encrypted`), runs inside the WebSocket byte stream with the same u32 framing (`mesh_ws.rs:12-15`) | Intended to be layered: TLS authenticates the server operator, the handshake authenticates the mesh identity (`mesh_ws.rs:59-65`). No test uses `wss://`. |
| Mesh QUIC | TLS 1.3 inside QUIC, self-signed server certificate generated per endpoint, client verifier accepts any certificate [as of 2026-11-21 the client verifies the certificate against an explicit trusted set, empty by default] (`src/spine-protocol/src/lib.rs:2229-2284, 2307-2342`); server uses `with_no_client_auth` (`lib.rs:2246`). SNI is the constant `"localhost"` (`mesh_quic.rs:199-202`). | Optional (`QuicNameTransport::authenticated`). Runs once on the first bidirectional stream. The derived session keys are discarded (`mesh_quic.rs:213-224, 308-315, 451-465`). | The handshake provides identity only; QUIC's TLS provides record protection. The handshake is not bound to the TLS session. See O1. |
| Native TCP and WebSocket (`spine-core`) | rustls via `tokio-rustls`, Ring provider, `with_safe_default_protocol_versions` (`src/spine-core/src/tls.rs:75-120`). Off by default (`TlsConfig::default`, `config.rs:230-233`; bind host defaults to `127.0.0.1`, `config.rs:213`). When `tls.enabled`, the same acceptor wraps TCP and the WS port (`main.rs:2227, 2339-2358, 2375-2385`). `mutual_tls` requires an explicit CA, `WebPkiClientVerifier`, optional CRL (`tls.rs:75-115`, `docs/NATIVE_OPERATIONS.md:25`). | Not used | Native clients are authenticated, if at all, by X.509 client certificates. There is no SPINE identity binding on this path. |
| Native QUIC (`spine-core`, `--features quic`) | Server endpoint from `QuicEndpointBuilder::build_server`: self-signed, TLS 1.3, no client auth (`main.rs:2176-2212`, `lib.rs:2238-2263`). Startup is refused when `tls.enabled` (`main.rs:2163-2167`). | Not used | Unauthenticated and not covered by the configured TLS policy. |
| `spine-protocol` `SpineConnection::from_quic` / `connect_auto` | Same permissive client verifier [as of 2026-11-21 it verifies against the web PKI roots, so a self-signed server is refused and `Auto` falls back to TCP] (`lib.rs:2266-2284`). `TransportMode::Auto` tries QUIC for 2 s and then falls back to the TCP future (`lib.rs:2768-2805`). | Not used | Silent fallback from QUIC to TCP. |
| Native symmetric layer in `ProtocolHandler` | Not TLS | Not the mesh handshake | Pre-shared-key AES-256-GCM, section 4.7. Not enabled by any production caller. |

Summary of the relationship: the mesh handshake and TLS are independent. The mesh handshake exists because the mesh is meant to work without certificates and to bind a connection to the Ed25519 identity that signs records. TLS is used on the native client-facing listeners and, in QUIC, as an unauthenticated record layer. Nothing in the repository derives one from the other (no exporter, no channel binding, no certificate carrying the mesh key).

## 4. Native framing

> As of 2026-11-21 the native wire is spine-wire v1, see 00-wire.md (normative, sections 2 to 14; the earlier wire is its Appendix A); the following describes the earlier wire.
>
> Correction: the native wire has no morphology, padding, speculative frames, Chameleon latent encoding or pre-shared-key AES-GCM any more; confidentiality on it comes from TLS only. The mesh handshake in sections 2 and 3 is a different protocol and is unchanged.

There are three layers of "framing" in the repository. The first two are used by the native server; the third is not.

### 4.1 Layers

| Layer | Header | Used by | Cite |
| --- | --- | --- | --- |
| A. Native frame | 5 to 16 bytes, shape chosen by a "morphology" state (section 4.3) | `ProtocolHandler` and `SpineConnection` in `spine-protocol`; the `spine-core` TCP, WS and QUIC listeners | `lib.rs:1893-1956`, `2646-2695` |
| B. Wire body | 8-byte `SP` header, then CBOR (or JSON, or CBOR+zstd) | The inside of layer A | `wire.rs:15-39, 142-195` |
| C. Transport `Frame` | 12-byte header | `FrameCodec`, `TcpConnection`, `WebSocketBridge::send_frame`, benches and tests | `spine-transport/src/lib.rs:346-383, 458-466` |

`spine-core` does not use layer C. It wraps TCP, WebSocket and QUIC streams as byte streams (`spine_transport::WebSocketStream`, `QuicStream`) and hands them to `ProtocolHandler` (`main.rs:2204-2205, 2339-2358, 2399-2415`). Layer C is exercised by `spine-transport` tests and benches (`tests/e2e_websocket.rs`, `tests/proptest_transport.rs`).

### 4.2 Layer B: wire body header (`wire.rs`)

```
offset  size  field
0       1     'S' (0x53)
1       1     'P' (0x50)
2       1     version     (WIRE_VERSION = 1)
3       1     format      0x01 JSON, 0x02 CBOR, 0x03 CBOR+zstd
4       4     payload_len (u32 big-endian)
8       N     payload
```

Behaviour of `decode` (`wire.rs:261-296`):

1. Reject if `buf.len() > MAX_MESSAGE_BYTES + 8` (16 MiB + 8).
2. If the first two bytes are not `SP`, treat the whole buffer as a headerless JSON `Message` (legacy v1.3.x fallback), after the same size check.
3. If the version byte is not 1, return `UnsupportedVersion(v)`. There is no negotiation and no list of supported versions.
4. Require `payload.len() == payload_len`, else `LengthMismatch`.
5. Dispatch on `format`; an unknown value is `UnknownFormat`. A peer can therefore always force the JSON path, either with format `0x01` or by omitting the header.
6. For `0x03`, decompress with a bounded decoder: output limit `MAX_MESSAGE_BYTES`, `window_log_max(23)` (`wire.rs:68-78`).

`encode` always emits `0x02`. `encode_compressed` emits `0x03` only when the CBOR is at least 128 bytes and the compressed form is smaller (`wire.rs:207-235, 96-105`). zstd level 3.

Body serialisation: serde through `ciborium` (CBOR, RFC 8949) for `Message`. `Message` is an externally tagged serde enum; the type tag on the wire is the variant name as a CBOR text string, not a number (`lib.rs:798-854`, no `#[serde(tag)]`). There is no numeric type registry.

### 4.3 Layer A: native frame

Written by `write_morphed_frame` and `write_frame`; read by `read_morphed_frame` and `read_frame`:

```
offset  size                       field
0       4                          length (u32), byte order chosen by morphology.big_endian
4       1                          frame_version byte (morphology.frame_version), not validated on read
5       header_size - 5            filler, every byte = morphology.checksum_variant (0..3), not validated on read
H       length                     body
        H = morphology.header_size, 5..=16
```

(`lib.rs:1893-1923, 1927-1956, 2646-2695, 697-714`.)

The morphology starts from `ProtocolMorphology::new(0)`: `frame_version` 0, `header_size` 5, big-endian, `checksum_variant` 0, padding `None` (`lib.rs:739-754, 1378`). So an unconfigured native connection uses a 5-byte header: u32 big-endian length and a zero byte. With `enable_chameleon*`, the morphology is re-seeded from a 32-byte secret and `evolve` is called after every message with an HMAC-SHA-256 PRF keyed by that secret, changing `header_size`, endianness, padding mode and version (`lib.rs:766-791`; used in `send_message` at 1636-1639 and `recv` at 2507-2513). Both ends must apply the same evolution in the same order.

Size limits: the length is checked against `MAX_TRANSPORT_FRAME_BYTES` = 16 MiB before allocation on read, and on write (`wire.rs:64`; `lib.rs:1894-1897, 1941-1949, 2651-2654, 2688-2691`). The exception is the QUIC branch of `SpineConnection::recv`, which reads a u32 little-endian length and allocates `vec![0u8; len]` with no limit check (`lib.rs:2494-2500`). See O11.

Body of a native frame, `ProtocolHandler::send_message` (the path `spine-core` uses, `main.rs:2418-2430`) in order (`lib.rs:1514-1641`):

1. `wire::encode(msg)` gives an `SP` body (layer B).
2. Wrap in a `SpeculativeFrame` and serialise as JSON, with the layer-B bytes as a JSON array of numbers when the payload is `Full`. Fields: `payload`, `next_prediction_hash`, `confidence`, `speculation_depth`.
3. Apply padding per morphology (`None`, `Pkcs7`, `Random`, `Chaotic`; `lib.rs:1959-2003`).
4. Compression flag byte: `0x01` followed by zstd level 3 if the data is at least 64 bytes, else `0x00` followed by raw data (`lib.rs:1577-1588`).
5. Optional Chameleon latent encoding, optional AES-GCM (section 4.7).
6. Write the layer-A frame.

`receive_message` reverses this and falls back to treating the data as a bare layer-B body when it does not parse as a `SpeculativeFrame` (`lib.rs:1760-1866`). Decompression is bounded to `MAX_DECODED_FRAME_BYTES` = 64 MiB with a window limit (`lib.rs:1751-1752`, `wire.rs:62`).

`SpineConnection` uses a second implementation, `ProtocolHandlerState::encode_message`, that does not apply the `SpeculativeFrame` wrapper (`lib.rs:2543-2644`). A `SpineConnection` receiver therefore cannot read frames produced by `ProtocolHandler::send_message`, because `decode_message` calls `wire::decode` directly on the JSON wrapper. The reverse direction works through the legacy fallback. There are two non-identical codecs for layer A in one crate.

### 4.4 Message type registry

`Message` has 24 variants (`lib.rs:798-854`), identified by variant name:

| Group | Variants |
| --- | --- |
| Core request path | `Request`, `Response`, `Event`, `BinaryProgram`, `LatentMessage`, `Sync`, `Speculative`, `PreComputed`, `MorphRequest`, `Ping`, `Pong` |
| Agentic | `ToolCall`, `ToolResult`, `StreamStart`, `StreamToken`, `StreamEnd`, `StreamCancel`, `CapabilityQuery`, `CapabilityAd` |
| Neural codec | `Encoded`, `CodecAd`, `CodecNegotiation`, `EmbeddingRequest`, `EmbeddingResponse` |

Name-resolution requests travel inside `Request` commands (`main.rs:424-505`), not as their own variants.

The `spine-core` session loop handles `CapabilityQuery`, `ToolCall`, `Ping` and the `Request` commands (`main.rs:2418-2492`). Searched `src/` for `StreamCancel` outside `spine-protocol`; no handler. The stream messages exist as types and round-trip tests (`tests/wire_encoding.rs:130`) but no server in the repository acts on them.

### 4.5 Layer C: transport `Frame` (12 bytes, not used by the native server)

```
offset  size  field
0       4     length    u32 LITTLE-endian
4       1     flags     bit0 COMPRESSED, bit1 ENCRYPTED, bit2 BATCHED, bit3 ACK_REQUIRED,
                        bit4 CONTROL, bit5 FIN, bit6 ZERO_COPY, bit7 PRIORITY
5       3     sequence  u24 little-endian (the in-memory field is u32; the top byte is dropped on encode)
8       2     stream_id u16 little-endian
10      2     reserved  u16, written and read back, never checked
12      N     payload
```

(`spine-transport/src/lib.rs:346-383, 458-466, 529-540`.) Facts:

- No magic and no version field. An unsupported-version outcome does not exist at this layer.
- `parse_header` uses `from_bits_truncate`, so unknown flag bits are silently ignored on the wire path (`lib.rs:535`); the serde path rejects them (`lib.rs:394-403`).
- `COMPRESSED` payloads are zstd or LZ4 (size-prepended). `Frame::decompress` calls `zstd::decode_all` and `lz4_flex::decompress_size_prepended` with no output limit (`lib.rs:441-455`). See O12.
- `ENCRYPTED` is a flag only. Searched `src/spine-transport/src` for code that sets it other than a test tagger (`plugin.rs:721`); there is no encryption behind it.
- Size limits: `FrameCodec::new(max_frame_size)` is caller-set (`frame.rs:28`); `TcpConnection` and the QUIC backend hard-code 16 MiB (`connection.rs:214, 361`); the WebSocket server bridge sets 16 MiB for message and frame (`websocket.rs:238-239`). `WebSocketBridge::recv_frame` takes everything after the 12 header bytes as payload and does not compare it with `header.length` (`websocket.rs:168-179`).
- `FrameCodec::write_frame` does a single `write_vectored` and discards the count (`frame.rs:161-174`); `write_frames` has a comment that partial writes are not tracked (`frame.rs:210`). Callers on layer C should not assume complete writes.

### 4.6 Mesh framing

Described in sections 2.4 and 2.9. For plaintext mesh connections, a mesh frame is `u32 BE length ‖ JSON MeshEnvelope`, length 1..=4 MiB (`mesh_tcp.rs:160-217`). For encrypted connections, the body is the sealed record. The QUIC mesh transport uses the plaintext envelope framing on each stream with no SPINE encryption (`mesh_quic.rs:250-251, 475`).

### 4.7 Native encryption in `ProtocolHandler`

| Mode | Construction | Cite |
| --- | --- | --- |
| `enable_encryption(key)` | AES-256-GCM, key supplied by the caller; per-message random 96-bit nonce from `OsRng`, prepended to the ciphertext; no AAD; no counter | `lib.rs:77-81, 1396-1399, 1620-1631, 1689-1700, 2583-2592` |
| `enable_chameleon(secret)` | A latent-vector encoding of the body (`ChameleonKey`), with key evolution. Obfuscation, not authenticated encryption. | `lib.rs:1402-1409` |
| `enable_chameleon_aead(secret)` | HKDF-SHA-256 (salt none, info `spine-latent-aead-key`) derives an AES key; the latent bytes are then AES-GCM sealed with a random nonce | `lib.rs:1417-1436, 1597-1611` |

All three take a 32-byte secret from the caller. There is no key exchange. Searched `src/` for callers of `enable_encryption`, `enable_chameleon`, `enable_chameleon_aead` outside `spine-protocol`'s own tests and examples: none. `spine-core` constructs `ProtocolHandler::new(stream)` and never enables any of them (`main.rs:2204, 2348, 2352, 2406`). In a production deployment the native wire is therefore plaintext unless TLS is enabled.

## 5. Streams

### 5.1 What exists on the native wire

Stream-shaped messages are `StreamStart`, `StreamToken`, `StreamEnd`, `StreamCancel` (`src/spine-protocol/src/agentic.rs`). They identify a stream by a string `id` shared by all frames of that stream. `StreamStart.id` "must be unique within a single transport connection lifetime" (`agentic.rs:104-107`). `StreamCancel` carries the id and an optional free-text `reason` (`agentic.rs:158-164`); the doc comment says the producer should stop and send a final `StreamEnd` with reason `Cancelled`. There is no numeric reason code. Multiplexing at this level is by interleaving messages with different `id` on one ordered byte stream; there is no per-stream flow control on the native wire and no handler in `spine-core` (section 4.4).

### 5.2 `spine-stream` crate

`spine-stream` implements a multiplexer, flow controller, priority queues and chunked transfer. It is not connected to any wire in this repository: the only dependent is `spine-web`, which re-exports it behind a feature (`src/spine-web/Cargo.toml:30,68`, `src/spine-web/src/lib.rs:84`). It has about 35 unit tests (`#[test]` count over `src/spine-stream/src/*.rs`). Its message type, `StreamMessage`, is serde-serialised with `Uuid` ids and a u64 sequence (`lib.rs:237-252`) and has no defined byte format of its own.

| Aspect | As implemented | Cite |
| --- | --- | --- |
| Stream identifiers | `u32`, allocated by `AtomicU32::fetch_add` from 1; 0 is the control stream. No wrap handling and no parity rule for who opens. | `multiplex.rs:110, 126` |
| Open | Registers the stream, queues a `Control(Open{stream_id, config: None})` on stream 0. The receive side does not create a stream on `Open`: `handle_control` ignores it (`_ => {}`). | `multiplex.rs:121-178, 237-282` |
| Close | Removes the stream and queues `Close{stream_id, reason: None}` | `multiplex.rs:181-209` |
| Reset | `Reset{stream_id, error_code: u32}` exists as a control message; on receipt the stream is removed. Nothing in the multiplexer sends it. | `lib.rs:331-333`, `multiplex.rs:270-278` |
| Cancel with reason code | Not present as a distinct operation. `Reset.error_code` is the nearest, unused for sending. | as above |
| Flow control state per stream | `FlowState{send_window, recv_window, paused, in_flight}` initialised from `window_size` (256 KiB default). `WindowUpdate` does `send_window += increment` on a `u32` with no bound check. The send path never reads `send_window` or `paused`: the forwarder task copies from the stream channel to the outbound channel without consulting them. | `multiplex.rs:53-75, 134-139, 166-175, 239-256` |
| Separate `FlowController` | Sliding window plus AIMD congestion window, RTT estimator, token and leaky buckets. `try_acquire` and `release` are not called by the multiplexer. | `flow.rs:25-337` |
| Credits | The `WindowUpdate` message and `FlowController::free_recv` are the credit mechanism. Not enforced in `StreamMultiplexer`. | `flow.rs:253-283` |
| Priority | `StreamMessage.priority: u8`, 0 highest, 8 levels by default; `PriorityQueue`, `MultiLevelQueue`, `WeightedFairQueue`, `DeadlineQueue`; `StreamScheduler` with per-stream weights and pause. The multiplexer sends one FIFO outbound channel and does not use them. | `lib.rs:157-192`, `priority.rs`, `multiplex.rs:374-484` |
| Backpressure | Bounded `mpsc` channels of `max_pending_items` (1024 default); `send().await` blocks when full. `try_acquire` returns false when `in_flight + bytes > min(send_window, cwnd)`. | `multiplex.rs:97, 127`, `flow.rs:131-150` |
| Chunked transfer | `ChunkMeta` with transfer id, index, total, CRC32. | `lib.rs:287-303`, `chunk.rs` |
| Head-of-line blocking | One ordered underlying connection; a large message blocks later ones on the same connection. | by construction |

### 5.3 Streams on QUIC in the mesh

`mesh_quic.rs` maps one request and response exchange to one QUIC bidirectional stream, so QUIC provides stream independence, flow control, and cancellation (stream reset) that SPINE code does not itself implement. The client opens a stream, writes one envelope with the `u32` length prefix, and reads the reply on the same stream in a background task (`mesh_quic.rs:230-275`). The server spawns a task per accepted stream (`mesh_quic.rs:467-490`). `max_concurrent_bidi_streams` is set from `TransportConfig.max_streams` (100) on the native server endpoint (`lib.rs:2259`). The mesh QUIC test `concurrent_lookups_do_not_block_one_another` runs on loopback.

### 5.4 Session identity, resumption and migration

- Native `spine-core` keeps a persistent "session" map of browser sessions with leases and a watchdog that reaps detached sessions (`main.rs:92-98, 142-200, 2418-2425`). This is application state keyed by a session id, not transport resumption. Undetermined whether a client can present a session id to re-attach after a new connection: searched `src/spine-core/src/main.rs` for `resume`/`attach` in message handling and found none.
- QUIC connection migration and 0-RTT: `TransportConfig.enable_0rtt` defaults to true but is read nowhere (searched `src/` for `enable_0rtt`); no 0-RTT configuration is applied to the quinn endpoints (`lib.rs:2238-2284`). Migration is whatever quinn does by default and is untested.

## 6. Transports: tested versus compiled

Evidence used: source tests, and the log `build_completion_final_tests.log` in the repository root (shows `test ... ok` lines for the tests named below; its file modification time is 2 Oct and predates the HEAD commit of 20 Nov, so it may not reflect HEAD). Also `build_completion_hardening_quic.log` (a `cargo check` of the `quic` feature) and `docs/COMPLETION_STATUS.md:48`, which says native QUIC "compilation" passed and "no runtime or TLS-policy equivalence claimed".

| Path | Compiled | Exercised by tests | Notes |
| --- | --- | --- | --- |
| Mesh TCP, plaintext | yes | yes: framing, resolution, bootstrap, reuse, eviction, malformed frame, oversized frame (`mesh_tcp.rs:1577-2060`) | |
| Mesh TCP, encrypted | yes | yes: resolution, capability lookup, confidentiality on the wire, wrong-identity refusal, plaintext client refused, tampered record, pooling, application pin with pooled socket (`mesh_tcp.rs:2103-2400`); present in the log | Admission, deadlines and cancelled-write tests at `mesh_tcp.rs:1232-1540` |
| Mesh WebSocket, plaintext | yes | yes, loopback `ws://` (`mesh_ws.rs:402-540`) | |
| Mesh WebSocket, encrypted | yes | one test, `a_name_resolves_over_an_encrypted_websocket` (`mesh_ws.rs:417`); no wrong-pin, tamper or `wss://` test | |
| Mesh QUIC, unauthenticated | yes | yes, UDP loopback: resolution, capability, connection sharing, concurrent lookups, announcement (`mesh_quic.rs:585-760`); present in the log | |
| Mesh QUIC, authenticated | yes | one test, `a_name_resolves_over_an_authenticated_quic_connection` (`mesh_quic.rs:599`). No wrong-pin, no MITM test. | |
| Native TCP, plaintext | yes | yes (`spine-protocol/tests/e2e_protocol.rs`, `integration.rs`, `spine-core/tests/agent_web.rs`) | |
| Native TCP and WS, rustls and mTLS | yes | `configured_mutual_tls_protects_tcp_and_websocket_origins` (`spine-core/tests/agent_web.rs:617`); not found in the log | |
| Native PSK AES-GCM, Chameleon | yes | yes at protocol level (`e2e_encrypted_tcp_roundtrip`, `e2e_chameleon_tcp_send_path`, `e2e_morph_mid_session`), not wired into `spine-core` | |
| Native QUIC (`spine-protocol` `QuicTransport`, `spine-core --features quic`) | only with the feature; `spine-core`'s `quic` feature is an empty feature gate (`spine-core/Cargo.toml:60-61`) and `spine-protocol` enables `quic` by default | No test found. Searched `src/spine-protocol/tests/` and `spine-core/tests/` for QUIC. | `docs/COMPLETION_STATUS.md:48` states compile-only |
| Transport `Frame` over TCP / WebSocket | yes | WebSocket e2e (`spine-transport/tests/e2e_websocket.rs`: round trip, empty, large, flags preserved, concurrent, reconnect) and proptests of `FrameCodec` | TCP `TcpConnection`: unit test of the health monitor only (`connection.rs:615`) |
| `spine-stream` | yes | unit tests only | Not on any wire |
| io_uring, RDMA backends | Linux feature / compiled | not examined | outside layer 1 as used |

`docs/FIRST_PRINCIPLES.md` says "QUIC is compiled but not exercised". More precisely: mesh QUIC is exercised on loopback; native QUIC is compile-checked only.

## 7. Formal models

Contents of `formal/`:

| File | Tool | What it models | Relation to the deployed handshake |
| --- | --- | --- | --- |
| `formal/tamarin/SpineKeyExchange.spthy` | Tamarin | An X3DH-style exchange with Diffie-Hellman built-in, a symbolic "RLWE" KEM (`rlwe_keygen`, `rlwe_encaps`, `rlwe_decaps`, `rlwe_ss`), three levels (Standard X25519, Hardened X25519+RLWE, PostQuantum RLWE-only), a key-evolution hash chain, and compromise rules for identity, RLWE and session keys | None of the three levels is the deployed protocol. The deployed handshake uses ML-KEM-768 with Ed25519 signatures and no DH, no signed prekeys, no ratchet. |
| `formal/tla/ChameleonProtocol.tla` + `_MC.tla` | TLC | Two peers with an epoch, morphology and key-hash state, bounded channels, morph requests, decoys | Models the Chameleon moving-target state synchronisation, not the mesh handshake or the AEAD record layer |
| `formal/tla/MeshRouting.tla` + `_MC.tla` | TLC | Routing tables, TTL forwarding, deduplication, gossip over 3 nodes | Mesh routing, not channel security |
| `formal/audit/CRYPTO_AUDIT.md` | manual | A 13-finding audit of older primitives (RLWE KEM, XOR cipher, key evolution) | Predates `handshake.rs`; its P0 items concern code that is not the mesh channel |
| `formal/misra/MISRA_COMPLIANCE.md` | manual | MISRA mapping for unsafe code | Not layer 1 |

Properties stated:

- Tamarin (`SpineKeyExchange.spthy:297-399`), 7 trace lemmas and 3 `exists-trace` sanity lemmas: `session_key_secrecy_standard`, `session_key_secrecy_hardened`, `session_key_secrecy_postquantum`, `perfect_forward_secrecy` (stated over the hash-chain key evolution, not over ephemeral keys), `kci_resistance_hardened`, `key_agreement_standard`, `key_agreement_hardened`, `sanity_standard`, `sanity_hardened`, `sanity_postquantum`. The docs describe "10 lemmas"; that count includes the three sanity lemmas. The header comment lists an unknown-key-share property (`:15`) for which no lemma exists. The initiator rule `X3DH_Initiator_Standard` contains a comment that the signature check is "simplified here" (`:132-133`).
- `ChameleonProtocol.tla` (`:225-272`): `EpochMonotonicity` (as written, `epoch >= 0` for both peers), `SynchronizedEvolution` (equal state when both channels are empty), `BoundedMessages`, `WellFormedChannel`, `KeyEvolved` (the file notes it is a weak check), liveness `EventualDelivery` and `Progress`.
- `MeshRouting.tla` (`:146-188`): `TTLMonotonicity` (as written, a bound `0 <= ttl <= MaxTTL`), `DeduplicationInvariant`, `PathBounded`, `NoRoutingLoops`, `ValidRoutes`, `SelfRouteZero`, `DeliveryCorrectness`; temporal `EventualResolution`, `GossipConvergence`.

Evidence of execution: none found.

- `formal/audit/CRYPTO_AUDIT.md:223-226` lists the status of the TLA+ and Tamarin artifacts as "Ready", not as verified.
- `docs/src/internals/formal.md` says "Verified with TLC model checker" without a run record.
- `COMPLETION_PLAN.md:168` has an unchecked item to "distinguish formal models that were written ... from models executed and properties proved".
- The repository contains no TLC configuration (`.cfg`) file, no TLC or Tamarin output, no proof file (`.spthy` `.proof`), no mention of `tamarin-prover` or `tlc` in `.github/workflows/ci.yml` or `scripts/`, and no versions or dates of tool runs. The `_MC.tla` files give constants and list invariants in comments; TLC normally also needs a `.cfg` that names them, and none exists.

Conclusion: the models were written. There is no repository evidence that they were executed, and the Tamarin model does not describe the deployed construction.

## 8. Comparison with a Noise-based design

This section is analysis, not a recommendation to rewrite.

### 8.1 Closest pattern

The deployed handshake is a two-message, signature-authenticated, ephemeral-KEM exchange. It does not correspond exactly to a Noise pattern because Noise authenticates with static Diffie-Hellman keys (tokens `es`, `se`, `ss`) and mixes every token into a running chaining key and hash, and here the static keys only sign.

Mapping by message shape:

```
Deployed          Noise XX                       Noise IK
-> ek, id_I, sig  -> e                           -> e, es, s, ss
<- ct, id_R, sig  <- e, ee, s, es                <- e, ee, se
                  -> s, se
```

- By number of messages and by the option to pin the responder, it is nearest to **IK**: two messages, the initiator may know the responder key in advance, and a pinned dial fails if the responder key differs.
- By what is exposed and by the unpinned bootstrap case, it is nearest to **XX**: neither side's static key is needed in advance and both static keys are transmitted. Unlike XX, they are transmitted in clear and there is no third message.
- With the ML-KEM extension, the nearest published formulation is a Noise pattern with a KEM token pair for the ephemeral (for example the Noise "hybrid forward secrecy" extension tokens or PQNoise-style patterns). Here the KEM is used alone (no X25519 in the exchange), so it would be "ephemeral ML-KEM instead of ephemeral DH", with Ed25519 signatures carried in the handshake payload as the authentication step. Undetermined which exact published extension a reviewer would prefer; no document in the repository names one.

### 8.2 Where the deployed handshake differs

| Aspect | Deployed | Noise (XX or IK) |
| --- | --- | --- |
| Static key use | Ed25519 signs; never used for key agreement | X25519 static key in DH tokens |
| Authentication of initiator | Signature over `ek ‖ id_I` only; no binding to R or to a value from R | Bound through DH (`es`, `se`, `ss`) and the handshake hash |
| Message count | 2; no initiator confirmation | XX: 3 messages; IK: 2 with `es`/`ss` in message 1 |
| Key confirmation | None explicit; implicit at first record | Implicit via AEAD on later handshake payloads and transport |
| Identity hiding | `id_I` and `id_R` in clear | XX hides `s` after the first DH; IK hides initiator `s` in message 1 |
| Handshake hash and chaining key | None running. Only `SHA-256(msg1 ‖ msg2)` used as HKDF salt after the exchange | `h` and `ck` updated with every token and every message |
| Handshake payload | None; no space for attestations | Encrypted payloads allowed in every message after the first DH |
| Key derivation | HKDF salt = transcript hash, ikm = `ss`, two Expand labels | HKDF over `ck`, `Split()` yields two keys |
| AEAD and nonce | AES-256-GCM; nonce = 4 zero bytes ‖ u64 BE counter (the same nonce layout as Noise `AESGCM`); **AAD = counter** | Same nonce layout; transport AAD is empty |
| Rekey | None | `Rekey()` defined; counter `2^64-1` reserved |
| Message size | Records up to 4 MiB | Noise messages are limited to 65 535 bytes, so large envelopes need a chunking layer |
| Suite negotiation | Label equality only | Protocol name hashed into `h`; no negotiation within a pattern |
| Prologue | None | Available for binding context (for example a TLS or QUIC exporter) |
| Replay of first message | Possible | IK: possible without extra measures; XX: not replayable into a completed session because both sides contribute ephemerals |
| Transcript binding of the signature | `sig_R` covers `H(msg1)`; `sig_I` does not cover R | Every authenticator is bound to the full handshake hash |
| Formal analysis | None for this construction | Pattern-level published analyses exist (for the DH patterns); the KEM variants have separate analyses |

### 8.3 What would have to change to adopt the pattern

Listed for an XX-style hybrid pattern with a pinned-responder option; IK-style would drop the third message.

1. Add a symmetric state `(ck, h, k, n)` with `MixHash`, `MixKey`, `EncryptAndHash`, `Split`, and initialise `h` from a protocol name that names the KEM, signature and AEAD.
2. Choose how Ed25519 identity authenticates a Noise static key: either carry an Ed25519 signature over the static DH public key and the handshake hash in an encrypted handshake payload, or derive and certify an X25519 static key from the identity. The deployed design has no DH static key, so this is a new key type and a new binding.
3. Replace the clear-text `id_I`, `id_R`, `sig_*` fields by encrypted payloads, which changes message sizes and the 4096 cap check.
4. Carry the ML-KEM-768 `ek` and `ct` through tokens that `MixKey` their shared secrets. Check that all messages stay under 65 535 bytes (they do: 1184 and 1088 byte fields).
5. Add the initiator's third message (XX) or move the initiator static key into message 1 (IK). The unpinned bootstrap dial maps to XX; the pinned dial maps to IK. The code currently has one flow for both.
6. Decide the transport-record rules: keep the 4-byte zero prefix and 8-byte counter nonce; decide whether to keep AAD = counter or follow Noise's empty AAD (the counter is already implied by the nonce); add `Rekey` and enforce a counter limit; limit message size to 65 535 or add a chunking layer under the envelope.
7. Provide a prologue that binds the transport context. For QUIC, this is where a TLS exporter value would go (see O1).
8. Replace the Tamarin model with one of the chosen pattern, and generate known-answer vectors.
9. Change the protocol label to a new name; there is no migration path from the current label because mismatch is a refusal (section 2.10).

Not required by the pattern but required by the target design: a way to carry a capability chain in an encrypted handshake payload (section 9).

## 9. Gaps against the target design

Status values: **No** means not present; **Partial** means some of the property exists; **Yes** means present and exercised.

| Target item | Status | What exists | What is missing | Evidence |
| --- | --- | --- | --- | --- |
| Connect-to-key addressing | Partial | The handshake carries and verifies the Ed25519 identity; the dialer can supply a pin; `Application` RPC pins `recipient_key` from signed payload and re-checks pooled connections. | No dial API takes a key. Dial target is an `AgentId` UUID mapped to a socket address from peer self-advertisement. `set_identity` has no production caller, so DHT dials are unpinned. `AgentId` is not derived from the public key. | `mesh_tcp.rs:786-797, 842-930`; `naming_mesh.rs:461-467`; `identity.rs:148, 167` |
| Stream cancel with reason code | No | `StreamCancel` with optional free-text `reason`; `Reset{error_code}` in `spine-stream` (receive side only). QUIC provides stream reset in the mesh QUIC transport. | No numeric reason registry, no handler in any server, no cancel on mesh channel. | `agentic.rs:158-164`; `multiplex.rs:270-278` |
| Session resumption | No | Application-level browser session leases in `spine-core`. | No resumption ticket, no PSK or 0-RTT path in the handshake, QUIC 0-RTT flag unused. | `main.rs:92-200`; `lib.rs:2101, 2116` |
| Connection migration | No | Whatever quinn does by default on QUIC. | The mesh channel is tied to one TCP connection and its counters; no connection identifier independent of the address tuple; untested. | section 5.4 |
| Capability chain carried in frames | No | `ApplicationMessage` carries the sender key, recipient and recipient key, signed. Worker allowlists apply in `spine-core` tasks. | No chain, no delegation, no capability field in any frame header or handshake payload. `FIRST_PRINCIPLES.md:41` says "Absent". | `mesh.rs:235-244`; `namespace.rs:123-140` |
| Formal model executed | No | Models exist for a different construction. | No run record; no model of `spine-mesh-hs-v1`; no known-answer vectors; no fuzz target. | section 7 |
| Standard construction reviewed | No | Independent review is listed as a release blocker. | `HANDSHAKE-REVIEW.md:191-195`, `CRYPTO_REVIEW_PACKET.md:23-30` | |
| Credit-based flow control wired to a wire | No | `WindowUpdate`, `FlowController`, queues exist in `spine-stream`. | The multiplexer does not read the windows and no transport uses the crate. | section 5.2 |
| Channel security on the native client wire | Partial | rustls TLS and mTLS, off by default; mTLS test exists. | No SPINE identity binding, no PSK or key exchange wired in, native QUIC unauthenticated and refused with TLS configured. | sections 3, 4.7 |

## 10. Observations for the reviewer

These describe classes of problem and their locations. They are not exploit descriptions. Each is "as observed in the code"; none has been tested by running code.

**O1. Mesh QUIC: handshake is not bound to the QUIC TLS session, and its keys are discarded.** `mesh_quic.rs:213-224, 308-315` run the handshake on the first stream and drop the resulting `Session`; `mesh_quic.rs:451-465` does the same on the server and then serves later streams with `ReplyPath::Plain` (`mesh_quic.rs:477`), so the authenticated identity is not recorded or consulted for later streams. The QUIC client accepts any certificate (`spine-protocol/src/lib.rs:2266-2284, 2314-2342`) and the server requests none (`lib.rs:2246`). Class: missing channel binding. A party that terminates TLS toward each side can relay the identity handshake unchanged while holding the plaintext of the TLS-protected streams. Envelope signatures still protect record integrity; the confidentiality purpose stated in `handshake.rs:1-8` is not met on this path. The module comment describes the discard as intended (`mesh_quic.rs:34-38`).

> Update 2026-11-21: the permissive QUIC client verifier is gone (`SkipServerVerification` was removed). `QuicEndpointBuilder::build_client` now verifies against the web PKI roots, and `build_client_with_roots`, `build_client_trusting` and `client_config_with_roots` verify against a given set; the mesh QUIC client (`QuicNameTransport`) trusts nothing until `trust_certificate` is called, and `serve_quic_node` returns the listener certificate to distribute. The missing channel binding and the discarded session in the paragraph above are unchanged: the handshake is still not bound to TLS and `ReplyPath::Plain` still carries no peer key.

**O2. Identity pins are not populated in the deployed path, and the dial identifier is not a key.** `set_identity` is the only writer of the pin map (`mesh_tcp.rs:786-797`) and has no non-test caller. In `spine-core` every non-`Application` dial is unpinned (`mesh_tcp.rs:888, 913-915`). The pool and address book are keyed by `AgentId`. `naming_mesh.rs:461-467` and the `NameHelloAck` branch (`naming_mesh.rs:1011-1019`) call `transport.learn(agent_id, endpoints)` with the `AgentId` from the unauthenticated-for-this-purpose `envelope.from` and endpoints from the peer's own message; `TcpNameTransport::learn` records the first parseable address (`mesh_tcp.rs:1059-1063`). `AgentId` is not derived from the public key (`identity.rs:148, 167`). Class: identifier not bound to the authenticating key; address-book poisoning would redirect DHT traffic to an unpinned peer. `HANDSHAKE-REVIEW.md:138-142` says unpinned dialing is used "exactly once"; in the deployed wiring it is the default for all non-application traffic. Application RPC is protected by `recipient_key` (`mesh_tcp.rs:919-930`).

> Update 2026-11-21: `envelope.from` is no longer trusted for decisions that depend on who sent an envelope. A `NameResolveResponse` is absorbed only if its signature verifies under a key recorded for `from` and, on an authenticated connection, under the session key; a `NameHello` or `NameHelloAck` must carry the session key (`naming_mesh.rs`, `handle_envelope_from_peer`, `authenticate_sender`). The unauthenticated `learn` calls, the unpinned DHT dials and the fact that `AgentId` is not derived from the key are unchanged; the trust table is in `src/spine-mesh/README.md`.

**O3. No key confirmation; initiator signature is not bound to the responder and gives no liveness.** `sig_I` covers `LABEL_INITIATOR ‖ ek ‖ id_I` only (`handshake.rs:215-219`). The responder derives keys, attaches a session and holds an admission slot before receiving any evidence that the initiator holds `dk` (`handshake.rs:393-394`, `mesh_tcp.rs:1143-1151`). A replayed `msg1` is accepted (acknowledged at `HANDSHAKE-REVIEW.md:119-125`). Class: missing key confirmation and liveness; work amplification bounded by `max_connections` and the handshake and frame timeouts. A third message or a responder-supplied value would close it.

**O4. Identities are sent in clear in both handshake messages.** `handshake.rs:223-224, 385-386`. The stated purpose is hiding which names an agent resolves, but a passive observer learns both mesh identities and which pairs communicate. Class: identity privacy.

**O5. No counter exhaustion guard and no rekey.** `self.tx_counter += 1` on a `u64` with no check (`handshake.rs:477-478`); `seal` cannot return an error and uses `expect` (`handshake.rs:490`). The nonce is the counter, so a wrapped counter would reuse a nonce under the same key. The state is unreachable in practice at 2^64 records, but the code does not enforce it, and a debug build would panic instead of wrapping. There is no per-key record or byte limit and no key update.

**O6. Signature verification uses non-strict Ed25519.** `check_signature` calls `Verifier::verify`, and `verifying_key` uses `VerifyingKey::from_bytes` (`handshake.rs:158-167`). `ed25519-dalek` offers `verify_strict`, which rejects small-order components and non-canonical encodings; the non-strict form does not. Class: malleability and weak-key acceptance. Pinned dials are unaffected because the pin compares bytes; unpinned dials and all responder-side verification accept any key that verifies.

**O7. Documentation understates the record layer.** `HANDSHAKE-REVIEW.md:144-145` says AAD is empty. The code authenticates the 8-byte counter as AAD on both seal and open (`handshake.rs:487, 517`) and the source comment says so (`handshake.rs:473-475`). A reviewer should use the code. The review document also says the pin is set "exactly once" (see O2) and lists 25 tests, which matches.

**O8. Stack copies of secrets are not wiped.** `shared` (`handshake.rs:292, 389`) and the intermediate `i2r` and `r2i` arrays (`handshake.rs:443-446`) are plain stack arrays copied into `Session`; only `Session` and `Initiator.dk_bytes` are zeroised. `finish` rebuilds a `DecapsulationKey` from `dk_bytes` (`handshake.rs:282-285`). Undetermined whether `ml-kem` 0.2.3 zeroises `DecapsulationKey` and the shared secret on drop; the crate source was not read.

**O9. Native `ProtocolHandler` encryption has no replay or order protection and no key exchange.** Per-message random 96-bit nonces, no AAD and no counter (`lib.rs:77-81, 1620-1631, 2583-2592`). Plain AES mode accepts replays and reordering; random nonces under one long-lived key are limited to about 2^32 messages by the usual bound, which the code does not track. The header, including the version byte and filler, is outside the AEAD and is not checked on read (`lib.rs:1927-1956`). Compression happens before encryption, which the source notes as a known leak (`lib.rs:1651-1654`). None of this is on a production path today (section 4.7); it matters if it is enabled.

**O10. The signed envelope omits routing fields and uses non-canonical JSON.** *Resolved as of 2026-11-21: signature v3 covers the domain string, `id`, `from`, `to`, the timestamp and the payload as one deterministic CBOR array (`ttl` and `hops` stay unsigned by design). The original finding follows.* `signature_data` signs `id ‖ from ‖ serde_json(payload)` and not `to`, `ttl`, `hops` or `timestamp` (`mesh.rs:916-924`), though the field comment says the signature covers `to` (`mesh.rs:163`). When `serde_json::to_vec(payload)` fails, the `if let Ok` skips the payload and the signature covers the first two fields only (`mesh.rs:920-922`). JSON serialisation of `serde_json::Value` map ordering depends on crate features. This is above layer 1 but determines what the channel's plaintext authenticates. Application RPC copes by signing `recipient` and `recipient_key` inside the payload (`mesh.rs:238-241`).

**O11. Unbounded allocation on the native QUIC receive path.** `SpineConnection::recv` over QUIC reads a u32 little-endian length and calls `vec![0u8; len]` without comparing with `MAX_TRANSPORT_FRAME_BYTES` (`lib.rs:2494-2500`). The TCP branches enforce 16 MiB (`lib.rs:2688-2691`). Class: peer-controlled allocation size.

**O12. Decompression without output bound in the transport `Frame` layer.** `Frame::decompress` uses `zstd::decode_all` and LZ4 size-prepended decode with no limit (`spine-transport/src/lib.rs:441-455`). The native wire layer uses a bounded decoder (`wire.rs:68-78`). Layer C is not used by the native server today.

**O13. Peer-controlled arithmetic in the stream crate.** `send_window += increment` on a `u32` from a received `WindowUpdate` (`multiplex.rs:244`) can overflow; in a release build it wraps, in a debug build it panics. The window is not used to limit sending (section 5.2), so the practical effect is a panic in debug builds.

**O14. Pinned changes do not evict WS and QUIC pooled connections.** `WsNameTransport::set_identity` and `QuicNameTransport::set_identity` only insert (`mesh_ws.rs:102-104`, `mesh_quic.rs:160-162`), whereas TCP closes the pooled connection on a changed pin (`mesh_tcp.rs:794-796`). These transports are not used in production wiring.

## 11. Undetermined

| Item | File or files searched | Reason |
| --- | --- | --- |
| Whether any code re-dials or recycles a mesh connection on a schedule, which would be the only rekey | `research/spine-agentic/src/mesh_tcp.rs`, `src/spine-core/src/namespace.rs` | Found only idle and frame timeouts and eviction on address or pin change |
| Whether a native client can re-attach to a detached session id after reconnecting | `src/spine-core/src/main.rs` | Session leases exist; no resume message found |
| Whether `ml-kem` 0.2.3 wipes `DecapsulationKey` and shared secrets on drop | not read (external crate) | Source not in the repository |
| Which published Noise hybrid or KEM extension to target | none | No document in the repository names one |
| Whether the tests listed in section 6 pass at HEAD | `build_completion_final_tests.log` | The log is older than HEAD; cargo was not run for this document |
| Whether `load_or_create_key` sets restrictive file permissions on the identity seed | `src/spine-core/src/namespace.rs` (lines 287-326 not read in detail) | `CRYPTO_REVIEW_PACKET.md:17` lists key storage as a review item |
| Whether mesh `from` (an `AgentId`) is checked against the authenticated session for non-application envelopes in any handler | `research/spine-agentic/src/naming_mesh.rs`, `mesh_tcp.rs:1173-1186` | Only the `Application` branch compares sender key and authenticated peer |
