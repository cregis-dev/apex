# Apex Gateway - API Contracts

**Generated:** 2026-03-10
**Scope:** Backend API 端点文档

## API 概览

Apex Gateway 提供以下 API 端点：

| 端点 | 方法 | 说明 | 认证 |
|------|------|------|------|
| `/v1/chat/completions` | POST | OpenAI 兼容聊天接口 | Required |
| `/v1/messages` | POST | Anthropic 兼容接口 | Required |
| `/v1/models` | GET | 可用模型列表 | Required |
| `/api/usage` | GET | Usage 记录查询 | Required |
| `/api/metrics` | GET | Metrics 汇总 | Required |
| `/api/metrics/trends` | GET | 趋势数据 | Required |
| `/api/metrics/rankings` | GET | 排行榜数据 | Required |
| `/mcp` | GET/POST/DELETE | MCP Streamable HTTP | Required |
| `/metrics` | GET | Prometheus 指标 | Optional |
| `/dashboard` | GET | Web Dashboard | Required |

---

## LLM Proxy API

### POST /v1/chat/completions

OpenAI 兼容的聊天补全接口。

**Request Headers:**
```
Authorization: Bearer sk-ap-xxx
Content-Type: application/json
```

**Request Body:**
```json
{
  "model": "gpt-4",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "temperature": 0.7,
  "max_tokens": 1000,
  "stream": false
}
```

**Response (Success 200):**
```json
{
  "id": "chatcmpl-xxx",
  "object": "chat.completion",
  "created": 1709999999,
  "model": "gpt-4",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "Hello! How can I help you?"
    },
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 10,
    "completion_tokens": 8,
    "total_tokens": 18
  }
}
```

**Response (Error 429 Rate Limit):**
```json
{
  "error": {
    "message": "Rate limit exceeded",
    "type": "rate_limit_error",
    "code": "too_many_requests"
  }
}
```

**Response (Error 502 Bad Gateway):**
```json
{
  "error": {
    "message": "Upstream provider error",
    "type": "upstream_error",
    "code": "bad_gateway"
  }
}
```

**Query Parameters:**
| 参数 | 类型 | 说明 |
|------|------|------|
| `stream` | boolean | 是否流式输出 (也可以在 body 中指定) |

---

### POST /v1/messages

Anthropic 兼容的消息接口。

**Request Headers:**
```
Authorization: Bearer sk-ap-xxx
Content-Type: application/json
x-api-key: sk-ap-xxx (alternative)
```

**Request Body:**
```json
{
  "model": "claude-3-opus-20240229",
  "max_tokens": 1024,
  "messages": [
    {"role": "user", "content": "Hello!"}
  ]
}
```

**Response (Success 200):**
```json
{
  "id": "msg_xxx",
  "type": "message",
  "role": "assistant",
  "content": [{
    "type": "text",
    "text": "Hello! How can I help you?"
  }],
  "model": "claude-3-opus-20240229",
  "usage": {
    "input_tokens": 10,
    "output_tokens": 8
  }
}
```

---

### GET /v1/models

获取可用模型列表。

**Response (Success 200):**
```json
{
  "object": "list",
  "data": [
    {
      "id": "gpt-4",
      "object": "model",
      "created": 1709999999,
      "owned_by": "openai"
    },
    {
      "id": "claude-3-opus-20240229",
      "object": "model",
      "created": 1709999999,
      "owned_by": "anthropic"
    }
  ]
}
```

---

## Observability API

### GET /api/usage

获取 Usage 使用记录。

**Query Parameters:**
| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `team_id` | string | 否 | - | 按团队筛选 |
| `router` | string | 否 | - | 按路由筛选 |
| `channel` | string | 否 | - | 按渠道筛选 |
| `model` | string | 否 | - | 按模型筛选 |
| `limit` | integer | 否 | 50 | 每页数量 (max 100) |
| `offset` | integer | 否 | 0 | 偏移量 |

**Request Example:**
```
GET /api/usage?team_id=demo-team&limit=20&offset=0
```

**Response (Success 200):**
```json
{
  "data": [
    {
      "id": 1,
      "timestamp": "2024-01-01T10:00:00Z",
      "team_id": "demo-team",
      "router": "default-router",
      "channel": "openai-main",
      "model": "gpt-4",
      "input_tokens": 100,
      "output_tokens": 200
    }
  ],
  "total": 1000,
  "limit": 20,
  "offset": 0
}
```

