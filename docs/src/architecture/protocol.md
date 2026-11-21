# Protocol

## Bodies

The body of a frame is one deterministic CBOR item (`docs/spec/00-wire.md` section 5).
The frame header's request id is the only correlation id; no body carries one.

A `REQUEST` body is a command from a closed set of 19. A command without
fields is the text of its name (`"get_ur"`); a command with fields is a
single-entry map `{name: fields}`. A name outside the set is `unknown_command`;
a known name with a defect (an unknown field, a wrong type, a present `null`
for an optional field, a non-deterministic encoding) is `invalid_request`.

| Name | Fields |
| --- | --- |
| `navigate` | `url` |
| `get_ur` | none |
| `get_raw_html` | none |
| `execute_binary` | a `SpineBinary` |
| `store_knowledge` | `key`, `value`, `tags` |
| `query_knowledge` | `query`, `tags`, `limit` |
| `get_binary_capabilities` | none |
| `resolve_name` | `name`, `if_none_match` (optional byte string, 32 bytes) |
| `resolve_names` | `names` |
| `find_providers` | `capability`, `limit` (optional) |
| `publish_name` | `record` |
| `fetch_name` | `name`, `if_none_match` (optional byte string, 32 bytes) |
| `crawl_names` | `seed`, `max_depth` (optional), `max_visits` (optional) |
| `publish_resource` | `record`, `content_type`, `data` (byte string) |
| `read_resource` | `hash` (byte string, 32 bytes), `offset`, `max_bytes` |
| `namespace_status` | none |
| `mesh_call` | `peer_id`, `peer_key` (byte string, 32 bytes), `payload` |
| `tool_call` | `name`, `args`, `trace` (optional) |
| `capability_query` | `selector` |

A `RESPONSE` body is a single-entry map: `{ok: result}` on success or
`{err: {code, message[, details]}}` on failure. The result's shape depends on
the command; `read_resource` answers a `ResourceChunk` whose `hash` and `data`
are byte strings, `capability_query` answers a capability advertisement
(`agent_id`, `capabilities`) and `tool_call` answers the tool's content.

Every body struct rejects unknown fields. Optional fields are omitted when
absent. Byte fields are CBOR byte strings, never arrays of integers. A
`serde_json::Value` field stays a value and is encoded as CBOR, with integer and
float widths as in section 5 of `docs/spec/00-wire.md`.

### Error codes

`spine_protocol::body` holds the constants (`ERR_*`) and `ALL_ERROR_CODES`.
Clients branch on `code`; `message` is for people. The complete table with the raising function for each code is section 13 of `docs/spec/00-wire.md`.

| Code | Raised when |
| --- | --- |
| `overloaded` | the connection already has `max_inflight` requests in flight (the daemon processes one request at a time and sends `max_inflight` 1, so it never produces this) |
| `unknown_command` | the body names a command outside the set |
| `invalid_request` | the body is a known command with a defect; also `read_resource` with `max_bytes` outside 1 to 65,536 or an `offset` past the end |
| `internal` | the daemon failed in a way the client cannot fix |
| `rate_limit` | the connection's request rate for session commands, or its separate `tool_call` rate, was exceeded |
| `session_capacity` | the daemon holds its maximum number of sessions; the connection stays open |
| `session_not_found` | `get_ur` or `get_raw_html` and the session record is gone |
| `session_expired` | `navigate` fetched a page and the session record was gone |
| `no_page_loaded` | `get_ur` or `get_raw_html` arrived before a successful `navigate`; `get_ur` reports a page that fails to parse as `parse_error` |
| `capability_denied` | an `execute_binary` program declares a capability other than `network`, `storage`, `memory` or `search` |
| `execution_failed` | a binary failed to compile, instantiate or run |
| `namespace_unavailable` | no namespace or mesh is configured |
| `invalid_peer_id`, `mesh_call_failed` | a `mesh_call` target is malformed or the call failed |
| `batch_too_large` | `resolve_names` exceeded its limit |
| `lookup_failed`, `lookup_unavailable` | provider lookup failed or is not configured |
| `record_expired`, `malformed_record`, `record_rejected`, `publish_failed` | a published record or resource was refused |
| `resource_not_found` | `read_resource`: this origin holds no blob for the hash |
| `invalid_seed` | a crawl seed is not a valid name |
| `invalid_args`, `unsupported_tool`, `parse_error` | a `tool_call` failed (the tool name is checked first, so an unknown tool reports `unsupported_tool` whatever its args); `parse_error` is also what `get_ur` reports for a loaded page that does not parse |
| `outbound_*` | the daemon's outbound HTTP policy refused or failed a fetch: `disabled`, `address_denied`, `scheme_denied`, `credentials_denied`, `fragment_denied`, `redirect_denied`, `encoding_denied`, `response_limit`, `busy`, `timeout`, `invalid_url`, `invalid_host`, `invalid_port`, `dns_failed`, `client_failed`, `request_failed`, `body_failed`, `invalid_utf8`, `url_limit`, `dns_limit`, `http_status` (the status is in `details.status`) |

The SDK returns an error as `NativeError::Remote { code, message, details }`
inside `anyhow::Error`; use `spine_protocol::native::remote_code`.

## Frame Format

The native wire is spine-wire v1 (specified in `docs/spec/00-wire.md`). Every
frame has a fixed 15-byte header, all integers big-endian:

```
┌────────────┬─────────┬──────────┬──────────┬────────────────┬──────┐
│ Length (4) │ Ver (1) │ Type (1) │ Flags(1) │ Request id (8) │ Body │
└────────────┴─────────┴──────────┴──────────┴────────────────┴──────┘
```

- **Length**: bytes after this field, `11 + len(body)`, at most the negotiated
  maximum (16 MiB; 64 KiB before the handshake completes)
- **Ver**: `1`
- **Type**: `HELLO`, `HELLO_ACK`, `CLOSE`, `PING`, `PONG`, `REQUEST`,
  `RESPONSE`, `STREAM_*` and `EXT`
- **Flags**: bit 0 means the body is zstd compressed; other bits must be zero
- **Request id**: correlates a `RESPONSE` with its `REQUEST`; a `PING` and its
  `PONG` share one
- **Body**: exactly one deterministic CBOR item

A connection starts with `HELLO` from the client and `HELLO_ACK` from the
server; nothing else may be sent first. The daemon closes a connection that has
not sent `HELLO` within `server.hello_timeout_secs` (default 10 seconds) with
`CLOSE(timeout)`. A violation ends the connection with a `CLOSE` code
(`malformed_frame`, `frame_too_large`, `protocol_violation`, ...).

The current daemon runs one request per connection at a time
(`max_inflight = 1`). A command travels as a `REQUEST`; the daemon's reply
travels as the `RESPONSE` with the same request id. `Ping` and `Pong` are the
`PING` and `PONG` frames, whose body is an unsigned integer token.

## Compression

Compression is off unless the sender turns it on. A body of at least 1 KiB is
then compressed with zstd when that makes it smaller, and the compressed flag
is set in the header. Smaller bodies are never compressed. The receiver needs
no setting.

## Transport Options

| Transport | Port | Use Case                 |
| --------- | ---- | ------------------------ |
| TCP       | 8080 | Default, lowest latency  |
| TLS       | 8080 | Encrypted, production (optional mutual TLS) |
| WebSocket | 8081 | Browser/polyglot clients (ws and wss) |
| QUIC      | 8082 | Only when `spine-core` is built with `--features quic` (off by default) |

The wire itself is not encrypted. Confidentiality and authentication on the
native path come from TLS only; see [Security](./security.md).
