# Local agent-web workflow

SPINE is a Rust stack for software agents: it turns HTML into structured semantic data, carries agent messages and tools over native frames, resolves signed `spine://` names through a mesh, and exposes HTTP, MCP, and gRPC bridges. Names identify publishers and capabilities; signed content hashes bind retrieved bytes to their publisher.

This guide covers the portable implementation in this checkout. Read [completion status](COMPLETION_STATUS.md) for validation evidence and outstanding release gates. These changes are local source changes, not a published release.

## Prerequisites

Install Rust, Cargo, and `protoc`. The verified development toolchain is Rust 1.98.1 on Windows x64. Rust 1.89 is the minimum supported toolchain; compilation at the minimum is checked with the GUI excluded, matching CI's minimum-toolchain scope. Build the native applications:

```sh
cargo build -p spine-core -p spine-cli -p spine-gateway
```

Run each node from its own data directory. Cargo may place binaries outside the workspace if a global target directory is configured; `cargo metadata --format-version 1 --no-deps` reports `target_directory`. The examples below assume `target/debug` and use `spine`; append `.exe` on Windows. Use an absolute executable path when changing working directories.

## Run two nodes

Save this as `node-a/spine.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8080
metrics_port = 9090

[namespace]
enabled = true
port = 9440
key_path = "spine-node.key"
records_path = "spine-names.sqlite"
maintain_secs = 60
seeds = []
```

Save this as `node-b/spine.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8180
metrics_port = 9190

[namespace]
enabled = true
port = 9540
key_path = "spine-node.key"
records_path = "spine-names.sqlite"
maintain_secs = 60
seeds = ["127.0.0.1:9440"]
```

Start a `spine-core` process in each directory. The origin listener uses TCP on the configured port, WebSocket on port + 1, and QUIC on port + 2 when enabled. The legacy cluster listener uses port + 1000. Mesh peers use a separate encrypted listener. DNS bootstrap is optional: set `dns_seeds = ["seed.example.org:9440"]`; DNS supplies addresses, and the mesh handshake verifies identities carried by responders. A self-signed identity is not an operator trust anchor; task calls explicitly pin the expected peer public key.

These examples use trusted local TCP clients. For remote origins, configure TLS with real certificates and connect through the Rust SDK's `connect_tls` / `connect_mtls` methods. Missing required certificate files cause startup to fail. Native TCP and WSS honor the same explicitly configured mTLS policy; see the connection limits and auxiliary listener boundaries in [native operations](NATIVE_OPERATIONS.md#connection-and-tls-policy). The `name` CLI workflow currently uses plain TCP; do not treat it as a confidential remote transport. The custom mesh handshake still requires independent review before production claims.

## Publish and fetch a resource

Create a small `example.txt`, then run these commands from an owner directory. Key and record output paths must not already exist:

```sh
spine name create --key owner.key --output record.json --endpoint 127.0.0.1:8080 --path /documents/example --capability example.documents --resource example.txt --ttl 3600
spine name --addr 127.0.0.1:8080 upload record.json example.txt --content-type text/plain
spine name --addr 127.0.0.1:8180 providers example.documents
```

`create` prints the public `spine://did:.../documents/example` name. Substitute that exact value for `NAME` below:

```sh
spine name --addr 127.0.0.1:8180 resolve NAME
spine name --addr 127.0.0.1:8180 fetch NAME --output downloaded.txt
spine name --addr 127.0.0.1:8080 renew record.json --key owner.key --output renewed.json --ttl 3600
```

Renewal signs a higher sequence with the same owner key and saves the record before sending it, so failed delivery leaves a recoverable publication. Preserve the latest record. A lower sequence cannot replace a newer stored record. Reusing a name with different bytes requires a new signed record with a higher sequence and an updated content hash.

Resource limits are 8 MiB per representation, 64 KiB per download chunk, and 256 MiB of serialized storage per origin. Reads verify the content hash, chunk bounds, progress, and final size. The SDK's `fetch_resource` resolves a name and tries TCP endpoints in priority order, with connection and transfer deadlines. Retries apply only to these immutable reads. `FetchName` continues to return endpoints without transferring bytes.