---

### GET /api/metrics

获取 Metrics 汇总数据。

**Response (Success 200):**
```json
{
  "total_requests": 10000,
  "total_errors": 50,
  "total_fallbacks": 20,
  "avg_latency_ms": 150.5,
  "total_input_tokens": 500000,
  "total_output_tokens": 800000
}
```

---

### GET /api/metrics/trends

获取趋势数据。

**Query Parameters:**
| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `period` | string | 否 | daily | `daily`, `weekly`, `monthly` |
| `start_date` | string | 否 | - | 开始日期 (YYYY-MM-DD) |
| `end_date` | string | 否 | - | 结束日期 (YYYY-MM-DD) |
| `team_id` | string | 否 | - | 按团队筛选 |

**Request Example:**
```
GET /api/metrics/trends?period=daily&start_date=2024-01-01&end_date=2024-01-07
```

**Response (Success 200):**
```json
{
  "period": "daily",
  "data": [
    {
      "date": "2024-01-01",
      "requests": 1000,
      "errors": 5,
      "fallbacks": 2,
      "input_tokens": 50000,
      "output_tokens": 80000,
      "avg_latency_ms": 145.2
    },
    {
      "date": "2024-01-02",
      "requests": 1200,
      "errors": 6,
      "fallbacks": 3,
      "input_tokens": 60000,
      "output_tokens": 95000,
      "avg_latency_ms": 152.1
    }
  ]
}
```

---

### GET /api/metrics/rankings

获取排行榜数据。

**Query Parameters:**
| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `by` | string | 否 | team | `team`, `model`, `channel` |
| `limit` | integer | 否 | 10 | 返回数量 |
| `start_date` | string | 否 | - | 开始日期 |
| `end_date` | string | 否 | - | 结束日期 |

**Request Example:**
```
GET /api/metrics/rankings?by=team&limit=5
```

**Response by Team (Success 200):**
```json
{
  "by": "team",
  "data": [
    {
      "name": "team-a",
      "requests": 5000,
      "input_tokens": 250000,
      "output_tokens": 400000,
      "percentage": 40
    },
    {
      "name": "team-b",
      "requests": 3000,
      "input_tokens": 150000,
      "output_tokens": 250000,
      "percentage": 30
    }
  ]
}
```

**Response by Model (Success 200):**
```json
{
  "by": "model",
  "data": [
    {
      "name": "gpt-4",
      "requests": 3500,
      "input_tokens": 175000,
      "output_tokens": 280000,
      "percentage": 35
    },
    {
      "name": "claude-3-opus",
      "requests": 4500,
      "input_tokens": 225000,
      "output_tokens": 360000,
      "percentage": 45
    }
  ]
}
```

---

## MCP API (Streamable HTTP Transport)

### MCP 端点概览

MCP 现在使用 **Streamable HTTP** 传输模式（MCP 协议版本 2025-11-25），单一端点 `/mcp` 支持多种 HTTP 方法。

| 方法 | 说明 | 请求头 |
|------|------|--------|
| `POST /mcp` | 发送 JSON-RPC 消息，返回响应 | `Accept: application/json` 或 `text/event-stream` |
| `GET /mcp` | 监听服务端主动推送的消息 | `Accept: text/event-stream` |
| `DELETE /mcp` | 终止会话 | `MCP-Session-Id: <session_id>` |

### 核心请求头

| 请求头 | 说明 |
|--------|------|
| `MCP-Protocol-Version: 2025-11-25` | 协议版本（可选，默认使用协商版本） |
| `MCP-Session-Id: <uuid>` | 会话 ID（初始化后必须携带） |
| `Accept: application/json, text/event-stream` | 支持的响应格式 |
| `Authorization: Bearer <api_key>` | 认证（或 `x-api-key`） |

---

### POST /mcp

发送 JSON-RPC 消息到 MCP 服务器。

**Request Headers:**
```
Content-Type: application/json
Accept: application/json, text/event-stream
MCP-Session-Id: 1868a90c-...  (初始化后必须携带)
```

