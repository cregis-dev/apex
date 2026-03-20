# Changelog

All notable changes to Apex Gateway will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Planned
- MCP Prompts API implementation
- MCP Tools execution framework
- Rule-based routing with content filtering
- PII masking engine for data compliance
- Team governance features

## [0.1.0] - 2026-03-10

Initial release of Apex Gateway.

### Added

#### Core Gateway
- Multi-LLM provider support (OpenAI, Anthropic, Gemini, DeepSeek, Moonshot, Minimax, Ollama)
- OpenAI protocol compatibility (`/v1/chat/completions`, `/v1/completions`, `/v1/models`)
- OpenAI Responses API support (`/v1/responses`)
- Anthropic protocol compatibility (`/v1/messages`, `/v1/messages/{id}`)
- Channel-based upstream configuration
- Router-based request routing with model pattern matching
- Load balancing strategies: round-robin, random, weighted

#### Advanced Routing
- Fallback routing with automatic retry on upstream failures
- Retry logic with configurable backoff (max attempts, delay, retry-on-status)
- Streaming response support for both OpenAI and Anthropic protocols
- Request/response logging with latency tracking

#### Authentication & Authorization
- Global API Key authentication
- Team-based multi-tenancy with isolated API Keys
- Policy enforcement (allowed routers, allowed models)
- Rate limiting per team (RPM - Requests Per Minute, TPM - Tokens Per Minute)

#### Observability
- SQLite database for Usage records persistence
- Usage API (`/api/usage`) with team filtering and date range
- Metrics collection: requests, errors, fallbacks, latency
- Metrics API (`/api/metrics`, `/api/metrics/trends`, `/api/metrics/rankings`)
- Prometheus metrics export (`/metrics`)
- Request latency histogram tracking

#### MCP Server
- MCP Protocol implementation with JSON-RPC 2.0
- SSE (Server-Sent Events) transport (`/mcp/sse`)
- Message endpoint for MCP requests (`/mcp/messages`)
- Session lifecycle management with in-memory storage
- Resource listing with API Key masking for security
- Support for tools/list, prompts/list, resources/list methods

#### Web Dashboard
- Next.js 16 App Router architecture
- Dashboard UI with metrics cards (Total Requests, Total Tokens, Avg Latency, Error Rate)
- Usage trends visualization with Recharts Area charts
- Error trends tracking
- Model usage rankings (Token Usage by Model)
- Channel fallback rankings
- Responsive design with shadcn/ui components
- Static export to `target/web` for serving by backend

#### Developer Experience
- Hot reload configuration watching (file system watcher)
- Graceful shutdown with tokio signal handling
- CLI subcommands (`gateway start`)
- Structured logging with tracing subscriber
- JSON log format option
- Request timeout configuration

#### MCP Resources
- Configuration resource (`config://config.json`) with read-only access
- Resource schema definition with name, description, mimeType, uri

### Changed

#### Architecture
- Refactored server initialization for better modularity
- Moved global config to `Arc<AppState>` pattern for shared state
- Unified middleware chain: Auth → RateLimit → Policy → Metrics → Logger
- Separated MCP server into dedicated module

#### Configuration
- Simplified global auth config to `auth_keys` array
- Moved web assets path to `web_dir` config option
- Configured hot reload as optional feature

### Fixed

- CORS preflight handling for MCP SSE connections
- Stream timeout handling for long-running requests
- Database connection cleanup on shutdown
- Memory leak in MCP session storage (implemented TTL cleanup)

### Removed

- Legacy command handlers (`apex mcp start`) - MCP is now config-based
- Hardcoded API Key prefixes - now configurable per team
- Deprecated metrics collection endpoints

### Technical Details

#### Dependencies Added
- `moka` - In-memory caching for MCP sessions
- `notify` - File system watching for hot reload
- `tokio-stream` - Stream utilities
- `recharts` - Dashboard charts (web)
- `@radix-ui/*` - UI primitives (web)

#### Database Schema
```sql
-- Usage tracking
CREATE TABLE usage_records (...);

-- Metrics collection
CREATE TABLE metrics_requests (...);
CREATE TABLE metrics_errors (...);
CREATE TABLE metrics_fallbacks (...);
CREATE TABLE metrics_latency (...);
```

#### Commit Highlights
- `4ceb3f4` - OpenAI Responses API support
- `eb89066` - OpenAI Responses API implementation
- `250d9f7` - Multi-platform build workflow
- `184d0c4` - Cargo fmt fixes
- `9ed4230` - Simplified global auth config
- Additional commits for MCP server, hot reload, PII masking, team governance

---

## Version History

| Version | Date | Description |
|---------|------|-------------|
| 0.1.0 | 2026-03-10 | Initial release with Core Gateway, MCP Server, Web Dashboard |

---

*For detailed implementation artifacts, see [_bmad-output/implementation/](_bmad-output/implementation/)*
