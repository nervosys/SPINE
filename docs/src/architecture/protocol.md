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
    Sync(SyncPayload),
    Speculative(SpeculativeFrame),
    PreComputed(PreComputedResponse),
    MorphRequest { seed: u64 },
    Ping { timestamp: u64 },
    Pong { timestamp: u64 },
}
```

## Frame Format

Messages are framed with a 16-byte header:

```
┌──────────┬──────────┬───────────┬───────────┐
│ Length(4) │ Type(1)  │ Flags(1)  │ Padding(10)│
└──────────┴──────────┴───────────┴───────────┘
│                 Payload                       │
└───────────────────────────────────────────────┘
```

- **Length**: 4 bytes, big-endian, payload size
- **Type**: Message discriminant
- **Flags**: `0x01` = zstd compressed, `0x00` = raw

## Compression

Adaptive compression using a 1-byte flag protocol:
- Payloads < 64 bytes are sent raw (compression overhead exceeds savings)
- Larger payloads use zstd compression
- Stack-allocated `[u8; 16]` headers avoid heap allocation

## Speculation

The protocol supports speculative message prediction:
- Bidirectional prediction with Titans architecture
- Pre-computed responses sent before request arrives
- Surprise-gated learning adjusts predictions over time
- Speculation hits eliminate round-trip latency entirely

## Transport Options

| Transport | Port | Use Case                 |
| --------- | ---- | ------------------------ |
| TCP       | 8080 | Default, lowest latency  |
| TLS       | 8080 | Encrypted, production    |
| WebSocket | 8081 | Browser/polyglot clients |
| QUIC      | 8082 | Feature-gated, UDP-based |
