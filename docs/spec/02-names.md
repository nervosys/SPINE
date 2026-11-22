# Layer 2: Names (as built)

Status: sections 2.5, 2.6, 2.8, 2.9, 3 (except 3.2), 6 and 9 are normative and match the code of branch `completion/agent-web` after the changes that accompany `docs/spec/02-names.vectors.json`. They cite `file: function` and are checked by the vectors (section 6). The other sections (2.1 to 2.4, 2.7, 4, 5, 7, 8) are the earlier read-through description of the code, unchanged except where noted; their `file:line` citations may have drifted and the paths under `research/spine-agentic` have since moved into `src/spine-mesh`. Where a claim there disagrees with a normative section, the normative section wins. Section 9 is the list of known open issues.

Primary sources: `src/spine-name/src/` (uri.rs, base32.rs, key.rs, record.rs, link.rs, lib.rs, store.rs, cache.rs, resolver.rs, routing.rs, frontier.rs), the naming code in `src/spine-mesh`, and the node glue in `src/spine-core`. Key words MUST and MUST NOT in the normative sections describe what a conforming implementation does; they restate the code, they do not change it.

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

### 2.5 Base32 (base32.rs: `encode`, `decode`, `decode_key`)

The alphabet is `abcdefghijklmnopqrstuvwxyz234567` (RFC 4648 section 6, lower case) with no padding.

- Encoding: the input bits are written most significant first and cut into 5-bit groups; a final partial group is padded on the right with zero bits. A 32-byte key is 256 bits, so 52 symbols, the last carrying one data bit and four zero padding bits. The empty input encodes to the empty string.
- Decoding accepts upper and lower case letters and nothing else. It MUST refuse a length of 1, 3 or 6 symbols modulo 8 (no byte string encodes to it) and MUST refuse a final group whose padding bits are not all zero. Hence a byte string has exactly one spelling up to letter case, and `decode_key` additionally requires exactly 32 bytes.
- The same codec spells a `NameKey` in JSON (key.rs: `key_bytes`) and a record signature in JSON (record.rs: `sig_bytes`).

History: before the change that accompanies the vectors, `decode` shifted the padding bits out without testing them, so several 52-symbol strings named one key. See `keys[].noncanonical_base32` and `invalid_names` in the vectors.

### 2.6 Routing key (uri.rs: `SpineUri::key`, `routing_form`)

The routing key of a name is SHA-256 over the UTF-8 bytes of `spine://` + canonical authority + normalised path. The query and the fragment are not part of it, so names that differ only there share a key and a slot in every `RecordStore` (store.rs: `put`, `get`). The signed name (3.3) does include them. The routing key has no domain separation (open issue 3). The vectors give the key of every name in `names[]`.

Capability routing key (spine-mesh naming.rs: `capability_key`): SHA-256 over `cap:` + lower-case term, with no `spine://` prefix and no trailing `/`. This is not `SpineUri::capability(term).key()`, which hashes `spine://cap:<term>/` (open issue 4).

### 2.7 Joining references

`SpineUri::join` (uri.rs:341-391) resolves a reference like an href. Absolute `spine://` references are re-parsed. A rooted reference replaces the path. Anything else is resolved against the base path's directory (up to and including its last `/`). The reference's own query and fragment replace the base's. Empty reference returns the base.

### 2.8 Non-ASCII input to the parser (resolved)

The earlier reading of this section found that `SpineUri::parse` and `SpineUri::join` could panic when byte 8 of the input fell inside a multi-byte character. The code now compares the scheme prefix through `str::get(..SCHEME.len())` (uri.rs: `parse`, `join`), which returns `None` rather than panicking. The vectors include non-ASCII paths, queries and fragments (`names[]`).

### 2.9 Resource id (lib.rs: `resource_id`, `verify_resource`, `validate_content_type`, `content_hash`)

`content_hash(bytes)` is plain SHA-256. A resource id is

    SHA-256( "spine/resource-id/v1" 0x00 || u32_be(len(content_type)) || content_type
             || u64_be(len(bytes)) || bytes )

so it commits to the content type as well as the bytes, and the prefix and length framing keep it from equalling the plain hash of any ordinary byte string and make ("a", "bcdef") and ("ab", "cdef") different. A content type is valid only in canonical form: 1 to 256 bytes of printable ASCII (0x20 to 0x7e), no upper-case letters, no leading or trailing space; otherwise `resource_id` returns `InvalidContentType`. `verify_resource(id, type, bytes)` recomputes the id and returns `ResourceIdMismatch` if it differs. `resource_id` is the only function that computes an id. `NameRecord::content_hash` holds a resource id, not a plain hash. Vectors: `resource_ids[]`, `invalid_content_types[]`.

## 3. NameRecord

### 3.1 Fields (record.rs: `NameRecord`, `Endpoint`; link.rs: `Link`, `Rel`)

