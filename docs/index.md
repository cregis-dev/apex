# Apex Gateway Documentation Index

**Type:** Multi-part monorepo with 2 parts
**Primary Language:** Rust, TypeScript
**Architecture:** AI API Gateway with MCP Server
**Last Updated:** 2026-03-10

## Project Overview

Apex Gateway 是一个使用 Rust 编写的高性能 AI API 网关服务，旨在统一企业内部的大模型访问入口。它屏蔽了多家模型提供商（OpenAI, Anthropic, Gemini, DeepSeek 等）的接口差异，提供统一的鉴权、路由、重试与监控能力。

项目包含两个主要部分：
1. **Backend (Rust)**: 核心网关服务，支持多租户、智能路由、MCP 协议
2. **Web Dashboard (Next.js)**: 可观测性仪表板，展示 Usage 记录和 Metrics 指标

## Project Structure

This project consists of 2 parts:

### Apex Backend (backend)

- **Type:** Backend API / CLI
- **Location:** `/` (project root)
- **Tech Stack:** Rust 2024, Axum, Tokio, SQLite, Prometheus
- **Entry Point:** `src/main.rs`

### Web Dashboard (web)

- **Type:** Web Application
- **Location:** `/web`
- **Tech Stack:** Next.js 16, React 19, shadcn/ui, Tailwind CSS
- **Entry Point:** `web/src/app/page.tsx`

## Quick Reference

### Backend Quick Ref

- **Stack:** Rust 2024, Axum 0.7.5, Tokio 1.43.0, SQLite
- **Entry:** `src/main.rs`
- **Pattern:** API Gateway with Provider Adapters
- **Database:** SQLite (rusqlite 0.32)
- **Deployment:** Standalone binary / Docker

### Web Quick Ref

- **Stack:** Next.js 16, React 19, shadcn/ui, Tailwind CSS 4
- **Entry:** `web/src/app/page.tsx`
- **Pattern:** App Router with Static Export
- **Output:** `target/web` (served by backend)

## Generated Documentation

### Core Documentation

- [Project Overview](./project-overview.md) - Executive summary and high-level architecture
- [Source Tree Analysis](./source-tree-analysis.md) - Annotated directory structure

### Backend Documentation

- [Architecture](./architecture-backend.md) - Backend technical architecture
- [API Contracts](./api-contracts-backend.md) - API endpoints and schemas
- [Data Models](./data-models-backend.md) - Database schema and models
- [Development Guide](./development-guide-backend.md) - Backend setup and dev workflow

### Web Documentation

- [Architecture](./architecture-web.md) - Web Dashboard technical architecture
- [Component Inventory](./component-inventory-web.md) - Catalog of UI components
- [Development Guide](./development-guide-web.md) - Web setup and dev workflow

### Integration

- [Current Release Model](./current-release-model.md) - Canonical build and release behavior for web assets
- [Integration Architecture](./integration-architecture.md) - How parts communicate
- [Deployment Guide](./deployment-guide.md) - Deployment process and infrastructure
- [Project Parts Metadata](./project-parts.json) - Machine-readable structure

### Part-Specific Documentation

#### Apex Backend (backend)

- [Architecture](./architecture-backend.md) - Technical architecture for backend
- [API Contracts](./api-contracts-backend.md) - API endpoints and schemas
- [Data Models](./data-models-backend.md) - Database schema and models
- [Development Guide](./development-guide-backend.md) - Setup and dev workflow

#### Web Dashboard (web)

- [Architecture](./architecture-web.md) - Technical architecture for web
- [Components](./component-inventory-web.md) - Component catalog
- [Development Guide](./development-guide-web.md) - Setup and dev workflow

### Integration

- [Integration Architecture](./integration-architecture.md) - How parts communicate
- [Project Parts Metadata](./project-parts.json) - Machine-readable structure

### Optional Documentation

- [Deployment Guide](./deployment-guide.md) - Deployment process and infrastructure
- [Contribution Guide](./contribution-guide.md) - Contributing guidelines and standards

## Existing Documentation

### Product Requirements

- [PRD](./PRD.md) - 产品需求文档，包含完整的功能需求和技术需求

### Architecture & Design

