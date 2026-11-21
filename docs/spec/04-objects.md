# Layer 4: Objects (resources, content, provenance)

Status: description of the code as built on branch `completion/agent-web`. It is not a design. Where the code does not determine a behaviour the text says "Undetermined" and names the file inspected. Every behavioural statement carries a `path:line` reference relative to the repository root. The target direction is in `docs/FIRST_PRINCIPLES.md` (layer 4, line 42); the gaps against it are collected in the final section.

## 1. Scope

This document covers what a node holds and serves as content:

- the signed name record that binds a name to a content hash (`spine-name`);
- the origin resource store and its transfer messages (`spine-core/src/resources.rs`, `spine-protocol/src/resource.rs`, handlers in `spine-core/src/main.rs`);
- the client side of transfer (`spine-agent/src/resources.rs`);
- typed links (`spine-name/src/link.rs`);
- the storage interface and its SQLite implementation (`spine-storage/src/lib.rs`);
- the semantic parser's output types (`spine-parser/src/lib.rs`, `spine-parser/src/extraction.rs`).

Out of scope here: name syntax and key handling (layer 2), the encrypted mesh channel (layer 1), tools, tasks and signed RPC (layer 5, `05-interaction.md`).

`spine-web` (`src/spine-web/src/lib.rs:1-124`) contains no logic. It re-exports component crates behind Cargo features and defines no objects, so it is not described further.

Two surfaces expose resources. The native surface is `BrowserCommand` over the native wire (section 3). The HTTP gateway (`spine-gateway/src/names.rs:84-155`) wraps the same SDK calls; it adds bearer authentication (`docs/AGENT_WEB_QUICKSTART.md`, "HTTP gateway") and is not specified further.

## 2. The resource model

### 2.1 What an object is today

There is no general object type. Content is an opaque byte string identified by a resource id (SHA-256 over its content type and bytes, section 2.6), plus a signed record that names it. The two halves are stored and moved separately.

| Half | Type | Where defined | Contents |
| --- | --- | --- | --- |
| Binding | `NameRecord` | `spine-name/src/record.rs:53-85` | name, sequence, TTL, endpoints, capabilities, content hash, links, metadata, signature |
| Bytes | `Blob` | `spine-core/src/resources.rs:12-16` | `content_type` string and `data` bytes, stored under the resource id |

### 2.2 `NameRecord` fields

| Field | Type | Meaning as built |
| --- | --- | --- |
| `name` | `SpineUri` | The bound name. `NameRecord::new` fails unless the authority is `did:` (`record.rs:90-93`). |
| `seq` | u64 | Higher `seq` supersedes lower (`record.rs:304-306`). |
| `ttl_secs` | u32 | Freshness after `published_at`. Default 3600 (`record.rs:22`). |
| `published_at` | u64 | Unix seconds. |
| `endpoints` | list of `Endpoint {transport, address, priority: u8}` | Where to reach the resource (`record.rs:26-34`). Lower `priority` is tried first (`record.rs:308-313`). |
| `capabilities` | list of string | Lower-cased terms indexed by the store (`record.rs:174-177`). |
| `content_hash` | optional 32 bytes | For a resource record, the resource id of the representation (section 2.6: SHA-256 over content type and bytes). This is the cache validator. |
| `links` | list of `Link` | Typed edges (section 5). |
| `meta` | map string to string | Free-form. No key is defined or interpreted anywhere in the files read. |
| `signature` | bytes (64) | Ed25519 over `signing_bytes()`; serialized as base32 (`record.rs:322-335`). |

Name authorities (`spine-name/src/uri.rs:35-48`): `did:` (Ed25519 key, base32), `blob:` (SHA-256, base32), `cap:` (capability term), `host:` (bootstrap address, unsigned). Only `did:` names can carry a record (`record.rs:90-93`). A `blob:` name is accepted syntactically, but `LocalResolver::resolve` has no `blob:` branch (`resolver.rs:296-333`): it falls through to the cache and store, where no record under a `blob:` name can exist. Whether any other code resolves `blob:` names is Undetermined from the files read (searched `spine-name/src/resolver.rs`, `spine-core/src/main.rs`).

### 2.3 What the signature covers

