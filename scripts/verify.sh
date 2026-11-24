#!/usr/bin/env bash
#
# Verify the workspace: tests green, Clippy silent, rustfmt clean, and the run
# actually complete.
#
# WHAT THIS DOES NOT CHECK — read this before trusting a green run.
#
# On 2026-08-25 CI was found to have been red on `master` for eight months,
# across a hundred runs, while this script passed the whole time. It was not
# lying; it was answering a narrower question than anyone was asking of it.
# rustfmt is checked here now because that was the first of the failures and it
# blocked every job behind it. These are still outside its reach:
#
#   * other platforms — spine-kernel had never compiled for aarch64 at all
#   * the MSRV — resolution at the floor, not on your toolchain
#   * `cargo deny check` and `cargo audit` — advisories and licences
#
# A clean run here means the tests pass and the code compiles and formats on
# *this* machine, with *this* toolchain. It does not mean CI will be green, and
# on that day it was true and CI was not for eight months.
#
# The last of those is why this script exists. The obvious way to check a
# workspace run is to sum the "test result:" lines and look for failures — and
# that cannot tell a passing run from a truncated one. If a test binary dies
# before reporting, or the harness is killed partway through, the surviving
# lines still say "ok" and the total is merely smaller. A run that never
# finished looks exactly like a smaller green run.
#
# This bit once. A run during Phase 43 reported one failure and, in the same
# breath, `ignored=1` where every other run reports `ignored=5` — four ignored
# tests do not vanish, so the output had been cut short rather than a test
# having genuinely failed. The failure was never reproduced across ten
# subsequent runs. Had the tally checked completeness, the anomaly would have
# named itself instead of costing an afternoon.
#
# So: assert the shape of the run, not just the absence of the word "failed".
#
# Usage:  scripts/verify.sh [expected_test_total]
# Exit:   0 green and complete, 1 otherwise.
#
# Do not pipe this into `tail`, `head`, or anything else without `pipefail`:
# the pipeline reports the *last* command's status, so a failing run comes back
# as exit 0 and the whole point of the exit code is lost. Read the output, or
# use `set -o pipefail` in the caller.

set -uo pipefail

# How many "test result:" lines a complete run emits. This is more than the
# crate count: integration-test binaries and doctests each report their own.
# Measured from six consecutive full runs rather than reasoned from the number
# of crates — the first draft of this script guessed 30 and would have passed a
# run that stopped halfway.
#
# Update it when the workspace gains or loses a test target. A mismatch here is
# the signal, not a nuisance.
#
# 70 -> 71: the session handshake moved out of spine-crypto into its own
# spine-session crate, so its unit tests (previously inside spine-crypto's lib
# suite) now report a "test result:" line of their own. The new crate's doctest
# target may add one more line; re-measure and tighten this on the next full run.
#
# 71 -> 72: the mesh stack (identity, mesh, naming, naming_mesh, mesh_tcp,
# mesh_ws) moved out of spine-agentic into its own spine-mesh crate, so its unit
# tests now report a "test result:" line of their own. Re-measure and tighten
# this on the next full run.
#
# 72 -> 45: the research crates moved to the separate `research/` workspace, so
# this script (which runs at the repository root) no longer sees their suites.
# Measured: the Linux CI run on a580b33 reported 45 suites, 993 passed,
# 0 failed, 3 ignored for the root workspace.
#
# `research/` is not covered by this script; run it there with
# `cd research && cargo test --workspace --exclude spine-browser --lib --tests`.
# Unchanged when spine-protocol lost its `research` feature: no test target went
# away (chaos, e2e_protocol, integration, proptest_protocol, wire_encoding and the
# lib and doctest targets all still build). The passed count falls: the PSK AES
# and chameleon tests were deleted and the chameleon property tests and the
# spine-nostd interop tests moved to research/spine-protocol-research, which the
# `research` CI job runs.
#
# NOT RE-MEASURED since the native wire became spine-wire v1 (docs/spec/00-wire.md):
# that migration deleted test files and added others, so the suite count, the
# ignored count and any passed total below were measured on the earlier wire.
# Re-measure on the next full run and update EXPECTED_SUITES_MIN, EXPECTED_IGNORED
# and the total passed as $1 (see scripts/release_gate.py, SPINE_EXPECTED_TESTS).
# Re-measured on the Linux CI run for c4439cb after the v1 wire: 47 suites,
# 1,078 passed, 0 failed, 3 ignored.
EXPECTED_SUITES_MIN=47
# 5 -> 3: all five ignored tests are `ignore`-fenced doctests. Two (research)
# left with the move: research/spine-cluster/src/marketplace.rs:27 and
# research/spine-recursive/src/lib.rs:49. Three remain at the root:
# src/spine-protocol/src/replay.rs:21 and src/spine-transport/src/websocket.rs
# lines 25 and 31. Confirmed by the same run.
EXPECTED_IGNORED=3
EXPECTED_TOTAL="${1:-}"

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

