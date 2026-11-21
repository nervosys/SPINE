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

- `research` (off by default) restores `AgentClient::enable_human_mode` and the
  human-paced `click` / `type_text` delays (`spine-human`), the public
  `neural_protocol` field and `transmit_neural` (`spine-agentic`). The
  Chameleon key is not part of the SDK: it is in
  `research/spine-protocol-research` and nothing on the native wire uses it.

The examples `agent_autonomous`, `encrypted_agent`, `latent_analysis`,
`llm_workflow_demo`, `offline_demo`, `research_swarm`, `simple_agent` and
`web_intelligence_demo` need it (`cargo run -p spine-agent --features research
--example simple_agent`). `get_latent_ur`, `transfer_session` and
`get_agentic_state` stay available without it, but need a daemon built with
`spine-core/research`.

## Documentation

API documentation: <https://docs.rs/spine-agent>

## Workspace dependencies

- `spine-agentic` (optional, `research`)
- `spine-cluster`
- `spine-compiler`
- `spine-human` (optional, `research`)
- `spine-parser`
- `spine-protocol`
- `spine-transport`
- `spine-wasm`

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
