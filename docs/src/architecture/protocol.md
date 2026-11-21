# Protocol

## Message Format

All communication uses the `Message` enum, serialized with serde:

```rust
pub enum Message {
    Request(Request),
    Response(Response),
    Event(Event),
    BinaryProgram(SpineBinary),
    LatentMessage(LatentVector),
    Ping { timestamp: u64 },
    Pong { timestamp: u64 },
}
```

## Frame Format

Messages are framed with a fixed 5-byte header. The header never changes
during a connection:

```
┌───────────┬──────────┐
│ Length(4) │ 0x00 (1) │
└───────────┴──────────┘
│       Payload        │
└──────────────────────┘
```

- **Length**: 4 bytes, big-endian, payload size (at most 16 MiB)
- **Fifth byte**: written as `0x00`, ignored on read
- **Payload**: starts with a flag byte, `0x01` = zstd compressed, `0x00` = raw

## Compression

Adaptive compression using a 1-byte flag protocol:
- Payloads < 64 bytes are sent raw (compression overhead exceeds savings)
- Larger payloads use zstd compression
- Stack-allocated `[u8; 5]` headers avoid heap allocation

## Transport Options

| Transport | Port | Use Case                 |
| --------- | ---- | ------------------------ |
| TCP       | 8080 | Default, lowest latency  |
| TLS       | 8080 | Encrypted, production    |
| WebSocket | 8081 | Browser/polyglot clients |
| QUIC      | 8082 | Feature-gated, UDP-based |