`signing_bytes` (`record.rs:200-248`) is built field by field, not by serializing the struct. Layout, in order:

1. the ASCII string `spine-name-record-v1` followed by one zero byte;
2. the canonical name string, then a zero byte;
3. `seq`, `ttl_secs`, `published_at` as big-endian integers (8, 4, 8 bytes);
4. endpoint count as big-endian u32, then for each endpoint `transport`, 0, `address`, 0, one `priority` byte;
5. capability count as big-endian u32, then each capability followed by 0;
6. `content_hash`: byte `1` plus the 32 bytes, or byte `0`;
7. link count as big-endian u32, then for each link the relation string, 0, the target name string, 0;
8. metadata count as big-endian u32, then for each entry key, 0, value, 0 (map order is sorted, `BTreeMap`).

Not covered by the signature: a link's `title` and `capabilities` (`link.rs:99-106` versus `record.rs:232-238`). They can be altered in transit without invalidating the record. The code comments describe them as hints (`link.rs:102-103`).

`content_type` is not a record field, but for a resource record it is covered by the signature indirectly: the signed `content_hash` is the resource id, which commits to the content type (section 2.6).

### 2.6 Resource id (content type bound into identity)

Resolved: the content type used to be covered by neither the hash nor any signature, so a serving peer could relabel verified bytes. The id of a resource is now computed by one function, `spine_name::resource_id(content_type, bytes)`, used by every publisher (SDK, CLI, gateway, origin) and every verifier (`spine_name::verify_resource`):

```
id = SHA-256( RESOURCE_ID_DOMAIN
           || u32_be(len(content_type)) || content_type
           || u64_be(len(bytes))        || bytes )
RESOURCE_ID_DOMAIN = "spine/resource-id/v1" followed by one zero byte (21 bytes)
```

`content_type` is the UTF-8 (ASCII) bytes of the string. Length prefixes make the encoding unambiguous, so moving a byte across the type/data boundary changes the id. The domain constant keeps a resource id from colliding with the plain `content_hash` of the same bytes. Content types are exact-match canonical, not normalised: 1 to 256 bytes of printable ASCII (0x20 to 0x7e), no uppercase letters, no leading or trailing space; anything else is `NameError::InvalidContentType`. A type or bytes that differ from what the id commits to is `NameError::ResourceIdMismatch`. The plain `content_hash` (SHA-256 of bytes) remains for `blob:` names and the `spine.content/hash` tool only; it is not a resource id.

Verification (`record.rs:269-282`) uses the public key inside the `did:` name; there is no other trust anchor. The signing key must match the name (`record.rs:254-265`).

### 2.4 Validator and freshness

- `content_hash` is the validator. `ResolveName` and `FetchName` accept `if_none_match` (a 32-byte hash). When it equals the record's `content_hash` the reply is `Unchanged { ttl_secs }` with no record (`main.rs:457-463`, `lib.rs:1199-1210` in `spine-protocol`). A record with no `content_hash` never matches.
- A record is stale when `now >= published_at + ttl_secs` (`record.rs:285-292`). `PublishName` and `PublishResource` refuse an already-expired record (`main.rs:637-639`, `main.rs:796-805`).
- Supersession: `(seq, published_at)` compared lexicographically (`record.rs:304-306`). Publishing an unequal record that does not supersede the stored one fails with `StalePublication` (`resolver.rs:198-202`, `resolver.rs:213-219`).

### 2.5 Size limits and quotas

| Limit | Value | Reference |
| --- | --- | --- |
| Representation size | 8 MiB (`MAX_RESOURCE_BYTES`) | `spine-protocol/src/resource.rs:6`, enforced `resources.rs:33` |
| Download chunk | 1 to 65,536 bytes (`MAX_RESOURCE_CHUNK` = 64 KiB) | `resource.rs:7`, `resources.rs:70-73` |
| Content type | 1 to 256 bytes, each byte 32 to 126 | `resources.rs:34-38` |
| Origin resource store | 256 MiB of serialized storage | `resources.rs:10`, `resources.rs:55-64` |
| Name records per node | 10,000 (`DEFAULT_CAPACITY`) | `spine-name/src/store.rs:39`, `resolver.rs:220-226` |
| Native message body | 16 MiB | `spine-protocol/src/wire.rs:60` |
| Batch resolution | 256 names, 16 concurrent | `main.rs:675-694` |
| Crawl defaults | depth 3, 1,000 visits | `spine-name/src/frontier.rs:68-76` |

