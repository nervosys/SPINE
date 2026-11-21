# Transport Layer

## Architecture

`spine-transport` provides high-performance I/O with composable plugins. It
supplies the socket pieces the native wire runs over (WebSocket bridge, QUIC
stream) and its own `FrameCodec` (12-byte header, a different format). It is not the
spine-wire v1 codec: that is `spine_protocol::frame` (15-byte header). The transport
crate adds no encryption to the native path; TLS does that.

Figures below were measured on earlier builds, before the native wire became
spine-wire v1 (2026-11-21), and have not been re-measured.

```rust
use spine_transport::{TransportPlugin, MetricsPlugin, RateLimiterPlugin};
```

## Plugin System

Plugins form an ordered pipeline (forward on send, reverse on receive):

| Plugin              | Purpose                       |
| ------------------- | ----------------------------- |
| `MetricsPlugin`     | Byte counters, message counts |
| `RateLimiterPlugin` | Token-bucket rate limiting    |
| `TaggingPlugin`     | Add metadata headers          |
| `LoggingPlugin`     | Request/response logging      |
| `SizeLimiterPlugin` | Max message size enforcement  |

Custom plugins implement the `TransportPlugin` trait:

```rust
pub trait TransportPlugin: Send + Sync + fmt::Debug {
    fn name(&self) -> &str;
    fn on_send(&self, frame: &mut Frame) -> TransportResult<()> { Ok(()) }
    fn on_recv(&self, frame: &mut Frame) -> TransportResult<()> { Ok(()) }
    // plus on_connect, on_disconnect and is_enabled, all with defaults
}
```

## BBR Congestion Control

Bottleneck Bandwidth and RTT-based congestion control:
- Pacing decision: 335 ps
- Bandwidth estimation without packet loss
- RTT-based delivery rate measurement

## Zero-Copy I/O

- Frame decode via `bytes::Bytes` slicing
- Buffer reuse pool for send/read/latent buffers
- Stack-allocated frame headers

## WebSocket Bridge

Client and server WebSocket adapters:

```rust
use spine_transport::{WebSocketBridge, WebSocketClientStream};

// Used internally by AgentClient::connect_ws()
let bridge = WebSocketBridge::connect("ws://localhost:8081").await?;
let stream = WebSocketClientStream::new(bridge);
```
