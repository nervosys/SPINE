# Layer 5: Interaction (tools, signed RPC, tasks)

Status: description of the code as built on branch `completion/agent-web`. It is not a design. Where the code does not determine a behaviour the text says "Undetermined" and names the file inspected. Every behavioural statement carries a `path:line` reference relative to the repository root. The target direction is in `docs/FIRST_PRINCIPLES.md` (layer 5, line 43); the gaps against it are in section 6.

## 1. Scope

Three mechanisms exist and they are not connected to one another except where stated.

| Mechanism | Transport | Authority | Section |
| --- | --- | --- | --- |
| Native tools: `ToolCall` and `ToolResult` messages | Native wire, any client of the origin | None beyond the connection | 2 |
| Signed RPC between nodes (`MeshCall` and the mesh `Application` payload) | Encrypted, key-pinned TCP between mesh nodes | The Ed25519 key of the calling node, checked against an allowlist by the receiver | 3 |
| Durable tasks and immutable knowledge, carried over the signed RPC | As above | Allowlisted node key; records are scoped to that key | 4, 5 |

Out of scope: channel establishment (layer 1), names (layer 2), resources (`04-objects.md`). The legacy swarm, delegation and planning commands are refused by the origin (`docs/AGENT_WEB_QUICKSTART.md`, "Durable work over the signed mesh"; test at `src/spine-core/tests/agent_web.rs:462-478`) and are not described.

> As of 2026-11-21 the native wire is spine-wire v1, see 00-wire.md (normative, sections 2 to 14; the earlier wire is its Appendix A); the following describes the earlier wire.
>
> Correction: the `ToolResult` type is gone (a tool outcome is `ToolOutcome::Ok` or `ToolOutcome::Err`, carried in the `{ok}` body), correlation is the frame header's request id, and the framing described below is replaced by the v1 frame.

Messages are encoded as in `04-objects.md` section 3: an 8-byte header (`SP`, version 1, format byte, big-endian length) and a CBOR or JSON body, maximum 16 MiB (`src/spine-protocol/src/wire.rs:52-64`, `wire.rs:90-94`).

## 2. Tool model

### 2.1 Description of a tool

A tool is described by `Capability` (`src/spine-protocol/src/agentic.rs:258-273`):

| Field | Type | Notes |
| --- | --- | --- |
| `uri` | string | The tool name; the same string is passed as `ToolCall.name` |
| `description` | string | Free text |
| `input_schema` | JSON | "JSON-Schema-shaped"; not validated by the origin (section 2.4) |
| `output_schema` | JSON | Same |
| `embedding` | optional list of f32 | For semantic selection; always `None` here |

The origin offers two tools (`src/spine-core/src/tools.rs:4-14`):

| `uri` | Description | Result |
| --- | --- | --- |
| `spine.html/parse` | Structured semantic representation of supplied HTML | `UnifiedRepresentation` as JSON (`04-objects.md` section 6) |
| `spine.content/hash` | SHA-256 of supplied UTF-8 text | `{"sha256": <64 hex chars>, "bytes": <length>}` (`tools.rs:30-34`) |

Both advertise the same `input_schema`: `{"type":"object","required":["text"],"properties":{"text":{"type":"string","maxLength":1048576}},"x-spine-max-utf8-bytes":1048576,"x-spine-version":1,"x-spine-permission":"tool.read"}`. Both advertise `output_schema` `{"type":"object"}` and nothing more (`tools.rs:9-12`). The `x-spine-*` keys are descriptive only; nothing reads `x-spine-permission` (searched `src/spine-core/src/main.rs`, `tools.rs`, `tasks.rs`).

Discovery: `CapabilityQuery { id, selector }` with selector `Exact(uri)`, `Prefix(prefix)`, `Semantic { embedding, top_k }` or `All` (`agentic.rs:225-242`). The origin answers `CapabilityAd { id, agent_id, capabilities }` (`agentic.rs:246-253`). `Exact` compares equality, `Prefix` uses `starts_with`, `All` returns everything, and `Semantic` always returns an empty list (`main.rs:2439-2450`). `agent_id` is the node's `did:` name string when the namespace is enabled, else the literal `spine-core:local-unattested` (`main.rs:2451-2456`). The advertisement is not signed. Capability queries are not rate limited (`main.rs:2439-2469`).

### 2.2 Invocation and result messages

