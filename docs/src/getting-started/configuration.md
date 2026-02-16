# Configuration

SPINE uses layered configuration: `spine.toml` → environment variables → defaults.

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
session_watchdog_secs = 600
persistence_interval_secs = 60
shutdown_timeout_secs = 30

[tls]
enabled = false
cert_path = "certs/server.crt"
key_path = "certs/server.key"
ca_path = "certs/ca.crt"

[cluster]
port_offset = 100
region = "us-east"
skills = ["search", "analysis"]

[logging]
format = "pretty"   # "json" or "pretty"
level = "info"      # trace, debug, info, warn, error
```

## Environment Variables

Override any setting via environment variables prefixed with `SPINE_`:

```bash
export SPINE_SERVER_HOST=0.0.0.0
export SPINE_SERVER_PORT=9000
export SPINE_TLS_ENABLED=true
export SPINE_LOGGING_LEVEL=debug
```

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
| `tls.enabled`              | `false`     |
| `logging.format`           | `pretty`    |
| `logging.level`            | `info`      |