| Field | Type | Signed | Notes |
| --- | --- | --- | --- |
| `name` | `SpineUri` | yes (canonical string, including query and fragment) | MUST have a `did` authority. |
| `seq` | `u64` | yes | Version counter. |
| `ttl_secs` | `u32` | yes | Default 3600 (`DEFAULT_TTL_SECS`). 0 is accepted and means expired at publication. At most `MAX_RECORD_TTL_SECS` (604800, 7 days): `sign` refuses more and `verify` fails with `TtlTooLong` (3.5). |
| `published_at` | `u64` | yes | Unix seconds, set by the publisher. A verifier with a clock refuses a value more than `MAX_CLOCK_SKEW_SECS` (300) ahead of it (3.5). |
| `endpoints` | `Vec<Endpoint>` | yes | Each `{transport, address, priority: u8}`; strings are free text. |
| `capabilities` | `Vec<String>` | yes | Free text. `with_capability` lower-cases; deserialisation does not, and the signature covers the string as stored. |
| `content_hash` | `Option<[u8; 32]>` | yes | A resource id (2.9) of the current representation. |
| `links` | `Vec<Link>` | yes | Every field of every link is signed: `rel`, `target`, `title` and `capabilities`. |
| `meta` | `BTreeMap<String, String>` | yes | Signed in byte-wise key order. The key `spine.revoked` is reserved: it marks a tombstone (3.7). |
| `signature` | `Vec<u8>` | n/a | 64 bytes of Ed25519; JSON form is unpadded lower-case base32. |

`Rel` is `child, parent, peer, provides, requires, alternate, describes, snapshot` or `Other(String)`. Its wire string is the lower-case name, and `Other` carries its own string. Deserialising `Other(s)` where `s` is exactly a built-in name yields the built-in variant (link.rs: `Rel::other`, `From<RelRepr>`), so the alias cannot come out of deserialisation; `Rel::parse` lower-cases and also maps built-in names. `Other("Mirrors")` keeps its case, which is signed. Constructing `Rel::Other("child")` directly in Rust is still possible and signs as `child` (open issue 1).

JSON shape (serde, record.rs and link.rs): an object with `name` (string), `seq`, `ttl_secs`, `published_at` (numbers), `endpoints` (array of `{transport, address, priority}`), `capabilities` (array of strings), `content_hash` (array of 32 integers or `null`), `links`, `meta` (object) and `signature` (base32 string). A link is `{rel, target}` plus `title` only when present and `capabilities` only when non-empty; `rel` is a string for a built-in relation and `{"other": "<name>"}` otherwise. Missing `endpoints`, `capabilities`, `content_hash`, `links`, `meta` and `signature` default to empty or null on input. The vectors carry this form as `records[].json`. This form is not canonical and is not what is signed.

### 3.2 Which names can have records

`NameRecord::new` fails with `NotSignable` unless the authority is `did`. `verify` fails with `NotSignable` for any other authority. `RecordStore::put` verifies first. Therefore a `blob:`, `cap:` or `host:` name can never be stored, cached via `cache_record`, or resolved from a store; `host:` records are synthesised (4.1).

### 3.3 Signed bytes (record.rs: `NameRecord::signing_bytes`, layout v2)

`str(x)` is a big-endian `u32` byte length followed by the UTF-8 bytes of `x`. Every count is a big-endian `u32`. Integers are big-endian. The signed bytes are the concatenation, in order, of:

| # | Bytes |
| --- | --- |
| 1 | `spine-name-record-v2` followed by `0x00` (21 bytes) |
| 2 | `str(name)`, `name` being the canonical string including query and fragment |
| 3 | `seq` (8 bytes), `ttl_secs` (4), `published_at` (8) |
| 4 | endpoint count; per endpoint, in stored order: `str(transport)`, `str(address)`, `priority` (1 byte) |
| 5 | capability count; per capability, in stored order: `str(term)` |
| 6 | `0x00`, or `0x01` followed by the 32 bytes of `content_hash` |
| 7 | link count; per link, in stored order: `str(rel)`, `str(target)`, then `0x00` or `0x01` followed by `str(title)`, then the count of link capabilities and `str(term)` for each in stored order |
| 8 | meta count; per entry in byte-wise key order: `str(key)`, `str(value)` |

Everything in the record except `signature` is covered. No field is terminated by a sentinel, so no string content can move a field boundary: the encoding is injective. Endpoints, capabilities, links and link capabilities are signed in stored order, so reordering them invalidates the signature; meta order does not matter. `str(rel)` is the wire string, so `Rel::Other("child")` and `Rel::Child` sign identically. `signing_bytes` is public API. The vectors give the bytes for every record in `records[]`; Python builds them from the structured description in `records[].record`.

### 3.4 Signature and verification (record.rs: `sign`, `verify`)

