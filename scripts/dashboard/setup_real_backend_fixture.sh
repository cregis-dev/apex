#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

FIXTURE_ROOT="${APEX_DASHBOARD_FIXTURE_ROOT:-/tmp/apex-dashboard-integration}"
HOST="${APEX_DASHBOARD_HOST:-127.0.0.1}"
PORT="${APEX_DASHBOARD_PORT:-12356}"
API_KEY="${APEX_DASHBOARD_API_KEY:-sk-dashboard-admin-key}"

DATA_DIR="$FIXTURE_ROOT/data"
LOG_DIR="$FIXTURE_ROOT/logs"
CONFIG_PATH="$FIXTURE_ROOT/config.json"
DB_PATH="$DATA_DIR/apex.db"

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing required command: $command_name" >&2
    exit 1
  fi
}

require_command sqlite3

mkdir -p "$DATA_DIR" "$LOG_DIR"
rm -f "$DB_PATH"

cat > "$CONFIG_PATH" <<EOF
{
  "version": "1.0",
  "global": {
    "listen": "$HOST:$PORT",
    "auth_keys": ["$API_KEY"],
    "timeouts": {
      "connect_ms": 1000,
      "request_ms": 10000,
      "response_ms": 30000
    },
    "retries": {
      "max_attempts": 2,
      "backoff_ms": 100,
      "retry_on_status": [500, 502, 503, 504]
    },
    "enable_mcp": false,
    "cors_allowed_origins": []
  },
  "logging": {
    "level": "info",
    "dir": "$LOG_DIR"
  },
  "data_dir": "$DATA_DIR",
  "web_dir": "$REPO_ROOT/target/web",
  "channels": [],
  "routers": [],
  "teams": [],
  "prompts": [],
  "metrics": {
    "enabled": true,
    "path": "/metrics"
  },
  "hot_reload": {
    "config_path": "$CONFIG_PATH",
    "watch": false
  }
}
EOF

sqlite3 "$DB_PATH" <<'EOF'
CREATE TABLE IF NOT EXISTS usage_records (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  timestamp TEXT NOT NULL,
  request_id TEXT,
  team_id TEXT NOT NULL,
  router TEXT NOT NULL,
  matched_rule TEXT,
  channel TEXT NOT NULL,
  model TEXT NOT NULL,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  latency_ms REAL,
  fallback_triggered INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'success',
  status_code INTEGER,
  error_message TEXT,
  provider_trace_id TEXT,
  provider_error_body TEXT
);

DELETE FROM usage_records;

WITH RECURSIVE seq(n) AS (
  VALUES(0)
  UNION ALL
  SELECT n + 1 FROM seq WHERE n < 24
)
INSERT INTO usage_records (
  timestamp, request_id, team_id, router, matched_rule, channel, model,
  input_tokens, output_tokens, latency_ms, fallback_triggered, status,
  status_code, error_message, provider_trace_id, provider_error_body
)
SELECT
  datetime('now', printf('-%d minutes', n * 5)),
  printf('req-live-%03d', n),
  CASE
    WHEN n < 15 THEN 'team-alpha'
    ELSE 'team-beta'
  END,
  CASE
    WHEN n < 21 THEN 'default'
    ELSE 'priority'
  END,
  CASE
    WHEN n < 15 THEN 'gpt-*'
    WHEN n < 21 THEN 'claude-*'
    ELSE 'fallback'
  END,
  CASE
    WHEN n < 15 THEN 'openai'
    WHEN n < 21 THEN 'bedrock'
    ELSE 'openai'
  END,
  CASE
    WHEN n < 15 THEN 'gpt-4o'
    WHEN n < 21 THEN 'claude-3-7-sonnet'
    ELSE 'gpt-4o-mini'
  END,
  CASE
    WHEN n < 15 THEN 100 + n
    WHEN n < 21 THEN 90 + n
    ELSE 60 + n
  END,
  CASE
    WHEN n < 15 THEN 200 + n * 2
    WHEN n < 21 THEN 160 + n * 2
    ELSE 100 + n
  END,
  CASE
    WHEN n < 15 THEN 150 + n * 3
    WHEN n < 21 THEN 220 + n * 4
    ELSE 3200 + n * 25
  END,
  CASE WHEN n >= 21 THEN 1 ELSE 0 END,
  CASE
    WHEN n < 21 THEN 'success'
    WHEN n < 23 THEN 'error'
    ELSE 'fallback_error'
  END,
  CASE WHEN n >= 21 THEN 502 ELSE 200 END,
  CASE WHEN n >= 21 THEN 'provider timeout' ELSE NULL END,
  CASE WHEN n >= 21 THEN printf('trace-live-%03d', n) ELSE NULL END,
  CASE WHEN n >= 21 THEN '{"error":"timeout"}' ELSE NULL END
FROM seq;
EOF

seeded_records="$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM usage_records;")"

echo "Fixture root: $FIXTURE_ROOT"
echo "Config path: $CONFIG_PATH"
echo "DB path: $DB_PATH"
echo "Seeded records: $seeded_records"
