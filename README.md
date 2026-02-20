# Apex AI Gateway

Simple, High Performance, High Availability AI Gateway.
面向企业内部的轻量 AI Gateway，基于 Rust 实现，使用 JSON 配置驱动，支持 OpenAI/Anthropic 兼容入口与 Proxy 转发，提供热加载、超时/重试、fallback 与 Prometheus 指标导出。

## 功能概览

- OpenAI/Anthropic 兼容入口与 Proxy 路由
- 多 provider channel，按 router 绑定与 fallback 切换
- 全局鉴权与 router vkey 鉴权
- connect/request/response 三级超时与重试
- Prometheus 指标导出
- CLI 管理配置与 router/channel

## 安装

```bash
cargo install --path .
```

## 快速开始

### 1) 初始化配置

```bash
apex init
```

默认路径：`~/.apex/config.json`

### 2) 添加 channel

```bash
apex channel add --name openai-main
```

> 系统将交互式引导您选择 Provider、确认 Base URL 并输入 API Key。

### 3) 添加 router

```bash
apex router add \
  --name default-openai \
  --type openai \
  --channel openai-main
```

### 4) 启动服务

```bash
# 前台运行
apex serve

# 后台运行 (Daemon)
apex serve -d
```

### 5) 停止服务

```bash
apex stop
```

默认监听：`0.0.0.0:12356`

## 客户端兼容性

为了更好地支持各类 AI 客户端（如 Chatbox、NextChat、Vercel AI SDK 等），Apex 提供了以下兼容性支持：

1.  **标准鉴权头**：支持使用 `Authorization: Bearer <key>`（OpenAI）或 `x-api-key: <key>`（Anthropic）。
2.  **路径兼容**：同时支持 `/v1/chat/completions` 和 `/chat/completions`（无 `/v1` 前缀）等路径，以适应不同客户端对 Base URL 的处理方式。
    - 如果客户端提示 "API endpoint not found"，请尝试将 Base URL 设置为 `http://localhost:12356` 或 `http://localhost:12356/v1`。
3.  **模型列表**：支持 `GET /v1/models`（或 `/models`）接口，返回可用模型列表（需鉴权）。

## 请求方式

### OpenAI 兼容

```bash
curl http://localhost:12356/v1/chat/completions \
  -H "content-type: application/json" \
  -H "Authorization: Bearer <router-vkey>" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"hello"}]}'
```

### Anthropic 兼容

```bash
curl http://localhost:12356/v1/messages \
  -H "content-type: application/json" \
  -H "x-api-key: <router-vkey>" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-3-5-sonnet-20240620","messages":[{"role":"user","content":"hello"}]}'
```

### Proxy 转发

```bash
curl http://localhost:12356/proxy/proxy-router/v1/models
```

## 鉴权

- 全局鉴权：请求头 `x-apex-api-key` 或 `Authorization: Bearer <key>`
- 路由鉴权：请求头 `x-apex-vkey` 或 `Authorization: Bearer <vkey>`
- provider key 仅用于上游访问，保存在 channel 中

> 注意：如果同时启用全局鉴权和路由鉴权，且都使用 `Authorization` 头，可能会导致冲突。建议优先使用 `x-apex-*` 专用头，或仅使用路由鉴权（绝大多数场景）。

## 配置示例

```json
{
  "version": "1",
  "global": {
    "listen": "0.0.0.0:12356",
    "auth": { "mode": "none", "keys": null },
    "timeouts": { "connect_ms": 2000, "request_ms": 30000, "response_ms": 30000 },
    "retries": { "max_attempts": 2, "backoff_ms": 200, "retry_on_status": [429, 500, 502, 503, 504] }
  },
  "channels": [
    {
      "name": "openai-main",
      "provider_type": "openai",
      "base_url": "https://api.openai.com",
      "api_key": "sk-xxx",
      "headers": null,
      "model_map": { "gpt-4": "gpt-4o" },
      "timeouts": null
    }
  ],
  "routers": [
    {
      "name": "default-openai",
      "type": "openai",
      "vkey": "vk_xxxxx",
      "channel": "openai-main",
      "fallback_channels": []
    }
  ],
  "metrics": { "enabled": true, "listen": "0.0.0.0:9090", "path": "/metrics" },
  "hot_reload": { "config_path": "~/.apex/config.json", "watch": true }
}
```

## 指标

默认地址：`http://localhost:9090/metrics`

- apex_requests_total{route,router}
- apex_errors_total{route,router}
- apex_upstream_latency_ms{route,router,channel}
- apex_fallback_total{router,channel}

## CLI 参考

```bash
cargo run -- channel add|update|delete|list
cargo run -- router add|update|delete|list
```

## 测试

```bash
cargo test
```
