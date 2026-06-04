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

## [0.4.4] - 2026-06-04

Patch release that closes the residual placeholder-credential hole from 0.4.3.

### Security

- `apex gateway run` (and the launchd / systemd services that wrap it) now refuses to bind if `global.auth_keys` or any `teams[].api_key` still contains one of the known placeholder strings shipped by `install-release.sh`, `install.sh`, `config.example.json`, or the historical v0.4.2 default config. Previously those strings (`replace-with-admin-key`, `sk-your-secret-key-here`, `sk-team-demo-key`, etc.) were accepted verbatim by the auth middleware, so a user who ignored the install-time warning would have a guessable preset key live on `0.0.0.0:12356`. The gateway now exits 1 with a multi-line message that lists every violation and points at `apex config path`. Hot-reload picks up the same check and refuses to swap in a config that re-introduces a placeholder.
- `apex config validate` runs the same check, so users can catch this before ever starting the service.

### Changed

- `apex config validate` exit code is now 1 when placeholder credentials are present (was 0 if JSON parsed).

### Notes for upgraders

If you installed v0.4.2 and never edited `~/.apex/config.json` (or `/opt/apex/config.json`), your default `auth_keys = ["sk-your-secret-key-here"]` and demo team key `sk-team-demo-key` will now block the service from starting. Edit the file (`apex config path` to find it), replace both with real secrets, then `apex service restart`. Same applies if you ran `install-release.sh` on 0.4.3 and left the `replace-with-admin-key` placeholder in.

## [0.4.3] - 2026-06-04

Patch release that stops `install-release.sh` from shipping a misleading default config.

### Security

- `install-release.sh` no longer copies `config.example.json` verbatim as the runtime `config.json`. The previous default contained a live-looking admin auth key (`sk-your-secret-key-here`) and a live-looking team API key (`sk-team-demo-key`) that the auth middleware would accept as real, so a fresh install — especially the no-sudo macOS `--service` flow added in 0.4.2 — would come up with two preset credentials exposed on `0.0.0.0:12356`.

### Changed

- `install-release.sh` now generates a clean placeholder `config.json` inline (mirroring `install.sh`): `auth_keys: ["replace-with-admin-key"]`, empty `teams` / `channels` / `routers`, absolute `data_dir` / `logging.dir` / `hot_reload.config_path` rooted at the install dir. Auth fails closed until the user sets a real key.
- The install script prints the exact fields that must be edited before starting and points users at `config.example.json` (still bundled) for field-structure reference only.
- `config.example.json` itself now uses obvious placeholders (`REPLACE-WITH-YOUR-ADMIN-KEY`, `REPLACE-WITH-YOUR-TEAM-API-KEY`, `/absolute/path/to/apex/...`) so anyone copying snippets out of it won't end up with live demo secrets.

### Removed

- Dropped the stale `model_map` example (`claude-3-5-sonnet` → `claude-sonnet-4-20250514`) from `config.example.json`; the alias was 13 months old by ship date.

## [0.4.2] - 2026-05-21

Patch release that makes `apex service` actually work on macOS.

### Changed

- Default `--install-dir` is now platform-aware: `/opt/apex` on Linux (unchanged), `~/.apex` on macOS. Override with `--install-dir` or `APEX_INSTALL_DIR`.
- `install-release.sh` `TARGET_DIR` default follows the same rule; the script honors `SUDO_USER` so `sudo` on macOS still resolves to the calling user's home.
- `service install` on macOS now unloads any previously bootstrapped plist after writing a new one, so the next `service start` picks up the new ExecStart / env / paths instead of running the stale in-memory copy.
- `service stop` on macOS is now idempotent — it no longer errors when the service is already unloaded.

### Fixed

- macOS launchd user agent used to start as the calling user but try to write logs into the root-owned `/opt/apex/logs/`, which made the service crash-loop under `KeepAlive`. Defaulting the install dir to the user's home removes the permission mismatch.
- `launchd_service_is_loaded` no longer leaks `launchctl print` output to the terminal during probing.

### Docs

- `README.md`, `README_zh-CN.md`, `docs/current/guides/deployment.md`, and `docs/current/guides/operations.md` now document separate Linux / macOS install paths and stop telling macOS users to `sudo`.

## [0.2.0] - 2026-03-28

Minor release focused on `z.ai` provider support and E2E runtime hygiene.

### Added

- Added `zai` as a first-class provider option in shared config, CLI scaffolding, and generated provider templates
- Added native dual-protocol support for `z.ai`, with OpenAI requests routed to `https://api.z.ai/api/coding/paas/v4` and Anthropic requests routed to `https://api.z.ai/api/anthropic`

### Changed

- Real E2E config generation now fills in provider-specific default `anthropic_base_url` values, including `z.ai`
- E2E smoke assertions now accept non-empty streaming content from real providers instead of requiring arbitrarily long responses

### Fixed

- Test runtime artifacts such as generated config, logs, router outputs, and SQLite data now stay under `.run/e2e/` instead of polluting `tests/`
- `z.ai` Anthropic requests no longer rely on OpenAI-compat bridging and now use the provider's native messages endpoint
- Real smoke and local E2E flows now align with the current `apex gateway start --config ...` CLI contract

## [0.1.2] - 2026-03-26

Patch release focused on installer behavior and explicit runtime configuration.

### Changed

- `install-release.sh` now installs only the Apex binary by default
- `install-release.sh` writes the packaged example config only when `--config-path` is explicitly provided
- `install-release.sh` no longer creates `data/` or `logs/` directories during install
- `apex gateway start` now requires an explicit `--config` or `-c` argument

### Fixed

- Updated local E2E and dashboard smoke scripts to use the explicit `gateway start --config ...` invocation

## [0.1.1] - 2026-03-26

Patch release focused on packaging and compatibility fixes.

### Fixed

- Linux x86_64 release packaging now builds with `x86_64-unknown-linux-musl`
- Prebuilt Linux packages no longer require newer glibc versions from GitHub runner images

### Changed

- Added a maintainer release runbook in `RELEASE.md`
- Upgraded GitHub Actions workflow dependencies to newer runtime-compatible versions

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
- Team leaderboard expanded to top 10 teams by token consumption
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
- Multi-platform GitHub Release packaging for Linux and macOS
- Prebuilt package installation via `install-release.sh`

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
