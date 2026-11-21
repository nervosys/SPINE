# Layer 2: Names (as built)

Status: description of the code on branch `completion/agent-web`, read 2026-11-20. It states what the code does, not what it should do. Nothing was executed to produce it; every behaviour below was determined by reading the cited source. Where reading did not settle a question the text says "Undetermined". Section 8 is the only place that discusses the target design.

Primary sources: `src/spine-name/src/` (uri.rs, base32.rs, key.rs, record.rs, link.rs, store.rs, cache.rs, resolver.rs, routing.rs, frontier.rs), `src/spine-agentic/src/naming.rs`, `src/spine-agentic/src/naming_mesh.rs`, the naming message builders in `src/spine-agentic/src/mesh.rs`, and the node glue in `src/spine-core/src/main.rs` and `src/spine-core/src/namespace.rs`.

## 1. Scope and terminology

In scope: the `spine://` name syntax, the `NameRecord` object and its signature, the record store and caches, the Kademlia-style lookup and the messages it exchanges, and publication and renewal. Out of scope: the session layer that carries the messages (layer 1) and authority (layer 3, absent).

| Term | Meaning in this document |
| --- | --- |
| name | A parsed `spine://` URI (`SpineUri`, uri.rs:141). |
| authority | The part after `spine://` up to the first `/`. One of four kinds: `did`, `blob`, `cap`, `host` (uri.rs:36). |
| origin | A name reduced to its authority and path `/` (uri.rs:319). |
| record | A `NameRecord` (record.rs:53). |
| routing key | `NameKey`, a 256-bit value; names and nodes share one keyspace (key.rs:18). |
| node key | A node's Ed25519 public key used directly as its keyspace position (mesh.rs:630). |
| store | `RecordStore`, a node's verified record collection (store.rs:43). |
| walk / lookup | An iterative Kademlia query (`Lookup`, naming.rs:214). |

All integers in the signed encoding are big-endian. "Hash" means SHA-256 (sha2 crate) everywhere.

## 2. Name syntax

### 2.1 Overall form

`spine://` authority [`/` path] [`?` query] [`#` fragment]. Parsing is `SpineUri::parse` (uri.rs:183-232).

Parse steps, in order:

1. Trim ASCII/Unicode whitespace from both ends of the whole input (uri.rs:184).
2. Require the 8-byte prefix `spine://`, compared case-insensitively. Otherwise `NotSpineScheme` (uri.rs:185-192).
3. Empty remainder is `InvalidAuthority("")` (uri.rs:194).
4. Split at the first `#`; the right side is the fragment (uri.rs:199). Then split the left side at the first `?`; the right side is the raw query (uri.rs:203). Then split at the first `/`; left is the authority, right (including the `/`) is the path; no `/` means path `/` (uri.rs:207-210).
5. Parse the authority (section 2.2), normalise the path (section 2.3), parse and sort the query (section 2.4). An empty fragment is dropped (uri.rs:230).

No percent-decoding or percent-encoding is performed on any component. Non-ASCII characters are accepted in path, query and fragment and are kept as is.

Canonical string form (`Display`, uri.rs:421-447): `spine://` + canonical authority + path, then `?` and `k=v` pairs joined by `&` (a pair with empty value prints as bare `k`), then `#fragment` if present. `FromStr`, serde `Serialize` (canonical string) and `Deserialize` (parses and rejects malformed input) are in uri.rs:450-470.

### 2.2 Authority kinds

The authority is split at the first `:` into tag and rest. The tag is lowercased before matching (uri.rs:79-83). No colon gives `InvalidAuthority`. An unknown tag gives `UnknownAuthorityKind` (uri.rs:123). The kinds:

| Kind | Grammar of `rest` | Canonical form | Source | Rejected |
| --- | --- | --- | --- | --- |
| `did` | base32 (2.5) decoding to exactly 32 bytes; an Ed25519 public key | `did:` + 52 lowercase symbols | uri.rs:84-86, 68 | any decode failure or length other than 32 bytes: `InvalidAuthority` |
| `blob` | same as `did`; a SHA-256 hash | `blob:` + 52 lowercase symbols | uri.rs:87-89, 69 | same |
| `cap` | trim, then lowercase; any string that is non-empty and contains no whitespace and no `/` | `cap:` + term | uri.rs:90-96, 70 | empty; contains whitespace or `/` |
| `host` | trim; non-empty. If the text after the last `:` is non-empty and all ASCII digits, it is a port (`u16`, must parse) and the part before it must be non-empty. Otherwise the whole text is the host. Host is lowercased | `host:` + host, plus `:port` when present | uri.rs:97-122, 71-74 | empty; port not fitting `u16`; empty host before a port |

Notes derived from the code:

- A `cap` term has no further syntax: no check for dots, character set or length (uri.rs:91-95). `Capability(t)` stores whatever passed. `SpineUri::capability` lowercases but does not validate (uri.rs:178-180).
- A `host` is not validated as a hostname or address. Any non-whitespace-trimmed text is accepted, including spaces inside it and characters that would not form a valid host. Only the final `:digits` suffix is interpreted.
- Because the host splits on the last colon, `host:fe80::1` parses as host `fe80:` with port 1, not as an IPv6 literal. The comment at uri.rs:102 claims bracketed IPv6 works; `[::1]:9440` does (host `[::1]`, port 9440) but a bare IPv6 address does not.
- `did` and `blob` have identical syntax and differ only in meaning. `is_self_certifying` is true for both; `is_immutable` is true for `blob` only (uri.rs:55-63).
- Kind semantics: `did` can sign records. `blob` can never have a record (section 3.2). `cap` names a set and cannot be resolved as a single name (resolver.rs:299). `host` resolves to a synthesised unsigned record (section 4.1).