**Request Body (Initialize):**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2025-11-25",
    "capabilities": {},
    "clientInfo": {
      "name": "my-client",
      "version": "1.0.0"
    }
  }
}
```

**Response (Initialize):**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2025-11-25",
    "capabilities": {
      "tools": {},
      "prompts": {},
      "resources": {}
    },
    "serverInfo": {
      "name": "apex-mcp-server",
      "version": "0.1.0"
    }
  }
}
```
*注意：响应中会包含 `MCP-Session-Id` 头，后续请求必须携带此头*

---

**Request Body (List Tools):**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/list",
  "params": {},
  "MCP-Session-Id": "1868a90c-..."
}
```

**Response (List Tools):**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "tools": [
      {
        "name": "list_models",
        "description": "List available models in the gateway"
      },
      {
        "name": "echo",
        "description": "Echo back the input"
      },
      {
        "name": "query_usage_summary",
        "description": "Query aggregate usage metrics with optional team and grouping filters"
      }
    ]
  }
}
```

---

**Request Body (Call Tool):**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "list_models",
    "arguments": {}
  }
}
```

**Response (Call Tool):**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "{\"models\": [\"gpt-4\", \"claude-3-opus\"]}"
      }
    ]
  }
}
```

---

**Response (Notification - 202 Accepted):**
```
HTTP/1.1 202 Accepted
```

---

### GET /mcp

建立 SSE 流以接收服务端主动推送的消息。

**Request Headers:**
```
Accept: text/event-stream
MCP-Session-Id: 1868a90c-...
```

**Response (SSE Stream):**
```
event: connected
data: MCP stream ready

event: message
data: {"jsonrpc":"2.0","method":"notifications/tools/list_changed","params":null}
```

---

### DELETE /mcp

主动终止 MCP 会话。

**Request Headers:**
```
MCP-Session-Id: 1868a90c-...
```

**Response:**
```
HTTP/1.1 200 OK
```

---

## 认证方式---

## Monitoring API

### GET /metrics

Prometheus 格式的性能指标。

**Response (Success 200):**
```
# HELP apex_requests_total Total number of requests
# TYPE apex_requests_total counter
apex_requests_total{route="/v1/chat/completions",router="default-router"} 1000

# HELP apex_errors_total Total number of errors
# TYPE apex_errors_total counter
apex_errors_total{route="/v1/chat/completions",router="default-router"} 5

# HELP apex_fallbacks_total Total number of fallbacks
# TYPE apex_fallbacks_total counter
apex_fallbacks_total{router="default-router",channel="openai-backup"} 2

# HELP apex_upstream_latency_ms Upstream latency in milliseconds
# TYPE apex_upstream_latency_ms histogram
apex_upstream_latency_ms_bucket{route="/v1/chat/completions",le="50"} 800
apex_upstream_latency_ms_bucket{route="/v1/chat/completions",le="100"} 950
apex_upstream_latency_ms_bucket{route="/v1/chat/completions",le="+Inf"} 1000
apex_upstream_latency_ms_sum{route="/v1/chat/completions"} 75000
apex_upstream_latency_ms_count{route="/v1/chat/completions"} 1000
```

---

## Static Files

### GET /dashboard/*

Web Dashboard 静态文件。

**Request Example:**
```
GET /dashboard/
GET /dashboard/index.html
GET /dashboard/_next/static/...
```

**Response:**
- HTML/JS/CSS 静态文件
- 需要有效的 API Key 认证 (通过 localStorage 存储)

---

## 错误码说明

| HTTP 状态码 | 说明 |
|-------------|------|
| 200 | 成功 |
| 400 | 请求参数错误 |
| 401 | 未认证 (缺少或无效 API Key) |
| 403 | 无权限 (Team Policy 拒绝) |
| 429 | 速率限制 (RPM/TPM 超限) |
| 500 | 服务器内部错误 |
| 502 | 上游 Provider 错误 |

---

## 认证方式

### Header 认证 (推荐)

```
Authorization: Bearer sk-ap-xxx
```

或

```
x-api-key: sk-ap-xxx
```

### Query Param 认证 (MCP 遗留支持)

```
GET /mcp?api_key=sk-ap-xxx
```

*注意：推荐使用标准的 Authorization 头或 x-api-key 头进行认证*

---

_Generated using BMAD Method `document-project` workflow_
