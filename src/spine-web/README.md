# spine-web

The umbrella crate for **SPINE** — an agentic-first web stack.

`spine-web` is a thin facade with no logic of its own. It re-exports the
`spine-*` component crates, each behind a Cargo feature of the same name, so you
can depend on one crate and pull in exactly the pieces you need.

```toml
# the agentic-web starter (default): protocol + transport + agent + name
spine-web = "1.0"

# the starter plus the research crates (neural, crypto, agentic, cluster, ...)
spine-web = { version = "1.0", features = ["research"] }

# just the wire protocol
spine-web = { version = "1.0", default-features = false, features = ["protocol"] }

# every reusable library
spine-web = { version = "1.0", features = ["full"] }
```

```rust
use spine_web::protocol; // == the `spine-protocol` crate
use spine_web::agent;    // == the `spine-agent` crate
```

## Features

`protocol`, `transport`, `parser`, `name`, `compiler`, `wasm`, `neural`,
`crypto`, `agent`, `agentic`, `cluster`, `human`, `stream`, `recursive`, `knowledge`,
`storage`, `kernel`, `gpu`, `grpc`, `mechgen`, `core`, `cache`, `k8s`.

`default = ["protocol", "transport", "agent", "name"]`: stable crates only, no
research crate. The research features are `neural`, `crypto`, `agentic`,
`cluster`, `human`, `recursive`, `knowledge`, `kernel`, `mechgen`, `gpu` and
`k8s`. `research` enables all of them except `gpu` and `k8s`, and also turns on
the research features of `spine-agent` and `spine-core` (`spine-protocol` has
no `research` feature). `full`
enables every reusable library (`research` plus the stable crates); the heavy
optional backends (`gpu`, `storage`'s SQLite, `k8s`) stay opt-in and are not
part of `full`.

Up to 2.0.2 `agentic` was a default feature; add `features = ["agentic"]` (or
`"research"`) to keep `spine_web::agentic`.

The `no_std` crates `spine-nostd` and `spine-embedded` are not re-exported here —
depend on them directly for embedded targets, since this facade is a `std` crate.

## License

AGPL-3.0-or-later, with a commercial option. Contact **opensource@nervosys.ai**.

See the [workspace repository](https://github.com/nervosys/SPINE) for the full
component list, benchmarks, and the publishing guide.