- Algorithm: Ed25519 over exactly the bytes of 3.3, no prehash, no context. The key is the 32-byte key of the `did:` authority of `name`.
- `sign` fails with `KeyMismatch` unless the signing key's public key equals the key in the name. It also fails, after that check, if the record breaks the lifetime rules of 3.5 (`TtlTooLong`, `MalformedTombstone`): no honest signer produces a record every verifier refuses.
- `verify` MUST fail with `BadSignature` unless the signature is exactly 64 bytes and the key in the name is a valid point, and then uses `verify_strict` (`ed25519_dalek::VerifyingKey::verify_strict`): in addition to RFC 8032 verification it rejects a small-order public key or R and a non-canonical S. The reference implementation's strict profile is RFC 8032 section 5.1.7 plus rejecting S >= L and a small-order A or R. `invalid_records[]` includes S + L and an identity public key with R = identity, S = 0 (accepted by non-strict verification).
- After the signature, `verify` applies the clock-free lifetime rules of 3.5 (`validate_lifetime`), so a record that verifies has `ttl_secs <= MAX_RECORD_TTL_SECS` and, if it carries the tombstone marker, is a well-formed tombstone. Every path that verifies untrusted input therefore enforces them.
- Verification is performed at every ingress (store put, resolver publish and cache, lookup responses, announcements, restore from disk). Signature validity does not depend on time.

### 3.5 Validity, expiry and lifetime bounds (record.rs: `validate_lifetime`, `validate_not_postdated`, `validate`, `is_expired`)

Constants: `MAX_RECORD_TTL_SECS` = 604800 (7 days), `MAX_CLOCK_SKEW_SECS` = 300.

- `expires_at = published_at + ttl_secs`, saturating. `is_expired(now)` is `now >= expires_at`. A record is fresh for `published_at <= t < expires_at`.
- Maximum lifetime. A record MUST have `ttl_secs <= MAX_RECORD_TTL_SECS`. `sign` refuses to sign more (`TtlTooLong`); `verify` fails with `TtlTooLong` for a signed record that has more; `validate_lifetime` makes the same test without a signature. The bound exists because a record cannot be recalled from a replica that never hears of its replacement or revocation: it limits how long a withdrawn or compromised record can keep resolving there to seven days from its `published_at`.
- Post-dating. A verifier that has a clock MUST refuse a record whose `published_at` is greater than `now + MAX_CLOCK_SKEW_SECS` (`PostDated`; `now + 300` saturates at the largest `u64`). Without it a signer extends a record's life by publishing it in the future. A record published in the past is never refused for that reason; whether it is still fresh is a separate test.
- Admission. `NameRecord::validate(now)` is the full check made on a record from outside: size (`validate_size`), then lifetime, then the bound on `published_at`. It is called, with the caller's clock and before `verify`, at: `RecordStore::put` (the bound on `published_at`, with `verify` supplying the lifetime rules), `LocalResolver::publish`, `LocalResolver::cache_record`, `LocalResolver::persistent` (restore: a record that fails is skipped, like a bad signature), `NameService::publish`, `NameService::accept_announcement` (mesh announcements), `Lookup` (records and providers in lookup responses: size, lifetime, and the bound when the lookup was given a clock, which a lookup started without one is not), the gateway resource routes, the CLI `read_record`, and the agent client's `read_resource` and `fetch_resource_with` (the bound). Pure functions take the time as an argument; none reads a clock.
- Signature validity does not depend on time. Stores accept expired records (`put` does not test expiry). Expiry is applied at read time: `get_fresh`, `providers_of`, `handle_request`, and by sweeping.
- Publication at the origin rejects an already-expired record (main.rs `PublishName`, `PublishResource`).
- Clock: injected function returning Unix seconds (resolver.rs, naming_mesh.rs). The only tolerance is the 300 s skew above.

### 3.6 Ordering and conflict resolution (record.rs: `supersedes`)

`self` replaces `other` iff `(seq, is_tombstone, published_at, signature)` of `self` is lexicographically greater than that of `other` (`false` below `true`; signatures compared as byte strings). The second component makes a tombstone beat an ordinary record of the same `seq` whatever the arrival order; between two ordinary records, or two tombstones, the rule is the earlier `(seq, published_at, signature)` order. The signature is the final tiebreak, which makes the order total over distinct records, so replicas converge when they see the same set. Records are compared only within one routing key (same authority and path; query and fragment ignored).

- `RecordStore::put`: an existing record for the key is replaced only if the new one supersedes it; otherwise the outcome is `Superseded` and the stored record is kept.
- `LocalResolver::publish`: a different record that does not supersede the stored one returns `StalePublication`; offering the identical record again is accepted as a no-op.
- A publisher that signs two records with the same `seq` and `published_at` is not detected or penalised; replicas converge on the one with the greater signature, which the publisher does not control.
- `seq` is a bare counter chosen by the signer: no chain, no hash link, no required step of one. `renewed` verifies the old record, increments `seq` by one (checked: `SequenceExhausted` at `u64::MAX`), sets `published_at` and `ttl_secs`, and re-signs.
- The resolver cache does not compare `seq` (cache.rs: `ResolverCache::put`), so a node can cache an older valid record over a newer one (8.6 g).