# Optional retained stage logs for release evidence. Never mix stages in one
# saved file, or a later green command could conceal an earlier failure.
if [ -n "${SPINE_VERIFY_LOG_DIR:-}" ]; then
    mkdir -p "$SPINE_VERIFY_LOG_DIR" || exit 1
fi
retain_log() {
    if [ -n "${SPINE_VERIFY_LOG_DIR:-}" ]; then
        cp "$log" "$SPINE_VERIFY_LOG_DIR/$1.log" || return 1
    fi
}

echo "running cargo test --workspace --no-fail-fast ..."
cargo test --workspace --no-fail-fast >"$log" 2>&1
test_status=$?
retain_log tests || exit 1

results=$(grep -c 'test result:' "$log" || true)
failed_suites=$(grep -c 'test result: FAILED' "$log" || true)
passed=$(grep -E 'test result:' "$log" | sed 's/.*ok\. //; s/.*FAILED\. //' \
    | awk '{s += $1} END {print s + 0}')
failed=$(grep -E 'test result:' "$log" \
    | awk '{for (i = 1; i <= NF; i++) if ($i == "failed;") s += $(i - 1)} END {print s + 0}')
ignored=$(grep -E 'test result:' "$log" \
    | awk '{for (i = 1; i <= NF; i++) if ($i == "ignored;") s += $(i - 1)} END {print s + 0}')

echo "suites=$results passed=$passed failed=$failed ignored=$ignored"

ok=0

if [ "$failed" -ne 0 ] || [ "$failed_suites" -ne 0 ]; then
    echo "FAIL: $failed test(s) failed across $failed_suites suite(s)"
    grep -E '^---- .* stdout|^test .* FAILED' "$log" | head -40
    ok=1
fi

# Completeness checks. Each of these catches a truncated run that the failure
# count above would report as success.
if [ "$results" -lt "$EXPECTED_SUITES_MIN" ]; then
    echo "FAIL: only $results suites reported, expected at least $EXPECTED_SUITES_MIN"
    echo "      a run this short did not finish; treat the counts above as unreliable"
    ok=1
fi

if [ "$ignored" -ne "$EXPECTED_IGNORED" ]; then
    echo "FAIL: $ignored ignored, expected $EXPECTED_IGNORED"
    echo "      ignored tests do not appear or vanish on their own — this is the"
    echo "      signature of output that was cut short"
    ok=1
fi

if [ -n "$EXPECTED_TOTAL" ] && [ "$passed" -ne "$EXPECTED_TOTAL" ]; then
    echo "FAIL: $passed passed, expected $EXPECTED_TOTAL"
    echo "      if tests were added deliberately, pass the new total as \$1"
    ok=1
fi

if [ "$test_status" -ne 0 ] && [ "$ok" -eq 0 ]; then
    echo "FAIL: cargo exited $test_status although every suite reported ok"
    ok=1
fi

echo "running cargo clippy --workspace --all-targets ..."
cargo clippy --workspace --all-targets -- -D warnings >"$log" 2>&1
clippy_status=$?
retain_log clippy || exit 1
if [ "$clippy_status" -ne 0 ]; then
    echo "FAIL: Clippy exited $clippy_status"
    tail -40 "$log"
    ok=1
fi

# rustfmt. Cheap, and it was the first domino: `lint` runs `cargo fmt --all --
# --check` before Clippy and is first in CI's dependency graph, so 85
# unformatted files were stopping every job downstream of it — including the one
# that uploads release binaries. Nothing local was looking.
echo "running cargo fmt --all -- --check ..."
cargo fmt --all -- --check >"$log" 2>&1
fmt_status=$?
retain_log fmt || exit 1
if [ "$fmt_status" -ne 0 ]; then
    echo "FAIL: rustfmt exited $fmt_status"
    head -40 "$log"
    echo "      fix with: cargo fmt --all"
    ok=1
fi

if [ "$ok" -eq 0 ]; then
    echo "OK: $passed passed, 0 failed, $ignored ignored, across $results suites; Clippy silent; rustfmt clean"
    echo "    (not checked here: other platforms, the MSRV, advisories, licences — see the header)"
fi
exit "$ok"
