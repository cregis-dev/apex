# 操作手册：简易 AI Gateway

## 适用范围

适用于本项目的本地部署与基础使用，包含安装、配置、启动、调用与运维检查。

## 安装

### 环境要求

- Rust（edition 2024）
- 网络可访问目标 provider 的 API

### 获取代码

```bash
git clone <your-repo>
cd apex
```

### 构建与安装

```bash
cargo install --path .
apex --version
```

## 使用

### 1. 初始化配置

```bash
apex init
```

默认配置路径：`~/.apex/config.json`

### 2. 添加 channel

```bash
# 交互式添加（推荐）
apex channel add --name openai-main
```

系统将引导您选择 Provider、确认 Base URL 并输入 API Key。

```bash
# 完整参数方式
apex channel add \
  --name openai-main \
  --provider openai \
  --base-url https://api.openai.com \
  --api-key sk-xxx
```

#### 关键参数详解

- **`--name`**: Channel 的唯一标识符，用于在 Router 中引用。
- **`--provider`**: 上游服务商类型（如 `openai`, `anthropic`, `deepseek` 等）。这决定了请求的协议转换逻辑。
- **`--base-url`**: 上游 API 的基础地址。
- **`--api-key`**: 用于访问上游服务的凭证（系统会自动添加到请求头中，如 `Authorization: Bearer` 或 `x-api-key`）。

#### 可选高级配置

- **`--header key=value`**: 追加自定义请求头。可多次使用。
  - 示例: `--header "X-Organization-Id=org-123" --header "User-Agent=MyBot/1.0"`

- **`--model-map old=new`**: 模型名称重映射。可多次使用。
  - **功能**: 当客户端请求的模型名称与上游 Provider 支持的模型名称不一致时，网关会自动替换请求体中的 `model` 字段。
  - **场景**:
    1.  **别名简化**: 客户端请求 `gpt-4`，实际转发给 `gpt-4-0613`。
    2.  **跨厂商适配**: 客户端请求 `claude-3-5-sonnet`，实际转发给 Azure OpenAI 的部署名 `dep-claude-sonnet`。
    3.  **兼容性修复**: 某些旧客户端硬编码了模型名，通过映射转发到新模型。
  - **CLI 示例**:
    ```bash
    apex channel add \
      --name azure-gpt \
      --provider openai \
      --base-url https://my-azure.openai.azure.com \
      --api-key xxx \
      --model-map "gpt-4=gpt-4-32k-0613" \
      --model-map "gpt-3.5-turbo=gpt-35-turbo-16k"
    ```

- **超时设置**:
  - `--connect-ms`: 连接超时（毫秒）。
  - `--request-ms`: 整个请求超时（毫秒）。
  - `--response-ms`: 响应读取超时（毫秒）。

### 3. 添加 router

```bash
apex router add \
  --name default-openai \
  --channels openai-main
```

如需指定 vkey：

```bash
apex router add \
  --name default-openai \
  --channels openai-main \
  --vkey vk_xxxxx
```

### 4. 启动服务

```bash
# 开发调试
cargo run -- gateway start

# 生产环境（后台运行）
apex gateway start -d
```

### 5. 停止服务

```bash
apex gateway stop
```

### 6. 查看 channel 详情

```bash
apex channel show --name openai-main
```

### 7. 查看列表

```bash
apex channel list
apex router list
```

默认 channel list 不显示 Base URL，如需查看完整信息请使用 `show` 命令。

## 双协议支持 (Dual Protocol)

对于同时支持 OpenAI 和 Anthropic 协议的 Provider（如 MiniMax, DeepSeek, Moonshot），Apex 提供了特殊的双协议支持。

### 配置方式

在添加 Channel 时，除了常规的 `Base URL` (用于 OpenAI 协议) 外，还可以配置 `Anthropic Base URL`。

```bash
apex channel add \
  --name minimax \
  --provider minimax \
  --base-url https://api.minimax.io/v1 \
  --anthropic-base-url https://api.minimax.io/v1 \
  --api-key <your-key>
```

### 自动路由

- 当客户端使用 **OpenAI 协议** (e.g. `/v1/chat/completions`) 请求时，流量转发至 `Base URL`。
- 当客户端使用 **Anthropic 协议** (e.g. `/v1/messages`) 请求时，流量转发至 `Anthropic Base URL`。

这使得您可以使用同一个 Router 同时服务于 OpenAI 客户端（如 Chatbox）和 Anthropic 客户端（如 Claude Dev）。

## Router 高级指南

本节详细介绍 Router 的高级配置功能，包括多通道负载均衡、路由策略、模型路由和故障转移。

### 1. 多通道负载均衡 (Multi-Channel)

Router 支持绑定多个 Channel，并为每个 Channel 分配权重（Weight）。流量将根据权重比例分配到不同的 Channel。

**CLI 示例**:
```bash
# 创建一个混合路由，30% 流量给 deepseek，70% 给 openai
apex router add \
  --name mixed-route \
  --channels deepseek:3,openai:7 \
  --strategy round_robin
```

**配置说明**:
- `--channels name:weight`：指定 Channel 名称和权重（默认权重为 1）。
- `--strategy`：指定负载均衡策略。

