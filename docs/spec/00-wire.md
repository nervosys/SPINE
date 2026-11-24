# spine-wire v1: the native client to daemon protocol

Status: spine-wire v1, as implemented at `c33e52b` (branch `completion/agent-web`), and revised in place on 2026-11-24 for a client identity and a per-connection value in the handshake (1.5, 6.3, 6.6). Normative for the reference implementation (`src/spine-protocol`, the daemon in `src/spine-core`, the SDK in `src/spine-agent`). Not externally reviewed. Nothing in this revision was compiled or run except the two Python commands of section 15.1; every rule below names the function that enforces it, and a rule that could not be confirmed by reading is marked "unverified".

This document replaced an earlier one that described the pre-v1 wire (now Appendix A), proposed v1 (the text of sections 2 to 12), planned the migration (Appendix B) and listed open questions (Appendix C). Where the proposal and the implementation differed, the implementation is now the specification, or, when the implementation contradicts another rule of this document or looks like a defect, the behaviour is specified as implemented and listed in section 16. Section labels of the earlier document (`C.7`, `G11`, ...) still appear in source comments and other documents; Appendix D maps them to the sections below.

## 1. Conventions, scope and security non-goals

### 1.1 Conventions

The key words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY are to be read as in RFC 2119 as clarified by RFC 8174, and only when in capitals. A statement without those words is a description of what the implementation does.

Citations are `file` plus `function` or type, relative to `src/spine-protocol/src` unless a crate is named. Line numbers are not used. "The daemon" is `spine-core`; "the SDK" is `spine-agent` (`AgentClient`); "the reference" is `scripts/wire_v1.py`.

All integers in frame headers are big-endian. "uint" means a CBOR unsigned integer, "text" a CBOR text string, "bytes" a CBOR byte string.

### 1.2 Scope

The client to daemon path: the SDK, the CLI, the gateway and the FFI, Go and Python bindings (all of which use the SDK) against `spine-core`. The node to node mesh path is a different protocol (JSON `MeshEnvelope` records, optionally inside the `spine-session` record layer) and is not described here. Compatibility with earlier SPINE wires, HTTP and MCP is not a goal (`docs/FIRST_PRINCIPLES.md`).

### 1.3 Parties

| Party | Role | Transports |
| --- | --- | --- |
| `spine-core` | server | TCP (`server.port`, default 8080), WebSocket (`port + ws_port_offset`, default 8081), QUIC (`port + quic_port_offset`, default 8082, only when built with `--features quic`) |
| `spine-agent` `AgentClient` | client | TCP, TCP plus TLS (optional mutual TLS), WebSocket (`ws://`, `wss://`); any `AsyncRead + AsyncWrite` through `AgentClient::over`. No QUIC client. |
| `spine-cli`, `spine-gateway`, `spine-ffi`, Go, Python | clients through the SDK | as the SDK; FFI and Go and Python use TCP only |

### 1.4 Security non-goals

This layer provides no confidentiality, no peer authentication and no replay protection. A frame is plaintext inside whatever carries it. The handshake can carry one client public key with a proof that the client holds its private key, and it gives every connection a random value (6.3, 6.6). The proof has no channel binding, so a relay can carry it (6.6), and nothing in the daemon acts on the key. No frame carries a capability, a grant or a request nonce; one that did would be a further revision (1.5).

Where protection comes from today:

- TLS 1.3 through rustls on the TCP and WebSocket listeners. It is off by default (`tls.enabled`), optionally with mutual TLS (`tls.mutual_tls`). The only authentication the daemon acts on is an X.509 client certificate or nothing. A client MAY also present a SPINE key with `IDENTIFY` (6.6): the daemon records it for its log, and it grants nothing.
- A non-loopback `server.host` is refused at startup unless `tls.mutual_tls` is on or `server.allow_unauthenticated_remote` is set (`spine-core` `config.rs` `SpineConfig::validate`). Every connected client has full control of the node.
- QUIC has no client authentication and its certificate is self-signed; startup with QUIC is refused when `tls.enabled` (`spine-core` `main.rs` `main`).
- The mesh path runs a different handshake (`spine-session`: ML-KEM-768, Ed25519, AES-256-GCM) that has had no external review.

Whether to run this wire over the `spine-session` handshake instead of TLS is an open question (Appendix C, G1). It stays open until that handshake is independently reviewed. The frame layer is agnostic: v1 frames would be the plaintext inside session records.

### 1.5 Revision history

2026-11-24: the wire was revised in place. The version byte is still 1 and `HELLO.versions` is still `[1]`. Before the revision, 1.4 said: "There is no field for a signature, a capability or a nonce, and adding one is a new version."

What changed: `HELLO_ACK` gained three required keys, `binding`, `authority` and `time` (6.3, 6.4); a frame type `IDENTIFY`, `0x06`, was allocated (3, 6.6) with its state rule (8.2) and its failure behaviour (7.3); limits and vectors were added (14, 15). The sentence above no longer holds, and 1.4 now says what the handshake carries and what it does not.

Why in place: nothing has been released, and this document is normative for the reference implementation in this repository only. The draft `00-wire-revision.md` (A.3) weighed a version 2 against revising v1 and recommended revising it; its open questions 1, 3 and 13 (in place, keep `IDENTIFY`, a pure-Python Ed25519 in the reference) were answered provisionally that way and remain the project owner's to overturn (`00-wire-revision.md` E).

Consequences. A peer built from the text before this revision does not send or check the three keys: a client of that kind works against a revised server (it ignores keys it does not know, 5.2 rule 9) and never sends `IDENTIFY`; a revised client against such a server closes with `protocol_violation` at `HELLO_ACK` (6.3). "Version 1" therefore names two behaviours with one version byte, and nothing on the wire tells a peer which one it faces except that failure. Nothing else of the draft is in this document: no `AUTH_REQUEST`, no `RECEIPT`, no new error codes, and the authority gate is still not called.

2026-11-24, second step (names): `NameResolution` was revised in the same way (12.4, 12.5). The `resolved` outcome gained a required key `log`, the succession log the node holds for the name, 0 to 64 entries; three outcomes were added, `retired`, `compromised` and `revoked`, each carrying the evidence a client verifies; and a revoked name is no longer reported as `invalid`. The version byte is still 1. The command set, the frame types and the error codes of section 13 are unchanged: these are new shapes of one `ok` value, not new commands or errors. Consequences: a client built before this step reads a revised daemon's `resolved` answer with an unknown key, which `NameResolution` refuses (the type is closed), so such a client fails to decode every resolve answer of a revised daemon; a revised client reading an older daemon's `resolved` answer finds `log` missing and refuses it the same way. As with the first step, "version 1" now names two behaviours and nothing on the wire tells a peer which one it faces except that failure. Of the draft `00-wire-revision.md`, this step implements C6 only.

2026-11-24, third step (delete and codes): two commands were added, `delete_resource` and `delete_knowledge` (12.2, 12.4), so the closed command set is 21, and four error codes, `knowledge_entry_too_large`, `knowledge_capacity`, `action_unsupported` and `delete_refused` (13), so the list is 51. Three refusals that were `execution_failed` now carry their own code: a `store_knowledge` entry over 64 KiB is `knowledge_entry_too_large`; a knowledge store past the per-session count or the daemon-wide byte budget, from `store_knowledge` or from a program, is `knowledge_capacity`; a program that requests an action the daemon does not perform is `action_unsupported`. `execution_failed` stays for a program that failed to compile, instantiate or run. The version byte is still 1. Consequences: an older client sends neither command, and reads the three renamed refusals as a code it does not know, which it MUST tolerate (13) and which no longer reads as `execution_failed`; an older daemon answers either new command with `unknown_command`, and an older SDK decoding a reply to one of them is not reachable because it never sends one. "Version 1" now names three behaviours. Of the draft `00-wire-revision.md`, this step implements C7 and the four non-authority codes of C4. Still not in this document: `AUTH_REQUEST`, `RECEIPT`, the `authority_*` codes, and a call of the authority gate.

## 2. Frame

### 2.1 Layout

```
 0                   4   5   6   7                  15
+-------------------+---+---+---+------------------+----------------+
| length : u32      | v | t | f | request id : u64 | body           |
+-------------------+---+---+---+------------------+----------------+
                      version, type, flags
length = 11 + len(body)           (the bytes after the length field)
```

Source: `frame.rs` (`HEADER_AFTER_LENGTH = 11`, `HEADER_LEN = 15`, `encode_frame`, `read_frame`, `decode_frame`).

### 2.2 Header fields

| Field | Size | Rule |
| --- | --- | --- |
| `length` | 4 | The count of bytes after this field, at least 11 and at most the maximum in force (2.3). |
| `version` | 1 | MUST be 1. Any other value is rejected with `unsupported_version` (`frame.rs` `parse_header`). `HELLO` and `HELLO_ACK` carry 1 like every frame; the selected version in `HELLO_ACK` is 1 because it is the only version implemented (6.4). |
| `type` | 1 | One of section 3. `0x00` and every unallocated value is rejected with `unknown_frame_type`. |
| `flags` | 1 | Bit 0: the body is compressed (section 4). Bits 1 to 7 MUST be zero; any of them set is rejected with `malformed_frame`. |
| `request id` | 8 | Section 8. Frame-level rules: `HELLO`, `HELLO_ACK` and `CLOSE` MUST carry 0; `REQUEST` and `PING` MUST carry a nonzero id. A violation is `protocol_violation` (`frame.rs` `check_request_id`). Other types are not constrained at this layer. |
| `body` | `length - 11` | Exactly one deterministic CBOR item (section 5), possibly compressed. A frame type with nothing to say still carries one item (`null`, byte `f6`, or an empty map `a0`, as that type defines). An empty body is not a CBOR item and is `malformed_frame`. |

### 2.3 Length and limits

The maximum in force is `min(m, 16,777,216)` where `m` is 65,536 before `HELLO` and `HELLO_ACK` have been exchanged and the negotiated `max_frame` afterwards (`frame.rs` `effective_max`, `PRE_HELLO_MAX_FRAME`, `MAX_FRAME`; `connection.rs` `Connection`). A caller that supplies a larger maximum is capped at 16,777,216.

A receiver MUST reject `length` below 11 or above the maximum in force with `frame_too_large`, and it MUST NOT read or allocate for the body of such a frame (`frame.rs` `check_length` is evaluated on the first four bytes, before anything else). Within the maximum, `read_frame` reads the body in chunks of at most 64 KiB, so memory grows with the bytes that arrive and not with the announced length. A receiver SHOULD do the same.

A sender MUST NOT produce a frame whose uncompressed length (`11 + len(body)`) exceeds the maximum in force, even if compression would make it smaller (`frame.rs` `encode_frame`). The negotiated `max_frame` applies in both directions.

### 2.4 Transports

The frame layer needs an ordered, reliable byte stream. Frame boundaries do not have to align with transport messages. One connection uses one stream.

| Transport | Notes |
| --- | --- |
| TCP | Optionally wrapped in rustls TLS (`tls.enabled`). |
| WebSocket | Optionally TLS. The byte stream is the concatenation of Binary messages; a message that is not Binary and not Close is skipped (`spine-transport` `websocket.rs` `WebSocketStream::poll_read`). The server accepts messages and frames up to 16 MiB. No path, subprotocol or Origin check exists. |
| QUIC | One bidirectional stream, the first, which must arrive within 10 seconds of the connection. Feature `quic` of `spine-core`, off by default; no test and no client in the workspace speaks it (section 16, item 6). |

## 3. Frame types

| Type | Name | Sender | Request id | Body |
| --- | --- | --- | --- | --- |
| `0x00` | invalid | none | | rejected: `unknown_frame_type` |
| `0x01` | `HELLO` | client, first frame only | 0 | map, 6.2 |
| `0x02` | `HELLO_ACK` | server, first frame only | 0 | map, 6.3 |
| `0x03` | `CLOSE` | either | 0 | `{code, message[, supported]}`, 6.5 |
| `0x04` | `PING` | either | nonzero | uint token, section 9 |
| `0x05` | `PONG` | either | the `PING`'s id | the same token |
| `0x06` | `IDENTIFY` | client, once, only right after `HELLO_ACK` | 0 | `{key, sig, channel}`, 6.6 |
| `0x10` | `REQUEST` | client | nonzero, not in flight | a command, section 12 |
| `0x11` | `RESPONSE` | server | the request's id | `{ok: any}` or `{err: {code, message[, details]}}`, 8.4 |
| `0x20` | `STREAM_START` | server | the request's id | `{role, model[, trace][, decode_hints]}`, section 10 |
| `0x21` | `STREAM_TOKEN` | server | the request's id | `{seq, data[, usage]}` |
| `0x22` | `STREAM_END` | server | the request's id | `{reason[, usage]}` |
| `0x23` | `STREAM_CANCEL` | client | the stream's id | `{[reason]}`; with no reason, the empty map `a0` |
| `0x80` | `EXT` | either | extension-defined | array `[index, kind, payload]`, section 11 |

All other values (`0x07` to `0x0f`, `0x12` to `0x1f`, `0x24` to `0x7f`, `0x81` to `0xff`) are unallocated. Source: `frame.rs` `FrameType`.

## 4. Compression

Flag bit 0 set means the body is `u32 uncompressed_len || zstd frame`, where `uncompressed_len` is big-endian and counts the CBOR bytes. Receivers need no setting to accept it.

