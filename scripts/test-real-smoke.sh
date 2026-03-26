#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ ! -f "${APEX_ENV_FILE:-$ROOT_DIR/.env.e2e}" ]]; then
  echo "[real-smoke] missing .env.e2e. Copy .env.e2e.example first."
  exit 1
fi

echo "[real-smoke] running Python SDK smoke against providers from .env.e2e"
PYTHONDONTWRITEBYTECODE=1 python3 tests/e2e/run_e2e.py