- [architecture.md](./architecture.md) - 架构设计文档
- [ux-design-specification.md](./planning-artifacts/ux-design-specification.md) - UX 设计规范

### Epics

- [E01-Core-Gateway.md](./epics/E01-Core-Gateway.md) - 核心网关功能
- [E02-Advanced-Routing.md](./epics/E02-Advanced-Routing.md) - 高级路由功能
- [E03-CLI-Management.md](./epics/E03-CLI-Management.md) - CLI 管理工具
- [E04-Observability.md](./epics/E04-Observability.md) - 可观测性 (Metrics, Logging, Dashboard)
- [E05-Rule-Based-Routing.md](./epics/E05-Rule-Based-Routing.md) - 基于规则的路由
- [E06-Team-Governance.md](./epics/E06-Team-Governance.md) - 团队治理与多租户
- [E07-Data-Compliance.md](./epics/E07-Data-Compliance.md) - 数据合规性
- [E08-MCP-Server.md](./epics/E08-MCP-Server.md) - MCP 服务器

### Implementation Artifacts

- [8-1-mcp-protocol-transport.md](./implementation-artifacts/8-1-mcp-protocol-transport.md) - MCP 协议与传输
- [8-2-session-lifecycle.md](./implementation-artifacts/8-2-session-lifecycle.md) - MCP 会话生命周期
- [8-3-mcp-resources.md](./implementation-artifacts/8-3-mcp-resources.md) - MCP 资源
- [8-4-resource-listing-key-masking.md](./implementation-artifacts/8-4-resource-listing-key-masking.md) - 资源列表与 Key 脱敏
- [8-5-mcp-prompts.md](./implementation-artifacts/8-5-mcp-prompts.md) - MCP 提示词
- [8-6-mcp-tools.md](./implementation-artifacts/8-6-mcp-tools.md) - MCP 工具
- [7-1-pii-masking-engine.md](./implementation-artifacts/7-1-pii-masking-engine.md) - PII 脱敏引擎
- [tech-spec-openai-responses-api.md](./implementation-artifacts/tech-spec-openai-responses-api.md) - OpenAI Responses API 支持
- [tech-spec-dashboard-heatmap.md](./implementation-artifacts/tech-spec-dashboard-heatmap.md) - Dashboard 热力图技术规格

### Test Artifacts

- [test-design-architecture.md](./test-artifacts/test-design-architecture.md) - 测试设计架构

### Operations

- [operations.md](./operations.md) - 操作手册
- [config-example.md](./config-example.md) - 配置示例说明
- [STORY-web-dashboard.md](./STORY-web-dashboard.md) - Web Dashboard 开发故事
- [LOGGING_SPEC.md](./LOGGING_SPEC.md) - 日志规范
- [WORKFLOW.md](./WORKFLOW.md) - 工作流说明
- [CHANGELOG.md](../CHANGELOG.md) - 版本变更日志

### Project Metadata

- [Project Parts](./project-parts.json) - 机器可读的项目结构元数据
- [Index](./index.md) - 本文档索引

## Getting Started

### Backend Setup

**Prerequisites:** Rust (edition 2024), Cargo

```bash
# 安装依赖
cargo build

# 开发模式运行
cargo run -- --config config.json

# 运行测试
cargo test
```

### Web Setup

**Prerequisites:** Node.js 18+, npm/pnpm

```bash
cd web

# 安装依赖
npm install

# 开发模式
npm run dev

# 构建生产版本 (输出到 target/web)
npm run build
```

## For AI-Assisted Development

This documentation was generated specifically to enable AI agents to understand and extend this codebase.

### When Planning New Features:

**UI-only features:**
→ Reference: `architecture-web.md`, `component-inventory-web.md`

**API/Backend features:**
→ Reference: `architecture-backend.md`, `api-contracts-backend.md`, `data-models-backend.md`

**Full-stack features:**
→ Reference: All architecture docs + `integration-architecture.md`

**MCP Server features:**
→ Reference: `epics/E08-MCP-Server.md`, `implementation-artifacts/8-*.md`

**Deployment changes:**
→ Review: `current-release-model.md`, `.github/workflows/`, `Dockerfile`, `docker-compose.yml`

---

_Documentation generated by BMAD Method `document-project` workflow_