1. The receiver checks `uncompressed_len <= maxinforce - 11` before it calls the decompressor, and a larger value is `frame_too_large`, not `malformed_frame` (`frame.rs` `decompress_body`, `FrameError::UncompressedTooLarge`). A body shorter than the four prefix bytes is `malformed_frame`.
2. The receiver decompresses with an output cap of `uncompressed_len + 1` bytes and a zstd window limit of 2^23 bytes (`window_log_max(23)`). Output whose length differs from `uncompressed_len`, a stream that is not zstd, and any other decompression error are `malformed_frame`.
3. The sender MUST emit a single zstd frame with no dictionary. What the receiver does with several concatenated frames, a skippable frame or trailing bytes is whatever the zstd decoder in use does; it is not specified here (unverified, `decompress_body` hands the slice to `zstd::stream::read::Decoder`).
4. Sender policy, not checked by the receiver: do not compress a body of fewer than 1,024 bytes, keep the compressed form only if `4 + compressed length < body length`, use zstd level 3 (`frame.rs` `COMPRESS_THRESHOLD`, `ZSTD_LEVEL`, `encode_frame`). A sender SHOULD NOT compress a body that mixes a secret with attacker-influenced data.
5. `Connection` compresses only after `set_compression(true)`; no caller in the workspace enables it, so no frame the daemon or the SDK sends is compressed (searched).

Compression is per frame. A frame of `max_frame` zeros compresses to a tiny frame, so the amplification per frame is large and only the request rate limit bounds a stream of them; the daemon has no per-connection byte budget (section 16, item 14).

## 5. Deterministic CBOR profile

Every body is one CBOR data item that satisfies RFC 8949 section 4.2.1 (core deterministic encoding) and the rules below. The scanner is `cbor.rs` `check_deterministic`; the encoder is `cbor.rs` `to_vec`; typed decoding is `cbor.rs` `from_slice` (scan first, then ciborium).

### 5.1 Encoding rules

1. Definite lengths only. An indefinite-length string, array or map is `IndefiniteLength`.
2. Integers and the lengths in string, array and map heads are in the shortest form (`NonShortestInteger`). A float is in the shortest of binary16, binary32 and binary64 that preserves the value (`NonShortestFloat`), infinities included; NaN of any payload or sign is written `f9 7e 00` and any other NaN is `NonCanonicalNan`. An integer is never written as a float.
3. Map keys are in strictly ascending bytewise order of their encodings, which also rejects duplicates (`UnsortedOrDuplicateKey`). Because the encoded key starts with its length, shorter text keys sort first.
4. No tags (`Tag`). Simple values are limited to `false`, `true`, `null`; `undefined` and every other simple value, including the `f8 xx` form, is `UnsupportedSimple`.
5. Text strings are valid UTF-8 (`InvalidUtf8`); no normalisation is applied.
6. Nothing follows the item (`TrailingBytes`). Arrays and maps nest at most 32 deep; scalars do not count, so 32 nested arrays around a scalar are accepted and 33 are `DepthExceeded`. A break byte outside an indefinite item is `UnexpectedBreak`, additional information 28 to 31 where none is defined is `ReservedInfo`, and input that ends inside an item or declares a length beyond the input is `Truncated`.

The scanner evaluates a head's argument, then the depth, then the declared element count against the remaining input, then the elements. Every rejection above is a `malformed_frame` close (7.3).

### 5.2 Data-model rules

7. Messages follow serde's defaults for the Rust types: a struct is a map with text keys; an enum with data is a single-entry map `{variant: fields}`; a unit variant is the text of its name; an `Option` field is omitted when absent and a present `null` for it is rejected; a byte-string field (hash, key, payload) is a CBOR byte string and never an array of integers (`wirefmt.rs` `bytes`, `byte_array`, `opt_byte_array`, `reject_null`). Variant and field names are wire identifiers, spelt with `#[serde(rename)]` (serde's `rename_all` would turn `GetUR` into `get_u_r`). A list that is omitted when empty (`DecodeHints.stop_sequences`, `TraceContext.state`) rejects a present empty list (`wirefmt.rs` `non_empty_vec`).
8. Structs are closed: an unknown field in a known struct is a schema error (`#[serde(deny_unknown_fields)]`). `CLOSE`, `RESPONSE.err`, every command and every type of section 12.3 is closed.
9. `HELLO` and `HELLO_ACK` are open: unknown keys are ignored, so later versions can add fields to negotiation. The keys of a body MUST be text; a map with a non-text key is `protocol_violation` (`hello.rs` `require_text_keys`).

A body that is deterministic CBOR but does not fit its type is a schema failure; the answer depends on the frame type (7.3).

### 5.3 JSON-shaped values

Several fields hold an arbitrary value (`store_knowledge.value`, `tool_call.args`, the `ok` of most replies). They are `serde_json::Value`. A JSON-shaped value is `null`, `true`, `false`, an integer in -2^63 to 2^64 - 1, a finite float, text, an array, or a map with text keys. It cannot hold a byte string, a tag, a non-text key, NaN or an infinity (`serde_json` turns NaN and infinities into `null`). A REQUEST that carries one of these in a JSON-shaped field is `invalid_request`, because `native.rs` `decode_strict` re-encodes the decoded value and rejects the body when the bytes differ. Integers below -2^63 are expected to fail in the same way (unverified: it rests on `serde_json::Value` having no 128-bit visitor).

Consequence: bytes cannot be returned inside a JSON-shaped `ok`. `read_resource` and `capability_query` therefore answer with typed results. Name records carry their hashes inside a JSON-shaped value as arrays of integers (section 16, item 3).

## 6. Establishing a connection

### 6.1 Sequence

The client sends `HELLO` as its first frame; the server answers with `HELLO_ACK` before any other frame. The client MUST NOT send another frame before it has read `HELLO_ACK` (`connection.rs` `Connection::client_handshake` returns a `Connection` only after the acknowledgement). Both frames are read and written under the pre-`HELLO` maximum of 65,536 bytes and are never compressed.

The server (`Connection::server_handshake`) reads the first frame under that maximum and a deadline (the daemon: `server.hello_timeout_secs`, default 10 s; the SDK waits 10 s for `HELLO_ACK`). The deadline covers the whole frame. On expiry the server sends `CLOSE(timeout)` and ends the connection. The same deadline bounds the write of `HELLO_ACK`.

The first frame, in the order the code checks it: the frame-layer checks of 7.2 (length, version, type, flags, request id, decompression); then its type must be `HELLO` (otherwise `protocol_violation`, and the body is not examined); then the body must be deterministic CBOR (`malformed_frame`); then it must decode as a `HELLO` and satisfy 6.2 (`protocol_violation`); then negotiation (6.4) may fail with `unsupported_version`. Every failure is answered with a best-effort `CLOSE` and the connection ends. A stream that ends before the first byte ends without a `CLOSE`.

The client reads the reply under the same maximum. A `CLOSE` in place of `HELLO_ACK` ends the handshake and is reported with its code and message. Otherwise the reply must be a `HELLO_ACK` that passes 6.3 and `validate_against` (`hello.rs` `HelloAck::validate_against`); any failure is `protocol_violation`, and a selected version other than 1 is `unsupported_version`, each answered with a best-effort `CLOSE`.

Sending requests before `HELLO_ACK` is not allowed in v1. After it, and before its first request, a client MAY send one `IDENTIFY` (6.6).

### 6.2 HELLO

A map; unknown keys are ignored (5.2 rule 9). In the deterministic order for these names:

| Key | Type | Optional | Rule |
| --- | --- | --- | --- |
| `versions` | array of uint (u64) | no | Not empty. Strictly ascending. |
| `max_frame` | uint (u32) | no | 1,024 to 16,777,216 inclusive. |
| `extensions` | array of text | no | May be empty. Each id ASCII and at most 128 bytes. No duplicates. |
| `agent` | text | yes | At most 128 bytes (UTF-8 length). A present `null` is rejected. Informational. |

Any violation is `protocol_violation` (`hello.rs` `Hello::validate`, `Hello::from_frame`): a missing or mistyped key, an empty `versions`, a descending or repeated version, `max_frame` outside its range or above 2^32 - 1 or not an integer, a duplicate or non-ASCII or over-long extension id, an over-long `agent`, a body that is not a map, and a first frame of another type. A body that is not deterministic CBOR is `malformed_frame` instead.

### 6.3 HELLO_ACK

A map; unknown keys are ignored.

| Key | Type | Optional | Rule |
| --- | --- | --- | --- |
| `version` | uint | no | The selected version: one the client offered. |
| `max_frame` | uint (u32) | no | `min(client max_frame, server maximum)`; 1,024 to 16,777,216. Applies to frames in both directions from now on. |
| `max_inflight` | uint (u32) | no | At least 1. The number of requests the server processes concurrently per connection. |
| `extensions` | array of text | no | The accepted subset of the offered ids, in the order that fixes the extension index (11). No duplicates. |
| `agent` | text | yes | Informational. Not length-limited. |
| `binding` | bytes, exactly 32 | no | A value the server draws for this connection from the operating system's random source. See below. |
| `authority` | text | no | The node's authority mode: `"off"`, `"audit"` or `"required"`, exactly these lower-case texts. |
| `time` | uint (u64) | no | The server's clock, Unix seconds, when it built the acknowledgement. |

In the deterministic order of its keys, the acknowledgement is: `time`, `agent`, `binding`, `version`, `authority`, `max_frame`, `extensions`, `max_inflight` (`agent` only when present).

A client MUST treat as `protocol_violation`: `max_frame` outside its range or above the one it offered, `max_inflight` of 0, a duplicate extension, a `version` it did not offer, an extension it did not offer, a missing or mistyped `binding`, `authority` or `time`, a `binding` that is not 32 bytes, an `authority` that is not one of the three texts (`HelloAck::validate`, `validate_against`).

`binding` is generated after a valid `HELLO` has been read and before the acknowledgement is written (`hello.rs` `negotiate`, `fresh_binding`, from `OsRng`). Nothing the client sent is an input, so a client cannot steer it, and it is unique per connection with overwhelming probability. It appears once on the wire, here, and in the signed message of `IDENTIFY` through the acknowledgement bytes (6.6). Nothing else uses it: it is not a session token and possession of it grants nothing. (`03-authority.md` calls the value an invocation would be bound to `session`; no invocation is checked by the daemon today.) Both ends keep the bytes of the two handshake bodies for the connection (`connection.rs` `Connection::transcript`, `binding`).

`authority` reports what the node's configuration says, to every client, anonymous or not. The wire has three values. The daemon reports `off`, the only mode in which it starts: it refuses `required` at startup (`spine-core` `config.rs` `AuthorityConfig::validate`) and has no `audit`. It enforces nothing in any mode (section 16, item 20). A client MUST accept all three values.

`time` is not authenticated: an active attacker can shift it. It is a hint for a client whose own clock may be wrong. No peer in this repository uses it.

### 6.4 Negotiation

`hello.rs` `negotiate`, with the daemon's `native.rs` `local_config`:

- Version: the highest offered version that the server supports. If there is none the server sends `CLOSE(unsupported_version)` whose `supported` lists the server's versions sorted ascending without duplicates. The only version implemented is 1; a server whose configuration listed another version still answers `unsupported_version` with `supported = [1]` rather than select it. An offer of only version 0, or of `[1, 2^64 - 1]`, behaves as the rules say (the second selects 1).
- `max_frame`: `min(client's, server's)`, where the server's own maximum is first clamped into 1,024 to 16,777,216 (a configured 100 becomes 1,024, a configured 0xFFFFFFFF becomes 16,777,216).
- `max_inflight`: the server's value, but at least 1. The daemon sends 8 (`native::DAEMON_MAX_INFLIGHT`): the number of requests one connection may have accepted at a time, the running one included (8.3).
- `extensions`: the offered ids that the server supports, in the order of the client's `HELLO`. The server's own order is ignored. The position in the resulting array is the extension index for the connection.
- `agent`: the server's own, omitted when it has none (the daemon has none). The SDK offers `spine-protocol/<crate version>`.
- `binding`: 32 fresh random bytes for every connection, unless the caller of `negotiate` injected one (`LocalConfig::binding`, for tests and the vectors).
- `authority`: the server's own mode (`LocalConfig::authority`, `off` by default; the daemon sets it from `[authority] mode`, `main.rs` `serve_native`).
- `time`: the system clock in Unix seconds, unless injected (`LocalConfig::time`).

The daemon accepts no extensions, so it answers an empty list.

### 6.5 CLOSE

`CLOSE` carries request id 0 and the closed map `{code: text, message: text[, supported: array of uint]}` (`hello.rs` `Close`). `supported` is meaningful only with `unsupported_version`. Codes: `unsupported_version`, `unknown_frame_type`, `malformed_frame`, `frame_too_large`, `protocol_violation`, `timeout`, `shutting_down` (7.3). A `message` is for people and is not specified. A receiver MUST accept an unknown `code` text as a close without interpreting it.

The sender closes after sending: it writes the frame (at most one second, under the pre-`HELLO` maximum, never compressed), then shuts down its write side (`connection.rs` `best_effort_close`). A peer MUST NOT rely on receiving a `CLOSE`. After sending or receiving one, or after an error, `Connection` refuses further use.

### 6.6 IDENTIFY

A client MAY prove, once per connection, that it holds an Ed25519 private key. The server records the public key as the connection's identity. The identity is recorded and grants nothing: the daemon uses it for its log and for nothing else, and an identified connection gets exactly the answers, limits and rights an anonymous one gets. What the proof does not show is stated below and in 1.4 and section 16 (items 19 and 20).

The frame has type `0x06`, is sent by the client only, and carries request id 0 (`frame.rs` `check_request_id`, `FrameType::Identify`). The body is a closed map (5.2 rule 8; `identify.rs` `Identify`). In the deterministic order of its keys:

| Key | Type | Rule |
| --- | --- | --- |
| `key` | bytes, exactly 32 | The client's Ed25519 public key. |
| `sig` | bytes, exactly 64 | The Ed25519 signature by `key` over the message `M` below. |
| `channel` | bytes | The channel-binding input. In this revision it MUST be empty, and a server MUST refuse a non-empty one. |

