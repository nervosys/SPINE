# Handoff — Phase 38: The Namespace

**Status:** complete and verified. 1,355 tests passing, 0 failures, 0 Clippy warnings.
**Not committed.** Everything below is in the working tree only.

---

## Why this phase existed

SPINE had a transport, a content format, user agents, and a trust model. It did
not have a **namespace** — and without one, a stack is not a web.

Names are what let a resource be referred to independently of the machine
currently serving it, linked to by someone who has never met its operator, and
found by an agent that does not already know where it lives. Before this phase,
a SPINE resource could only be addressed by the socket address of whatever
happened to be hosting it. `Navigate { url }` fetched HTTP from the *human* web
via `reqwest`; the agent mesh could only reach peers whose addresses were
already known; and every discovery index (`OntologyRegistry`, the capability
marketplace) was a `DashMap` in a single process.

So SPINE was an excellent agent-native client for the human web, plus an RPC
mesh. This phase makes it a web of its own.

---

## What was built

~8,100 lines across 18 files. One new crate, three new modules in `spine-agentic`,
one in `spine-crypto`.

### `spine-name` — the namespace (new tier-0 crate)

| Module | What it does |
|---|---|
| `uri` | The `spine://` scheme: parsing, normalization, relative-reference resolution, serde |
| `record` | Signed `NameRecord` — endpoints, capabilities, content hash, links, metadata |
| `key` / `routing` | 256-bit XOR keyspace and Kademlia k-buckets |
| `link` / `frontier` | Typed edges and a deduplicating, budgeted crawl queue |
| `store` | Record store with a **capability index** alongside the keyspace index |
| `cache` / `resolver` | Stale-while-revalidate, negative caching, batch resolution |
| `base32` | RFC 4648 lowercase-unpadded — the wire spelling of every key |

Four authority kinds:

```
spine://did:<52-char base32 ed25519 key>/tools/search   self-certifying identity
spine://blob:<52-char base32 sha-256>/                  immutable, content-addressed
spine://cap:web.search/                                 an ability, not an endpoint
spine://host:node.example.org:9440/                     bootstrap escape hatch
```

### Federated resolution

- `spine-agentic::naming` — Kademlia lookup as pumpable state machines. No sockets,
  so convergence is testable without a network.
- `spine-agentic::naming_mesh` — the live driver: dispatch, request/response
  correlation, timeouts, unreachable-peer fallback, and the `NameKey` ↔ `AgentId`
  bridge between the two routing spaces.

### Transports (the mesh had **none** before this phase)

| Module | Model |
|---|---|
| `mesh_tcp` | One pooled byte stream per peer. Length-prefixed framing, transparent single re-dial. |
| `mesh_ws` | Same, over a WebSocket upgrade. Shares the stream-generic code path. |
| `mesh_quic` | One connection per peer, **one stream per exchange**. |

### `spine-crypto::handshake`

ML-KEM-768 (FIPS 203) + Ed25519 signed-ephemeral handshake, AES-256-GCM frames
with per-direction keys and counter nonces.

### Protocol + origin

`ResolveName`, `ResolveNames` (batched), `FindProviders`, `PublishName`,
`FetchName`, `CrawlNames` — served by `spine-core`, which is now an *origin* in
the agent web rather than only a proxy onto the human one.

---

## Where things live

Every file below is new except the four marked *(edited)*.

