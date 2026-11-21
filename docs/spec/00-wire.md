# Wire: the native client to daemon protocol

Status: part A describes the code as built at commit `156f7e6` (branch `completion/agent-web`). Parts B to G are a proposal ("spine-wire v1") and are not implemented anywhere. Nothing was compiled or run to produce this document, with one exception: the two hex examples and the as-built size count in sections A.2 and C.12 were cross-checked with a throwaway Python script kept outside the repository. No SPINE code and no cargo command was run.

Citations are `path:line` relative to the repository root. A statement without a citation is either a definition in this document or is marked "unverified". The ciborium crate source is not present on the development machine (`~/.cargo/registry` holds only the index), so every claim about ciborium's byte output is marked unverified.

Scope: the client to daemon path (SDK, CLI, gateway, bindings to `spine-core`). The node to node mesh path is a different protocol and is described only to say where it differs (A.1, C.11). Compatibility with earlier SPINE versions, HTTP and MCP is not a goal (`docs/FIRST_PRINCIPLES.md`, first paragraph).

Related: `docs/spec/01-session.md` section 4 describes the same framing at an older commit (`7f7d1fc`); several of its line numbers and one finding (the unbounded QUIC read) no longer hold. Where this document and that one disagree, this one is later and cites its lines.

## A. As built

### A.1 Who speaks the native wire

| Party | Role | Transport | Cite |
| --- | --- | --- | --- |
| `spine-core` daemon | server | TCP (port 8080), WebSocket (port+1), QUIC (port+2, only when built with `--features quic`, which is off by default) | `src/spine-core/src/config.rs:221-224`, `src/spine-core/src/main.rs:2255-2323, 2441-2456, 2476-2487`, `src/spine-core/Cargo.toml` (`default = []`, `quic = ["spine-protocol/quic"]`) |
| `spine-agent` SDK (`AgentClient`) | client | TCP, TCP+TLS (optional mTLS), WebSocket (`ws://` or `wss://`). No QUIC client. | `src/spine-agent/src/lib.rs:34-47, 107-218, 220-236` |
| `spine-cli` | client via SDK | TCP, TLS, WebSocket | `src/spine-cli/src/commands/connect.rs:28,48`, `mcp.rs:30`, `tasks.rs:41`, `init.rs:88-199` |
| `spine-gateway` | client via SDK | TCP only (`AgentClient::connect`) | `src/spine-gateway/src/main.rs:292`, `names.rs:280` |
| `spine-ffi` (and Go through cgo) | client via SDK | TCP only (`AgentClient<TcpStream>`) | `src/spine-ffi/src/lib.rs:98-127`, `src/spine-go/spine.go:4,38-39` |
| `spine-python` | client via SDK | TCP only. Not in the root workspace (`exclude`), so not built by the root CI jobs. | `src/spine-python/src/lib.rs:29,110,281-284`, root `Cargo.toml` (`exclude = ["src/spine-python", "src/spine-js", "research"]`) |
| `spine-js`, `spine-wasm` | none | Offline parsing and compilation only. They import types, not the wire. | `src/spine-js/src/lib.rs:112-143`, `src/spine-wasm/src/lib.rs:20` |
| Browser extension | speaks something else | JSON text over `ws://localhost:9800/ws` with `{type: ...}` objects. No listener on 9800 exists in `src/**/*.rs` (searched). It does not speak the native wire. | `extension/background.js:4,18,33,87` |
| `research/spine-browser` | client via SDK | not in CI (`--exclude spine-browser`, `.github/workflows/ci.yml:106,109`) | `research/spine-browser/Cargo.toml:14` |
| Mesh node to node | different protocol | TCP, u32 BE length, JSON `MeshEnvelope`, 4 MiB cap, optionally inside the `spine-session` AEAD record layer | `src/spine-mesh/src/mesh_tcp.rs:62,160-217`, `src/spine-session/src/handshake.rs:87-102,483-509` |

`scripts/smoke_candidate.py` does not speak the native frame. It drives the CLI, opens bare sockets to test connection admission (it holds two idle TCP sockets and expects the third to be closed, `scripts/smoke_candidate.py:303-318`), and performs one raw TLS plus WebSocket upgrade request to test client-certificate policy (`:355-375`).

`src/spine-mesh/src/signed_frame.rs` (an Ed25519 signature over `wire::encode` output) is not on either path. Nothing outside its own tests calls `sign_frame` or `verify_frame` (searched `src` and `research`; `research/spine-agentic/src/lib.rs:83` re-exports the module).

### A.2 The layers, client to daemon and daemon to client

The framing is symmetric: both directions use the same `ProtocolHandler` (`src/spine-protocol/src/handler.rs:210-1039`). There is no handshake and no version exchange. The first byte on the connection is the first frame.

Sender path of `ProtocolHandler::send_message`, which is what the daemon and the SDK use (daemon calls at `src/spine-core/src/main.rs:2562-2687`, SDK at `src/spine-agent/src/lib.rs:332-345`). Default build (`research` off), no cipher, no morph:

```
L1  cbor      ciborium::into_writer(Message)                             wire.rs:207-213
L2  sp_body   'S' 'P' 0x01 0x02 || u32_be(len(L1)) || L1                  wire.rs:52-58,159-177,248-254
L3  json      serde_json::to_vec(SpeculativeFrame{
                payload: Full(L2 as Vec<u8>), next_prediction_hash: 0,
                confidence: 0.0, speculation_depth: 1 })                  handler.rs:598-606, lib.rs:137-176
              Vec<u8> serialises as a JSON array of decimal integers.
L4  padded    apply_padding(L3) per morphology.padding_mode               handler.rs:614, 923-967
L5  flagged   0x01 || zstd_level3(L4)   if len(L4) >= 64
              0x00 || L4                otherwise                          handler.rs:616-627
L6  sealed    L5, or nonce12 || AES-256-GCM(key, nonce, L5)
              only after enable_encryption(); no production caller        handler.rs:633-645, 309-312
L7  frame     length:u32 (BE if morphology.big_endian, else LE)
              || frame_version:u8
              || (header_size - 5) filler bytes, each = checksum_variant
              || L6                                                       handler.rs:857-887
```

Default morphology, `ProtocolMorphology::new(0)` (`handler.rs:151-166`, used at `:291`): `frame_version = 0`, `header_size = 5`, `big_endian = true`, `checksum_variant = 0`, `padding_mode = None`, `hmac_key = [0; 32]`. So an unmorphed frame is `u32_be length || 0x00 || flag byte || zstd(...)`. The `length` covers only L6.

Concrete size, `Message::Ping { timestamp: 42 }`, hand-counted and cross-checked with an outside script (unverified against Rust):

| Layer | Bytes | Note |
| --- | --- | --- |
| L1 CBOR | 19 | `a1 64 50 69 6e 67 a1 69 74 69 6d 65 73 74 61 6d 70 18 2a`, i.e. `{"Ping": {"timestamp": 42}}` if ciborium emits serde's external tagging as expected (unverified) |
| L2 `SP` body | 27 | `53 50 01 02 00 00 00 13` + L1 |
| L3 JSON | 177 | 87 fixed bytes of wrapper plus 90 bytes of array text |
| L5 | 1 + zstd(177 bytes) | the zstd output size was not computed |
| L7 | 5 + L5 | |

Consequence: the wrapper alone is 87 bytes, so `len(L4) >= 64` is true for every frame on this path when padding is `None`, and the `0x00` raw branch is reachable only through `send_message_raw`. The compression "threshold" is therefore not a threshold on `send_message`.

`send_message_raw` (`handler.rs:658-701`) is a second sender format: L2 directly (no L3 wrapper), then L4 to L7. It is used by tests, benches, `replay.rs:368` and `src/spine-agent/examples/offline_demo.rs`; no production caller (searched). The receiver accepts both (below).

Receiver path of `receive_message` (`handler.rs:704-831`), in order:

1. Read `header_size` bytes under the current morphology. The version byte and the filler bytes are read and ignored (`handler.rs:891-903`).
2. Length field to `usize`; reject if over 16 MiB (`:905-913`). Then `read_buf.resize(len, 0)` and `read_exact` (`:915-919`).
3. If a cipher is set: first 12 bytes are the nonce, AES-GCM decrypt, no AAD (`:713-721`). Otherwise skip.
4. First byte: `0x01` means zstd, bounded by `wire::decode_zstd_limited(.., MAX_DECODED_FRAME_BYTES)`; any other value, including `0x02` to `0xff`, means "raw" and the byte is dropped (`:727-731`).
5. `remove_padding` per morphology. It is lenient: invalid PKCS7 padding is returned unchanged, and Random and Chaotic modes truncate to whatever length the trailing bytes claim, clamped only by `total_padding <= data.len()` or `truncate(original_len)` (`:970-1009`).
6. Try `serde_json::from_slice::<SpeculativeFrame>`. If it parses, pick the message bytes by variant: `Full(v)` uses `v`; `Confirmed` looks up `prediction_cache` and returns an error if absent (`:769-773`); `Partial` and `Batch` use `suffix` and `fallback` respectively (`:775-796`). Then `wire::decode` (`:799`).
7. If step 6 fails for any reason, treat the whole buffer as a bare `SP` body and call `wire::decode` (`:814-830`).
8. `wire::decode` itself: if the first two bytes are not `SP`, parse the whole buffer as a headerless JSON `Message` (`wire.rs:266-270`). With `SP`, require version 1 and an exact length match, then dispatch on format `0x01` JSON, `0x02` CBOR, `0x03` CBOR+zstd (`wire.rs:272-297`).
9. If the message is `MorphRequest { seed }`, call `morph_now(seed)` (`handler.rs:802-804, 819-821`).

So one logical message has many accepted byte encodings: headerless JSON; `SP` plus JSON, CBOR or CBOR+zstd; each optionally wrapped in any of four `SpeculativePayload` shapes; each padded and flagged in several ways. The receiver is maximally permissive and the sender is not canonical.

Sender limits: `wire::encode` fails if the CBOR exceeds 16 MiB (`wire.rs:60,80-88,207-213`); `write_morphed_frame` fails if L6 exceeds 16 MiB (`handler.rs:858-861`); the write has a 30 second deadline after which the stream is shut down (`lib.rs:61-111`).

### A.3 Transports

| Transport | What wraps the byte stream | Differences |
| --- | --- | --- |
| TCP | Optional rustls TLS (`main.rs:2441-2456`, `src/spine-core/src/tls.rs`, off by default: `config.rs` `TlsConfig::default`). A non-loopback bind requires mutual TLS or `allow_unauthenticated_remote` (`config.rs:414-416`). | None. The handler reads and writes the stream directly. |
| WebSocket (daemon port 8081 by default) | Optional TLS, then `WebSocketServerBridge::accept` with tungstenite `max_message_size` and `max_frame_size` of 16 MiB (`src/spine-transport/src/websocket.rs:232-250`). `WebSocketStream` presents Binary messages as a byte stream, skips Text messages (`websocket.rs:417-421`), and turns each `poll_write` into one Binary message (`:443`). The adapters do not override vectored writes (searched `websocket.rs`), and `write_header_body` (`lib.rs:89-111`) therefore most likely issues the header and the body as two separate WebSocket messages (inference from the default `AsyncWrite::poll_write_vectored`, not verified in tokio). The byte layout above is unchanged because the reader re-assembles a stream. The client uses `connect_async` with tungstenite defaults (`websocket.rs:96-99`); its limits were not checked. No path, subprotocol or Origin check exists in SPINE code. |
| QUIC (daemon only, feature off by default) | `QuicEndpointBuilder::build_server`: self-signed certificate, TLS 1.3, no client authentication (`src/spine-protocol/src/lib.rs:929-954`). Startup is refused when `tls.enabled` (`main.rs:2261-2265`). The first bidirectional stream must arrive within 10 seconds (`main.rs:2302-2306`). | The stream is wrapped in `QuicStream` and handed to the same `ProtocolHandler` (`main.rs:2309-2311`), so the layer stack is identical. No client in the root workspace connects over QUIC. The only QUIC clients are the research-gated `SpineConnection::from_quic`/`connect_auto` (`research.rs:456, 828-866`), which write a u32 little-endian length and no 5-byte header (`research.rs:518-523`), which the daemon's `ProtocolHandler` would not parse. The only client builder skips server certificate verification (`lib.rs:956-975, 998-1048`). |

