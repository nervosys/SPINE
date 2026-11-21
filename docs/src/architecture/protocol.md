# Protocol

## Message Format

All communication uses the `Message` enum, serialized with serde:

```rust
pub enum Message {
    Request(Request),
    Response(Response),
    Ping { timestamp: u64 },
    Pong { timestamp: u64 },
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    StreamStart(StreamStart),
    StreamToken(StreamToken),
    StreamEnd(StreamEnd),
    StreamCancel(StreamCancel),
    CapabilityQuery(CapabilityQuery),
    CapabilityAd(CapabilityAdvertisement),
}
```

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
(`max_inflight = 1`). A `Request`, `ToolCall` or `CapabilityQuery` travels as a
`REQUEST`; the daemon's `Response`, `ToolResult` or `CapabilityAd` travels as
the `RESPONSE` with the same request id. `Ping` and `Pong` are the `PING` and
`PONG` frames, whose body is an unsigned integer token.

## Compression

Compression is off unless the sender turns it on. A body of at least 1 KiB is
then compressed with zstd when that makes it smaller, and the compressed flag
is set in the header. Smaller bodies are never compressed. The receiver needs
no setting.

## Transport Options

| Transport | Port | Use Case                 |
| --------- | ---- | ------------------------ |
| TCP       | 8080 | Default, lowest latency  |
| TLS       | 8080 | Encrypted, production    |
| WebSocket | 8081 | Browser/polyglot clients |
| QUIC      | 8082 | Feature-gated, UDP-based |
