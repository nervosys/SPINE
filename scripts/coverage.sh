#!/usr/bin/env bash
# SPINE Coverage Tracking
#
# Prerequisites:
#   rustup component add llvm-tools-preview
#   cargo install cargo-llvm-cov
#
# Usage:
#   ./scripts/coverage.sh           # Run tests with coverage, open HTML report
#   ./scripts/coverage.sh --json    # Output JSON summary (CI-friendly)
#   ./scripts/coverage.sh --lcov    # Output LCOV format for external tools

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
COVERAGE_DIR="$PROJECT_DIR/target/coverage"

cd "$PROJECT_DIR"

# Ensure prerequisites
if ! command -v cargo-llvm-cov &>/dev/null; then
    echo "Installing cargo-llvm-cov..."
    cargo install cargo-llvm-cov
fi

if ! rustup component list --installed | grep -q llvm-tools; then
    echo "Installing llvm-tools-preview..."
    rustup component add llvm-tools-preview
fi

mkdir -p "$COVERAGE_DIR"

MODE="${1:-html}"

case "$MODE" in
    --json)
        echo "==> Running tests with coverage (JSON output)..."
        cargo llvm-cov --workspace --json --output-path "$COVERAGE_DIR/coverage.json" \
            --ignore-filename-regex '(tests/|benches/|examples/|target/)' 2>&1
        echo "Coverage JSON: $COVERAGE_DIR/coverage.json"

        # Extract summary
        if command -v python3 &>/dev/null; then
            python3 -c "
import json, sys
with open('$COVERAGE_DIR/coverage.json') as f:
    data = json.load(f)
totals = data.get('data', [{}])[0].get('totals', {})
lines = totals.get('lines', {})
covered = lines.get('count', 0)
total = covered + lines.get('count', 0)
pct = lines.get('percent', 0)
print(f'Line coverage: {pct:.1f}% ({covered}/{total})')
if pct < 80:
    print('WARNING: Coverage below 80% target')
    sys.exit(1)
"
        fi
        ;;

    --lcov)
        echo "==> Running tests with coverage (LCOV output)..."
        cargo llvm-cov --workspace --lcov --output-path "$COVERAGE_DIR/lcov.info" \
            --ignore-filename-regex '(tests/|benches/|examples/|target/)' 2>&1
        echo "LCOV data: $COVERAGE_DIR/lcov.info"
        ;;

    *)
        echo "==> Running tests with coverage (HTML report)..."
        cargo llvm-cov --workspace --html --output-dir "$COVERAGE_DIR/html" \
            --ignore-filename-regex '(tests/|benches/|examples/|target/)' 2>&1
        echo ""
        echo "Coverage report: $COVERAGE_DIR/html/index.html"

        # Also print summary to terminal
        cargo llvm-cov --workspace --text \
            --ignore-filename-regex '(tests/|benches/|examples/|target/)' 2>&1 | tail -20
        ;;
esac
