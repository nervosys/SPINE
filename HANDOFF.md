# Handoff — Phase 38: The Namespace

**Status:** complete and verified. 1,404 tests passing, 0 failures, 0 Clippy warnings.
**Committed** on branch `phase-38-namespace`, which is not merged into `master`.
Phases 39 (bootstrap), 40 (replication), 41 (HTTP interop), 42 (handshake
randomness), and 43 (maintenance cost) followed on the same branch and have their
own sections near the end of this file.

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

**~~Bootstrap is still manual.~~** *Solved in Phase 39 — see below.* Seeds are
configured in `spine.toml`, dialed by address, and prove their own identity. What
is still missing is *automatic* discovery: no DNS bootstrap, no rendezvous, no
peer exchange beyond what a seed hands back at greeting time.

**~~No record replication or republication.~~** *Solved in Phase 40 — see below.*
Records are stored at the K closest nodes and re-offered periodically. What is
still missing is **renewal**: a record's expiry is signed into it, so nothing but
the holder of its signing key can extend a name's life. A node whose owner stops
re-signing loses its names on schedule, by design, and the maintenance pass only
reports the coming lapse.

**`FetchName` resolves but does not fetch.** It returns the record and its ranked
endpoints; dialing them is left to the caller. This is deliberate — conflating
resolution and transfer would hide which half failed — but it means there is no
one-call "give me the bytes at this name".

---

## Phase 39 — bootstrap (done, on branch `phase-38-namespace`)

Step 1 below turned out to be larger than "a seed list", because the DHT had a
second defect hiding behind the first: **referrals carried no mesh identity**, so
a peer learned mid-lookup could be placed in the shortlist and never addressed.
Multi-hop lookups therefore did not work over *any* real transport — the
in-process test harness pre-registered every node, which is why the unit tests
passed. A lookup could only reach peers introduced by hand.

Both are fixed together, since both come from the same split between "knowing a
peer" and "being able to reach one":

- `KeyspacePeer` pairs a keyspace position with the `AgentId` needed to address it
- `NameTransport::learn` teaches the transport's address book what the resolver
  discovers, at every point a peer is learned
- `NameHello`/`NameHelloAck` establish identity from an address, verified against
  the key the message carries
- `NameTransport::send_to` dials by address on TCP, WebSocket, and QUIC
- `[namespace]` in `spine.toml`, a persisted node key, and `spine-core` joining
  the mesh on startup with fall-through resolution

New tests worth knowing about: `a_lookup_reaches_a_peer_it_was_never_introduced_to`
(the defect above, over real sockets), `a_node_bootstraps_into_the_mesh_from_an_address_alone`,
and `a_hello_whose_signature_does_not_match_its_key_is_refused`.

One bug the tests caught during development: a seed included *the newcomer
itself* in the contacts it referred back, because the newcomer had just been
added to its routing table and is by definition the closest entry to its own key.
Fixed in `peers_for_newcomer`.

---

## Phase 40 — replication (done, on branch `phase-38-namespace`)

Step 2 below. The gap was not only that nothing replicated, but that nothing
*could*: publishing broadcast to whatever peers the publisher was connected to,
and the DHT had no way to ask "which nodes are closest to this key" at all. A
name lookup cannot answer that — it stops the moment any node hands back the
record, which is usually long before the walk has converged.

- `ResolveQuery::Node` — Kademlia's FIND_NODE, the walk with no early answer to
  stop on. `LookupOutcome::Closest` carries the converged shortlist.
- `MeshNode::announce_name_to` — the directed store. Copies go where the record
  belongs, not where the publisher happens to have a connection.
- `MeshNameResolver::replicate` / `find_node` / `maintain`, and
  `ReplicationReport` / `MaintenanceReport`
- `spine-core` runs a maintenance task on a `maintain_secs` interval (default
  3600), re-offering held records and dropping expired ones

Two decisions worth knowing about:

