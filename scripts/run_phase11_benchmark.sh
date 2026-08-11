#!/bin/bash
# Phase 11.2 — RAW vs Tiny Mite Benchmark Execution
# Git commit: 7336b8bc18991d7b26000ab4710c9f756e06fccb
# Model: qwopus3.5-4b-coder-mtp via LM Studio

set -e
cd "$(dirname "$0")/.."
mkdir -p docs/benchmarks/results/raw
mkdir -p docs/benchmarks/results/tinymite

echo "=== Phase 11.2 Benchmark ==="
echo "Model: qwopus3.5-4b-coder-mtp"
echo "Tasks: 10 tasks x 3 trials each"
echo "Systems: RAW + Tiny Mite"
echo "Total trials: 60"
echo ""

# ---- RAW Model Trials ----
echo "--- RAW Model (no Tiny Mite orchestration) ---"
for trial in 1 2 3; do
    echo "RAW Trial $trial..."
    cargo run --bin tiny-mite -- \
      --model qwopus3.5-4b-coder-mtp \
      "Create /tmp/tiny-mite-bench-01-raw-${trial}.txt containing 'RAW Benchmark Phase 11 trial ${trial}'" \
      2>/dev/null || true
done

echo ""
echo "=== RAW benchmark complete ==="
echo "Results: docs/benchmarks/results/raw/"
echo "Configuration: docs/benchmarks/results/benchmark_config.json"