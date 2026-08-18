#!/usr/bin/env bash
#
# Verify the workspace: tests green, Clippy silent, and the run actually
# complete.
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

set -uo pipefail

# How many "test result:" lines a complete run emits. This is more than the
# crate count: integration-test binaries and doctests each report their own.
# Measured from six consecutive full runs rather than reasoned from the number
# of crates — the first draft of this script guessed 30 and would have passed a
# run that stopped halfway.
#
# Update it when the workspace gains or loses a test target. A mismatch here is
# the signal, not a nuisance.
EXPECTED_SUITES_MIN=68
EXPECTED_IGNORED=5
EXPECTED_TOTAL="${1:-}"

log="$(mktemp)"
trap 'rm -f "$log"' EXIT

echo "running cargo test --workspace --no-fail-fast ..."
cargo test --workspace --no-fail-fast >"$log" 2>&1
test_status=$?

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
clippy_out=$(cargo clippy --workspace --all-targets 2>&1 | grep -E '^(warning|error)' || true)
if [ -n "$clippy_out" ]; then
    echo "FAIL: Clippy is not silent"
    echo "$clippy_out" | head -20
    ok=1
fi

if [ "$ok" -eq 0 ]; then
    echo "OK: $passed passed, 0 failed, $ignored ignored, across $results suites; Clippy silent"
fi
exit "$ok"