### 2.3 Path normalisation

`normalize_path` (uri.rs:396-419). Split on `/`. Empty and `.` segments are dropped. `..` pops one segment if any (so it cannot climb above the root). A trailing `/` in the input is preserved if at least one segment remains. The result is `/` joined to each segment; if empty, `/`. Path comparison is case-sensitive. Test: `/a//b/./c` and `/a/b/x/../c` both become `/a/b/c` (uri.rs:557-564); `/../../etc/passwd` becomes `/etc/passwd` (uri.rs:567-571).

Consequence of preserving the trailing slash: `/a` and `/a/` are different paths, therefore different routing keys (section 2.6).

### 2.4 Query and fragment

Query: split on `&`, drop empty pairs, split each at the first `=`; no `=` gives an empty value. The pairs are sorted lexicographically by (key, value), so order is not significant (uri.rs:215-225). Duplicates are kept. The query does not take part in the routing key. The fragment does not either (uri.rs:257-261, 323-335).

### 2.5 Base32 variant

RFC 4648 section 6 alphabet, lowercase, no padding (base32.rs:11, `abcdefghijklmnopqrstuvwxyz234567`).

- Encode (base32.rs:14-41): 5 input bytes map to 8 symbols, most significant bits first. A final chunk of 1, 2, 3, 4 bytes emits 2, 4, 5, 7 symbols. 32 bytes encode to 52 symbols (base32.rs:108-113).
- Decode (base32.rs:45-68): accepts upper and lower case (symbol_value, base32.rs:81-88). Rejects any other character and any chunk length of 1, 3 or 6 symbols (final chunk). `decode_key` additionally requires exactly 32 bytes (base32.rs:71-79).
- Non-canonical trailing bits are not checked. The final symbol of a 52-symbol string carries 4 bits beyond the 256 data bits, because 52 x 5 = 260. `decode` shifts those bits out without testing that they are zero (base32.rs:51-65). Therefore several 52-symbol strings can decode to the same key. For names this is harmless to equality because parsing re-encodes to the canonical string, but a second implementation that rejects non-zero trailing bits would reject names this one accepts. The same decoder is used for signatures in JSON (record.rs:323-335).
- The same encoding appears for `NameKey` in JSON (key.rs:92-105).

### 2.6 Routing key

`SpineUri::key()` (uri.rs:328-335) is SHA-256 over the UTF-8 bytes of the string `spine://` + canonical authority + normalised path. Query and fragment are excluded. Two names that differ only in query or fragment have the same key and are the same slot in every store (store.rs:150, 116).

Capability routing key (different derivation): SHA-256 over the UTF-8 string `cap:` + lowercase term, with no `spine://` prefix and no trailing `/` (naming.rs:80-82). This differs from `SpineUri::capability(term).key()`, which hashes `spine://cap:<term>/`.

### 2.7 Joining references

`SpineUri::join` (uri.rs:341-391) resolves a reference like an href. Absolute `spine://` references are re-parsed. A rooted reference replaces the path. Anything else is resolved against the base path's directory (up to and including its last `/`). The reference's own query and fragment replace the base's. Empty reference returns the base.

### 2.8 Defect: parser can panic on non-ASCII input (by reading, not executed)

`SpineUri::parse` falls back to `trimmed.split_at(trimmed.len().min(8))` when `strip_prefix` fails (uri.rs:187-191). `str::split_at` panics if the index is not a char boundary. Input whose byte 8 falls inside a multi-byte character (for example seven ASCII bytes followed by a three-byte character) reaches that call and, by the standard library's contract, panics. `join` slices `reference[..SCHEME.len()]` after only a length check (uri.rs:346), with the same hazard. `parse` is reached with client-supplied strings at `resolve_one` (main.rs:442) and when deserialising any `SpineUri` from the wire (uri.rs:465-470), including inside records and responses from peers. No test covers non-ASCII input. This should be treated as a remotely triggerable panic until a test proves otherwise.

## 3. NameRecord

### 3.1 Fields

Struct `NameRecord` (record.rs:53-85).

| Field | Type | Signed | Notes |
| --- | --- | --- | --- |
| `name` | `SpineUri` | yes (canonical string, including query and fragment) | Must have a `did` authority to be created or verified. May carry a path, query and fragment. |
| `seq` | `u64` | yes | Version counter. |
| `ttl_secs` | `u32` | yes | Default 3600 (record.rs:22). 0 is accepted and means expired at publication. No maximum. |
| `published_at` | `u64` | yes | Unix seconds. Set by the publisher. No bound against the clock is checked anywhere in spine-name. |
| `endpoints` | `Vec<Endpoint>` | yes | Each `{transport: String, address: String, priority: u8}`. `transport` and `address` are free text (documented as tcp, ws, wss, quic, grpc but not enforced). |
| `capabilities` | `Vec<String>` | yes | Free text terms. `with_capability` lowercases (record.rs:174); deserialisation does not. |
| `content_hash` | `Option<[u8; 32]>` | yes | SHA-256 of the current representation. |
| `links` | `Vec<Link>` | partly | Only `rel` and `target` of each link are signed (3.3). `Link.title` and `Link.capabilities` are NOT signed (link.rs:99-106). |
| `meta` | `BTreeMap<String, String>` | yes | Sorted by key in the encoding. |
| `signature` | `Vec<u8>` | n/a | 64 bytes of Ed25519 signature; JSON form is unpadded lowercase base32 (record.rs:322-335). |