| Path | Lines | What to expect |
|---|---:|---|
| `src/spine-name/src/uri.rs` | 567 | The scheme. Parsing, normalization, `join()`, serde. |
| `src/spine-name/src/record.rs` | 385 | `NameRecord`, canonical signing bytes, `supersedes`. |
| `src/spine-name/src/cache.rs` | 369 | Stale-while-revalidate, negative caching, eviction. |
| `src/spine-name/src/frontier.rs` | 364 | Crawl queue: dedup, budgets, skipped-with-reason. |
| `src/spine-name/src/resolver.rs` | 340 | `Resolver` trait, `LocalResolver`, provenance. |
| `src/spine-name/src/store.rs` | 313 | Keyspace index + capability index. |
| `src/spine-name/src/routing.rs` | 298 | k-buckets, freshness-aware eviction. |
| `src/spine-name/src/link.rs` | 177 | `Rel` and traversal priorities. |
| `src/spine-name/src/lib.rs` | 167 | Crate docs — **start here**. |
| `src/spine-name/src/key.rs` | 162 | XOR metric, bucket index. |
| `src/spine-name/src/base32.rs` | 136 | RFC 4648 lowercase-unpadded. |
| `src/spine-name/examples/agent_web.rs` | 120 | Runnable end-to-end demo. |
| `src/spine-agentic/src/mesh_tcp.rs` | 1300 | Framing, pooling, encryption mode, TCP listener. |
| `src/spine-agentic/src/naming.rs` | 992 | `Lookup` state machine, `NameService`. |
| `src/spine-agentic/src/naming_mesh.rs` | 695 | The live driver and the two-routing-space bridge. |
| `src/spine-crypto/src/handshake.rs` | 715 | ML-KEM + Ed25519 channel. Read the module docs first. |
| `src/spine-agentic/src/mesh_quic.rs` | 595 | Stream-per-exchange QUIC. |
| `src/spine-agentic/src/mesh_ws.rs` | 444 | WebSocket establishment. |
| `src/spine-parser/src/lib.rs` | *(edited)* | `Element::AgentLink`, `agent_links()`. |
| `src/spine-protocol/src/lib.rs` | *(edited)* | Namespace commands, `NameResolution`. |
| `src/spine-core/src/main.rs` | *(edited)* | Handlers; `BrowserState.names`. |
| `src/spine-agentic/src/mesh.rs` | *(edited)* | Three `MeshPayload` variants, envelope builders. |

**Reading order if you are new to it:** `spine-name/src/lib.rs` for the concepts,
then `examples/agent_web.rs` to see them used, then `naming.rs` for how
resolution decides, then `naming_mesh.rs` for how it runs.

Every module carries a docs header explaining *why* it is shaped the way it is —
those headers are the real design record, and are more current than this file
will be a month from now.

---

## Three design decisions worth knowing

**1. Names certify themselves.** A `did:` authority *is* an Ed25519 public key, so
a record verifies against the name with no CA in the resolution path. Crucially,
it is the *same* key that positions the node in the DHT keyspace and authenticates
the transport handshake — so "the peer I dialed", "the node responsible for this
key", and "the peer whose records I trust" are one fact, not three that must be
kept in sync.

**2. Capabilities are addressable.** `cap:web.search` names an ability and resolves
to ranked providers. Capability queries hash the term itself, so providers cluster
at one keyspace point and "who can do X" is a *routed* question, not a broadcast.
The human web could only express "what is at this host", which is why it needed
centralized search engines to answer the other question.

**3. Replies go back on the arriving connection.** Never by dialing the asker back.
This is what makes resolution work through NAT — the asker may have no dialable
address at all. Routing replies through an address book would also require every
responder to be pre-introduced to every possible asker.

---

## Bugs found by integration

Each of these passed its own unit tests and only appeared when layers were
connected. Listed because they say something about where the remaining risk is.

**Lookups discarded in-flight answers.** A capability query settled the moment its
shortlist emptied, even with requests outstanding. Two providers on two peers →
the faster peer's reply ended the lookup and the second was silently dropped. A
wrong answer that looked complete. Fixed with in-flight tracking.
→ `a_lookup_waits_for_peers_still_in_flight_before_declaring_itself_done`

**Local records were invisible to `resolve()`.** The lookup went straight to the
keyspace without checking the node's own store, so a node could not resolve a name
it had published itself. Added `NameService::resolve_locally`, checked before any
walk — also the hot path once a cache is warm.

**Replies had no return path.** The responder tried to dial the asker back through
an address book it had never been told about. Not fixable by introducing peers
bidirectionally — it is wrong in principle and impossible through NAT. Fixed
structurally: every connection is symmetric.
→ `the_responder_answers_without_knowing_how_to_dial_the_asker`

**α-waves were serial.** `dispatch` awaited each peer's send in turn, so Kademlia's
α parameter — which exists precisely to overlap slow peers — did nothing. Every
lookup paid the *sum* of its peers' latencies instead of the max. Invisible over
loopback TCP; only surfaced when QUIC's connect latency made it measurable. Now
concurrent, which speeds up all three transports.

**rustls 0.23 panics without a crypto provider.** Not a test artifact — it would hit
any QUIC user as a panic three layers down that never names the cause. Now
installed lazily via `Once`.

---

## Verification