Bounds on the record itself (count of endpoints, links, capabilities, size of `meta`, length of strings): none found in `spine-name/src/record.rs`, `store.rs`, `resolver.rs` or `uri.rs`. A record is therefore bounded only by the message limit and, for mesh announcements, by the mesh frame limit (4 MiB, `docs/NATIVE_OPERATIONS.md`, "Connection and TLS policy"). Whether the mesh announcement path adds a record-size check is Undetermined (searched `spine-agentic/src/naming_mesh.rs` handling of `NameAnnounce`, lines 938-955, and `spine-agentic/src/naming.rs` only by that call; `accept_announcement` itself was not read).

Quota accounting detail. The store counts serialized bytes (`resources.rs:49-64`). `Blob.data` is a plain `Vec<u8>` with no byte-string attribute (`resources.rs:12-16`) and is stored with `serde_json`, which writes an array of decimal integers. Each data byte therefore occupies between 2 and 4 bytes of stored value. The quota check reads every stored value in the namespace on each insert (`resources.rs:55-60`). Nothing deletes a stored resource: the store has no delete method (`resources.rs:18-97`).

## 3. Transfer protocol

> As of 2026-11-21 the native wire is spine-wire v1, see 00-wire.md (normative, sections 2 to 14; the earlier wire is its Appendix A); the following describes the earlier wire.
>
> Correction for this section: a request is now a command from a closed set of 19 in a REQUEST frame; the request id is in the 15-byte frame header, not in the body; a response is `{ok: ...}` or `{err: {code, message}}`; bodies are deterministic CBOR (no JSON format byte, no `SP` header). Byte fields travel as CBOR byte strings.

All commands are `BrowserCommand` variants (`spine-protocol/src/lib.rs:1024-1195`) carried in `Message::Request { id, command }` and answered by `Message::Response { id, result: Option<json>, error: Option<string> }` (`lib.rs:1276-1287`). The session loop is `main.rs:2419-2637`.

Message encoding: an 8-byte header (magic `SP`, version byte `1`, format byte, big-endian u32 payload length) followed by the body (`wire.rs:52-58`, `wire.rs:142-195`). Format `0x01` JSON, `0x02` CBOR, `0x03` CBOR with zstd (`wire.rs:90-94`). Maximum body 16 MiB (`wire.rs:60`).

### 3.1 Publish

Request: `PublishResource { record: json, content_type: string, data: bytes }` (`lib.rs:1175-1180`). `data` is a CBOR byte string, so an 8 MiB payload fits the frame (`resource.rs:20-41`).

Origin processing (`main.rs:630-659`), in order:

1. Deserialize `record` as `NameRecord`; failure is an error.
2. Refuse if `record.is_expired(now)` with `record is expired`.
3. `ResourceStore::put` (`resources.rs:31-67`): verify the record signature; refuse data over 8 MiB; refuse an invalid content type; require `record.content_hash` and check `verify_resource(id, content_type, data)` (invalid type: `InvalidContentType`; wrong bytes or type: `ResourceIdMismatch`); if a blob under that id already exists, return success (the id fixes the type, so there is nothing to ignore); otherwise check the 256 MiB quota and write the blob under namespace `spine-resources-v2`, key = the 32 id bytes.
4. `LocalResolver::publish` (`resolver.rs:195-248`): verify, compare against the stored record for the name, enforce capacity, persist the record, then replace the in-memory copy.
5. Replicate: if a mesh is present, `mesh.publish(record)` and report counts (`main.rs:395-421`).

Success response: a JSON object with `status: "published"`, `name`, `stored: true`, `local_persistent`, `replicas`, `replication_sent`, `replication_failed`, `replication_broadcast`, `replication_error`, `replication_acknowledged` (always 0), `durable` (always false) and a `retry` string (`main.rs:413-420`). Only the record is replicated to peers; resource bytes are not sent to them (Undetermined in the strict sense: `MeshNameResolver::publish` was not read, but the announcement payload is `NameAnnounce(AnnouncedRecord)` which carries a record, `spine-agentic/src/mesh.rs:216-220`).

