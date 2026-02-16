# Transport Layer

## Architecture

`spine-transport` provides high-performance I/O with composable plugins:

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
pub trait TransportPlugin: Send + Sync {
    fn on_send(&self, data: &mut Vec<u8>) -> Result<()>;
    fn on_recv(&self, data: &mut Vec<u8>) -> Result<()>;
}
```

## BBR Congestion Control

Bottleneck Bandwidth and RTT-based congestion control:
- Pacing decision: 335 ps
- Bandwidth estimation without packet loss
- RTT-based delivery rate measurement

## Zero-Copy I/O

- Frame decode via `bytes::Bytes` slicing (30% faster)
- Buffer reuse pool for send/read/latent buffers
- Stack-allocated frame headers (`[u8; 16]`)

## WebSocket Bridge

Client and server WebSocket adapters:

```rust
use spine_transport::WebSocketClientStream;

// Used internally by AgentClient::connect_ws()
let stream = WebSocketClientStream::connect("ws://localhost:8081").await?;
```
