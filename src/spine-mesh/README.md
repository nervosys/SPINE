# spine-mesh

The SPINE agent mesh: Ed25519 identities, signed envelopes, signed name records, and authenticated TCP/WebSocket transports

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

The QUIC mesh transport and the signed-frame codec live here too. They use
`spine-protocol`, which has no research dependency, so `spine-neural` and
`spine-crypto` are not in this crate's dependency tree. `spine-agentic`
re-exports this crate's modules and `AgentId`.

## Documentation

API documentation: <https://docs.rs/spine-mesh>

## Workspace dependencies

- `spine-name`
- `spine-protocol` (default features off, `quic` on)
- `spine-session`
- `spine-transport`

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).

## Trust model

Written 2026-11-21 against the code in this crate. Every claim names the function that enforces it; where a property does not hold it says so.

### What authenticates a sender

An envelope carries `from`, an `AgentId`. `AgentId` is a UUID, but for every identity that signs mesh envelopes it is a public function of the key: `AgentId::from_public_key` = first 16 bytes of SHA-256(`"spine-agent-id-v1\0"` || Ed25519 public key), set by `SigningIdentity::new` and `from_seed` (updated 2026-11-21). Every handler that learns or uses an (id, key) pair from the network requires `id == AgentId::from_public_key(key)` and drops the pair otherwise, so `from` cannot be attached to a key it does not derive from. The derivation says which key an id names; it does not say the sender holds that key. Nothing about an envelope is trustworthy until its signature has been verified under that key, and a key is bound to an agent id by the pairing this node recorded itself (`MeshNameResolver::register_peer`, or a `NameHello`/`NameHelloAck` handled in `naming_mesh.rs`), each of which now passes the id check first.

| Entry point | What authenticates the connection | `peer` passed to `handle_envelope_from_peer` |
| --- | --- | --- |
| TCP, encrypted (`Security::Encrypted`) | `spine-session` handshake: the peer proved an Ed25519 key (`ReplyPath::authenticated_peer`). Pinned to the key the dialer expects, see "Identity pinning" below | `Some(session key)` |
| TCP, plaintext | nothing | `None` |
| WebSocket, encrypted / plaintext | as TCP, through the same `pump` | `Some(session key)` / `None` |
| QUIC, `authenticated` | TLS 1.3, with the server certificate verified against the set given to `QuicNameTransport::trust_certificate` (empty by default), plus a mesh handshake on the first stream. The key it proves is kept for the whole connection and carried by every later stream's reply path (`SocketTransport::from_authenticated_writer`); a connection that fails the handshake is closed and none of its streams is read. Not bound to the TLS channel (see below). Pinned like TCP | `Some(handshake key)` |
| QUIC, unauthenticated (`QuicNameTransport::new`) | TLS 1.3 and the certificate check only; nothing says which mesh node answered | `None` |
| In-process (`handle_envelope`, `handle_envelope_with_reply`; tests and embedders) | whatever the embedder did | `None` |

### Per payload variant

"Binding" below means `MeshNameResolver::authenticate_sender`: the envelope signature must verify under a key recorded for `envelope.from`, and, when the connection has an authenticated peer key, that key must be the signer.