Ordering consequence: step 3 completes before step 4. If step 4 fails (for example `StalePublication`), the blob remains stored and is not rolled back (`main.rs:640-641`).

Client side (`spine-agent/src/resources.rs:21-44`): verifies the record, checks size and that the record's hash equals the hash of the data, sends the command, and fails if `error` is set. No client deadline is applied in this function.

Who may publish: any client connected to the native origin. The handler performs no identity check beyond the record signature (`main.rs:630-659`, and `handle_session` applies only a rate limit, `main.rs:2521-2531`). The record's signer need not be the node's key. `docs/NATIVE_OPERATIONS.md` states that native origin clients share one trust boundary.

### 3.2 Download

Request: `ReadResource { hash: [u8;32], offset: u64, max_bytes: u32 }` (`lib.rs:1182-1186`). The request names the blob by hash only; no record or name is presented.

Origin processing (`main.rs:608-629`, `resources.rs:69-96`):

1. `max_bytes` must be in 1 to 65,536, else error `chunk must be 1 through 65536 bytes`.
2. Load the blob; absent gives `resource not held by this origin`.
3. Decode the stored JSON, require `data.len() <= 8 MiB` and `verify_resource(hash, stored content_type, data)`, else `stored resource failed integrity check` (a relabelled stored blob fails here). This runs on every chunk request, over the whole blob.
4. `offset` must be at most `data.len()`, else `offset beyond resource`. An offset equal to the length yields an empty final chunk.
5. Return `ResourceChunk { hash, offset, total_bytes, content_type, data, complete }` where `data` is up to `max_bytes` bytes from `offset` and `complete` is true when the chunk reaches the end (`spine-protocol/src/resource.rs:9-18`).

`ResourceChunk` is placed in `Response.result` with `serde_json::to_value` (`main.rs:616`), so `data` crosses the wire as a JSON array of integers inside the CBOR body, not as a CBOR byte string. This is read from the code; it was not measured.

### 3.3 Client verification

`read_resource` (`spine-agent/src/resources.rs:48-120`) requires a signed `NameRecord` with a `content_hash`, and refuses an expired record. It then loops, requesting `offset = bytes received so far` and `max_bytes = 65536`, each request under a 30 second timeout. For every chunk it enforces:

- `chunk.hash` equals the requested hash and `chunk.offset` equals the bytes received;
- `total_bytes <= 8 MiB` and chunk data `<= 64 KiB`;
- `total_bytes` is the same in every chunk;
- `content_type` is the same in every chunk;
- received bytes never exceed `total_bytes`;
- `complete` is true exactly when received bytes equal `total_bytes`;
- a non-final chunk carries at least one byte.

After the final chunk it requires `verify_resource(record.content_hash, content_type, data)`, so the peer-supplied content type is accepted only if it is the one the signed id commits to (`resource integrity check failed`, caused by `ResourceIdMismatch`). The gateway's `GET /v1/resources` goes through `read_resource` and sets `Content-Type` from the verified value. The result is `RetrievedResource { record, content_type, data }` (`resources.rs:13-18`).

`fetch_resource(name)` (`resources.rs:125-159`): resolve the name, verify the record signature and expiry, then try each endpoint in priority order whose `transport` is exactly `tcp`. Each attempt is a new plaintext TCP connection with a 10 second connect timeout, followed by `read_resource`. The first success returns; failures are collected into one error. No outbound address policy is applied to the endpoints (`docs/NATIVE_OPERATIONS.md`, "Configure outbound navigation": the policy does not govern signed mesh resource endpoints).

### 3.4 What is signed, and by whom

| Item | Signed | By | Verified by |
| --- | --- | --- | --- |
| Name-to-hash binding (`NameRecord`) | Yes | The `did:` key in the name | Origin on publish (`resources.rs:32`, `resolver.rs:196`); client on read and fetch |
| Content bytes | No signature; bound by the resource id in the signed `content_hash` | Not applicable | Client after the last chunk; origin on put and on every read |
| `content_type` | No signature of its own; committed to by the resource id in the signed `content_hash` (section 2.6) | Not applicable | Same checks as the bytes: origin on put and read, client after the last chunk, gateway on upload |
| `ResourceChunk` and `Response` | No | Not applicable | Integrity relies on the hash check only; the native connection is unauthenticated unless TLS or mTLS is configured |
| Link `title` and `capabilities` | No | Not applicable | Not verifiable |

