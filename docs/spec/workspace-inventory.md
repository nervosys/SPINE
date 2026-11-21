# SPINE workspace inventory

> **2026-11-20 note: the split has been carried out.** The 14 research crates (spine-agentic, spine-browser, spine-cluster, spine-crypto, spine-embedded, spine-gpu, spine-human, spine-k8s, spine-kernel, spine-knowledge, spine-mechgen, spine-neural, spine-nostd, spine-recursive) now live under `research/` in a separate cargo workspace; the other 18 stay under `src/`. The body below describes the tree before the move, and its `research/spine-*` paths were updated to the new location while the line numbers were not re-checked. Stack crates reference research crates only through optional or dev path dependencies.

> **2026-11-21 note: measured before the native wire migration.** The crate, test and suite counts (70 suites, 1478 tests), the line counts and the `verify.sh` totals in this document were measured on 2026-10-02 (the retained test log) and 2026-11-20, before the native wire became spine-wire v1 on 2026-11-21. They were not re-measured. The extraction steps in section 5 have since been carried out as far as the Unreleased section of `CHANGELOG.md` records (the research code is out of `spine-protocol`, which no longer has a `research` feature, and the daemon and every client speak spine-wire v1); see that section for what landed, and treat any step not listed there as not verified. The wire description in section 1 and elsewhere that names `wire.rs`, the `SP` header, `ProtocolHandler` or the morphology describes the earlier wire; see `docs/spec/00-wire.md`.

Status: facts gathered 2026-11-20 on branch `completion/agent-web`, for the split described in docs/FIRST_PRINCIPLES.md (seven-layer stack, research code to a separate workspace). Every statement carries a path and line; where I could not determine something it is listed in section 6. Nothing was built or tested to produce this: sources are the Cargo manifests, `cargo metadata --no-deps`, text search of the Rust sources, the retained test log `build_completion_gate_checks/tests.log` (2026-10-02), `.github/workflows/ci.yml` and `scripts/`.

Contents: 1 crate table; 2 blocking edges; 3 spine-agentic module map; 4 spine-crypto module map; 5 order of extraction; 6 open questions; 7 corrections to the proposed classification.

## 1. Crate table

| Crate | Rust lines (all / src) | Suites, tests (2026-10-02 log) | Proposed class | Workspace dependencies | Workspace dependents |
| --- | --- | --- | --- | --- | --- |
| spine-agent | 3,588 / 1,248 | 2, 12 | STACK | agentic, compiler, embedded (dev), human, kernel (dev), name, nostd (dev), parser, protocol, transport, wasm | browser, cli, core, ffi, gateway, web (opt) |
| spine-agentic | 33,539 / 31,240 | 2, 401 | SPLIT (STACK mesh/RPC + RESEARCH) | compiler, crypto, knowledge, name, neural, protocol, transport, wasm (opt) | agent, browser, core, mechgen, web (opt) |
| spine-browser | 1,772 / 1,772 | 1, 0 | RESEARCH | agent, agentic, cluster, human, protocol, wasm | - |
| spine-cache | 861 / 861 | 2, 16 | STACK | - | web (opt) |
| spine-cli | 2,113 / 1,814 | 2, 17 | STACK | agent, core, name, parser, protocol, transport | - |
| spine-cluster | 4,182 / 4,182 | 2, 37 | RESEARCH | protocol | browser, core, web (opt) |
| spine-compiler | 2,800 / 2,800 | 2, 21 | STACK | protocol | agent, agentic, core, ffi, gateway, human, web (opt) |
| spine-core | 8,626 / 7,871 | 4, 77 | STACK | agent, agentic, cluster, compiler, human, name, neural, parser, protocol, storage, transport, wasm | cli, gateway, web (opt) |
| spine-crypto | 4,216 / 3,715 | 3, 83 | SPLIT (STACK handshake + RESEARCH) | neural | agentic, protocol, web (opt) |
| spine-embedded | 921 / 921 | 2, 24 | UNDECIDED | nostd | agent (dev), protocol (dev) |
| spine-ffi | 937 / 937 | 1, 34 | EDGE | agent, compiler, parser | - |
| spine-gateway | 2,936 / 2,936 | 1, 61 | EDGE | agent, compiler, core, name, parser, protocol, recursive, transport | - |
| spine-gpu | 1,439 / 1,439 | 2, 12 | RESEARCH | - | web (opt) |
| spine-grpc | 1,129 / 904 | 3, 16 | EDGE | protocol | web (opt) |
| spine-human | 735 / 735 | 2, 23 | RESEARCH | compiler, parser, protocol | agent, browser, core, web (opt) |
| spine-k8s | 858 / 858 | 2, 13 | RESEARCH | - | web (opt) |
| spine-kernel | 5,053 / 4,820 | 2, 36 | RESEARCH | - | agent (dev), web (opt) |
| spine-knowledge | 1,752 / 1,752 | 2, 21 | UNDECIDED | neural, storage | agentic, web (opt) |
| spine-mechgen | 387 / 232 | 2, 5 | RESEARCH | agentic | web (opt) |
| spine-name | 4,356 / 4,204 | 2, 116 | STACK | storage (dev), storage (opt) | agent, agentic, cli, core, gateway, parser, web (opt) |
| spine-neural | 2,295 / 2,295 | 2, 21 | RESEARCH | - | agentic, core, crypto, knowledge, protocol, web (opt) |
| spine-nostd | 675 / 675 | 2, 30 | UNDECIDED | - | agent (dev), embedded, protocol (dev) |
| spine-parser | 1,467 / 1,351 | 3, 53 | EDGE | name | agent, cli, core, ffi, gateway, human, web (opt) |
| spine-protocol | 11,748 / 9,238 | 7, 151 | STACK | crypto, embedded (dev), neural, nostd (dev) | agent, agentic, browser, cli, cluster, compiler, core, gateway, grpc, human, stream, transport, wasm, web (opt) |
| spine-recursive | 3,520 / 2,633 | 3, 24 | RESEARCH | - | gateway, web (opt) |
| spine-storage | 925 / 925 | 2, 24 | STACK | - | core, knowledge, name (dev), name (opt), web (opt) |
| spine-stream | 5,055 / 4,875 | 2, 35 | STACK | protocol, transport | web (opt) |
| spine-transport | 14,207 / 9,726 | 4, 85 | STACK | protocol | agent, agentic, cli, core, gateway, stream, web (opt) |
| spine-wasm | 2,080 / 2,080 | 2, 30 | STACK | protocol | agent, agentic (opt), browser, core, web (opt) |
| spine-web | 124 / 124 | 2, 0 | STACK | agent (opt), agentic (opt), cache (opt), cluster (opt), compiler (opt), core (opt), crypto (opt), gpu (opt), grpc (opt), human (opt), k8s (opt), kernel (opt), knowledge (opt), mechgen (opt), name (opt), neural (opt), parser (opt), protocol (opt), recursive (opt), storage (opt), stream (opt), transport (opt), wasm (opt) | - |
| Total | 124,296 / 109,163 | 70, 1478 | | | |

Notes on the table.

