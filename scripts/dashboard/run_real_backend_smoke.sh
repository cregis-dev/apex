#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

FIXTURE_ROOT="${APEX_DASHBOARD_FIXTURE_ROOT:-/tmp/apex-dashboard-integration}"
HOST="${APEX_DASHBOARD_HOST:-127.0.0.1}"
PORT="${APEX_DASHBOARD_PORT:-12356}"
API_KEY="${APEX_DASHBOARD_API_KEY:-sk-dashboard-admin-key}"
BASE_URL="http://$HOST:$PORT"
CONFIG_PATH="$FIXTURE_ROOT/config.json"
SERVER_LOG="$FIXTURE_ROOT/server.log"

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing required command: $command_name" >&2
    exit 1
  fi
}

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}

wait_for_backend() {
  local retries=30

  while (( retries > 0 )); do
    if curl -fsS -H "Authorization: Bearer $API_KEY" \
      "$BASE_URL/api/dashboard/analytics?range=24h" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
    retries=$((retries - 1))
  done

  echo "Backend did not become ready at $BASE_URL" >&2
  if [[ -f "$SERVER_LOG" ]]; then
    echo "--- server log ---" >&2
    tail -n 80 "$SERVER_LOG" >&2 || true
  fi
  return 1
}

require_command cargo
require_command curl
require_command npx

trap cleanup EXIT

"$SCRIPT_DIR/setup_real_backend_fixture.sh"

if [[ ! -f "$REPO_ROOT/target/web/dashboard/index.html" ]]; then
  echo "target/web is missing, building frontend export..."
  (cd "$REPO_ROOT/web" && npm run build)
fi

if [[ ! -x "$REPO_ROOT/target/debug/apex" ]]; then
  echo "debug binary is missing, building Rust backend..."
  (cd "$REPO_ROOT" && cargo build)
fi

mkdir -p "$FIXTURE_ROOT"
rm -f "$SERVER_LOG"

"$REPO_ROOT/target/debug/apex" gateway start "$CONFIG_PATH" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!

wait_for_backend

echo "Backend ready at $BASE_URL"
curl -fsS -H "Authorization: Bearer $API_KEY" \
  "$BASE_URL/api/dashboard/analytics?range=24h" >/dev/null

(
  cd "$REPO_ROOT/web"
  RUN_REAL_DASHBOARD_TESTS=true \
  BASE_URL="$BASE_URL" \
  DASHBOARD_API_KEY="$API_KEY" \
  npx playwright test tests/dashboard.backend.spec.ts --config playwright.real.config.ts
)