`Rel` is `child, parent, peer, provides, requires, alternate, describes, snapshot` or `Other(String)`; wire strings are the lowercase names, `Other` carries its own string (link.rs:17-70). There is no length or count limit on any field in spine-name. The only size bounds met on the path are the transport frame limit of 4 MiB (mesh_tcp.rs:62) and, at the origin, the JSON command envelope; whether a record is rejected for size before parsing is Undetermined (not found in spine-name or naming*.rs).

Serialisation of a record for storage and transport is serde JSON (naming_mesh payloads are JSON envelopes, mesh_tcp.rs:164; persistence is `serde_json::to_vec`, resolver.rs:231). Within it `content_hash` serialises as an array of 32 integers (no custom serde on that field, record.rs:73-74) and `signature` as a base32 string. This JSON form is not canonical and is not what is signed.

### 3.2 Which names can have records

`NameRecord::new` fails with `NotSignable` unless the authority is `did` (record.rs:90-93). `verify` fails with `NotSignable` for any other authority (record.rs:269-273). `RecordStore::put` verifies first (store.rs:113). Therefore a `blob:` name, a `cap:` name and a `host:` name can never be stored, cached via `cache_record`, or resolved from a store. `blob:` names are only content addresses; this layer has no operation that turns a `blob:` name into bytes or into a record. `host:` records are synthesised (4.1).

### 3.3 Canonical signed bytes

`signing_bytes` (record.rs:200-248). The byte string is the concatenation, in this order:

| # | Bytes |
| --- | --- |
| 1 | ASCII `spine-name-record-v1` followed by one `0x00` (21 bytes) |
| 2 | UTF-8 of `name.to_string()` (canonical, with query and fragment), then `0x00` |
| 3 | `seq` as 8 bytes |
| 4 | `ttl_secs` as 4 bytes |
| 5 | `published_at` as 8 bytes |
| 6 | endpoint count as u32 (4 bytes); then for each endpoint in stored order: `transport` UTF-8, `0x00`, `address` UTF-8, `0x00`, `priority` (1 byte) |
| 7 | capability count as u32; then for each in stored order: UTF-8, `0x00` |
| 8 | `content_hash`: `0x01` then 32 bytes, or `0x00` alone |
| 9 | link count as u32; then for each in stored order: `rel` wire string, `0x00`, `target.to_string()`, `0x00` |
| 10 | meta count as u32; then for each in key order (BTreeMap, byte-wise order of the key string): key, `0x00`, value, `0x00` |

Properties a second implementation must reproduce or knowingly diverge from:

- Endpoints, capabilities and links are signed in stored order, so reordering them invalidates the signature. Meta is order-independent (test record.rs:494-503).
- Strings are not length-prefixed; they are NUL-terminated. A string that itself contains `0x00` (nothing in the code forbids it: `cap`, `host`, path and query accept it, and endpoint, capability and meta strings are unrestricted) can shift field boundaries within a section. Counts are prefixed, which limits but does not remove the ambiguity. Whether a concrete collision exists was not constructed.
- Unsigned: `Link.title`, `Link.capabilities`. A relaying node can alter these without invalidating the signature.
- The `name` is signed in canonical string form, so two spellings of one name sign identically. The signature does not cover the routing key separately; it is derived from the name.

### 3.4 Signature algorithm and verification

- Algorithm: Ed25519 (ed25519_dalek) over `signing_bytes`, no prehash, no context string (record.rs:262).
- Signing key must match the key in the name, else `KeyMismatch` (record.rs:254-261).
- Verification (record.rs:269-282): signature length must be 64; the 32 bytes in the name must be a valid curve point (`VerifyingKey::from_bytes`); then `Verifier::verify`. The code uses `verify`, not `verify_strict`, so the strict-verification restrictions on small-order keys and malleable signatures are not applied. Impact on this layer: a publisher can choose such a key for its own name; the record only ever claims that name.
- Verification is performed at every ingress: `RecordStore::put` (store.rs:113), `LocalResolver::publish` and `cache_record` (resolver.rs:196, 252), `Lookup::on_response` for records and providers (naming.rs:320, 332), `announce_name` and `announce_name_to` before sending (mesh.rs:644, 662), and when restoring from disk (resolver.rs:160).

### 3.5 Validity and expiry

- `expires_at = published_at + ttl_secs`, saturating (record.rs:285). `is_expired(now)` is `now >= expires_at` (record.rs:290). A record is fresh for `published_at <= t < expires_at`; there is no lower bound, so a record with `published_at` in the future is treated as fresh now.
- Signature validity does not depend on time. `verify` does not check expiry. Stores accept expired records (`put` does not test expiry, store.rs:112-145). Expiry is applied at read time: `get_fresh`, `providers_of`, `handle_request` (naming.rs:559), and by sweeping (store.rs:197).
- Publication at the origin rejects an already-expired record (main.rs:796), but `LocalResolver::publish` itself does not.
- Clock: injected function returning Unix seconds (resolver.rs:121, naming_mesh.rs:341). No skew tolerance exists.

### 3.6 Ordering and conflict resolution

`supersedes` (record.rs:304-306): `self` replaces `other` iff `(seq, published_at)` of self is lexicographically greater than that of other. Records are compared only within one routing key (same authority and path; query and fragment ignored).

