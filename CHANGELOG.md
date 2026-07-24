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

## [0.9.2] - 2026-07-24

Patch release fixing token accounting for Anthropic-protocol streaming through
OpenAI-compatible / dual-protocol channels (e.g. Claude Code → Qwen/DashScope).

### Fixed
- Streaming usage was recorded as 0 tokens when an upstream framed SSE lines as
  `data:{...}` without the optional space after the colon (DashScope's Anthropic
  endpoint does this). The usage tracker and the OpenAI→Anthropic stream
  converter only matched `data: ` (with a space), so every event was skipped.
  Both now accept `data:` with or without the leading space, per the SSE spec.
- Anthropic streaming token counts are now taken as the max across
  `message_start` / `message_delta` snapshots instead of summed. Upstreams that
  repeat `input_tokens` in both frames (DashScope) no longer double-count, and a
  latent output over-count on standard Anthropic streams is fixed.

## [0.9.1] - 2026-07-23

Patch release fixing token accounting for OpenAI-compatible channels.

### Fixed
- Streaming responses on OpenAI-compatible channels (e.g. Qwen/DashScope,
  OpenRouter, Ollama, custom-dual/Z.ai on their OpenAI route) recorded 0 tokens
  in usage records / Live Tail. The gateway never injected
  `stream_options.include_usage=true` on outgoing streaming requests — only the
  Gemini adapter did — so upstreams omitted the terminal `usage` chunk and the
  usage tracker read zero. Injection now covers every OpenAI-format outgoing
  request, gated by protocol so Anthropic passthrough bodies are left untouched.
  This also removes a fabricated zero-usage delta on the Anthropic→OpenAI
  streaming conversion path.

## [0.9.0] - 2026-07-23

Behavior profiling and abuse/waste detection for the control plane: identify
loops, idle spinning, retry storms, zero-output calls, spend/rate spikes, and
off-hours automation, then review flagged users on a new Governance page and act
(rate-limit or disable) after manual confirmation. Opt-in via a new `profiling`
config section; entirely inert when unconfigured.

### Added
- Request fingerprinting (`usage_records.req_hash`): a one-way blake3 hash of the
  request's semantic payload — hash only, never prompt text — powering same-user
  repeat detection. Gated by `profiling.hash_requests`.
- Hourly rollup pre-aggregation table (`usage_rollup`) with a background job
  (one-time backfill, trailing-window refresh, independent retention) as the
  substrate for per-member behavior baselines.
- New optional `profiling` config section (enable switch, rollup settings, and
  detection thresholds) with validation.
- Read-time, stateless detection in the analytics API: a `behavior` section that
  flags members on six signals — repeat rate, rate spike, error storm, zero
  output, spend spike, off-hours — via rule thresholds plus rolling z-score
  baselines, with advisory suggested actions.
- Control-plane **Governance** page: flagged-user list with evidence and
  one-click disposition (set rate limit / disable) after manual confirmation;
  hidden entirely when profiling is not enabled (via `/api/cp/info`).
- Design doc: `docs/design/behavior-profiling.md`.

### Fixed
- Control plane: removed a duplicated filter bar on the Overview page.

## [0.8.0] - 2026-07-22

Cost tracking and per-channel billing for the control plane, plus a fix for the
misleading Team/Group terminology.

### Added
- **Cost tracking**: prompt-cache tokens are now recorded per request and priced
  separately from full-price input. Cache usage is read from each provider's usage
  payload — Anthropic (`cache_read/creation_input_tokens`), OpenAI
  (`prompt_tokens_details.cached_tokens`), DeepSeek (`prompt_cache_hit_tokens`) and
  Gemini (`cachedContentTokenCount`). New `cache_read_tokens` / `cache_write_tokens`
  columns are added via an idempotent migration.
- **Pricing rules** (`pricing` config): named rules selected per channel
  (`channel.pricing`). A pay-as-you-go rule is a rate card — per-model rows,
  first match wins — so one channel can price models differently (e.g. DeepSeek
  V4-Flash vs V4-Pro). A subscription rule is a fixed monthly fee with an optional
  quota; the fee is allocated to users by token share and utilization is tracked
  against the quota.
