# Gateway Logging Specification

## 1. Objectives
- **Request Tracing**: Track requests through client -> gateway -> upstream -> response flow.
- **Troubleshooting**: Identify failures (auth, routing, upstream errors) with context.
- **Performance**: Measure latency (gateway overhead vs upstream response time).
- **Security**: Redact sensitive data (API keys) while logging necessary context.

## 2. Technology Stack
- **Core**: `tracing` (Structured logging facade)
- **Subscriber**: `tracing-subscriber` (Format logs as JSON/Text based on env)
- **Middleware**: `tower-http` (Request ID generation, HTTP access logs)

## 3. Log Levels Strategy
| Level | Usage | Example |
|-------|-------|---------|
| `ERROR` | Non-recoverable failures requiring intervention | Config load failure, Panic, Critical upstream outage |
| `WARN` | Recoverable issues or potential problems | Retry attempts, Fallback triggered, Slow requests (>2s) |
| `INFO` | High-level operational events | Server startup, Graceful shutdown, Access logs (Method/Path/Status/Latency) |
| `DEBUG` | Detailed flow information | Request headers (sanitized), Routing decisions, Response body snippets |
| `TRACE` | Extremely verbose low-level details | Full raw payloads, Byte-level streaming chunks |

## 4. Key Log Events
### 4.1. Access Log (Middleware)
Standard HTTP access logging for every request.
- `request_id`: UUID (e.g., `req-123e4567-e89b-12d3-a456-426614174000`)
- `method`: GET/POST
- `uri`: /v1/chat/completions
- `status`: 200/400/500
- `latency`: 150ms
- `client_ip`: 192.168.1.5

### 4.2. Routing Log
Context about how the request was routed.
- `router`: "openai-router"
- `channel`: "google-gemini"
- `model`: "gpt-4" -> "gemini-pro" (mapped)

### 4.3. Upstream Interaction
Details about the upstream call.
- `upstream_url`: https://generativelanguage.googleapis.com/...
- `attempt`: 1 (Retry count)
- `upstream_status`: 200
- `upstream_latency`: 120ms

## 6. Configuration & Operations

### 6.1 Configuration
Configure logging via `config.json`:
```json
{
  "logging": {
    "level": "info",    // trace, debug, info, warn, error
    "dir": "/var/log/apex" // Optional: override default log directory
  }
}
```

### 6.2 Log Rotation
- **Daemon Mode**: Logs are written to `apex.log.YYYY-MM-DD` with daily rotation.
- **Foreground Mode**: Logs are output to stdout.

### 6.3 Viewing Logs
Use the CLI command to locate and tail the latest log file:
```bash
apex logs
```
