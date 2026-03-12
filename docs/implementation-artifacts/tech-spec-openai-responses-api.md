---
title: '支持 OpenAI Responses API (/v1/responses)'
slug: 'openai-responses-api'
created: '2026-03-06'
status: 'ready-for-dev'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ["Rust", "Axum", "Tokio"]
files_to_modify: ["src/server.rs"]
code_patterns: ["Adapter 模式", "请求/响应转换"]
test_patterns: ["集成测试 - 端点调用"]
---

# Tech-Spec: 支持 OpenAI Responses API (/v1/responses)

**Created:** 2026-03-06

## Overview

### Problem Statement

Apex Gateway 目前不支持 OpenAI 最新的 Responses API 端点 (`/v1/responses`)，需要添加兼容支持以对接新版 OpenAI 客户端。

### Solution

添加 `/v1/responses` 和 `/responses` 端点支持，复用现有的路由选择、认证鉴权、限流和 provider 转换能力。

### Scope

**In Scope:**
- 添加 `/v1/responses` 和 `/responses` 端点 (支持 POST 和流式)
- 复用现有认证、限流、路由机制
- 请求/响应直通上游 (不做格式转换)

**Out of Scope:**
- 格式转换 (由上游 provider 处理)
- Web Search / Computer Use 等高级功能

## Context for Development

### Codebase Patterns

- `RouteKind` 枚举定义在 `src/providers.rs:19`，目前有 `Openai` 和 `Anthropic`
- 端点在 `src/server.rs:207-218` 定义，使用 `handle_openai` 处理
- `process_request` 函数根据 `RouteKind` 处理请求转换
- Provider Adapter 模式处理不同 provider 的路径映射

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `src/server.rs` | HTTP 端点定义和请求处理 |
| `src/providers.rs` | RouteKind 枚举、Provider Adapter |
| `src/converters.rs` | 请求/响应转换逻辑 |

### Technical Decisions

1. **复用 handle_openai**: Responses API 与 Chat Completions API 请求格式高度兼容，可复用处理函数
2. **RouteKind 复用**: 决定复用 `RouteKind::Openai`，不需要新增枚举变体
   - adapter 会保持 `/v1/responses` 路径不变，直接传递到上游
3. **端点兼容**: 同时支持 `/v1/responses` 和 `/responses` 两种路径
4. **实现范围**: 只需修改 `src/server.rs`，添加 2 个新路由即可
5. **格式说明**: 请求体直通上游，由上游 provider 处理格式兼容性 (如 OpenAI 官方支持 Responses API)

## Implementation Plan

### Tasks

- [ ] Task 1: 添加 `/v1/responses` 端点
  - File: `src/server.rs`
  - Action: 在 `model_routes` Router 中添加 `.route("/v1/responses", post(handle_openai))`
  - Notes: 复用 `handle_openai` 处理函数

- [ ] Task 2: 添加 `/responses` 端点 (无 /v1 前缀)
  - File: `src/server.rs`
  - Action: 在 `model_routes` Router 中添加 `.route("/responses", post(handle_openai))`
  - Notes: 与 Task 1 相同

- [ ] Task 3: 验证构建
  - File: N/A
  - Action: 运行 `cargo build` 确保编译通过

- [ ] Task 4: 运行测试
  - File: N/A
  - Action: 运行 `cargo test` 确保现有测试通过

### Acceptance Criteria

- [ ] AC 1: Given 服务已启动，当发送 POST 请求到 `/v1/responses` 时，返回有效响应 (200/400/401/403/429/502)
- [ ] AC 2: Given 服务已启动，当发送 POST 请求到 `/responses` 时，返回有效响应
- [ ] AC 3: Given 配置了 Team API Key，当发送请求到 `/v1/responses` 时，认证机制正常工作
- [ ] AC 4: Given 请求频率超限，当发送请求到 `/v1/responses` 时，限流机制返回 429
- [ ] AC 5: Given 请求包含有效 model，当发送请求到 `/v1/responses` 时，请求能正确路由到上游 provider
- [ ] AC 6: Given 发送 GET 请求到 `/v1/responses` 时，返回 405 Method Not Allowed
- [ ] AC 7: Given 上游未配置，当发送请求到 `/v1/responses` 时，返回 502 Bad Gateway
- [ ] AC 8: Given 请求包含 `stream: true`，当发送请求到 `/v1/responses` 时，支持流式响应

## Additional Context

### Dependencies

- 现有 `handle_openai` 处理逻辑 (无需修改)
- 现有认证和限流中间件 (自动应用)
- 现有 Router 选择逻辑 (自动应用)

### Testing Strategy

1. 单元测试: 无需新增 (路由定义在 compile time 验证)
2. 集成测试: 启动服务后使用 curl 测试端点
   ```bash
   # 测试前请替换 YOUR_API_KEY 和 PORT

   # 测试 /v1/responses (Responses API 格式)
   curl -X POST http://localhost:{PORT}/v1/responses \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer YOUR_API_KEY" \
     -d '{"model":"gpt-4o","input":"Hello"}'

   # 测试 /responses (无 v1 前缀)
   curl -X POST http://localhost:{PORT}/responses \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer YOUR_API_KEY" \
     -d '{"model":"gpt-4o","input":"Hello"}'

   # 测试流式响应
   curl -X POST http://localhost:{PORT}/v1/responses \
     -H "Content-Type: application/json" \
     -H "Authorization: Bearer YOUR_API_KEY" \
     -d '{"model":"gpt-4o","input":"Hello","stream":true}'
   ```

### Notes

- 客户端请求体直通上游，由上游 provider 处理格式兼容性
- 响应格式也会直接由上游 provider 返回
- 端口从配置文件获取，默认为 12356