- **Control plane**: a new **Pricing** page (read-only rule table + modal editor)
  and a pricing-rule selector in the **Channels** editor. `GET`/`PUT /admin/pricing`
  apply live via `commit_config` — no restart. The dashboard gains a **Cost**
  section (spend, per-user / per-model cost, subscriptions); filtered views only
  count subscriptions with traffic in scope.

### Changed
- **Terminology**: the control plane now calls each API key a **User** and its
  optional group a **Team** (previously "Team" / "Group"), matching how deployments
  actually use them. Display only — wire field names, API routes, and config keys
  are unchanged.

## [0.7.1] - 2026-06-08

Control plane polish: correct the router detail view for multi-rule routers and add a brand favicon.

### Fixed
- **Control plane — Routers**: the router detail panel collapsed every rule into a
  single global strategy plus a flattened channel list, so a channel reused across
  rules appeared multiple times (e.g. "Gemini, Gemini") and each rule's own strategy
  was hidden. The detail view is now rule-centric — each rule shows its own match
  models, strategy, and channels.

### Added
- The control plane now ships a brand favicon, served at `/cp/favicon.svg`, so the
  dashboard tab no longer falls back to the browser default icon.

## [0.7.0] - 2026-06-08

Bound the usage/metrics SQLite database and stop dashboard queries from blocking request-path logging.

### Added
- **Data retention**: new top-level `retention { days, interval_hours }` config
  (defaults: 90 days / 24h; `days: 0` disables). A background task prunes
  `usage_records` and the `metrics_*` tables and reclaims freed pages so the
  SQLite database stays bounded over time.
- SQLite now opens with `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout`,
  and `auto_vacuum=INCREMENTAL`, removing the per-request fsync on the logging
  path. Added timestamp indexes on `metrics_errors`, `metrics_fallbacks`, and
  `metrics_latency`.

### Changed
- Dashboard analytics/records endpoints now paginate and aggregate in SQL instead
  of loading entire time windows into memory, and read through a dedicated
  read-only connection so a slow dashboard query no longer blocks request-path
  logging.

## [0.6.0] - 2026-06-07

Retire the legacy Next.js dashboard; the Control Plane at `/cp` is now the sole web UI.

### Removed
- **Legacy Next.js dashboard** (`/dashboard`): the old web UI under `web/` has been
  retired in favor of the Control Plane at `/cp`. Removed the `web/` frontend source,
  its Playwright tests, the `scripts/dashboard/` smoke scripts, the `frontend-tests`
  CI workflow, and the dashboard build step from the release workflow. The shared
  `/api/dashboard/analytics` and `/api/dashboard/records` endpoints are unchanged —
  the Control Plane consumes them.

### Changed
- The `/dashboard` routes and the `/_next/static/*` asset route are gone; the root
  page (`/`) no longer serves the dashboard `index.html` and now links to `/cp`.
- `install.sh` and the source build instructions use `cd cp && pnpm build` instead of
  the old web build, and point operators at `/cp`.

## [0.5.1] - 2026-06-07

Control-plane observability and management additions on top of the 0.5.0 dashboard.

### Added
- **Client (tool) attribution**: every request is tagged with the calling tool —
  Claude Code, Codex, Gemini CLI, official SDKs, scripts, etc. — classified from
  request headers. New `usage_records.client` / `user_agent` columns, a "Clients"
  breakdown on the dashboard, and a Client column + filter on the Records page.
- **Dashboard rankings**: Top Teams / Models / Channels, with a Requests/Tokens toggle.
- **Inline rate-limit editor** on the Rate Limits and Teams pages, with usage-based
  suggestions derived from each team's busiest-hour traffic.
- **Per-team API key reveal**: `GET /admin/teams/{id}/api_key` returns the full key
  (admin-authenticated); the dashboard's copy action now copies the real key instead
  of the masked form.

### Changed
- Account menu moved from the top bar to the sidebar footer.
- Rate Limits page redesigned with summary stats and sorted, searchable team lists.

## [0.5.0] - 2026-06-06

### Added
- Control-plane admin CRUD for channels, routers, and teams.
- Team-scoped `/v1/models` listing.
- Full dashboard overhaul (analytics, topology, records, rate limits).

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