### 3.7 Tombstones (revocation) (record.rs: `NameRecord::tombstone`, `is_tombstone`)

A tombstone withdraws a name. It is not a new signed object type: it is an ordinary `NameRecord`, signed with the unchanged layout v2 (3.3), for the same name as the records it withdraws.

- Encoding. The `meta` entry `spine.revoked` with value `1` (`REVOKED_META_KEY`). Because `meta` is signed (3.3, row 8), the marker is covered by the signature, and no byte of the layout changes: the signing domain stays `spine-name-record-v2`, and every existing record keeps its bytes and signature. `is_tombstone` is true iff the key `spine.revoked` is present.
- Shape. A well-formed tombstone has `meta` equal to exactly `{spine.revoked: "1"}`, no endpoints, no capabilities, no content hash and no links, and `ttl_secs` equal to `MAX_RECORD_TTL_SECS`. Anything else carrying the key (another value, extra metadata, content, a different ttl) is refused with `MalformedTombstone`, by `sign` and by `verify`.
- Supersession. A tombstone has a `seq` above that of the records it withdraws and supersedes them by 3.6. Replicas converge on it by the same rule as for any record.
- Holders. A store, resolver or replica that holds a fresh tombstone for a name MUST NOT accept a record for that name of the same or lower `seq` (it does not supersede, 3.6: `RecordStore::put` answers `Superseded`, `LocalResolver::publish` answers `StalePublication`) and MUST answer a resolution of the name with the distinct outcome revoked, not "not found": `LocalResolver::resolve` returns `NameError::Revoked`, from cache or store. `find_providers` never returns a tombstone (it has no capabilities). A higher `seq` re-publishes the name.
- Replication. A tombstone is announced, stored, replicated, re-offered by maintenance and answered in a lookup exactly like a record; nothing on those paths needs an endpoint or a capability. A lookup returns it as the highest-`seq` record (`MeshNameResolver::resolve` returns the signed tombstone, which the caller maps to revoked); the daemon reports it as `NameResolution::Invalid` with a reason naming the revocation, as it does for a locally held tombstone.
- Renewal. `NameRecord::renewed` refuses a tombstone (`Revoked`), and owner renewal (5.3) skips a tombstone and any name this node holds a fresh tombstone for.
- Lifetime and the limit of revocation. A tombstone has the same maximum lifetime as any record and is required to have exactly that: `ttl_secs == MAX_RECORD_TTL_SECS`. After it expires the name is simply unknown again unless it is re-published with a higher `seq`. Revocation by tombstone is therefore not permanent: permanence needs the succession log planned for a later stage (open issue 9.10).

Replay argument. Let T be a tombstone and R a record it withdraws, signed by an owner who assigns `seq` in publication order, so `published_at(R) <= published_at(T)`; since `ttl(R) <= MAX_RECORD_TTL_SECS = ttl(T)`, `expires_at(R) <= expires_at(T)`, so R is already expired when T lapses and an attacker replaying R afterwards gets a record that `is_expired` and that no replica serves. The bound on `ttl_secs` is what makes `ttl(T) = MAX` enough, and the bound on `published_at` is what stops R from having been post-dated beyond what its signer's own clock allowed when T was signed; a key holder who signs a lower-`seq` record dated after T is acting with the key, which no tombstone defends against.

### 3.8 Provisional decisions

Taken for the lifetime and revocation change; each is open to revision, with the alternative that was not taken.

1. Maximum lifetime 7 days (604800 s). Alternative: 24 hours (shorter exposure, but owners offline for a day lose their names) or 30 days (fewer renewals, a longer window for a withdrawn record).
2. Clock skew 300 s. Alternative: 60 s (less room for honest clock error) or 900 s.
3. Tombstone marker as the reserved `meta` key `spine.revoked` = `1`, with the v2 signing layout unchanged. Alternative: a v3 layout with an explicit record-kind byte, which is unambiguous in the layout but invalidates every signed record and the vectors.
4. A tombstone's `ttl_secs` must equal the maximum. Alternative: at least the maximum or free; equality is the simplest rule to verify and to prove (3.7).
5. A tombstone beats an ordinary record of equal `seq`. Alternative: leave equal-`seq` ties to `published_at`, which lets two replicas that see a tombstone and a record in different orders disagree.
6. The revoked outcome is `NameError::Revoked` at the local resolver, and the mesh returns the tombstone record, mapped by the daemon to `NameResolution::Invalid`. Alternative: a `NameResolution::Revoked` wire variant, which changes spine-protocol and spec 00 and is left to a wire revision.
7. The lifetime bound (clock-free) is part of `verify`; the clock-dependent bound is part of `validate(now)`. Alternative: only in `validate(now)`, which leaves every plain `verify` call site (resource store, mesh envelope builders) unprotected.
8. `RecordStore::put`, `NameService::publish` and `NameService::accept_announcement` take `now`. Alternative: a clock stored in the store, which is a hidden time source in a pure structure.
9. A `Lookup` without a clock (`now` = 0) skips the bound on `published_at`, as it already skips expiry. Alternative: refuse every record, which breaks callers that never supply a clock.

