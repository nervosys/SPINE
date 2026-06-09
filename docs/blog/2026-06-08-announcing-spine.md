---
title: "Announcing SPINE: An Agentic-First Web Stack — and an Honest Way to Measure One"
date: 2026-06-08
author: Nervosys
tags: [agentic-web, llm-agents, protocols, benchmarks, open-source]
---

# SPINE: building — and measuring — the agentic web

The web was built for people reading documents. Agents aren't people, and they
aren't reading. They stream tokens, call tools, advertise capabilities, ship
embeddings, and coordinate in swarms — and they do all of it over a transport
stack designed for browsers fetching HTML.

Today we're announcing **SPINE** (*Synaptic Pathways INterconnecting Entities*),
an **agentic-first web stack** that treats the things modern LLM agents actually
need as first-class wire primitives rather than JSON glued onto HTTP after the
fact. Alongside it, we're shipping a major update to **agentic-eval**, our
open-source benchmark for scoring how well a web stack fits agentic use.

Both are open source. SPINE is dual-licensed under **AGPL-3.0-or-later** with a
commercial option; agentic-eval is AGPL-3.0-or-later.

---

## What SPINE is

SPINE is a Rust workspace (28 crates) that makes agent-native operations
first-class `Message` variants at the wire level:

- **Token streaming** — `StreamStart` → `StreamToken { seq, data, usage? }` →
  `StreamEnd`, where `StreamData` carries `Text | Bytes | ToolCall |
  Encoded(EncodedFrame)`. Latents and mid-stream function calls fall out of the
  same frame. Multiplex-aware `StreamCancel` cancels one stream by id instead of
  bluntly closing the socket like SSE.
- **Tool calling (MCP-shaped)** — `ToolCall → ToolResult`, mapping cleanly to
  Anthropic MCP and OpenAI function calling.
- **Capability discovery** — a native `CapabilityQuery` (exact / prefix /
  semantic-by-embedding / all) → `CapabilityAdvertisement` with JSON Schema per
  capability.
- **Distributed tracing** — W3C `TraceContext` attached inline on tool calls,
  results, and stream starts.
- **A neural encoder-decoder protocol** — self-describing `EncodedFrame`s that
  carry a codec id, modality, shape, and dtype inline, so a latent is its own
  schema.

And because the rest of the world speaks other protocols, SPINE ships **three
deployable bridges**: a runnable MCP stdio server, an OpenAI-compatible
`/v1/chat/completions` + `/v1/embeddings` gateway, and a reflection-enabled gRPC
`AgentService`. A standard MCP, OpenAI, or gRPC client can drive a SPINE agent
today with stock stubs.

---

## agentic-eval: scoring a stack for agents, not browsers

