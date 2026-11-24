# spine-cli

Command-line interface for the SPINE agentic web stack

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

## Commands

The default address is `127.0.0.1:8080`. `spine connect [addr]` opens a REPL
(`--ws`, `--tls`, `--ca`, `--domain`, `--client-cert`, `--client-key`) whose
commands are `nav`, `ur`, `html`, `ping`, `exec`, `caps`, `help` and `quit`.
One-shot forms: `spine query navigate|store|knowledge|html|exec|ping`,
`spine name`, `spine task`, `spine mcp`, `spine init`, `spine deploy`,
`spine benchmark`, `spine status` and `spine cert generate|info`. There are no
`morph`, `stats` or `search` commands.

## Features

`research` (off by default) forwards to `spine-core/research`.

## Documentation

API documentation: <https://docs.rs/spine-cli>

## Workspace dependencies

- `spine-agent`
- `spine-core`
- `spine-name`
- `spine-parser`
- `spine-protocol`
- `spine-transport`

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