**A broadcast does not count as a replica.** A node with no addressable keyspace
peers still broadcasts, because warming a cache is better than nothing, but
`ReplicationReport::replicas()` reports zero. The peers a broadcast reaches were
not chosen for their position, so nothing about durability follows from it, and
counting them would let a single-node mesh report itself as durable.

**Maintenance re-offers; it does not renew.** Re-announcing cannot move an expiry
that is signed into the record. Conflating the two would produce a node that
looks like it is keeping names alive and is not. Lapsing names come back in the
report instead.

Tests worth knowing about:
`a_published_record_is_served_by_a_node_that_did_not_publish_it` (over real
sockets — the point of the whole phase), `a_replica_reaches_a_node_the_publisher_never_met`,
and `maintenance_re_offers_a_record_to_a_peer_that_arrived_later`.

One test-writing note: `publish` returns once the copy is on the wire, and an
announcement has no ack, so asserting on the holder immediately after publishing
races the delivery. The TCP test polls rather than sleeping a fixed interval.

---

## Phase 41 — HTTP interop (done, on branch `phase-38-namespace`)

Step 4 below: the namespace over plain HTTP, which was the cheapest interop win
available. `/v1/names/{resolve,providers,publish,endpoints,crawl}` on
`spine-gateway`, plus the `AgentClient` methods they needed — the protocol
carried these commands and `spine-core` served them, but the SDK client had no
way to send one, so the gateway had nothing to call.

Two things worth knowing:

**Namespace outcomes are mapped onto HTTP status codes rather than stuffed into
a 200 body.** Resolved is `200`, unchanged is `304` with the TTL in
`Cache-Control`, unpublished is `404`, malformed is `400`. An HTTP client's cache
and retry logic is then correct without knowing anything about SPINE.

**Provenance survives translation, including the unattested kind.** Every
resolution reports where it came from and whether anything vouched for it. A
`host:` name's address was read out of the name and nobody signed it; returning
it shaped like a signed binding would launder exactly the distinction the
namespace's trust model rests on.

**A real defect found on the way: `PublishName` never left the node.** It wrote
to the local resolver only, so a record published over the protocol was
resolvable on one machine and findable nowhere — invisible until Phase 40 gave
it something to be measured against. It now replicates and reports the count.

The endpoints deliberately take no session: resolution is stateless, and making
a caller create one, resolve, and tear it down is three round trips for a
one-round-trip question. The cost is a backend connection per request, which is
worth revisiting if these become hot.

---

## Phase 42 — the handshake's randomness (done, on branch `phase-38-namespace`)

Step 3 below was "get the handshake reviewed", which I cannot do. Preparing the
package that makes it possible found a defect first.

**The ephemeral ML-KEM keypair was generated from a counter.**
`Initiator::start` and `Responder::accept` took a `seed: u64` and built an RNG
with `StdRng::seed_from_u64(seed)`. The TCP and WebSocket listeners passed a
counter starting at 0, incremented per accepted connection; dialers passed a
configured constant plus the connection-pool size. So the Nth connection a node
accepted after start-up always used the same ephemeral keypair, and anyone who
could count connections — or try small integers — could regenerate the
decapsulation key, recover the shared secret, and read the session.

Forward secrecy was nominal. The keypair was ephemeral in lifetime but not in
value, and the long-term-key-compromise argument the module makes was true and
irrelevant, because the ephemeral key was the thing worth attacking.

Fixed by generating from `OsRng` on both sides, with `start_with_rng` /
`accept_with_rng` as a deterministic seam for tests — bounded on `CryptoRng` so
the seam cannot become the production path by accident. The `seed` parameter is
gone from all three transports and from `EncryptionConfig`.

**Why the tests did not catch it.** `separate_connections_derive_independent_keys`
existed and passed. It handed the two handshakes different constants, so it only
ever proved that different seeds produce different keys — never that a key was
unpredictable. Two new tests assert the property that was actually wanted:
`every_handshake_generates_a_fresh_ephemeral_key` and
`a_responder_draws_fresh_randomness_for_every_connection`.