| `MeshPayload` variant | What the handler requires of the sender | What it then trusts from the envelope |
| --- | --- | --- |
| `NameAnnounce` | Nothing about the envelope. The record must verify under its own key (`accept_announcement`), and is kept only if this node is near its key | Nothing: `from` is never read, so no admission, rate or reputation decision depends on who relayed it. Relaying a signed record is legitimate; records are self-certifying. The optional `log` (the name's key succession log, absent for a name without one) is self-certifying too: it is read only for an announcement this node is responsible for, only if it is for the record's own name, is refused before any signature check if it has more than 64 entries or a signature that is not 64 bytes (`succession_logs_oversize`), and is then verified from the name's inception key. The record is admitted against the best log held: a superseded key, a retired or compromised name and a head claim beyond the log held are refused and counted (`records_refused_by_succession`, `records_need_log`). A valid log that conflicts with the one held fails the name closed (`equivocations_found`) |
| `NameResolveRequest` | Nothing. The answer is public data | `from` only addresses the reply, and only where no connection exists to answer on (in-process); on a connection the answer goes back on it |
| `NameResolveResponse` | Binding (which now also requires `from` to derive from the signing key), and the bound key must be one the lookup asked (`NameService::on_response_from_keys`). Otherwise dropped and counted in `ResolverMetrics::envelopes_unauthenticated` | Records and providers inside are verified individually, against the key succession log held for their name when there is one. The optional `logs` are taken only from a response that will be counted (bound, asked, not yet answered), at most K + 1 of them, each bounded like an announced log and each for a name the response is about (the queried name, the record's, a provider's); the rest are dropped and counted. Each is verified from its inception key, so the sender is trusted for nothing about it. `closer` referrals are unsigned, so they are only learned from a bound, asked peer, as referrals that never displace a known contact, and only entries whose id derives from their key (others counted in `agent_id_mismatches`) |
| `NameHello` | The signature verifies under the key the hello carries, and on an authenticated connection that key is the session key. Otherwise dropped and counted in `greetings_rejected` | The signature verifies under the carried key and `from` must be that key's derived id; a hello whose id is not derived is dropped and counted in `greetings_rejected` and `agent_id_mismatches`. The pairing is then recorded, and both halves are proven (updated 2026-11-21) |
| `NameHelloAck` | As `NameHello` | As `NameHello`, plus: it completes a pending bootstrap dial by the endpoints it claims, so a bootstrap seed is trusted on first use unless pinned. Its `closer` list is unsigned and bounded, and an entry whose agent id is not derived from the key it names is dropped and counted in `agent_id_mismatches` (`derived_referrals`); the rest are claims about which address a key lives at, not proofs |
| `Application` | The signature verifies under `message.public_key`; `pump` additionally requires the session key to equal it, so plaintext connections cannot carry it. `from` must be the id derived from `message.public_key`, else the envelope is dropped and counted in `envelopes_unauthenticated` and `agent_id_mismatches`, whether or not there is a connection to answer on. A request answered without a connection also requires binding of `from` to that key. A response is matched to a waiter by (key, `from`) recorded when the call was made. Stale or repeated request ids are refused (`ReplayGuard`, keyed by the verified signing key, at most 256 live entries per key out of 4096) | The caller is the signing key (`MeshApplication::handle` receives the key, never `from`). `from` is used only to route the reply |
| `AgentMessage`, `PeerAnnounce`, `PeerQuery`, `Ping`, `Pong`, `RouteRequest`, `RouteResponse`, `KnowledgeSync`, `SwarmInvite`, `SwarmResponse` | No handler exists. `handle_envelope*` returns `false` and `pump` discards them | Nothing. `MeshNode::process_envelope`, which used to deliver and forward them and verified a signature only for senders already in a trust map (unknown senders passed through unchecked), had no caller and was removed. An embedder that handles these variants must verify them itself with `MeshNode::verify_carried_key` against a key it has bound to `from` |

### What holds

- A name record cannot be forged or altered in transit: it verifies under its own key whatever carried it.
- A `NameResolveResponse` is absorbed only if it was signed by a key recorded for the agent id it claims, and, on an authenticated connection, delivered by that key's own session. A forged or relayed response, signed or not, is dropped and counted.
- A `NameHello` or `NameHelloAck` on an authenticated connection carries the key of that connection.
- An application caller is the key that signed the request and, on TCP, WebSocket and authenticated QUIC, owns the connection it arrived on. Requests are fresh and unique within a 120 second window, and one key cannot fill the replay table for the others.
- An agent id learned from a hello, ack, referral or registration is the derivation of the key it is paired with, in the routing table, in the lookup and in each transport's pin table (`learn_peer`, `PinBook::admit_pair`; counted per resolver in `agent_id_mismatches()` and per transport in `id_mismatches()`). A dial to an agent id whose peer proves a key that does not derive it fails (`PinBook::pin_proved`).
- A dial that expects a key does not complete with another one (updated 2026-11-21, "Identity pinning" below), on every transport that runs the mesh handshake.

### Identity pinning

Updated 2026-11-21. A node's keyspace id is its Ed25519 public key, so a name that identifies a node also names the key it must prove. Where the dialer holds a key, it passes it as `expected_peer`, and a responder that proves another key fails the dial with `NameMeshError::PinMismatch` (logged, and counted in `pin_mismatches()`); there is no fallback to an unpinned connection.

| Dial | Expects | Recorded in |
| --- | --- | --- |
| Pooled dial to an agent id (DHT referral, re-dial), TCP, WebSocket, QUIC | The key named by the hello, ack or referral that introduced the peer (`NameTransport::learn_peer`; first claim stands), or `set_identity`; for a peer nothing named, the first key it proved | the transport's per-agent identity map, in memory |
| Application RPC | The signed `recipient_key`, whatever else is recorded | not recorded |
| Bootstrap dial to a seed address | The key configured for it (`namespace.seed_keys`, `MeshNameResolver::pin_seed`), else the key recorded at first contact in this process | `PinBook`, per endpoint, in memory |
| Plaintext transports | Nothing (no handshake); `pin_seed` is refused there | none |

Pins are not persisted: no store handle is available at the transport, so persisting them would need new plumbing. A restart makes a seed without a configured key trust on first use again.

### What does not hold

- The id check does not authenticate anyone. A referral is still an unsigned claim about *where* a key can be reached (its endpoints) and, for a key the referrer did not make, a claim the key exists; the id check removes only the freedom to pair a key with a label it does not derive. Authentication is the signature and the session key; see "Agent ids are derived from the key" below.
- Only identities that go through `SigningIdentity::new` or `from_seed` are self-certifying. `SigningIdentity::from_agent_id` builds an identity whose id is not derived from its key, and every hardened handler refuses it; it exists to build impostors in tests. `AgentId::new()` (random) remains the id of keyless agents outside the mesh and is never accepted for a key.
- First contact with a bare seed address that has no configured key is trust on first use, once per process: the key that answers is recorded and required afterwards, but the first answerer is accepted, and the ack is matched to the dial by the endpoints it claims. Recorded pins are in memory and are forgotten on restart.
- The QUIC mesh handshake is not bound to the TLS channel: `spine-session`'s handshake has no input for channel context and is not modified here. A party that can terminate the TLS connection with a certificate the client trusts can relay a genuine peer's handshake and act as a man in the middle of the connection. The client's protection is trusting only the certificates it was given and pinning the expected mesh key; QUIC streams are not sealed with the mesh session keys, so TLS alone provides their confidentiality.
- A pin learned from a referral is the referrer's claim. The first claim for an agent id stands and a later referral does not replace it. Since an id has exactly one derived key, a referral cannot pre-claim a different key for an id (updated 2026-11-21); a malicious referrer can still name a real key at an address that does not hold it, which makes dials fail rather than reach an impostor.
- The pin and address tables are bounded (1024 entries each); once full, new peers are not pinned from referrals and new first-contact seed keys are not recorded.
- Only `Application` requests have a freshness window. A recorded, genuine `NameResolveResponse` can be replayed to the node it answered while that lookup is open, on a connection that is not session-authenticated (plaintext, unauthenticated QUIC, in-process); lookups use sequential request ids.
- `ttl` and `hops` are not signed and are advisory. No transport entry point forwards envelopes, so nothing here routes on them.

### Agent ids are derived from the key

Updated 2026-11-21. `AgentId::from_public_key(key)` is the first 16 bytes of SHA-256(`"spine-agent-id-v1\0"` || key). `SigningIdentity::new`, `from_seed` (and so the node key file's seed in `spine-core`) set it; the golden test `agent_id_derivation_is_stable` pins one value. 128 bits give a 2^64 collision bound between keys, which is acceptable because the id is a routing label and authentication is always against the full 32-byte key.

| Property | What the id check gives | What still relies on signatures or the session |
| --- | --- | --- |
| `envelope.from` | It names exactly one key. A hello, ack or `Application` envelope whose `from` does not derive from the key it carries or is signed by is dropped | That the sender holds the key: the envelope signature, and on an authenticated connection the session key |
| Routing table, lookup, pin tables | A referral, hello, ack or registration cannot pair a key with an id it does not derive (`adopt_peer`, `register_peer`, `derived_referrals`, `learn_peer`, `PinBook::admit_pair`); the pin for an id is the key it derives | That a referred key is reachable at the endpoints claimed: unsigned, learned only as a hint, and verified by the dial's handshake |
| `authenticate_sender` | Keys recorded for an id are filtered by the derivation, so a recorded pair that does not derive is never used | The signature over the envelope under that key |
| Application caller | The id in `from` is the signer's, so a reply addressed by id reaches the signer | The caller is the signing key; `MeshApplication::handle` still receives the key, never `from` |
| Name records | Not involved | Records verify under their own key |

Persisted state: the node key file holds the 32-byte seed only, so the derived id of an existing node changes on upgrade (breaking, see the CHANGELOG). No store in this crate persists an id next to a key.
