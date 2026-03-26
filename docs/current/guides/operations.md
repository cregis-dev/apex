# 操作手册：Apex AI Gateway

## 适用范围

适用于本项目的本地部署与基础使用，包含安装、配置、启动、调用与运维检查。

## 安装

### 推荐：Docker 部署

请参考 [README](../README_zh-CN.md) 中的快速开始指南。

### 手动安装 (Cargo)

#### 环境要求
- Rust (edition 2024)
- 网络可访问目标 provider 的 API

#### 获取代码
```bash
git clone <your-repo>
cd apex
```

#### 构建与安装
```bash
cargo install --path .
apex --version
```

### 交叉编译 (Cross-Compilation)

如果您需要在 macOS 或 Windows 上构建 Linux 二进制文件，推荐使用 `cross` 工具，它会自动处理交叉编译环境。

1. **安装 cross**:
   ```bash
   cargo install cross
   ```

2. **构建 Linux (musl) 静态链接版本**:
   此版本不依赖系统库，适用于任何 Linux 发行版（包括 Alpine）。
   ```bash
   cross build --target x86_64-unknown-linux-musl --release
   ```
   构建产物位于：`target/x86_64-unknown-linux-musl/release/apex`

3. **构建 Linux (gnu) 动态链接版本**:
   适用于 Ubuntu, CentOS 等标准 Linux 系统。
   ```bash
   cross build --target x86_64-unknown-linux-gnu --release
   ```

## 核心概念

Apex 使用 **Team (团队)** 作为鉴权和治理的核心单元。
- **Team**: 拥有一个唯一的 API Key (自动生成)，并关联特定的权限策略 (Policy)。
- **Router**: 流量入口，负责将请求分发给后端 Channel。
- **Channel**: 上游 Provider 的连接通道 (包含 API Key, Base URL 等)。

## 使用流程

### 1. 初始化配置

```bash
apex init
```
默认配置路径：`~/.apex/config.json`

### 2. 添加 Channel (上游通道)

Channel 代表一个实际的 AI 提供商账号或端点。

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

### 3. 添加 Router (路由)

Router 定义了客户端如何访问模型。

```bash
# 创建一个基础路由，包含一个 channel
apex router add \
  --name default-openai \
  --channels openai-main
```

### 4. 添加 Team (团队/用户)

**重要**: 客户端必须使用 Team API Key 才能访问网关。

```bash
# 添加一个团队，允许访问 default-openai 路由
apex team add --id demo-team --routers default-openai
```
输出示例：
```
Team 'demo-team' added successfully.
API Key: sk-ap-XyZ123...
```
请妥善保存生成的 API Key。

### 5. 启动服务

```bash
# 前台运行
apex gateway start --config /path/to/config.json

# 后台运行 (Daemon)
apex gateway start --config /path/to/config.json -d
```

### 6. 验证调用

使用 Team API Key 发起请求：

```bash
curl http://localhost:12356/v1/chat/completions \
  -H "Authorization: Bearer <Your-Team-API-Key>" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

## 高级配置

### 团队治理与多租户 (Team Governance & Multi-Tenancy)

Apex 使用 Team ID 和 API Key 进行多租户管理。每个 Team 拥有独立的权限策略和限流配额。

#### 典型配置示例

**1. 基础接入 (Basic Access)**

最简单的场景，为团队分配一个路由的访问权限。

```bash
apex team add --id frontend-app --routers default-router
```

**2. 多路由与模型限制 (Multi-Router & Model Restrictions)**

允许团队访问多个路由，但限制只能使用特定模型（如仅允许使用低成本模型）。

```bash
apex team add \
  --id internal-testing \
  --routers openai-router,anthropic-router \
  --models "gpt-3.5-*,claude-instant-*"
```
*注意：`--models` 支持通配符匹配，且不区分大小写。*

**3. 高优先级与限流 (High Priority & Rate Limiting)**

为核心业务配置宽松的限流，防止滥用。

```bash
apex team add \
  --id core-service \
  --routers main-router \
  --rpm 1000 \
  --tpm 500000
