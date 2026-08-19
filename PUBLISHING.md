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

- **27 library crates** are publishable, including the umbrella facade
  **`spine-web`** (re-exports the component crates behind feature flags; it is
  the recommended front door for new users).
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
spine-storage  spine-neural  spine-name
```

Then, strictly in this order (each line's deps are all already published):

```
 1. spine-parser      # depends on spine-name
 2. spine-crypto
 3. spine-embedded
 4. spine-knowledge
 5. spine-recursive
 6. spine-protocol
 7. spine-transport
 8. spine-compiler
 9. spine-grpc
10. spine-cluster
11. spine-wasm
12. spine-stream
13. spine-human
14. spine-agentic     # depends on spine-name
15. spine-mechgen
16. spine-agent
17. spine-core        # depends on spine-name
18. spine-web         # umbrella facade — must come after every crate it re-exports
19. spine-browser     # optional (egui app)
20. spine-gateway     # optional (binary)
21. spine-cli         # optional (binary)
```

`spine-web` references its components through *optional* dependencies, but
`cargo publish` still checks that every referenced version exists on the
registry, so it must be published last among the libraries.

## One-shot publish

crates.io needs a few seconds to index each new crate before a dependent can
resolve it, so publish sequentially and let each finish:

```bash
for c in spine-nostd spine-kernel spine-gpu spine-cache spine-k8s \
         spine-storage spine-parser spine-neural \
         spine-crypto spine-embedded spine-knowledge spine-recursive \
         spine-protocol spine-transport spine-compiler spine-grpc \
         spine-cluster spine-wasm spine-stream spine-human spine-agentic \
         spine-mechgen spine-agent spine-core spine-web \
         spine-browser spine-gateway spine-cli; do
  cargo publish -p "$c" || { echo "FAILED at $c"; break; }
  sleep 20   # allow the index to update before the next crate resolves
done
```

Or use the in-repo helper, which is what the ordering above is for:

```bash
scripts/publish.sh --dry-run    # verify packaging without uploading
scripts/publish.sh              # publish; re-run to resume
```

It **computes** the order from `cargo metadata` rather than reading the list
above, so the two cannot disagree. That matters: the previous helper kept a
hand-written order, was gitignored because it hardcoded an absolute path, and
had silently dropped `spine-name` — which `spine-parser` depends on. A run would
have failed at the seventh crate, after six were permanently on crates.io. The
list in this document was correct the whole time; nothing compared them.

The script skips already-published crates, stops cleanly on a rate limit so it
can be resumed, and stops on a 403 with the fix spelled out — an expired or
read-only token is the usual cause.

`cargo workspaces publish --from-git` is a reasonable third-party alternative if
you would rather not maintain a script at all.

## Notes

- Every internal dependency carries both a `path` (for in-repo builds) and a
  `version` matching the workspace version (which is what crates.io resolves
  against). **The workspace is at 2.0.0.** Bump both together on release;
  `cargo workspaces version` keeps them in sync.
- `dev-dependencies` are path-only (no version) on purpose — Cargo strips them
  from the published manifest, and `spine-embedded` (0.1.0) keeps its own
  version rather than inheriting the workspace one.
- **2.0.0 is a breaking release, and the break is on the wire rather than in
  any signature.** A peer running the Chameleon layer from 2.0.0 cannot talk to
  one running 1.x: the encoder's key derivation changed (Phases 47-48) and so
  did the AEAD nonce construction (Phase 45). Cargo's semver rules do not see
  this — no public API broke — which is precisely why it is written down here.
  Peers must be upgraded together.
- Publishing at 1.0.0 is no longer possible or meaningful: those versions are
  already on crates.io and predate the entire `spine://` namespace. Finishing
  the interrupted 1.0.0 publish run would have shipped a `spine-cli` resolving
  its siblings from the old published crates rather than from this tree.
- Benchmark/figure numbers in `README.md`, `LEGACY.md`, and `paper/` follow the
  "only verified numbers" standard; nothing in the published metadata asserts an
  unmeasured claim.