### 2. 路由策略 (Strategy)

支持以下三种路由策略：

- **round_robin** (默认): 加权轮询（Weighted Round Robin）。系统根据 Channel 的权重比例随机分发请求。
- **priority**: 优先级模式。始终尝试使用列表中的第一个可用 Channel。只有当第一个 Channel 被移除或不可用时（需配合健康检查，目前主要按列表顺序），才会考虑后续。
- **random**: 纯随机模式。忽略权重，完全随机选择 Channel。

**CLI 示例**:
```bash
# 优先级模式：总是优先使用 primary，仅在特殊配置下使用 secondary
apex router add \
  --name ha-route \
  --channels primary,secondary \
  --strategy priority
```

### 3. 基于规则的路由 (Rule-Based Routing)

Apex 使用**规则链 (Rules Chain)** 来决定如何处理请求。Router 会从上到下依次匹配规则，一旦命中即停止匹配。

每条规则包含：
1.  **匹配条件 (Match)**: 如模型名称（支持 Glob 通配符）。
2.  **执行策略 (Strategy)**: 如 `round_robin` (轮询), `priority` (优先级)。
3.  **渠道列表 (Channels)**: 流量分发的目标渠道，支持权重配置。

**CLI 示例**:
目前建议直接编辑 `config.json` 来配置高级规则。

### 4. 故障转移 (Failover)

故障转移现在内置于每条规则的 `channels` 列表中。如果选中的 Channel 请求失败（如超时、50x 错误），Router 会自动尝试列表中的下一个可用 Channel（根据策略决定）。

### 5. 完整配置示例 (JSON)

手动编辑 `config.json` 可以进行精细的路由配置：

```json
{
  "routers": [
    {
      "name": "unified-api",
      "vkey": "sk-proj-123456",
      "rules": [
        {
          "//": "规则1：Minimax 模型 -> 双路负载均衡",
          "match": {
            "models": ["abab*", "def*"]
          },
          "strategy": "round_robin",
          "channels": [
            { "name": "minimax-key-1", "weight": 1 },
            { "name": "minimax-key-2", "weight": 1 }
          ]
        },
        {
          "//": "规则2：Gemini 模型 -> 走 Google 官方 (单 Channel 可省略 strategy)",
          "match": {
            "model": "gemini*"
          },
          "channels": [
            { "name": "google-official" }
          ]
        },
        {
          "//": "规则3：默认兜底 -> OpenRouter 主用，OpenAI 备用",
          "match": {
            "model": "*"
          },
          "strategy": "priority",
          "channels": [
            { "name": "openrouter-agg", "weight": 10 },
            { "name": "openai-backup", "weight": 1 }
          ]
        }
      ]
    }
  ]
}
```

> **配置说明**: 
> 1. `match` 字段支持 `model` (单字符串) 或 `models` (字符串数组) 两种格式，方便配置多组匹配规则。
> 2. `strategy` 字段默认为 `round_robin`。如果 `channels` 只有一个，可以省略 `strategy` 和 `weight`。
> 3. 旧版本的 `model_matcher` 和顶层 `channels` 配置仍然支持，但在加载时会自动转换为上述规则格式。建议新配置直接使用 `rules`。

## 调用示例

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

## 鉴权

### 认证方式

Apex 支持通过标准请求头传递凭证：

1. **OpenAI 协议**：
   - `Authorization: Bearer <key>`
2. **Anthropic 协议**：
   - `x-api-key: <key>`

### 鉴权层级

- **全局鉴权**：保护整个网关，需在 `config.json` 中配置 `global.auth`。
- **路由鉴权**：保护特定路由，需在 Router 配置中设置 `vkey`。
- **Provider Key**：仅用于访问上游服务，保存在 Channel 配置中，不对外暴露。

> 提示：在使用 Chatbox、NextChat 等标准 OpenAI 客户端时，直接将 Router VKey 填入 API Key 字段即可。

## 配置结构

配置文件为 JSON，主要结构如下：

- `global.listen`：监听地址
- `global.auth`：全局鉴权
- `global.timeouts`：全局超时
- `global.retries`：重试策略
- `channels`：上游通道集合
- `routers`：路由集合
- `metrics`：指标导出
- `hot_reload`：热加载开关

### 示例

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
      "model_map": null,
      "timeouts": null
    }
  ],
  "routers": [
    {
      "name": "default-openai",
      "vkey": "vk_xxxxx",
      "channels": [
          {"name": "openai-main", "weight": 1}
      ],
      "fallback_channels": []
    }
  ],
  "metrics": { "enabled": true, "listen": "0.0.0.0:9090", "path": "/metrics" },
  "hot_reload": { "config_path": "~/.apex/config.json", "watch": true }
}
```

## 运维检查

### 指标

默认地址：`http://localhost:9090/metrics`

- apex_requests_total{route,router}
- apex_errors_total{route,router}
- apex_upstream_latency_ms{route,router,channel}
- apex_fallback_total{router,channel}

### 常见问题

- 401：检查 `x-apex-api-key` / `x-apex-vkey`
- 404：检查 router 名称与 channel 绑定
- 502：检查上游 base_url 与网络连通性

## 测试

```bash
cargo test
```