`HANDSHAKE-REVIEW.md` is the review package: protocol description, threat model,
claimed properties, the concessions, and the specific questions a reviewer should
answer. It documents this defect too, because it says where the remaining risk
most likely is — the composition is conventional and the primitives are vetted,
so what is left is primitives being fed something they do not require, and tests
that assert something adjacent to the property they name.

---

## Phase 43 — bounding what maintenance costs (done, on branch `phase-38-namespace`)

A defect in my own Phase 40 work. `maintain` iterated every record in the store
and called `replicate` on each, and `replicate` runs a full keyspace walk. So a
node holding a thousand records ran a thousand walks per tick, and did it for
records it was only holding a drifted copy of — spending a walk to tell the
rightful holders something they already had. Every node behaving that way
multiplies mesh traffic by the number of stale copies in circulation.

`is_responsible_for` had been sitting in `naming.rs` since Phase 38, written for
precisely this question, with no caller. Now `records_to_maintain` uses it, and
also drops already-expired records, which were being re-offered pointlessly.

Passes are bounded by `MaintenancePolicy::max_records` (default 64) and resume
after the last key handled, so a store larger than the budget is covered across
ticks rather than having its tail starved. `deferred` and `not_ours` are in the
report and in the log line: a bounded pass must not read as an exhaustive one,
and `not_ours` climbing is how an operator learns a node is holding copies
nothing will ever ask it for.

**One caveat on the verification.** Across the runs for this phase, one workspace
run reported a single failure. It did not reproduce in seven subsequent runs
(four of `spine-agentic`, three of the other network-heavy crates, plus full
workspace runs), and I did not capture the test name before it vanished — so I
cannot say which test it was or rule out a genuine flake. The network suites bind
real loopback ports and the disk was near full at the time, both of which make
environmental failure plausible, but that is a hypothesis and not a finding.

---

## Suggested next steps, in order

1. ~~**Bootstrap discovery.**~~ Done — see above.
2. ~~**Replication.**~~ Done — see above.
3. **Get the handshake reviewed** — still outstanding, and now possible to
   commission: `HANDSHAKE-REVIEW.md` is written for someone with no knowledge of
   this codebase. Phase 42 fixed one defect in the construction; that it existed
   at all is the argument for paying for the review rather than skipping it.
4. ~~**Wire the namespace into the gateway.**~~ Done — see above. What remains
   there is a connection pool for the backend hop, and TLS termination in front
   of the gateway if these endpoints are exposed publicly.
5. **Find the flaky test** described at the end of Phase 43, or establish that it
   was environmental. A test suite with one unidentified intermittent failure is
   a suite whose green runs mean slightly less than they should.

---

## Housekeeping

- **Phases 38-40 are committed on `phase-38-namespace`**, which is not merged into
  `master`. Each phase is a coherent group of dependency-ordered commits.
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
- **The disk is the binding constraint on this machine.** It has since filled
  twice more; Phase 40 hit `LNK1318` (a PDB write failure that is really ENOSPC
  wearing a linker's clothes) with 637 MB free, and clearing
  `target/debug/incremental` again was what unblocked it. Budget a few GB before
  a full workspace build, and expect to clear that directory rather than
  `target/debug/deps`.

---

## Reproducing the verification

```bash
cargo test --workspace                     # 1,404 passed / 0 failed / 5 ignored
cargo clippy --workspace --all-targets     # 0 warnings
cargo run -p spine-name --example agent_web
```

The network suites bind real OS-assigned ports on `127.0.0.1`, so they need no
fixtures and no external services, but they will contend with a firewall that
blocks loopback. QUIC additionally installs a process-wide rustls crypto
provider on first use; if something else in the process installs one first, that
one is used and QUIC still works.
