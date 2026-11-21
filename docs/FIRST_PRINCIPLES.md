# SPINE from first principles

Revised 2026-11-20. The goal is fixed: **an entirely new web stack for AI agents, built from the ground up, without regard to the existing install base.** The first version of this document (2026-11-19) assumed adoption was the goal and recommended riding HTTP, MCP and A2A. That assumption was wrong and those recommendations are withdrawn; what replaces them is below.

This is an assessment and a recommendation. It draws on the repository as of `completion/agent-web`.

## Conclusion

A clean slate removes one constraint, compatibility. It does not remove the question of what a web for agents has to do differently from a web for people. Judged on that question, SPINE today has the right foundations in two layers, a gap where the most important layer should be, and a large amount of code that is not part of a web stack at all.

- **Right:** names that verify themselves, content addressed by hash, signed records, signed durable tasks, bounded resource use everywhere.
- **Missing:** authority. The human web bolted identity and permission on afterwards and never recovered. An agent web's defining feature should be that every action carries proof of who delegated it and what it may do. SPINE has none of this yet.
- **Misdirected:** the native wire is justified by throughput, which agents cannot use, and not by the semantics agents need. The channel rests on an unreviewed bespoke handshake. Roughly half the workspace is research that sits beside the stack, not in it.

The recommendation is to define SPINE as seven layers with a narrow waist, write that waist down as a specification, and build outward from it.

## What an agent needs that a person does not

| # | Premise | What it demands of the stack |
| --- | --- | --- |
| 1 | The reader is a model. Its budget is tokens and attention. | Content is typed and compact, declares its size before transfer, and can be requested at a budget. |
| 2 | An agent acts for someone else: software, operator, and principal are three parties. | Authority is delegated, narrowed, time-limited, revocable and carried in every request. |
| 3 | Everything fetched is untrusted input to a system that can act. | Data can never carry authority. Only a capability holder can issue an instruction, and provenance travels with content into the model's context. |
| 4 | Work is a task, not a request. It runs for seconds to days and outlives its connection. | Tasks, streaming, cancellation and resumption are primitives, not conventions. |
| 5 | Agents take actions with consequences. | Every operation declares its effect class (read, reversible, irreversible) and cost, supports dry-run and idempotent retry, and returns a signed receipt. |
| 6 | Agents look for abilities, not addresses. | Discovery is by capability, and resists spam and Sybil identities by construction. |
| 7 | Agents operate at machine speed. | Limits, quotas and metering are part of the protocol; cost is a first-class field. |
| 8 | Inference is the bottleneck: tens to thousands of tokens per second. | Transport is optimised for time to first token, cancellation latency, resumption and mobility. Bulk throughput matters only for tensors. |
| 9 | Agents move code as well as data. | Tools can ship as sandboxed, metered, capability-confined modules. |

Premises 3 and 5 are where a ground-up design can do what the existing web cannot. HTTP has no way to say "this body is data and may not instruct you", and no way to say "this call cannot be undone". A new stack can make both structural.

## The stack

The waist is layers 2 to 4: names, authority and objects. Everything above and below can be replaced without breaking a peer.

| Layer | Design | SPINE today |
| --- | --- | --- |
| **1. Session** | Connect to a key, not to a host. Multiplexed streams with no head-of-line blocking, credit-based backpressure, stream cancel with a reason code, session resumption and migration across networks. Channel security from a published, formally modelled construction (a Noise pattern with hybrid ML-KEM), authenticated by the name itself with no certificate authority. | TCP, WebSocket and QUIC adapters. A bespoke handshake that `HANDSHAKE-REVIEW.md` lists as unreviewed, and signed coordination depends on it. QUIC is compiled but not exercised. Tamarin and TLA models exist in `formal/` but are not recorded as executed. |
| **2. Names** | Self-certifying identity that survives key change: the name commits to an inception key and a signed succession log, so rotation and revocation do not end the name. Content names are hashes. Human-meaningful labels are attestations layered on top, never part of the trust path. | `did:` is a raw Ed25519 key; `blob:` is a SHA-256 hash; `cap:` names an ability. `spine-name` has no rotation or revocation: a lost key ends the name. |
| **3. Authority** | Object capabilities. A request carries a chain from principal to operator to agent; each link can only narrow scope, shorten lifetime or lower a spend limit. Revocation is checkable offline within a stated staleness bound. There is no ambient authority anywhere. | **Absent.** Node identity, a coordinator allowlist, and operator-level access for every client connected to the native server. |
| **4. Objects** | Typed, schema-identified, content-addressed objects linked into a graph. Every object carries a signed provenance label and a declared size in bytes and approximate tokens. A fetch can ask for a representation within a budget. Objects are data only: the type system has no way to express an instruction to the reader. | Signed `NameRecord`s, typed links, verified chunked transfer up to 8 MiB. Provenance stops at the transfer boundary and never reaches the model. No budgeted representations. |
| **5. Interaction** | Tasks with offer, accept, progress stream, cancel, resume and signed receipt. Tools are typed interfaces declaring effect class, cost and idempotency. Irreversible effects require a capability that says so, which is where human approval attaches. | Signed, recipient-bound, durable tasks with dependencies, deadlines and restart recovery. Two read-only tools. No effect classes, no receipts beyond completion, no dry-run. |
| **6. Discovery** | Capability lookup returning signed provider records plus attestations. Ranking is computed by the caller's own trust policy, not by the network. Publishing costs something: a vouch, a stake or proof of work. | Kademlia lookup of `cap:` names. No admission cost or attestation; the threat model's Sybil analysis covers swarm voting, not the namespace. |
| **7. Execution** | Mobile code runs in a WebAssembly sandbox with only the capabilities passed to it, metered in fuel and memory, with deterministic replay for audit. | `spine-wasm` and `spine-compiler` exist with tests. Not connected to tasks or authority. |