```
ToolCall   { id: string, name: string, args: json, trace: optional TraceContext }
ToolResult { id: string, outcome: ToolOutcome, trace: optional TraceContext }
ToolOutcome = Ok { content: json } | Err { code: string, message: string, details: optional json }
TraceContext { trace_id: [u8;16], span_id: [u8;8], flags: u8, state: list of (string,string) }
```

Definitions: `agentic.rs:51-94`, `agentic.rs:286-296`. They are variants `Message::ToolCall` and `Message::ToolResult` (`src/spine-protocol/src/lib.rs:826-829`).

Correlation: the caller chooses `id`; the result echoes it, and echoes `trace` (`tools.rs:43-46`, `main.rs:2474-2483`). The origin requires `id` to be 1 to 256 bytes (`tools.rs:18-20`). It keeps no table of ids: a repeated id is executed again.

### 2.3 Origin processing

For each `ToolCall` on a connection (`main.rs:2470-2491`):

1. Consume one token from the connection's rate bucket. Bucket capacity 100 tokens, refill 10 per second (`main.rs:361-382`). The bucket is keyed by a random id created per connection (`main.rs:2424-2427`) and removed when the connection ends (`main.rs:86-90`). Requests over `BrowserCommand` use a separate bucket keyed by the session id (`main.rs:2519-2531`). If no token is available the result is `Err { code: "rate_limit", message: "tool request rate exceeded" }`.
2. Call `tools::invoke(call)` inline in the connection's receive loop (`main.rs:2472`). Calls on one connection are therefore serial, and a 1 MiB parse blocks that connection's loop while it runs.
3. Send the `ToolResult`; a send failure ends the connection.

`invoke` (`tools.rs:16-55`) checks, in this order: `id` length; that `args.text` exists and is a string; that `text` is at most 1,048,576 bytes; then dispatches on `name`. Because `text` is checked before the name, a call to an unknown tool with no `text` returns `invalid_args`, not `unsupported_tool`. Extra argument fields are ignored.

Error codes produced (`tools.rs:18-40`, `main.rs:2478`):

| `code` | Condition |
| --- | --- |
| `invalid_args` | id length, `text` missing or not a string, `text` over 1 MiB |
| `unsupported_tool` | `name` is neither tool |
| `parse_error` | `parse_html` returned an error (`parse_html` returns `Ok` on every path read, `spine-parser/src/lib.rs:142-166`) |
| `internal` | result could not be serialised |
| `rate_limit` | bucket empty |

`details` is always `None`. No timeout is applied by the origin to a tool call. The SDK applies 30 seconds (section 2.5).

### 2.4 Schemas are advisory

The origin does not validate `args` against `input_schema`. It performs the checks listed in 2.3 by hand. A second implementation that validated schemas would reject different inputs than this one.

### 2.5 Client behaviour

`AgentClient::capabilities` sends `CapabilityQuery` with selector `All` and id `cap-<counter>`, and waits up to 10 seconds for the matching `CapabilityAd`, ignoring other messages (`src/spine-agent/src/lib.rs:254-280`). `call_tool` uses id `tool-<counter>`, sends the call and waits up to 30 seconds for a `ToolResult` with the same id, answers `Ping` with `Pong`, ignores other messages, and never retries. After a timeout the SDK says to reconnect (`lib.rs:353-383`). The client does not check that the result's tool or schema matches what was advertised.

### 2.6 Who may call

Any client that can open a native connection and complete whatever TLS or mTLS policy the operator configured. There is no authentication, session or per-caller permission in the `ToolCall` path (`main.rs:2470-2491`). `docs/NATIVE_OPERATIONS.md` states that native origin clients share one single-user trust boundary. The two tools are pure and read-only (`tools.rs:1`), which limits what an unauthenticated caller can do with them to consuming CPU within the rate bucket.

An MCP stdio adapter maps these two tools to MCP names and forwards calls to the origin (`src/spine-protocol/src/mcp.rs`, `docs/NATIVE_OPERATIONS.md`, "Use MCP tools"). It adds no authority.

### 2.7 Other native commands relevant to interaction

`BrowserCommand` carries many other operations (`lib.rs:1024-1195`): navigation, clicks, WASM execution (`ExecuteBinary`), knowledge storage, namespace publication and others. All pass through the same unauthenticated session loop (`main.rs:2495-2583`) with only the per-session rate bucket. They are not tools in the `Capability` sense and are not described here. `MeshCall` is described in section 3.

