# CLI Commands

The `spine` CLI provides project management, interactive connections, and operations. The default server address is `127.0.0.1:8080`. `--verbose` is a global flag.

| Command | Purpose |
| --- | --- |
| `spine mcp` | Serve the core's read-only tools over MCP stdio |
| `spine task` | Submit and inspect durable work over the signed mesh |
| `spine name` | Publish, discover, renew and retrieve signed agent-web resources |
| `spine init` | Scaffold a project |
| `spine connect` | Interactive REPL |
| `spine query` | One-shot queries |
| `spine deploy` | Start a server |
| `spine benchmark` | Performance benchmarks |
| `spine status` | Check server health |
| `spine cert` | Development certificates |

There are no `morph`, `stats` or `search` commands; the protocol morphing,
statistics and search commands they used no longer exist.

## `spine init [path]`

Scaffold a new SPINE agent project:

```bash
spine init my-agent
spine init my-agent --name my-agent --tls    # With TLS configuration
```

`--examples` (on by default) includes example agent code.

## `spine connect [addr]`

Interactive REPL connected to a SPINE server:

```bash
spine connect 127.0.0.1:8080
spine connect 127.0.0.1:8080 --tls --domain spine.example.com --ca certs/ca.crt
spine connect ws://127.0.0.1:8081 --ws
```

Flags: `--ws`, `--tls`, `--ca`, `--domain`, `--client-cert`, `--client-key`.

REPL commands:

| Command          | Description                |
| ---------------- | -------------------------- |
| `nav <url>` (also `navigate`, `go`) | Navigate to URL |
| `ur` (also `get`) | Get Unified Representation |
| `html` (also `raw`) | Get raw HTML             |
| `ping`           | Measure latency            |
| `exec <script>` (also `run`) | Execute HLS script |
| `caps` (also `capabilities`) | List binary capabilities |
| `help`           | Show commands              |
| `quit`           | Disconnect                 |

## `spine query <subcommand>`

One-shot queries. The server address is the `-a`/`--addr` option of each
subcommand, not a positional argument:

```bash
spine query navigate https://example.com -a 127.0.0.1:8080 -f json
spine query store my-key '{"a": 1}' -t tag1,tag2
spine query knowledge "programming languages" -t language -l 10
spine query html https://example.com
spine query exec "let x = 42; x"
spine query ping -n 5
```

| Subcommand | Arguments and options |
| --- | --- |
| `navigate <url>` | `-a`, `-f text\|json\|compact` |
| `store <key> <json>` | `-t` tags (comma-separated), `-a` |
| `knowledge <query>` | `-t` tags, `-l` limit (default 10), `-a` |
| `html <url>` | `-a` |
| `exec <script or file>` | `-a` |
| `ping` | `-a`, `-n` count (default 5) |

## `spine deploy`

Start a SPINE server using configuration:

```bash
spine deploy
spine deploy -c custom.toml -p 9000 --host 0.0.0.0
```

## `spine benchmark [addr]`

Performance benchmarks:

```bash
spine benchmark 127.0.0.1:8080 -n 100
spine benchmark 127.0.0.1:8080 -c 10 -n 50
```

## `spine status [addr]`

Check server health:

```bash
spine status 127.0.0.1:8080
```

## `spine cert`

```bash
spine cert generate -o certs   # CA, server and client certificates for development
spine cert info certs/ca.crt   # Show a PEM certificate
```

## `spine name --addr <addr> <action>`

Actions: `status`, `create`, `revoke <record>`, `resolve <name>`,
`providers <capability>`, `publish <record>`, `renew <record>`,
`upload <record> <resource>` and `fetch <name> --output <file>`. Run
`spine name --help` for each action's options.

A record's `--ttl` is at most 604800 seconds (7 days); `create` and `renew`
refuse more, and every node refuses a record with a longer lifetime or one
dated more than five minutes ahead of its clock.

`revoke <record> --key <key> --output <file>` signs a tombstone for the name in
`<record>`: a record one sequence number higher, with no endpoints or content and
a 7-day lifetime, which supersedes every lower-sequence record for the name. Like
`create`, it only writes the file; send it with `publish <file>`. A name with a
tombstone resolves as revoked, not as missing. The tombstone lapses after at most
7 days, after which the name is unknown again unless re-published with a higher
sequence, so keep re-signing it if the name must stay withdrawn.

## `spine task --addr <addr> --peer <id> --peer-key <hex> <action>`

Sends a `mesh_call` through the daemon to a pinned peer (a 64-hex-character
Ed25519 key). Actions: `capabilities`, `submit <spec file>`, `status <id>`,
`cancel <id>`, `knowledge-put <key> <file>` and `knowledge-get <key>`.

## `spine mcp`

Serves the core's read-only tools over MCP stdio, connecting to the daemon with
`--addr`, and `--tls`, `--ca`, `--domain`, `--client-cert`, `--client-key` for TLS.