- `RecordStore::put` (store.rs:116-124): an existing record for the key is replaced only if the new one supersedes it. Otherwise the outcome is `Superseded` (not an error) and the stored record is kept. Note the enum comment says "newer or equal".
- `LocalResolver::publish` (resolver.rs:198-202): if a different record is offered that does not supersede the stored one, it returns `StalePublication`. Offering the identical record again is accepted as a no-op.
- Equal `(seq, published_at)` with different content: neither supersedes the other, so each store keeps whichever it received first. Two replicas can then hold different records for one name permanently. The doc comment at record.rs:301-303 says replicas converge regardless of arrival order; that holds only when the pairs differ. A publisher that signs two records with the same seq and published_at (equivocation) is not detected or penalised anywhere.
- `seq` is a bare counter chosen by the signer. There is no chain, no hash link to the previous record, and no requirement that seq increases by one. A record with a high seq and large `ttl_secs` can only be replaced by a higher seq signed by the same key.
- `renewed` (record.rs:156-167): verifies the old record, increments seq by one (checked; at `u64::MAX` returns `SequenceExhausted`), sets `published_at = now` and `ttl_secs`, re-signs. Contents are otherwise unchanged.
- The resolver cache does not compare seq. `ResolverCache::put` overwrites any entry for the key (cache.rs:140-154) and `cache_record` calls it after only a signature check (resolver.rs:251-255). A node that learns an older valid record after a newer one can therefore cache the older one.

## 4. Resolution

### 4.1 Local algorithm (`LocalResolver::resolve`, resolver.rs:296-333)

1. `cap` authority: error `NotASingleName`.
2. `host` authority: build an unsigned record from the name itself: sequence 0, ttl from `DEFAULT_TTL_SECS`, one endpoint `("tcp", host[:port])`, return provenance `Address` (record.rs:120-140). Never cached. `is_attested` is false.
3. Cache lookup:
   - Fresh hit: provenance `Cache`.
   - Negative hit: error `NotFound`.
   - Stale hit: if the store holds a fresh record, return it with `Local`; otherwise return the stale record with `StaleCache`.
   - Miss: continue.
4. Store: fresh record gives `Local`; otherwise error `NotFound`.

`blob` names go through the same path and, since no record can exist for them, always end in `NotFound` locally (the cache never holds them either, since `cache_record` verifies). The cache's "immutable entries never expire" branch (cache.rs:119) is therefore unreachable for `blob` names in practice.

Provenance values: `Cache`, `StaleCache`, `Local`, `Network`, `Address` (resolver.rs:27-45). `Network` is attached at the origin layer, not inside spine-name (main.rs:484).

`find_providers(term)`: store only; fresh records whose capability set contains the lowercased term, sorted by endpoint count (descending), then `published_at` (descending), then name string (store.rs:168-186). The resolver cache is not consulted.

### 4.2 Network lookup parameters

| Parameter | Value | Source |
| --- | --- | --- |
| Keyspace | 256 bit; names hashed per 2.6, capabilities per the second paragraph of 2.6; nodes use their Ed25519 public key unhashed | key.rs:22, mesh.rs:630 |
| Distance | XOR of the two keys, compared as big-endian unsigned (derived `Ord`) | key.rs:40-46, 70-71 |
| Bucket index | 255 minus the number of leading zero bits of the XOR distance | key.rs:51-54 |
| k (routing table bucket width) | 20 (`DEFAULT_K`) | routing.rs:17 |
| K (shortlist size, max providers returned, max nodes referred) | 20 | naming.rs:35 |
| alpha (queries per wave) | 3 | naming.rs:32 |
| Maximum waves per lookup | 16 (`MAX_ROUNDS`); a wave is issued on every response as well as at start, so the bound on distinct peers queried is 16 x 3 = 48 | naming.rs:39, 279-310 |
| Overall lookup deadline | 10 s (`DEFAULT_LOOKUP_TIMEOUT`), one deadline for the whole lookup, none per peer | naming_mesh.rs:64, 768 |
| Routing table size | unbounded number of buckets (up to 256), at most 20 nodes each | routing.rs:45-51 |

Routing table insertion (routing.rs:89-122): a known node is refreshed if the new `last_seen` is not older. When a bucket is full a newcomer replaces the entry with the smallest `last_seen` only if the newcomer's `last_seen` is strictly greater. `last_seen` is a value supplied by whoever described the node (see section 8.8).

### 4.3 Messages

Payload variants carried in a signed `MeshEnvelope` (mesh.rs:220-224 and builders mesh.rs:640-724). The envelope signature is over `id || from || serde_json(payload)` (mesh.rs:916-924); envelope and transport authentication belong to layer 1 and are not specified here.

| Message | Fields | Direction and use |
| --- | --- | --- |
| `NameResolveRequest` | `request_id: u64`, `query` | Directed to one peer (`MeshTarget::Agent`). |
| `ResolveQuery` | `Name(SpineUri)`, `Capability(String)`, `Node(NameKey)` | Target key per 2.6 for names, capability key for capabilities, the key itself for `Node` (naming.rs:77-85). |
| `NameResolveResponse` | `request_id`, `record: Option<NameRecord>`, `providers: Vec<NameRecord>` (default empty), `closer: Vec<KeyspacePeer>` (default empty) | Sent back on the connection the request arrived on when the transport supports it (naming_mesh.rs:964-971). |
| `KeyspacePeer` | `info: NodeInfo {id: NameKey, endpoints: Vec<String>, last_seen: u64}`, `agent_id: AgentId` | Referral: a keyspace position plus the mesh identity needed to address it (naming.rs:109). |
| `NameAnnounce` | `AnnouncedRecord { record }` | Directed to a peer by replication, or broadcast as a fallback (mesh.rs:640-667). No reply is defined. |
| `NameHello` | `public_key: [u8;32]`, `endpoints` | Broadcast envelope to an address with unknown identity (mesh.rs:699). |
| `NameHelloAck` | `public_key`, `endpoints`, `closer: Vec<KeyspacePeer>` | Reply to hello (mesh.rs:710). |

