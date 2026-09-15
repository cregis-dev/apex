# Apex Gateway 项目指南

## 项目概述

Apex Gateway 是一个 Rust 编写的 AI API 网关，支持多提供商路由、负载均衡、认证鉴权、用量记录与计费统计，并自带一个 Web 控制面（Control Plane）。

## 技术栈

- **语言**: Rust (edition 2024)
- **Web 框架**: Axum
- **异步运行时**: Tokio
- **HTTP 客户端**: Reqwest
- **CLI**: clap (derive)
- **存储**: SQLite (rusqlite, bundled)
- **配置格式**: JSON
- **控制面**: React + Vite + TypeScript（`cp/`），构建产物经 rust-embed 打进二进制
- **其他主要依赖**: tower-http, tracing, prometheus, moka (缓存)

## 核心模块

| 模块 | 说明 |
|------|------|
| `src/config.rs` | 配置解析、校验与保存 |
| `src/server.rs` | HTTP 服务器主逻辑、代理与 admin/控制面 API |
| `src/router_selector.rs` | 路由选择、负载均衡、会话亲和与 failover |
| `src/providers.rs` | LLM 提供商客户端封装与协议适配 |
| `src/converters.rs` | OpenAI ↔ Anthropic 协议互转 |
| `src/gemini_compat.rs` | Gemini 原生入口兼容层 |
| `src/middleware/auth.rs` | API Key 认证 |
| `src/middleware/ratelimit.rs` | 速率限制 |
| `src/middleware/policy.rs` | 团队策略（允许的路由/模型）校验 |
| `src/middleware/compliance.rs` | 合规检查 |
| `src/database.rs` | SQLite 存储层（用量记录、rollup、保留策略） |
| `src/usage.rs` | 用量与成本统计 |
| `src/request_hash.rs` | 请求指纹（行为画像用） |
| `src/compliance.rs` | 合规规则实现 |
| `src/log_stream.rs` | 进程内日志环形缓冲 + SSE 推送 |
| `src/logs.rs` | `apex logs` 的文件日志读取 |
| `src/metrics.rs` | Prometheus 指标 |
| `src/service.rs` | systemd / launchd 服务安装与管理 |
| `src/upgrade.rs` | `apex upgrade` 自升级 |
| `src/web_assets.rs` | 控制面静态资源加载（嵌入或文件系统） |
| `cp/` | 控制面前端（React SPA） |

## 核心概念

### 1. Channels (通道)
定义上游 LLM 提供商连接配置。`provider_type` 支持：
`openai`, `anthropic`, `gemini`, `custom_dual`, `deepseek`, `moonshot`,
`minimax`, `ollama`, `jina`, `openrouter`, `zai`

常用可选字段：`anthropic_base_url`（同时提供 Anthropic 协议入口）、
`model_map`（请求模型名 → 上游模型名，精确匹配）、`headers`、`timeouts`、`pricing`。

### 2. Routers (路由)
基于模型名匹配规则，将请求路由到不同通道：
- 匹配模式: 通配符 `*` (如 `gpt-4*`)
- 负载策略: `round_robin`（按 weight 加权随机，weight 为 0 表示禁用）、
  `random`、`priority`（总是选第一个）
- 支持规则内 failover 与 `session_affinity`（同一会话粘到同一通道）

### 3. Teams (团队)
多租户支持，可配置:
- API Key 认证
- 允许的路由和模型
- 速率限制 (RPM/TPM)

### 4. 控制面 (Control Plane)
`cp/` 下的 React SPA，由网关在 `/cp/` 提供。涵盖总览、Live Tail、用量记录、
日志、通道/路由/计费/限流配置与团队管理。通过 `/admin/*` 与 `/api/cp/*` 读写配置，
写入会落盘到 `config.json` 并被热重载接管。

> 注：MCP (Model Context Protocol) 相关能力已在 v0.2.0 下线，代码中不再有 `src/mcp/`。

## 运行方式

`apex` 需要子命令，`--config` 是全局参数，要放在子命令之前。仓库里有两个
bin（`apex` 和 `apex-e2e-config`），所以 `cargo run` 必须指定 `--bin apex`。

```bash
# 前台运行
cargo run --bin apex -- --config config.json gateway run

# 调试日志
RUST_LOG=debug cargo run --bin apex -- --config config.json gateway run

# 后台启动 / 停止（发布后的二进制同理）
apex --config config.json gateway start -d
apex --config config.json gateway stop
```

其他常用子命令：`init`、`config`、`channel`、`router`、`team`、`status`、
`logs`、`service`、`upgrade`。

## 测试

```bash
# 单元测试 + 全部集成测试
cargo test

# 单个集成测试目标
cargo test --test gateway

# 本地 E2E（Rust 黑盒；加 RUN_PYTHON_E2E=1 一并跑 Python SDK 冒烟）
./scripts/test-local-e2e.sh
RUN_PYTHON_E2E=1 ./scripts/test-local-e2e.sh

# 控制面构建（改动 cp/ 后需要，产物在 target/web/）
cd cp && pnpm build
```

提交前请确保 `cargo fmt --check` 与 `cargo clippy --all-targets -- -D warnings` 通过，
CI 的 `rust-quality` 会卡这两项。

## 配置示例

配置文件为 JSON 格式，主要结构:
```json
{
  "version": "1.0",
  "global": {
    "listen": "0.0.0.0:12356",
    "auth_keys": ["..."],
    "timeouts": { "connect_ms": 1000, "request_ms": 10000, "response_ms": 30000 },
    "retries": { "max_attempts": 3, "backoff_ms": 100, "retry_on_status": [500, 502, 503, 504] }
  },
  "channels": [...],
  "routers": [...],
  "teams": [...],
  "metrics": { "enabled": true, "path": "/metrics" },
  "hot_reload": { "config_path": "config.json", "watch": true },
  "data_dir": "~/.apex/data",
  "logging": { "level": "info" },
  "retention": {...},
  "pricing": {...},
  "compliance": {...},
  "profiling": {...}
}
```

详细配置说明见 [docs/current/reference/config-reference.md](docs/current/reference/config-reference.md)

## 代码规范

- 使用 `tracing` 进行结构化日志
- 错误处理使用 `anyhow`
- 配置使用 `serde` 序列化
- 中间件遵循 tower 模式
