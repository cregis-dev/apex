# Apex Gateway 配置示例

本文档详细说明 Apex Gateway 的所有配置选项。JSON 版本见 [config.example.json](../config.example.json)。

## 目录

- [顶层配置](#顶层配置)
- [Global 全局设置](#global-全局设置)
- [Logging 日志配置](#logging-日志配置)
- [Channels 通道定义](#channels-通道定义)
- [Routers 路由规则](#routers-路由规则)
- [Teams 团队配置](#teams-团队配置)
- [Prompts 提示词模板](#prompts-提示词模板)
- [Metrics 指标配置](#metrics-指标配置)
- [Hot Reload 热重载](#hot-reload-热重载)

---

## 顶层配置

```json
{
  "version": "1.0",
  "global": { ... },
  "logging": { ... },
  "data_dir": "...",
  "web_dir": "...",
  "channels": [ ... ],
  "routers": [ ... ],
  "teams": [ ... ],
  "prompts": [ ... ],
  "metrics": { ... },
  "hot_reload": { ... }
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `version` | string | 是 | 配置文件版本，当前为 "1.0" |
| `global` | object | 是 | 全局服务器设置 |
| `logging` | object | 否 | 日志配置，默认为 info 级别 |
| `data_dir` | string | 否 | 运行数据目录，默认 `~/.apex/data` |
| `web_dir` | string | 否 | Web 静态目录覆盖项，默认 `target/web`，仅文件系统模式使用 |
| `channels` | array | 否 | 通道列表，默认为空 |
| `routers` | array | 否 | 路由规则列表，默认为空 |
| `teams` | array | 否 | 团队列表，默认为空 |
| `prompts` | array | 否 | MCP 提示词模板 |
| `metrics` | object | 是 | 指标配置 |
| `hot_reload` | object | 是 | 热重载配置 |

---

## Web 静态资源目录

`web_dir` 是可选字段。

使用规则：

- 未启用 `embedded-web` 时，后端从该目录读取静态导出资源
- 启用 `embedded-web` 时，发布二进制从内嵌资源读取 Dashboard，`web_dir` 不再是发布必需项
- 开发态如果需要从文件系统读取静态资源，建议使用默认值 `target/web`

示例：

```json
"web_dir": "target/web"
```

---

## Global 全局设置

```json
"global": {
  "listen": "0.0.0.0:12356",
  "auth": { ... },
  "timeouts": { ... },
  "retries": { ... },
  "gemini_replay": { ... }
}
```

### listen

| 类型 | 默认值 | 说明 |
|------|--------|------|
| string | "0.0.0.0:12356" | 服务器监听地址和端口 |

### auth

```json
"auth": {
  "mode": "api_key",
  "keys": ["sk-key1", "sk-key2"]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `mode` | string | 认证模式：`"none"`（无认证）或 `"api_key"`（API Key 认证） |
| `keys` | array | API Key 列表，客户端通过 `X-API-Key` header 传递 |

### timeouts

```json
"timeouts": {
  "connect_ms": 1000,
  "request_ms": 10000,
  "response_ms": 30000
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `connect_ms` | number | 连接超时（毫秒） |
| `request_ms` | number | 请求超时（毫秒） |
| `response_ms` | number | 响应超时（毫秒） |

### retries

```json
"retries": {
  "max_attempts": 3,
  "backoff_ms": 100,
  "retry_on_status": [500, 502, 503, 504]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `max_attempts` | number | 最大重试次数 |
| `backoff_ms` | number | 重试间隔（毫秒） |
| `retry_on_status` | array | 需要重试的 HTTP 状态码 |

### gemini_replay

```json
"gemini_replay": {
  "ttl_hours": 24
}
```

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `ttl_hours` | number | `24` | Gemini Claude Code 兼容层持久化 replay state 的 TTL，单位为小时。用于恢复 `thought_signature` 和缺失的 tool turn 历史 |

---

## Logging 日志配置

```json
"logging": {
  "level": "info",
  "dir": "logs"
}
```

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `level` | string | "info" | 日志级别：`trace`, `debug`, `info`, `warn`, `error` |
| `dir` | string | null | 日志目录，支持 `~` 表示 home 目录 |

---

## Channels 通道定义

Channels 定义上游 LLM 提供商的连接配置。

```json
"channels": [
  {
    "name": "openai-coding",
    "provider_type": "openai",
    "base_url": "https://api.openai.com/v1",
    "api_key": "${OPENAI_API_KEY}",
    "headers": { "X-Custom": "value" },
    "model_map": { "gpt-4": "gpt-4-turbo" },
    "timeouts": { "connect_ms": 2000, "request_ms": 60000, "response_ms": 60000 }
  }
]
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 通道名称（供路由引用） |
| `provider_type` | string | 是 | 提供商类型：`openai`, `anthropic`, `gemini`, `deepseek`, `moonshot`, `minimax`, `ollama`, `jina`, `openrouter`, `zai` |
| `base_url` | string | 是 | API 基础 URL |
| `api_key` | string | 是 | API Key，支持环境变量 `${VAR_NAME}` |
| `anthropic_base_url` | string | 否 | 该 provider 在 Anthropic 协议下使用的基础 URL。适用于原生同时支持 OpenAI / Anthropic 协议的 provider，如 `deepseek`, `moonshot`, `minimax`, `ollama`, `openrouter`。`zai` 不需要该字段，Anthropic 客户端请求会通过网关转换后复用 `https://api.z.ai/api/paas/v4/` |
| `headers` | object | 否 | 自定义 HTTP 头 |
| `model_map` | object | 否 | 模型映射：key = 请求模型名，value = 实际提供商模型 |
| `timeouts` | object | 否 | 通道级别超时覆盖 |

---

## Routers 路由规则

Routers 决定如何将请求路由到不同的通道。

```json
"routers": [
  {
    "name": "default",
    "rules": [
      {
        "match": {
          "models": ["gpt-4*", "claude-3-opus"]
        },
        "channels": [
          { "name": "openai-coding", "weight": 1 }
        ],
        "strategy": "round_robin"
      }
    ],
    "fallback_channels": ["minimax-coding"]
  }
]
```

### Router 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | string | 路由名称 |
| `rules` | array | 路由规则列表，按顺序匹配 |
| `fallback_channels` | array | 备用通道列表（主通道全部失败时使用） |

### Rule 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `match.models` | array | 匹配的模型模式，支持通配符 `*` |
| `channels` | array | 目标通道列表（带权重） |
| `strategy` | string | 负载策略：`round_robin`, `random`, `weighted` |

### Channel 权重

```json
{ "name": "channel-name", "weight": 1 }
```

权重用于加权轮询（weighted round-robin）。

---

## Teams 团队配置

Teams 实现多租户路由和策略控制。

```json
"teams": [
  {
    "id": "demo-team",
    "api_key": "sk-team-key",
    "policy": {
      "allowed_routers": ["default"],
      "allowed_models": ["gpt-4", "claude-3-5-sonnet"],
      "rate_limit": { "rpm": 100, "tpm": 50000 }
    }
  }
]
```

### Team 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | string | 团队 ID |
| `api_key` | string | 团队 API Key（通过 `X-API-Key` header 传递） |
| `policy` | object | 团队策略 |

### Policy 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `allowed_routers` | array | 允许使用的路由 |
| `allowed_models` | array | 允许使用的模型（null = 允许所有） |
| `rate_limit` | object | 速率限制 |

---

## Prompts 提示词模板

MCP 协议的预定义提示词模板。

```json
"prompts": [
  {
    "name": "code-review",
    "description": "Generate a code review",
    "arguments": [
      { "name": "language", "description": "Programming language", "required": true }
    ],
    "messages": [
      { "role": "system", "content": { "text": "You are an expert code reviewer." } },
      { "role": "user", "content": { "text": "Review this {{language}} code.\n\n```\n{{diff}}\n```" } }
    ]
  }
]
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | string | 提示词名称 |
| `description` | string | 提示词描述 |
| `arguments` | array | 参数列表 |
| `messages` | array | 消息列表（支持 `{{variable}}` 占位符） |

---

## Metrics 指标配置

```json
"metrics": {
  "enabled": true,
  "path": "/metrics"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `enabled` | boolean | 是否启用 Prometheus 指标 |
| `path` | string | 指标端点路径 |

### 可用指标

- `apex_requests_total` - 总请求数（按路由分组）
- `apex_errors_total` - 错误总数
- `apex_token_total` - Token 消耗（按模型/通道分组）
- `apex_upstream_latency_ms` - 上游延迟

---

## Hot Reload 热重载

```json
"hot_reload": {
  "config_path": "config.json",
  "watch": true
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `config_path` | string | 配置文件路径 |
| `watch` | boolean | 是否监听文件变化自动重载 |

启用后，修改配置文件无需重启服务器即可生效。