### 3.5 Error cases

Errors are returned as `Response.error` text; there is no error code field. The strings produced by the code:

| Condition | Message source |
| --- | --- |
| Malformed or unsigned record | `serde` error, or `NameError::BadSignature` (`record.rs:269-282`) |
| Expired record | `record is expired` (`main.rs:637-639`) |
| Over 8 MiB | `resource exceeds 8 MiB` (`resources.rs:33`) |
| Invalid content type | `NameError::InvalidContentType` |
| Id mismatch (wrong bytes or content type) | `NameError::ResourceIdMismatch` |
| Quota | `resource store quota exceeded` (`resources.rs:61-64`) |
| Stale record | `StalePublication` via `NameError` (`resolver.rs:200`) |
| Bad chunk size | `chunk must be 1 through 65536 bytes` (`resources.rs:70-73`) |
| Unknown hash | `resource not held by this origin` (`resources.rs:74-77`) |
| Corrupt stored blob | `stored resource failed integrity check` (`resources.rs:79-82`) |
| Offset past end | `offset beyond resource` (`resources.rs:84`) |

A disconnected namespace is reported as `NameResolution::Unavailable`, distinct from `NotFound` (`spine-protocol/src/lib.rs:1199-1218`, `main.rs:500-503`). A confirmed miss is negative-cached for 30 seconds (`main.rs:495`, `spine-name/src/cache.rs:22`).

### 3.6 Resolution provenance (not content provenance)

`NameResolution::Resolved` carries `provenance: NameProvenance` with values `Cache`, `StaleCache`, `Local`, `Network`, `Address` (`lib.rs:1220-1238`). It records where a name record came from. It is a property of the answer to a lookup, not of the bytes, and it is not attached to resource chunks, parsed representations or tool results. `Address` marks an unsigned `host:` binding (`resolver.rs:305-311`).

## 4. Storage and restart behaviour

### 4.1 Interface

`StorageBackend` is a namespaced key-value interface: `get`, `put`, `delete`, `scan(namespace, prefix)`, `keys`, `batch_put`, `count`, `clear`, `exists` (`spine-storage/src/lib.rs:38-79`). Implementations: in-memory (`lib.rs:146-232`), SQLite (`lib.rs:250-306` and following), and a RocksDB variant (`lib.rs:480` onward; not used by `spine-core`).

SQLite: one table per namespace, `ns_` plus the namespace with non-alphanumeric characters replaced by `_`; columns `key BLOB PRIMARY KEY`, `value BLOB` (`lib.rs:278-306`). Pragmas: `journal_mode=WAL`, `synchronous=NORMAL` (`lib.rs:259-262`). `put` is `INSERT OR REPLACE` (`lib.rs:335`). One mutex-guarded connection serves all callers (`lib.rs:250-252`). Because the sanitiser maps different namespace strings to the same table name when they differ only in punctuation, such namespaces would collide; the namespaces actually used are listed below and do not.

### 4.2 What lives where

All three use one backend instance chosen at startup (`main.rs:1592-1628`): SQLite at `namespace.records_path` (default `spine-names.sqlite`, `config.rs:279`) or, when that path is empty, an in-memory backend that loses everything on exit.

| Namespace string | Contents | Writer |
| --- | --- | --- |
| `spine-name-publishers-v1` | Records published at this node, key = canonical name | `resolver.rs:233-235` |
| `spine-resources-v2` | `Blob` JSON, key = 32-byte resource id | `resources.rs:9`, `resources.rs:65` |
| `spine-tasks-v1`, `spine-task-knowledge-v1` | Layer 5 state | see `05-interaction.md` |

Session state is separate: a JSON manifest `spine-sessions-v1.json` written atomically with a 64 MiB limit (`docs/NATIVE_OPERATIONS.md`, "Monitor and stop"; code in `spine-core/src/session_store.rs`, not read in detail). It holds navigation sessions and per-session knowledge, not resources.