Responder behaviour (`NameService::handle_request`, naming.rs:553-584): for `Name`, return the stored record if fresh (`get_fresh`); for `Capability`, return up to K fresh providers; for `Node`, nothing. In every case also return `closer`: up to K nodes from the routing table nearest the target, filtered to those strictly closer to the target than the responder itself, and only those whose mesh identity is known (`as_keyspace_peers`, naming.rs:527). An answer is a response with a record or at least one provider (naming.rs:152).

### 4.4 Lookup state machine (`Lookup`, naming.rs:214-421)

- Seeds: the up to K routing-table nodes nearest the target (naming.rs:704). For a `Capability` query the local store's providers are also fed in as a pseudo-response (naming.rs:706-715).
- `next_wave`: up to 3 not-yet-queried shortlist nodes, nearest first; each is marked queried at once. Returns empty if finished, if 16 waves have been issued, or if the shortlist has no unqueried node. In the last case the lookup is exhausted only when nothing is in flight (naming.rs:279-310).
- `on_response`: a returned record counts only if `record.verify()` passes and `record.name.key()` equals the queried key (naming.rs:319-329, 391-394). Providers count only if they verify and advertise the term (`has_capability`) and are not already present by name (naming.rs:331-338). All `closer` entries are merged into the shortlist, which is sorted by distance and truncated to K (naming.rs:340, 409-420).
- Completion: a `Name` lookup completes on the first valid record. A `Capability` lookup completes at 20 providers or when exhausted. A `Node` lookup never completes early; its result is the converged shortlist (naming.rs:358-372).
- Outcome: `Found(record)`, `Providers(list)`, `Closest(nodes)`, or `NotFound` (naming.rs:375-389).
- Not checked: that a returned record is unexpired, that it carries the highest seq among all replicas, or that a referred node is genuinely nearer than the referrer.

Driver (`MeshNameResolver::run_lookup`, naming_mesh.rs:738-785): first tries local resolution for `Name` and `Node` (skipped for `Capability`). Otherwise starts the lookup, dispatches each wave concurrently, and waits up to the timeout. A peer with no known mesh identity, or a failed send, is marked unreachable, removed from the routing table, and the lookup continues with a follow-up wave (naming_mesh.rs:810-851). If nothing was delivered and nothing remains, the lookup settles at once as `NotFound` (naming_mesh.rs:855-857). There is no retry of a request to the same peer and no per-peer timeout: a peer that accepts the send and never answers holds the lookup until the 10 s deadline.

### 4.5 Local naming-mesh node behaviour

- `is_responsible_for(key)`: true if fewer than K known nodes are closer to the key than this node (naming.rs:792-800). With an empty routing table this is true for every key.
- Announcements: `accept_announcement` verifies, then stores only if responsible; otherwise returns `Rejected` (naming.rs:488-496). The handler counts accepted, declined (rejected for proximity or capacity) and rejected (error, for example a bad signature) (naming_mesh.rs:938-955).
- Store capacity 10,000 records (`DEFAULT_CAPACITY`, store.rs:39). When full, the record whose key is furthest from the node's own key is evicted, and an incoming record that is itself furthest is refused. Without a home key, the oldest-published is evicted (store.rs:89-145).
- Own publication through `NameService::publish` is stored without a proximity test (naming.rs:469-474) but subject to the capacity rule.

### 4.6 Caching and negative caching

- Resolver cache: at most 4096 positive entries (resolver.rs:124); eviction removes the oldest-inserted mutable entry first, then oldest of any (cache.rs:216-233). The negative map is not bounded by capacity in the code read (cache.rs:88, 157-160); `sweep` removes expired entries.
- Negative TTL: 30 s (cache.rs:22). `put` clears the negative entry for that key (cache.rs:143). `LocalResolver::publish` removes both positive and negative entries for the name (resolver.rs:238, 246).
- Positive TTL: the record's own `expires_at`. A stale entry is returned as `StaleCache` only if no fresh stored record exists (resolver.rs:320-325).
- `touch` rewrites `published_at` of a cached record in place without re-signing (cache.rs:173-179). It has no caller in the workspace outside tests (searched); if it were used, a cached record would no longer verify.
- Defect: the negative cache does not suppress network walks. A negative hit returns `NameError::NotFound` (resolver.rs:317), and `resolve_one` treats every `NotFound` from the local resolver as a reason to call `resolve_over_mesh` (main.rs:478). So a name cached as absent is walked again on every request. The negative entry is only effective for callers that use `LocalResolver` directly.

### 4.7 Found, absent and unavailable at the origin

`resolve_one` (main.rs:435-510) and `NameResolution` (spine-protocol lib.rs:1199-1218) report one of:

| Result | When |
| --- | --- |
| `Resolved{record, provenance}` | A record came from cache, store, host synthesis, or a successful mesh walk (a mesh-found record is cached after signature verification, main.rs:480). |
| `Unchanged{ttl_secs}` | Caller supplied `if_none_match` equal to the record's `content_hash` (main.rs:457-463). |
| `NotFound{name}` | The local resolver returned `NotFound` and `resolve_over_mesh` returned `Ok(None)`; then a 30 s negative entry is written (main.rs:492-499). `Ok(None)` arises when there is no mesh configured at all (main.rs:517-519), or when the mesh lookup outcome was not `Found`, including a lookup that settled `NotFound` because no peer could be reached or all dispatches failed (naming_mesh.rs:855-857, 761). |
| `Invalid{name, reason}` | Parse failure, serialisation failure, or any local resolver error other than `NotFound`, including `cap:` names (`NotASingleName`). |
| `Unavailable{name, reason}` | Only when the mesh is configured but the routing table has zero addressable peers (main.rs:520-522), or the mesh lookup returned an error, which in practice is the 10 s timeout (naming_mesh.rs:781-782, main.rs:526). |

