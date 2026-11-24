# Configuration

SPINE uses layered configuration. From highest priority to lowest: environment variables, `spine.toml`, built-in defaults.

## Configuration File

Create `spine.toml` in your project root:

```toml
[server]
host = "127.0.0.1"
port = 8080
ws_port_offset = 1          # WebSocket on port+1
quic_port_offset = 2        # QUIC on port+2
metrics_port = 9090
max_sessions = 1000
max_connections_per_ip = 50
idle_timeout_secs = 300
hello_timeout_secs = 10
session_watchdog_secs = 600
persistence_interval_secs = 60
shutdown_timeout_secs = 30
allow_unauthenticated_remote = false

[tls]
enabled = false
cert_path = "certs/cert.pem"
key_path = "certs/key.pem"
ca_path = "certs/ca.pem"
mutual_tls = false

[cluster]            # used only when built with the `research` feature
port_offset = 100
region = "us-east"
skills = ["search", "analysis"]

[logging]
format = "pretty"   # "json" or "pretty"
level = "info"      # trace, debug, info, warn, error
```

## Environment Variables

Common settings can be overridden with environment variables:

```bash
# 0.0.0.0 is not loopback: the daemon refuses it unless mutual TLS is on
# (SPINE_TLS_MTLS=true with TLS and a client CA) or
# server.allow_unauthenticated_remote is set. Use 127.0.0.1 for a local node.
export SPINE_HOST=0.0.0.0
export SPINE_PORT=9000
export SPINE_TLS=true
export SPINE_TLS_MTLS=true
export SPINE_LOG_LEVEL=debug
```

Others include `SPINE_METRICS_PORT`, `SPINE_MAX_SESSIONS`, `SPINE_TLS_CERT`, `SPINE_TLS_KEY`, `SPINE_TLS_CA`, `SPINE_HELLO_TIMEOUT`, `SPINE_IDLE_TIMEOUT`, `SPINE_SHUTDOWN_TIMEOUT` and `SPINE_CONFIG_PATH` (an alternative config file path). See `src/spine-core/src/config.rs` for the full list.

## Exposing the Daemon

The native server has no per-client authorization: any connected client can
publish names and resources and call tools. A non-loopback `server.host` is
refused at startup unless `tls.mutual_tls` is on or
`server.allow_unauthenticated_remote` is set, which states that the network
itself is trusted. TLS is the only source of confidentiality and
authentication on the native wire.

## Loading Config in Code

```rust
use spine_core::SpineConfig;

let config = SpineConfig::load();
println!("Listening on {}:{}", config.server.host, config.server.port);
```

## Default Values

| Setting                    | Default     |
| -------------------------- | ----------- |
| `server.host`              | `127.0.0.1` |
| `server.port`              | `8080`      |
| `server.max_sessions`      | `1000`      |
| `server.idle_timeout_secs` | `300`       |
| `server.hello_timeout_secs` | `10`      |
| `server.allow_unauthenticated_remote` | `false` |
| `tls.enabled`              | `false`     |
| `logging.format`           | `pretty`    |
| `logging.level`            | `info`      |