## 3. Signed RPC

### 3.1 Path of a call

1. A client of node A sends `BrowserCommand::MeshCall { peer_id: string (UUID), peer_key: [u8;32], payload: json }` (`lib.rs:1190-1194`). Any native client of A can do this; the call is made with A's identity, not the client's (`main.rs:582-607`).
2. A calls `MeshNameResolver::application_call(peer, peer_key, payload)` (`research/spine-agentic/src/naming_mesh.rs:379-438`).
3. B's mesh listener receives the envelope, B's pump checks the authenticated connection key (`research/spine-agentic/src/mesh_tcp.rs:1173-1186`), then `handle_envelope_with_reply` verifies and dispatches (`naming_mesh.rs:882-937`).
4. B's `MeshApplication::handle(caller_key, request)` produces the answer (`naming_mesh.rs:43-48`). The task service is the only implementation in the origin (`src/spine-core/src/namespace.rs:122-138`).
5. B signs a response envelope and returns it on the same socket (`naming_mesh.rs:916-935`).

The SDK wrapper `mesh_call` unwraps the reply: an `error` object becomes an error; otherwise the `result` field is returned (`src/spine-agent/src/lib.rs:294-320`).

### 3.2 Envelope fields

The signed unit is `MeshEnvelope` (`research/spine-agentic/src/mesh.rs:149-166`):

| Field | Type | Role in an Application RPC |
| --- | --- | --- |
| `id` | UUID | Random per envelope (`mesh.rs:741`) |
| `from` | `AgentId` | Sender's 16-byte UUID. Derived as the first 16 bytes of SHA-256 over the secret seed (`research/spine-agentic/src/lib.rs:245-250`, `identity.rs:166-167`), so a receiver cannot recompute it from the public key |
| `to` | `MeshTarget` | `Agent(recipient)` for RPC |
| `ttl` | u8 | `config.max_hops` (`mesh.rs:754`) |
| `hops` | list of `AgentId` | Starts with the sender |
| `payload` | `MeshPayload` | `Application(ApplicationMessage)` |
| `signature` | bytes | Ed25519 |
| `timestamp` | UTC time | Creation time |

The payload is `ApplicationMessage` (`mesh.rs:235-244`):

| Field | Type | Meaning |
| --- | --- | --- |
| `public_key` | [u8;32] | The sender's Ed25519 key |
| `recipient` | `AgentId` | Intended receiver |
| `recipient_key` | [u8;32] | Intended receiver's key |
| `request_id` | UUID | Random per call (`naming_mesh.rs:412`); copied into the response |
| `response` | bool | False in a request, true in a response |
| `data` | JSON | The application payload |

### 3.3 What is signed

`signature_data` is the concatenation of `id` (16 bytes), `from` (16 bytes) and `serde_json::to_vec(payload)` (`mesh.rs:916-924`). The signature is `identity.sign(...)` (Ed25519, `mesh.rs:746`).

Consequences, from the code:

- `recipient`, `recipient_key`, `request_id`, `response`, `public_key` and `data` are inside the signed payload.
- `to`, `ttl`, `hops` and `timestamp` are not signed. The comment on `signature` says it covers `id || from || to || payload` (`mesh.rs:162`) but the function does not include `to` (`mesh.rs:916-924`). Binding to the recipient therefore rests on `ApplicationMessage.recipient`, not on `to`.
- The signed bytes are `serde_json` output of the enum, not a specified canonical form. Field order follows the struct definition; object key order inside `data` follows `serde_json::Map`, which is sorted unless the `preserve_order` feature is enabled. No workspace `Cargo.toml` enables it (searched `Cargo.toml` and `src/*/Cargo.toml`); whether a dependency enables it transitively is Undetermined (Cargo.lock features not examined). A second implementation must reproduce this serialisation exactly or define a canonical one.

### 3.4 Recipient binding and pinning

Sender side (`naming_mesh.rs:379-438`): refuse a payload whose JSON exceeds 2 MiB; acquire one of 64 concurrency permits (`naming_mesh.rs:315`, `rpc_slots`) or fail with `application RPC concurrency exhausted`; find the peer by agent id in the authenticated routing table, else fail; require the table's key to equal `peer_key`, else fail with `application peer key does not match the pinned key`.