## 4. Resolution

### 4.1 Local algorithm (`LocalResolver::resolve`)

1. `cap` authority: error `NotASingleName`.
2. `host` authority: build an unsigned record from the name itself: sequence 0, ttl from `DEFAULT_TTL_SECS`, one endpoint `("tcp", host[:port])`, return provenance `Address` (record.rs:120-140). Never cached. `is_attested` is false.
3. Cache lookup:
   - Fresh hit: provenance `Cache`; a fresh tombstone gives error `Revoked`.
   - Negative hit: error `NotFound`.
   - Stale hit: if the store holds a fresh record, return it with `Local` (a fresh tombstone gives `Revoked`); otherwise return the stale record with `StaleCache`, except that a lapsed tombstone gives `NotFound`.
   - Miss: continue.
4. Store: fresh record gives `Local`; a fresh tombstone gives error `Revoked`; otherwise error `NotFound`.

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

Payload variants carried in a signed `MeshEnvelope` (mesh.rs:220-224 and builders mesh.rs:640-724). The envelope and its signature are defined by spine-mesh (`src/spine-mesh`, mesh.rs) and are about to change; they are not specified here, and this layer does not depend on their layout.

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
| `Invalid{name, reason}` | Parse failure, serialisation failure, or any local resolver error other than `NotFound`, including `cap:` names (`NotASingleName`) and a revoked name (`NameError::Revoked`, locally held or found as a tombstone on the mesh; the tombstone is cached so later resolutions answer the same without a walk). |
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

- Owner renewal (`spawn_owner_renewal`, namespace.rs:199-235): off unless `renew_owned_records` is set (config.rs:287). Every `renewal_secs` (default 60) it takes persisted records whose authority equals the node's own key, that are not tombstones and whose name the node does not hold a fresh tombstone for, `ttl_secs > 0`, and remaining life no more than `min(lapse_window_secs, ttl_secs / 2)`, renews each with `renewed(...)` keeping the same ttl (capped at `MAX_RECORD_TTL_SECS`), and publishes locally and to the mesh, at most `maintain_batch` (64) per tick (namespace.rs: `owned_renewals`). A revoked name is not renewed back to life. It only renews records signed by the node key; records for other `did` names that were published to this node are never renewed here.
- Replica maintenance (`maintain`, naming_mesh.rs:662-716), run every `maintain_secs` (config default 3600, namespace.rs:245-260): sweep expired records from the mesh store; take held, unexpired records for which this node is responsible; re-offer up to `max_records` (64) of them via `replicate`, one at a time, resuming from a key cursor on the next pass; report names within `lapse_window_secs` (900) of expiry as lapsing. Re-announcing does not extend life because expiry is signed into the record (naming_mesh.rs:659-661).
- Sweep also drops expired entries from the `LocalResolver` store and cache (resolver.rs:274-278); who calls it on a schedule was not traced (Undetermined; searched namespace.rs and main.rs for `sweep`).

## 6. Test vectors

The vectors for this layer are `docs/spec/02-names.vectors.json`, generated by `scripts/names_v1.py` (standard-library Python, 3.8 or later, written from this specification and not from the Rust code) and checked against the crate by `src/spine-name/tests/vectors.rs`.

    python scripts/names_v1.py generate > docs/spec/02-names.vectors.json
    python scripts/names_v1.py check docs/spec/02-names.vectors.json

The script carries its own Ed25519 (RFC 8032 section 6 reference algorithm) and asserts RFC 8032 section 7.1 test vectors 1 to 3 before generating or checking. Generation is deterministic (fixed seeds `0x01`, `0x02`, `0x03` repeated 32 times, sorted keys, LF, no times). CI (the `wire-vectors` job) runs `check` and diffs `generate` against the committed file; the `test` job runs the Rust side.