### 4.3 Restart

- Records: on startup `LocalResolver::persistent` scans `spine-name-publishers-v1`, rejects the whole snapshot if it holds more than 10,000 records, requires each key to equal the record's name, verifies every signature, and re-admits only records that are not expired (`resolver.rs:141-167`). An expired record stays in storage as a sequence marker so the owner can renew above it (`resolver.rs:137-139`).
- Resource bytes: read from storage on demand; there is no startup scan or index. Integrity is re-checked on each read (`resources.rs:79-82`). `resources.rs:106-143` is the test for restart, partial reads, and rejection of a tampered blob.
- Not persisted: learned cache entries, remote replicas, negative results and peer contacts (`resolver.rs:139`, `docs/AGENT_WEB_QUICKSTART.md`, "Publish and fetch a resource").
- Not-expired persisted records are re-offered to the mesh after startup (`main.rs:1619-1625`). Owner renewal runs only when `renew_owned_records` is set (`config.rs:110`, `namespace.rs`).
- Durability: with `synchronous=NORMAL` in WAL mode, SQLite may lose the most recent commits on power loss. This is SQLite behaviour for those settings, not something the repository tests.

## 5. Typed links

`Link { rel, target, title: Option<string>, capabilities: list of string }` (`link.rs:93-107`).

`Rel` values, wire spelling lower-case snake_case (`link.rs:15-70`):

| `Rel` | Wire | Default traversal priority (lower first) |
| --- | --- | --- |
| `Requires` | `requires` | 0 |
| `Provides` | `provides` | 1 |
| `Child` | `child` | 2 |
| `Snapshot` | `snapshot` | 3 |
| `Alternate` | `alternate` | 4 |
| `Peer` | `peer` | 5 |
| `Parent` | `parent` | 6 |
| `Describes` | `describes` | 7 |
| `Other(string)` | the string, lower-cased on parse | 8 |

Priorities: `link.rs:77-89`. Parsing is case-insensitive; unknown names become `Other` (`link.rs:58-70`). Semantics are given only by comments (`link.rs:18-36`); no code enforces them. For instance, nothing checks that a `Snapshot` target is a `blob:` name equal to the source's `content_hash`.

Use of links as built:

- `CrawlNames { seed, max_depth, max_visits }` resolves names through the node's local resolver only (`main.rs:880`, not the mesh), expands each record's links into a `CrawlFrontier`, and reports per visit `name`, `depth`, `capabilities`, and the count of links, or `unresolved: true` (`main.rs:861-896`). The frontier orders by depth, then relation priority, then insertion order, de-duplicates by routing key, and records why candidates were skipped (`frontier.rs:9-15`, `frontier.rs:100-124`). The `follow` filter exists in `CrawlBudget` (`frontier.rs:65`) but `CrawlNames` does not set it (`main.rs:868-874`).
- `UnifiedRepresentation::agent_links` turns parsed `AgentLink` elements into `Link` values (section 6.3).

## 6. What the semantic parser emits

Entry point: `spine_parser::parse_html(html: &str) -> anyhow::Result<UnifiedRepresentation>` (`spine-parser/src/lib.rs:142-166`). It uses the `scraper` crate's HTML5 parser and always returns `Ok` as written.

### 6.1 `UnifiedRepresentation`

| Field | Type | Value as built |
| --- | --- | --- |
| `title` | string | Text of the first `title` element, or the literal `No Title` (`lib.rs:146-150`) |
| `elements` | list of `Element` | One entry per child of `body` that yields an element (`lib.rs:152-159`). A document with no `body` yields an empty list. |
| `metadata` | map string to string | Always empty; nothing populates it (`lib.rs:161-165`) |

Serialization: `serde` derive (`lib.rs:30`), so externally tagged enums when written as JSON. The `spine.html/parse` tool returns `serde_json::to_value` of this struct (`spine-core/src/tools.rs:35-39`). `GetUR` returns `{parsed_ur, vdom_tree, vdom_ur}` (`main.rs:969-977`).

### 6.2 `Element` variants

(`lib.rs:37-85`)