Transport (`mesh_tcp.rs:918-945`, `mesh_tcp.rs:1020-1052`): an Application envelope is refused unless the transport is encrypted; the dial or pooled connection must have an authenticated peer key equal to `recipient_key` (`mesh_tcp.rs:847-852`, handshake pinned at `mesh_tcp.rs:888-896`); `to` must equal `Agent(message.recipient)`; bootstrap delivery by bare address is refused. WebSocket and QUIC are not used for this path (`docs/AGENT_WEB_QUICKSTART.md`: "Plaintext, unpinned bootstrap delivery, WebSocket, and QUIC application sends are refused"; Undetermined in code for those two transports: searched `mesh_tcp.rs` only, `mesh_ws.rs` and `mesh_quic.rs` not read).

Receiver side (`naming_mesh.rs:882-890`): the envelope is dropped silently, with no reply, unless all of these hold: `message.recipient` equals the local agent id; `message.recipient_key` equals the local key; `envelope.to` equals `Agent(local id)`; the envelope signature verifies under `message.public_key` (`mesh.rs:734-737`); and `data` serialises to at most 2 MiB. A caller whose message is dropped sees only the 30 second timeout.

### 3.5 Sender authentication

Two checks, both required:

1. The signature verifies under the key carried in the message (`verify_carried_key`, `mesh.rs:734-737`). This proves possession of that key and nothing else, because the carried key is attacker-supplied.
2. `pump` discards an Application envelope unless the secure connection's authenticated peer key equals `message.public_key` (`mesh_tcp.rs:1175-1178`). The authenticated key comes from the channel handshake (layer 1).

The handler receives the verified 32-byte key as `caller`, never a claimed id (`naming_mesh.rs:43-48`, `naming_mesh.rs:903-907`). `envelope.from` is not checked against the key. On the response path the caller's waiter accepts a reply only when `message.public_key` equals the pinned key and `envelope.from` equals the pinned agent id (`naming_mesh.rs:891-901`).

The "caller" is a node key. All native clients of that node act under it (section 2.6, 3.1).

### 3.6 Replay and duplicate handling

- Within one encrypted connection, frames carry a counter used as AEAD nonce and associated data (`research/spine-crypto/src/handshake.rs:473-490`); a frame that fails to open ends the connection, and the comment names replay as a cause (`mesh_tcp.rs:597-601`).
- There is no application-level replay protection. The Application path does not consult `MessageDedup` (that tracker, `mesh.rs:358-409`, is used only inside `MeshNode::process_envelope`, `mesh.rs:763-776`, which nothing outside `mesh.rs` calls; searched `src/` for `process_envelope`). The `timestamp` is not checked. `request_id` is not recorded by the receiver.
- A repeated request from an authenticated peer therefore re-runs the handler. Every operation the task service offers is idempotent or read-only (section 4.3), so this is harmless today. The doc comment on `application_call` states the contract: timeouts never imply that a mutation did not run, and application ids must make retries idempotent (`naming_mesh.rs:376-378`).
- On the response side a waiter is removed on the first matching response; later duplicates find no waiter and are ignored (`naming_mesh.rs:891-901`).
- Delivery failures are never retried transparently for Application payloads (`mesh_tcp.rs:935-942`), and the SDK says never to retry a mutation after uncertain delivery (`lib.rs:292-293`).
- A separate `SignedFrame` module (`research/spine-agentic/src/signed_frame.rs`: magic `SPS1`, 32-byte key, 64-byte signature, then a wire frame) offers per-message detached signatures. Nothing outside `spine-agentic` references it (searched `src/` for `signed_frame::`), so it is not part of any path described here.

### 3.7 Allowlist

The task service holds a set of 32-byte keys from configuration `namespace.task_peers`, hex-decoded at startup (`namespace.rs:130-135`, `src/spine-core/src/config.rs:79-80`). The set must be non-empty (`tasks.rs:46-50`, and config validation at `config.rs:407`) and requires the namespace to be enabled (`namespace.rs:129`). `handle` returns `{"error":{"code":"not_authorized", ...}}` for any caller outside the set (`tasks.rs:354-357`); `request` refuses again with `peer signing key is not authorized for tasks` (`tasks.rs:141-145`). The set is read once at startup; changing it requires a restart (`docs/NATIVE_OPERATIONS.md`, "Supported task policy").