[agentic-eval](https://github.com/nervosys/AetherShell) ranks seven web stacks —
SPINE, the OpenAI API, the Anthropic API, MCP, gRPC, plain HTTP+JSON, and
GraphQL — on five agent-native axes:

1. **streaming** — is LLM-shaped output a first-class frame, or a bolt-on?
2. **tool-discoverability** — can an agent introspect capabilities from the
   protocol, or must it read prose?
3. **encoding-efficiency** — wire compactness for the LLM/tool-call workload.
4. **interop** — does the agent ecosystem actually speak this? (Network effects
   are real and we score them honestly.)
5. **security-primitives** — auth, tracing, integrity, and per-message identity
   carried by the protocol itself.

Composite fitness is the unweighted mean of the five. Here's where the stacks land:

| Stack         | Fitness | Streaming | Tools | Encoding | Interop | Security |
| ------------- | ------- | --------- | ----- | -------- | ------- | -------- |
| **SPINE**     | **0.90** | 0.98     | 0.95  | 0.95     | 0.67    | 0.95     |
| gRPC          | 0.83    | 0.70      | 0.85  | 0.95     | 0.85    | 0.80     |
| openai-api    | 0.69    | 0.85      | 0.70  | 0.35     | 1.00    | 0.55     |
| anthropic-api | 0.66    | 0.85      | 0.70  | 0.35     | 0.85    | 0.55     |
| graphql       | 0.60    | 0.50      | 0.95  | 0.35     | 0.75    | 0.45     |
| mcp           | 0.56    | 0.40      | 0.95  | 0.40     | 0.65    | 0.40     |
| http-json     | 0.54    | 0.55      | 0.40  | 0.30     | 1.00    | 0.45     |

SPINE leads the composite, edging gRPC by +0.07 and the OpenAI API by +0.21. It
is strongest on the axes it was designed for and at protobuf-parity on encoding.

**And here's the axis we did *not* dress up: interop, at 0.67 — SPINE's weakest
score.** The bridges map the agentic *surface*, not SPINE's native binary frames,
and a brand-new protocol has ~zero native install base. The OpenAI API scores a
perfect 1.00 there for a reason. Closing that gap is a publish-and-get-users
problem that no amount of code in the repo can fake.

---

## The numbers — and how we got honest about them

SPINE is fast, and we can show it against a *real* modern protocol. All figures
below were re-measured on 2026-06-08 against the actual `h2` HTTP/2 crate and
real `serde_json`, on TCP loopback — not hand-rolled baselines.

**vs real HTTP/2:**

- Single-stream latency: **1.6–2.4× faster**
- Single-stream throughput: **1.8–2.3× higher**
- N=64 pipelined multiplexing: **~32×** (≈1.3M requests/sec on one connection)

**Agentic workloads (vs HTTP/2 + JSON):**

- Embedding batches (1536-dim, RAG / fleet broadcast): **~6–25×**
- LLM token streaming: **hundreds of millions of tokens/sec**, 9–15× over
  HTTP/2+binary at large batches — where OpenAI-style JSON-SSE caps near
  **~10M tok/s** and collapses on big batches.

These are loopback medians: the **direction and order of magnitude reproduce**
run-to-run, but absolute peaks are bandwidth- and scheduler-bound and vary by
machine. We say so, in the README and in the paper.

### We'd rather be correct than impressive

Earlier drafts of our docs (and our paper) carried four- and five-digit speedups
— "533× lower latency," "620× higher throughput" — against "standard TCP." When
we re-audited, those comparisons turned out to be a **category error**: they
measured a real kernel syscall path on one side against an in-process function
call on the other. They are not like-for-like, so we **withdrew them**.

They now live in a [`LEGACY.md`](https://github.com/nervosys/SPINE/blob/master/LEGACY.md)
with the reason each one failed validation, and the README and the paper carry
only numbers we re-ran this cycle. Where a re-run disagreed with an old absolute
— frame-codec throughput came in at ~51–62 GiB/s versus a previously published
~110–141 GiB/s — we corrected the number rather than the conclusion.

If a benchmark in our docs can't be reproduced from the bench in the repo, it
shouldn't be in our docs. That's the standard now.

---

## The neural codec, and what it actually costs

SPINE makes the *latent form* a first-class payload. `TitansLatentCodec` (a real
Titans Neural Long-Term Memory projector, not a stub) turns text into a
fixed-width latent and frames it as a self-describing `EncodedFrame`. We
benchmarked it for agentic use:

- **On the wire, it's compact:** the frame is **66–71% smaller than its JSON
  form** (dim 256: 1241 B vs 3942 B; dim 1024: 4314 B vs 14803 B), because the
  latent rides as a CBOR byte string instead of a JSON float array.
- **The encode is a real forward pass.** Cost is superlinear in width — ~94 µs
  at dim 128, ~847 µs at 256, ~3.1 ms at 512, ~26 ms at 1024. That's the honest
  price of a *semantic* projection, paid once at the sender. It is not a memcpy,
  and we don't pretend it is.

That cost/benefit lives in agentic-eval's evidence too: the wire-size win backs
SPINE's 0.95 encoding score; the encode latency is recorded right next to it.

---

## What changed in agentic-eval

This release (v0.14.x) re-scored SPINE after recent protocol work and, just as
importantly, **anchored every SPINE evidence string in a runnable benchmark**:

- the transport head-to-head (`spine_vs_http2`, `agentic_ai_workload`,
  `llm_tok_per_sec`),
- the neural codec (`neural_codec_bench`),
- the wire-size measurements (`wire_sizes`).

Scores moved only where real capability moved (the gRPC bridge maturing lifted
interop 0.15 → 0.67 across releases). The benchmarks substantiate the scores;
they were not used to inflate them. Directional tests assert the judgments that
*should* hold — e.g. gRPC's install base beats SPINE's on interop, and SPINE's
per-message Ed25519 signatures beat channel-only mTLS on security — and they
still pass.

---

## Try it

```bash
# Clone and build
git clone https://github.com/nervosys/SPINE
cargo build --release

# Reproduce the benchmarks yourself
cargo bench -p spine-transport --bench spine_vs_http2
cargo bench -p spine-transport --bench agentic_ai_workload
cargo bench -p spine-transport --bench llm_tok_per_sec
cargo bench -p spine-protocol  --bench neural_codec_bench

# Score the web stacks
cargo run -p agentic-eval --example web_benchmark
```

- **SPINE:** https://github.com/nervosys/SPINE — AGPL-3.0-or-later + commercial
- **agentic-eval:** https://github.com/nervosys/AetherShell — AGPL-3.0-or-later

For commercial licensing (closed-source SaaS, on-prem, embedded), contact
**opensource@nervosys.ai**.

---

## What's next

The honest read on SPINE today: it wins the agent-native axes it was built for,
it's at protobuf-parity on encoding with a latent data plane nothing else has
natively, and its one real weakness is adoption. The transport is never the
bottleneck — an LLM generates 50–200 tokens/sec/user; SPINE moves them by the
hundreds of millions. The work that matters now isn't another order of magnitude
on a microbenchmark. It's getting the bridges into real agent runtimes and
turning a 0.67 interop score into an earned one.

We'll keep measuring it the same way: in public, reproducibly, and with the
numbers we can actually stand behind.

— The Nervosys team