| Variant | Fields | Produced from |
| --- | --- | --- |
| `Text` | string | A text node, trimmed; empty text is dropped (`lib.rs:188-195`) |
| `Heading` | `level: u8`, `text` | `h1` to `h6`; text is the descendant text joined by single spaces (`lib.rs:199-203`, `lib.rs:170-184`) |
| `Link` | `text`, `url` | `a` whose `href` does not parse as a `spine://` name; `url` is the raw `href`, empty if absent (`lib.rs:204-226`) |
| `AgentLink` | `text`, `target`, `rel`, `capabilities` | `a` whose `href` parses as a `spine://` name. `target` is the canonical name. `rel` is the `rel` attribute normalised through `Rel::parse`, default `peer`. `capabilities` come from `data-capabilities`, split on commas and spaces, lower-cased (`lib.rs:8-14`, `lib.rs:211-223`) |
| `Button` | `text`, `action_id` | `button`; `action_id` is the `id`, else `name`, else `btn_` plus the lower-cased text with spaces replaced by `_` (`lib.rs:227-237`) |
| `Input` | `label`, `input_type`, `id` | `input`; label is `placeholder`, else `name`, else `input`; type defaults to `text`; id is `id`, else `name`, else `unknown` (`lib.rs:261-278`) |
| `Image` | `alt`, `src` | `img`; empty strings when absent (`lib.rs:238-242`) |
| `List` | `items: list of Element`, `ordered: bool` | `ul` and `ol`; only direct `li` children, and the `li` children are flattened into `items` (`lib.rs:243-260`) |
| `Container` | `tag`, `children` | Any other element that has at least one parsed child; a childless unknown element is dropped (`lib.rs:279-294`) |

Observed consequences of the code, for a reader relying on the output:

- `script` and `style` elements are not special-cased, so their text appears as `Text` inside a `Container { tag: "script" }` or `"style"` (`lib.rs:279-294`). Whether the `scraper` crate excludes such text from `Node::Text` was not checked.
- Nothing distinguishes hidden content (for example elements hidden by attributes or CSS) from visible content. The parser reads no `style`, `hidden` or `aria` attributes.
- There is no depth limit in `parse_node`; recursion depth follows document nesting. Bounds on input size come only from the callers (1 MiB for the tool, `tools.rs:26`; 2 MiB default fetch, `docs/NATIVE_OPERATIONS.md`).
- No field records the source of the HTML, whether it was signed, or any provenance.

### 6.3 Derived links

`UnifiedRepresentation::agent_links` walks the whole tree (including lists and containers) and returns a `Link` per `AgentLink`: relation from `Rel::parse`, title set to `text` when non-empty, capabilities copied, targets that fail to parse skipped (`lib.rs:95-140`). `has_agent_links` is true when that list is non-empty.

### 6.4 Schema-driven extraction (library only)

`spine-parser/src/extraction.rs` defines a second output family. No call site outside the crate was found by searching `src/` for `extraction::`, so it is not reachable from the origin, the tools or the SDK as built.

- `ExtractionSchema { name, fields: list of FieldDef }`; `FieldDef { name, selector (CSS), field_type, required, attribute: Option, transforms }` (`extraction.rs:13-33`).
- `FieldType`: `Text`, `Integer`, `Float`, `Boolean`, `Url`, `List(FieldType)`, `Record(list of FieldDef)` (`extraction.rs:37-47`).
- `Transform`: `Trim`, `Lowercase`, `Uppercase`, `RegexCapture(string)` (`extraction.rs:51-57`).
- `DataValue`: `Text`, `Integer(i64)`, `Float(f64)`, `Boolean`, `Url`, `List`, `Record(map)`, `Null` (`extraction.rs:61-70`).
- `ExtractedData { schema_name, fields: map to DataValue, warnings: list of string }` (`extraction.rs:74-79`).
- `ExtractionError`: `SchemaNotFound`, `InvalidSelector`, `RequiredFieldMissing` (`extraction.rs:83-90`).
- `SchemaRegistry::register`, `list_schemas`, `get_schema`, `extract(html, schema_name)` (`extraction.rs:94-116`).

Schemas are identified by a free-form `name` string held in an in-process registry. They are not content-addressed and not published as objects.