An allowed key may perform every task and knowledge operation. The `permissions` list in the capabilities reply (`task.submit`, `task.read`, `task.cancel`, `knowledge.read`, `knowledge.publish`) is advertised text; no code assigns permissions to keys (`tasks.rs:148-153`, `tasks.rs:141-221`).

### 3.8 Response format and limits

Reply `data` is `{"result": <value>}` on success or `{"error": {"code", "message"}}` on failure. Codes produced: `not_authorized`, `task_rejected` (any error from `request`, with `message` the error text), `unsupported` (application RPC disabled), `response_too_large` (reply over 2 MiB) (`tasks.rs:352-367`, `naming_mesh.rs:909-915`). Timeout: 30 seconds on the caller (`naming_mesh.rs:431-437`).

## 4. Task model

Defined in `src/spine-protocol/src/tasks.rs`; implemented in `src/spine-core/src/tasks.rs`. All requests below are the `payload` of a `MeshCall`, as `TaskOperation` JSON.

### 4.1 Operations

`TaskOperation` is tagged by an `operation` field in snake_case, with unknown fields denied (`protocol/tasks.rs:46-66`):

| `operation` | Fields | Effect |
| --- | --- | --- |
| `capabilities` | none | Returns `{protocol:"spine.tasks", version:1, tools, ownership:"caller signing key", permissions, limits}` (`core/tasks.rs:148-153`) |
| `submit` | `spec` | Admit a task |
| `status` | `id` | Return the task record |
| `cancel` | `id` | Cancel if pending or running |
| `knowledge_put` | `key`, `value` | Store an immutable entry (section 5) |
| `knowledge_get` | `key` | Read an entry |

### 4.2 Data

`TaskSpec { id: UUID, tool: string, args: json, dependencies: list of UUID (default empty), input_from: optional TaskInput, timeout_secs: u32 }`, unknown fields denied (`protocol/tasks.rs:13-25`).
`TaskInput { task_id: UUID, pointer: string }`, a JSON pointer into a dependency's successful result content (`protocol/tasks.rs:5-11`).
`TaskRecord { owner: [u8;32], spec, state, created_at: u64 (Unix seconds), completed_at: optional u64 (Unix seconds when the task first became terminal; absent in older records), result: optional ToolResult, error: optional string }` (`protocol/tasks.rs:36-44`).
`TaskState` is `Pending`, `Running`, `Completed`, `Failed`, `Cancelled`, serialised as those names (`protocol/tasks.rs:27-34`).

The caller supplies the task `id`. There is no server-assigned id and no caller-supplied idempotency key beyond that id.

### 4.3 Admission and idempotency

`submit` (`core/tasks.rs:154-183`):

1. Validate the spec (`core/tasks.rs:83-118`): `tool` must be `spine.html/parse` or `spine.content/hash`; `timeout_secs` 1 to 300; at most 16 dependencies; no self-dependency; `args` a JSON object of at most 64 KiB serialised; if `input_from` is set its `task_id` must be in `dependencies` and `pointer` at most 256 bytes.
2. If a record exists under this owner and id: return it when the specs are equal, else fail with `task id conflicts with an existing specification`. This check precedes the capacity check, so a duplicate succeeds even when the store is full.
3. Fail if the owner already holds 64 pending or running tasks. If the store holds 256 tasks (all owners, all states), evict one terminal task by the policy in 4.8, or fail with `task store capacity reached` when none is evictable.
4. Each dependency must already exist under the same owner, else the lookup fails (`task not found for this owner`). With the self-reference rule, this makes the graph acyclic.
5. Persist a `Pending` record with `created_at = now`, add it to the pending set, and return the record.

The record is persisted before the reply (`core/tasks.rs:181`).

### 4.4 States and transitions

