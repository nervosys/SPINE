# Connecting

## Transport Options

Every constructor runs the spine-wire v1 handshake (`HELLO` / `HELLO_ACK`)
before it returns, with a ten-second deadline. TLS is the only thing that
protects the connection; plain TCP and `ws://` are plaintext.

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

Use a `wss://` URL for a TLS WebSocket.

### Auto-Reconnect

```rust
use std::time::Duration;

let mut client = AgentClient::connect_with_retry(
    "127.0.0.1:8080",
    5,                          // max retries
    Duration::from_secs(1),     // base delay (exponential backoff)
).await?;
```

`connect_tls_with_retry(addr, domain, ca_path, client_cert, max_retries)` does
the same over TLS. `AgentClient::over(stream)` runs the handshake on a stream you
already hold.

## Connection Lifecycle

1. **Connect** — Establish the TCP/TLS/WS connection and complete `HELLO` / `HELLO_ACK`
2. **Use** — Send commands, receive responses; the daemon answers one request at a time (`max_inflight = 1`)
3. **Drop** — Connection closes when `AgentClient` is dropped

The connection is stateful — the server maintains session state (the loaded page, cached UR, knowledge) per connection.

## Methods

All methods are async and return `anyhow::Result`.

| Method | Returns | Wire command |
| --- | --- | --- |
| `navigate(url)` | `()` | `navigate` |
| `get_ur()` | `UnifiedRepresentation` | `get_ur` |
| `get_raw_html()` | `String` | `get_raw_html` |
| `execute_binary(SpineBinary)` | `Value` | `execute_binary` |
| `execute_hls(script)` | `WasmExecutionResult` | `execute_binary` (compiles locally) |
| `store_knowledge(key, value, tags)` | `()` | `store_knowledge` |
| `query_knowledge(query, tags, limit)` | `Vec<Value>` | `query_knowledge` |
| `get_binary_capabilities()` | `Vec<String>` | `get_binary_capabilities` |
| `capabilities()` | `CapabilityAdvertisement` | `capability_query` |
| `call_tool(name, args)` | `ToolOutcome` | `tool_call` |
| `namespace_status()` | `Value` | `namespace_status` |
| `mesh_call(peer_id, peer_key, payload)` | `Value` | `mesh_call` |
| `resolve_name(name, if_none_match)` | `NameResolution` | `resolve_name` |
| `resolve_names(&[String])` | `Vec<NameResolution>` | `resolve_names` |
| `find_providers(capability, limit)` | `Value` | `find_providers` |
| `publish_name(record)` | `Value` | `publish_name` |
| `fetch_name(name, if_none_match)` | `Value` | `fetch_name` |
| `crawl_names(seed, max_depth, max_visits)` | `Value` | `crawl_names` |
| `ping()` | `u64` (round trip, ms) | `PING` frame |

`spine_agent::resources` adds `publish_resource(record, content_type, data)`,
`read_resource(record)` and `fetch_resource(name)`, which verify signed records and
content hashes end to end (`publish_resource` and `read_resource` are the
`publish_resource` and `read_resource` wire commands). The public `conn` field is
the underlying `Connection<S>` for direct frame access.

## Errors

When the daemon answers `{err: {code, message}}` the returned `anyhow::Error`
holds `spine_protocol::native::NativeError::Remote { code, message, details }`.
Branch on the code, never on the message:

```rust
use spine_protocol::native::remote_code;

if let Err(err) = client.get_ur().await {
    match remote_code(&err) {
        Some("no_page_loaded") => { /* call navigate first */ }
        Some(code) => eprintln!("daemon refused: {code}"),
        None => eprintln!("connection problem: {err}"),
    }
}
```

`call_tool` is different: a tool failure comes back as
`Ok(ToolOutcome::Err { code, message, details })`; only protocol-level failures
are `Err(..)`. The error codes are listed in [Protocol](../architecture/protocol.md).
