# Connecting

## Transport Options

### TCP (plaintext)

```rust
let mut client = AgentClient::connect("127.0.0.1:8080").await?;
```

### TLS

```rust
let mut client = AgentClient::connect_tls(
    "127.0.0.1:8080",
    "spine.example.com",
    Some(Path::new("certs/ca.crt")),
    None, // No client certificate
).await?;
```

### Mutual TLS

```rust
let mut client = AgentClient::connect_tls(
    "127.0.0.1:8080",
    "spine.example.com",
    Some(Path::new("certs/ca.crt")),
    Some((Path::new("certs/client.crt"), Path::new("certs/client.key"))),
).await?;
```

### WebSocket

```rust
let mut client = AgentClient::connect_ws("ws://127.0.0.1:8081").await?;
```

### Auto-Reconnect

```rust
use std::time::Duration;

let mut client = AgentClient::connect_with_retry(
    "127.0.0.1:8080",
    5,                          // max retries
    Duration::from_secs(1),     // base delay (exponential backoff)
).await?;
```

## Connection Lifecycle

1. **Connect** — Establish TCP/TLS/WS connection
2. **Use** — Send commands, receive responses
3. **Drop** — Connection closes when `AgentClient` is dropped

The connection is stateful — the server maintains session state (navigation history, cached URs, knowledge) per connection.
