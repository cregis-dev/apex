---
title: '修复 MCP 路由走 team 认证的问题'
slug: 'mcp-team-auth-fix'
created: '2026-03-03'
status: 'ready-for-dev'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['Rust', 'Axum']
files_to_modify: ['src/server.rs']
code_patterns: ['axum middleware', 'router nest', 'tower ServiceBuilder']
test_patterns: ['cargo test']
---

# Tech-Spec: 修复 MCP 路由走 team 认证的问题

**Created:** 2026-03-03

## Overview

### Problem Statement

MCP client 连接时，正确填写了 Authorization header (全局 API key)，但服务端报错：`Auth Failed: Invalid Team API Key 'goodluck123' provided in Authorization`。

这是因为 MCP 路由被错误地应用了 team 认证中间件，应该只使用全局 API key 认证。

### Solution

修改 `server.rs` 中的路由构建逻辑，让 MCP 路由独立于 model_routes，避免继承 team_auth 中间件。

### Scope

**In Scope:**
- 修改 `src/server.rs` 路由构建逻辑
- MCP 路由只使用全局 API key 认证 (`mcp_auth_guard`)
- 确保模型请求仍然走团队认证

**Out of Scope:**
- 不修改 team 认证逻辑
- 不修改全局认证逻辑

## Context for Development

### Codebase Patterns

- Axum 路由使用 `.layer()` 添加中间件
- `.merge()` 会继承父路由的中间件
- `.nest()` 也会继承父路由的中间件（关键问题点）
- 全局中间件通过 `tower::ServiceBuilder` 添加

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `src/server.rs:200-260` | HTTP 服务器主逻辑，路由构建 |
| `src/middleware/auth.rs` | team_auth 中间件实现 |
| `src/mcp/server.rs:772-843` | mcp_auth_guard 中间件实现 |

### Technical Decisions

**问题代码结构 (server.rs:206-248):**
```rust
// model_routes 有 team_auth 中间件
let model_routes = Router::new()
    .layer(team_policy)
    .layer(team_auth);

// admin_routes 合并 model_routes，继承 team_auth
let mut admin_routes = Router::new()
    .merge(model_routes);

// nest 也会继承 team_auth！MCP 请求会被 team_auth 拦截
admin_routes = admin_routes.nest(
    "/mcp",
    Router::new()
        .layer(mcp_auth_guard)  // <-- 在 team_auth 之后执行，但已经被拦截
);
```

**修复方案 (Party Mode 讨论优化):**
创建独立的 MCP 路由，与 model_routes 分别添加各自的认证中间件，避免混在一起。

```rust
// Model routes - explicitly with team auth
let model_routes = Router::new()
    .route("/v1/chat/completions", post(handle_openai))
    ...
    .layer(team_policy)
    .layer(team_auth);

// MCP routes - standalone with mcp auth (创建独立 Router)
let mcp_routes = Router::new()
    .route("/mcp/sse", get(sse_handler))
    .route("/mcp/messages", post(messages_handler))
    .layer(axum::middleware::from_fn_with_state(
        state.mcp_server.as_ref().clone(),
        crate::mcp::server::mcp_auth_guard,
    ));

// Admin routes - merge both WITHOUT team_auth at this level
let admin_routes = Router::new()
    .merge(model_routes)
    .merge(mcp_routes)
    .route("/admin/teams", get(handle_admin_teams))
    ...
```

**关键点:**
- MCP 路由必须是独立的 Router，不能通过 `.nest()` 添加
- `admin_routes` 级别不再有 `team_auth`，`team_auth` 只在 `model_routes` 中
- 这样架构更清晰：每种路由类型管理自己的认证

## Implementation Plan

### Tasks

- [ ] Task 1: 重构 `server.rs` 路由构建逻辑
  - File: `src/server.rs`
  - Action:
    1. 创建独立的 `mcp_routes` Router，添加 mcp_auth_guard 中间件
    2. 将 `team_auth` 中间件只保留在 `model_routes` 中
    3. `admin_routes` 通过 `.merge()` 合并两个路由，不添加任何认证中间件
  - Notes: 架构更清晰，MCP 和 Model 路由各自管理自己的认证

### Acceptance Criteria

- [ ] AC 1: Given MCP 客户端发送带有正确全局 API key 的请求，当请求到达 `/mcp/sse` 或 `/mcp/messages` 时，then 通过 mcp_auth_guard 认证成功
- [ ] AC 2: Given MCP 客户端发送带有错误 API key 的请求，当请求到达 `/mcp/sse` 或 `/mcp/messages` 时，then 返回 401 Unauthorized（MCP 不会走 team_auth）
- [ ] AC 3: Given 模型请求 (如 `/v1/chat/completions`) 发送带有团队 API key 的请求，当请求到达时，then 通过 team_auth 认证成功
- [ ] AC 4: Given 模型请求发送不带 API key 或带错误的 API key，当请求到达时，then 返回 401 Unauthorized
- [ ] AC 5: When 运行 `cargo test`，then 所有测试通过

## Additional Context

### Dependencies

无新依赖。

### Testing Strategy

- 单元测试：运行 `cargo test` 验证修复没有破坏其他功能
- 手动测试：使用 MCP 客户端连接验证

### Notes

- 风险：修改路由构建方式，需确保 `mcp_auth_guard` 正确应用于 MCP 路由
- 修复后 MCP 路由路径保持不变：`/mcp/sse` 和 `/mcp/messages`
- 架构改进：认证逻辑分离，每种路由类型管理自己的中间件