Consequence: "absent" is reported when every peer in a non-empty routing table was unreachable or when the walk ended without an answer, and it is then negatively cached for 30 s. "Unavailable" is reported only for an empty table or a timeout. COMPLETION_STATUS.md describes disconnected lookup as unavailable and notes that partial visibility cannot prove global absence; the code agrees with the second statement and only partly with the first.

### 4.8 Batches and capability queries at the origin

- `ResolveNames`: more than 256 names is an error for the whole command (main.rs:675-684). Names are processed in chunks of 16, each chunk concurrently; results are returned in input order; a failed task aborts the whole batch (main.rs:686-711). Each name gets its own result (main.rs:672-719).
- Library trait default `resolve_many` is sequential and independent per name (resolver.rs:84-90).
- `FindProviders`: local providers, then, if the mesh has at least one peer, the mesh providers; each remote record that is unexpired and verifies is cached and merged by name, preferring the higher `(seq, published_at)`. A mesh error fails the whole call with "capability lookup unavailable" (main.rs:721-762). Result is re-sorted as in 4.1 and truncated to `limit` if given.

## 5. Publication and renewal

### 5.1 Publication path (`PublishName`, main.rs:791-828)

1. Deserialise the record; reject malformed input.
2. Reject if `is_expired(now)` (main.rs:796).
3. `LocalResolver::publish` (resolver.rs:195-248): verify; reject a different, non-superseding record for the same routing key with `StalePublication`; if a persistent backend is configured, apply the persistence steps below; insert into the in-memory store; a store `Rejected` outcome becomes `Capacity`; remove the name from the resolver cache.
4. If a mesh resolver exists, `MeshNameResolver::publish` stores the record in the mesh node's own `NameService` store (a second, separate store from the `LocalResolver` store) and calls `replicate` (naming_mesh.rs:583-586).
5. `replicate` (naming_mesh.rs:601-640): run a `Node` lookup on `record.name.key()` to refresh the routing table; take up to K addressable peers nearest the key (excluding self); send a directed `NameAnnounce` to each, concurrently. If no addressable peer exists, send one broadcast `NameAnnounce` and report `broadcast = true`.

Persistence (feature `storage`, resolver.rs:203-240): table `spine-name-publishers-v1`; key is the UTF-8 canonical name string (including query and fragment); value is the record as serde JSON. The write happens after verification, a supersedes check against the persisted record, a capacity check (10,000 rows), and a trial insert on a clone of the store; the in-memory store is replaced only after the backend write succeeds. On start, all rows are loaded, their key is checked against the record name, signatures verified, and rows still fresh are published to the store. Expired rows stay on disk as sequence markers. Mesh replicas, resolver cache entries and negative entries are not persisted (resolver.rs:136-139).

Inconsistency: the persisted row key is the full canonical name, while the in-memory store key is the routing key (query and fragment dropped). Two names that differ only in query would be two rows on disk but one slot in memory. Behaviour in that case was not traced further.

Replicas receiving a `NameAnnounce` apply `accept_announcement` (4.5). A replica's copy is held in memory only (NameService uses `RecordStore`, naming.rs:430) and is lost on that node's restart.

### 5.2 What the publisher learns

The origin returns JSON with `stored: true`, `local_persistent`, `replicas` and `replication_sent` (count of completed directed sends), `replication_failed`, `replication_broadcast`, `replication_error`, `replication_acknowledged: 0`, `durable: false` (main.rs:413-420). `ReplicationReport::is_durable()` is hard-coded false and its comment states that no remote storage acknowledgement exists (naming_mesh.rs:222-226). There is no acknowledgement message for `NameAnnounce`; a successful send means only that the transport accepted the envelope. Receivers do not report accept or decline to the sender (counters are local, naming_mesh.rs:948-952).

### 5.3 Renewal and maintenance

- Owner renewal (`spawn_owner_renewal`, namespace.rs:199-235): off unless `renew_owned_records` is set (config.rs:287). Every `renewal_secs` (default 60) it takes persisted records whose authority equals the node's own key, `ttl_secs > 0`, and remaining life no more than `min(lapse_window_secs, ttl_secs / 2)`, renews each with `renewed(...)` keeping the same ttl, and publishes locally and to the mesh, at most `maintain_batch` (64) per tick (namespace.rs:180-197, 216-226). It only renews records signed by the node key; records for other `did` names that were published to this node are never renewed here.
- Replica maintenance (`maintain`, naming_mesh.rs:662-716), run every `maintain_secs` (config default 3600, namespace.rs:245-260): sweep expired records from the mesh store; take held, unexpired records for which this node is responsible; re-offer up to `max_records` (64) of them via `replicate`, one at a time, resuming from a key cursor on the next pass; report names within `lapse_window_secs` (900) of expiry as lapsing. Re-announcing does not extend life because expiry is signed into the record (naming_mesh.rs:659-661).
- Sweep also drops expired entries from the `LocalResolver` store and cache (resolver.rs:274-278); who calls it on a schedule was not traced (Undetermined; searched namespace.rs and main.rs for `sweep`).

## 6. Test vectors

The existing tests contain almost no fixed expected outputs. Fixed keys are used (`[seed; 32]` byte arrays, for example `SigningKey::from_bytes(&[1; 32])`), but no test asserts a literal hash, public key, name string, canonical byte string or signature. A repository search of spine-name and the naming modules for long hex literals found none. The only literal vectors are below.