Locally published records and resource bytes persist in SQLite. Learned resolution caches, peer contacts, and remote replicas are ephemeral and revalidated through the mesh after restart. Publication responses report local persistence, directed sends, failures, broadcast fallback, and mesh errors. `replicas` is a legacy alias for directed sends; `replication_acknowledged` is zero and `durable` is false because remote storage has no acknowledgement contract. Periodic maintenance reoffers live records; it cannot promise remote disk durability.

To renew records owned by the node's own key automatically, set `renew_owned_records = true` and `renewal_secs = 60`. Choose a cadence below half the shortest record TTL. The node never renews records signed by another owner. Back up owner keys securely: changing a key changes its DID authority and requires publishing new names, rather than taking over the old names.

## HTTP gateway and real model output

Run `spine-gateway` with the same backend `server.host` and `server.port` settings as node A. Its HTTP port is `metrics_port + 1`, normally 9091. Set `SPINE_GATEWAY_BEARER_TOKEN` through your process environment or secret manager. Every route uses the existing bearer-auth middleware; `SPINE_GATEWAY_ALLOW_UNAUTH=1` is an explicit development override.

Configure an OpenAI-compatible model base that includes `/v1`:

```text
SPINE_MODEL_URL=http://127.0.0.1:8000/v1
SPINE_MODEL_TOKEN=<optional model token>
```

`POST /v1/chat/completions` forwards complete JSON messages, model, tools, and provider options. Non-streaming replies are bounded to 8 MiB. With `stream: true`, it forwards SSE bytes, including Unicode, tool deltas, usage, and terminal events, with backpressure. Dropping the HTTP response closes the upstream stream. Connection timeout is 10 seconds and the request deadline is 300 seconds. Errors redact upstream bodies and credentials. SPINE does not execute tool calls emitted by a model automatically.

For remote models through IronGate, set `IRONGATE_URL` to its base (for example `http://127.0.0.1:7700`, without `/v1`), optional `IRONGATE_TOKEN`, and `SPINE_REQUIRE_GATEWAY=1`. Only the gateway token goes to IronGate; provider credentials stay out of that request. Loopback model sources remain direct. With no model configured, chat returns 503. Echo output requires `SPINE_GATEWAY_DEMO=1`, which cannot be combined with a configured model or required gateway.

`POST /v1/resources` accepts `{ "record": <signed record JSON>, "content_type": "text/plain", "data": [<bytes>] }`. Its JSON body limit is 40 MiB to accommodate the byte-array representation, while decoded resource data remains limited to 8 MiB. `GET /v1/resources?name=NAME` resolves and reads verified bytes from the configured origin only; it does not dial arbitrary advertised endpoints. URI-encode the name. Unsupported or disconnected lookups remain visible; they are not negative-cached as confirmed absence. The download sets `nosniff` and private revalidation cache policy. HTTP namespace operations remain available under `/v1/names` and the Swagger UI.

## Native tools

`AgentClient::capabilities` queries versioned tool schemas and permissions; `call_tool` sends real native `ToolCall` / `ToolResult` messages with correlation IDs. The portable origin implements `spine.html/parse` and `spine.content/hash`, each accepting `{ "text": "..." }` with a 1 MiB text limit. Tools have no filesystem, network, or process access and are subject to connection-specific rate limits. Unknown tools return `unsupported_tool`. Calls are serialized, have a 30 second SDK deadline, and are never replayed automatically; reconnect after a timeout.

## Durable work over the signed mesh

Run `spine name --addr 127.0.0.1:8080 status` to obtain the coordinator's public key. In node B's namespace configuration set `tasks_enabled = true` and `task_peers = ["COORDINATOR_PUBLIC_KEY_HEX"]`, then restart B. Run `spine name --addr 127.0.0.1:8180 status` and retain B's agent ID and public key through a trusted operator channel. Bootstrap gives routing contacts; it does not authorize task access. Native clients of A share its identity, so keep A inside your single-user trust boundary.

Query the worker's contract through A:

```sh
spine task --addr 127.0.0.1:8080 --peer WORKER_AGENT_ID --peer-key WORKER_PUBLIC_KEY_HEX capabilities
```

Save `first.json`:

```json
{"id":"00000000-0000-4000-8000-000000000001","tool":"spine.content/hash","args":{"text":"artifact source"},"dependencies":[],"timeout_secs":300}
```

Save `second.json`; the dependency's `/sha256` result becomes its text input:

