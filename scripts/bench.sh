#!/usr/bin/env bash
# SPINE Benchmark Suite — Run all criterion benchmarks and generate reports
#
# Usage:
#   ./scripts/bench.sh          # Run all benchmarks
#   ./scripts/bench.sh --save   # Run and save baseline
#   ./scripts/bench.sh --compare # Compare against saved baseline

set -euo pipefail

BASELINE_NAME="baseline"

case "${1:-}" in
    --save)
        echo "═══ Running benchmarks and saving as baseline ═══"
        cargo bench --workspace -- --save-baseline "$BASELINE_NAME"
        echo "Baseline saved as '$BASELINE_NAME'"
        ;;
    --compare)
        echo "═══ Running benchmarks and comparing against baseline ═══"
        cargo bench --workspace -- --baseline "$BASELINE_NAME"
        ;;
    *)
        echo "═══ SPINE Performance Benchmark Suite ═══"
        echo ""
        echo "Running workspace-level hot-path benchmarks..."
        cargo bench --bench hot_path_bench
        echo ""
        echo "Running kernel benchmarks..."
        cargo bench -p spine-kernel --bench kernel_bench
        echo ""
        echo "Running scalability benchmarks..."
        cargo bench -p spine-agentic --bench scalability_bench
        echo ""
        echo "═══ Reports written to target/criterion/ ═══"
        echo "Open target/criterion/report/index.html in a browser"
        ;;
esac