### 6.1 Vectors present in the tests

Base32 (base32.rs:145-153; RFC 4648 section 10, lowercased, unpadded):

| Input (ASCII) | Output |
| --- | --- |
| `f` | `my` |
| `fo` | `mzxq` |
| `foo` | `mzxw6` |
| `foob` | `mzxw6yq` |
| `fooba` | `mzxw6ytb` |
| `foobar` | `mzxw6ytboi` |

Rejections (base32.rs:122-142): `decode("ab1c")`, `"ab0c"`, `"ab8c"`, `"-bcd"` return none; `decode("a")`, `"abc"`, `"abcdef"` return none; `decode_key` of 16-byte and 33-byte inputs returns none; a 32-byte key of `0xAB` bytes encodes to 52 symbols (the symbols themselves are not given in the test). Parse rejections (uri.rs:539-554): `https://example.com/` is `NotSpineScheme`; `spine://ftp:whatever/` is `UnknownAuthorityKind`; `spine://did:mzxw6/` is `InvalidAuthority`; `spine://` is an error.

Name behaviours with literal inputs: `spine://cap:Web.Search/` parses to term `web.search` (uri.rs:505); `spine://host:node.example.org:9440/` gives host `node.example.org`, port 9440, and without `:9440` gives no port (uri.rs:513-527); `/a//b/./c` and `/a/b/x/../c` both normalise to `/a/b/c` and share a key (uri.rs:557-564); `/../../etc/passwd` becomes `/etc/passwd` (uri.rs:567-571); `?b=2&a=1` prints as `?a=1&b=2` (uri.rs:574-581); `join` cases on base path `/tools/search`: `index` gives `/tools/index`, `../about` gives `/about`, `/top` gives `/top` (uri.rs:593-613).

Record behaviours with literal inputs (record.rs:432-456): published_at 1000, ttl 60 gives `expires_at` 1060; fresh at 1059, expired at 1060; remaining 30 at 1030 and 0 at 9999. Ordering: (seq 2, t 1000) supersedes (seq 1, t 1000); (seq 2, t 2000) supersedes (seq 2, t 1000); a record does not supersede itself.

### 6.2 Vectors that must be generated (none exist in the repository)

Do not treat any of these as known until produced by a reference run and reviewed:

1. Public key and 52-symbol `did:` string for a fixed Ed25519 seed (suggest seed `[1u8; 32]`, which the tests already use).
2. SHA-256 routing key of `spine://did:<...>/` and of a name with a path, and of `cap:web.search`, each in hex and base32 (confirms 2.6 including the `cap:` derivation).
3. `blob_of(b"hello agents")` name string.
4. Full `signing_bytes` hex for a minimal record (no endpoints, no capabilities, no content hash, no links, no meta) and for a record using every field including a non-default priority, a `None` and a `Some` content hash, a link, and two meta keys inserted in reverse order.
5. The Ed25519 signature (hex, and JSON base32) over each of those; Ed25519 is deterministic, so the value is stable.
6. Negative vectors: tampered field per field, wrong-length signature, signature under a different name, trailing-bit-set 52-symbol `did:` string (to pin the behaviour in 2.5), non-ASCII input to `SpineUri::parse` (to settle 2.8), equal-(seq, published_at) conflicting records (to pin 3.6).
7. XOR distance and bucket index examples for named keys (key.rs tests assert only relations and the two extreme bucket indexes 255 and 0 for keys differing in the top and bottom bit).

## 7. What is not defined in this layer

No wire encoding for naming messages is specified independently of serde JSON of Rust types; no limits on record size or field counts; no rate limits on announcements or lookups; no definition of how `Link` targets are fetched. These matter for a second implementation and are not answered by the code read.

## 8. Gaps against the target design

Target design reference: docs/FIRST_PRINCIPLES.md layer 2 and layer 6. Each item states what the code does today.

### 8.1 Key rotation: absent

- A `did:` authority is the raw Ed25519 public key (uri.rs:39, 84). The name is the key; there is no inception key, no succession record, and no field in `NameRecord` that names a successor or a prior key (record.rs:53-85).
- Verification checks the signature against the key in the name only (record.rs:269-282). A new key means a new name, and nothing links the two.
- A lost or compromised key therefore ends the name or hands it to the attacker permanently. The node key is a file written once (`spine-node.key`, namespace.rs:289-328) and doubles as the node's DHT position, so a node identity change also relocates the node.

### 8.2 Revocation: absent

- No record type, flag, or message revokes a name, a key or a record. Searched spine-name and the naming modules for revoke, revocation, rotate, succession: no matches.
- The only ways a record stops being served are expiry (record.rs:290; no cap on `ttl_secs`) and replacement by a higher `(seq, published_at)` signed by the same key (record.rs:304). An attacker holding the key can publish a higher seq with a long ttl and the legitimate holder cannot override it without the same key and a still higher seq.
- Removal APIs exist locally (`RecordStore::remove`, store.rs:220) but are not signed or propagated.
- Rollback exposure at lookup: a `Name` lookup ends at the first valid record returned (naming.rs:364) without seeking the highest seq, so a stale replica or a malicious peer holding an older, still-unexpired signed record can answer for the name. Combined with no revocation, an old record (for example an old endpoint list) stays authoritative until it expires.

### 8.3 Human-meaningful labels and attestations: absent

- The namespace has four authority kinds, none of which is a label (uri.rs:36-50). The only free text is `meta` (signed but self-asserted, record.rs:80-81), `Link.title` (unsigned, link.rs:99-100) and `cap:` terms (record.rs:68).
- No attestation object, no verifier of third-party claims, no petname or label-to-key binding exists. Searched for attest: the only hits are `is_attested`, which means "has a signature that verifies" (record.rs:144-146, resolver.rs:68-70), a different concept.

