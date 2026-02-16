# CLI Commands

The `spine` CLI provides project management, interactive connections, and operations.

## `spine init <name>`

Scaffold a new SPINE agent project:

```bash
spine init my-agent
spine init my-agent --tls    # With TLS configuration
```

Creates: `Cargo.toml`, `spine.toml`, `src/main.rs`, `examples/`, `.gitignore`

## `spine connect <addr>`

Interactive REPL connected to a SPINE server:

```bash
spine connect 127.0.0.1:8080
spine connect 127.0.0.1:8080 --tls --domain spine.example.com
spine connect ws://127.0.0.1:8081 --ws
```

REPL commands:

| Command          | Description                |
| ---------------- | -------------------------- |
| `nav <url>`      | Navigate to URL            |
| `ur`             | Get Unified Representation |
| `html`           | Get raw HTML               |
| `search <query>` | Search                     |
| `ping`           | Measure latency            |
| `exec <script>`  | Execute HLS script         |
| `stats`          | Speculation stats          |
| `morph`          | Trigger protocol morphing  |
| `caps`           | List server capabilities   |
| `help`           | Show commands              |
| `quit`           | Disconnect                 |

## `spine query <addr> <subcommand>`

One-shot queries:

```bash
spine query 127.0.0.1:8080 navigate --url https://example.com
spine query 127.0.0.1:8080 search --query "rust async"
spine query 127.0.0.1:8080 ping --count 5
spine query 127.0.0.1:8080 html --url https://example.com --format text
spine query 127.0.0.1:8080 exec --script "let x = 42; x"
```

## `spine deploy`

Start a SPINE server using configuration:

```bash
spine deploy
spine deploy --config custom.toml
```

## `spine benchmark <addr>`

Performance benchmarks:

```bash
spine benchmark 127.0.0.1:8080 --iterations 100
spine benchmark 127.0.0.1:8080 --concurrent 10 --iterations 50
```

## `spine status <addr>`

Check server health:

```bash
spine status 127.0.0.1:8080
spine status 127.0.0.1:8080 --metrics-port 9090
```