The signed message (`||` is concatenation, `u8(n)` is one byte, `0x00` is one zero byte):

```
M = "spine-wire-identify-v1" || 0x00
 || SHA-256(H)                   H = the body bytes of the HELLO frame, exactly as sent
 || SHA-256(A)                   A = the body bytes of the HELLO_ACK frame, exactly as sent
 || u8(len(channel)) || channel
 || key
```

`H` and `A` are deterministic CBOR (5.1), so both ends hold the same bytes without agreeing on anything else, and neither frame is ever compressed (6.1). `A` holds `binding` (6.3) and `time`, so `M` belongs to one connection. The signature is checked with strict Ed25519 verification, as `ed25519_dalek::VerifyingKey::verify_strict` does it: RFC 8032, with `S` below the group order and a small-order public key or `R` refused (`identify.rs` `Identify::verify`). The domain string is not used by any other signed object of SPINE. With an empty `channel` the body is 118 bytes and the frame 133 (`length` field `0x81`).

Rules:

1. A client MAY send one `IDENTIFY`, as the first frame after it has read `HELLO_ACK` and before any other except `PING` or `PONG`. A server treats an `IDENTIFY` as `protocol_violation` when it has already received one, or a `REQUEST` (answered `overloaded` or not), a `STREAM_CANCEL` (for an id in flight or not) or an `EXT`. A `PING` or `PONG` does not close the window. The server handles frames in arrival order, so a client may send the `IDENTIFY` and its first request without waiting (`connection.rs` `Tracker::check_incoming`).
2. The server checks the frame before it handles the next one, in this order: the body is deterministic CBOR (`malformed_frame`, as for every body, 7.2 step 6); it is a closed map of the three keys with the stated types and lengths; `channel` is empty; `key` is a valid Ed25519 public key; the signature verifies. A failure of any check after the first is `CLOSE(protocol_violation)`; there is no new close code. The connection then ends (7.3).
3. On success the connection has an identity, the 32-byte `key`, for the rest of its life. There is no reply frame. The server accepts `IDENTIFY` whatever authority mode it reports.
4. A client that receives an `IDENTIFY` closes with `protocol_violation` (8.2). A client that tries to send a second one, or one after a request, gets `ConnectionError::Usage` and nothing is sent (`Tracker::check_outgoing`).
5. A client that sends none is anonymous. The daemon logs the identity at debug level as the key in hex or as `anonymous`, and never uses it as a metric label, because keys cost nothing to mint.

What the proof shows: some party that holds the private key of `key` signed `M` after the server wrote this exact acknowledgement. `M` covers `A`, which holds the 32 random bytes of this connection, so a proof made for another connection does not verify on this one.

What it does not show:

- That the signer is the party that sends the frames on this stream. There is no channel binding (the `channel` field is reserved for a security layer that gives both ends a value to sign, and none does today), so an active relay can forward the client's `HELLO`, the server's acknowledgement and the client's `IDENTIFY` unchanged and then hold a connection that the server attributes to the client's key. This is why an identity grants nothing.
- Who the server is: the acknowledgement carries no server key.
- Anything about the key: that it is unique, fresh or held by a person.

No peer in this repository enforces authority (section 16, item 20). A later revision may use the identity; if it does, it has to say so here.

## 7. Error behaviour

### 7.1 Two classes

Byte-level non-conformance (a frame or body that breaks sections 2 to 6, or a state rule of section 8) ends the connection, normally with a `CLOSE` that carries a code. A well-formed request the server cannot serve is answered with a `RESPONSE` whose `err` carries a code, and the connection stays open.

### 7.2 Check order

The receive path checks, in this order, and the first failure decides the outcome. Steps 1 to 5 are `frame.rs` (`read_frame`, `decode_frame`); step 6 and 7 are `connection.rs` (`Connection::accept`).

1. The first four bytes. If the stream ends cleanly before the first byte, the connection ended normally. If it ends after one to three bytes, the frame is truncated (no `CLOSE`).
2. `length` below 11 or above the maximum in force: `CLOSE(frame_too_large)`. Nothing more is read.
3. The 11 header bytes (truncation: no `CLOSE`), then in this order: `version` (`unsupported_version`), `type` (`unknown_frame_type`), `flags` bits 1 to 7 (`malformed_frame`), the request-id rule of the type (`protocol_violation`). The body has not been read yet.
4. The body, in chunks (truncation: no `CLOSE`).
5. If flag bit 0 is set: shorter than four bytes (`malformed_frame`); declared length above `maxinforce - 11` (`frame_too_large`); not zstd or a decoding error (`malformed_frame`); output length differs from the declared length (`malformed_frame`).
6. The body is one deterministic CBOR item (`malformed_frame`). This applies to every type, including `CLOSE`, `PING` and a stray `HELLO`.
7. By type: `CLOSE`: the closed `{code, message[, supported]}` map, else `CLOSE(protocol_violation)` or `malformed_frame` as the decoder reports it; a valid one is delivered to the caller as a close. `PING` and `PONG`: the body must be a uint, else `protocol_violation`. `HELLO` and `HELLO_ACK`: always `protocol_violation`. `IDENTIFY`: the window rule and the proof of 6.6, each `protocol_violation`. Any other type: the direction, id and state rules of section 8 and the `EXT` rule of section 11, each `protocol_violation`; at the cap, a server answers `overloaded` instead (8.3).

During the handshake the order is the one in 6.1; step 7 does not apply and the first frame must be `HELLO`.

After a frame is delivered, the daemon decodes a `REQUEST` body (`native.rs` `decode_command`): a name outside the command set is `unknown_command`, any other defect `invalid_request` (12.1). Then, for the session commands, the daemon applies the connection's command rate limit (`rate_limit`), creates the connection's session if it has none (`session_capacity` when the daemon holds `server.max_sessions`; the connection stays open), and runs the command, whose failures are in section 13. A command refused by the rate limit creates no session. `tool_call` and `capability_query` need no session; `tool_call` is limited by a separate per-connection bucket and `capability_query` is not limited (`spine-core` `main.rs` `handle_session`).

### 7.3 Close codes

| Code | Sent when |
| --- | --- |
| `unsupported_version` | A frame's `version` byte is not 1; a `HELLO` offers no version the server supports (with `supported`); a `HELLO_ACK` selects a version other than 1. |
| `unknown_frame_type` | `type` is `0x00` or unallocated. |
| `malformed_frame` | Flag bits 1 to 7 set; a compressed body shorter than 4 bytes, not zstd, undecodable or of the wrong length; a body that is not one deterministic CBOR item (any rule of 5.1, an empty body included). |
| `frame_too_large` | `length` below 11 or above the maximum in force; a declared uncompressed length above `maxinforce - 11`. |
| `protocol_violation` | A frame-level request-id rule broken; a well-formed frame that breaks a state rule (wrong direction, reused in-flight id, `RESPONSE` or `STREAM_*` for an id in the wrong state, a second `HELLO` or `HELLO_ACK`, a `PING` or `PONG` whose body is not a uint, an `EXT` for an extension that was not accepted); a `HELLO`, `HELLO_ACK`, `CLOSE` or `IDENTIFY` body that is deterministic CBOR but does not fit its type or breaks a bound of 6.2, 6.3 or 6.6, including a `HELLO_ACK` without a valid `binding`, `authority` or `time`; an `IDENTIFY` that is a second one, or follows a `REQUEST`, `STREAM_CANCEL` or `EXT`, or has a non-empty `channel`, or whose signature does not verify (6.6); a first frame that is not `HELLO`. |
| `timeout` | No `HELLO` within the deadline; no frame within `server.idle_timeout_secs` (`Connection::recv_timeout`); a write that exceeded 30 seconds is not answered with a `CLOSE` (7.5). |
| `shutting_down` | The daemon is shutting down: sent to an established connection once its current request is answered (`main.rs` `handle_session`), and to a connection whose handshake completed during shutdown (`serve_native`). |

### 7.4 Error replies

These answer a `REQUEST` with a `RESPONSE` whose `err.code` is as shown, the same request id, and the connection stays open:

| Code | Condition |
| --- | --- |
| `unknown_command` | The body is a text string, or a single-entry map with a text key, that names a command outside the 21 of section 12. |
| `invalid_request` | The body fails to decode as a command for any other reason: a missing, unknown or mistyped field, a present `null` for an optional field, a known name with the wrong shape, a body that is not a command (a map with two entries, a non-text key), or a decoded value that does not re-encode to the same bytes. |
| `overloaded` | The server is at `max_inflight`; the request is not executed and its id is not tracked (8.3). |

Every other code (section 13) comes from the command that ran. A `RESPONSE` body that does not fit its type is not a close condition in the implementation (section 16, item 8).

### 7.5 Where no CLOSE is sent

- The stream ends or fails inside a frame, or the transport fails: nothing can be sent.
- A connection refused at admission (`server.max_connections`, `server.max_connections_per_ip`) is closed without a byte, before any handshake; QUIC uses `refuse()` (`spine-core` `connections.rs`, `main.rs`). During shutdown a connection is accepted and answered with `CLOSE(shutting_down)` after its handshake (7.3).
- A write that exceeded 30 seconds (`native.rs` `send_bounded`, `send_command`): the stream may hold a partial frame, so the connection is abandoned and nothing more is written.
- A TLS handshake or WebSocket upgrade that fails or exceeds 10 seconds ends without a frame.

## 8. Requests, responses and in-flight rules

### 8.1 Ids

A client request id is a nonzero u64 the client chooses. It names the exchange until the reply (`RESPONSE`, or `STREAM_END` for a stream) has been received. `Connection::request` uses a counter from 1 that skips ids still in flight and 0. Reusing an id that is in flight is a protocol violation. The two directions have independent id spaces: a `PING` carries a sender-chosen id and its `PONG` echoes it. Id 0 is carried by `HELLO`, `HELLO_ACK`, `CLOSE` and `IDENTIFY`, and may be carried by `PONG`, `RESPONSE`, `STREAM_*` and `EXT` at the frame layer, where the state rules of this section then apply (`RESPONSE` and `STREAM_*` for id 0 are never in flight, so they are violations). No body carries an id; the header id is the only correlation.

### 8.2 Direction

| Type | Client may send | Server may send |
| --- | --- | --- |
| `HELLO`, `HELLO_ACK` | only `HELLO`, first | only `HELLO_ACK`, first |
| `CLOSE`, `PING`, `PONG`, `EXT` | yes | yes |
| `IDENTIFY` | once, right after `HELLO_ACK` (6.6) | no |
| `REQUEST`, `STREAM_CANCEL` | yes | no |
| `RESPONSE`, `STREAM_START`, `STREAM_TOKEN`, `STREAM_END` | no | yes |

A frame in the wrong direction is `protocol_violation` on receipt and `ConnectionError::Usage` on send (`Connection::check_incoming`, `check_outgoing`).

### 8.3 REQUEST handling

A server that receives a `REQUEST` checks, in order: the id is already in flight (`protocol_violation`); the number in flight is already `max_inflight` (a `RESPONSE` with `err.code = "overloaded"`, same id, the request is neither tracked nor delivered, the connection stays open); otherwise the id is tracked and the request delivered. A client SHOULD NOT have more than `max_inflight` requests outstanding; `Connection::request` does not enforce it.

A server MAY run requests concurrently and answer in any order up to `max_inflight`. `Connection` supports it. The daemon reads while a command runs but does not run the commands of one connection concurrently: it sends `max_inflight = 8` and splits each connection with `Connection::split` into three loops (`main.rs` `handle_session`). A reader task checks every frame with the rules above and never waits for a command. It hands an accepted `REQUEST` to a queue with room for `max_inflight` jobs; it answers a request that arrives at the cap with `overloaded` straight away, whatever is running. The command loop runs the accepted requests one at a time, in arrival order, so the `RESPONSE`s of one connection arrive in the order of their requests and each carries its own id. A writer task owns the write half and writes what the other two queue, in order, each frame under the 30-second write deadline. Because an id stays tracked until its `RESPONSE` is written, at most `max_inflight` requests are running or queued, which bounds both queues. The connection layer releases an id before the last byte of its `RESPONSE` is written, so a client that reuses the id the moment it has read the answer is not refused. A server that processes one request at a time is conformant; so is this one.

### 8.4 RESPONSE

The server answers each `REQUEST` with exactly one `RESPONSE` carrying its id, unless it started a stream (section 10). The body is a single-entry map: `{ok: value}` or `{err: {code, message[, details]}}`. `err` is closed; `details`, when present, is any JSON-shaped value and a present `null` is rejected; the daemon never sets it. `code` is a stable snake_case identifier and `message` is for people. A client MUST branch on `code`, never on `message`, and MUST tolerate a code it does not know.

A `RESPONSE` for an id that is not in flight, or whose request is streaming, is `protocol_violation` on receipt, and a server that tries to send one gets `ConnectionError::Usage` (`Connection::check_incoming`, `check_outgoing`). The id is released when the `RESPONSE` is sent or received. When the daemon cannot send an answer it computed (for example it exceeds `max_frame`), it replaces it with `{err: {code: "internal"}}` so the id is released (`spine-core` `main.rs` `reply`).

## 9. PING and PONG

A `PING` carries a nonzero id and a uint token. The receiver MUST answer with a `PONG` carrying the same id and the same body bytes. `Connection::recv` answers inside the call and does not hand the `PING` to the caller; the reader half of a split connection cannot write, so it returns the `PONG` as `Incoming::Reply` for the caller to queue to the writer. A `PONG` is handed to the caller; `Connection` does not match it to an outstanding `PING`, so an unsolicited `PONG` is accepted, and a `PING` id is never entered in the in-flight table and may equal a request id. A body that is not a uint (negative, float, text) is `protocol_violation` for both types (`Connection::accept`).

