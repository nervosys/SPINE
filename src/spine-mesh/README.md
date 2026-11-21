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
| TCP, encrypted (`Security::Encrypted`) | `spine-session` handshake: the peer proved an Ed25519 key (`ReplyPath::authenticated_peer`). Pinned to an expected key only when the dialer supplies one (`set_identity`, or the signed `recipient_key` of an application call); other dials learn the key from the handshake | `Some(session key)` |
| TCP, plaintext | nothing | `None` |
| WebSocket, encrypted / plaintext | as TCP, through the same `pump` | `Some(session key)` / `None` |
| QUIC | TLS 1.3, with the server certificate verified against the set given to `QuicNameTransport::trust_certificate` (empty by default). With `authenticated`, a mesh handshake also runs on the first stream, but its session key is discarded and not bound to the TLS channel, and the replies are `ReplyPath::Plain` | `None` |
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
| `Application` | The signature verifies under `message.public_key`; `pump` additionally requires the session key to equal it, so plaintext connections cannot carry it. A request answered without a connection also requires binding of `from` to that key. A response is matched to a waiter by (key, `from`) recorded when the call was made. Stale or repeated request ids are refused (`ReplayGuard`) | The caller is the signing key (`MeshApplication::handle` receives the key, never `from`). `from` is used to route the reply and to key the replay window |
| `AgentMessage`, `PeerAnnounce`, `PeerQuery`, `Ping`, `Pong`, `RouteRequest`, `RouteResponse`, `KnowledgeSync`, `SwarmInvite`, `SwarmResponse` | No handler exists. `handle_envelope*` returns `false` and `pump` discards them | Nothing. `MeshNode::process_envelope`, which used to deliver and forward them and verified a signature only for senders already in a trust map (unknown senders passed through unchecked), had no caller and was removed. An embedder that handles these variants must verify them itself with `MeshNode::verify_carried_key` against a key it has bound to `from` |

### What holds

- A name record cannot be forged or altered in transit: it verifies under its own key whatever carried it.
- A `NameResolveResponse` is absorbed only if it was signed by a key recorded for the agent id it claims, and, on an authenticated connection, delivered by that key's own session. A forged or relayed response, signed or not, is dropped and counted.
- A `NameHello` or `NameHelloAck` on an authenticated connection carries the key of that connection.
- An application caller is the key that signed the request and, on TCP and WebSocket, owns the session it arrived on. Requests are fresh and unique within a 120 second window.

### What does not hold

- `AgentId` is not self-certifying. A node that completes a hello can attach any `from` label to its own key, and a node refers peers by unsigned `(agent id, key)` pairs. The label is therefore routing metadata; only the key is identity. See "Open: deriving `AgentId` from the key" below.
- First contact is trust on first use: a bootstrap dial by address learns whichever key answers there, and the ack is matched to the dial by the endpoints it claims.
- QUIC does not surface the peer's mesh key to `pump` and its handshake is not channel-bound to TLS, so `Application` messages (which need an authenticated peer key) are not accepted over QUIC, and the mesh handshake adds no protection against a party that holds a certificate the client trusts.
- Only `Application` requests have a freshness window. A recorded, genuine `NameResolveResponse` can be replayed to the node it answered while that lookup is open, on a connection that is not session-authenticated (plaintext, QUIC, in-process); lookups use sequential request ids.
- `ttl` and `hops` are not signed and are advisory. No transport entry point forwards envelopes, so nothing here routes on them.

### Open: deriving `AgentId` from the key

Making `from` self-certifying means `AgentId = first 16 bytes of SHA-256("spine-agent-id-v1" || public key)` with a constructor `AgentId::from_public_key`, used by `SigningIdentity::new` and `from_seed`, and a check in each handler that `from` equals the derivation for the signing key. That removes the need for the recorded (agent id, key) table in the bind check. It is not a contained change: `AgentId::new()` is the identity of agents that have no key at all in `research/` (about 180 call sites), `SigningIdentity::from_agent_id` exists to attach a key to a given id, ids are persisted in research crates' state, and tests throughout this crate construct peers with `AgentId::new()`. 128 bits also leaves a birthday bound of 2^64 for collisions between honestly generated ids, which a 256-bit id would not. It is left as a proposal.
