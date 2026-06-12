# Publishing SPINE to crates.io

This workspace is **28 interdependent crates**. crates.io has no atomic
multi-crate publish, so they must go up **in dependency order** (a crate can
only be published after every crate it depends on is already on the registry).

All `spine-*` names were confirmed available on crates.io as of 2026-06-12.
(The bare name `spine` is taken by an unrelated crate; we do not use it.)

## Prerequisites

```bash
cargo login            # token from https://crates.io/settings/tokens
cargo metadata --no-deps >/dev/null   # sanity: all manifests parse
```

The package owner must accept the **AGPL-3.0-or-later** license terms; the
commercial dual-license is documented in `LICENSE` and is not expressible in the
SPDX `license` field, so the crate page will show AGPL-3.0-or-later only.

## What is published

- **26 library crates** are publishable.
- **`spine-ffi`** is marked `publish = false` (C `cdylib`/`staticlib`, consumed
  via the C ABI, not via crates.io).
- `spine-python` and `spine-js` are **excluded** from the workspace (PyO3 / wasm
  bindings, distributed via PyPI / npm, not crates.io).
- `spine-cli`, `spine-gateway` (binaries) and `spine-browser` (egui app) are
  publishable so `cargo install` works, but you may skip them if you only want
  the libraries.

## Publish order

Tier 0 has no internal dependencies and can go in any order:

```
spine-nostd  spine-kernel  spine-gpu  spine-cache  spine-k8s
spine-storage  spine-parser  spine-neural
```

Then, strictly in this order (each line's deps are all already published):

```
 1. spine-crypto
 2. spine-embedded
 3. spine-knowledge
 4. spine-recursive
 5. spine-protocol
 6. spine-transport
 7. spine-compiler
 8. spine-grpc
 9. spine-cluster
10. spine-wasm
11. spine-stream
12. spine-human
13. spine-agentic
14. spine-mechgen
15. spine-agent
16. spine-core
17. spine-browser     # optional (egui app)
18. spine-gateway     # optional (binary)
19. spine-cli         # optional (binary)
```

## One-shot publish

crates.io needs a few seconds to index each new crate before a dependent can
resolve it, so publish sequentially and let each finish:

```bash
for c in spine-nostd spine-kernel spine-gpu spine-cache spine-k8s \
         spine-storage spine-parser spine-neural \
         spine-crypto spine-embedded spine-knowledge spine-recursive \
         spine-protocol spine-transport spine-compiler spine-grpc \
         spine-cluster spine-wasm spine-stream spine-human spine-agentic \
         spine-mechgen spine-agent spine-core \
         spine-browser spine-gateway spine-cli; do
  cargo publish -p "$c" || { echo "FAILED at $c"; break; }
  sleep 20   # allow the index to update before the next crate resolves
done
```

Or automate the ordering and wait-for-index with a helper:

```bash
cargo install cargo-workspaces
cargo workspaces publish --from-git    # computes order, waits on the index
```

## Notes

- Every internal dependency carries both a `path` (for in-repo builds) and a
  `version = "1.0.0"` (used once on crates.io). Bump both together on release;
  `cargo workspaces version` keeps them in sync.
- `dev-dependencies` are path-only (no version) on purpose — Cargo strips them
  from the published manifest, and `spine-embedded` (0.1.0) must not be pinned
  to the 1.0.0 workspace version.
- Benchmark/figure numbers in `README.md`, `LEGACY.md`, and `paper/` follow the
  "only verified numbers" standard; nothing in the published metadata asserts an
  unmeasured claim.