### 8.4 Sybil and admission cost for `cap:` publishing: absent

- Any `did:` key can publish a record claiming any capability terms and any number of them. The code states the premise itself: minting a `did:` name costs one keygen (store.rs:35-38, naming.rs:478-487).
- The only bounds are store capacity (10,000 records, store.rs:39) with distance-based eviction (store.rs:89-145), proximity filtering of announcements (naming.rs:488-496, 792-800), and the 20-provider cap on a lookup (naming.rs:367). None is an admission cost. With an empty routing table every key counts as "near" (naming.rs:792-800).
- Providers are ranked by endpoint count, then recency, then name (store.rs:178-185, main.rs:763-769). Endpoint count is self-declared and unsigned by anyone else, so a spammer wins ranking by listing many endpoints. There is no vouch, stake, proof of work or reputation field.
- Capability routing defect: a capability lookup is routed toward `SHA-256("cap:" + term)` (naming.rs:80-82), but replication places a record only at the nodes closest to `record.name.key()` (naming_mesh.rs:602-603). No code path stores a record under the capability key. So remote capability discovery succeeds only if a queried node happens to hold the provider's record for other reasons; nodes near the capability key are not given provider records. The mesh test at naming_mesh.rs:1617 exercises a small mesh; its behaviour at scale was not examined (Undetermined for network-scale behaviour). In practice `FindProviders` mostly returns local providers.
- Eclipse and poisoning exposure in the DHT, relevant to any naming use: referrals are unauthenticated hints. Every `closer` entry from any response is inserted into the peer map and routing table (naming.rs:726-729, 340-420). `NodeInfo.last_seen` is attacker-supplied, and a full bucket replaces the stalest contact whenever the newcomer's `last_seen` is greater (routing.rs:108-121), so a peer can claim a huge `last_seen` to displace established contacts. The agent-id to key mapping is taken from the referral (naming.rs:727). The peers map (`NameService.peers`) has no size cap (naming.rs:437), unlike the transport address book (mesh_tcp.rs:67). Whether the transport later rejects an agent whose key does not match its claimed `agent_id` was not determined here (Undetermined; identity.rs derives `AgentId` by hashing the seed, identity.rs:167, lib.rs:245-249, so the id is not derivable from the public key by a verifier).

### 8.5 Acknowledged replication: absent

- Replication sends `NameAnnounce` and counts completed sends (naming_mesh.rs:629-634). There is no acknowledgement message, no read-back verification, no retry accounting beyond periodic re-offer every `maintain_secs` (default 3600, config.rs:292) for at most 64 records per pass, and replicas are memory-only (naming.rs:430). The origin reports `durable: false` and `replication_acknowledged: 0` (main.rs:419-420); COMPLETION_STATUS.md lists acknowledged replica storage as an open gate (docs/COMPLETION_STATUS.md:84).
- A publisher cannot distinguish "peer stored it", "peer declined it for proximity or capacity", and "peer dropped it" (naming_mesh.rs:948-952 count locally only).

### 8.6 Other defects found while reading

| # | Finding | Reference |
| --- | --- | --- |
| a | Parser may panic on non-ASCII input (section 2.8) | uri.rs:187-191, 346 |
| b | Negative cache does not prevent repeated mesh walks (section 4.6) | main.rs:478, resolver.rs:317 |
| c | "Absent" reported when all known peers were unreachable (section 4.7) | naming_mesh.rs:855-857, main.rs:492-499 |
| d | Equal `(seq, published_at)` conflicting records do not converge (section 3.6) | record.rs:304-306, store.rs:117 |
| e | `Link.title` and `Link.capabilities` are outside the signature | record.rs:232-238, link.rs:99-106 |
| f | Strings in `signing_bytes` are NUL-terminated, not length-prefixed | record.rs:200-248 |
| g | Cache accepts an older valid record over a newer one | cache.rs:140-154, resolver.rs:251 |
| h | Lookup does not check that a returned record is unexpired | naming.rs:319-329 |
| i | Envelope signature is taken over `serde_json` output, the construct `signing_bytes` avoids for records | mesh.rs:916-924 |
| j | `verify` rather than `verify_strict` | record.rs:280 |
| k | Record store capacity is count-based only; no byte or field-count limits | store.rs:39, record.rs |
| l | `host:` parsing treats a bare IPv6 address as host plus port | uri.rs:104-106 |

### 8.7 Undetermined items

| Item | Looked in |
| --- | --- |
| Whether a record is size-limited before JSON parsing at the origin or in mesh ingress | naming.rs, naming_mesh.rs, spine-name (no check found; frame limit 4 MiB in mesh_tcp.rs:62 only) |
| Whether a concrete NUL-injection collision exists in `signing_bytes` | record.rs (analysis only; none constructed) |
| Whether non-ASCII input to `SpineUri::parse` actually panics | uri.rs (by reading the standard library contract; not run) |
| Who calls `LocalResolver::sweep` on a schedule | namespace.rs, main.rs |
| Whether the transport binds a peer's claimed `agent_id` to its public key | identity.rs, mesh_tcp.rs (mesh.rs:734 verifies only against the key the message carries) |
| Behaviour of capability lookup at network scale | naming_mesh.rs tests read only to function level |
| Behaviour when two names differing only in query are published to the persistent resolver | resolver.rs:198-240 |
| The wire encoding used by the QUIC and WebSocket mesh transports | mesh_tcp.rs only for TCP: JSON frames; other transport files not read |