`SpineConnection` (`research.rs:412-581`, research-gated) is a second codec with its own `ProtocolHandlerState` (`research.rs:583-`). It does not apply the `SpeculativeFrame` wrapper, so its output is read by `ProtocolHandler` only through the legacy fallback in step 7 above. Neither the daemon nor the SDK uses it.

### A.4 Morphology as exercised by the default daemon

`BrowserCommand::Morph` and the daemon's proactive `MorphRequest` both change the frame header of every later frame (`handler.rs:178-203, 1035-1038`). The relevant facts, all from reading:

- It is active in a research-free daemon. `handle_session` intercepts `Morph` before `handle_command` (`main.rs:2656-2671`), so the research-gated `Morph` arm at `main.rs:1351` and the `unsupported_without_research` arm at `:1610-1619` are unreachable for `Morph` in both builds. The task description lists `Morph` among the variants a research-free daemon refuses; it does not refuse it.
- The daemon morphs by itself. A task started at `main.rs:2230-2250` runs every 30 seconds and sets `needs_morph = true` on every session that has a `last_command`. The next `Request` on that session makes the daemon send `MorphRequest { seed }` under the old framing and then call `morph_now(seed)` (`main.rs:2636-2650`). The client switches when it receives `MorphRequest` (`handler.rs:802-804`). A client that has already pipelined a request before it reads `MorphRequest` sends that request under the old framing to a server already on the new one, so it would desynchronise. The SDK is strictly request then response and does not pipeline (`src/spine-agent/src/lib.rs:332-360`); this is by reading, not tested.
- Without a chameleon the evolution is not secret. `ProtocolMorphology::new` sets `hmac_key = [0; 32]` (`handler.rs:163`); only `new_keyed`, called by the research-gated `enable_chameleon*`, sets a real key (`handler.rs:169-173, 315-351`). `evolve` therefore runs an HMAC under an all-zero key on a counter and a seed that is sent in the clear. Anyone observing the connection can compute the next framing.
- Morph can select `Pkcs7`, `Random` or `Chaotic` padding and header sizes 5 to 16 bytes (`handler.rs:193-202`).
- The desync fixed in `8722dc8`/`b8a0a4e` (the SDK's `morph()` not calling `morph_now`) is gone from `lib.rs`; the SDK now takes the seed from the response and calls it (`src/spine-agent/src/lib.rs`, the `morph` method).

### A.5 Speculation: what is live

- Default build: `SpeculativePredictor` is a stand-in whose `predict_next` returns `(0, 0.0)` and whose other methods return empty values (`handler.rs:27-63`). `ProtocolHandler::new` still allocates two of them and two `Vec` caches (`handler.rs:285-306`).
- `send_message` emits `Full` unless the peer's last advertised `next_prediction_hash` equals this message's hash (`handler.rs:559-575`). The peer's value is always 0 with the stand-in, so `Confirmed` is chosen only on a 64-bit hash collision with 0. "Always yields a Full frame" is true in practice and not by construction.
- The receiver accepts all four payload shapes from a peer regardless of build (`handler.rs:743-796`). A peer can make the receiver return the error "Speculation hit but prediction not in cache" at will.
- `enable_speculation` sets two booleans that nothing reads (`handler.rs:354-357` writes `output_speculation` and `input_speculation`; a search of `src/spine-protocol/src` finds no read of either field, in the stand-in or in the research predictor, `research.rs:69-71`).
- `precompute_response` fills `precomputed_cache`; `get_precomputed` is private and has no caller (`handler.rs:360-381`; searched `src`). So a `PreComputed` message is never transmitted in either build. The daemon still builds `PreComputedResponse` values on every `Request` through `speculate_responses` and discards them (`main.rs:2718-2754`); with the stand-in `predict_batch` returns nothing, so the callback never runs.
- Statistics (`SpeculationStats`) are read only to print them: `src/spine-cli/src/commands/init.rs:228`, `connect.rs:112`, `benchmark.rs:97`.

### A.6 Limits and what happens on violation

| Limit | Value | Enforced at | On violation |
| --- | --- | --- | --- |
| Frame length (receive) | 16 MiB (`MAX_TRANSPORT_FRAME_BYTES`, `wire.rs:64`) | `handler.rs:905-913`, checked after the header is read and before the body buffer is resized | `receive_message` returns an error; `handle_session` logs it and breaks, so the connection is dropped (`main.rs:2757-2761`). Up to 16 MiB is zero-filled before any body byte arrives (`handler.rs:916-918`): a peer that announces 16 MiB and stops sending holds 16 MiB until the idle timeout. |
| Frame length (send) | 16 MiB | `handler.rs:858-861` | `send_message` returns an error to the caller |
| Decompressed outer frame | 64 MiB (`MAX_DECODED_FRAME_BYTES`, `wire.rs:62`) and zstd window log 23 (`wire.rs:68-78`) | `handler.rs:728` | error, connection dropped |
| Inner message | 16 MiB (`MAX_MESSAGE_BYTES`, `wire.rs:60`); `decode` rejects buffers over 16 MiB + 8 | `wire.rs:262-265, 80-88, 272-276` | error, connection dropped |
| `SP` version | must be 1 | `wire.rs:272-274` | `UnsupportedVersion`, connection dropped. There is no negotiation. |
| `SP` length field | must equal the remaining bytes | `wire.rs:276-281` | `LengthMismatch`, connection dropped |
| Unknown `SP` format byte | known: 1, 2, 3 | `wire.rs:289-296` | `UnknownFormat`, connection dropped |
| Frame header version and filler bytes | none | not validated (`handler.rs:891-903`) | accepted |
| Unknown or malformed message body | serde error | `wire::decode` | error, connection dropped |
| Valid `Message` the daemon does not handle | n/a | `Ok(_) => {}` at `main.rs:2756` | silently ignored: no error and no response; a waiting client times out |
| Idle | `idle_timeout_secs`, default 300 | `main.rs:2530-2537`, `config.rs:229` | connection dropped. The timer covers one whole `receive_message`, including a half-sent frame. |
| Connections | 256 total, 50 per IP (`config.rs:227-228`) | `main.rs:2422-2425, 2463-2466, 2289-2295` | socket dropped without a byte (QUIC: `incoming.refuse()`) |
| Sessions | 1000 | `main.rs:2600-2621` | `Response` with error "Server busy: maximum sessions reached", then the session loop returns |
| Request rate | 100 token bucket, 10 per second refill, per session | `main.rs:372-384, 2625-2634` | `Response` with error "Rate limit exceeded", connection stays open. `ToolCall` uses a separate bucket keyed by a per-connection id (`main.rs:2524-2527, 2571-2583`) and answers `ToolOutcome::Err { code: "rate_limit" }`. |
| TLS handshake, WebSocket upgrade, QUIC connect, QUIC stream accept | 10 s each | `main.rs:2299-2306, 2444, 2479, 2504-2508` | connection dropped |
| WebSocket message and frame (server) | 16 MiB | `websocket.rs:238-239` | tungstenite error, surfaced as an `io::Error` from `poll_read` |
| Write | 30 s | `lib.rs:66-86` | stream shut down |
| Max uploaded resource | 8 MiB (`MAX_RESOURCE_BYTES`), download chunk 64 KiB | `src/spine-protocol/src/resource.rs:6-7`, `src/spine-core/src/resources.rs:143-152` | command-level error |

The daemon handles one frame at a time on a connection and awaits each command inline (`main.rs:2672-2685`), so a slow `Navigate` blocks `Ping` and everything else on that connection.

### A.7 Dispatch in the daemon

`handle_session` (`src/spine-core/src/main.rs:2519-2763`): `CapabilityQuery` to `CapabilityAd` (2539-2569; `Semantic` selector matches nothing, 2548); `ToolCall` to `ToolResult` (2570-2591); `Ping` to `Pong` (2592-2594); `Request` (2595-2755) as below; anything else is dropped (2756). A `Request` creates a session lease on first use (2600-2621), may trigger a proactive morph (2636-2650), runs `handle_command` (or the inline `Morph` path), replies with `Response`, then sends one `LatentMessage` per vector that an `ExecuteBinary` program produced (2704-2716).

### A.8 Message inventory

`Message` (`src/spine-protocol/src/lib.rs:273-329`) has 24 variants. "Research-free" means a daemon built with `default = []` features. "Sent in root workspace" means a non-test, non-example, non-bench constructor exists in `src/` and is reachable from a binary (SDK methods count; callers listed). Searches: `rg "Message::<Variant>"` over `src` and `research`, then SDK method callers in `spine-cli`, `spine-gateway`, `spine-ffi`, `spine-core`.

| # | Variant | Direction | Produced by | Consumed by | Research-free daemon | Sent in root workspace |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `Request` | C to S | SDK `send_request` (`spine-agent/src/lib.rs:339`) | daemon `main.rs:2595` | yes | yes (all SDK commands) |
| 2 | `Response` | S to C | daemon (`main.rs:2613, 2627, 2687`) | SDK `send_request` | yes | yes |
| 3 | `Event` | S to C | only `ProtocolHandler::send_decoy` (`handler.rs:1012-1032`); `research/spine-browser/src/main.rs:994` calls it | SDK forwards to `event_tx` | daemon never sends one | no |
| 4 | `BinaryProgram` | n/a | nothing | nothing (daemon drops it, `main.rs:2756`) | dropped | no |
| 5 | `LatentMessage` | S to C | daemon after `ExecuteBinary` (`main.rs:1091, 2704-2716`) | SDK `send_request` forwards to `latent_tx` | yes (path is not gated) | yes, by the daemon, when an HLB program has `StreamLatent` |
| 6 | `Sync` | n/a | nothing outside a test (`wire.rs:354`) | nothing | dropped | no |
| 7 | `Speculative` | n/a | nothing | nothing | dropped | no |
| 8 | `PreComputed` | n/a | built at `main.rs:2730, 2742` and never transmitted (A.5) | nothing | not sent | no |
| 9 | `MorphRequest` | S to C | daemon `main.rs:2642` | `handler.rs:802-804` | yes, sent by default every 30 s per active session | yes |
| 10 | `Ping` | either | SDK `ping` (`spine-agent/src/lib.rs`), CLI `status`, `connect`, `benchmark`, `init`, `query`, gateway, ffi | daemon `main.rs:2592` | yes | yes |
| 11 | `Pong` | either | daemon `main.rs:2593`; SDK `call_tool` (`lib.rs:384`) | SDK | yes | yes |
| 12 | `ToolCall` | C to S | SDK `call_tool` (`lib.rs:377`); CLI `mcp.rs` | daemon `main.rs:2570` | yes (two read-only tools, `tools.rs:4-14`) | yes |
| 13 | `ToolResult` | S to C | daemon `main.rs:2585` | SDK | yes | yes |
| 14 | `StreamStart` | S to C | nothing | nothing | dropped | no. The types are used by the gateway's SSE translation (`spine-gateway/src/agentic_sse.rs`), not sent. |
| 15 | `StreamToken` | S to C | nothing | nothing | dropped | no |
| 16 | `StreamEnd` | S to C | nothing | nothing | dropped | no |
| 17 | `StreamCancel` | C to S | nothing | nothing (no handler anywhere in `src`) | dropped | no |
| 18 | `CapabilityQuery` | C to S | SDK `capabilities` (`lib.rs:270`); CLI `mcp.rs` | daemon `main.rs:2539` | yes | yes |
| 19 | `CapabilityAd` | S to C | daemon `main.rs:2563` | SDK | yes | yes |
| 20 | `Encoded` | either | nothing outside tests and benches | nothing | dropped | no |
| 21 | `CodecAd` | either | nothing | nothing | dropped | no |
| 22 | `CodecNegotiation` | either | nothing | nothing | dropped | no |
| 23 | `EmbeddingRequest` | C to S | nothing | nothing | dropped | no |
| 24 | `EmbeddingResponse` | S to C | nothing | nothing | dropped | no |

`Request` is `{ id: String, command: BrowserCommand }` and `Response` is `{ id: String, result: Option<serde_json::Value>, error: Option<String> }` (`lib.rs:751-762`). `Event` is `{ name, data: Value }` (`:764-768`).

`BrowserCommand` (`lib.rs:498-670`) has 35 variants. The "research-free" column says what `handle_command` (`main.rs:556-1628`) does. "Root senders" are non-test callers of the SDK method in `spine-cli`, `spine-gateway`, `spine-ffi`, `spine-core` (spine-python is outside the root workspace and is noted separately).

| # | Variant | Daemon arm | Research-free daemon | Root senders |
| --- | --- | --- | --- | --- |
| 1 | `Navigate` | `main.rs:945` | works (outbound fetch policy); the HLB transpile is empty without research (`:964-975`) | CLI `benchmark`, `connect`, `init`, `query`; gateway; ffi |
| 2 | `GetUR` | `:997` | works | CLI `benchmark`, `connect`, `init`, `query`; gateway; ffi |
| 3 | `GetRawHTML` | `:1032` | works | CLI `connect`, `query`; gateway; ffi |
| 4 | `Click` | `:1293` | stub: prints to stdout, returns `{"status":"clicked"}`, does nothing else | none (python only) |
| 5 | `Type` | `:1301` | stub, same | none (python only) |
| 6 | `ExecuteBinary` | `:1055` | works, partly gated inside (`:1140-1202`). The result of `wasm_runtime.execute` is unwrapped with `.expect` (`:1086-1089`). | CLI `connect`, `query`; gateway; ffi (through `execute_hls`) |
| 7 | `HandleEvent` | `:1228` | implemented | none (examples only) |
| 8 | `GetLatentUR` | `:1310` (research); else `:1610-1619` | `unsupported_without_research` | CLI `benchmark` (skips when unsupported); python |
| 9 | `Morph` | handled in `handle_session` `:2657`; the arms at `:1351` and `:1612` are unreachable | works (A.4) | CLI `connect`; ffi; python |
| 10 | `Search` | `:1366` | returns a hard-coded result (`"Found relevant content locally..."`, relevance 0.92) plus `"status": "local_only"` | CLI `query`, `connect`; ffi; gateway `main.rs:430` (receiver type not checked) |
| 11 | `TransferSession` | `:1415` (research); else `:1610-1619` | `unsupported_without_research` | none (examples only) |
| 12 | `StoreKnowledge` | `:1453` | works (in-memory, per session) | CLI `init`, `query`; ffi; python |
| 13 | `QueryKnowledge` | `:1484` | works | CLI `init`, `query`; ffi; python |
| 14 | `DeleteKnowledge` | `:1505` | works | none |
| 15 | `GetSessionHistory` | `:1515` | works | none (examples only) |
| 16 | `GetCapabilities` | `:1527` | works; returns the capability list of the session's current binary, not the origin's tools | CLI `connect`; ffi; python |
| 17 | `SetAutonomousMode` | `:1539` | sets a flag; the loop that reads it is research-gated (`:2169-`) | CLI `init` |
| 18 | `SwarmSearch` | `:1557-1565` | `unsupported_legacy_swarm` in every build | CLI `init` |
| 19 | `DelegateTask` | same | same | none (a test asserts the refusal, `tests/agent_web.rs:462-478`) |
| 20 | `ProposeKnowledge` | same | same | none |
| 21 | `CreateSwarmPlan` | same | same | CLI `init` |
| 22 | `ExecutePlanTask` | same | same | none (test asserts refusal) |
| 23 | `NeuralTransmit` | `:1567` (research); else `:1610-1619` | `unsupported_without_research` | none (examples only) |
| 24 | `GetAgenticState` | `:1594` (research); else `:1610-1619` | `unsupported_without_research` | CLI `init` |
| 25 | `SendSpeechAct` | `:1620-1623` | `unsupported_speech_act` in every build | none (test asserts refusal) |
| 26 | `ResolveName` | `:688` | works (needs a namespace for network answers) | CLI `names`; gateway; SDK `resources.rs` |
| 27 | `ResolveNames` | `:700` | works | gateway |
| 28 | `FindProviders` | `:749` | works | CLI `names`; gateway |
| 29 | `PublishName` | `:819` | works | CLI `names`; gateway |
| 30 | `FetchName` | `:858` | works | gateway |
| 31 | `CrawlNames` | `:889` | works | gateway |
| 32 | `PublishResource` | `:651` | works | CLI `names`; gateway; SDK `resources.rs` |
| 33 | `ReadResource` | `:629` | works | gateway; SDK `resources.rs` |
| 34 | `NamespaceStatus` | `:579` | works | CLI `names` |
| 35 | `MeshCall` | `:603` | works | CLI `tasks` |

Python additionally sends `Click`, `Type`, `GetLatentUR`, `Morph`, `Search`, `StoreKnowledge`, `QueryKnowledge`, `GetCapabilities` (`src/spine-python/src/lib.rs:116-262`).

### A.9 Where the task description needed correcting

1. "Research-free daemon answers research variants with `unsupported_without_research`": true for four commands only (`GetLatentUR`, `TransferSession`, `NeuralTransmit`, `GetAgenticState`, `main.rs:1610-1619`). `Morph` is not among them in practice (A.4). Five swarm commands and `SendSpeechAct` are refused in every build with different error strings. Every research-flavoured `Message` variant (`LatentMessage` inbound, `Encoded`, `CodecAd`, `CodecNegotiation`, `EmbeddingRequest`, `EmbeddingResponse`, `Sync`, `Speculative`, `PreComputed`, `BinaryProgram`, `Event`, `Stream*`) is not answered at all: `Ok(_) => {}` drops it (`main.rs:2756`).
2. "Speculation fields, prediction cache and per-message predictor are dead": confirmed, and also dead in a research build: nothing ever reads `precomputed_cache` (A.5). "Always yields a Full frame" holds only up to a hash collision.
3. "`Morph` changes framing mid-connection": confirmed, with three additions: the daemon triggers it itself every 30 s; it works in a research-free daemon; without a chameleon the evolution uses an all-zero key (A.4).
4. "padded per `ProtocolMorphology`, then a flag byte and zstd at >= 64 bytes": the order is right, but the compression threshold applies to the padded JSON wrapper, which is at least 87 bytes, so the compressed branch is the only one reachable on `send_message`. There is also a second sender format, `send_message_raw`, and the receiver accepts many more encodings than either sender produces (A.2).
5. "A message is CBOR-encoded with an 8-byte `SP` header, then wrapped in a `SpeculativeFrame` serialised as JSON with the body as an array of numbers": correct.
6. "No encryption unless TLS": correct. `enable_encryption` exists (PSK AES-256-GCM, random nonce, no AAD) and has no production caller (`rg enable_encryption` finds only tests).
7. "The mesh path ... signed_frame.rs": `signed_frame.rs` is not used by the mesh path or anything else (A.1). The mesh uses JSON envelopes in `mesh_tcp.rs`.
8. "Rustdoc warnings denied" in CI: I found no `RUSTDOCFLAGS` anywhere in `.github/workflows/`. The `docs` job runs `cargo doc --workspace --no-deps` with the global `RUSTFLAGS: "-D warnings"` (`ci.yml:15-16, 340-341`). I believe `RUSTFLAGS` does not make rustdoc lints errors, but I did not verify that. Treat "rustdoc warnings denied" as unconfirmed.

### A.10 Other findings, recorded and not fixed

- `ExecuteBinary` panics on a failed execution: `.expect("Failed to execute HLB in WASM")` (`main.rs:1086-1089`). `WasmRuntime::execute` returns `Err` on a WAT parse failure, a module compile failure and an instantiation failure, and sets a fuel budget that traps (`src/spine-wasm/src/lib.rs:702-745`). The release profile has `panic = "abort"` (root `Cargo.toml`, `[profile.release]`). By reading, a client-supplied program that fails to compile or runs out of fuel aborts the whole daemon in a release build. Not run.
- `src/spine-protocol/src/negotiation.rs` defines `VersionOffer`/`VersionResponse`/`ProtocolFeature` (JSON, `negotiation.rs:68-101, 279-319`). Nothing on any wire uses it; only `fuzz/fuzz_targets/fuzz_negotiation.rs` does. `evaluate_offer` accepts any `offer.major >= 1` and ignores `min_major`/`min_minor` (`negotiation.rs:171-190`). Its feature list names Chameleon, Speculation, Morphology, Rlwe (`:17-40`).
- The SDK `AgentClient::connect` returns after the TCP connect; the test helper treats that as "daemon ready" (`src/spine-core/tests/agent_web.rs:419-444`).
- `Search`, `Click` and `Type` are stubs that return fabricated or empty success (A.8). A caller cannot tell them from a working implementation.

## B. Requirements

Derived from `docs/FIRST_PRINCIPLES.md` (premise numbers P1 to P9 refer to its table; "change 1" is "Rejustify the wire"). Each is testable by a named kind of test.

| # | Requirement | Source | Test |
| --- | --- | --- | --- |
| R1 | Framing is fixed for the life of a connection. No frame changes the framing of a later frame. | change 1; A.4 | Property test: decoding a frame sequence gives the same result whatever frames precede it; grep gate: no state in the codec other than the negotiated limits. |
| R2 | Every length field is bounded before any allocation or read that depends on it. An out-of-range length ends the connection. | P7; A.6 | Feed a 4-byte length of 0xFFFFFFFF and of limit+1; assert no read of the body and no allocation above 64 KiB (counting allocator or a reader that fails on a body read). |
| R3 | Decompressed size is declared, checked against the limit before inflation, and checked again after. | P7; A.6 | A zstd bomb whose declared length is small and whose true output is large fails; one whose declared length exceeds the limit fails without calling the decompressor. |
| R4 | For every accepted frame body, re-encoding the decoded value yields the identical bytes. Non-canonical input is rejected, not normalised. | Stage 5 first item (canonical encoding); change 1 (capability chains and receipts will cover bytes) | Differential test: decode then encode equals input for every vector; a corpus of non-canonical encodings (indefinite length, non-shortest integer, unsorted keys, duplicate keys, trailing bytes) is rejected. |
| R5 | A version and capability exchange precedes any message, with a defined outcome when no version is shared. | change 1 | Handshake tests: shared version, no shared version, unknown extension offered, HELLO timeout. |
| R6 | Every request carries a correlation id in the frame header; replies echo it; several requests may be in flight; unsolicited frames use id 0. | P4, P8 | Two requests in flight answered out of order are matched correctly. |
| R7 | Streams and cancellation are frame types. A cancel is processed while a response is in progress and the producer stops within one frame. | P4, P8 (cancel latency) | A producer emitting tokens stops after `STREAM_CANCEL`, and emits `STREAM_END` with reason `cancelled`. |
| R8 | Per-message overhead is a fixed 15 bytes plus the deterministic CBOR body; there is no per-connection model, predictor or cache. | P8, change 1 | Size assertion on an empty-bodied frame; no allocation of predictor state in the connection constructor. |
| R9 | The core message enums contain no research variants. Extensions are negotiated by name before use. | change 7 | The core crate compiles with the research workspace absent; an unnegotiated extension frame closes the connection. |
| R10 | Every violation class has exactly one defined outcome, either a connection close with a code or an error response that leaves the connection open. | A.6 | A table-driven conformance suite, one vector per row of C.7. |
| R11 | The wire never relies on the frame layer for confidentiality or authentication. The deployment refuses to carry it in the clear off-loopback. | P3; `config.rs:414-416` | Existing check kept; a test that a non-loopback bind without a secure transport fails to start. |
| R12 | Error responses carry a stable machine-readable `code` and a human `message`. | P1 (typed), P5 | Every error path in the daemon returns a code from a published list. |
| R13 | The server declares a per-connection in-flight request cap, and a request over the cap is answered with an error immediately, without being executed. | P7 | Send cap + 1 requests; the extra one is answered `overloaded`. |
| R14 | A second implementer can build a conformant peer from this document and its vectors alone. | change 6 | Hex vectors for every frame type, produced by the reference implementation and decoded by an independent decoder in another language, run in CI. |
| R15 | Deadlines are defined for connection setup: a peer that has not completed HELLO within a fixed time is closed. | P7; A.6 | Open a socket and send nothing; assert close within the deadline. |

## C. Proposed format: spine-wire v1

This is a refinement of the direction in the brief. Deviations are listed in C.13.

### C.1 Frame

All integers are big-endian. One frame:

```
 0                   4   5   6   7                  15
+-------------------+---+---+---+------------------+----------------+
| length : u32      | v | t | f | request id : u64 | body           |
+-------------------+---+---+---+------------------+----------------+
                      version, type, flags
length = 11 + len(body)           (the bytes after the length field)
```

| Field | Size | Rule |
| --- | --- | --- |
| `length` | 4 | Count of all bytes after this field. Minimum 11. Maximum is the negotiated `max_frame` (default and absolute ceiling 16,777,216, which is the current `MAX_TRANSPORT_FRAME_BYTES`, `wire.rs:64`, and is large enough for the 8 MiB resource upload, `resource.rs:6,19-37`). Before HELLO has been processed the maximum is 65,536. If `length` is below 11 or above the maximum the receiver closes the connection without reading the body and without allocating for it. It MAY send `CLOSE` first on a best-effort basis. |
| `version` | 1 | The connection's selected version. `1` in this document. `HELLO` always carries `1` (C.6). A frame whose version differs from the selected one closes the connection (`unsupported_version`). |
| `type` | 1 | C.3. |
| `flags` | 1 | Bit 0: body is compressed (C.4). Bits 1 to 7 MUST be zero; a frame with any of them set closes the connection (`malformed_frame`). |
| `request id` | 8 | C.8. |
| `body` | `length - 11` | Exactly one deterministic CBOR data item (C.5), possibly compressed. A frame type that defines an empty body is still one CBOR item (`null`, byte `f6`). |

Buffer allocation: the receiver SHOULD grow its buffer as bytes arrive, not reserve `length` up front. The as-built code zero-fills the announced length first (`handler.rs:916-918`).

### C.2 Transport assumptions

The frame layer needs an ordered, reliable byte stream: TCP, TLS over TCP, one QUIC bidirectional stream, or WebSocket Binary messages treated as a stream. Frame boundaries do not have to align with transport messages. One native connection uses one stream. The frame layer adds no confidentiality, no peer authentication and no replay protection (C.11).

### C.3 Frame types

| Type | Name | Sender | Request id | Body |
| --- | --- | --- | --- | --- |
| `0x00` | invalid | none | | receiving it closes the connection (`unknown_frame_type`) |
| `0x01` | `HELLO` | client, first frame only | 0 | map, C.6 |
| `0x02` | `HELLO_ACK` | server, first frame only | 0 | map, C.6 |
| `0x03` | `CLOSE` | either | 0 | `{code, message[, supported]}`; sender closes after sending |
| `0x04` | `PING` | either | nonzero | CBOR unsigned integer token |
| `0x05` | `PONG` | either | echoes `PING` | the same token |
| `0x10` | `REQUEST` | client | nonzero, unique among that client's in-flight requests | a command, C.5 and D |
| `0x11` | `RESPONSE` | server | echoes the request | `{ok: any}` or `{err: {code, message[, details]}}` |
| `0x20` | `STREAM_START` | server | echoes the request | `{role, model[, trace]}` plus fields from D |
| `0x21` | `STREAM_TOKEN` | server | echoes the request | `{seq, data[, usage]}` |
| `0x22` | `STREAM_END` | server | echoes the request | `{reason[, usage]}` |
| `0x23` | `STREAM_CANCEL` | client | names the stream's request | `{[reason]}` |
| `0x80` | `EXT` | either | extension-defined | array `[index, kind, payload]`, C.10 |

All other values (`0x06` to `0x0f`, `0x12` to `0x1f`, `0x24` to `0x7f`, `0x81` to `0xff`) are unallocated.

### C.4 Compression

Flag bit 0 set means the body is `u32 uncompressed_len || zstd frame`, where `uncompressed_len` counts the CBOR bytes. Rules:

1. The receiver checks `uncompressed_len <= max_frame - 11` before it calls the decompressor. If not, it closes the connection (`frame_too_large`).
2. The receiver decompresses with an output limit of `uncompressed_len` (it may read one byte more to detect overflow, as `decode_zstd_limited` does, `wire.rs:68-78`) and a window limit of 2^23 bytes (the as-built `window_log_max(23)`, `wire.rs:70`). If the output length differs from `uncompressed_len` it closes the connection (`malformed_frame`).
3. The zstd stream is a single frame with no dictionary.
4. Sender policy, not enforced by the receiver: do not compress a frame whose body is smaller than 1 KiB, keep the compressed form only if it is smaller, and do not compress a body that carries a secret chosen by one party and an attacker-influenced value from another (the compression-oracle concern the as-built code notes at `handler.rs:665-668`). The thresholds are proposals; the as-built values are 64 and 128 (`handler.rs:303`, `wire.rs:101`).

The bound is per frame. A frame of `max_frame` zeros compresses to a very small compressed size (not measured here), so the amplification per frame is large and only the request rate limit bounds a stream of such frames. v1 therefore also requires a per-connection budget of decompressed bytes per second, configured by the deployment. The as-built limiter counts commands, not bytes (`main.rs:372-384`).

### C.5 Body encoding: deterministic CBOR

A body is one CBOR data item that satisfies RFC 8949 section 4.2.1 (core deterministic encoding), plus the following v1 rules:

1. Definite lengths only. No indefinite-length strings, arrays or maps.
2. Integers and lengths in the shortest form. Floats in the shortest of binary16, binary32 and binary64 that preserves the value; NaN is `f9 7e 00`. No integer to float conversion.
3. Map keys sorted by the bytewise lexicographic order of their encodings; no duplicate keys.
4. No tags. Simple values limited to `false`, `true`, `null`. No `undefined`.
5. Text strings are valid UTF-8; no normalisation is applied.
6. Nothing may follow the item. Nesting depth at most 32 (a proposal; ciborium's own recursion limit was not checked).
7. Messages are modelled as follows, matching serde's defaults so the Rust types need no custom serialisers: a struct is a map with text keys; an enum with data is a single-entry map `{variant: fields}`; a unit variant is the text string of its name; `Option` fields are omitted when absent and a present `null` for an optional field is rejected; byte-string fields (hashes, keys, payloads) are CBOR byte strings, never arrays of integers. Variant and field names are wire identifiers. v1 uses snake_case names chosen explicitly with `#[serde(rename)]`, because serde's `rename_all = "snake_case"` turns `GetUR` into `get_u_r` (from serde's algorithm, not run).
8. Structs are closed: an unknown field in a known struct is a schema error (C.7). This differs from serde's default of ignoring unknown fields (no `deny_unknown_fields` was found in `agentic.rs` or `lib.rs`; not searched exhaustively).
9. Only the `HELLO` and `HELLO_ACK` bodies are open: unknown keys are ignored, so later versions can add fields to negotiation.

What the current stack guarantees, and what would have to change.

| Property | As built | Basis |
| --- | --- | --- |
| Map key order | Not sorted. Struct fields come out in declaration order (serde derive). `serde_json::Value::Object` is a `BTreeMap` (the lockfile shows `serde_json` has no `indexmap` dependency, so `preserve_order` is off; `Cargo.lock:3948-3959`) and sorts keys by code point and length-insensitively, which differs from RFC 8949 ordering (`"b"` sorts after `"aa"` in a `BTreeMap` and before it in CBOR). | serde derive; `Cargo.lock` |
| Nondeterministic containers | `SpineBinary.exported_functions` is a `std::collections::HashMap` (`lib.rs:377`), whose iteration order is randomised per process. Two encodings of the same value can differ. | `lib.rs:377` |
| Shortest-form integers, float width, definite lengths | Probably yes for integers and lengths; float width unknown. | unverified (ciborium source absent) |
| Duplicate keys and non-canonical input on decode | `ciborium::from_reader` is a general decoder and, I expect, accepts non-shortest integers, indefinite lengths and duplicate map keys. | unverified |
| `Vec<u8>` and `[u8; N]` | Arrays of integers unless `serde_bytes` is used. Used for `StreamData::Bytes`, `PublishResource.data` and `EncodedFrame` payloads; not used for `ReadResource.hash`, `ResolveName.if_none_match`, `TraceContext` ids, `MeshCall.peer_key`, which are `[u8; N]` and so encode as arrays (`lib.rs:611, 639, 658, 667`, `agentic.rs:286-293`). | `lib.rs`, `agentic.rs` |
| Many encodings per message | yes (A.2) | `handler.rs`, `wire.rs` |

What would have to change (no code is changed by this document):

- Add a validating scanner `check_deterministic(&[u8]) -> Result<(), Reason>` that walks the item once and enforces rules 1 to 6 before serde sees the bytes. Roughly 150 lines, no new dependency, testable against RFC 8949 Appendix A and a corpus of invalid inputs.
- Encode through `ciborium::Value`: serialise, sort every map's entries by encoded key bytes, then write. `Value::Map` is a `Vec<(Value, Value)>`, so this needs no ordering assumptions about ciborium. Or hand-write the encoder. A one-time extra allocation per message.
- Replace the `HashMap` in `SpineBinary` with a `BTreeMap` or a sorted `Vec`, or rely on the sorting pass above.
- Use `serde_bytes` on every fixed-size byte field listed above.
- Retire the three encodings of `wire.rs` (`FORMAT_JSON`, `FORMAT_CBOR`, `FORMAT_CBOR_ZSTD`), the headerless-JSON fallback (`wire.rs:266-270`) and `encode_json`.

Why determinism: nothing on the native wire is signed today, but the layer-3 and layer-5 designs in `docs/FIRST_PRINCIPLES.md` (capability chains, receipts) will sign or hash bytes that travel in these bodies, and Stage 5's first item asks for one canonical encoding for everything signed or hashed. A decoder that rejects non-canonical input lets a signer cover the received bytes without re-encoding them. `src/spine-mesh/src/signed_frame.rs` already signs `wire::encode` output, and that output is not deterministic (above).

### C.6 First frames: HELLO and HELLO_ACK

The client sends `HELLO` as its first frame, and the server answers with `HELLO_ACK` before any other frame. The client MUST NOT send any other frame before it has read `HELLO_ACK`. This costs one round trip; sending `REQUEST` frames optimistically is deferred (G10). The server closes a connection that has not completed a `HELLO` within 10 seconds (the existing handshake deadlines are 10 s, `main.rs:2299, 2444, 2479`). The deadline is a proposal and must be configurable: `scripts/smoke_candidate.py:303-318` holds idle sockets for several seconds.

`HELLO` body (keys shown in the deterministic order for these names; unknown keys ignored):

| Key | Type | Meaning |
| --- | --- | --- |
| `versions` | array of unsigned integers, strictly ascending | frame versions the client can speak |
| `max_frame` | unsigned integer, 1024 to 16,777,216 | the largest frame the client will accept |
| `extensions` | array of text, no duplicates | extension ids the client offers (C.10), possibly empty |
| `agent` | text, at most 128 bytes, optional | software name and version, informational |

`HELLO_ACK` body:

| Key | Type | Meaning |
| --- | --- | --- |
| `version` | unsigned integer | the selected version; must be one of the offered ones that the server supports |
| `max_frame` | unsigned integer | `min(client max_frame, server maximum)`; applies to frames in both directions from now on |
| `max_inflight` | unsigned integer, at least 1 | the number of requests the server will process concurrently per connection (R13) |
| `extensions` | array of text | the accepted subset of the offered extensions, in the order that fixes the extension index (C.10) |
| `agent` | text, optional | informational |

If the server supports no offered version it sends `CLOSE` with code `unsupported_version` and `supported` listing its versions, then closes. The `HELLO` frame itself always has version byte 1, so that negotiation survives later versions.

Reuse of existing types: `negotiation.rs` (`VersionOffer`, `VersionResponse`, `ProtocolFeature`) is the nearest existing code and does not fit. It is JSON (`negotiation.rs:279-319`), its magic and major/minor fields duplicate the frame header, its feature enum names the research features this proposal removes, and `evaluate_offer` ignores the client's minimum (A.10). `CapabilityQuery` and `CapabilityAdvertisement` (`agentic.rs:224-281`) advertise tools by URI with JSON schemas (layer 5 discovery); they are not transport capabilities and stay as a `REQUEST` command and its reply, usable after the handshake. The proposal replaces `negotiation.rs`.

### C.7 Error behaviour

| Condition | Outcome |
| --- | --- |
| `length` below 11 or above the maximum | close; best-effort `CLOSE(frame_too_large)`; body not read |
| Bad `version`, or `HELLO` offering no supported version | `CLOSE(unsupported_version)` then close |
| Flag bits 1 to 7 set; compressed-length rules violated | `CLOSE(malformed_frame)` then close |
| Unknown frame type (unallocated or `0x00`) | `CLOSE(unknown_frame_type)` then close. Skipping would keep the byte stream in sync but not the state machines (a skipped `STREAM_END`). Types are added by a new version or an extension, never silently. |
| A known type in the wrong direction, a `REQUEST` with id 0, a reused in-flight id, a frame other than `HELLO` first, a second `HELLO`, an `EXT` for an extension that was not accepted or an index out of range | `CLOSE(protocol_violation)` then close |
| Body is not well-formed CBOR, or fails any rule of C.5 (indefinite length, non-shortest, unsorted or duplicate keys, trailing bytes, depth) | `CLOSE(malformed_frame)` then close |
| `REQUEST` body is deterministic CBOR but names an unknown command variant | `RESPONSE` with `err.code = "unknown_command"`, same id; connection stays open |
| `REQUEST` body is a known command with a missing, unknown or mistyped field | `RESPONSE` with `err.code = "invalid_request"`, same id; stays open |
| The same schema failure in a `RESPONSE`, `STREAM_*`, `PONG` or `HELLO_ACK` | `CLOSE(protocol_violation)` then close: the receiver cannot recover the exchange |
| Requests in flight already at `max_inflight` | `RESPONSE` with `err.code = "overloaded"`, same id, request not executed; stays open |
| `STREAM_CANCEL` for an id that is unknown or already ended | ignored (it races with `STREAM_END`) |
| Per-command failures (rate limit, not found, policy denial) | `RESPONSE` with `err`, stays open |
| No `HELLO` within the deadline; idle beyond the configured idle timeout | `CLOSE(timeout)` then close |
| Server shutting down | `CLOSE(shutting_down)` then close |

`CLOSE` body: `{code: text, message: text}` and, for `unsupported_version`, `supported: [uint, ...]`. Codes: `unsupported_version`, `unknown_frame_type`, `malformed_frame`, `frame_too_large`, `protocol_violation`, `timeout`, `shutting_down`. A peer MUST NOT rely on receiving `CLOSE`. `RESPONSE.err`: `{code, message[, details]}`, which is the shape of the existing `ToolOutcome::Err` (`agentic.rs:81-96`).

Strictness is deliberate. The as-built receiver accepts anything it can parse, and an unparsable message ends the connection with no code (A.6). v1 makes the two classes explicit: byte-level non-conformance ends the connection with a reason; a well-formed request the server cannot serve is an error response.

### C.8 Requests, responses and concurrency

- A client request id is a nonzero `u64` the client chooses (a counter from 1 is fine). It names the request until the reply (`RESPONSE`, or `STREAM_END` for a stream) is received. Using an id that is still in flight closes the connection. The two directions have independent id spaces: a `PING` from the server carries a server-chosen id and its `PONG` carries the same id back.
- Id 0 is for frames that do not start an exchange: `HELLO`, `HELLO_ACK`, `CLOSE`, and extension notifications.
- A server MAY run requests concurrently and answer in any order, up to `max_inflight`. A server that processes one request at a time (the as-built daemon does, `main.rs:2672-2685`) is conformant. The frame layer defines no priority and no flow-control window; head-of-line blocking at the transport (TCP) is not addressed (C.11).
- Because ids replace the string ids inside bodies, `Request.id`, `ToolCall.id`, `CapabilityQuery.id` and the matching fields of `ToolResult`, `CapabilityAdvertisement` and the stream structs are removed in v1 (D).

### C.9 Streaming

A stream is the reply to a `REQUEST` whose command is defined to stream (none is implemented today; see D and G2). The id of the stream is the id of the request.

1. The server sends `STREAM_START` (type `0x20`), then zero or more `STREAM_TOKEN` (`0x21`) with `seq` starting at 0 and increasing by one, then `STREAM_END` (`0x22`). All carry the request id. The request receives no `RESPONSE` if `STREAM_START` was sent; if the request fails before that point the server sends `RESPONSE` with `err`.
2. The bodies are the existing `StreamStart`, `StreamToken`, `StreamEnd` minus their `id` field (`agentic.rs:104-218`). `StreamEnd.reason` keeps the `StreamEndReason` set; `usage` stays optional. A `seq` gap or repeat is `protocol_violation`: the transport is reliable and ordered, so a gap is a bug.
3. The client cancels with `STREAM_CANCEL` (type `0x23`, same id, optional `reason`). The server stops producing as soon as it reads the frame and sends `STREAM_END` with reason `cancelled`; tokens already in flight may still arrive before it. Cancel latency is one frame plus the time for the server to poll its input between tokens, which a server that processes frames one at a time cannot meet during a long computation (R7 needs a server that reads while producing).
4. Interleaving: frames of several streams and responses may interleave; `seq` is per stream.
5. `StreamData::Encoded` and `StreamStart.stream_codec` belong to the neural-codec extension (D). `StreamStart.decode_hints` is defined in `agentic_codec.rs`, the neural module (`agentic.rs:42` imports it); where it lives is open (G).

### C.10 Extensions

An extension id is an opaque, case-sensitive ASCII text of at most 128 bytes, conventionally `<domain>/<name>/<major>`, for example `nervosys.org/neural-codec/1`. A change that is not backward compatible changes the major number and so the id. Ids starting with `spine.` are reserved for extensions specified in the SPINE specification set.

Negotiation: the client lists offered ids in `HELLO.extensions`; the server answers `HELLO_ACK.extensions` with the subset it accepts. The position in that array is the extension index for the connection. Only accepted extensions may be used.

`EXT` frame (`0x80`): body `[index, kind, payload]`, `index` an index into `HELLO_ACK.extensions`, `kind` an unsigned integer defined by the extension, `payload` any deterministic CBOR item. The request id follows the extension's own rules; 0 is allowed. An `EXT` frame whose index is out of range, or whose kind is unknown to a negotiated extension, closes the connection (`protocol_violation`). An extension specification must define its kinds, its payloads and its error handling; this document defines none.

The core `REQUEST` command set and the core frame types are closed. An extension cannot add a variant to them. This is how the core enums never carry research variants (R9).

### C.11 What the format does not provide

- Confidentiality, integrity, peer authentication, replay protection, forward secrecy: none. The frame is plaintext inside whatever carries it.
- Resumption, migration, multi-stream flow control, priorities, head-of-line-blocking avoidance: none. One connection is one TCP or QUIC stream.
- Authority: no capability chain, no delegation, no signed receipts (layer 3 and 5 designs). There is no header field for them; adding one is a new version (G8).
- Content-level provenance, effect classes, dry-run and idempotency markers (P3, P5): not in the frame, to be defined with layers 4 and 5.

Where security comes from today and the open question. As built, confidentiality and authentication are TLS 1.3 through rustls on the TCP and WebSocket listeners, off by default and required off-loopback unless a flag is set (`config.rs:414-416`, `main.rs:2441-2456`), with optional X.509 client certificates; there is no SPINE identity on this path. QUIC is unauthenticated and the repository's client verifier accepts any certificate. The mesh path instead runs the `spine-session` handshake (ML-KEM-768 key exchange, Ed25519 identities, AES-256-GCM records), which has had no external review (`src/spine-session/src/lib.rs:1-9`, `HANDSHAKE-REVIEW.md`).

Recommendation (question G1): run the native wire over the `spine-session` handshake once it has been independently reviewed, and keep TLS 1.3 as the interim binding. Reasoning:
1. P3 and P2 need requests bound to a key the principal and name layers know; X.509 binds to a host name and a certificate authority, neither of which the stack otherwise has (`FIRST_PRINCIPLES.md` layer 1: "authenticated by the name itself with no certificate authority").
2. One channel design for client and mesh removes a second security review.
3. Costs: every client needs a key (the initiator signs `msg1` with an Ed25519 key, `handshake.rs:215-219`; an ephemeral per-connection key would give confidentiality without a stable client identity); browsers cannot use it except through a WebSocket bridge; the construction is unreviewed, so TLS stays the fallback until review finishes.
4. The frame layer is agnostic: v1 frames are the plaintext carried inside session records (`Session::seal`/`open`, `handshake.rs:483-509`) or inside a TLS stream. The record's own length prefix then delimits it on the wire and the frame's `length` stays for streams without a record layer.
5. Do not run both: layering TLS under the session handshake would add a certificate requirement without adding an identity.

### C.12 Worked examples

Both are hand-computed, unverified by code. They were cross-checked with a throwaway Python encoder written from the rules above, kept outside the repository; no SPINE code was run, and they have not been checked against any Rust implementation (none exists).

Example 1: the client's `HELLO` offering version 1, a 16 MiB frame limit and no extensions. Body is the map `{"versions": [1], "max_frame": 16777216, "extensions": []}`. Keys sorted by encoded bytes: `versions` (`68 ...`, 8 characters), `max_frame` (`69 ...`, 9), `extensions` (`6a ...`, 10).

```
00 00 00 32                     length = 0x32 = 50 = 11 + 39
01                              version = 1
01                              type = HELLO
00                              flags = 0
00 00 00 00 00 00 00 00         request id = 0
a3                              map(3)
  68 76 65 72 73 69 6f 6e 73      "versions"
  81 01                           [1]
  69 6d 61 78 5f 66 72 61 6d 65   "max_frame"
  1a 01 00 00 00                  16777216
  6a 65 78 74 65 6e 73 69 6f 6e 73  "extensions"
  80                              []
```

Total 54 bytes: `00 00 00 32 01 01 00 00 00 00 00 00 00 00 00 a3 68 76 65 72 73 69 6f 6e 73 81 01 69 6d 61 78 5f 66 72 61 6d 65 1a 01 00 00 00 6a 65 78 74 65 6e 73 69 6f 6e 73 80`.

Example 2: `REQUEST`, request id 1, calling the tool `spine.content/hash` with `{"text": "hi"}`. The command is externally tagged with the proposed wire name `tool_call`; the inner map's keys sort `args` (`64 61 ...`) before `name` (`64 6e ...`).

```
00 00 00 3d                     length = 0x3d = 61 = 11 + 50
01                              version = 1
10                              type = REQUEST
00                              flags = 0
00 00 00 00 00 00 00 01         request id = 1
a1                              map(1)
  69 74 6f 6f 6c 5f 63 61 6c 6c   "tool_call"
  a2                              map(2)
    64 61 72 67 73                  "args"
    a1 64 74 65 78 74 62 68 69      {"text": "hi"}
    64 6e 61 6d 65                  "name"
    72 73 70 69 6e 65 2e 63 6f 6e 74 65 6e 74 2f 68 61 73 68   "spine.content/hash" (18 bytes)
```

Total 65 bytes: `00 00 00 3d 01 10 00 00 00 00 00 00 00 00 01 a1 69 74 6f 6f 6c 5f 63 61 6c 6c a2 64 61 72 67 73 a1 64 74 65 78 74 62 68 69 64 6e 61 6d 65 72 73 70 69 6e 65 2e 63 6f 6e 74 65 6e 74 2f 68 61 73 68`.

For comparison, the as-built `Ping` of A.2 is 19 bytes of CBOR plus 8, wrapped to 177 bytes of JSON, then compressed and framed. A v1 `PING` with token 42 and request id 7 is `00 00 00 0d 01 04 00 00 00 00 00 00 00 00 07 18 2a`: 4 + 11 + 2 = 17 bytes (also hand-computed, unverified by code).

### C.13 Deviations from the brief, and why

1. `HELLO` is always version 1 and negotiates; the other frames carry the selected version. The brief fixed `version = 1`; this keeps the one-byte version useful once a second version exists.
2. Compressed bodies carry a 4-byte `uncompressed_len` prefix. This lets the receiver reject a bomb before inflating and allocate exactly. The brief asked for a bomb bound but not a mechanism.
3. A `max_frame` is negotiated, below an absolute 16 MiB ceiling, and a 64 KiB limit applies before `HELLO`. The brief asked for a stated maximum; the pre-`HELLO` limit keeps an unauthenticated peer from forcing large allocations.
4. Correlation ids replace the string ids inside bodies (C.8), and `PING`/`PONG` carry ids. The brief asked for a header id; the consequence is to delete the redundant body ids (D).
5. Structs are closed and the CBOR rules are stricter than RFC 8949 section 4.2 alone (no tags, depth bound, absent rather than null optionals, bytes as byte strings). This is for R4 (one encoding per value).
6. Unknown frame type closes the connection; unknown command in a `REQUEST` is an error response. The brief asked for both to be defined; the split is by whether the byte stream and state machines can continue.
7. One frame type for streams per phase, with the stream id equal to the request id, instead of a separate stream-id space.

## D. Message set v1

### D.1 `Message` variants (24)

Disposition: Keep (stays, in the form shown), Extension (moves out of the core, named), Delete (nothing sends or consumes it).

| # | Variant | Disposition | v1 form |
| --- | --- | --- | --- |
| 1 | `Request` | Keep | frame type `REQUEST` (0x10); body is the command; `id` moves to the header |
| 2 | `Response` | Keep | frame type `RESPONSE` (0x11); `{ok: any}` or `{err: {code, message[, details]}}`; the `Option<Value>` plus `Option<String>` pair is replaced |
| 3 | `Event` | Delete | only the decoy path sends it. The type number 0x12 stays unallocated. |
| 4 | `BinaryProgram` | Delete | nothing sends it; `SpineBinary` stays as the argument of `ExecuteBinary` |
| 5 | `LatentMessage` | Extension `nervosys.org/neural-codec/1` | the daemon's `ExecuteBinary` latent output (`main.rs:2704-2716`) is the one live sender; without the extension negotiated the daemon sends nothing (G9) |
| 6 | `Sync` | Delete | |
| 7 | `Speculative` | Delete | |
| 8 | `PreComputed` | Delete | |
| 9 | `MorphRequest` | Delete | |
| 10 | `Ping` | Keep | `PING` (0x04) |
| 11 | `Pong` | Keep | `PONG` (0x05) |
| 12 | `ToolCall` | Keep | command `tool_call` `{name, args[, trace]}` in a `REQUEST` |
| 13 | `ToolResult` | Keep | `RESPONSE`; `trace` is not echoed |
| 14 | `StreamStart` | Keep (specified, unimplemented) | `STREAM_START` |
| 15 | `StreamToken` | Keep (specified, unimplemented) | `STREAM_TOKEN` |
| 16 | `StreamEnd` | Keep (specified, unimplemented) | `STREAM_END` |
| 17 | `StreamCancel` | Keep (specified, unimplemented) | `STREAM_CANCEL` |
| 18 | `CapabilityQuery` | Keep | command `capability_query` `{selector}` |
| 19 | `CapabilityAd` | Keep | the `ok` value of its `RESPONSE`: `{agent_id, capabilities}` |
| 20 | `Encoded` | Extension `nervosys.org/neural-codec/1` | kind 1 |
| 21 | `CodecAd` | Extension `nervosys.org/neural-codec/1` | kind 2 |
| 22 | `CodecNegotiation` | Extension `nervosys.org/neural-codec/1` | kind 3 |
| 23 | `EmbeddingRequest` | Extension `nervosys.org/neural-codec/1` | kind 4 |
| 24 | `EmbeddingResponse` | Extension `nervosys.org/neural-codec/1` | kind 5 |

Count: 12 kept (of which 6 become frame types or commands rather than enum variants and 4 are specified but have no implementation), 6 moved to an extension (kinds 1 to 5 plus `LatentMessage` as kind 6), 6 deleted. 12 + 6 + 6 = 24. The streaming types are kept because token streaming and cancel are named premises (P4, P8), the gateway already maps these structs to SSE (`agentic_sse.rs`), and deleting them would discard the one design for them; the cost is four frame types with tests and no sender until Stage 9. If the owner prefers the strict "nothing sends it" rule, the count becomes 8 kept, 6 extension, 10 deleted.

### D.2 `BrowserCommand` variants (35)

| # | Variant | Disposition | Note |
| --- | --- | --- | --- |
| 1 | `Navigate` | Keep | wire name `navigate` |
| 2 | `GetUR` | Keep | `get_ur` |
| 3 | `GetRawHTML` | Keep | `get_raw_html` |
| 4 | `Click` | Delete | stub |
| 5 | `Type` | Delete | stub |
| 6 | `ExecuteBinary` | Keep | `execute_binary`; the panic on failure (A.10) is fixed independently of the wire |
| 7 | `HandleEvent` | Delete | no sender but examples |
| 8 | `GetLatentUR` | Extension `nervosys.org/research-session/1` | |
| 9 | `Morph` | Delete | with morphology (E) |
| 10 | `Search` | Delete | returns a canned result |
| 11 | `TransferSession` | Extension `nervosys.org/research-session/1` | |
| 12 | `StoreKnowledge` | Keep | |
| 13 | `QueryKnowledge` | Keep | |
| 14 | `DeleteKnowledge` | Delete | no sender; return it with a designed knowledge object |
| 15 | `GetSessionHistory` | Delete | no sender but examples |
| 16 | `GetCapabilities` | Keep | rename to `get_binary_capabilities`: it lists the loaded program's capabilities, not the origin's tools |
| 17 | `SetAutonomousMode` | Extension `nervosys.org/research-session/1` | its reader is research-gated |
| 18 to 22 | `SwarmSearch`, `DelegateTask`, `ProposeKnowledge`, `CreateSwarmPlan`, `ExecutePlanTask` | Delete | refused in every build |
| 23 | `NeuralTransmit` | Extension `nervosys.org/research-session/1` | |
| 24 | `GetAgenticState` | Extension `nervosys.org/research-session/1` | |
| 25 | `SendSpeechAct` | Delete | refused in every build |
| 26 to 35 | `ResolveName`, `ResolveNames`, `FindProviders`, `PublishName`, `FetchName`, `CrawlNames`, `PublishResource`, `ReadResource`, `NamespaceStatus`, `MeshCall` | Keep | |

Count: 17 kept, 5 moved to an extension, 13 deleted (17 + 5 + 13 = 35). The command set of v1 is the 17 kept plus `tool_call` and `capability_query` from D.1: 19 commands. `SwarmPlan`, `PlanTask`, `PlanStatus`, `TaskStatus` in `lib.rs:715-749` are used by `research/spine-cluster/src/lib.rs` and `research/spine-browser/src/main.rs` (`spine_protocol::SwarmPlan`, `TaskStatus`); they are not wire types after the commands go and can stay or move with the cluster crate (F, step 7).

Not message types but part of the HLB program format and unaffected by this table: `Instruction::MorphProtocol`, `Decoy`, `StreamLatent` (`lib.rs:398-410`), consumed in `src/spine-core/src/vdom.rs:311` and `src/spine-wasm/src/lib.rs:403`.

## E. Morphology, chameleon, speculation

| Feature | Status under v1 | Consequence |
| --- | --- | --- |
| Morphology (`ProtocolMorphology`, `PaddingMode`, header size, endianness, `MorphRequest`, `Morph`, `morph_now`, the daemon's 30 s monitor) | Removed. It changes framing mid-connection, which R1 forbids, so it cannot be layered above the frame. Without a chameleon it uses an all-zero key (A.4). | The framing is the 15-byte header of C.1. The `PROTOCOL_MORPHS` metric (`main.rs:2691`) and the Grafana panel that uses it (`deploy/grafana/spine-dashboard.json`) are removed. |
| Padding | Removed (it is part of morphology). Traffic-shape hiding, if wanted, is a frame-level option for the session layer or an extension. | `apply_padding`/`remove_padding` go. |
| Speculation (`SpeculativeFrame`, `SpeculativePayload`, `SpeculationStats`, the predictors, `precompute_response`, `speculate_responses`, `Message::Speculative`, `PreComputed`, `Sync`) | Removed. It is dead in a default build and half-dead in a research build (A.5); I did not look for a measured benefit and did not find one cited in the places I read. | `get_speculation_stats` disappears from the SDK and from three CLI commands. |
| Chameleon (`ChameleonKey`, `enable_chameleon*`, `latent_aead`, `LatentVector` bytes) | Not in the core. Its own source says it is not confidentiality (`lib.rs:113-120`). If kept, it is a research crate that depends on the public frame API and, if it needs to transform bodies, defines an extension (`EXT`). It cannot change framing. | `src/spine-protocol/src/research.rs` leaves the crate; the `research` feature and the optional `spine-neural` and `spine-crypto` dependencies of `spine-protocol` go. |
| Pre-shared-key AES-GCM layer (`enable_encryption`) | Removed. No production caller; confidentiality comes from the session or TLS layer (C.11). | `cipher` and `fresh_nonce` go from the handler. |
| Decoy traffic (`send_decoy`) | Removed with `Event`. | |

Consequence for `ProtocolHandler` (`handler.rs`, 1,222 lines): it ceases to exist in its current form. Its replacement is a connection type that owns the stream, the negotiated limits (`max_frame`, `max_inflight`, accepted extensions), and a frame reader and writer implementing C.1 to C.4. It holds no predictor, no cache, no morphology, no cipher and no statistics. `AgentClient.handler` is a public field (`spine-agent/src/lib.rs:21-23`), so its type changes for SDK users; nothing outside the repository is known to depend on it.

## F. Migration inventory

Constraints from CI (`.github/workflows/ci.yml`): `cargo fmt --check`; `cargo clippy --workspace --all-targets -D warnings` (the job's `RUSTFLAGS` also has `-D warnings`); the same clippy for `spine-protocol` with `--no-default-features`, with `--no-default-features --features quic`, and with `--features research`; for `spine-web`, `spine-core`, `spine-agent`, `spine-gateway` with `--features research`; for `spine-mesh` alone (`ci.yml:44-71`); the `research` job runs clippy and tests over `research/` excluding `spine-browser` (`:73-110`); tests on Linux, macOS and Windows for the workspace libs, integration tests and doc tests, plus `spine-protocol --features research` tests on Linux only (`:112-169`); `cargo doc --workspace --no-deps` (`:340-341`). `research/spine-cluster` and `research/spine-human` depend on `spine-protocol` (`research/spine-cluster/Cargo.toml:15`, `research/spine-human/Cargo.toml:16`), so the research job re-compiles against every protocol change. `spine-python`, `spine-js` and `spine-browser` are not built by any job.

No step may be compiled locally. Each is sized to be reviewed by reading. Steps 1 to 3 are additive. Steps 4 to 8 delete and are independent of the new frame. Step 9 is additive and step 10 is the cut-over.

| # | Name | Files | What changes | Tests to rewrite or add | Risk |
| --- | --- | --- | --- | --- | --- |
| 1 | `cbor-core` | new `src/spine-protocol/src/cbor.rs`; `lib.rs` (add `pub mod cbor;`) | `to_vec`, `from_slice` and `check_deterministic` per C.5; no caller yet. Sorting pass over `ciborium::Value` or a hand encoder. | New unit tests: RFC 8949 Appendix A examples, the C.12 bodies, a rejected-input corpus (indefinite length, non-shortest integer, unsorted keys, duplicate keys, trailing bytes, depth 33), float cases. | Low: additive. The first compile happens in CI, so keep it to one file. The unverified ciborium behaviour is exactly what the new tests establish. |
| 2 | `frame-codec` | new `src/spine-protocol/src/frame.rs`; `lib.rs` | `FrameHeader`, `FrameType`, `encode_frame`, `decode_frame`, async `read_frame`/`write_frame` with the checks of C.1 and C.4, the error enum with the close codes of C.7. Streaming frame types exist as constants only. No caller. | New: the two hex vectors of C.12 decoded and re-encoded byte for byte; oversize length reads no body (a reader that panics on read); flag bits; every row of C.7 that concerns framing; compressed body with a lying `uncompressed_len`. Add a proptest round trip. | Low: additive. |
| 3 | `hello-close` | `frame.rs` or new `handshake.rs`; delete `src/spine-protocol/src/negotiation.rs` and `pub mod negotiation` (`lib.rs:7`); `fuzz/fuzz_targets/fuzz_negotiation.rs` | `Hello`, `HelloAck`, `Close` bodies and `negotiate(hello, local) -> Result<Ack, Close>`. Remove the unused JSON negotiation. Retarget the fuzz target at `check_deterministic` and `decode_frame`. | Replace the unit tests inside `negotiation.rs` (its 15 `#[test]` functions) with: no shared version, extension subset, unknown `HELLO` keys ignored, `max_frame` clamp. | Low. Removing a `pub mod` is an API break for nobody in the workspace (only the fuzz crate uses it, and the fuzz crate is not in CI). |
| 4 | `drop-speculation` | `handler.rs` (`SpeculativePredictor` stand-in 27-63, caches, `enable_speculation`, `precompute_response`, `get_precomputed`, `speculate_responses`, `get_speculation_stats`, `classify_message`, `MessageType`, the wrapper in `send_message`, the `SpeculativeFrame` branch in `receive_message`); `lib.rs` (`SpeculativeFrame`, `SpeculativePayload`, `Message::{Speculative, PreComputed, Sync}`, `PreComputedResponse`, `SyncPayload`); `research.rs` (`SpeculativePredictor` and its users); `spine-core/src/main.rs:2718-2754`; `spine-agent/src/lib.rs` (`get_speculation_stats`); `spine-cli/src/commands/init.rs:228`, `connect.rs:112`, `benchmark.rs:97`; `spine-agent/examples/{simple_agent,encrypted_agent,agent_autonomous,research_swarm}.rs` (`enable_speculation` calls); `fuzz` unaffected. | The sender emits the `SP` body directly (today's `send_message_raw` format); the receiver drops the JSON-wrapper branch. | `handler.rs` tests `a_default_handler_emits_a_full_speculative_frame` (delete) and `handlers_exchange_small_and_large_messages_both_ways`, `handlers_stay_in_sync_when_both_morph_with_one_seed`, `a_receiver_that_did_not_morph_cannot_read_the_next_frame` (they decode the wrapper in a helper; adjust); `wire.rs` `encode_never_compresses_but_encode_compressed_can` (uses `SyncPayload`; switch to another large message); `tests/proptest_protocol.rs` (`arb_message` and the chameleon block); `tests/e2e_protocol.rs` `e2e_all_command_types_over_tcp`; `tests/integration.rs` `protocol_all_command_types`. | Medium: wide but mechanical. Both research and default clippy configurations must still compile: the `#[cfg(feature = "research")]` branches in `handler.rs` and `research.rs` change in the same commit. |
| 5 | `drop-morphology` | `handler.rs` (`ProtocolMorphology`, `PaddingMode`, `apply_padding`, `remove_padding`, `morph_now`, `write_morphed_frame` becomes a fixed 5-byte header `u32_be length || 0x00`, the `MorphRequest` handling at 802-821); `lib.rs` (`Message::MorphRequest`, `BrowserCommand::Morph`); `spine-core/src/main.rs` (`needs_morph` at 48 and 72, the monitor task 2230-2250, the proactive block 2636-2650, the `Morph` interception 2656-2671 and 2689-2692, the `PROTOCOL_MORPHS` metric, the dead arms at 1351 and 1612); `spine-agent/src/lib.rs` (`morph`); `spine-ffi/src/lib.rs:315-336` and `include/spine.h:79-83`; `src/spine-go/spine.go:172-` (`Morph`); `src/spine-python/src/lib.rs:223-` (`morph`); `spine-cli/src/commands/connect.rs` (the `morph` command); `deploy/grafana/spine-dashboard.json`; `research.rs` (every use of `morphology`). | The header is fixed; there is no padding. | `handler.rs` `test_protocol_morphology`, `handlers_stay_in_sync_when_both_morph_with_one_seed`, `a_receiver_that_did_not_morph_cannot_read_the_next_frame` (delete); `tests/e2e_protocol.rs` `e2e_morph_mid_session`; `tests/integration.rs` `protocol_morph_changes_morphology`; `tests/proptest_protocol.rs` (`arb_message` `MorphRequest` line); `src/spine-ffi/src/lib.rs` `test_morph_null_handle`; `src/spine-core/tests/agent_web.rs` `configured_mutual_tls_protects_tcp_and_websocket_origins` (remove the `agent.morph()` steps at 663-665); `src/spine-agent/src/lib.rs` tests that construct `BrowserCommand::Morph` (`test_request_response_protocol`, `test_message_variants`). | Medium. The Go file and `spine.h` are not compiled by root CI, so a stale reference there would not fail CI; review them by reading. |
| 6 | `relocate-research` | `src/spine-protocol/src/research.rs` (982 lines) to a new crate in `research/` (name open, G6) or deleted; `handler.rs` (`chameleon_*` methods, `enable_chameleon*`, `latent_aead`, `enable_encryption`, `cipher`, `fresh_nonce`, `send_decoy`); `src/spine-protocol/Cargo.toml` (`research` feature, the optional `spine-neural` and `spine-crypto` dependencies, the `neural_codec_bench` entry, the `chameleon_demo` example); `agentic_codec.rs` (`TitansLatentCodec` gate moves with it); `spine-core`, `spine-agent`, `spine-gateway`, `spine-web` `Cargo.toml` (drop `spine-protocol/research` from their `research` features); `.github/workflows/ci.yml` (remove the two `spine-protocol ... research` steps at 55-56 and 166-169); `spine-agent/examples/{simple_agent,encrypted_agent,agent_autonomous,research_swarm,offline_demo}.rs` (use `enable_chameleon*`); `spine-protocol/examples/chameleon_demo.rs`; `spine-protocol/benches/neural_codec_bench.rs`. | The core crate loses every research dependency. | Delete `tests/integration.rs` `protocol_chameleon_encoding` (165 cfg), `tests/e2e_protocol.rs` `e2e_chameleon_tcp_send_path` (248 cfg), `tests/proptest_protocol.rs` `chameleon_encode_produces_output` and `chameleon_epoch_monotonic` (162 cfg), and the PSK tests `protocol_encrypted_roundtrip`, `protocol_encrypted_multiple_messages`, `e2e_encrypted_tcp_roundtrip`, `e2e_encrypted_multiple_messages`, `chaos_encrypted_connection_drop`, `chaos_mismatched_encryption_keys`. The new research crate gets the moved tests. | High. It is the largest diff and touches CI configuration, three Cargo manifests and the research workspace lockfile. If the owner prefers deletion over relocation, the risk drops to medium. Keep `quic` as the only feature of `spine-protocol`. |
| 7 | `trim-commands` | `lib.rs` (`BrowserCommand`: delete 13 variants, move 5 per D.2, rename `GetCapabilities`); `spine-core/src/main.rs` (`handle_command` arms, the unsupported arms at 1557-1565 and 1610-1623, `last_command` bookkeeping); `spine-agent/src/lib.rs` (delete `click`, `type_text`, `handle_event`, `search`, `delete_knowledge`, `get_history`, the five swarm methods, `send_speech_act`; the five extension methods leave the SDK with the extension); `spine-ffi/src/lib.rs` (`spine_search`) and `include/spine.h`, `spine.go`; `src/spine-python/src/lib.rs` (`click`, `type_text`, `search`, `get_latent_ur`); `spine-cli/src/commands/{init,query,connect,benchmark}.rs`; `spine-gateway/src/main.rs:430` if it calls `search` on the SDK; `research/spine-browser/src/main.rs` (not in CI, will break; list in the PR); `spine-agent/examples/*`. | The command set is the 17 kept plus the two from D.1. | `tests/agent_web.rs` `signed_mesh_tasks_dependencies_knowledge_restart_and_authorization` (the refusal assertions at 462-478 change from "`unsupported_*` error" to a decode failure or are deleted); `tests/e2e_protocol.rs` `e2e_all_command_types_over_tcp`; `tests/integration.rs` `protocol_all_command_types`; `tests/proptest_protocol.rs` `arb_browser_command`; `src/spine-ffi/src/lib.rs` tests that call the removed functions; `src/spine-agent/src/lib.rs` `test_message_variants`. | Medium. Many call sites; a missed one fails a clippy configuration. `rg` for each removed SDK method name before review. |
| 8 | `trim-messages` | `lib.rs` (`Message`: delete `Event`, `BinaryProgram`; move `LatentMessage`, `Encoded`, `CodecAd`, `CodecNegotiation`, `EmbeddingRequest`, `EmbeddingResponse` out); `agentic_codec.rs` and `agentic.rs` (`StreamData::Encoded`, `StreamStart.stream_codec`, `decode_hints` placement); `spine-core/src/main.rs:2704-2716` (the latent stream, now absent); `spine-agent/src/lib.rs` (`send_request` forwarding of `LatentMessage` and `Event`, `start_listener`, `subscribe_latent`); `spine-cli/src/commands/benchmark.rs` (latent measurement); `spine-protocol/benches/hot_path_bench.rs` and `examples/wire_sizes.rs` (use removed variants); `spine-gateway`, `spine-grpc` only if they name the moved types. | `Message` shrinks to the 12 kept variants. | `tests/wire_encoding.rs` (`representative_frames_roundtrip` and the size tests use `Encoded`); `tests/integration.rs` `protocol_plaintext_latent_vector`; `tests/proptest_protocol.rs` (`arb_message`, `message_json_roundtrip`); `src/spine-agent/src/lib.rs` `test_latent_message`; `src/spine-protocol/src/agentic.rs` `message_envelope_dispatch`. | Medium. |
| 9 | `connection` | new `src/spine-protocol/src/connection.rs`; `lib.rs` | `Connection<S>` owning `S`, the negotiated limits and the frame reader and writer; `client_handshake` and `server_handshake` per C.6; request id table, in-flight cap, `PING`/`PONG`, `CLOSE` on every violation row of C.7. Still no caller in the daemon or SDK. | New, over `tokio::io::duplex`: one test per row of C.7; two requests in flight answered out of order; `max_inflight + 1`; HELLO deadline with paused time; stream start, tokens, cancel, end (the daemon does not produce them; the test uses a scripted peer). | Low to medium: additive but the first user of async I/O in the new code. |
| 10 | `cutover` | `spine-core/src/main.rs` (`handle_session` 2519-2763, the three listener spawns at 2279-2323, 2441-2456 and 2476-2487, `handle_websocket` 2500-2517); `spine-agent/src/lib.rs` (`AgentClient` fields and the five `connect*` constructors, `send_request`, `call_tool`, `capabilities`, `ping`; the public `handler` field becomes `conn`); delete `handler.rs` and the `SP`-header functions `wire::{encode, encode_compressed, encode_json, decode}`, `SpineWireHeader`, the headerless-JSON fallback, `FORMAT_*`; `replay.rs:359-368` (`replay_sends` uses `send_message_raw`); `spine-mesh/src/signed_frame.rs` if it still calls `wire::encode` (step 12); `spine-protocol/examples/{wire_sizes,mcp_stdio_server}.rs`; `spine-protocol/benches/hot_path_bench.rs`. The daemon answers `HELLO`, applies `max_inflight` of 1 at first (sequential, as now) and closes with codes per C.7. Bodies are still the current structs with ids in them; header ids are set and ignored on receive. | The wire changes in one commit. | `tests/chaos.rs` all 11 (`chaos_connection_drop_mid_session`, `chaos_abrupt_client_drop`, `chaos_corrupted_frame_header`, `chaos_truncated_message`, `chaos_zero_length_message`, `chaos_rapid_reconnection`, `chaos_many_small_messages`, `chaos_large_payload`, `chaos_bidirectional_flood`, plus the two encrypted ones deleted in step 6); `tests/e2e_protocol.rs` (`e2e_tcp_request_response`, `e2e_tcp_ping_pong`, `e2e_concurrent_tcp_sessions`, `e2e_rapid_reconnect`, `e2e_stress_tcp_100_messages`, `e2e_all_command_types_over_tcp`, and `spawn_echo_server`); `tests/integration.rs` (all remaining); `src/spine-core/tests/agent_web.rs` (`connect` helper 419-444 now waits for `HELLO_ACK`, which makes it a better readiness probe; `configured_mutual_tls_protects_tcp_and_websocket_origins` constructs `ProtocolHandler` at 672; `native_idle_connections_release_global_admission` at 592 relies on `idle_timeout_secs = 6` and an `excess` client that must see an error); `src/spine-agent/src/lib.rs` tests `test_connect_invalid_address`, `test_connect_with_retry_fails_fast`. | High. It is the one commit where every client and the daemon must change together. The idle test, the admission test and `scripts/smoke_candidate.py` (idle sockets held up to 8 s while the HELLO deadline is 10 s) are the likeliest to regress. |
| 11 | `bodies-v1` | `lib.rs`, `agentic.rs`, `resource.rs`, `main.rs`, SDK | Remove `Request.id`, `ToolCall.id`, `CapabilityQuery.id` and their echoes; replace `Response` with `{ok}`/`{err}` and replace free-text errors with codes; route every body through `cbor::to_vec`/`from_slice`; `serde_bytes` on all byte fields; closed structs (`deny_unknown_fields`); snake_case renames; replace the `HashMap` in `SpineBinary`. Add the stream frames to the daemon's vocabulary but not a producer. | `tests/wire_encoding.rs` (rewrite on the new bodies), `tests/proptest_protocol.rs` (add `decode(encode(x)) == x` and `encode(decode(b)) == b`), `src/spine-protocol/src/resource.rs` `maximum_upload_fits_the_native_frame_limit` (asserts the CBOR size of `PublishResource` with 8 MiB), every SDK test that matches on `Response.error` strings such as `unsupported_speech_act`, `outbound_address_denied` (`agent_web.rs:17-66` asserts substrings of the error text; these become `err.code` assertions). | Medium to high: the error strings are asserted in several acceptance tests. |
| 12 | `signed-frame` | `src/spine-mesh/src/signed_frame.rs`, `src/spine-mesh/src/lib.rs`, `research/spine-agentic/src/lib.rs:83` | Delete (no callers), or re-point at `cbor::to_vec` of a v1 body. | Its five tests (`sign_then_verify_roundtrips`, `tampered_payload_is_rejected`, `swapped_key_is_rejected`, `bad_magic_is_rejected`, `short_buffer_is_rejected`) are deleted or rewritten. | Low. |
| 13 | `bindings-scripts-docs` | `src/spine-ffi/include/spine.h`, `src/spine-go/{spine.go,types.go,spine_test.go}`, `src/spine-python/src/lib.rs`, `extension/*`, `scripts/smoke_candidate.py`, `docs/spec/01-session.md` section 4, `docs/spec/04-objects.md` section 3, `docs/spec/05-interaction.md:17`, `README.md`, `ARCHITECTURE.md`, `docs/src/architecture/protocol.md`, `docs/NATIVE_OPERATIONS.md`, `SECURITY.md`, `BENCHMARK_REPORT.md` | The FFI, Go and Python layers need no wire change after step 10 because they wrap `AgentClient`; they need the removed-command cleanups of steps 5 and 7. The extension speaks JSON to a port with no listener (A.1): delete it or give it a purpose (G11). The smoke script needs a longer or configurable HELLO deadline, or its held sockets must send `HELLO`. Update every document that describes `SpeculativeFrame`, morphology or the `SP` header. | `src/spine-go/spine_test.go`; the smoke script (not run by CI as far as I found; not verified). | Low, except that the Go, Python and extension changes are not covered by CI. |
| 14 | `conformance-vectors` | new `docs/spec/00-wire.vectors.json` (or `.md`), a test in `spine-protocol` that reads it, an independent decoder under `scripts/` | Hex vectors for every frame type and every row of C.7, produced by the reference implementation and re-decoded by a second implementation (R14). An optional CI job runs the second decoder. | New. | Low. |

Reviewability: steps 1 to 3 and 9 are additive and can be read in isolation. Steps 4 and 5 are the largest deletions that touch only the existing wire. Step 6 should be reviewed with `cargo tree` output from CI for each configuration. Step 10 should be the smallest possible cut-over: it keeps the current body types so that 11 is a separate review.

Ordering constraint: steps 4 to 6 must precede 10 because each leaves the old wire working and shrinks what 10 must replace. Step 5 depends on step 6's decision about `research.rs` only in that `research.rs` also reads `morphology`; if step 6 comes first, step 5 does not touch that file.

## G. Open questions for the owner

| # | Question | Recommendation |
| --- | --- | --- |
| G1 | Should the native path run over the `spine-session` handshake instead of TLS? | Yes, after independent review of the handshake; TLS 1.3 until then, and not both (C.11). Clients need an Ed25519 key; an ephemeral one per connection is enough for confidentiality. |
| G2 | Keep the four streaming frame types though nothing implements them? | Keep. They are the design for P4 and P8, and the gateway already maps the structs. Implement a producer in Stage 9. |
| G3 | The 16 MiB frame ceiling is set by the 8 MiB single-frame resource upload. Lower it and chunk uploads as downloads already are (64 KiB)? | Keep 16 MiB for v1; chunked upload is a layer-4 change (Stage 9). Negotiation already lets a deployment lower `max_frame`. |
| G4 | Closed structs (reject unknown fields) or open? | Closed in v1, open only in `HELLO`/`HELLO_ACK` (C.5). Additive evolution goes through extensions or a new version. |
| G5 | Deterministic CBOR: a validator plus a sorting pass over ciborium, or another crate? | Validator plus sorting pass, no new dependency (`docs/DEPENDENCY_EXCEPTIONS.md` governs new ones; its content was not read for this document). Revisit only if the extra allocation shows up in a measurement. |
| G6 | Move the chameleon and neural-codec code to a `research/` crate, or delete it? | Delete morphology, padding and speculation. Move chameleon and the neural-codec types to a research crate; git history keeps the rest. |
| G7 | Compression: keep the flag, or drop compression from v1? | Keep the flag, default off below 1 KiB, never on frames carrying secrets (C.4). If no measurement supports it, drop the flag in v1 and let a later version add it. |
| G8 | Capability chains in the frame header (`FIRST_PRINCIPLES.md`, change 1)? | Not in v1. Authority (layer 3) is not designed (`docs/spec/03-authority.md` is a draft); adding a header field is a new version, and that is acceptable because nothing is released. |
| G9 | `LatentMessage` is the only live research message (`ExecuteBinary` with `StreamLatent`). Return the vectors inside the `ExecuteBinary` result instead of an extension? | Extension only if a user needs streaming; otherwise return them in the result, bounded, and drop the extension kind. |
| G10 | Allow `REQUEST` before `HELLO_ACK` to save a round trip (P8, time to first token)? | Not in v1. Revisit with measurements; the cost is one RTT per connection, and connections are long-lived. |
| G11 | The browser extension speaks JSON to `ws://localhost:9800/ws`, where nothing listens. | Delete it from this repository or give it a v1 client (WebSocket plus the frame codec in JavaScript); do not leave a second protocol. |
| G12 | The panic path in `ExecuteBinary` (A.10) is independent of this proposal. Fix before any release? | Yes, as its own change: map the error to a `RESPONSE` error. Not part of this migration. |
