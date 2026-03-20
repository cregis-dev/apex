#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

RUN_REAL=0
for arg in "$@"; do
  if [[ "$arg" == "--real" ]]; then
    RUN_REAL=1
  else
    echo "[test-all] unknown argument: $arg"
    exit 1
  fi
done

echo "[test-all] running cargo test"
cargo test

echo "[test-all] running local E2E"
./scripts/test-local-e2e.sh

if [[ "$RUN_REAL" == "1" ]]; then
  echo "[test-all] running real provider smoke"
  ./scripts/test-real-smoke.sh
fi
