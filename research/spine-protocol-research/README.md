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
- `LatentVector` and the neural-codec types (`EncodedFrame`, `CodecRegistry`, the
  `NeuralCodec` trait, `EmbeddingRequest`, ...), moved here from `spine-protocol`.
- `TitansLatentCodec`: a `NeuralCodec` over `spine-neural`'s
  `NeuralLatentEncoder`. The gateway registers it behind its own `research`
  feature.
- Tests for `spine-nostd` and `spine-embedded` frame-header interop (they do not
  touch `spine-protocol`; the root crate may not depend on `research/`).

## Dependencies

It uses only public items of `spine-protocol`, plus `spine-neural` and
`spine-crypto`. Two small private helpers (`fresh_nonce`, `decode_zstd_limited`)
are copied here rather than made public there. The moved latent and codec types
add `bincode`, `bytemuck` and `thiserror` (`spine-protocol` dropped `bincode`,
`bytemuck` and `parking_lot`).

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

## Extension candidates: `Message` variants removed from the core set

Step 8 of the wire migration cut `Message` down to twelve variants (`Request`,
`Response`, `Ping`, `Pong`, `ToolCall`, `ToolResult`, `StreamStart`,
`StreamToken`, `StreamEnd`, `StreamCancel`, `CapabilityQuery`, `CapabilityAd`).
The six neural-codec variants left the core. The types they carried moved here
(`LatentVector` in `latent.rs`; the codec types, the `NeuralCodec` trait and
`CodecRegistry` in `neural_codec.rs`), and the variants are recorded below so a
neural-codec extension can carry them on `EXT` frames once there is a carrier.
Payloads are the old serde shapes (externally tagged JSON as `Message`, CBOR on
the native wire).

| Old variant | Payload | Meaning |
| --- | --- | --- |
| `LatentMessage` | `LatentVector { components: Vec<f32>, dim_hint: u16, epoch: u64 }` | One latent vector. The daemon sent one per `latent_streams` entry after the `Response` to an `ExecuteBinary`, with `dim_hint: 0, epoch: 0`. The SDK forwarded `components` to `start_listener`/`subscribe_latent` receivers. Both are gone. The response to `ExecuteBinary` still reports `latent_streams_count`. |
| `Encoded` | `EncodedFrame { codec, variant, data, metadata: EncodedMetadata { modality, shape, dtype, original_len, source_hash }, trace }` | A self-describing latent payload. |
| `CodecAd` | `CodecAdvertisement { id, agent_id, codecs: Vec<CodecDescriptor> }` | Which encoders and decoders the sender speaks; the answer to a `CapabilityQuery` with selector `Exact("spine:codecs")`, or an unsolicited push. |
| `CodecNegotiation` | `CodecNegotiation { id, offered, accepted, reason }` | Offer and select handshake: the offerer sends a ranked list and the responder echoes `id` with `accepted` set. |
| `EmbeddingRequest` | `EmbeddingRequest { id, input: Text \| Texts \| Encoded, codec, trace }` | "Embed this for me." `Encoded` input asks for a transcode. |
| `EmbeddingResponse` | `EmbeddingResponse { id, codec, embeddings: Vec<EncodedFrame>, trace }` | One frame per input element. |

Two stream fields went the same way: `StreamData::Encoded(EncodedFrame)` (a latent
chunk inside a token stream) and `StreamStart::stream_codec` (the codec the
producer will use for those chunks, so the receiver can resolve it once).
`StreamStart::decode_hints` and `DecodeHints` stayed in `spine-protocol`.

Two variants were deleted outright, not recorded as extension candidates, because
nothing produced them any more: `Event` (a name and a JSON value; the daemon never
sent one and the SDK only forwarded it) and `BinaryProgram(SpineBinary)` (a
duplicate of `BrowserCommand::ExecuteBinary`). The `Event` struct went with the
variant.

The gateway serves `POST /v1/embeddings` and `GET /v1/agentic/codecs` from the
types here, behind its own `research` feature. A gateway built without the
feature answers both routes with `503 Service Unavailable`.
