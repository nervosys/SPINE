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

An envelope carries `from`, an `AgentId`. That is a label, not a key: `AgentId` is a UUID (random for `SigningIdentity::new`, a hash of the seed for `from_seed`), never derived from the public key. Nothing about an envelope is trustworthy until it has been bound to a key, and a key is bound to an agent id only by the pairing this node recorded itself (`MeshNameResolver::register_peer`, or a `NameHello`/`NameHelloAck` handled in `naming_mesh.rs`).

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
| `NameAnnounce` | Nothing about the envelope. The record must verify under its own key (`accept_announcement`), and is kept only if this node is near its key | Nothing: `from` is never read, so no admission, rate or reputation decision depends on who relayed it. Relaying a signed record is legitimate; records are self-certifying |
| `NameResolveRequest` | Nothing. The answer is public data | `from` only addresses the reply, and only where no connection exists to answer on (in-process); on a connection the answer goes back on it |
| `NameResolveResponse` | Binding, and the bound key must be one the lookup asked (`NameService::on_response_from_keys`). Otherwise dropped and counted in `ResolverMetrics::envelopes_unauthenticated` | Records and providers inside are verified individually. `closer` referrals are unsigned, so they are only learned from a bound, asked peer, as referrals that never displace a known contact |
| `NameHello` | The signature verifies under the key the hello carries, and on an authenticated connection that key is the session key. Otherwise dropped and counted in `greetings_rejected` | The pairing (`from`, carried key) is recorded as given. The key is proven, the agent id label is the sender's claim |
| `NameHelloAck` | As `NameHello` | As `NameHello`, plus: it completes a pending bootstrap dial by the endpoints it claims, so a bootstrap seed is trusted on first use unless pinned. Its `closer` list is unsigned and bounded |
| `Application` | The signature verifies under `message.public_key`; `pump` additionally requires the session key to equal it, so plaintext connections cannot carry it. A request answered without a connection also requires binding of `from` to that key. A response is matched to a waiter by (key, `from`) recorded when the call was made. Stale or repeated request ids are refused (`ReplayGuard`, keyed by the verified signing key, at most 256 live entries per key out of 4096) | The caller is the signing key (`MeshApplication::handle` receives the key, never `from`). `from` is used only to route the reply |
| `AgentMessage`, `PeerAnnounce`, `PeerQuery`, `Ping`, `Pong`, `RouteRequest`, `RouteResponse`, `KnowledgeSync`, `SwarmInvite`, `SwarmResponse` | No handler exists. `handle_envelope*` returns `false` and `pump` discards them | Nothing. `MeshNode::process_envelope`, which used to deliver and forward them and verified a signature only for senders already in a trust map (unknown senders passed through unchecked), had no caller and was removed. An embedder that handles these variants must verify them itself with `MeshNode::verify_carried_key` against a key it has bound to `from` |

### What holds

- A name record cannot be forged or altered in transit: it verifies under its own key whatever carried it.
- A `NameResolveResponse` is absorbed only if it was signed by a key recorded for the agent id it claims, and, on an authenticated connection, delivered by that key's own session. A forged or relayed response, signed or not, is dropped and counted.
- A `NameHello` or `NameHelloAck` on an authenticated connection carries the key of that connection.
- An application caller is the key that signed the request and, on TCP, WebSocket and authenticated QUIC, owns the connection it arrived on. Requests are fresh and unique within a 120 second window, and one key cannot fill the replay table for the others.
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

- `AgentId` is not self-certifying. A node that completes a hello can attach any `from` label to its own key, and a node refers peers by unsigned `(agent id, key)` pairs. The label is therefore routing metadata; only the key is identity. See "Open: deriving `AgentId` from the key" below.
- First contact with a bare seed address that has no configured key is trust on first use, once per process: the key that answers is recorded and required afterwards, but the first answerer is accepted, and the ack is matched to the dial by the endpoints it claims. Recorded pins are in memory and are forgotten on restart.
- The QUIC mesh handshake is not bound to the TLS channel: `spine-session`'s handshake has no input for channel context and is not modified here. A party that can terminate the TLS connection with a certificate the client trusts can relay a genuine peer's handshake and act as a man in the middle of the connection. The client's protection is trusting only the certificates it was given and pinning the expected mesh key; QUIC streams are not sealed with the mesh session keys, so TLS alone provides their confidentiality.
- A pin learned from a referral is the referrer's claim. The first claim for an agent id stands and a later referral does not replace it, so a malicious referrer can pre-claim a wrong key for an id it names first, which makes dials to that id fail rather than reach an impostor.
- The pin and address tables are bounded (1024 entries each); once full, new peers are not pinned from referrals and new first-contact seed keys are not recorded.
- Only `Application` requests have a freshness window. A recorded, genuine `NameResolveResponse` can be replayed to the node it answered while that lookup is open, on a connection that is not session-authenticated (plaintext, unauthenticated QUIC, in-process); lookups use sequential request ids.
- `ttl` and `hops` are not signed and are advisory. No transport entry point forwards envelopes, so nothing here routes on them.

### Open: deriving `AgentId` from the key

Making `from` self-certifying means `AgentId = first 16 bytes of SHA-256("spine-agent-id-v1" || public key)` with a constructor `AgentId::from_public_key`, used by `SigningIdentity::new` and `from_seed`, and a check in each handler that `from` equals the derivation for the signing key. That removes the need for the recorded (agent id, key) table in the bind check. It is not a contained change: `AgentId::new()` is the identity of agents that have no key at all in `research/` (about 180 call sites), `SigningIdentity::from_agent_id` exists to attach a key to a given id, ids are persisted in research crates' state, and tests throughout this crate construct peers with `AgentId::new()`. 128 bits also leaves a birthday bound of 2^64 for collisions between honestly generated ids, which a 256-bit id would not. It is left as a proposal.
