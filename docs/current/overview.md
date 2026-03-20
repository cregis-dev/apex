# Apex Gateway - Project Overview

**Date:** 2026-03-10
**Type:** Backend (Rust) + Web (Next.js)
**Architecture:** AI API Gateway with MCP Server

## Executive Summary

Apex Gateway 是一个使用 Rust 编写的高性能 AI API 网关服务，旨在统一企业内部的大模型访问入口。它屏蔽了多家模型提供商（OpenAI, Anthropic, Gemini, DeepSeek 等）的接口差异，提供统一的鉴权、路由、重试与监控能力。

项目包含两个主要部分：
1. **Backend (Rust)**: 核心网关服务，支持多租户、智能路由、MCP 协议
2. **Web Dashboard (Next.js)**: 可观测性仪表板，展示 Usage 记录和 Metrics 指标

## Project Classification

- **Repository Type:** Multi-part monorepo
- **Project Type(s):** Backend + Web
- **Primary Language:** Rust 2024, TypeScript/JavaScript
- **Architecture Pattern:** API Gateway with Provider Adapters

## Multi-Part Structure

This project consists of 2 distinct parts:

### Apex Backend (backend)

- **Type:** Backend API / CLI
- **Location:** `/` (project root)
- **Purpose:** AI 网关核心服务，提供 OpenAI/Anthropic 兼容接口
- **Tech Stack:** Rust 2024, Axum, Tokio, Reqwest, SQLite, Prometheus

### Web Dashboard (web)

- **Type:** Web Application
- **Location:** `/web`
- **Purpose:** 可观测性仪表板，展示使用记录和指标
- **Tech Stack:** Next.js 16, React 19, shadcn/ui, Tailwind CSS, Recharts

### How Parts Integrate

1. **静态文件服务**: Next.js 构建后输出到 `target/web`，由 Rust 后端统一静态资源层提供服务；发布态推荐通过 `embedded-web` 嵌入到二进制
2. **API 通信**: Web Dashboard 通过 `GET /api/usage` 和 `GET /api/metrics` 端点与后端通信
3. **认证**: 使用相同的 API Key 体系 (Team-based) 进行认证

## Technology Stack Summary

### Backend Stack

| Category | Technology |
|----------|------------|
| Language | Rust 2024 |
| Web Framework | Axum 0.7.5 |
| Async Runtime | Tokio 1.43.0 |
| HTTP Client | Reqwest 0.12.12 |
| Database | SQLite (rusqlite 0.32) |
| Config | JSON (serde) |
| Logging | tracing + tracing-subscriber |
| Metrics | Prometheus |
| Cache | Moka 0.12.10 |
| CLI | Clap 4.5.27 |

### Web Stack

| Category | Technology |
|----------|------------|
| Framework | Next.js 16.1.6 (App Router) |
| Language | TypeScript 5 |
| UI Library | React 19.2.3 |
| Components | shadcn/ui |
| Styling | Tailwind CSS 4 |
| Charts | Recharts 2.15.4 |
| Testing | Playwright 1.58.2 |

## Key Features

### Core Gateway
- **统一 API**: OpenAI & Anthropic 协议兼容
- **智能路由**: 基于模型名称的内容路由，支持轮询、优先级、随机策略
- **故障转移**: 自动 Fallback 机制 (429, 500, 502, 503, 504)
- **重试机制**: 可配置最大重试次数和退避时间
- **多租户**: Team-based API Key 鉴权和限流

### Provider 支持
- OpenAI (标准协议)
- Anthropic (Claude 系列)
- Gemini (Google)
- DeepSeek, Moonshot (Kimi), Minimax
- Ollama (本地模型)
- Jina (Embedding)
- OpenRouter (聚合平台)

### MCP Server
- **协议**: MCP/JSON-RPC 2.0
- **传输**: SSE (Server-Sent Events)
- **功能**: Resources, Prompts, Tools
- **认证**: 与网关共用 API Key 体系

### Observability
- **Prometheus 指标**: `/metrics` 端点
- **SQLite 存储**: Usage 记录和 Metrics 事件
- **Web Dashboard**: `/dashboard` 可视化界面
- **趋势分析**: 日/周/月请求量趋势
- **排行榜**: Team/Model/Channel 使用量排名

## Architecture Highlights

### 核心架构模式

```
Client Apps → [Apex Gateway] → Provider Adapters → LLM Providers
                 │
                 ├── Auth & Rate Limit
                 ├── Smart Router
                 ├── MCP Server
                 └── Metrics & Usage Logger
```

### 中间件链
- **Auth Middleware**: API Key 验证
- **Rate Limit Middleware**: RPM/TPM 限制
- **Policy Middleware**: Team Policy 检查
- **Metrics Middleware**: 指标采集

## Development Overview

### Prerequisites

**Backend:**
- Rust (edition 2024)
- Cargo

**Web:**
- Node.js 18+
- npm/pnpm/bun

### Getting Started

#### Backend Setup

```bash
# 安装依赖
cargo build

# 开发模式运行
cargo run -- --config config.json

# 调试模式
RUST_LOG=debug cargo run -- --config config.json

# 运行测试
cargo test
```

#### Web Setup

```bash
cd web

# 安装依赖
npm install

# 开发模式
npm run dev

# 构建生产版本
npm run build

# 运行测试
npm test
```

### Key Commands

#### Backend

- **Build:** `cargo build --release`
- **Dev:** `cargo run -- --config config.json`
- **Test:** `cargo test`
- **Lint:** `cargo fmt && cargo clippy`

#### Web

- **Dev:** `npm run dev`
- **Build:** `npm run build`
- **Test:** `npm test` (Playwright)
- **Lint:** `npm run lint`

## Repository Structure

```
apex/
├── src/                      # Rust 源代码
│   ├── main.rs               # CLI 入口和命令处理
│   ├── server.rs             # HTTP 服务器和路由
│   ├── config.rs             # 配置解析
│   ├── providers.rs          # LLM 提供商客户端
│   ├── router_selector.rs    # 路由选择逻辑
│   ├── database.rs           # SQLite 数据库操作
│   ├── usage.rs              # Usage 记录
│   ├── metrics.rs            # Prometheus 指标
│   ├── compliance.rs         # 合规性检查
│   ├── logs.rs               # 日志配置
│   ├── converters.rs         # 协议转换
│   ├── mcp/                  # MCP 服务器模块
│   └── middleware/           # HTTP 中间件
│       ├── auth.rs           # 认证中间件
│       └── ratelimit.rs      # 限流中间件
├── tests/                    # Rust 集成测试
├── web/                      # Next.js 前端
│   ├── src/app/              # App Router 页面
│   ├── dashboard/            # Dashboard 页面
│   └── tests/                # Playwright 测试
├── docs/                     # 项目文档
│   ├── current/              # 当前事实文档
│   └── index.md              # 文档导航
├── _bmad-output/             # BMAD 工作流产物
│   ├── planning/             # 规划与研究
│   ├── implementation/       # story / tech spec / sprint status
│   └── test-artifacts/       # 历史测试产物
├── config.example.json       # 配置示例
└── Cargo.toml                # Rust 依赖配置
```

## Documentation Map

For detailed information, see:

- [index.md](./index.md) - Master documentation index
- [system-overview.md](./architecture/system-overview.md) - 系统架构
- [operations.md](./guides/operations.md) - 操作手册

---

_Generated using BMAD Method `document-project` workflow_