Every frame read, `PING` included, restarts the idle timer of `recv_timeout`. The daemon never sends a `PING` and ignores a `PONG`. The daemon reads while it executes a command, so a `PING` sent meanwhile is answered at once, before that command's `RESPONSE` (section 16, item 9). The idle timer of the reader is suspended while any request is in flight, so a client that waits for a slow answer is not idle; if it fires with nothing in flight the daemon sends `CLOSE(timeout)`. After the last answer, a silent client is therefore closed between one and two idle periods later. The SDK's `ping` sends a `PING` with a counter id from 1 and the send time in milliseconds as the token and waits, with no timeout of its own, for the `PONG`.

## 10. Streaming frames

Specified; no producer exists. Nothing in the workspace sends `STREAM_START`, `STREAM_TOKEN` or `STREAM_END`, no command is defined to stream, and the daemon ignores `STREAM_CANCEL` (the connection layer delivers it only for an in-flight id; the daemon's reader sees it while a command runs and drops it, because v1 defines cancel only for a stream and no command streams: there is no frame that cancels a request). What is implemented is the frame types, the state machine in `Connection`, the body types and `native.rs` `encode_stream_body` and `decode_stream_body`.

A stream is the reply to a `REQUEST` whose command is defined to stream; the stream id is the request id.

1. The server sends `STREAM_START`, then zero or more `STREAM_TOKEN`, then `STREAM_END`, all with the request id. A request that has received `STREAM_START` receives no `RESPONSE`; a request that fails before then receives a `RESPONSE` with `err`. `Connection` enforces this: `STREAM_START` only while the request is pending, `STREAM_TOKEN` and `STREAM_END` only while it is streaming, a `RESPONSE` never while streaming; each violation is `protocol_violation`. `STREAM_END` releases the id.
2. Bodies (`agentic.rs`), all closed: `STREAM_START` `{role: "assistant"|"tool"|"user"|"system", model: text[, trace: TraceContext][, decode_hints: DecodeHints]}`; `STREAM_TOKEN` `{seq: uint, data: StreamData[, usage: StreamUsage]}` with `StreamData` one of `{"text": text}`, `{"bytes": bytes}`, `{"tool_call": ToolCall}`; `STREAM_END` `{reason: StreamEndReason[, usage]}` with the reason one of the texts `"stop"`, `"length"`, `"tool_use"`, `"content_filter"`, `"cancelled"` or `{"error": {code: text, message: text}}`; `StreamUsage` `{input_tokens: uint, output_tokens: uint}`; `DecodeHints` is a map of optional sampling parameters (`temperature`, `top_p`, `repetition_penalty`, `presence_penalty`, `frequency_penalty` floats; `top_k`, `max_tokens` u32; `seed` u64; `stop_sequences` array of text).
3. `seq` starts at 0 and increases by one per token per stream. A producer MUST NOT repeat or skip a value. No code checks it (section 16, item 12).
4. The client cancels with `STREAM_CANCEL` (the stream's id, an optional `reason` text; with none, the empty map). `Connection` lets a client send it for any id; a server that receives it for an id that is not in flight ignores it silently, because it races with `STREAM_END`. A producer SHOULD stop within one token and send `STREAM_END` with reason `cancelled`. Cancel latency is bounded below by a server that reads frames while it produces; the daemon now does (section 16, item 9), so the first producer needs only a cancellation signal from its reader to the command loop.
5. Frames of several streams and responses may interleave; `seq` is per stream.
6. The latent-codec chunk (`Encoded`) and `StreamStart.stream_codec` are not part of v1; they were removed with the research code (Appendix A).

## 11. Extensions

An extension id is opaque, case-sensitive ASCII text of at most 128 bytes, conventionally `<domain>/<name>/<major>`; a change that is not backward compatible changes the major number and so the id. Ids starting with `spine.` are reserved for extensions specified in the SPINE specification set. No extension is defined.

The client lists ids in `HELLO.extensions`; the server answers the accepted subset in `HELLO_ACK.extensions`, in the client's order (6.4). The index of an extension is its position in that array. Only accepted extensions may be used.

`EXT` (`0x80`) has the body `[index, kind, payload]`: a three-element array whose first element is a uint index into `HELLO_ACK.extensions`, `kind` a uint defined by the extension, `payload` any deterministic CBOR item. The request id follows the extension's rules; 0 is allowed. `Connection` checks only that the body is an array of three starting with a uint, and that the index is in range; otherwise `protocol_violation` (`connection.rs` `ext_index`, `check_ext`). It does not check `kind` or `payload`. The daemon accepts no extension, so every `EXT` frame it receives is a violation. An extension specification MUST define its kinds, payloads and error handling; this document defines none.

The core command set and the frame types are closed; an extension cannot add a variant to them.

## 12. Commands

### 12.1 Encoding and decoding

A `REQUEST` body is one of 21 commands (`lib.rs` `BrowserCommand`, `COMMAND_NAMES`). A command without fields is the text of its name; a command with fields is a single-entry map `{name: fields}`, where `fields` is a map with text keys. `execute_binary`, `tool_call` and `capability_query` carry one struct whose fields are the map. Names are lower snake_case and exactly as listed.

The daemon decodes with `native.rs` `decode_command`: the body must be deterministic CBOR (else the connection closes, 7.2), must decode as a command, and the decoded command must re-encode to exactly the received bytes (`decode_strict`). The re-encode check catches bodies that the decoder alone would accept, for example `{"get_ur": {"x": 1}}`, which would otherwise read as the unit command. Failure is `unknown_command` if `claimed_command_name` finds a name outside `COMMAND_NAMES` (a text body, or a map with one entry and a text key), else `invalid_request`.

Integer widths in the tables are those of the Rust fields: `usize` is read as a u64. Optional fields are omitted when absent and a present `null` is `invalid_request`. "JSON" means a JSON-shaped value (5.3).

### 12.2 Field tables

Commands with no fields: `get_ur`, `get_raw_html`, `get_binary_capabilities`, `namespace_status` (each is its name as text).

| Command | Field | Type | Optional | Bytes |
| --- | --- | --- | --- | --- |
| `navigate` | `url` | text | no | |
| `execute_binary` | `instructions`, `data`, `render_start`, `exported_functions`, `capabilities` | a `SpineBinary` (12.3) | no | |
| `store_knowledge` | `key` | text | no | |
| | `value` | JSON | no | |
| | `tags` | array of text | no | |
| `query_knowledge` | `query` | text | no | |
| | `tags` | array of text | no | |
| | `limit` | uint (usize) | no | |
| `resolve_name` | `name` | text | no | |
| | `if_none_match` | 32 bytes | yes | yes |
| `resolve_names` | `names` | array of text | no | |
| `find_providers` | `capability` | text | no | |
| | `limit` | uint (usize) | yes | |
| `publish_name` | `record` | JSON (a name record) | no | |
| `fetch_name` | `name` | text | no | |
| | `if_none_match` | 32 bytes | yes | yes |
| `crawl_names` | `seed` | text | no | |
| | `max_depth` | uint (u32) | yes | |
| | `max_visits` | uint (usize) | yes | |
| `publish_resource` | `record` | JSON (a name record) | no | |
| | `content_type` | text | no | |
| | `data` | bytes | no | yes |
| `read_resource` | `hash` | 32 bytes | no | yes |
| | `offset` | uint (u64) | no | |
| | `max_bytes` | uint (u32) | no | |
| `mesh_call` | `peer_id` | text | no | |
| | `peer_key` | 32 bytes | no | yes |
| | `payload` | JSON | no | |
| `tool_call` | `name` | text | no | |
| | `args` | JSON | no | |
| | `trace` | `TraceContext` | yes | |
| `capability_query` | `selector` | `CapabilitySelector` | no | |
| `delete_resource` | `hash` | 32 bytes | no | yes |
| | `owner` | 32 bytes | no | yes |
| | `proof` | 64 bytes | no | yes |
| `delete_knowledge` | `key` | text | no | |

A byte field of the wrong length (not 32 bytes for the 32-byte fields, not 64 for `proof`) is `invalid_request`.

**`delete_resource`** removes the bytes of a blob that `publish_resource` stored; `hash` is its resource id, `owner` the Ed25519 public key that published it, and `proof` that key's signature, made for this connection, over the message

```
M = "spine-wire-delete-resource-v1" 00 || hash (32 bytes) || binding (32 bytes)
```

where the domain text is 29 ASCII bytes followed by one zero byte and `binding` is the `HELLO_ACK.binding` of this connection (6.3). `M` is 94 bytes. The signature is Ed25519, verified strictly as in 6.6 (`S` below the group order, a small-order key or `R` refused). The domain text is unique to this message, so a signature made for `IDENTIFY` or for anything else does not verify, and a proof made for another connection does not verify on this one: a captured request cannot be replayed on another connection. Within one connection a proof can be repeated, which deletes nothing the second time (see below).

The daemon checks, in this order:

1. The body decodes (7.4): else `unknown_command` or `invalid_request`, as for any command, and a byte field of the wrong length is `invalid_request`.
2. The proof: strict verification of `proof` over `M` under `owner`. A proof that does not verify, including any proof under a small-order `owner` (the all-zero key of an ownerless blob is one), is `delete_refused`. Nothing else is read or changed before this step.
3. The store: the blob is deleted when it exists and `owner` is exactly the key recorded as its publisher; the reply is `{deleted: true}`. A hash nobody holds, and a hash held by another key, are both `{deleted: false}`, a success: a client that proves some key learns nothing about blobs that key does not own.

Who may delete: only the recorded publisher, which is the key the store recorded when it accepted the blob. That key is the name's inception key (`02-names.md` decision 42), the key of the record's own name. For a name whose key has rotated, the current key is not the recorded publisher and cannot delete; the superseded inception key can. That is the same rule as for storing and for the per-publisher quota, and section 16, item 25 records it as a limit. Deleting is allowed while a name still refers to the blob: the name record is not touched, `resolve_name` still answers, and `read_resource` of the hash then answers `resource_not_found`, as for any hash nobody holds. The accounted bytes (the row length plus 1,024) are released from both the publisher's share and the store total. A repeat is `{deleted: false}`. The blob can be published again.

**`delete_knowledge`** removes every entry of this connection's session whose key equals `key` exactly (a byte-for-byte comparison of the text: no prefix, no case folding, no tag filter) and replies `{deleted: n}` with the number removed. Entries of other sessions are not read or changed. The daemon-wide knowledge budget (13, `knowledge_capacity`) is released by the same amount the entries were charged, so the room can be stored into again. A key nothing holds, a connection that has stored nothing, and a repeat all reply `{deleted: 0}`, a success. The command does not create a session and is not counted against `server.max_sessions`. It does draw on the connection's command bucket (14).

### 12.3 Shared types

All closed, all optional fields omitted when absent.

| Type | Wire form |
| --- | --- |
| `TraceContext` | `{trace_id: 16 bytes, span_id: 8 bytes, flags: uint 0..255[, state: array of [text, text] pairs, omitted when empty]}` |
| `CapabilitySelector` | the text `"all"`, or `{"exact": text}`, `{"prefix": text}`, `{"semantic": {embedding: array of float, top_k: uint (u32)}}` |
| `Capability` | `{uri: text, description: text, input_schema: JSON, output_schema: JSON[, embedding: array of float]}` |
| `CapabilityAdvertisement` | `{agent_id: text, capabilities: array of Capability}` |
| `ResourceChunk` | `{hash: 32 bytes, offset: uint, total_bytes: uint, content_type: text, data: bytes, complete: bool}` |
| `SpineBinary` | `{instructions: array of Instruction, data: bytes, render_start: uint, exported_functions: map text to uint, capabilities: array of text}` |

`Instruction` (`lib.rs`) is externally tagged: a unit variant is its name as text, a one-field variant is `{name: value}`, a struct variant is `{name: {fields}}`. Variants and fields (ids and indexes are u32 unless noted, `uint` is usize or u64): `define_element {id, tag: text}`, `set_attribute {id, key: text, value: text}`, `add_child {parent_id, child_id}`, `emit_event {name: text, payload: JSON}`, `stream_latent {vector: array of float}`, `morph_protocol {seed: u64}`, `decoy {noise: array of float}`, `declare_state {name: text, initial_json: JSON}`, `update_state {name: text, value_json: JSON}`, `push` JSON, `pop`, `load` text, `store` text, `bin_op` one of `add sub mul div mod eq ne lt le gt ge and or concat`, `unary_op` one of `not neg`, `jump` uint, `jump_if` uint, `jump_if_not` uint, `call {name: text, num_args: uint}`, `call_target` uint, `return`, `define_element_from_stack {id}`, `set_attribute_from_stack {id, key: text}`, `add_child_from_stack {parent_id, child_id}`, `emit_event_from_stack {name: text}`, `define_text_from_stack`, `declare_state_from_stack {name: text}`, `update_state_from_stack {name: text}`, `navigate_from_stack`, `search_from_stack`, `store_knowledge_from_stack {tags: array of text}`, `query_knowledge_from_stack {tags: array of text, limit: uint}`.

The name record inside `record` is a `spine_name::NameRecord`, specified by layer 2 and not here; for orientation its keys are `name`, `seq`, `ttl_secs`, `published_at`, `endpoints`, `capabilities`, `content_hash`, `links`, `meta`, `signature` (`spine-name` `record.rs`).

### 12.4 Reply shapes

The `ok` value, by command (`spine-core` `main.rs` `handle_command`, `handle_session`). "JSON" values are JSON-shaped.

| Command | `ok` value |
| --- | --- |
| `navigate` | `{status: "success", session_id: text}` |
| `get_ur` | `{parsed_ur: JSON, vdom_tree: JSON or null, vdom_ur: JSON or null}` |
| `get_raw_html` | `{html: text}` |
| `execute_binary` | `{status: "executed", stats: {instructions_executed, compile_time_us, execution_time_us, wasm_size_bytes, memory_used_bytes}, elements, events, actions, latent_streams_count: uint, patches}` |
| `store_knowledge` | `{status: "knowledge_stored"}` |
| `query_knowledge` | `{results: array of {key: text, value: JSON, tags: array of text, timestamp_ns: uint}}`, at most `limit` entries and never more than 128, from this connection's session only |
| `get_binary_capabilities` | `{capabilities: array of text}` of the session's current binary, empty if none |
| `resolve_name` | a `NameResolution` (below) |
| `resolve_names` | `{resolutions: array of NameResolution}`, in request order |
| `find_providers` | `{capability: text, providers: array of name record, count: uint}` |
| `publish_name`, `publish_resource` | a publication report: `{status: "published", name, stored, local_persistent, replicas, replication_sent, replication_failed, replication_broadcast, replication_error, replication_acknowledged, durable, retry}` |
| `fetch_name` | `{resolution: NameResolution, endpoints: array of text}` |
| `crawl_names` | `{visited: array of {name, depth, capabilities, links} or {name, depth, unresolved: true}, skipped: uint, pending: uint}` |
| `read_resource` | a `ResourceChunk` (typed, byte strings) |
| `namespace_status` | `{enabled: bool, connectivity: text, peers: array[, agent_id, public_key, tasks_enabled]}` |
| `mesh_call` | the peer application's reply, JSON; the SDK reads `result`, or `error.{code, message}`, from it |
| `tool_call` | the tool's content, JSON |
| `capability_query` | a `CapabilityAdvertisement` (typed); `agent_id` is the node's DID when a namespace is configured, else `"spine-core:local-unattested"`; a `semantic` selector matches nothing |
| `delete_resource` | `{deleted: bool}`: true when a blob was removed (12.2) |
| `delete_knowledge` | `{deleted: uint}`: the number of entries removed, 0 when none (12.2) |

`NameResolution` is externally tagged, with one variant per outcome (12.5):

| Outcome | Fields |
| --- | --- |
| `resolved` | `{record, provenance, log}` |
| `unchanged` | `{ttl_secs: uint32}` |
| `not_found` | `{name}` |
| `invalid` | `{name, reason}` |
| `unavailable` | `{name, reason}` |
| `retired` | `{name, log}` |
| `compromised` | `{name, logs}` |
| `revoked` | `{record, provenance, log}` |

`record` is a JSON-shaped `NameRecord` (5.3). `provenance` is one of `cache`, `stale_cache`, `local`, `network`, `address`. `log` is an array of 0 to 64 succession entries, `logs` an array of exactly two arrays of 1 to 64 entries each; an entry is the JSON object of `02-names.md` 10.1 (below). Every variant is a closed map: a missing or extra key is a decode failure. `resolve_name` always answers `ok`; a bad or missing name is a variant, not an `err`.

The daemon's tools (`tool_call`) are `spine.html/parse` and `spine.content/hash`, each taking `{text}` of at most 1 MiB.

### 12.5 Name outcomes and what a client verifies

The node holds, for a name, a record and possibly a succession log (`02-names.md` section 10). A client that wants to trust a rotated name must see that log, so the node sends the log it holds and the client checks it. The node never asks a client to trust it.

**Carrying a log.** A log is an array of entries in the JSON form of `02-names.md` 10.1, the convention a name record has in the same answer (5.3). An entry is a closed map of exactly eight keys: `index` and `effective_at` unsigned integers; `inception_key`, `key` and `signature` non-empty text of at most 128 bytes; `prev` and `commitment` such text or `null`; `kind`, `rotate` or `retire`. The wire layer checks these shapes and the sizes; whether the signatures verify and the keys chain is the client's check (below), because a decoder that read more would have to repeat the succession rules. A log of more than 64 entries is a decode failure.

**The outcomes.**

- `resolved` always carries `log`. Empty means exactly "this node holds no log for the name": the whole truth for a name that never rotated and nothing more. A node does not send `resolved` for a record whose head claim (`spine.succession`, `02-names.md` 10.7) reaches past the log it holds: it answers `unavailable` with a reason that says the log is missing. `not_found` would be a lie (a record exists) and `resolved` with a short log would invite the client to refuse a record the node could have explained.
- `retired` carries `name` and a `log` of 1 to 64 entries whose last entry is `retire` (`02-names.md` 10.3): no key may sign for the name, ever, for any verifier that holds the log.
- `compromised` carries `name` and `logs`, exactly two logs of 1 to 64 entries each: the two logs of an equivocation proof (`02-names.md` 10.5, 10.10 decision 51). Both verify from the name's inception key and neither is a prefix of the other.
- `revoked` carries the tombstone as `record`, its `provenance` and the `log` the node holds, which the tombstone is judged under (a tombstone of a rotated name is signed by the current key, `02-names.md` decision 35). A tombstone lapses (7 days): `revoked` is not permanent, and a name that has lapsed resolves as `not_found` again.
- `unchanged` is sent only when the node would otherwise have sent `resolved` with that record; for a retired, compromised or revoked name it sends that outcome instead.

`resolve_names` carries one of these per name, in request order, and `fetch_name` carries one in `resolution`. `find_providers` and `crawl_names` are unchanged: providers are records without logs, and a client that wants a provider's key history resolves the name.

**What a client MUST verify.** The name asked for decides the inception key: for a `did:` name it is the key in the name. A client MUST NOT act on an outcome that carries evidence until it has checked the evidence:

1. `resolved`: the record is the record of the name asked for; every entry of `log` verifies from the inception key, in order (signature of the key current at each index, `prev` chain, `commitment` reveal, `effective_at` order, nothing after a `retire`: `02-names.md` 10.3); and the record is authorised by the state the log gives (`02-names.md` 10.6): signed by the key current at the log's head, with its head claim, if any, no higher than the log reaches. A record that needs more log than the node sent is not authorised (`NeedSuccessionLog`); a record signed by a superseded key is refused. A name with no key (`blob:`, `host:`) has no log and the log must be empty.
2. `retired`: the log verifies from the inception key and its state is retired.
3. `compromised`: both logs verify from the inception key, and neither is a prefix of the other (`02-names.md` 10.5). The two are the proof; the client treats the name as compromised and authorises no record for it.
4. `revoked`: the record is a tombstone of the name asked for (`02-names.md` 3.7), it has not lapsed, the log verifies, and the tombstone's signature verifies under the key the log makes current.
5. A client that also holds a log of its own combines the two by the rule a verifier's log book keeps (`02-names.md` 10.5): a log that is a prefix of the other gives way to the longer valid one, and two valid logs that conflict are an equivocation, which is the `compromised` outcome.

Evidence that fails any of these is not a statement about the name: the node is wrong or hostile. A client MUST report it as a failure that is distinct from `not_found`, `unavailable`, `retired` and `compromised`, and MUST NOT treat the name as absent because of it. (The reference client reports `NameOutcomeError::InvalidEvidence`, naming the outcome and the check that failed.) A node cannot make a client accept a record the key history forbids; it can withhold or truncate a log, which leaves the client with `NeedSuccessionLog`, and it can answer `retired` or `compromised` falsely only by producing evidence that verifies, which needs the name's keys. What stays open is `02-names.md` 10.8: a node that is behind holds a shorter log than exists, which a client cannot see, and a record signed by a superseded key that carries no head claim is accepted by a client that holds no entry superseding it (10.6).

**Bound.** A log is at most 64 entries of about 600 bytes of JSON, so an answer is at most about 40 KB with `log` and about 80 KB with `compromised`. One answer fits any frame (2.3). A `resolve_names` batch of 256 names with full logs could exceed `max_frame` (16 MiB at most); the rule of 8.4 applies, and a reply too large to send is replaced by an `internal` error. The 256 limit of `resolve_names` is unchanged.

**What the reference daemon answers.** From its local resolver's log book (`spine-core` `name_outcome.rs`): a record it holds, with the log it holds, is `resolved`; a name whose log ends in `retire` is `retired`; a name with an equivocation proof is `compromised`; a fresh tombstone is `revoked`; a record that carries a head claim the held log does not reach is `unavailable`. A name that the mesh reports as retired is adopted into the local book first and then answered as `retired`. A name known to be compromised only to the mesh service (which holds the proof, and exposes no accessor to it) stays `unavailable` with the reason; closing that needs a change to `spine-mesh` (section 16, item 22).

## 13. Error codes

Complete list a v1 daemon produces (`body.rs` `ALL_ERROR_CODES`, 51 codes), with the condition and the place that raises it. A client MUST tolerate a code not in this list.

| Code | Condition | Raised in |
| --- | --- | --- |
| `overloaded` | server at `max_inflight` (by the daemon when a ninth request arrives while eight are running or queued, 8.3) | `connection.rs` `accept` |
| `unknown_command` | 7.4 | `native.rs` `decode_command` |
| `invalid_request` | 7.4; also `read_resource` with `max_bytes` outside 1 to 65,536 or an `offset` beyond the resource | `native.rs`; `main.rs` `handle_command` |
| `internal` | an answer too large to send; a `resolve_names` task failed; a `read_resource` failure that is neither of the above (including a stored blob that fails its integrity check); a `spine.html/parse` result that could not be encoded; the unreachable arm for `tool_call` and `capability_query` in `handle_command` | `main.rs` `reply`, `handle_command`; `tools.rs` `invoke` |
| `rate_limit` | the connection's command bucket (burst 100, 10 per second) is empty for a session command; the connection's separate bucket is empty for a `tool_call` | `main.rs` `check_rate_limit`, `handle_session` |
| `session_capacity` | the connection's first session command found `server.max_sessions` sessions; the connection stays open | `main.rs` `handle_session`, `create_session` |
| `session_not_found` | `get_ur` or `get_raw_html`, and the session record is gone | `main.rs` `handle_command` |
| `session_expired` | `navigate` fetched a page and the session record was gone | `main.rs` `handle_command` |
| `no_page_loaded` | `get_ur` or `get_raw_html` before a successful `navigate`; `get_ur` does not use it for a page that fails to parse (`parse_error`) | `main.rs` `handle_command` |
| `capability_denied` | `execute_binary` lists a capability other than `network`, `storage`, `memory`, `search`, or the program requests an action it did not list (a navigation needs `network`, a search `search`, a knowledge write `storage`); nothing the program asked for is carried out. The list is the program's declaration, not a grant | `main.rs` `handle_command` |
| `execution_failed` | `execute_binary`: the program failed to compile, instantiate or run, ran out of fuel (host work draws on it too), or exceeded a fixed limit: 65,536 instructions; 64 KiB of string data; a host value stack of 1,024; 64 KiB per host value; 1 MiB of live host values; 4,096 elements, 4,096 events and 64 actions per run; 32 arguments per call; 16 MiB of guest memory; 4 navigations per run; 256 state variables per session. A program's refusal for knowledge past a limit is `knowledge_capacity`, and for an action the daemon does not perform `action_unsupported`; both are listed below and neither is this code. At most four programs run at once; one that finds no free slot for 2 s is answered `overloaded`. A program that runs longer than 2 s from the start of the guest is stopped at its next host call; building, parsing and compiling come before that and are bounded by the instruction cap, and one host call is not interrupted | `main.rs` `handle_command` |
| `knowledge_entry_too_large` | `store_knowledge`: the entry (key, compact JSON of the value and tags, in bytes) exceeds 64 KiB. The entry is not stored and nothing is charged | `main.rs` `handle_command`; `knowledge.rs` `admit_entry` |
| `knowledge_capacity` | a knowledge store would pass a bound: the session already holds 1,024 entries, or the daemon-wide budget of 32 MiB in total (each entry counts its bytes plus 128) is exhausted. From `store_knowledge`, and from `execute_binary` when the program's stores would pass either bound (the whole program is then refused before it runs, and nothing is reserved). `delete_knowledge` releases room | `main.rs` `handle_command`, `append_knowledge`; `knowledge.rs` `admit_entry` |
| `action_unsupported` | `execute_binary`: the run requests an action the daemon does not perform; the program is refused whole, before any action runs, naming the action. In a default build the performed actions are `navigate` and `store_knowledge`, and `search`, `query_knowledge` and `reason` are refused (`search` is performed only in a build with the `research` feature; `query_knowledge` and `reason` in none; the response lists a run's actions but has no field for one that was skipped) | `main.rs` `handle_command`; `actions.rs` `unsupported_action` |
| `namespace_unavailable` | `mesh_call` and no namespace is configured (checked before `peer_id`) | `main.rs` `handle_command` |
| `invalid_peer_id` | `mesh_call` with a namespace, and `peer_id` is not a UUID | `main.rs` `handle_command` |
| `mesh_call_failed` | `mesh_call` could not be delivered or the peer refused it | `main.rs` `handle_command` |
| `batch_too_large` | `resolve_names` with more than 256 names | `main.rs` `handle_command` |
| `lookup_failed` | `find_providers`: the local index failed | `main.rs` `handle_command` |
| `lookup_unavailable` | `find_providers`: the mesh lookup failed (only when peers are connected) | `main.rs` `handle_command` |
| `record_expired` | `publish_name` or `publish_resource`: the record is past its expiry | `main.rs` `handle_command` |
| `malformed_record` | `publish_name` or `publish_resource`: `record` does not decode as a name record | `main.rs` `handle_command` |
| `record_rejected` | `publish_name`: the store refused the record (signature or policy); `publish_resource`: the same after the bytes were stored (a blob this request added is removed) | `main.rs` `handle_command` |
| `publish_failed` | `publish_resource`: the store refused the blob (record verification, over 8 MiB, no signed resource id, id or content-type mismatch, no room under the accounted-byte quota after eviction; storage rules in 04-objects.md section 4.4) | `main.rs`; `resources.rs` `put_as`, which stores for the key that signed the record |
| `resource_not_found` | `read_resource`: no blob for the hash | `main.rs` `handle_command` |
| `delete_refused` | `delete_resource`: `proof` does not verify strictly over `M` (12.2) under `owner` for this connection's `binding`. A valid proof of a key that does not own the blob is not this code (`deleted: false`) | `main.rs` `delete_resource`; `resource.rs` `verify_delete_proof` |
| `invalid_seed` | `crawl_names`: `seed` does not parse as a name | `main.rs` `handle_command` |
| `invalid_args` | `tool_call` of a known tool: `args.text` is not text or exceeds 1 MiB (the tool name is checked first) | `tools.rs` `invoke` |
| `unsupported_tool` | `tool_call`: the name is neither tool, whatever `args` holds | `tools.rs` `invoke` |
| `parse_error` | `tool_call` `spine.html/parse`: parsing failed; `get_ur`: the loaded page does not parse | `tools.rs` `invoke`; `main.rs` `handle_command` |
| `outbound_disabled` | `navigate` and `outbound.enabled` is false | `outbound.rs` `fetch_checked` |
| `outbound_address_denied` | a resolved address is not public and the origin is not in `outbound.internal_origins` | `outbound.rs` `fetch` |
| `outbound_scheme_denied` | the scheme is not `http` or `https` | `outbound.rs` `parse_url` |
| `outbound_credentials_denied` | the URL has a user name or password | `outbound.rs` `parse_url` |
| `outbound_fragment_denied` | the URL has a fragment | `outbound.rs` `parse_url` |
| `outbound_redirect_denied` | the origin answered 3xx | `outbound.rs` `fetch` |
| `outbound_encoding_denied` | the response has a `Content-Encoding` other than `identity` | `outbound.rs` `fetch` |
| `outbound_response_limit` | the body exceeds `outbound.max_response_bytes` | `outbound.rs` `fetch` |
| `outbound_busy` | `outbound.max_concurrent` fetches are running, or the session already runs `outbound.max_concurrent_per_caller` fetches | `outbound.rs` `fetch` |
| `outbound_timeout` | the fetch exceeded `outbound.timeout_secs`, or no body chunk arrived within `outbound.read_timeout_secs` | `outbound.rs` `fetch_checked`, `fetch` |
| `outbound_invalid_url` | the URL does not parse | `outbound.rs` `parse_url` |
| `outbound_invalid_host` | the URL has no host | `outbound.rs` `parse_url`, `fetch` |
| `outbound_invalid_port` | no port can be determined, or the URL's effective port is not in `outbound.allowed_ports` and its origin is not in `outbound.internal_origins`; checked before any DNS lookup | `outbound.rs` `fetch` |
| `outbound_dns_failed` | name resolution failed | `outbound.rs` `fetch` |
| `outbound_client_failed` | the HTTP client could not be built | `outbound.rs` `fetch` |
| `outbound_request_failed` | sending the request failed other than by timeout | `outbound.rs` `fetch` |
| `outbound_body_failed` | reading the body failed other than by timeout | `outbound.rs` `fetch` |
| `outbound_invalid_utf8` | the body is not UTF-8 | `outbound.rs` `fetch` |
| `outbound_url_limit` | the URL is longer than 8192 bytes | `outbound.rs` `parse_url` |
| `outbound_dns_limit` | name resolution returned no address or more than 64 | `outbound.rs` `fetch` |
| `outbound_http_status` | the origin answered a status that is neither success nor redirect; `details` is `{status: uint}` | `outbound.rs` `fetch` |

`navigate` passes the fetcher's typed error code through unchanged (`main.rs` `navigation_failure`); only `outbound_http_status` carries `details`.

## 14. Limits, timeouts and configuration

Configuration keys are in `spine.toml` (`SPINE_CONFIG_PATH`) unless an environment variable is named. Source: `spine-core` `config.rs` unless another is named. Validation ranges are those of `SpineConfig::validate`.

| Limit | Value | Key or source | On violation |
| --- | --- | --- | --- |
| Frame length, before `HELLO` | 65,536 | `frame.rs` `PRE_HELLO_MAX_FRAME`; fixed | `CLOSE(frame_too_large)` |
| Frame length, after | negotiated `max_frame`, 1,024 to 16,777,216 | the daemon and the SDK both offer 16,777,216 and it is not configurable (`native.rs` `local_config`, `client_hello`) | `CLOSE(frame_too_large)` |
| Declared uncompressed length | `max_frame - 11` | fixed | `CLOSE(frame_too_large)` |
| zstd window | 2^23 bytes | `frame.rs`; fixed | `CLOSE(malformed_frame)` |
| CBOR depth | 32 | `cbor.rs` `MAX_DEPTH`; fixed | `CLOSE(malformed_frame)` |
| Requests in flight | 8 | `native.rs` `DAEMON_MAX_INFLIGHT` through `local_config`; not configurable | `overloaded` |
| `IDENTIFY` | at most 1 per connection, before any request; `channel` empty; body 118 bytes, frame 133 | `identify.rs`, `connection.rs` `Tracker`; fixed | `CLOSE(protocol_violation)` |
| `binding` | 32 bytes, fresh for every connection | `hello.rs` `fresh_binding`; fixed | |
| `HELLO` deadline | 10 s | `server.hello_timeout_secs` (`SPINE_HELLO_TIMEOUT`); must be above 0 | `CLOSE(timeout)` |
| Idle | 300 s | `server.idle_timeout_secs` (`SPINE_IDLE_TIMEOUT`); must be above 0. Restarted by every frame read, `PING` included, and covers a half-sent frame; suspended while a request is in flight (9). | `CLOSE(timeout)` |
| Connections, total | 256, 1 to 65,536 | `server.max_connections`; counts TCP, WebSocket and QUIC, handshakes included | closed without a byte |
| Connections per IP | 50, above 0 | `server.max_connections_per_ip` | closed without a byte |
| Sessions | 1,000, 1 to 65,536 | `server.max_sessions` (`SPINE_MAX_SESSIONS`) | `session_capacity` |
| Request rate | burst 100, refill 10 per second, per connection for session commands; `tool_call` has its own bucket per connection | `main.rs` `check_rate_limit`; fixed | `rate_limit`, connection stays open |
| Write deadline | 30 s per frame | `native.rs` `WRITE_DEADLINE`; fixed | connection abandoned, no `CLOSE` (7.5) |
| Best-effort `CLOSE` write | 1 s | `connection.rs`; fixed | abandoned |
| TLS handshake, WebSocket upgrade, QUIC connect, QUIC first stream | 10 s each | `main.rs`; fixed | dropped |
| WebSocket message and frame, server | 16 MiB | `websocket.rs`; fixed | transport error |
| Listener ports | 8080, +1, +2 | `server.port`, `server.ws_port_offset`, `server.quic_port_offset` | |
| Bind address | `127.0.0.1` | `server.host`; non-loopback needs `tls.mutual_tls` or `server.allow_unauthenticated_remote` | startup refused |
| Shutdown drain | 30 s | `server.shutdown_timeout_secs` (`SPINE_SHUTDOWN_TIMEOUT`) | process exits |
| `resolve_names` batch | 256 names | `main.rs`; fixed | `batch_too_large` |
| Knowledge | 1,024 entries per session, 64 KiB per entry, 32 MiB in total (each entry counts its bytes plus 128) | `knowledge.rs`, `main.rs`; fixed | `knowledge_capacity`; `knowledge_entry_too_large` |
| `delete_resource` proof | 64 bytes, over a 94-byte message | `resource.rs` `DELETE_RESOURCE_DOMAIN`; fixed | `delete_refused` |
| Resource upload | 8 MiB (`MAX_RESOURCE_BYTES`) | `resource.rs`; fixed | `publish_failed` |
| Resource read chunk | 1 to 65,536 bytes | `resource.rs` `MAX_RESOURCE_CHUNK`; fixed | `invalid_request` |
| Resource store | 256 MiB of accounted bytes (row length plus 1,024 per blob), 64 MiB fair share per publisher; see 04-objects.md section 4.4 | `resources.rs` `MAX_STORED_BYTES`; fixed | `publish_failed` |
| Tool text | 1 MiB | `tools.rs`; fixed | `invalid_args` |
| Navigation | `[outbound]`: `enabled` true, `internal_origins` [] (at most 64), `max_response_bytes` 2 MiB (1 to 8 MiB), `timeout_secs` 30 (1 to 300), `max_concurrent` 16 (1 to 256), `allowed_ports` [80, 443] (1 to 16 entries, none 0), `read_timeout_secs` 10 (1 to 300, capped at `timeout_secs`), `max_concurrent_per_caller` 4 (1 to 256, capped at `max_concurrent`), `allow_link_local_exceptions` false; URL at most 8,192 bytes | `outbound.rs` `FetchConfig` | `outbound_*` |
| SDK TCP connect, TLS handshake | 10 s each | `spine-agent` `AgentClient::connect`, `connect_tls` | error |
| SDK `HELLO_ACK` | 10 s | `native.rs` `DEFAULT_HELLO_DEADLINE` | error |
| SDK `capabilities`, `call_tool` | 10 s, 30 s | `spine-agent` | error; the client MUST then reconnect (`recv` is not cancel safe) |

No other SDK call has a deadline of its own.

## 15. Conformance

### 15.1 Vectors and checker

`docs/spec/00-wire.vectors.json` holds conformance vectors generated by `scripts/wire_v1.py`, an implementation of this specification written independently of the Rust code in Python 3 with only the standard library. Two commands, run from the repository root, check them:

```
python scripts/wire_v1.py check docs/spec/00-wire.vectors.json
python scripts/wire_v1.py generate | diff - docs/spec/00-wire.vectors.json
```

The first runs every vector through the reference; the second fails if the committed file differs from what the generator writes (compare with line endings normalised when a checkout has converted them; the committed file is LF, `.gitattributes`). The CI job `wire-vectors` runs both. The Rust side is `src/spine-protocol/tests/conformance_vectors.rs`, which feeds every vector to `cbor::check_deterministic`, `frame::decode_frame`, `frame::encode_frame`, the typed decoders of `native.rs`, `Hello::from_frame`, `HelloAck::from_frame`, `hello::negotiate`, `Identify::from_frame` and `Identify::verify`, `native::decode_strict::<NameResolution>`, `verify_strict` and a server `Connection` over an in-memory stream.

The reference signs and verifies with the pure-Python RFC 8032 Ed25519 of `scripts/names_v1.py` (which checks itself against the RFC's test vectors when the script starts), including the strict rule: `S` below the group order, a small-order key or `R` refused. It is not independent of the name reference, and it is slow.

The file has these sections: `valid_bodies` (deterministic CBOR the decoder must accept), `invalid_bodies` (each with the rejecting `CborError` variant), `valid_frames` (decode, re-encode and typed read), `invalid_frames` (each with the `FrameError` variant and close code), `incomplete_frames` (valid prefixes; the number of further bytes needed) and `handshakes` (a `HELLO` frame, the server configuration, with its `binding`, `authority` and `time`, and the expected `HELLO_ACK` or `CLOSE`). Four sections cover the revision of 1.5: `hello_acks` (`HELLO_ACK` bodies a client accepts, or refuses with `protocol_violation`: each missing or mistyped key, a `binding` of 0, 31 or 33 bytes, an `authority` of another text, a `time` that is not a uint), `identify` (an `IDENTIFY` body against the two handshake bodies it was signed for: valid, with the test seed, the signed message `M` and the deterministic signature; and invalid: a wrong binding, a wrong `HELLO`, a flipped signature bit, another key, a non-empty `channel`, wrong lengths and types, a missing or extra key, a body that is not a map, bodies that are not deterministic CBOR, a small-order key, a small-order `R` and an `S` not below the group order), `identify_sessions` (the frames a server receives after the handshake and the outcome: the identity, or the `CLOSE` code; `PING` and `PONG` before the proof, a second proof, a proof after a request, after a `STREAM_CANCEL` or an `EXT`) and `ed25519_strict` (the verdicts of plain RFC 8032 verification and of strict verification on the same inputs; the Rust test checks the strict one). A seventh section covers the names step of 1.5: `name_results` (the `ok` value of a `resolve_name` answer: each outcome with and without logs, logs of 0, 1, 3 and 64 entries, and the refusals: a missing, mistyped or extra key, a log of 65 entries, one log or three in `compromised`, an empty log in `retired`, an entry with a missing, extra or mistyped field). Four sections cover the third step of 1.5: `delete_proofs` (the `delete_resource` proof: valid ones with the test seed, the signed message `M` and the deterministic signature; invalid ones: a proof made for another connection or another hash, by another key, under another owner, a flipped bit in `R` or `S`, an `S` not below the group order, a small-order owner, the all-zero owner, and messages with another domain, without the domain, or with the hash and binding swapped), `invalid_commands` (request bodies of the delete commands that are `invalid_request`: a byte field of the wrong length or type, a missing or extra field, a bare name; and two `unknown_command` names), `delete_results` (the `ok` value of the two delete replies: a boolean for `delete_resource`, an unsigned integer for `delete_knowledge`, and the refusals) and `error_codes` (the 51 codes in the order of the table of 13; the checker also compares it with that table). Frames with flag bit 0 hold their body in a zstd frame of raw blocks, which is valid zstd and not smaller. A `note` on a vector explains a rule; since this revision no note records a disagreement between the text and the implementation.

### 15.2 What conforming means

An implementation conforms to v1 as a sender if every frame it emits satisfies sections 2 to 5, 6 (client or server side), 8 and 9, and as a receiver if it rejects, with the outcome of 7.2 to 7.4, what those sections tell it to reject. It passes the vectors if it gives each vector's stated result. Behaviour left to a command (what `navigate` does) is outside the wire layer.

### 15.3 Worked examples

Each is reproduced byte for byte by the reference and by the Rust test (vectors `c12_hello`, `c12_request_tool_call`, `c12_ping_token_42_id_7`).

Example 1: the client's `HELLO` offering version 1, a 16 MiB frame limit and no extensions. The body is the map `{"versions": [1], "max_frame": 16777216, "extensions": []}`; keys sort by encoded bytes: `versions` (`68 ...`, 8 characters), `max_frame` (`69 ...`, 9), `extensions` (`6a ...`, 10).

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

Example 2: `REQUEST`, id 1, the command `tool_call` with `{"name": "spine.content/hash", "args": {"text": "hi"}}`; inside the inner map `args` (`64 61 ...`) sorts before `name` (`64 6e ...`).

```
00 00 00 3d                     length = 0x3d = 61 = 11 + 50
01 10 00                        version 1, REQUEST, flags 0
00 00 00 00 00 00 00 01         request id = 1
a1 69 74 6f 6f 6c 5f 63 61 6c 6c   map(1), "tool_call"
a2                              map(2)
  64 61 72 67 73                  "args"
  a1 64 74 65 78 74 62 68 69      {"text": "hi"}
  64 6e 61 6d 65                  "name"
  72 73 70 69 6e 65 2e 63 6f 6e 74 65 6e 74 2f 68 61 73 68   "spine.content/hash"
```

A `PING` with token 42 and request id 7 is `00 00 00 0d 01 04 00 00 00 00 00 00 00 00 07 18 2a`: 4 + 11 + 2 = 17 bytes.

### 15.4 What the vectors do not cover

The vectors cover the frame codec, the CBOR profile, the first-frame bodies and negotiation. They do not cover, and the reference does not implement:

- Connection state: id reuse, direction, `HELLO` first, a second `HELLO`, extension indexes, `max_inflight`, timeouts, the streaming state machine. These are covered by the unit tests in `connection.rs` only. The one exception is the window of `IDENTIFY` (`identify_sessions`).
- Real zstd compression. The reference reads a zstd frame only as far as its structure goes (magic number, declared length against the limit, raw and RLE blocks) and reports others as opaque. So no vector exercises the window limit, a checksum, concatenated or skippable frames, or trailing bytes after the zstd frame (4, rule 3), where the Rust decoder and the reference may differ.
- Request classification. There is no vector for `unknown_command` or `invalid_request`. The reference's command check looks at names and field names only, not at field types, byte-field lengths or the re-encode rule, so it accepts bodies the daemon rejects (for example `navigate` with a number as `url`). JSON-shaped fields holding an integer below -2^63 are accepted by the reference and expected to be rejected by the daemon (5.3).
- Non-text map keys in `HELLO_ACK` and `CLOSE`: only `HELLO` has vectors for them (the unit tests in `hello.rs` cover all three).
- `CLOSE` bodies as invalid inputs: only `HELLO`, `HELLO_ACK` and `IDENTIFY` have invalid-body vectors.
- That `binding` is random and fresh. A vector fixes it; a test of the Rust code (`hello.rs`) checks that two negotiations differ.
- Transports, TLS, the daemon's session and rate behaviour and every reply shape of 12.4.

## 16. Known deviations and defects

Behaviour the implementation has that the specification author would change. Each is specified above as implemented; the right behaviour is what to do instead.

1. **Fixed: `read_resource` error codes.** `ResourceStore::read` returns a typed `ReadError` (`NotFound`, `InvalidRange`, `Internal`), and `main.rs` `read_failure` maps the variant to `resource_not_found`, `invalid_request` or `internal`. No code depends on message text. (An out-of-range `max_bytes` or `offset` still uses `invalid_request`; a dedicated code remains a possible refinement.)
2. **Fixed: `navigate` codes.** `outbound.rs` `Fetcher::fetch_text` returns a typed `FetchError` that carries its code; `main.rs` `navigation_failure` sends it unchanged. `outbound_url_limit`, `outbound_dns_limit` and `outbound_http_status` (status in `details.status`) are listed codes, and `navigation_failed` is retired: nothing produces it.
3. **Bytes inside JSON-shaped results travel as arrays of integers.** `NameRecord.content_hash` is a `[u8; 32]` and `serde_json::to_value` turns it into 32 integers inside the `record` of `publish_name`, `publish_resource`, `resolve_name`, `fetch_name` and `find_providers` (`spine-name` `record.rs`; `main.rs` `resolve_one`). The signature of the same record is base32 text, so one record uses two byte conventions, and the integer array is about twice the size. Right: one encoding, a typed `NameRecord` with byte strings on this wire.
4. **Fixed: `CLOSE(shutting_down)`.** `main.rs` sets a shared watch signal when shutdown starts; each `handle_session` selects on it between commands, sends `CLOSE(shutting_down)` (bounded by the connection layer's one-second write timeout) and ends, and a connection whose handshake completes during shutdown gets the same `CLOSE` from `serve_native`. A command already running is answered first, and a slow peer delays only its own task: the drain loop still ends at `server.shutdown_timeout_secs`. The QUIC listener is not covered.
5. **Fixed in part: `CLOSE` on refusal.** The refusal after the handshake (shutdown) carries `CLOSE(shutting_down)`, and `session_capacity` no longer ends the connection (no close code means "busy", so the client keeps the connection and may retry). A connection refused at `server.max_connections` or `server.max_connections_per_ip` is still closed without a byte, before any handshake, because the code set has no code for it; QUIC uses `refuse()`.
6. **The QUIC listener is untested and unreachable from the SDK.** `spine-core`'s `quic` feature is off by default and no CI job builds `spine-core` with it; no test and no client in the workspace connects over QUIC; the listener's certificate is self-signed with no client authentication, and the library's client builder skips server certificate verification (`spine-protocol` `lib.rs` `SkipServerVerification`). Startup is refused when `tls.enabled`. Right: either test and document it, or remove the listener.
7. **Fixed: one null-rejecting helper.** `hello.rs` uses `wirefmt.rs` `reject_null`; its private copy is gone.
8. **A schema failure in a `RESPONSE` or a `STREAM_*` body does not close the connection.** The original design closed with `protocol_violation`. `Connection` delivers any deterministic body; the client library's `native.rs` `decode_reply` and `decode_stream_body` report `NativeError::Body` to the caller, the in-flight id has already been released, and the connection stays open. Only `HELLO_ACK`, `CLOSE`, `PING` and `PONG` bodies are checked inside `Connection`. Right: the connection layer closes with `protocol_violation`, or the specification says the failure is local (as 7.4 now does); the two should not both be true.
9. **Fixed: the daemon reads while it executes a command.** `Connection::split` gives a reader and a writer half that share the in-flight table behind a mutex (never held across an await), and `main.rs` `handle_session` runs a reader task, the command loop and a writer task per connection (8.3). During a slow `navigate` or `execute_binary` a `PING` is now answered at once, a request over `max_inflight` (now 8) is refused with `overloaded`, and a closed connection is noticed. The reader's idle timer is suspended while a request is in flight. A command that is running when the peer disconnects (EOF, `CLOSE`, violation, idle timeout) or the write deadline fires is dropped at its next await point, its session detached as at any connection end; on shutdown it runs to completion, its answer is sent, requests still queued get no answer, and `CLOSE(shutting_down)` follows. What remains: commands of one connection still run one at a time (the session is per connection), so a request queues behind a slow one; `STREAM_CANCEL` is read and dropped, because v1 defines cancel only for streams and nothing streams, so a running request cannot be cancelled by the client except by closing the connection; and the QUIC listener serves its session through the same code but is covered by no test.
10. **Fixed: `get_ur` on an unparseable page.** `main.rs` `handle_command` reports the parse failure as `parse_error`; `no_page_loaded` now means only that no page is loaded.
11. **Fixed: `tool_call` checks the name first.** `tools.rs` `invoke` answers `unsupported_tool` for an unknown name whatever `args` holds; `invalid_args` applies only to the two known tools.
12. **Stream `seq` is not enforced.** Neither `Connection` nor any helper checks that `STREAM_TOKEN.seq` starts at 0 and increments by one, and no producer exists, so the rule of section 10 is unexercised. Right: the receiver closes with `protocol_violation` on a gap or repeat, once a producer exists.
13. **Fixed: no `CLOSE` after an interrupted write.** `native.rs` `send_bounded` and `send_command` finish the connection with `Connection::abandon` when the 30-second deadline fires and write nothing more; the caller drops the stream.
14. **No per-connection decompression byte budget.** Section 4 notes the amplification of compressed frames. No sender in the workspace compresses, so it is latent. Right: a budget of decompressed bytes per second per connection, configured by the deployment.
15. **Fixed: non-text map keys in `HELLO`, `HELLO_ACK` and `CLOSE`.** Checked against ciborium 0.2.2: an integer key was already refused (`deserialize_identifier` accepts only text or bytes), but a byte-string key was accepted as a field name (`h'636f6465'` as `code`), and an open type ignored an unknown one. `hello.rs` `decode_body` now reads the top-level map first (`require_text_keys`) and answers `protocol_violation` for any non-text key. Unit test in `hello.rs`; the vectors `invalid_body_integer_key`, `invalid_body_unknown_byte_string_key` and `invalid_body_byte_string_key_spells_a_field` cover `HELLO`.
16. **`max_frame`, `max_inflight` and the HELLO `agent` string are not configurable.** `native.rs` `local_config` fixes them, so the statement that a deployment can lower `max_frame` by configuration (Appendix C, G3) is true of the protocol and not of the daemon. Right: configuration keys for them.
17. **The rate limiter's constants are hard-coded.** `main.rs` `check_rate_limit` (100 and 10 per second). Right: configuration keys. (Fixed: a rate-limited first command no longer creates a session, because the bucket is the connection's and is checked before `create_session`.)
18. **The SDK has no deadline on most calls.** Only the handshake (10 s), `capabilities` (10 s), `call_tool` (30 s) and a frame write (30 s) have one; `ping`, `navigate` and the rest wait indefinitely for a reply. Right: a per-call deadline option, with the reconnect rule of section 14.
19. **An identity is recorded and grants nothing, and there is no channel binding.** `IDENTIFY` (6.6) proves that a key signed this connection's handshake, not that the signer is the party at the other end of the stream: a relay between the client and the daemon can forward the handshake and the proof and hold a connection that the daemon attributes to the client's key. The daemon stores the key per connection and logs it at debug level; it changes no answer, limit or right. Right: a value from the security layer under the wire (a TLS exporter, or the planned session layer) in `channel`, which the frame already has room for, and a rule that a server over that layer refuses an empty one.
20. **Authority is still not enforced.** `HELLO_ACK.authority` reports the configured mode and the daemon starts only in `off`; `AuthorityGate` (`spine-core` `authority.rs`) is not called by any request path, a request cannot carry a chain or an invocation, and `binding` is not checked against anything. Right: the carrier of `00-wire-revision.md` (C3), which is a draft.
21. **`time` and `authority` are unauthenticated.** An active attacker on a plaintext connection can change either in the acknowledgement. Neither is used by any peer in this repository. Right: do not act on them without TLS, or a layer that authenticates the acknowledgement.

## Appendix A. History: the pre-v1 wire

The text below condenses the description that this document originally contained, written against `156f7e6` (before the migration of Appendix B). Everything in it has been removed. Its line citations refer to that commit and do not resolve now.

### A.1 Layers

One logical message passed through seven layers, in the default build with no cipher and no morph:

```
L1  cbor      ciborium::into_writer(Message)
L2  sp_body   'S' 'P' 0x01 0x02 || u32_be(len(L1)) || L1
L3  json      serde_json::to_vec(SpeculativeFrame{ payload: Full(L2 as Vec<u8>), ... })
              (a Vec<u8> serialises as a JSON array of decimal integers)
L4  padded    apply_padding(L3) per morphology.padding_mode
L5  flagged   0x01 || zstd_level3(L4) if len(L4) >= 64, else 0x00 || L4
L6  sealed    L5, or nonce12 || AES-256-GCM(key, nonce, L5), after enable_encryption() (no production caller)
L7  frame     length:u32 (BE if morphology.big_endian, else LE) || version || filler || L6
```

The default header was `u32_be length || 0x00` plus the flag byte. The `Ping { timestamp: 42 }` message was 19 bytes of CBOR, 27 with the `SP` header, 177 as the JSON wrapper, and then compressed: the wrapper alone was 87 bytes, so the "compress at 64 bytes" threshold was always met on that path and the raw branch was reachable only through `send_message_raw`. The receiver accepted headerless JSON, `SP` plus JSON, CBOR or CBOR plus zstd, each optionally wrapped in one of four speculative shapes, each padded and flagged in several ways: maximally permissive, and the sender was not canonical. There was no handshake and no version exchange.

### A.2 Morphology, speculation, limits

`BrowserCommand::Morph` and a 30-second daemon task changed the frame header of every later frame (5 to 16 bytes, byte order, filler, padding) under an HMAC with an all-zero key unless a research chameleon was enabled, so the evolution was not secret. The speculative layer (`SpeculativeFrame`, predictors, caches, `PreComputed`) was dead in the default build (`predict_next` returned `(0, 0.0)`) and its `enable_speculation` set two booleans nothing read. Limits: a 16 MiB frame (zero-filled before the body arrived), 64 MiB decompressed, a 16 MiB inner message, a version-1 `SP` header, idle 300 s, 256 connections and 50 per IP, 1,000 sessions, a 100-token bucket refilling 10 per second, 30-second writes. Messages the daemon did not handle were dropped silently. A malformed message ended the connection with no code. The two recorded findings of that review were an `ExecuteBinary` that aborted the daemon on a failed program (fixed in `e3e5a2d`) and the unused JSON `negotiation.rs` (deleted with step 3).

### A.3 Message inventory

`Message` had 24 variants. The fate column is the disposition of the proposal as carried out.

| # | Variant | Pre-v1 role | Fate in v1 |
| --- | --- | --- | --- |
| 1 | `Request` | the command, with a string id | frame `REQUEST`; the id moved to the header |
| 2 | `Response` | `{id, result: Option<Value>, error: Option<String>}` | frame `RESPONSE`, `{ok}` or `{err}` |
| 3 | `Event` | only the decoy path sent it | deleted |
| 4 | `BinaryProgram` | nothing sent it | deleted |
| 5 | `LatentMessage` | daemon after `ExecuteBinary` with `StreamLatent` | moved to the research crate; the reply carries `latent_streams_count` |
| 6 | `Sync` | tests only | deleted |
| 7 | `Speculative` | nothing | deleted |
| 8 | `PreComputed` | built and never transmitted | deleted |
| 9 | `MorphRequest` | daemon, every 30 s | deleted |
| 10 | `Ping` | either | frame `PING` |
| 11 | `Pong` | either | frame `PONG` |
| 12 | `ToolCall` | client | command `tool_call` |
| 13 | `ToolResult` | server | the `RESPONSE` |
| 14 | `StreamStart` | nothing sent it | frame `STREAM_START` |
| 15 | `StreamToken` | nothing | frame `STREAM_TOKEN` |
| 16 | `StreamEnd` | nothing | frame `STREAM_END` |
| 17 | `StreamCancel` | nothing | frame `STREAM_CANCEL` |
| 18 | `CapabilityQuery` | client | command `capability_query` |
| 19 | `CapabilityAd` | server | the `ok` of its `RESPONSE` |
| 20 to 24 | `Encoded`, `CodecAd`, `CodecNegotiation`, `EmbeddingRequest`, `EmbeddingResponse` | nothing outside tests | moved to the research crate |

Count: 12 kept, 6 moved, 6 deleted. `Message` in `lib.rs` is now a record type for traces (`replay.rs`) and not a wire body.

### A.4 Command inventory

`BrowserCommand` had 35 variants. The 17 kept commands plus `tool_call` and `capability_query` were the 19 commands of section 12 until the delete commands of 1.5 made them 21.

| # | Variant | Pre-v1 behaviour in the research-free daemon | Fate in v1 |
| --- | --- | --- | --- |
| 1 to 3 | `Navigate`, `GetUR`, `GetRawHTML` | worked | `navigate`, `get_ur`, `get_raw_html` |
| 4, 5 | `Click`, `Type` | stubs returning success | deleted |
| 6 | `ExecuteBinary` | worked (aborted the daemon on failure) | `execute_binary` |
| 7 | `HandleEvent` | implemented, no sender | deleted |
| 8 | `GetLatentUR` | `unsupported_without_research` | moved to the research crate |
| 9 | `Morph` | worked | deleted |
| 10 | `Search` | a hard-coded result | deleted |
| 11 | `TransferSession` | `unsupported_without_research` | moved |
| 12, 13 | `StoreKnowledge`, `QueryKnowledge` | worked | kept |
| 14, 15 | `DeleteKnowledge`, `GetSessionHistory` | worked, no sender | deleted |
| 16 | `GetCapabilities` | listed the loaded program's capabilities | `get_binary_capabilities` |
| 17 | `SetAutonomousMode` | set a flag its reader (research) used | moved |
| 18 to 22 | `SwarmSearch`, `DelegateTask`, `ProposeKnowledge`, `CreateSwarmPlan`, `ExecutePlanTask` | refused in every build | deleted |
| 23, 24 | `NeuralTransmit`, `GetAgenticState` | `unsupported_without_research` | moved |
| 25 | `SendSpeechAct` | refused in every build | deleted |
| 26 to 35 | `ResolveName`, `ResolveNames`, `FindProviders`, `PublishName`, `FetchName`, `CrawlNames`, `PublishResource`, `ReadResource`, `NamespaceStatus`, `MeshCall` | worked | kept, with snake_case names |

Count: 17 kept, 5 moved, 13 deleted.

## Appendix B. Migration record

The 14-step plan, all executed on branch `completion/agent-web` (pull request 49) and CI-verified as of `c4439cb`. Hashes are from `git log` of that branch and `COMPLETION_PLAN.md`.

| # | Step | What changed | Commits |
| --- | --- | --- | --- |
| 1 | `cbor-core` | `cbor.rs`: scanner, encoder, decoder | `68109a0` |
| 2 | `frame-codec` | `frame.rs`: the 15-byte frame, limits, bounded decompression | `f556e26` |
| 3 | `hello-close` | `hello.rs`; the unused `negotiation.rs` deleted | `ce795e6`, clippy fix `9b37a10` |
| 4 | `drop-speculation` | the speculative layer and three `Message` variants removed | `eb46c69` |
| 5 | `drop-morphology` | morphology, padding, `MorphRequest`, `Morph`, the 30 s monitor removed | `838d363` |
| 6 | `relocate-research` | `research.rs`, chameleon and the PSK AES layer moved to `research/spine-protocol-research` or deleted | `254e642` |
| 7 | `trim-commands` | 13 commands deleted, 5 moved: 17 remain | `930fed7` |
| 8 | `trim-messages` | 6 `Message` variants deleted, 6 moved: 12 remain | `8bf13c8`, manifest fix `b841fae` |
| 9 | `connection` | `connection.rs`: handshake, ids, in-flight cap, `PING`, close codes | `53a982c` |
| 10 | `cutover` | the daemon and every client switched; `ProtocolHandler` and the `SP` header deleted | `96ae4b5`, fixes `6fc82b6`, `fbf1b64`, `8ee8a67` |
| 11 | `bodies-v1` | header ids replace body ids; `{ok}` / `{err}` with codes; closed structs; byte strings; `native.rs`, `body.rs`, `wirefmt.rs` | `f9ce5c3`, `b3b1ec3` |
| 12 | `signed-frame` | `spine-mesh/src/signed_frame.rs` deleted (no callers) | `96ae4b5` |
| 13 | `bindings-scripts-docs` | Go, FFI, Python, extension notes, scripts, documents | `2774c31`, `6d197b5`, `fa5b63c`, `b029b52`, `c4439cb` |
| 14 | `conformance-vectors` | `docs/spec/00-wire.vectors.json`, `scripts/wire_v1.py`, `tests/conformance_vectors.rs` | `68810d6`, clippy fix `b9088df` |

The plan itself (the document that held this table) was committed in `f4f0a67`. Documentation commits that recorded the steps: `7d82c0a`, `4959358`, `9a5706a`, `ebcc059`, `3c41221`, `1734ce8`. This reconciliation (the document you are reading) closes the item that `c33e52b` added to `COMPLETION_PLAN.md`.

## Appendix C. Open questions

Questions the earlier document put to the owner, with what became of each. The labels G1 to G12 are kept because other documents cite them (for example `extension/README.md` cites G11).

| # | Question | Status |
| --- | --- | --- |
| G1 | Run the native path over the `spine-session` handshake instead of TLS? | Still open. TLS 1.3 stays the only protection; the session handshake needs independent review first; not both at once (1.4). |
| G2 | Keep the four streaming frame types though nothing implements them? | Resolved: kept. They are specified (section 10) and enforced by `Connection`; the gateway maps the structs to SSE. A producer is still to be written. |
| G3 | The 16 MiB ceiling is set by the 8 MiB single-frame upload; lower it and chunk uploads? | Resolved: 16 MiB kept for v1 (`MAX_FRAME`); uploads stay one frame, downloads are chunked at 64 KiB. Lowering `max_frame` is possible in the protocol, not by daemon configuration (section 16, item 16). |
| G4 | Closed structs or open? | Resolved: closed everywhere except `HELLO` and `HELLO_ACK` (5.2). |
| G5 | Deterministic CBOR: a validator and a sorting pass over ciborium, or another crate? | Resolved: `cbor.rs` hand-written scanner and a sorting encoder through `ciborium::Value`; no new dependency. |
| G6 | Move chameleon and the neural codec to a research crate, or delete? | Resolved: morphology, padding and speculation deleted; chameleon and the neural-codec types moved to `research/spine-protocol-research`. |
| G7 | Compression: keep the flag? | Resolved: kept, off by default, sender threshold 1 KiB, receiver always accepts (section 4). No sender in the workspace enables it. |
| G8 | Capability chains in the frame header? | Resolved for v1: no header field. Authority (layer 3) is a draft and nothing enforces it. A client key (`IDENTIFY`) and a per-connection value (`binding`) were added to the handshake on 2026-11-24 (1.5, 6.6); they grant nothing, and a carrier for a capability is not specified (`00-wire-revision.md`, a draft). |
| G9 | `LatentMessage`: an extension or inside the `ExecuteBinary` result? | Resolved: removed from the wire; the result carries only `latent_streams_count`. |
| G10 | Allow `REQUEST` before `HELLO_ACK`? | Resolved: no (6.1). Revisit with measurements. |
| G11 | The browser extension speaks JSON to `ws://localhost:9800/ws`, where nothing listens. | Still open. `extension/README.md` marks it as not speaking the native wire; it is neither removed nor given a v1 client. |
| G12 | The `ExecuteBinary` panic path. | Resolved: fixed in `e3e5a2d`; a failed program is `execution_failed` (section 13). |

## Appendix D. Concordance with the earlier section labels

Source comments, other documents and the generator cite labels of the document this one replaced. This table resolves them.

| Earlier label | Now |
| --- | --- |
| A (as built), A.1 to A.10 | Appendix A |
| B, requirements R1 to R15 | D.1 below |
| C.1 | 2.1 to 2.3 |
| C.2 | 2.4 |
| C.3 | 3 and 8.4 |
| C.4 | 4 |
| C.5 (rules 1 to 9) | 5.1 (rules 1 to 6), 5.2 (rules 7 to 9), 5.3 |
| C.6 | 6.1 to 6.4 |
| C.7 | 7 and 6.5 |
| C.8 | 8 |
| C.9 | 10 |
| C.10 | 11 and 6.4 |
| C.11 | 1.4 |
| C.12 | 15.3 |
| C.13 | folded into the sections it concerned |
| D.1, D.2 | 12, and the fate columns of A.3 and A.4 |
| E | A.2 |
| F | Appendix B |
| G1 to G12 | Appendix C |

### D.1 The requirements of the earlier document

| Req | Statement | Status at `c33e52b` |
| --- | --- | --- |
| R1 | Framing is fixed for the life of a connection | Met. |
| R2 | Every length is bounded before an allocation or read depends on it | Met (2.3). |
| R3 | Decompressed size is declared and checked before and after inflation | Met (4). |
| R4 | One encoding per value; non-canonical input is rejected | Met for commands, replies and stream bodies (`decode_strict`); for `HELLO`, `HELLO_ACK` and `CLOSE` the profile is enforced and a non-text key is refused, but typed re-encoding is not checked. |
| R5 | A version exchange precedes any message | Met (6). |
| R6 | A header correlation id; several requests in flight | Met by `Connection`; the daemon accepts up to 8 per connection and runs them one at a time (8.3). |
| R7 | Streams and cancel are frame types; cancel is processed during production | Frame types and state rules met; no producer; the daemon reads during a command but has nothing to cancel (section 16, items 9 and 12). |
| R8 | A fixed 15-byte overhead; no per-connection predictor | Met. |
| R9 | The core carries no research variants; an unnegotiated extension closes | Met (11). |
| R10 | Every violation class has one defined outcome | Met except the `RESPONSE` and `STREAM_*` schema case (section 16, item 8). |
| R11 | The wire does not rely on the frame for confidentiality; no clear off-loopback by default | Met (1.4). |
| R12 | Error responses carry a stable code | Met; two codes are derived from text (section 16, items 1 and 2). |
| R13 | A per-connection in-flight cap answered with `overloaded` | Met in `Connection`; the daemon's cap of 1 never trips it. |
| R14 | A second implementer can build a conformant peer from the document and vectors | Partly: the vectors do not cover connection state (except the window of `IDENTIFY`), commands or real zstd (15.4). |
| R15 | A deadline for connection setup | Met (6.1, `server.hello_timeout_secs`). |
22. **A rotated record without its log is `unavailable`, and a name compromised only on the mesh is too.** A node that holds a record whose head claim reaches past its log answers `unavailable` (12.5), which a client cannot tell from a disconnected namespace except by the reason text. Right: a reason code, or a fetch of the log from a peer before answering. The mesh service holds equivocation proofs that `spine-mesh` does not expose, so a name compromised there is answered by the daemon as `unavailable` until the proof reaches the local resolver's book (when a log of the name is offered to it, as a lookup that carries one does).
23. **`find_providers` and `crawl_names` carry no logs.** A client that gets a provider record from them cannot judge a rotated name's key history from that answer; it resolves the name (12.5).
24. **Nothing carries a log to a client that is behind.** The log in an answer is what the node holds (`02-names.md` 10.8); a client has no way to ask for a longer one or to learn that one exists.
25. **A rotated name's current key cannot delete its blob.** `delete_resource` is by the recorded publisher, the inception key of the name, as is storing and the per-publisher quota (12.2). After a rotation the holder of the current key has no wire command that deletes the bytes, and the holder of the superseded key can still delete them. Right: record the publisher as the name, or let a delete be authorised by any key of the name's succession log, which needs the log at the store. Not done: it changes what the store records for every blob.
