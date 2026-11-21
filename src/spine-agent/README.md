# spine-agent

Agent interface for SPINE

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

## Features

`default = []`: the default build has no research crate in its dependency tree.

- `research` (off by default) pulls in `spine-agentic` for the examples that
  use its workflow, runtime and knowledge-graph types. The SDK itself has no
  research-gated code. The Chameleon key is not part of the SDK: it is in
  `research/spine-protocol-research` and nothing on the native wire uses it.

The examples `agent_autonomous`, `encrypted_agent`, `llm_workflow_demo`,
`offline_demo`, `research_swarm`, `simple_agent` and `web_intelligence_demo`
need it (`cargo run -p spine-agent --features research --example
simple_agent`).

The SDK carries only the core command set: `navigate`, `get_ur`,
`get_raw_html`, `execute_binary` / `execute_hls`, `store_knowledge`,
`query_knowledge`, `get_binary_capabilities`, the namespace and resource
calls, `call_tool`, `capabilities` and `ping`. The research-session commands
(latent encoding, session transfer, neural transmit, agentic state, autonomous
mode) are recorded in `research/spine-protocol-research/README.md`.

## Documentation

API documentation: <https://docs.rs/spine-agent>

## Workspace dependencies

- `spine-agentic` (optional, `research`)
- `spine-cluster`
- `spine-compiler`
- `spine-parser`
- `spine-protocol`
- `spine-transport`
- `spine-wasm`

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