## 7. Gaps against the target design

Target: `docs/FIRST_PRINCIPLES.md` line 42. Each item states the as-built position.

- [ ] **Schema-identified typed objects.** Not present. A resource is bytes plus a `content_type` that is bound into the resource id (section 2.6) but is otherwise a free-text label. `NameRecord` has no schema or type identifier field (`record.rs:53-85`). Extraction schemas are named by a local string and are not part of any record (section 6.4).
- [ ] **Object graph.** Partial. Records carry typed `links` (section 5) and the frontier can traverse them, but links are unauthenticated hints in two fields, the relation set is closed except for `Other`, no relation is checked against content, and `CrawlNames` reads only the local store.
- [ ] **Provenance label carried to the caller.** Absent. `NameProvenance` describes a lookup, not content (section 3.6). `ResourceChunk`, `RetrievedResource`, `UnifiedRepresentation`, `GetUR` and the `spine.html/parse` result carry no source name, signer, signed-or-unsigned flag or fetch origin (`resource.rs:9-18`, `resources.rs:13-18`, `lib.rs:30-35`, `main.rs:969-977`). `Navigate` stores fetched HTML in the session and returns only `status` and `session_id` (`main.rs:943-948`).
- [ ] **Declared token size.** Absent. The only declared size is `total_bytes` in bytes, delivered inside chunks after the first request (`resource.rs:13`). The record has no size field and no token estimate.
- [ ] **Budgeted representations.** Absent. A resource has one representation; `ReadResource` has no budget parameter. `Rel::Alternate` exists as a link relation (`link.rs:29-30`) but nothing selects among alternates by size.
- [ ] **Data cannot instruct (typing).** Absent. The parser emits text, links and `Button` and `Input` elements as ordinary data (`lib.rs:37-85`), with no marker that the content is untrusted data and no span-level source tag. Tool results are untyped JSON (`05-interaction.md`, section 2). Nothing in the type system separates reader-addressed instructions from content.
- [ ] **Streaming beyond 8 MiB.** Absent. 8 MiB is a hard cap enforced at publish, store and client (`resource.rs:6`, `resources.rs:33`, `resources.rs:80`, `spine-agent/src/resources.rs:81`). Ranged chunking exists, but the whole blob is stored as one value, parsed from JSON and re-hashed in full for every 64 KiB request (`resources.rs:74-82`), so cost grows with size times chunk count.

Further defects and unsafe properties found while reading (not in the requested checklist):

- [ ] **Unauthenticated publication.** Any native client can publish a name and bytes signed by any key it holds, and can fill the 256 MiB store, with no way to remove the data afterwards (section 2.5, section 3.1).
- [ ] **Orphaned blobs.** A failed name publication leaves its blob stored (section 3.1).
- [x] **Unsigned `content_type`.** Resolved by the resource id (section 2.6): a different type is a different id, and every fetch path verifies it.
- [ ] **No bounds on record fields** (section 2.5).
- [ ] **Unsigned link metadata** (section 2.3).
- [ ] **Client dials publisher-chosen addresses over plaintext TCP** with no outbound policy (section 3.3).
- [ ] **Resource bytes are not replicated.** The mesh carries records; a record's endpoints point at origins that must stay up (section 3.1).
- [ ] **No blob resolution.** `blob:` names cannot be resolved to bytes by the resolver as built (section 2.2).

## 8. Undetermined

| Item | Files searched |
| --- | --- |
| Whether any code resolves a `blob:` name | `spine-name/src/resolver.rs`, `spine-core/src/main.rs` |
| Size limit applied to `NameRecord` received by mesh announcement | `spine-agentic/src/naming_mesh.rs` (lines 938-955); `accept_announcement` in `spine-agentic/src/naming.rs` not read |
| Whether `MeshNameResolver::publish` transmits anything other than the record | `spine-agentic/src/mesh.rs:216-220` (payload type only) |
| Whether `scraper` yields `script` and `style` text as text nodes | Not checked; no vendored source read |
| Wire size of a `ResourceChunk` response | Derived from `main.rs:616`; not measured |
| Contents and limits of `session_store.rs` | Referenced through `docs/NATIVE_OPERATIONS.md` only |