| From | To | Trigger | Reference |
| --- | --- | --- | --- |
| (none) | `Pending` | Submit | `tasks.rs:173-181` |
| `Pending` | `Failed`, `error` = `task deadline exceeded` | `now - created_at >= timeout_secs` found while scheduling | `tasks.rs:235-240` |
| `Pending` | `Failed`, `error` = `dependency failed or was cancelled` | Any dependency `Failed` or `Cancelled` | `tasks.rs:247-255` |
| `Pending` | `Failed`, `error` = `dependency input pointer did not resolve` | `input_from` pointer finds nothing in the dependency's `Ok` content, or the dependency has no `Ok` result | `tasks.rs:259-270` |
| `Pending` | `Running` | All dependencies `Completed` (and pointer resolved); state persisted before execution | `tasks.rs:256-280` |
| `Running` | `Completed` | Tool returns `Ok` and the serialised `ToolResult` is at most 256 KiB | `tasks.rs:317-325` |
| `Running` | `Failed` with `result` set, `error` None | Tool returns `Err` outcome | `tasks.rs:319-324` |
| `Running` | `Failed`, `error` = `tool deadline, output bound, or execution failure` | Timeout, result over 256 KiB, or join failure | `tasks.rs:326-330` |
| `Running` | `Failed`, `error` = `task deadline exceeded` | Deadline passed by the time the result is recorded | `tasks.rs:311-316` |
| `Pending` or `Running` | `Cancelled`, `error` = `cancelled by owner` | `cancel` | `tasks.rs:185-193` |
| `Running` | `Pending` | Process restart | `tasks.rs:70-75` |

`Completed`, `Failed` and `Cancelled` are terminal; `cancel` on a terminal task returns the record unchanged (`tasks.rs:187-192`).

Dependency data flow: when `input_from` resolves, the pointed-to value replaces `args["text"]` in the execution copy only; the stored spec is unchanged so duplicate detection still compares the submitted spec (`tasks.rs:271-275`). If the value is not a string the tool returns `invalid_args` and the task becomes `Failed`.

### 4.5 Ownership

The owner is the verified caller key (`tasks.rs:173-175`). Storage keys are the 32 owner bytes followed by the 16 id bytes (`tasks.rs:34-36`), and every read, cancel and dependency lookup uses the caller's own key (`tasks.rs:119-125`, `tasks.rs:184-193`). One owner cannot read, cancel or depend on another's tasks. The 256-task store limit is global; an owner's pending and running tasks are capped at 64, and eviction at capacity is fair-share (4.8).

### 4.6 Scheduling, deadlines, cancellation

- A background loop ticks every 100 ms, skipping missed ticks (`tasks.rs:337-349`). Each tick may start up to 16 tasks, one after another, each awaited (`tasks.rs:285-335`). Execution is on a blocking thread with a timeout equal to the remaining deadline, minimum 1 second (`tasks.rs:296-303`).
- Pending tasks are examined in key order (owner bytes, then id) (`tasks.rs:225-226`, set type `BTreeSet`). There is no priority and no fairness across owners.
- The deadline is measured from `created_at`, so it includes time spent waiting for dependencies and time the process was down (`tasks.rs:235`, `protocol/tasks.rs:23`).
- Cancellation wins over a result: after execution the record is re-read, and if it is not `Running` the output is discarded (`tasks.rs:304-310`). A tool already running is not interrupted; `spawn_blocking` work continues until it returns (`tasks.rs:301`).
- Tasks owned by a key no longer in the allowlist are never scheduled (`tasks.rs:76-78`, `tasks.rs:232-234`) but remain in storage and count toward the 256 limit.

### 4.7 Persistence and restart recovery

Records are JSON values in namespace `spine-tasks-v1`, written on every state change (`tasks.rs:126-139`); the backend is the shared one from `04-objects.md` section 4.2 (SQLite at `records_path`, or memory when that path is empty, `main.rs:1592-1600`). In memory, tasks do not survive restart.

On startup (`tasks.rs:46-81`): scan all records; refuse to start if more than 256 exist, if any record's key differs from owner plus id, if JSON does not decode, or if any spec fails `validate`; rewrite every `Running` record as `Pending`; queue every `Pending` record owned by an allowed key. Interrupted tasks then run again from the start. The code comment states this is safe only because every admitted tool is read-only and deterministic and "must change before adding side effects" (`tasks.rs:71-72`). Because the deadline runs from `created_at`, a task interrupted by a long outage fails with `task deadline exceeded` when next examined.

### 4.8 Limits

