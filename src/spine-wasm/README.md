# spine-wasm

WebAssembly runtime for SPINE Binary (HLB) execution

Part of [SPINE](https://github.com/nervosys/SPINE), an agentic-first web
stack. This crate is published from the SPINE workspace and versioned with
it; see the [workspace README](https://github.com/nervosys/SPINE#readme) for
how the pieces fit together and the
[CHANGELOG](https://github.com/nervosys/SPINE/blob/master/CHANGELOG.md) for
what changed in this release.

## Documentation

API documentation: <https://docs.rs/spine-wasm>

## Limits

`WasmRuntime::execute` runs untrusted programs, so every resource a program
can use is bounded. Exceeding a host budget stops the program and returns an
error that names the budget. All are `pub const` in the crate root.

| Limit | Value | What it bounds |
| --- | --- | --- |
| `MAX_INSTRUCTIONS` | 65,536 | instructions per program, checked before any compiling |
| `MAX_STRING_DATA_BYTES` | 64 KiB | string data a program carries (the guest memory is one page) |
| `DEFAULT_FUEL_BUDGET` | 1,000,000,000 | guest instructions plus host work, per run |
| `HOST_CALL_FUEL` | 64 | fuel charged per host call, plus one unit per byte it reads or scans |
| `DEFAULT_MAX_MEMORY_PAGES` | 256 (16 MiB) | guest linear memory, enforced by a wasmtime `ResourceLimiter` |
| `MAX_STACK_DEPTH` | 1,024 | host value stack |
| `MAX_VALUE_BYTES` | 64 KiB | one host value, including a string built by concat or add |
| `MAX_VALUE_DEPTH` | 64 | nesting of one value |
| `MAX_LIVE_BYTES` | 1 MiB | all live host values together |
| `MAX_ELEMENTS` / `MAX_EVENTS` / `MAX_ACTIONS` | 4,096 / 4,096 / 64 | elements, events and actions one run may produce |
| `MAX_CALL_ARGS` | 32 | `num_args` of a call |
| `MAX_LATENT_STREAMS` / `MAX_LATENT_DIMS` | 256 / 16,384 | latent vectors and their size |

Sizes are checked before anything is allocated. There is no wall-clock
timeout inside the crate: the run time is bounded by the fuel and the host
budgets above, and a caller that needs a hard deadline runs `execute` on a
thread it can abandon. `WasmStats::memory_used_bytes` is the guest's linear
memory, not the host values.

## Workspace dependencies

- `spine-compiler`
- `spine-protocol`

## License

AGPL-3.0-or-later. See [LICENSE](https://github.com/nervosys/SPINE/blob/master/LICENSE).