| Section | Content |
| --- | --- |
| `keys` | seed, public key, base32, `did:` authority, name, and a non-canonical spelling that MUST be refused |
| `names` | parse input, canonical form, routing key, and the parsed authority, path, query and fragment |
| `invalid_names` | strings that MUST NOT parse, including non-zero trailing bits |
| `resource_ids` | content type, bytes, id and plain hash; empty bytes; the ("a", "bcdef") / ("ab", "cdef") boundary pair |
| `invalid_content_types` | types that `resource_id` MUST refuse |
| `rel_aliases` | `Rel` JSON forms, including `{"other": "<built-in>"}` |
| `records` | structured record, signing bytes, signature, `is_tombstone` and serde JSON: minimal, every optional field, every relation, multi-byte UTF-8, empty strings, numeric extremes (with the maximum ttl), case, a record at the maximum lifetime, and a tombstone. All MUST verify |
| `invalid_records` | each with a `reason`. `signature`: one field changed after signing (per field), signature damaged in several ways, a signature by another key, and a weak-key signature. `ttl`: correctly signed, `ttl_secs` one over the maximum and at the `u32` maximum. `tombstone`: correctly signed, carrying the marker but with a ttl other than the maximum, an endpoint, a capability, a content hash, extra metadata, another marker value, or on an ordinary record. All MUST fail `verify` (with `BadSignature`, `TtlTooLong`, `MalformedTombstone` respectively) and `sign` MUST refuse the last two groups |
| `admission` | a record from the two sections above, the verifier's clock `now`, and whether it is admitted (3.5): published exactly the skew ahead (admit), one second past it (reject), in the past, at the `u64` maximum with the clock at the maximum (admit) and at 1.7e9 (reject), the maximum lifetime, a tombstone, and over-long or malformed ones |

The Rust test pins the layout two ways: it compares `signing_bytes()` byte for byte with the vector, and, because Ed25519 is deterministic, re-signs the deserialised record with the vector's seed and compares the signature. It also compares `serde_json::to_value(&record)` with `records[].json`.