| Limit | Value | Reference |
| --- | --- | --- |
| Tasks stored | 256, all owners, all states | `tasks.rs:17`, `tasks.rs:164-167` |
| Dependencies per task | 16 | `tasks.rs:95-98` |
| `args` | JSON object, 64 KiB | `tasks.rs:18`, `tasks.rs:103-106` |
| Result | 256 KiB serialised | `tasks.rs:19`, `tasks.rs:318` |
| Deadline | 1 to 300 seconds | `tasks.rs:91-94` |
| Pointer length | 256 bytes | `tasks.rs:112-115` |
| RPC request and reply | 2 MiB each | `naming_mesh.rs:385-393`, `naming_mesh.rs:913-915` |
| Outgoing concurrent RPCs per node | 64 | `naming_mesh.rs:315` |
| Starts per tick | 16, tick 100 ms | `tasks.rs:286`, `tasks.rs:340` |

Eviction when the store is full. A terminal task is a candidate only if no pending or running task of the same owner (and not the task being admitted) lists it as a dependency. Owners are ranked by entries held (any state), most first, ties to the smaller owner key. A submitting owner that already holds 64 or more entries may evict only its own candidates and is refused otherwise; any other submitter evicts from the highest-ranked owner that has a candidate. The victim is that owner's candidate with the earliest `completed_at` (`created_at` if absent), ties broken by earlier `created_at` then smaller task id. One owner therefore cannot displace another's finished results once it holds its share, and a task that finished recently is not evicted ahead of older finished work. If no victim exists, `submit` fails with `task store capacity reached`. Duplicate submissions still succeed when the store is full.

## 5. Immutable knowledge entries

Operations `knowledge_put` and `knowledge_get` (`tasks.rs:194-219`), stored in namespace `spine-task-knowledge-v1`.

- Key: 1 to 256 bytes; the storage key is the 32-byte owner key followed by the key bytes (`tasks.rs:37-43`). Entries are visible only to the key that wrote them.
- Value: any JSON value whose serialisation is at most 64 KiB (`tasks.rs:196-197`).
- Put on a new key stores the value. Put on an existing key succeeds only if the serialised bytes are identical; otherwise it fails with `knowledge key already holds different content; use a new versioned key` (`tasks.rs:198-204`). Equality is byte equality of `serde_json` output, so object key order does not vary (sorted maps, see 3.3).
- Capacity: 64 entries per owner and 256 in total, checked on first insertion; a new key over either limit is refused with a distinct error (`knowledge owner quota reached` or `knowledge worker capacity reached`). Entries are never evicted and there is no delete. Residual exposure (Stage 6, authority and admission cost): owner keys are free to mint, so four allowed identities can fill the worker total.
- Get returns the stored JSON value directly (not wrapped), or fails with `knowledge not found for this owner` (`tasks.rs:213-219`).

Entries carry no signature, hash, timestamp or author field beyond the implicit owner key in the storage key. Nothing links an entry to a task or a resource; the quickstart suggests storing a JSON descriptor by convention (`docs/AGENT_WEB_QUICKSTART.md`, "Durable work over the signed mesh").

Note: this store is unrelated to the session-level `StoreKnowledge`, `QueryKnowledge`, `DeleteKnowledge` and `ProposeKnowledge` native commands (`lib.rs:1058-1097`), which are mutable, per-session, and not covered here.

## 6. Gaps against the target design

Target: `docs/FIRST_PRINCIPLES.md` line 43 and premise 5 (line 25).