```json
{"id":"00000000-0000-4000-8000-000000000002","tool":"spine.content/hash","args":{},"dependencies":["00000000-0000-4000-8000-000000000001"],"input_from":{"task_id":"00000000-0000-4000-8000-000000000001","pointer":"/sha256"},"timeout_secs":300}
```

Use the same pinned address options for each call:

```sh
spine task --addr 127.0.0.1:8080 --peer WORKER_AGENT_ID --peer-key WORKER_PUBLIC_KEY_HEX submit first.json
spine task --addr 127.0.0.1:8080 --peer WORKER_AGENT_ID --peer-key WORKER_PUBLIC_KEY_HEX submit second.json
spine task --addr 127.0.0.1:8080 --peer WORKER_AGENT_ID --peer-key WORKER_PUBLIC_KEY_HEX status 00000000-0000-4000-8000-000000000002
```

Allocate fresh UUIDs for new work. An identical submission with the same owner, UUID, and specification is idempotent; changed specifications fail. Dependencies must already exist under that owner. `cancel ID` prevents pending execution and wins over result publication. A timeout does not prove a submission failed: query the original ID after reconnecting. Interrupted deterministic read-only work resumes after restart within its original deadline, including time spent queued. Completed work remains queryable. There are no automatic mutative RPC retries.

To share knowledge, save a JSON artifact descriptor in `artifact.json`, then call `knowledge-put artifact-v1 artifact.json` and `knowledge-get artifact-v1` with the same address options. Knowledge is owner scoped and immutable; publish changed content under a new versioned key. Resource artifacts remain signed content-addressed publications retrievable through the separate resource API.

Limits: 256 stored tasks and 256 knowledge keys per worker, 16 dependencies per task, 64 KiB arguments/knowledge, 256 KiB task result, deadlines from 1 to 300 seconds, and 64 concurrent outgoing mesh RPCs with a 30-second timeout. Task RPC requires encrypted TCP: its recipient key pins the handshake, pooled sockets must match that authenticated peer, and inbound task signatures must match the connection's authenticated caller. Plaintext, unpinned bootstrap delivery, WebSocket, and QUIC application sends are refused. Only hashing and HTML parsing are permitted. Unsupported legacy swarm/delegation/planning commands return `unsupported_legacy_swarm`; their previous simulated execution is removed. Arbitrary agent planning, shell execution, and model-driven task execution remain experimental.

## Backup, restart, and validation

Stop the node before copying its key, SQLite database, sessions, and knowledge directories. With a running SQLite database, use a SQLite online backup operation or include its WAL state; copying only the main file during writes is insufficient. Preserve restrictive Unix key permissions or apply an owner-only Windows ACL in the deployment account. Restart from the same directory to retain identities, records, and resource bytes. Treat externally created owner keys as separate backup assets.

Run the local acceptance and validation commands:

```sh
cargo test -p spine-core --test agent_web -- --nocapture
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
python scripts/test_verify.py
cargo audit
cargo deny check advisories licenses
```

The process-level tests exercise discovery, capability schemas, native tools, multi-chunk resource integrity, publisher restart and renewal, managed shutdown, and three-node signed task coordination. They complete dependent tasks, reject an unauthorized peer and incorrect recipient key, and restore tasks and knowledge after forced worker restart. They do not establish hardware performance, production tenancy isolation, public-network availability, or an independent cryptographic review. See [native operations](NATIVE_OPERATIONS.md) for archive installation, deployment, backups, and coordinated upgrades.

For complete retained validation logs, set `SPINE_VERIFY_LOG_DIR` to a local directory and run `bash scripts/verify.sh 1450` (use Git Bash on Windows). The script writes separate `tests.log`, `clippy.log`, and `fmt.log` files. Test totals must be updated when tests are deliberately added, and logs should remain local.


### Use an MCP client

With the local core running, configure an MCP host to launch `spine mcp --addr 127.0.0.1:8080`. It exposes the native core's actual HTML parsing and content hashing tools as `spine_html_parse` and `spine_content_hash`. Tool schemas and results cross the native protocol; the CLI does not execute a sample echo handler. For TLS/mTLS flags, limits, and reconnect behavior see [native operations](NATIVE_OPERATIONS.md#use-mcp-tools).
