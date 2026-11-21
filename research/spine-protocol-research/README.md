# spine-protocol-research

Experimental protocol research for SPINE. **It is unreviewed, it is not on the
native wire, and nothing here is a security feature.** Do not use it to protect
data. Use TLS (or the session layer) for confidentiality and authentication.

The code was part of `spine-protocol` behind a `research` cargo feature. It moved
here when the native wire was cut down to the `SP` body, a compression flag byte
and a fixed 5-byte frame. `spine-protocol` no longer has a `research` feature and
no crate in the root workspace depends on this one except as an optional
dependency behind a `research` feature (`spine-gateway`).

## What is here

- `ChameleonKey`: a latent-space key that encodes a byte string into a
  `LatentVector` with a neural encoder and evolves per message. Its output is
  lossy and keyed by a 64-bit-derived seed in places; it is not an AEAD.
- `SpineConnection` and `connect_auto`: a second framing codec over TCP or QUIC
  with optional pre-shared-key AES-GCM and the chameleon layer. It does not
  speak the native wire.
- The protocol evolution framework (`ProtocolGenome`, `ProtocolPopulation`,
  `CoevolutionaryArena`, ...).
- `TitansLatentCodec`: a `spine_protocol::NeuralCodec` over `spine-neural`'s
  `NeuralLatentEncoder`. The gateway registers it behind its own `research`
  feature.
- Tests for `spine-nostd` and `spine-embedded` frame-header interop (they do not
  touch `spine-protocol`; the root crate may not depend on `research/`).

## Dependencies

It uses only public items of `spine-protocol`, plus `spine-neural` and
`spine-crypto`. Two small private helpers (`fresh_nonce`, `decode_zstd_limited`)
are copied here rather than made public there.

## Build

```
cd research
cargo test -p spine-protocol-research --lib --tests
cargo run -p spine-protocol-research --example chameleon_demo
cargo bench -p spine-protocol-research --bench neural_codec_bench
```

The crate has a crate-level `#![allow(dead_code)]`, carried over from
`spine-protocol`.

## Extension candidates: commands removed from the core set

The native wire spec (`docs/spec/00-wire.md`, D.2) moves five `BrowserCommand`
variants out of the core command set and into the research extension
`nervosys.org/research-session/1`. Extension (`EXT`) frames have no carrier
until the wire cut-over, so these five were removed from `spine-protocol`, the
daemon, the SDK, the CLI and the bindings, and are recorded here so the
extension can be rebuilt on `EXT` later. Payloads are the old serde shapes
(externally tagged JSON, `Uuid` as its string form, `Vec<u8>` as an array of
numbers). Each command ran inside a session, so the extension needs the same
session notion the daemon had.

| Old variant | Payload | Daemon behaviour (`spine-core` with `research`) |
| --- | --- | --- |
| `GetLatentUR` | `{ dimensions: usize }` | Parsed the session's page to a UR, JSON-encoded it, ran it through a shared `spine_neural::NeuralLatentEncoder::new(256, 1024, &[512, 256], 8, 42)` and answered `{ "ur", "latent_vector": [f32], "dimensions", "status": "encoded" }`. `dimensions` was echoed, not used. Errors: `No page loaded`, `Session not found`. |
| `TransferSession` | `{ target_node_id: Uuid }` | Serialised the whole session to JSON and handed it to `spine_cluster::ClusterNode::send_session_data(session_id, bytes)` (the target id was not passed on); answered `{ "status": "transferred", "target" }`. The receive side, `ClusterEvent::SessionDataReceived`, still imports the session in the daemon. |
| `SetAutonomousMode` | `{ enabled: bool }` | Set a per-session flag. A 5 second loop read it and ran `spine_human::ReasoningEngine::create_plan` on the session's page, appending `Click`/`Type` entries to the session history; nothing executed them. The loop, the flag and the history are gone from the daemon. |
| `NeuralTransmit` | `{ data: Vec<u8>, domain: String }`, `domain` one of `RealTime`, `BulkData`, `SecureControl`, `IoT` (anything else meant `BulkData`) | Looked up or created a per-domain `spine_agentic::NeuralProtocol::new(1000.0, 5.0)` and answered the JSON of its `transmit(&data, domain)` result (`TransmissionResult`). |
| `GetAgenticState` | no payload | Answered `{ "miras_variant", "surprise_level": null, "agent_id", "trust_level" }` from a `spine_agentic::AgenticWebRuntime` built from an `AgentProfile` named "SPINE Core Agent". |

SDK pieces that only served them and went with them: `AgentClient::get_latent_ur`,
`transfer_session`, `set_autonomous_mode`, `get_agentic_state`,
`transmit_neural(&[u8], spine_agentic::ProtocolDomain)`, the public
`neural_protocol` field, and the `enable_human_mode` pacing (its only readers
were the deleted `click` and `type_text`). `spine-core` dropped its optional
`spine-neural` and `spine-agentic` dependencies, and `spine-agent`'s `research`
feature dropped `spine-human`. The `Session` snapshot fields `history`,
`last_command` and `autonomous_mode` are gone with them.
