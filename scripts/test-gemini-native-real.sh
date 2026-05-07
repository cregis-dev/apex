#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

ENV_FILE="${APEX_GEMINI_NATIVE_ENV_FILE:-$ROOT_DIR/.env.gemini-native}"
ENV_MODEL_OVERRIDE="${APEX_GEMINI_NATIVE_MODEL:-}"
if [[ -f "$ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$ENV_FILE"
  set +a
fi
if [[ -n "$ENV_MODEL_OVERRIDE" ]]; then
  APEX_GEMINI_NATIVE_MODEL="$ENV_MODEL_OVERRIDE"
fi

GEMINI_API_KEY="${GEMINI_API_KEY:-${APEX_GEMINI_API_KEY:-}}"
if [[ -z "$GEMINI_API_KEY" ]]; then
  echo "[gemini-native-real] missing GEMINI_API_KEY."
  echo "[gemini-native-real] Put it in .env.gemini-native or export it before running:"
  echo "  GEMINI_API_KEY=... APEX_GEMINI_NATIVE_MODEL=gemini-2.5-flash ./scripts/test-gemini-native-real.sh"
  exit 1
fi

RUNTIME_DIR="$ROOT_DIR/.run/e2e/gemini-native"
CONFIG_PATH="$RUNTIME_DIR/generated.gemini-native.config.json"
ENV_OUT="$RUNTIME_DIR/generated.gemini-native.env"
SERVER_LOG="$RUNTIME_DIR/server.log"
mkdir -p "$RUNTIME_DIR"

LISTEN="${APEX_GEMINI_NATIVE_LISTEN:-127.0.0.1:12357}"
TEAM_KEY="${APEX_GEMINI_NATIVE_TEAM_KEY:-sk-gemini-native-team}"
ADMIN_KEY="${APEX_GEMINI_NATIVE_ADMIN_KEY:-sk-gemini-native-admin}"
ROUTER_NAME="${APEX_GEMINI_NATIVE_ROUTER:-gemini-native-real}"
MODEL_ALIAS="${APEX_GEMINI_NATIVE_ALIAS:-apex-gemini-native}"
UPSTREAM_MODEL="${APEX_GEMINI_NATIVE_MODEL:-gemini-2.5-flash}"
BASE_URL="${APEX_GEMINI_NATIVE_BASE_URL:-https://generativelanguage.googleapis.com/v1beta}"

cat > "$ENV_OUT" <<EOF
APEX_E2E_LISTEN=$LISTEN
APEX_E2E_TEAM_ID=gemini-native-team
APEX_E2E_TEAM_KEY=$TEAM_KEY
APEX_E2E_ADMIN_KEY=$ADMIN_KEY
APEX_E2E_ROUTER_NAME=$ROUTER_NAME
APEX_E2E_ROUTER_STRATEGY=priority
APEX_E2E_TEST_MODEL=$MODEL_ALIAS
APEX_E2E_METRICS_PATH=/metrics

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=gemini_native_real
APEX_UPSTREAM_1_TYPE=gemini
APEX_UPSTREAM_1_BASE_URL=$BASE_URL
APEX_UPSTREAM_1_API_KEY=$GEMINI_API_KEY
APEX_UPSTREAM_1_MODEL=$UPSTREAM_MODEL
APEX_UPSTREAM_1_WEIGHT=1
EOF

echo "[gemini-native-real] generating Apex config"
cargo run --bin apex-e2e-config -- --env-file "$ENV_OUT" --output "$CONFIG_PATH" >/dev/null
python3 - "$CONFIG_PATH" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
config = json.loads(path.read_text())
allowed_models = config["teams"][0]["policy"].setdefault("allowed_models", [])
if "gemini-native" not in allowed_models:
    allowed_models.append("gemini-native")
path.write_text(json.dumps(config, indent=2) + "\n")
PY

echo "[gemini-native-real] starting Apex on $LISTEN"
cargo run --bin apex -- gateway start --config "$CONFIG_PATH" >"$SERVER_LOG" 2>&1 &
SERVER_PID=$!
cleanup() {
  if kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

python3 - "$LISTEN" <<'PY'
import socket
import sys
import time

listen = sys.argv[1]
host, port_text = listen.rsplit(":", 1)
deadline = time.time() + 20
while time.time() < deadline:
    try:
        with socket.create_connection((host, int(port_text)), timeout=1):
            sys.exit(0)
    except OSError:
        time.sleep(0.25)
print(f"timed out waiting for Apex at {listen}", file=sys.stderr)
sys.exit(1)
PY

echo "[gemini-native-real] running Gemini native REST tests"
APEX_CONFIG="$CONFIG_PATH" \
APEX_BASE_URL="http://$LISTEN" \
APEX_TEAM_KEY="$TEAM_KEY" \
APEX_ADMIN_KEY="$ADMIN_KEY" \
APEX_TEST_MODEL="$MODEL_ALIAS" \
APEX_GEMINI_NATIVE_UPSTREAM_MODEL="$UPSTREAM_MODEL" \
APEX_GEMINI_NATIVE_BASE_URL="$BASE_URL" \
GEMINI_API_KEY="$GEMINI_API_KEY" \
PYTHONDONTWRITEBYTECODE=1 \
python3 tests/e2e/test_gemini_native.py

echo "[gemini-native-real] OK"
