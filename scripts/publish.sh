#!/usr/bin/env bash
#
# Ordered, resumable crates.io publish for the SPINE workspace.
#
# The order is *computed* from the dependency graph, not maintained by hand.
# The hand-written list this replaced had drifted: it omitted `spine-name`
# entirely, and since `spine-parser` depends on it and sat seventh, a run would
# have failed there — after six crates were already permanently on crates.io.
# `PUBLISHING.md` had the correct order written down the whole time. Nothing
# compared the two, because one of them was a gitignored local file.
#
# So this script is tracked, path-independent, and derives what it publishes
# from `cargo metadata`. A crate added to the workspace cannot be forgotten.
#
# Usage:   scripts/publish.sh [--dry-run]
# Resume:  just re-run it; already-published crates are skipped.
# Exit:    0 done, 1 error, 2 stopped by a rate limit (re-run later).

set -uo pipefail

cd "$(dirname "$0")/.." || exit 1

DRY=""
[ "${1:-}" = "--dry-run" ] && DRY="--dry-run"

LOG="publish.log"
: > "$LOG"

say() { echo "$*" | tee -a "$LOG"; }

# Topological order over intra-workspace dependencies, leaves first.
# Crates with `publish = false` are excluded via cargo metadata's publish field.
ORDER=$(cargo metadata --no-deps --format-version 1 | python -c '
import json, sys

pkgs = json.load(sys.stdin)["packages"]
publishable = {p["name"] for p in pkgs if p.get("publish") is None}
deps = {
    p["name"]: {d["name"] for d in p["dependencies"]
                if d["name"] in publishable and d.get("kind") is None}
    for p in pkgs if p["name"] in publishable
}

ordered, seen = [], set()
while len(ordered) < len(deps):
    ready = sorted(n for n in deps if n not in seen and deps[n] <= seen)
    if not ready:
        # A cycle cannot be published in any order, so say so rather than emit
        # a list that happens to be wrong.
        sys.stderr.write("dependency cycle among: %s\n"
                         % ", ".join(sorted(set(deps) - seen)))
        sys.exit(1)
    ordered += ready
    seen.update(ready)

print(" ".join(ordered))
') || { echo "could not compute publish order"; exit 1; }

count=$(echo "$ORDER" | wc -w)
say "==== publish order ($count crates, computed from the dependency graph) ===="
say "$ORDER"
[ -n "$DRY" ] && say "==== DRY RUN — nothing will be uploaded ===="

published=0
unverifiable=0
skipped=0
for c in $ORDER; do
  say "==== $c ($(date '+%H:%M:%S')) ===="
  out=$(cargo publish $DRY -p "$c" 2>&1); rc=$?
  echo "$out" >> "$LOG"

  if [ $rc -eq 0 ]; then
    published=$((published + 1))
    say "OK: $c ($published so far)"
    # crates.io needs a moment to index a new version before a dependent of it
    # can be verified against the registry.
    [ -z "$DRY" ] && sleep 5
    continue
  fi

  if echo "$out" | grep -qiE "already (uploaded|exists)"; then
    skipped=$((skipped + 1))
    say "SKIP (already published): $c"
    continue
  fi

  if echo "$out" | grep -qiE "429|too many|rate.?limit"; then
    say "RATE LIMITED at $c — stopping. Re-run to resume."
    echo "$out" | tail -6 | tee -a "$LOG"
    exit 2
  fi

  # A dry run resolves dependencies against the registry, so during a
  # coordinated version bump every crate with an intra-workspace dependency
  # fails until that dependency is actually published — there is no ordering
  # that avoids it. Note it and carry on, so the run still validates packaging
  # for everything it can rather than stopping at the first tier-1 crate.
  #
  # Two wordings, one condition. Cargo says "failed to select a version" when
  # the crate exists at a different version, and "no matching package named"
  # when it has never been published at all. A new crate always produces the
  # second, so matching only the first passes a dry run right up until the
  # first release that adds one — which is exactly this release.
  if [ -n "$DRY" ] && echo "$out" | grep -qE "failed to select a version for the requirement|no matching package named"; then
    unverifiable=$((unverifiable + 1))
    say "UNVERIFIABLE (dry run): $c — depends on a workspace crate not yet on crates.io"
    continue
  fi

  if echo "$out" | grep -qiE "403|authentication failed"; then
    say "AUTH FAILED at $c — stopping. Nothing was published."
    say "Run 'cargo login' with a token carrying publish-new and"
    say "publish-update scopes; an expired or read-only token gives this 403."
    exit 1
  fi

  say "ERROR publishing $c — stopping."
  echo "$out" | tail -20 | tee -a "$LOG"
  exit 1
done

if [ -n "$DRY" ]; then
  say "==== DRY RUN DONE: $published packaged, $unverifiable unverifiable, $skipped already published ($((published + unverifiable + skipped))/$count) ===="
  say "Unverifiable crates depend on workspace crates not yet on crates.io;"
  say "they can only be checked once their dependencies are actually published."
else
  say "==== DONE: $published published, $skipped already there ($((published + skipped))/$count) ===="
fi