- Line counts were taken on 2026-11-20 by counting every line of every `*.rs` file under each crate directory excluding `target/` ("all" = src, tests, benches, examples, build scripts; "src" = `src/` only). The workspace total of 124,296 lines is higher than the "about 122,000" in FIRST_PRINCIPLES.md because it includes tests, benches and examples.
- Suites and tests come from the retained log `build_completion_gate_checks/tests.log` (run of 2026-10-02), not from a fresh run: 70 suites, 1,478 passed, 5 ignored, which matches what `scripts/verify.sh 1478` expects. Integration-test binaries are attributed to their crate: `agent_web` to spine-core; `chaos`, `e2e_protocol`, `integration`, `proptest_protocol`, `wire_encoding` to spine-protocol; `e2e_websocket`, `proptest_transport` to spine-transport; `gateway_route` to spine-recursive; `cli_tests` and the `spine` binary to spine-cli; `grpc_e2e` to spine-grpc; `proptest_crypto` to spine-crypto; `proptest_parser` to spine-parser. Every crate also has a doctest suite, which is why most crates show two suites. Ignored tests are all doctests: spine-cluster 1, spine-protocol 1, spine-recursive 1, spine-transport 2.
- "(opt)" is an optional dependency, "(dev)" a dev-dependency; the facade crate spine-web has every dependency optional (src/spine-web/Cargo.toml:18-40).
- Class names follow your proposal. spine-agentic and spine-crypto are marked SPLIT because they contain both kinds of code (sections 3 and 4). Sections 2 and the "Corrections" list below show that spine-protocol, spine-core and spine-agent are also not purely STACK.
- Not workspace members: `src/spine-python` (331 lines) and `src/spine-js` (145 lines) are in `exclude` (Cargo.toml:`exclude = ["src/spine-python", "src/spine-js"]`). spine-python depends by path on spine-agent, spine-protocol, spine-parser, spine-compiler, spine-transport (src/spine-python/Cargo.toml:15-19); spine-js on spine-parser, spine-compiler, spine-protocol (src/spine-js/Cargo.toml:14-16). Both are EDGE and reach spine-neural, spine-crypto and (python) spine-human, spine-agentic transitively. `src/spine-go` has no Cargo.toml and no Rust. `fuzz/Cargo.toml:12-16` is a separate fuzz package that lists spine-protocol, spine-parser, spine-transport, spine-crypto, spine-compiler; no fuzz target uses `spine_crypto` (fuzz/fuzz_targets/*.rs), so that entry is unused.

Direction rule used below: a dependency from the research workspace onto the stack workspace is acceptable; a dependency from the stack or edge workspace onto research code is a blocker.

## 2. Blocking edges

### 2.1 Summary

Counting manifest edges from a STACK or EDGE crate:

| Target kind | Edges | Real | Facade only (spine-web, optional) | Unused manifest entry |
| --- | --- | --- | --- | --- |
| Onto a crate classed wholly RESEARCH | 15 | 6 (B1-B6) | 8 (B8-B15) | 1 (B7) |
| Onto a SPLIT crate (agentic, crypto), research half | 5 | 3 (M1-M3) | 2 (M4, M5) | 0 |
| Onto an UNDECIDED crate | 6 | 1 (U5) | 1 (U6) | 0 real; 4 dev-only (U1-U4) |

That is 26 edges in all. A further group of blockers is not a manifest edge: research code that sits inside a STACK crate's own source (section 2.4). Those are harder to remove than any manifest line.

Every STACK and EDGE crate except spine-name, spine-storage, spine-cache and spine-parser reaches spine-neural and spine-crypto transitively, because spine-protocol depends on both (B1, M1). spine-transport and spine-stream reach them only through manifest entries that nothing in those crates uses (section 2.5); spine-core and spine-agent reach them through their own dependencies as well.

### 2.2 Edges onto wholly RESEARCH crates

| ID | Edge | Manifest | Items used (path:line) | Status |
| --- | --- | --- | --- | --- |
| B1 | spine-protocol to spine-neural | src/spine-protocol/Cargo.toml:41 | `MirasNeuralEncoder, MirasVariant, NeuralEncoderConfig, NeuralLatentEncoder` imported at src/lib.rs:89. Used by `ChameleonKey` (fields lib.rs:485, 487, 497; constructors 528, 534, 540-576; variant selection 661-676) and by `TitansLatentCodec` (src/agentic_codec.rs:448 field, 484 `spine_neural::NeuralLatentEncoder::new`, docs 422-438). | Real, unconditional (no feature gate; default build). Example-only: examples/chameleon_demo.rs:8 (`MirasVariant`). Bench-only: benches/neural_codec_bench.rs (through `TitansLatentCodec`). Tests reach it through `ChameleonKey` only (tests/e2e_protocol.rs:245, tests/integration.rs:167, tests/proptest_protocol.rs:165-184, lib.rs:4866-4911). |
| B2 | spine-agent to spine-human | src/spine-agent/Cargo.toml:19 | `HumanInteractionEngine` imported at src/lib.rs:4; stored in `human_engine: Option<_>` (26, 42, 172, 224); set by `enable_human_mode` (235); read in `click` (424) and `type_text` (447), where it only adds sleeps (`reaction_time_ms`, `simulate_click_duration`, `generate_typing_delays`). | Real in source but inert by default (`None` in every constructor). Example: examples/simple_agent.rs:68 (`spine_human::HumanInteractionEngine::default()`). |
| B3 | spine-core to spine-cluster | src/spine-core/Cargo.toml:23 | `ClusterNode, NodeCapabilities` (src/main.rs:9); `NodeId` (129, 134); node started at 1566-1576 and held in `state.cluster` (151, 1637); broadcast sites 1101-1125, 1323-1355, 1406-1407; the whole event loop 1804-2090 over `ClusterEvent::{SessionTransferRequested, SessionDataReceived, SearchRequested, SearchResultReceived, KnowledgeSynced, SwarmSearchRequested, TaskDelegated, KnowledgeProposed, KnowledgeVoteReceived, KnowledgeCommitted, SwarmPlanProposed, PlanTaskUpdated}`; also 2238, 2504. | Real, but only in the `spine-core` binary (src/main.rs). The library half (src/lib.rs and its modules) never mentions spine_cluster. Config fields `config.cluster.*` (main.rs:1561-1572) are in src/config.rs. |
| B4 | spine-core to spine-human | src/spine-core/Cargo.toml:22 | `HumanTranspiler::transpile` (src/main.rs:922, on every `Navigate`: HTML to HLB binary); `ReasoningEngine::new` and `create_plan` (1144, 2087). | Real, binary only. The 922 use is on the main browsing path, not a side feature. |
| B5 | spine-core to spine-neural | src/spine-core/Cargo.toml:21 | `NeuralLatentEncoder` (src/main.rs:11, 150 `state.encoder`, 1265-1266 `encode`, 1301-1303 `evolve`, 1636 `new(256, 1024, &[512, 256], 8, 42)`). | Real, binary only. |
| B6 | spine-gateway to spine-recursive | src/spine-gateway/Cargo.toml:25 | `gateway_route::{Route, Settings}` (src/model.rs:14; used 35, 56, 60, 208). | Real, but the only item used is the 222-line module research/spine-recursive/src/gateway_route.rs, which has no imports at all (its only `use` is `use super::*` in its test module at line 149). Also tested by research/spine-recursive/tests/gateway_route.rs (4 tests, suite `gateway_route`). |
| B7 | spine-agent to spine-kernel (dev) | src/spine-agent/Cargo.toml:39 | None. No `spine_kernel` appears anywhere in spine-agent (src, examples). | Unused manifest entry. |
| B8-B15 | spine-web to spine-neural, spine-cluster, spine-human, spine-recursive, spine-kernel, spine-gpu, spine-mechgen, spine-k8s | src/spine-web/Cargo.toml:24, 28, 29, 31, 34, 35, 37, 40 | One `pub use` each in src/spine-web/src/lib.rs: neural 60, cluster 76, human 80, recursive 88, kernel 100, gpu 104, mechgen 112, k8s 124. | Feature-gated. Features at Cargo.toml:62, 66, 67, 69, 72, 73, 75, 78. `default` is `protocol, transport, agent, agentic, name` (Cargo.toml:45); `full` (Cargo.toml:49) enables neural, cluster, human, recursive, kernel, mechgen (and others) but not gpu or k8s. spine-agent in `default` brings spine-human in anyway through B2. |

### 2.3 Edges onto SPLIT or UNDECIDED crates

| ID | Edge | Manifest | Items used (path:line) | Status |
| --- | --- | --- | --- | --- |
| M1 | spine-protocol to spine-crypto | src/spine-protocol/Cargo.toml:42 | `LatticeParams, QuantumSpeculativeProtocol, TransformerConfig, TransformerPredictor` (src/lib.rs:86-88). `TransformerPredictor` inside `SpeculativePredictor` (240, 274, 284). `QuantumSpeculativeProtocol` and `LatticeParams` inside `ChameleonKey` (489, 506-530, 515, 542-576). | Real, unconditional. `ProtocolHandler::new` (lib.rs:1372-1382) and `ProtocolHandlerState::new` (lib.rs:2525-2533) build two `SpeculativePredictor`s, hence two Titans transformers, for every connection whether or not speculation is used. spine-protocol never uses `spine_crypto::handshake`. |
| M2 | spine-agent to spine-agentic | src/spine-agent/Cargo.toml:20 | `NeuralProtocol` (src/lib.rs:27, 43, 173, 225), `ProtocolDomain` (783), `TransmissionResult` (784, 799). All three live in the block "NEURAL PROTOCOL STUBS (retained for spine-agent / spine-core / spine-browser)" at research/spine-agentic/src/lib.rs:8328-8424. | Real. `transmit_neural` (agent lib.rs:780) sends `BrowserCommand::NeuralTransmit`, defined at src/spine-protocol/src/lib.rs:1108. Examples: examples/web_intelligence_demo.rs:25 (`AgentProfile, AgenticWebRuntime, KnowledgeGraph`), examples/llm_workflow_demo.rs:5 (`workflow`), examples/latent_analysis.rs:8 (`ProtocolDomain`): example-only. |
| M3 | spine-core to spine-agentic | src/spine-core/Cargo.toml:24 | Stack half, library: src/namespace.rs:38-41 (`identity::SigningIdentity`, `mesh::{MeshConfig, MeshNode}`, `mesh_tcp::{serve_node_with, EncryptionConfig, Security, TcpNameTransport}`, `naming_mesh::{MaintenancePolicy, MeshNameResolver}`), 53 and 125 (`naming_mesh::MeshApplication`), src/config.rs:300-302 (`mesh_tcp::ConnectionLimits`), src/tasks.rs:353 (`impl MeshApplication for TaskService`). Stack half, binary: src/main.rs:176, 525, 589 (`MeshNameResolver`, `NameMeshError`, `AgentId`). Research half, binary only: main.rs:152 (`AgenticWebRuntime`), 158, 1499-1509 (`NeuralProtocol`, `ProtocolDomain`), 1578-1587 (`AgentProfile`, `AgentCapability`, `TrustLevel`). | Real on both sides. |
| M4, M5 | spine-web to spine-crypto, spine-agentic | src/spine-web/Cargo.toml:25, 27 | `pub use` at src/spine-web/src/lib.rs:64, 72. | Feature-gated. |
| U1-U4 | spine-protocol (dev) and spine-agent (dev) to spine-nostd and spine-embedded | spine-protocol/Cargo.toml:57-58; spine-agent/Cargo.toml:37-38 | spine-protocol: tests/e2e_protocol.rs:371-372, 387-389, 408-409 (three tests, `e2e_nostd_frame_header_roundtrip`, `e2e_embedded_message_frame_compat`, `e2e_embedded_latent_vector_compatibility`). spine-agent: examples/offline_demo.rs:16-19, 278. | Test-only and example-only. Nothing in the stack depends on either crate at runtime. |
| U5 | spine-agentic to spine-knowledge | research/spine-agentic/Cargo.toml:31 | `UnifiedConfig, UnifiedMemory` (src/lib.rs:102); field `unified_memory` in `AgenticWebRuntime` (846), built at 922, accessors 955-960, `KnowledgeValue::Text` at 998. | Real, but only inside the research half of spine-agentic (`AgenticWebRuntime`, lib.rs:833). |
| U6 | spine-web to spine-knowledge | src/spine-web/Cargo.toml:32 | `pub use` at src/spine-web/src/lib.rs:92. | Feature-gated. |

Edges inside the SPLIT crates, which disappear when the research half is carved off and so are not counted above:

- spine-agentic to spine-neural: research/spine-agentic/Cargo.toml:27; `MirasVariant` at src/lib.rs:103, used at 916.
- spine-agentic to spine-crypto: Cargo.toml:28; `MirasTitansPredictor` at src/lib.rs:101, used 848 and 906-916 (`spine_crypto::TitansConfig` at 907), which is the research half; and `spine_crypto::handshake::{Initiator, Responder, Session, MAX_HANDSHAKE_BYTES}` at src/mesh_tcp.rs:54 (also 403), which is the stack half.
- spine-agentic to spine-compiler: Cargo.toml:30, non-optional, but the only use is `spine_compiler::Compiler::compile` at src/lib.rs:1281 inside a `#[cfg(feature = "wasm-exec")]` function. spine-wasm is correctly optional (Cargo.toml:38, feature at 55), spine-compiler is not.
- spine-crypto to spine-neural: research/spine-crypto/Cargo.toml:14; one import at src/lib.rs:42-45 (`Activation, DenseLayer, MirasNeuralEncoder, MirasVariant, MultiHeadAttention, NeuralEncoderConfig, TitansMemory`), used only in the Titans region (lib.rs:65-830). Example-only: examples/miras_demo.rs:9.

### 2.4 Research code inside STACK crates (not a manifest edge)

These must be removed before the manifest edges B1 and M1 can go. They are why the first two crate moves are easy and the protocol move is not.

spine-protocol, src/lib.rs (5,350 lines):

| Lines | Content | Research or stack | Used outside the crate |
| --- | --- | --- | --- |
| 196-395 | `SpeculativeFrame`, `SpeculativePayload`, `SpeculativePredictor` (wraps `TransformerPredictor`) | Research (speculative decoding of messages) | `Message::Speculative` variant (lib.rs:809); handled at 1560, 1761 |
| 395-483 | `LatentVector` | Research (neural latent encoding) | fuzz/fuzz_targets/fuzz_latent_vector.rs:7; `Message::LatentMessage` |
| 483-697 | `ChameleonKey` ("moving-target" latent cryptography) | Research | spine-agent examples/web_intelligence_demo.rs:26, 148, 387; spine-protocol examples/chameleon_demo.rs; `ProtocolHandler::enable_chameleon` (1403), `enable_chameleon_aead` (1425), `SpineConnection` (2378, 2414, 2429) |
| 697-796 | `ProtocolMorphology`, `PaddingMode` | Research | `Message::MorphRequest`, `Message::Sync` |
| 796-860 | `Message` enum: `LatentMessage`, `Sync`, `Speculative`, `PreComputed`, `MorphRequest` are research variants among the stack variants (`Request`, `Response`, `Event`, `ToolCall`, `StreamStart`, ...) | Mixed; this is the wire type | every crate that sends frames |
| 898-1020 | `SpineBinary`, `Instruction`, `ProtocolBinOp`, `ProtocolUnaryOp` (HLB bytecode) | Stack (layer 7 input) | spine-compiler src/lib.rs:13, spine-wasm src/lib.rs:20, spine-human src/lib.rs:8 |
| 1024-1198 | `BrowserCommand`, including `NeuralTransmit` (1108) and `GetAgenticState` (1113) | Mixed | spine-agent, spine-core main.rs:1497, 1523 |
| 1199-1240 | `NameResolution`, `NameProvenance` | Stack | spine-core, spine-gateway src/names.rs:38 |
| 1241-1275 | `SwarmPlan`, `PlanTask`, `PlanStatus`, `TaskStatus` | Research (swarm) | spine-cluster src/lib.rs:218-733, spine-core main.rs:147, spine-browser main.rs:25, 53, 1496-1505, spine-agent src/lib.rs:748, 772 (as `BrowserCommand::CreateSwarmPlan`, `ExecutePlanTask`) |
| 1299-2084 | `ProtocolHandler` (chameleon, morphology, predictors, latent AEAD) | Mixed | spine-core, spine-agent, tests |
| 2085-2360 | `TransportMode`, `QuicTransport`, `QuicEndpointBuilder` | Stack | spine-agentic src/mesh_quic.rs:47 |
| 2361-2814 | `SpineConnection` | Mixed | tests |
| 2815-4860 | `ProtocolGenome`, `EvolvedProtocol`, `ProtocolPopulation`, `CoevolutionaryArena`, attacker/defender genomes (about 2,045 lines) | Research | No use anywhere outside src/spine-protocol/src/lib.rs (grepped `EvolvedProtocol`, `ProtocolPopulation`, `CoevolutionaryArena`, `ProtocolGenome`, `benchmark_protocol` over src/, fuzz/: no hits outside lib.rs) |
| 4862-5350 | `mod tests` (32 tests) | 27 of them (lib.rs:4866-5253) target Chameleon, speculation, morphology or the evolution region | |

spine-protocol, other files: `agentic.rs` (683), `mcp.rs` (740), `negotiation.rs` (509), `replay.rs` (534), `resource.rs` (42), `tasks.rs` (66), `wire.rs` (385) do not import spine_neural or spine_crypto (grepped). `agentic_codec.rs` (929) does, in `TitansLatentCodec` (lines 419-~500, import at 448 and 484); the rest of the file (codec registry, `EncodedFrame`, `CodecNegotiation`, `EmbeddingRequest`) is stack. `TitansLatentCodec` is used by spine-gateway src/agentic_sse.rs:372, 383 (the default codec in the gateway registry).

spine-core src/main.rs (2,646 lines, the daemon binary): cluster, neural encoder, human transpiler and `AgenticWebRuntime` are all in the one `State` struct (main.rs:147-176) and the one command dispatcher. The library half of spine-core (src/lib.rs plus config, connections, operations, outbound, resources, session_store, tasks, tls, tools, namespace) uses only stack crates (spine-agentic mesh modules, spine-name, spine-storage). Note that `spine-core` is both a lib and a binary in one package, so removing a research dependency from the daemon requires editing the package manifest that spine-cli and spine-gateway also link.

spine-agent src/lib.rs: `human_engine` and `neural_protocol` are public fields of `AgentClient` (lib.rs:25-27).

### 2.5 Manifest entries with no use in source

These can be deleted with no behavioural change. Each was found by searching every `.rs` file in the crate (src, tests, examples, benches) for the dependency's crate name.

| Entry | Manifest | Result |
| --- | --- | --- |
| spine-transport to spine-protocol | src/spine-transport/Cargo.toml:93 | No `spine_protocol` in any file of spine-transport. |
| spine-stream to spine-protocol | src/spine-stream/Cargo.toml:42 | No `spine_protocol` in any file of spine-stream. |
| spine-agent to spine-kernel (dev) | src/spine-agent/Cargo.toml:39 | None (B7). |
| spine-core to spine-compiler | src/spine-core/Cargo.toml:20 | None. |
| spine-core to spine-agent | src/spine-core/Cargo.toml:18 | Only tests/agent_web.rs:3 (`AgentClient`); should be a dev-dependency. |
| spine-cli to spine-parser, spine-transport | src/spine-cli/Cargo.toml:23, 25 | None in src or tests. |
| spine-gateway to spine-transport | src/spine-gateway/Cargo.toml:24 | None. |

Deleting the first two matters most: spine-transport and spine-stream then stop depending, even on paper, on spine-protocol and so on spine-neural and spine-crypto. Their docs and READMEs still say they carry spine-protocol messages (src/spine-transport/src/lib.rs:22, README.md:18 in both crates).

## 3. spine-agentic module map

Crate: research/spine-agentic, 33,539 lines of Rust, of which src 31,240, examples 2,005, benches 294. `src/lib.rs` declares 25 modules (lib.rs:60-85) and contains 8,424 lines of its own. Every module imports `crate::AgentId` (a 27-line newtype, lib.rs:234-260) and nothing else from lib.rs except as noted. Tests are the 401 unit tests in the 2026-10-02 log (19 in lib.rs plus the per-module counts below).

Cross-crate imports: grepped `spine_agentic` across src/, fuzz/. Modules not listed under "imported by crates" have no importer outside spine-agentic.

| Module | Lines | Purpose | Class | Imports from this crate | Imported by (modules) | Imported by (crates) | Tests |
| --- | --- | --- | --- | --- | --- | --- | --- |
| identity.rs | 443 | Ed25519 keypair, `SigningIdentity`, `PublicIdentity`, `SignedEnvelope` (uses ed25519-dalek directly) | STACK | AgentId | mesh, mesh_tcp, mesh_ws, mesh_quic, naming_mesh, signed_frame, lib.rs:2824 (`AgentDID`) | spine-core src/namespace.rs:38 | 13 |
| mesh.rs | 1,411 | `MeshNode`, signed `MeshEnvelope`, routing table, dedup, peer state; `MeshPayload` carries name-resolution and `Application` RPC variants (181-236) | STACK | identity, naming (types at 220-230), AgentId; spine_name (630-661) | naming_mesh, mesh_tcp, mesh_ws, mesh_quic | spine-core src/namespace.rs:39 | 24 |
| naming.rs | 1,590 | Kademlia-style name-lookup state machines (`NameService`, `Lookup`, `ResolveRequest`) over spine-name | STACK | spine_name, AgentId | mesh (payload types), naming_mesh | none directly | 34 |
| naming_mesh.rs | 1,968 | Drives name resolution over a live mesh; `MeshNameResolver`, `NameTransport`, `MeshApplication` (the signed-RPC application trait, line 46), bootstrap, replication, maintenance | STACK | identity, mesh, naming, spine_name | mesh_tcp, mesh_ws, mesh_quic | spine-core src/namespace.rs:41, 53, 125; src/main.rs:176, 525; src/tasks.rs:353 | 28 |
| mesh_tcp.rs | 2,380 | TCP socket layer: framing, pooling, `serve_node_with`, `ConnectionLimits`, optional encrypted channel via `spine_crypto::handshake` | STACK | mesh, naming_mesh, AgentId; spine_crypto::handshake (54, 403) | mesh_ws, mesh_quic | spine-core src/namespace.rs:40, src/config.rs:300-302 | 36 |
| mesh_ws.rs | 556 | WebSocket transport for the mesh | STACK | mesh, mesh_tcp, naming_mesh; spine_transport::websocket (23) | none | none | 9 |
| mesh_quic.rs | 760 | QUIC transport for the mesh, per-stream | STACK | mesh, mesh_tcp, naming_mesh; spine_protocol::QuicEndpointBuilder (47), spine_transport::websocket::QuicStream (48) | none | none | 9 |
| signed_frame.rs | 179 | Per-message signed wire frames (`sign_frame`, `verify_frame`) | STACK | identity; spine_protocol::wire, spine_protocol::Message (32-33) | none | none | 5 |
| lib.rs (AgentId only, lines 234-260) | 27 | Agent identifier newtype | STACK (shared kernel) | none | every module | spine-core src/main.rs:589 | |
| lib.rs (rest) | 8,397 | See the region list below | RESEARCH (with exceptions) | spine_crypto, spine_knowledge, spine_neural, spine_compiler, spine_wasm | anomaly, chaos, lifecycle, policy, swe, ontology_vocab and others import `AgentId`, `AgentProfile`, `AgentCapability`, `TrustLevel`, `AgentOntology`, `OntologyTerm` | see below | 19 |
| anomaly.rs | 609 | Behavioural anomaly detection for swarms | RESEARCH | AgentId | chaos | none | 11 |
| chaos.rs | 936 | Fault-injection campaigns for swarms | RESEARCH | anomaly, AgentId | none | none | 20 |
| consensus.rs | 805 | Byzantine-fault-tolerant collective decision | RESEARCH | AgentId | none | spine-mechgen examples/collab_swe.rs:10 | 10 |
| contract.rs | 987 | Agent-to-agent contract negotiation | RESEARCH | AgentId | none | none | 12 |
| federation.rs | 751 | Cross-swarm trust boundaries | RESEARCH | AgentId | none | none | 11 |
| lifecycle.rs | 750 | Agent lifecycle management | RESEARCH | AgentId, AgentProfile | none | none | 11 |
| ontology_vocab.rs | 1,575 | Ontology vocabulary (includes `swarm/consensus/raft` terms, line 627) | RESEARCH | AgentOntology, OntologyTerm | none | none | 19 |
| policy.rs | 1,279 | Policy engine with time windows and conditions | RESEARCH, but see the authority note below | AgentCapability, TrustLevel | none | none | 24 |
| replay.rs | 555 | Deterministic replay debugger | RESEARCH | AgentId | none | none | 9 |
| sandbox.rs | 671 | "WASM sandboxing" bookkeeping; does not import spine_wasm | RESEARCH | AgentId | none | none | 11 |
| scheduler.rs | 745 | Work-stealing task scheduler | RESEARCH | AgentId | none | none | 13 |
| stigmergy.rs | 526 | Pheromone-inspired coordination | RESEARCH | AgentId | none | none | 10 |
| swe.rs | 439 | Collaborative-SWE artifacts and work graph | RESEARCH | AgentCapability, AgentId | none | spine-mechgen src/lib.rs:14, examples/collab_swe.rs:13 | 6 |
| topology.rs | 645 | Adaptive swarm topology | RESEARCH | AgentId | none | none | 10 |
| tracing.rs | 519 | Distributed trace propagation across mesh hops | RESEARCH (candidate for STACK: layer 5 needs it) | AgentId | none | none | 12 |
| visualizer.rs | 487 | Swarm visualisation data | RESEARCH | AgentId | none | none | 11 |
| workflow.rs | 1,250 | Workflow orchestration | RESEARCH | AgentId | none | spine-agent examples/llm_workflow_demo.rs:5 | 24 |

Regions of `src/lib.rs` (8,424 lines, all RESEARCH unless marked), from the section banners:

| Lines | Content |
| --- | --- |
| 105-227 | `GraphicalSwarmOptimizer`, `KnowledgeGraph` |
| 228-368 | `AgentId` (STACK), `AgentCapability`, `TrustLevel`, `AgentProfile` |
| 369-825 | Intentions, goals, plans, actions, `AgentMessage`, swarms |
| 826-1238 | `AgenticWebRuntime` (holds `UnifiedMemory` and `MirasTitansPredictor`, lib.rs:846-848, 906-922) |
| 1239-1571 | Plan `ExecutionEngine`; `execute_sandboxed` under `wasm-exec` (1278-1285) |
| 1572-2113 | `SwarmCoordinator` |
| 2114-2523 | `AgentRegistry`, `AgentServer`, `AgentClient` (own TCP protocol, `AgentProtocolMessage`) |
| 2524-2808 | Behaviour trees, reactive state, `AgentSystem` |
| 2809-2964 | `AgentDID`, `DIDDocument` (uses `identity::Ed25519Keypair`, lines 2824, 2884, 2922) |
| 2965-3230 | Protocol negotiation, composite agents |
| 3232-3630 | `AgentMarketplace` |
| 3631-4142 | `TemporalReasoner`, `ContextBridge` |
| 4143-5750 | Builder, versioning/migrations, `SemanticRouter`, `ConsensusEngine` (in lib.rs, separate from consensus.rs), `AgentTracer`, `PolicyEngine` (separate from policy.rs), `EventStore` |
| 5747-6516 | Goal decomposition, `NegotiationProtocol`, `ResourceManager` |
| 6517-7124 | `AgentOntology` and registry |
| 7125-7430 | `mod tests` (19 tests) |
| 7430-8326 | Learning, skill library, trust and reputation (`TrustSystem`) |
| 8328-8424 | "Neural protocol stubs": `ProtocolDomain`, `TransmissionResult`, `SpeechAct`, `Performative`, `NeuralProtocol`. This is the only part of the research half that spine-agent and spine-core import (M2, M3) |

Minimal set the stack needs, from the import graph above:

- Modules: identity, mesh, naming, naming_mesh, mesh_tcp, mesh_ws, mesh_quic, signed_frame, plus `AgentId` from lib.rs:234-260. That is 8 module files totalling 9,287 lines (including their inline tests, 158 of the 401 tests) plus 27 lines.
- What they pull in: spine-name (naming, mesh, naming_mesh, mesh_tcp tests), spine-crypto only through `handshake` (mesh_tcp.rs:54), spine-protocol (`Message`, `wire`, `QuicEndpointBuilder`) in signed_frame and mesh_quic, spine-transport (`websocket`) in mesh_ws and mesh_quic, plus third-party tokio, ed25519-dalek, sha2, quinn, rustls, uuid, async-trait, futures, url (from research/spine-agentic/Cargo.toml). They do not import spine-neural, spine-knowledge, spine-compiler, spine-wasm, petgraph, chrono, dashmap or regex; those come from the research half.
- `mesh.rs` and `naming.rs` are mutually dependent at module level (mesh.rs:220-230 uses `crate::naming` types; naming's tests use `crate::mesh`), so they have to move together.
- Only four of the eight are imported by another crate today (identity, mesh, mesh_tcp and naming_mesh, all by spine-core). mesh_ws, mesh_quic and signed_frame have no caller outside their own tests: they are STACK by purpose (layer 1 transports and signed frames) but currently unused.
- Which spine-core call sites would change: src/namespace.rs:38-41, 53, 125; src/config.rs:300-302; src/tasks.rs:353; src/main.rs:176, 525, 589. All are import-path changes if the new crate re-exports the same names.

Where the proposal is not quite right for spine-agentic:

- `policy.rs` (1,279 lines) is classed RESEARCH here because nothing in the stack imports it, but FIRST_PRINCIPLES.md layer 3 (authority) will need a policy or capability engine; this one is time-window and condition based, not an object-capability chain. Whether to keep it is a design call.
- `tracing.rs` (519 lines, trace propagation across mesh hops) maps to layer 5 interaction; it is unused by the stack today.
- `AgentDID` and `DIDDocument` (lib.rs:2809-2964) are a second identity scheme beside spine-name's `did:` authority (src/spine-name/src/uri.rs:36); they are RESEARCH but touch the names layer, and `identity.rs` is imported by them.

## 4. spine-crypto module map

Crate: research/spine-crypto, 4,216 lines of Rust (src 3,715, tests 215, examples 286). Two source files. Its Cargo.toml (lines 13-25) lists spine-neural, serde, thiserror, log, rand, sha2, aes-gcm, hkdf, subtle, ml-kem, ed25519-dalek, zeroize.

| Item | Lines | Purpose | Class | Needs spine-neural | Used by | Tests |
| --- | --- | --- | --- | --- | --- | --- |
| src/handshake.rs | 903 (tests from 536) | Signed ephemeral ML-KEM-768 key exchange with Ed25519 authentication, HKDF-SHA256 key schedule, directional AES-256-GCM session keys (`Initiator`, `Responder`, `Session`, `PROTOCOL_LABEL`, `MAX_HANDSHAKE_BYTES`) | STACK (the construction FIRST_PRINCIPLES.md and HANDSHAKE-REVIEW.md call bespoke and unreviewed) | No. Imports only aes_gcm, ed25519_dalek, hkdf, ml_kem, rand, sha2, zeroize (handshake.rs:74-83) and nothing from lib.rs | spine-agentic src/mesh_tcp.rs:54, 403 only | 25 |
| src/lib.rs 65-830 | about 765 | Titans/MIRAS neural next-byte predictor: `PositionalEncoding`, `LayerNorm`, `FeedForward`, `TitansBlock`, `ByteTokenizer`, `OutputProjection`, `TitansPredictor`, `MirasTitansPredictor`, `TitansConfig`, aliases `TransformerPredictor`/`TransformerConfig` (803-804) | RESEARCH | Yes, import at lib.rs:42-45 | spine-protocol src/lib.rs:86-87 (`TransformerPredictor`, `TransformerConfig`); spine-agentic src/lib.rs:101, 907; examples/miras_demo.rs:8 | part of 45 lib tests |
| src/lib.rs 833-1010 | about 180 | `LatticeParams`, `RingElement` (RLWE ring arithmetic) | RESEARCH | No | spine-protocol lib.rs:87, 515, 551; tests/proptest_crypto.rs | part of 45; proptest 13 |
| src/lib.rs 1013-1135 | about 120 | `KemAlgorithm` (variants `Rlwe`, `MlKem512`, `MlKem768`, `MlKem1024`, `Hybrid`), private `MlKemKeyPair`, private `mod mlkem_ops` wrapping ML-KEM-512/768/1024 | ML-KEM calls are standard primitives, but nothing outside `QuantumKeyEvolution` uses this wrapper (the handshake calls ml_kem directly) | No | only the next two rows | |
| src/lib.rs 1138-1510 | about 370 | `QuantumKeyPair`, `QuantumKeyEvolution` (key "evolution" over RLWE or ML-KEM with HKDF) | RESEARCH | No | `QuantumSpeculativeProtocol` | |
| src/lib.rs 1513-1712 | about 200 | `QuantumSpeculativeProtocol`, `QuantumMessage`, `MessagePayload` | RESEARCH | No | spine-protocol lib.rs:87, 489, 530 (inside `ChameleonKey`) | |
| src/lib.rs 1714-2812 | about 1,100 | `mod tests` | mixed | | | 45 |

Findings that differ from the proposal:

- There is no Chameleon code in spine-crypto. `ChameleonKey` is in spine-protocol src/lib.rs:483 (section 2.4). spine-crypto's research content is the Titans predictor, the RLWE ring, and the quantum key evolution. There is no X3DH implementation either; the only occurrences of "x3dh" in Rust are `println!` strings at research/spine-agentic/examples/first_agentic_web.rs:146 and 302.
- "Standard primitives used by the stack's handshake and signatures" are not in spine-crypto lib.rs. The handshake is the separate file handshake.rs. Ed25519 record signing in the stack uses ed25519-dalek directly in spine-name (src/spine-name/Cargo.toml:21) and spine-agentic identity.rs, not spine-crypto. So the stack-side spine-crypto is exactly handshake.rs plus a manifest without spine-neural.
- spine-core has an `ed25519-dalek` dependency of its own (src/spine-core/Cargo.toml:55) and does not import spine_crypto at all.
- spine-protocol never uses `handshake`; it uses only the Titans, RLWE and quantum items. So spine-protocol could drop spine-crypto entirely once Chameleon and speculation are carved out.

## 5. Proposed order of extraction

Rules for every step: run `cargo check --workspace` and then `scripts/verify.sh <new total>` (do not run them on this machine; memory). Each step below lists the expected change in `verify.sh` inputs, computed from the 2026-10-02 per-crate counts, so a mismatch is a signal as the script itself says (scripts/verify.sh header). `EXPECTED_SUITES_MIN=70` (verify.sh:56) is a floor, so any removal of a suite fails the run until it is lowered; `EXPECTED_IGNORED=5` (verify.sh:57) is exact; the total is passed as `$1` (`scripts/release_gate.py:19`, `"scripts/verify.sh", "1478"`). I could not run any of these steps, so "leaves the workspace compiling" is the intended invariant, not something I verified.

Nothing already published is affected by moving source: 29 crates were uploaded to crates.io at 2.0.1 (publish.log, last lines); a crate that leaves the workspace keeps its registry entry, and the new stack workspace should not list it as a dependency.

Common items touched by every step that removes a crate: root `Cargo.toml` `members` list; `scripts/publish_crates.sh` hard-coded `ORDER` (lines 13-16; `scripts/publish.sh` computes its order from `cargo metadata` and needs no edit); `PUBLISHING.md` (says "29 interdependent crates", lines 3, 30-85, 120-123); README.md crate sections and test table (lines 292-359, 479, 794, 1002-1009, 1291, 1390-1445); ARCHITECTURE.md; docs/src/architecture/crates.md; `.github/copilot-instructions.md`; ROADMAP.md; docs/COMPLETION_STATUS.md lines 38-75; COMPLETION_PLAN.md line 60.

Pre-existing inconsistencies to fix at the same time, found while reading: README.md:769 says `scripts/verify.sh 1418` and README.md:1395-1398 say 1,418 tests, while the badge (README.md:7), release_gate.py, docs/COMPLETION_STATUS.md:38 and COMPLETION_PLAN.md:11, 60 say 1,478; README.md:311 says "28 specialized crates" while README.md:1383 says 30; PUBLISHING.md says 29 crates and 25 library crates.

| Step | Change | Why safe and in what order | verify.sh after (suites, tests, ignored) | CI, scripts, docs to update |
| --- | --- | --- | --- | --- |
| 0 | Delete the unused manifest entries of section 2.5: spine-transport:93, spine-stream:42, spine-agent:39 (kernel), spine-core:20 (compiler), spine-cli:23 and 25, spine-gateway:24; move spine-core:18 (agent) to `[dev-dependencies]`; drop spine-crypto from fuzz/Cargo.toml:15. | No source change. Removes the transitive path from layer 1 (transport, stream) to spine-neural and spine-crypto, and shrinks every dependent set. | 70, 1478, 5 (unchanged) | Cargo.lock only. README.md of spine-transport and spine-stream line 18 list spine-protocol as a dependency. |
| 1 | Move spine-browser. | Zero dependents (table 1). Already excluded from every CI job. 1,772 lines, one suite with zero tests. | 69, 1478, 5 | Remove `--exclude spine-browser` at ci.yml:48, 96, 99, 102, 132, 173, 268 and release_gate.py:23; publish_crates.sh ORDER; PUBLISHING.md:3 (four binaries) and the browser notes; deny.toml:42 and the seven eframe/AccessKit rows of docs/DEPENDENCY_EXCEPTIONS.md (derivative, instant, paste, ttf-parser, zbus_names, zvariant, zvariant_derive) with RUSTSEC ids 2024-0388, 2024-0384, 2024-0436, 2026-0192, 2026-0370/0371/0372 at deny.toml:25, 29, 33, 39, 43-45 (comment at 42); docs/book. |
| 2 | Move spine-gpu, spine-k8s, spine-kernel, spine-mechgen, with their spine-web features and re-exports. | Their only stack dependents are the facade (B8-B15, B7 already removed at step 0). mechgen depends on spine-agentic, so the research workspace needs spine-agentic from the registry or by path until step 8. | 61, 1412, 5 (removes 8 suites, 66 tests) | spine-web Cargo.toml:34-35, 37, 40, features 72-73, 75, 78, `full` list, src/lib.rs:100-124; ci.yml:301 (`cargo bench -p spine-kernel --bench kernel_bench`); scripts/bench.sh:30 and scripts/bench.ps1:34; docs/DEPENDENCY_EXCEPTIONS.md row RUSTSEC-2025-0012 (`backoff` via kube-runtime, k8s), deny.toml:37; docs/src/internals/{gpu,kernel,kubernetes}.md and SUMMARY.md; deploy/ unaffected (grepped). |
| 3 | Decide constrained devices, then if out of scope move spine-nostd and spine-embedded. | No runtime dependents. First delete the three tests at spine-protocol tests/e2e_protocol.rs:371-420 and the dev-dependencies at spine-protocol/Cargo.toml:57-58 and spine-agent/Cargo.toml:37-38, and edit or drop spine-agent examples/offline_demo.rs (uses both). | 57, 1355, 5 (removes 4 suites and 54 tests of nostd and embedded, plus the 3 protocol tests) | publish_crates.sh ORDER (first entries `spine-nostd`, `spine-embedded`); README; docs. If constrained devices stay first-class, skip this step and instead specify the waist to fit them. |
| 4 | Move gateway_route.rs (222 lines, no dependencies) and its 4 tests into spine-gateway, then move spine-recursive. | Removes B6 with a copy, not a rewrite. spine-recursive keeps its own `gateway_route` because `llm_dispatchers.rs:16` uses it, so the file exists in two workspaces. | 54 suites, 1,331 tests plus whatever tests of `gateway_route` are re-homed in spine-gateway (4 in the integration file, plus its unit tests at gateway_route.rs:149, which are counted inside spine-recursive's 20 lib tests and were not counted separately), ignored 4 (spine-recursive's doctest is one of the five) | spine-gateway Cargo.toml:25 and src/model.rs:14; spine-web `recursive` feature (Cargo.toml:31, 69; lib.rs:88); docs. |
| 5 | Remove spine-human from spine-agent and spine-core, then move spine-human. | B2: delete `human_engine`, `enable_human_mode`, and the two blocks at spine-agent src/lib.rs:424-460, and examples/simple_agent.rs:68. B4: `HumanTranspiler::transpile` at spine-core src/main.rs:922 is the HTML-to-HLB step on `Navigate`; the replacement must exist before this step (see section 6). `ReasoningEngine` at 1144 and 2087 is removable. | -2 suites, -23 tests | spine-agent Cargo.toml:19; spine-core Cargo.toml:22; spine-web human feature; docs. |
| 6 | Cut spine-cluster and spine-neural out of the spine-core daemon, then move spine-cluster. | B3 and B5. Remove main.rs:9, 11, 129-134, 147, 150-151, 1101-1125, 1265-1303, 1323-1355, 1406-1407, 1561-1576, 1636-1637, 1804-2090, 2238, 2504, the `SwarmPlan` map and the `config.cluster` fields. `SwarmPlan`, `TaskStatus` and `BrowserCommand::CreateSwarmPlan`, `ExecutePlanTask`, `NeuralTransmit`, `GetAgenticState` stay in spine-protocol until step 7; spine-cluster keeps depending on spine-protocol (allowed direction). | -2 suites (cluster), -37 tests, ignored 4 to 3 (spine-cluster's doctest) | spine-core Cargo.toml:21, 23; spine-web cluster feature; docs. spine-core/src/config.rs `cluster` section is a config schema change (deploy files and docs). |
| 7 | Carve the research regions out of spine-protocol: first the unused evolution region (lib.rs:2815-4860, 2,045 lines, no external users), then `ChameleonKey`, `ProtocolMorphology`, `SpeculativePredictor`/`SpeculativeFrame`, `LatentVector`, `TitansLatentCodec`, and the `Message` variants that need them. | Evolution region first: no external references anywhere, so it can go with only its own tests (about 20 of the 27 test functions at lib.rs:4944-5253). The second part changes the wire type `Message` (lib.rs:796-860), the two handlers (`ProtocolHandler` 1299, `ProtocolHandlerState` 2525) and ripples to spine-gateway src/agentic_sse.rs:372-383, spine-agent example web_intelligence_demo.rs:26, 148, 387, fuzz_latent_vector.rs:7, fuzz_message_deser.rs. | Unit tests in protocol fall by up to 27 of 100; integration tests that use `ChameleonKey` (tests/proptest_protocol.rs:165-184, tests/e2e_protocol.rs:245, tests/integration.rs:167) and benches/neural_codec_bench.rs and examples/chameleon_demo.rs must go or be rewritten | Then drop spine-protocol Cargo.toml:41-42. ci.yml bench job runs `hot_path_bench` in spine-protocol (ci.yml:299); check that it does not use Chameleon (not checked). docs/DEPENDENCY_EXCEPTIONS.md `bincode` row mentions spine-protocol. This is the largest step. |
| 8 | Split spine-agentic: new stack crate with identity, mesh, naming, naming_mesh, mesh_tcp, mesh_ws, mesh_quic, signed_frame and `AgentId`; the remainder (including `AgenticWebRuntime`, `NeuralProtocol` stubs and the 17 other modules) goes to the research workspace, which re-exports and depends on the new crate. | Needs step 5 and 6 done (they remove other agentic users), and step 7 (agentic's research half would otherwise still import neural/crypto, harmless but pointless). Remove M2: spine-agent `neural_protocol`, `transmit_neural` (lib.rs:25-27, 43, 173, 225, 780-799); M3: spine-core main.rs:152, 158, 1497-1523, 1578-1587; protocol `NeuralTransmit`/`GetAgenticState` (lib.rs:1108-1113). | Tests: 158 stay with the new crate, 243 (including lib.rs's 19) move. mesh tests keep their counts. +1 or 2 suites for the new crate | spine-core Cargo.toml:24 and imports at namespace.rs:38-41, config.rs:300, tasks.rs:353, main.rs:176, 525, 589; spine-agent Cargo.toml:20; spine-web agentic feature; publish_crates.sh; docs/src/internals/{mesh,ontology,workflow}.md; chrono advisory row (RUSTSEC-2026-0543, deny.toml:41) loses its primary owner. spine-knowledge and spine-neural lose their last stack dependents here and move with the research half. |
| 9 | Split spine-crypto: keep `handshake.rs` with a manifest that drops spine-neural; move lib.rs (Titans, RLWE, quantum) to the research workspace. | Needs steps 7 and 8 first (their last stack users: protocol lib.rs:86-88; agentic lib.rs:101, 907). mesh_tcp.rs:54 keeps working unchanged. | -1 suite, -58 tests (45 lib, 13 proptest); handshake keeps 25 | spine-web crypto feature; fuzz/Cargo.toml; docs/CRYPTO_REVIEW_PACKET.md and HANDSHAKE-REVIEW.md (refer to the crate); THREAT_MODEL.md if it names RLWE. |
| 10 | Facade and final scripts: remove the remaining research features from spine-web, set final `verify.sh` constants. | Last because earlier steps each had to keep the facade compiling. | Whatever the sum is | README.md, docs/book, COMPLETION_STATUS.md. |

Totals in the `verify.sh after` column are cumulative from the 2026-10-02 log and are exact through step 3; from step 4 they depend on how tests are re-homed and from step 7 on they depend on which tests are rewritten, so recompute them from a real run before editing `verify.sh`.

## 6. Open questions and things I could not determine

1. Test counts are from a 2026-10-02 log, not a fresh run; COMPLETION_PLAN.md:27 says that on Rust 1.100.0 Clippy now fails on a lint (verification not repeated here). Counts after step 3 are approximate.
2. Whether `spine-core`'s `HumanTranspiler::transpile` call at src/main.rs:922 can be replaced by spine-parser output plus spine-compiler. spine-human depends on spine-compiler, spine-parser, spine-protocol (research/spine-human/Cargo.toml:14-16) and its `HumanTranspiler` is HTML to HLB, which looks like gateway/edge work, not "human simulation". It is the one item in the RESEARCH set whose function the browsing path uses.
3. Whether the hot-path bench (ci.yml:299, spine-protocol `hot_path_bench`) or transport benches use `ChameleonKey` or `LatentVector`; not checked. benches/neural_codec_bench.rs does (by name).
4. Whether the 27 protocol unit tests at lib.rs:4866-5253 are the only tests of the research regions; I did not trace tests in tests/*.rs beyond the references listed.
5. Wire compatibility: removing `Message::LatentMessage`, `Sync`, `Speculative`, `PreComputed`, `MorphRequest` (lib.rs:796-860) changes the serde enum. The docs under docs/spec and negotiation.rs (src/spine-protocol/src/negotiation.rs, 509 lines) were not checked for a version byte that would need bumping.
6. spine-cluster's `sybil.rs` (310 lines; Argon2id proof of work and stake-weighted voting, research/spine-cluster/src/sybil.rs:1-5) is the only admission-cost code in the repo, and FIRST_PRINCIPLES.md layer 6 calls for exactly that. Moving spine-cluster drops it from the stack workspace. Same for spine-cluster `raft.rs`, `marketplace.rs`.
7. `spine-transport` also holds a plugin "marketplace" (src/spine-transport/src/marketplace.rs, 471 lines) used only by fuzz/fuzz_targets/fuzz_semver_parse.rs:7, and hardware backends (`rdma.rs` 1,052 lines, `uring.rs` 737, `uring_backend.rs` 323; Cargo features `dpdk`, `gpu-direct`, `io-uring`, `rdma`). They are outside your proposal; flagging them because spine-transport is 14,207 lines and 4,047 of those are benches.
8. Release surface: crates.io has the 29 crates from 2.0.1 (publish.log). Whether to publish the research workspace under the same names, new names, or not at all is undecided; spine-web's `full` feature currently documents itself as "everything that is a reusable library" (Cargo.toml:`full` comment).
9. spine-python and spine-js (excluded from the workspace) were only checked by manifest; I did not read their sources to see whether they expose Chameleon, human or neural APIs (their manifests depend on spine-agent, which exposes `human_engine` and `neural_protocol`).
10. The docs book output under docs/book is generated HTML (28 references in print.html); regenerate rather than edit.
11. I did not run `cargo` for anything beyond `cargo metadata --no-deps`, did not run tests, and did not read git history; the claim "no external users" for the evolution region is by text search of `src/` and `fuzz/` only (not `docs/`, not README snippets).

## 7. Corrections to the proposed classification

- Chameleon is not in spine-crypto. It is `ChameleonKey` in spine-protocol src/lib.rs:483. spine-crypto's research content is the Titans predictor, RLWE ring and quantum key evolution (lib.rs:65-1712), and the stack content is the separate file src/handshake.rs.
- spine-protocol is classed STACK but its research content is large (the evolution region alone is 2,045 of the 5,350 lib.rs lines, plus the regions at 196-860 and `TitansLatentCodec`), and the research types are in the `Message` wire enum and constructed on every connection (lib.rs:1380-1381). Treat it as SPLIT, with the research half small enough to be a lab crate that depends back on the stack.
- spine-core and spine-agent are classed STACK but each has research inside the crate (section 2.4). spine-core's library half is clean; its binary is not.
- spine-recursive: the only stack-visible item is a self-contained 222-line module; cheaper to copy into spine-gateway than to treat as a blocker (B6).
- spine-knowledge (UNDECIDED): its only non-facade dependent is the research half of spine-agentic (U5); it should follow that half.
- spine-nostd and spine-embedded (UNDECIDED): no runtime dependents in the workspace; only dev-dependencies of spine-protocol and spine-agent. They can move at any time after the tests and example in section 2.3 (U1-U4) are dealt with.
- spine-human: classed RESEARCH, but its `HumanTranspiler` is used on spine-core's `Navigate` path (B4).
- spine-mechgen: moves only with spine-agentic's research half (it imports `swe`, `consensus` and lib.rs types).
- spine-agentic: three of the eight "stack" modules (mesh_ws, mesh_quic, signed_frame) have no caller outside the crate; the stack's own use is only identity, mesh, mesh_tcp, naming_mesh (spine-core).