Not covered by vectors: distance and bucket arithmetic (key.rs tests assert relations and the extreme indexes), `join`, equal-`(seq, published_at)` conflicts and the tombstone ordering of 3.6, and the resolution and replication behaviour of sections 4 and 5 (the Rust tests cover a tombstone's supersession, the revoked outcome locally and through a lookup from a second node, and refusal at every admission point).

## 7. What is not defined in this layer

No wire encoding for naming messages is specified independently of serde JSON of Rust types; no limits on record size or field counts; no rate limits on announcements or lookups; no definition of how `Link` targets are fetched. These matter for a second implementation and are not answered by the code read.

## 8. Gaps against the target design

Target design reference: docs/FIRST_PRINCIPLES.md layer 2 and layer 6. Each item states what the code does today.

### 8.1 Key rotation: absent

- A `did:` authority is the raw Ed25519 public key (uri.rs:39, 84). The name is the key; there is no inception key, no succession record, and no field in `NameRecord` that names a successor or a prior key (record.rs:53-85).
- Verification checks the signature against the key in the name only (record.rs:269-282). A new key means a new name, and nothing links the two.
- A lost or compromised key therefore ends the name or hands it to the attacker permanently. The node key is a file written once (`spine-node.key`, namespace.rs:289-328) and doubles as the node's DHT position, so a node identity change also relocates the node.

### 8.2 Revocation: names yes, keys no

Resolved for names (3.5, 3.7): record lifetime is capped at 7 days, and an owner withdraws a name with a signed tombstone that supersedes lower-`seq` records and is answered as revoked. What this read originally found, and what remains:

- A name is revoked by a tombstone; a key is not. There is still no key rotation, succession or key revocation record (8.1): an attacker holding the key can publish a higher `seq`, including a tombstone of its own, and the legitimate holder cannot override it without a still higher `seq` under the same key.
- Revocation lasts until the tombstone expires (at most 7 days after publication). It is not permanent (open issue 9.10).
- The only ways a record stops being served are expiry, replacement by a record that supersedes it, and a tombstone. Removal APIs exist locally (`RecordStore::remove`) but are not signed or propagated.
- Rollback exposure at lookup is reduced, not removed: a `Name` lookup now settles on the highest-`seq` record seen once a quorum has answered (naming.rs: `name_settled`), so a tombstone held by one peer wins against an older record held by another, but a lookup whose peers all hold only the old record cannot learn of a tombstone it never hears, and the old record stays authoritative there for at most its remaining lifetime, bounded by 7 days.

### 8.3 Human-meaningful labels and attestations: absent

- The namespace has four authority kinds, none of which is a label (uri.rs:36-50). The only free text is `meta` (signed but self-asserted, record.rs:80-81), `Link.title` (signed since layout v2, link.rs: `Link`) and `cap:` terms (record.rs:68).
- No attestation object, no verifier of third-party claims, no petname or label-to-key binding exists. Searched for attest: the only hits are `is_attested`, which means "has a signature that verifies" (record.rs:144-146, resolver.rs:68-70), a different concept.

### 8.4 Sybil and admission cost for `cap:` publishing: absent

- Any `did:` key can publish a record claiming any capability terms and any number of them. The code states the premise itself: minting a `did:` name costs one keygen (store.rs:35-38, naming.rs:478-487).
- The only bounds are store capacity (10,000 records, store.rs:39) with distance-based eviction (store.rs:89-145), proximity filtering of announcements (naming.rs:488-496, 792-800), and the 20-provider cap on a lookup (naming.rs:367). None is an admission cost. With an empty routing table every key counts as "near" (naming.rs:792-800).
- Providers are ranked by endpoint count, then recency, then name (store.rs:178-185, main.rs:763-769). Endpoint count is self-declared and unsigned by anyone else, so a spammer wins ranking by listing many endpoints. There is no vouch, stake, proof of work or reputation field.
- Capability routing defect: a capability lookup is routed toward `SHA-256("cap:" + term)` (naming.rs:80-82), but replication places a record only at the nodes closest to `record.name.key()` (naming_mesh.rs:602-603). No code path stores a record under the capability key. So remote capability discovery succeeds only if a queried node happens to hold the provider's record for other reasons; nodes near the capability key are not given provider records. The mesh test at naming_mesh.rs:1617 exercises a small mesh; its behaviour at scale was not examined (Undetermined for network-scale behaviour). In practice `FindProviders` mostly returns local providers.
- Eclipse and poisoning exposure in the DHT, relevant to any naming use: referrals are unauthenticated hints. Every `closer` entry from any response is inserted into the peer map and routing table (naming.rs:726-729, 340-420). `NodeInfo.last_seen` is attacker-supplied, and a full bucket replaces the stalest contact whenever the newcomer's `last_seen` is greater (routing.rs:108-121), so a peer can claim a huge `last_seen` to displace established contacts. The agent-id to key mapping is taken from the referral (naming.rs:727). The peers map (`NameService.peers`) has no size cap (naming.rs:437), unlike the transport address book (mesh_tcp.rs:67). Whether the transport later rejects an agent whose key does not match its claimed `agent_id` was not determined here (Undetermined; identity.rs derives `AgentId` by hashing the seed, identity.rs:167, lib.rs:245-249, so the id is not derivable from the public key by a verifier).

> Update 2026-11-21: the open question is answered and the id is now derivable. `AgentId::from_public_key` (first 16 bytes of SHA-256(`"spine-agent-id-v1\0"` || key)) is the id of every identity that signs mesh envelopes, and the resolver drops any `KeyspacePeer` referral, hello, ack or registration whose `agent_id` is not that derivation of its key (`ResolverMetrics::agent_id_mismatches`) before it reaches the peer map, the routing table or a transport. Referrals remain unsigned hints about endpoints; the `last_seen` and unbounded-peer-map observations above are unchanged.

### 8.5 Acknowledged replication: absent

- Replication sends `NameAnnounce` and counts completed sends (naming_mesh.rs:629-634). There is no acknowledgement message, no read-back verification, no retry accounting beyond periodic re-offer every `maintain_secs` (default 3600, config.rs:292) for at most 64 records per pass, and replicas are memory-only (naming.rs:430). The origin reports `durable: false` and `replication_acknowledged: 0` (main.rs:419-420); COMPLETION_STATUS.md lists acknowledged replica storage as an open gate (docs/COMPLETION_STATUS.md:84).
- A publisher cannot distinguish "peer stored it", "peer declined it for proximity or capacity", and "peer dropped it" (naming_mesh.rs:948-952 count locally only).

### 8.6 Other defects found while reading

| # | Finding | Reference |
| --- | --- | --- |
| a | Parser panic on non-ASCII input: resolved (section 2.8) | uri.rs: `parse`, `join` |
| b | Negative cache does not prevent repeated mesh walks (section 4.6) | main.rs:478, resolver.rs:317 |
| c | "Absent" reported when all known peers were unreachable (section 4.7) | naming_mesh.rs:855-857, main.rs:492-499 |
| d | Equal `(seq, published_at)` conflicting records did not converge: resolved by the signature tiebreak (section 3.6) | record.rs: `supersedes` |
| e | `Link.title` and `Link.capabilities` were outside the signature: resolved, signed since layout v2 (section 3.3) | record.rs: `signing_bytes` |
| f | Strings in `signing_bytes` were NUL-terminated: resolved, length-prefixed since layout v2 (section 3.3) | record.rs: `signing_bytes` |
| g | Cache accepts an older valid record over a newer one | cache.rs:140-154, resolver.rs:251 |
| h | Lookup does not check that a returned record is unexpired | naming.rs:319-329 |
| i | Resolved 2026-11-21: the envelope signature (v3) is taken over deterministic CBOR via `spine-cbor`, not `serde_json` output | spine-mesh mesh.rs `signature_data` |
| j | `verify` rather than `verify_strict`: resolved (section 3.4) | record.rs: `verify` |
| k | Record store capacity is count-based only; no byte or field-count limits | store.rs:39, record.rs |
| l | `host:` parsing treats a bare IPv6 address as host plus port | uri.rs:104-106 |

### 8.7 Undetermined items

| Item | Looked in |
| --- | --- |
| Whether a record is size-limited before JSON parsing at the origin or in mesh ingress | naming.rs, naming_mesh.rs, spine-name (no check found; frame limit 4 MiB in mesh_tcp.rs:62 only) |
| Who calls `LocalResolver::sweep` on a schedule | namespace.rs, main.rs |
| Whether the transport binds a peer's claimed `agent_id` to its public key | identity.rs, mesh_tcp.rs (mesh.rs:734 verifies only against the key the message carries) |
| Behaviour of capability lookup at network scale | naming_mesh.rs tests read only to function level |
| Behaviour when two names differing only in query are published to the persistent resolver | resolver.rs:198-240 |
| The wire encoding used by the QUIC and WebSocket mesh transports | mesh_tcp.rs only for TCP: JSON frames; other transport files not read |

## 9. Open issues

Each item was checked against the source when this section was written.

1. `Rel::Other("child")` signs identically to `Rel::Child` (link.rs: `Rel::as_str`; record.rs: `signing_bytes`). Deserialisation now normalises the alias (`Rel::other`), but the variant is public, so Rust code can still build `Other("child")`, which is then a distinct value that compares unequal to `Child` and signs the same. Making `Other` unconstructible (private field or a newtype) would close it.
2. The base32 decoder used to accept non-zero trailing bits, giving several spellings of one key. Fixed (base32.rs: `decode`); retained here because any other implementation MUST do the same, and `invalid_names` pins it.
3. The routing key has no domain separation: it is a bare SHA-256 of `spine://` + authority + path (uri.rs: `SpineUri::key`), in a keyspace shared with node ids that are raw Ed25519 keys (key.rs: `NameKey::from_bytes`). It excludes the query and fragment while the signed name (record.rs: `signing_bytes`) includes them, so records for names that differ only there collide in the store's by-key map (store.rs: `put`). The persisted row key is the full canonical name string (resolver.rs: `LocalResolver::publish`, `persistent`), unlike the in-memory key, so the two views of "one name" differ.
4. `capability_key(term)` in spine-mesh hashes `"cap:" + term` (naming.rs: `capability_key`) while `SpineUri::capability(term).key()` hashes `"spine://cap:" + term + "/"` (uri.rs: `SpineUri::key`). Two different points in the keyspace for one capability.
5. Lengths are written with `as u32` (record.rs: `signing_bytes`, `put_str`, `put_count`; lib.rs: `resource_id`), which truncates silently above 4 GiB (or above 2^32 elements). Not reachable for records that pass `validate_size`, but nothing forces that call.
6. `validate_size` is not called inside `sign` or `verify` (record.rs). A peer can sign an enormous record and `verify` will accept it; ingress code must call `validate_size` first (now `validate(now)`, which also applies the lifetime and clock bounds; the lifetime bound alone is inside `verify`).
7. One Ed25519 key signs name records, mesh envelopes (spine-mesh mesh.rs: `ENVELOPE_SIG_DOMAIN`, `spine-mesh-envelope-v2\0`) and the session handshake (spine-session handshake.rs: `LABEL_INITIATOR`, `spine-mesh-hs-v1/initiator`), separated only by those prefixes. Record and envelope prefixes both end in a NUL after a version; the handshake label has no NUL. The domains are distinct, but a single key compromise breaks all three and the prefixes are the only separation.
8. A `cap:` term is not validated beyond non-empty, no whitespace, no `/`; a `host:` is not validated as a host; an IPv6 literal without brackets mis-parses (8.6 l).
9. Names that differ only in query or fragment share a routing key and a store slot, so a tombstone for one revokes the others in the store, while the persisted publisher rows (one per full canonical name) are separate; owner renewal consults the store (`LocalResolver::is_revoked`) so it does not renew the others past the tombstone, but the rows remain on disk.
10. Revocation is not permanent. A tombstone lapses after at most 7 days (3.7) and the name is then unknown again, available to anyone who can sign for it. A permanent revocation needs a succession log (key rotation, 8.1), planned for a later stage. Until then an owner who wants a name to stay dead must renew the tombstone (publish a new one with a higher `seq`) at least every 7 days; the tombstone is the only record `renewed` refuses, so this means signing a fresh one with `spine name revoke`.
11. A tombstone is an ordinary record to the store's capacity rule: under pressure it can be evicted before it expires, after which a replica no longer blocks an older unexpired record, for at most that record's remaining lifetime.
12. `NameResolution` has no revoked variant: the daemon reports a revoked name as `Invalid` with a reason (3.8, decision 6).
13. A key holder can sign a lower-`seq` record dated after a tombstone; the replay argument in 3.7 assumes `seq` follows publication order.