```
cargo test --workspace          1,355 passed / 0 failed / 5 ignored
cargo clippy --workspace --all-targets    0 warnings
cargo run -p spine-name --example agent_web
```

Session start was 1,135 tests. The example publishes three signed records, finds
a tool by capability, resolves a name, batch-resolves, and crawls the graph —
walking `requires` before `peer`, as the relation priorities intend.

Network tests use real OS-assigned ports and cover the failure modes, not just the
happy path: oversized frames refused before allocating, truncated and corrupt
frames rejected, a down peer that does not hang the lookup, a forged record
rejected after crossing the wire, a tampered frame dropping its connection, and a
hostile client sending garbage that closes only its own connection while a
well-behaved peer resolves normally.

---

## Known limits — read before building on this

**The handshake is bespoke crypto without external review.** The primitives are
vetted (FIPS 203 ML-KEM, Ed25519, AES-GCM, HKDF) and the arrangement is
conventional, but the composition has not been reviewed by anyone but me. It is
appropriate for a mesh whose payloads are independently signed. It is **not** a
substitute for TLS where the transport is the only thing between an adversary and
unauthenticated data. One accepted limitation is documented in the module: a
recorded `InitiatorHello` can be replayed to a responder, which gains the attacker
nothing (no ephemeral key → no session key → no readable or writable frame), but
eliminating it would need a third message.

**QUIC endpoint certificates authenticate nothing.** `QuicEndpointBuilder` uses
self-signed certs with a permissive verifier. Confidentiality is real;
endpoint authentication comes from the mesh handshake, which must be enabled
explicitly via `QuicNameTransport::authenticated`. `QuicNameTransport::new()`
gives you an *unauthenticated* peer — fine, because envelopes are signed, but know
which one you are constructing.

**Bootstrap is still manual.** `register_peer` + `set_address` must be called with
known peers. There is no seed-node discovery, no DNS bootstrap, no rendezvous. The
DHT converges fine once it has *any* entry point; getting the first one is not
solved.

**No record replication or republication.** `needing_republish` reports what is
about to lapse, and `is_responsible_for` says whether this node is among the K
closest to a key — but nothing acts on either. Records live only where they were
published, so a node going down takes its names with it.

**`FetchName` resolves but does not fetch.** It returns the record and its ranked
endpoints; dialing them is left to the caller. This is deliberate — conflating
resolution and transfer would hide which half failed — but it means there is no
one-call "give me the bytes at this name".

---

## Suggested next steps, in order

1. **Bootstrap discovery.** The DHT is useless without an entry point. A seed-node
   list in `spine.toml` plus a `host:` authority resolver is probably enough.
2. **Replication.** Store each record at the K closest nodes and republish before
   TTL expiry. The predicates already exist; they need a background task.
3. **Get the handshake reviewed** if the transport will ever carry anything whose
   safety does not rest on the signed envelopes inside it.
4. **Wire the namespace into the gateway** so `spine://` names are resolvable over
   plain HTTPS, which is the cheapest possible interop win given Interop is the
   self-assessed weakest axis.

---

## Housekeeping

- **Nothing is committed.** The tree also carries pre-existing uncommitted changes
  to `.gitignore` and `Cargo.lock` from before this work started.
- **`spine-cli` is still unpublished** to crates.io — the old publish run hit a 429
  rate limit at the last crate in the order. One re-run finishes it.
- **Docs updated:** README (namespace section, transport table, crate list),
  ROADMAP (Phase 38, header counts, license corrected from Apache 2.0 to
  AGPL-3.0-or-later), PUBLISHING.md (ordering — `spine-name` is tier 0,
  `spine-parser` moves after it).
- **I deleted `target/debug/incremental`** (47.7 GB) mid-session when repeated
  builds took the disk from 71 GB free to zero and the test runner started
  failing with ENOSPC. It is regenerable compiler cache; I chose it over
  `target/debug/deps` specifically to avoid forcing a full dependency rebuild.

---

## Reproducing the verification

```bash
cargo test --workspace                     # 1,355 passed / 0 failed / 5 ignored
cargo clippy --workspace --all-targets     # 0 warnings
cargo run -p spine-name --example agent_web
```

The network suites bind real OS-assigned ports on `127.0.0.1`, so they need no
fixtures and no external services, but they will contend with a firewall that
blocks loopback. QUIC additionally installs a process-wide rustls crypto
provider on first use; if something else in the process installs one first, that
one is used and QUIC still works.