```

**4. 严格限流 (Strict Rate Limiting)**

为试用用户或不可信来源配置严格的 RPM/TPM 限制。

```bash
apex team add \
  --id trial-user \
  --routers default-router \
  --models "gpt-3.5-turbo" \
  --rpm 5 \
  --tpm 10000
```

#### 管理命令

- **查看所有团队**: `apex team list`
- **删除团队**: `apex team remove --id <team-id>`

参数说明：
- `--routers`: (必填) 允许访问的路由列表，逗号分隔。
- `--models`: (可选) 允许访问的模型通配符列表。若不传则允许该路由下的所有模型。
- `--rpm`: (可选) 每分钟请求数限制 (Requests Per Minute)。
- `--tpm`: (可选) 每分钟 Token 数限制 (Tokens Per Minute)。

#### 配置参考 (Configuration Reference)

您可以直接编辑 `config.json` 中的 `teams` 字段进行配置：

```json
{
  "teams": [
    {
      "//": "示例1：基础接入",
      "id": "frontend-app",
      "api_key": "sk-ap-generated-key-1",
      "policy": {
        "allowed_routers": ["default-router"]
      }
    },
    {
      "//": "示例2：多路由与模型限制",
      "id": "internal-testing",
      "api_key": "sk-ap-generated-key-2",
      "policy": {
        "allowed_routers": ["openai-router", "anthropic-router"],
        "allowed_models": ["gpt-3.5-*", "claude-instant-*"]
      }
    },
    {
      "//": "示例3：高优先级与限流",
      "id": "core-service",
      "api_key": "sk-ap-generated-key-3",
      "policy": {
        "allowed_routers": ["main-router"],
        "rate_limit": {
          "rpm": 1000,
          "tpm": 500000
        }
      }
    }
  ]
}
```

### 基于规则的路由 (Rule-Based Routing)

Apex 支持强大的路由规则链。建议直接编辑 `config.json` 的 `routers` 部分：

```json
{
  "routers": [
    {
      "name": "main-router",
      "rules": [
        {
          "//": "规则1：GPT-4 走 Azure",
          "match": { "models": ["gpt-4", "gpt-4-32k"] },
          "strategy": "priority",
          "channels": [
            { "name": "azure-east-us", "weight": 1 },
            { "name": "openai-fallback", "weight": 1 }
          ]
        },
        {
          "//": "规则2：Claude 走 Anthropic",
          "match": { "model": "claude-*" },
          "channels": [{ "name": "anthropic-main" }]
        },
        {
          "//": "规则3：默认兜底",
          "match": { "model": "*" },
          "strategy": "round_robin",
          "channels": [
            { "name": "deepseek-v2", "weight": 3 },
            { "name": "minimax-v1", "weight": 1 }
          ]
        }
      ]
    }
  ]
}
```

### 双协议支持 (Dual Protocol)

对于同时支持 OpenAI 和 Anthropic 协议的 Provider（如 MiniMax, DeepSeek, Ollama, OpenRouter），配置 `anthropic_base_url`：

```bash
apex channel add \
  --name minimax \
  --provider minimax \
  --base-url https://api.minimax.io/v1 \
  --anthropic-base-url https://api.minimax.io/anthropic \
  --api-key <your-key>
```
网关会自动根据客户端请求协议（OpenAI vs Anthropic）选择对应的 Base URL。

## 运维检查

### 指标 (Metrics)
默认地址：`http://localhost:9090/metrics`
核心指标：
- `apex_requests_total`: 请求总量
- `apex_errors_total`: 错误总量
- `apex_upstream_latency_ms`: 上游延迟

### 常用命令
- `apex team list`: 查看团队及 Key
- `apex channel list`: 查看 Channel
- `apex router list`: 查看 Router
- `apex status`: 查看服务状态
- `apex logs`: 查看日志

## MCP 使用与管理 (MCP Usage & Management)

