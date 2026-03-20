# Apex Gateway 项目指南

## 项目概述

Apex Gateway 是一个 Rust 编写的 AI API 网关，支持多提供商路由、负载均衡、认证鉴权和 MCP (Model Context Protocol) 协议。

## 技术栈

- **语言**: Rust (edition 2024)
- **Web 框架**: Axum
- **异步运行时**: Tokio
- **HTTP 客户端**: Reqwest
- **配置格式**: JSON
- **主要依赖**: tower-http, tracing, prometheus, moka (缓存)

## 核心模块

| 模块 | 说明 |
|------|------|
| `src/config.rs` | 配置解析和验证 |
| `src/server.rs` | HTTP 服务器主逻辑 |
| `src/router_selector.rs` | 路由选择和负载均衡 |
| `src/providers.rs` | LLM 提供商客户端封装 |
| `src/middleware/auth.rs` | API Key 认证 |
| `src/middleware/ratelimit.rs` | 速率限制 |
| `src/metrics.rs` | Prometheus 指标 |
| `src/mcp/` | MCP 服务器实现 |

## 核心概念

### 1. Channels (通道)
定义上游 LLM 提供商连接配置，支持: `openai`, `anthropic`, `gemini`, `deepseek`, `moonshot`, `minimax`, `ollama`

### 2. Routers (路由)
基于模型名匹配规则，将请求路由到不同通道，支持:
- 匹配模式: 通配符 `*` (如 `gpt-4*`)
- 负载策略: `round_robin`, `random`, `weighted`

### 3. Teams (团队)
多租户支持，可配置:
- API Key 认证
- 允许的路由和模型
- 速率限制 (RPM/TPM)

### 4. MCP 服务器
支持 MCP 协议的 prompts、tools、resources 功能

## 运行方式

```bash
# 开发模式
cargo run -- --config config.json

# 调试模式
RUST_LOG=debug cargo run -- --config config.json
```

## 测试

```bash
# 默认离线回归
cargo test

# 本地确定性黑盒
./scripts/test-local-e2e.sh

# 真实 provider 冒烟（需要 .env.e2e）
./scripts/test-real-smoke.sh

# 全套（本地 + 真实）
./scripts/test-all.sh --real
```

### 提交前测试约定

- 日常开发中，至少运行 `cargo test`
- 准备提交分支或开 PR 前，运行 `cargo test` 和 `./scripts/test-local-e2e.sh`
- 如果改动涉及 `src/providers.rs`、`src/server.rs`、`src/router_selector.rs`、`src/config.rs`、`src/e2e.rs`、`tests/e2e/` 或真实上游接入逻辑，并且本地存在 `.env.e2e`，在提交前额外运行 `./scripts/test-real-smoke.sh`
- 准备合并到 `main` 前，优先运行 `./scripts/test-all.sh --real`
- Codex 在准备提交、创建 PR、或交付需要“可合并”状态的改动时，应主动遵循以上规则；如果缺少 `.env.e2e` 或真实凭据，应明确说明只能完成离线层验证

## 配置示例

配置文件为 JSON 格式，主要结构:
```json
{
  "version": "1.0",
  "global": { "listen": "0.0.0.0:12356", "auth": {...} },
  "channels": [...],
  "routers": [...],
  "teams": [...],
  "metrics": { "enabled": true, "path": "/metrics" },
  "hot_reload": { "config_path": "config.json", "watch": true }
}
```

详细配置说明见 [docs/current/reference/config-reference.md](docs/current/reference/config-reference.md)

## 代码规范

- 使用 `tracing` 进行结构化日志
- 错误处理使用 `anyhow`
- 配置使用 `serde` 序列化
- 中间件遵循 tower 模式