Two things sit outside the stack on purpose.

- **A gateway to the old web.** A new web with nothing on it is empty on day one. A gateway that ingests HTML into layer-4 objects, with provenance marking them as unsigned third-party content, gives agents something to read. `spine-parser` and the outbound fetch policy are that gateway. This is bootstrapping, not compatibility: nothing in the stack depends on it.
- **Bridges for existing clients** (the OpenAI-compatible gateway, MCP, gRPC). Useful for demonstration. They should not shape any layer.

## What I would change in the existing design

1. **Rejustify the wire.** A native framing is now clearly in scope. Its case should be the semantics in layers 1, 3 and 5: capability chains in the frame header, cancel and resume, effect classes. The README leads with 300–550M tokens per second on loopback, which is five orders of magnitude beyond any model. Replace the headline measures with time to first token, cancel latency, resumption after network change, and bytes per tensor.
2. **Replace the bespoke handshake with a standard construction and get it reviewed.** A new stack makes this more necessary, not less: there is no TLS underneath to fall back on. Choosing a Noise pattern narrows the review to composition and integration, and makes the Tamarin model tractable. The independent review in `HANDSHAKE-REVIEW.md` stays a release blocker.
3. **Build authority before anything multi-party.** Sessions, tasks, publication and discovery all need it, and retrofitting it is what went wrong on the human web. This is the single largest piece of new work.
4. **Give names a life cycle.** Rotation and revocation, then attestations for human-meaningful labels.
5. **Carry provenance into the model.** The parser should emit spans tagged with their source object, and the SDK should hand those tags to the caller, so a model's context records where each passage came from and that none of it is an instruction.
6. **Write the waist down.** Layers 2 to 4 as a specification with test vectors, independent of the Rust code. A stack nobody else can implement is a program, not a web. Without an install base to inherit, the specification is the only route to a second implementation.
7. **Cut the stack from the research.** A ground-up stack needs a small core that can be reviewed.

## What this means for the code

The workspace is about 122,000 lines of Rust in 30 crates.

- **The stack:** `spine-protocol`, `spine-transport`, `spine-stream` (layer 1); `spine-name` (2); a new authority crate (3); `spine-core` resources, `spine-storage`, `spine-cache` (4); the signed RPC and task parts of `spine-agentic` and `spine-core` (5); the naming mesh (6); `spine-wasm`, `spine-compiler` (7); `spine-web`, `spine-agent`, `spine-cli` as the client surface.
- **Edge:** `spine-parser` and the outbound policy (gateway); `spine-gateway`, `spine-grpc`, the MCP adapter (bridges).
- **Not part of a web stack:** `spine-neural`, `spine-recursive`, `spine-cluster`, `spine-human`, `spine-mechgen`, `spine-kernel`, `spine-gpu`, `spine-k8s`, `spine-browser`, the swarm, game-theory and marketplace parts of `spine-agentic` (which is 33,500 lines, over a quarter of the total), and the RLWE and Chameleon research in `spine-crypto`. Whether these move to a separate workspace or stay as clearly marked experiments is a decision below. They currently cost build time, review surface and advisories: 34 dependency vulnerabilities arrived in seven weeks.
- **`spine-nostd` and `spine-embedded`** are worth keeping only if constrained devices are meant to be first-class peers. If so, the waist must be specified to fit them, which argues for deciding early.

`COMPLETION_PLAN.md` needs revising to match. Stages 0 to 2 stand. Stage 4's authorization items move to the front and grow into layer 3. Stage 3's replication and discovery items stay but gain the Sybil and attestation work. Stage 6's interoperability work shrinks to the bridges. A specification stage is new.

## What I am not sure of

- **How far down "ground up" should go.** Loss recovery and congestion control are the same problem for agents as for anyone, and getting them wrong is costly. My recommendation is to own everything from the session layer up and run it over QUIC's packet machinery with SPINE's own key-based authentication, as `quinn` is already in the tree. Writing a new reliable transport over UDP is possible but I see no agent-specific requirement that needs it.
- **Whether a DHT is the right substrate for discovery.** It gives decentralisation at the price of Sybil exposure and unprovable absence. An admission cost may fix the first; nothing fixes the second. This needs a design study.
- **Whether offline revocation can meet a useful staleness bound** without an always-reachable authority. This is the hard problem in layer 3.
- **Whether budgeted representations and provenance tags improve agent outcomes.** Both are plausible and unmeasured. They need an evaluation with real models before the object model is frozen.
- **Token counts are tokenizer-specific.** A declared "approximate tokens" field needs a neutral definition or it becomes a per-vendor field.

## Decisions that are yours

1. **Transport depth:** SPINE's session layer over QUIC's packet machinery (recommended), or a transport written from UDP up.
2. **Research code:** move it to a separate workspace (recommended), or keep it in-tree behind an experimental marker.
3. **Constrained devices:** first-class peers, or out of scope for the first specification.
4. **Licence for the specification.** The implementation is AGPL-3.0. If others are meant to implement the stack, the specification and its test vectors need terms that permit that.