- [ ] **Effect class declaration (read, reversible, irreversible).** Absent. `Capability` has no effect field (`agentic.rs:258-273`). Both tools are read-only by construction, and the only record of that is the string `tool.read` in an unread schema key (`tools.rs:11`) and a task-restart comment (`tasks.rs:71-72`). Nothing prevents adding a tool without a declaration.
- [ ] **Declared cost.** Absent. The `Capability` docs defer cost to "higher-layer marketplaces" (`agentic.rs:255-256`). Limits are fixed constants and are partly advertised in the task `capabilities` reply (`tasks.rs:152`); per-call cost and metering are not.
- [ ] **Idempotency keys.** Partial and task-specific. `submit` is idempotent on `(owner, id, spec)` (`tasks.rs:156-163`), and knowledge puts are idempotent on identical content. `ToolCall` has none: a repeated id is executed again and the origin tracks no ids (section 2.2). Mesh `request_id` is not recorded (section 3.6).
- [ ] **Dry-run.** Absent. No message, field or tool parameter requests validation without execution.
- [ ] **Signed receipts.** Absent. A `ToolResult` and a `TaskRecord` are unsigned JSON. The worker's response envelope is signed (`naming_mesh.rs:916-926`), but verification happens at the caller's node; the caller's client receives only the unwrapped result over its own native connection, with no signature retained or exposed (`main.rs:595-606`, `spine-agent/src/lib.rs:310-319`). Nothing binds a result to the request, the executing key, the input hash or the time, in a form a third party can check.
- [ ] **Resume after disconnect.** Partial for tasks only: state is durable and can be queried again by id after reconnecting, and interrupted work is re-run (section 4.7). There is no progress stream, no cursor and no resume of a `ToolCall`; the SDK says to reconnect after a timeout (`spine-agent/src/lib.rs:381-382`). Session resume is listed as open work in `docs/NATIVE_OPERATIONS.md` ("Native session ownership and reconnect/resume APIs remain open work").
- [ ] **Authorization scopes per caller.** Absent. `ToolCall` needs no authority (section 2.6). For tasks the allowlist is binary: any allowed node key may submit, read, cancel and use knowledge (section 3.7). The advertised `permissions` list is not enforced. Every native client of a node can issue `MeshCall` under that node's key (section 3.1). There is no delegation, narrowing, expiry or revocation of authority.
- [ ] **Task eviction.** Absent. Terminal tasks and all knowledge entries stay forever and count toward fixed limits of 256, shared across owners; a few callers can exhaust the store permanently (section 4.8).
- [ ] **Connection of tasks to WASM execution.** Absent. The task validator admits only the two named tools (`tasks.rs:84-90`); `tools::invoke` reaches no WASM runtime. A WASM runtime exists in the origin state (`main.rs:1640`) and is reachable through the separate `ExecuteBinary` native command, which is unauthenticated and not task-mediated (`lib.rs:1037`). Whether `ExecuteBinary` is metered or capability-confined was not examined (Undetermined; searched only for its definition in `lib.rs` and the construction of the runtime in `main.rs`).

Further defects and unsafe properties found while reading (not in the requested checklist):

- [ ] **Unauthenticated tool calls and unauthenticated `MeshCall`.** Section 2.6 and 3.1. `MeshCall` lets any native client act with the node's signing key toward any allowlisting peer. The protection is that the node's own task allowlist (on the receiving worker) must contain the calling node's key; the origin itself does not restrict who on its side may use that key.
- [ ] **No application-level replay defence** (section 3.6). Safe today only because every operation is idempotent; the first non-idempotent operation will be exposed.
- [ ] **`to`, `ttl`, `hops` and `timestamp` unsigned; signed bytes depend on `serde_json` serialisation** (section 3.3). The comment at `mesh.rs:162` misdescribes the signed fields.
- [ ] **`AgentId` derived from the secret seed** rather than the public key (section 3.2). It cannot be checked against the public key by a receiver, and the routing table's agent-to-key mapping is the only binding.
- [ ] **Silent drops.** An envelope that fails any receiver check gets no reply and the caller waits 30 seconds (section 3.4).
- [ ] **Deadline includes downtime** and tasks restart from scratch (section 4.7); fine for pure read-only tools, wrong for any tool with side effects.
- [ ] **Result cap makes large parses fail.** A `spine.html/parse` result over 256 KiB serialised yields `Failed` with a generic error (`tasks.rs:318-330`); the caller is not told which bound was hit.
- [ ] **Unknown-tool error ordering** (`invalid_args` before `unsupported_tool`, section 2.3).
- [ ] **`Semantic` capability selection returns nothing** although the selector is defined (`main.rs:2448`).

## 7. Undetermined

| Item | Files searched |
| --- | --- |
| Whether a transitive dependency enables `serde_json/preserve_order` (affects signed bytes) | `Cargo.toml`, `src/*/Cargo.toml`; `Cargo.lock` not examined |
| Behaviour of Application RPC over WebSocket and QUIC mesh transports in code | `mesh_tcp.rs` only; `mesh_ws.rs` and `mesh_quic.rs` not read |
| Metering and capability confinement of `ExecuteBinary` | `spine-protocol/src/lib.rs:1037`, `spine-core/src/main.rs:1640`; handler body not read |
| Whether `accept_announcement` and the mesh enforce size or admission limits on announced records | `naming_mesh.rs:938-955` only |
| Contents of the channel handshake that provide the authenticated peer key | Layer 1; only `handshake.rs:406-490` consulted for frame counters |