Apex 内置了 Model Context Protocol (MCP) Server，支持通过 MCP 协议向客户端（如 Claude Desktop, Cursor, AI IDEs）暴露配置、提示词 (Prompts) 和工具 (Tools)。

### 1. 启用 MCP

MCP 服务在 `config.json` 中通过 `global.enable_mcp` 配置项控制，默认为 `true`。

```json
{
  "global": {
    "enable_mcp": true
  }
}
```

启动网关后，MCP Server 会自动在以下端点可用：

- **MCP 端点**: `/mcp` (与网关共用端口，默认 12356)
- **支持方法**: GET, POST, DELETE

### 2. 运行模式

Apex 的 MCP 现在支持 **Streamable HTTP** 传输模式（MCP 协议版本 2025-11-25）。

**启动主服务**:
```bash
apex gateway start --config /path/to/config.json
```

### 3. 连接指南

如果您的 Apex 部署在 `https://gateway.cregis.ai`，MCP Client 连接配置如下：

- **MCP URL**: `https://gateway.cregis.ai/mcp`
- **Auth**: 需要提供 API Key（与 OpenAI/Anthropic 接口共用 Team/Global Key）

**认证方式**:
- `Authorization: Bearer sk-your-team-key` (推荐)
- `x-api-key: sk-your-team-key` (推荐)
- Query Param: `?api_key=sk-your-team-key` (遗留支持)

**协议头**:
- `MCP-Protocol-Version: 2025-11-25` (可选)
- `MCP-Session-Id: <uuid>` (初始化后必须携带)

**注意**:
- 初始化请求会返回 `MCP-Session-Id` 头，后续请求必须携带此头
- 所有的 MCP 交互（List Tools, Call Tool 等）都受相同的 Team Policy 控制
- POST 请求默认返回 JSON 响应，也可通过 `Accept: text/event-stream` 请求流式响应
- 确保您的 API Key 具有访问权限。
- 客户端会自动接收 `endpoint` 事件，指向 `/mcp/messages?session_id=...`（客户端需处理相对路径或追加到 Base URL）。
- 所有的 MCP 交互（List Tools, Call Tool 等）都受相同的 Team Policy 控制。

### 5. 功能特性 (Features)

#### 资源 (Resources)
Apex 通过 `resources` 暴露配置文件的只读访问，支持 `config://` 协议。

*   `config://config.json`: 完整配置文件（敏感信息如 API Key 会被自动脱敏）。
*   `config://teams`: 团队配置列表。
*   `config://routers`: 路由配置列表。
*   `config://channels`: 通道配置列表。

**使用示例**:
在 Client 中输入 `@apex-gateway/config.json` 即可读取当前网关配置。

#### 提示词 (Prompts)
在 `config.json` 中定义 `prompts`，Client 可直接调用。

**配置示例**:
```json
{
  "prompts": [
    {
      "name": "code-review",
      "description": "标准代码审查模板",
      "arguments": [
        { "name": "code", "description": "待审查代码", "required": true }
      ],
      "messages": [
        {
          "role": "user",
          "content": {
            "type": "text",
            "text": "请审查以下代码并提供改进建议：\n\n{{code}}"
          }
        }
      ]
    }
  ]
}
```

#### 工具 (Tools)
Apex 提供内置工具供 Agent 调用进行诊断或查询。

*   `list_models`: 列出当前所有 Channel 支持的模型映射关系。
*   `echo`: 测试工具，原样返回输入。

**使用示例**:
Agent 可以调用 `list_models` 来查询当前网关可用的模型列表，以便智能选择模型。

### 6. 热重载 (Hot Reload)
修改 `config.json` 后，Apex 会自动检测变更并通过 MCP 协议通知客户端刷新 Resources, Prompts 和 Tools 列表，无需重启服务或客户端。

### 7. 故障排查
*   **连接失败**: 检查 MCP 是否在配置中启用 (`global.enable_mcp: true`)，确认 API Key 是否正确。
