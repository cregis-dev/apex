#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "[local-e2e] running Rust blackbox tests"
cargo test \
  --test e2e_harness_test \
  --test e2e_local_blackbox_test \
  --test e2e_regression_baseline_test

if [[ "${RUN_PYTHON_E2E:-0}" == "1" ]]; then
  echo "[local-e2e] running Python SDK smoke via tests/e2e/run_e2e.py"
  python3 tests/e2e/run_e2e.py
else
  echo "[local-e2e] skipping Python SDK smoke (set RUN_PYTHON_E2E=1 to enable)"
fi
