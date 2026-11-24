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

`--host 0.0.0.0` binds every interface. A host that is not loopback is refused
at startup unless the configuration enables mutual TLS (`tls.mutual_tls = true`,
with TLS and a client CA) or sets `server.allow_unauthenticated_remote = true`,
which states that the network itself is trusted. Leave `--host` off, or use
`127.0.0.1`, for a node that only this machine reaches.

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

Actions: `status`, `create`, `revoke <record>`, `rotate`, `retire`,
`verify <log>`, `resolve <name>`, `providers <capability>`,
`publish <record>`, `renew <record>`, `upload <record> <resource>` and
`fetch <name> --output <file>`. Run `spine name --help` for each action's
options.

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

### Changing a name's key

A name is its first key (the inception key) for ever; a succession log says
which key signs its records now. `rotate`, `retire` and `verify` work offline
on files, like `create` and `revoke`.

```bash
# First rotation: owner.key signs; k1.key becomes current; k2.key is committed
# to for the next rotation. k1.key and k2.key are created if they do not exist.
spine name rotate --key owner.key --reveal-key k1.key --next-key k2.key \
    --output log1.json

# Records of the name are now signed by the current key, with the log.
spine name create --key k1.key --log log1.json --output record.json \
    --endpoint 127.0.0.1:8080
spine name publish record.json --log log1.json

# A later rotation reveals the key committed to before and commits to a new one.
spine name rotate --log log1.json --key k1.key --reveal-key k2.key \
    --next-key k3.key --output log2.json

# Check a log, and a record against it.
spine name verify log2.json --record record.json

# Retire the name for good: no key signs for it afterwards.
spine name retire --log log2.json --key k2.key --reveal-key k3.key \
    --output retired.json
```

- `rotate` writes a new log file and never changes the old one. Keep the
  `--next-key` file: without it the name can be neither rotated again nor
  retired. The key files are written before the log.
- `--log <file>` on `create`, `renew` and `revoke` signs as the log's current
  key (`--key` must be that key) for the name of the log; the record carries
  the log position it was signed at. `publish --log` sends the log with the
  record. Without `--log` these commands behave as they always did.
- `retire` with no `--log` retires a name that never rotated, with its own
  key. With a log it needs `--reveal-key`, the key the last entry committed to.
- A node that holds the log refuses every record signed by an older key, and
  refuses everything for a retired name. A node that has seen two different
  logs for one name treats the name as compromised and refuses all of it.
- `upload <record> <resource> --log <file>` publishes a rotated name's
  resource: the record is checked against the log and the log is sent with it.
  `fetch <name> --output <file> --log <file>` does the same for a read. The
  daemon's answer to a resolve carries no log, so `fetch` without `--log`
  refuses a rotated name's record ("needs its succession log") instead of
  trusting it; and `upload` without `--log` refuses a record that carries a
  succession claim. A name that never rotated needs no `--log`. The same
  refusal applies to the gateway's resource routes, which hold no log.
- A node renews a rotated name of its own (`renew_owned_records`) only when
  its one key is the name's current key; otherwise it skips the name and logs a
  warning. See `docs/spec/02-names.md` section 10.11.

## `spine task --addr <addr> --peer <id> --peer-key <hex> <action>`

Sends a `mesh_call` through the daemon to a pinned peer (a 64-hex-character
Ed25519 key). Actions: `capabilities`, `submit <spec file>`, `status <id>`,
`cancel <id>`, `knowledge-put <key> <file>` and `knowledge-get <key>`.

## `spine mcp`

Serves the core's read-only tools over MCP stdio, connecting to the daemon with
`--addr`, and `--tls`, `--ca`, `--domain`, `--client-cert`, `--client-key` for TLS.
