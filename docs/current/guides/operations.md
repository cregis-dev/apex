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

### Gemini 原生工具入口

当 channel 配置为 `provider_type: "gemini"` 时，同一个 channel 可同时服务 OpenAI/Anthropic 兼容入口和 Gemini 原生入口。原生入口使用 `/gemini/...` 前缀，例如：

```bash
curl http://127.0.0.1:12356/gemini/v1beta/models/gemini-3-flash-preview:generateContent \
  -H "Authorization: Bearer sk-ap-team" \
  -H "Content-Type: application/json" \
  -d '{
    "contents": [{"role": "user", "parts": [{"text": "Search and summarize the current docs."}]}],
    "tools": [{"google_search": {}}, {"url_context": {}}, {"code_execution": {}}]
  }'
```

Apex 会剥离客户端的 gateway 鉴权头，向上游注入 channel 的 `x-goog-api-key`，并保留 Gemini 原生字段，例如 `tools`, `toolConfig`, `generationConfig`, `groundingMetadata`, URL Context 元数据和 Code Execution parts。模型列表和 File Search Store 资源路由使用合成模型键 `gemini-native`，因此严格限制模型的团队需要把 `gemini-native` 加入 `allowed_models`。

## 使用流程

### 1. 初始化配置

```bash
apex init
```
默认配置路径：`~/.apex/config.json`

配置路径解析顺序固定为：

1. 命令行 `--config` / `-c`
2. 环境变量 `APEX_CONFIG`
3. 默认路径 `~/.apex/config.json`

常用诊断命令：

```bash
apex config path
apex config validate
APEX_CONFIG=/opt/apex/config.json apex config path
apex -c /opt/apex/config.json config validate
```

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

### 5. 启动与服务管理

前台运行推荐使用 `gateway run`：

```bash
apex gateway run
APEX_CONFIG=/opt/apex/config.json apex gateway run
```

`apex gateway start` 仍保持兼容，`apex gateway start --daemon` 仍使用内置 daemon/pid 文件模式。生产环境推荐使用原生服务管理：

```bash
apex -c /opt/apex/config.json service install --install-dir /opt/apex
apex service start --install-dir /opt/apex
apex service status --install-dir /opt/apex
apex service logs --install-dir /opt/apex
```

Linux 使用 systemd，macOS 使用 launchd user agent。升级已通过 release installer 安装的实例：

```bash
apex upgrade --dry-run --install-dir /opt/apex
apex upgrade --restart --install-dir /opt/apex
```

### 面向自动化 / AI Skills 的 CLI 使用约定

对 `channel`、`router`、`team` 这三个命令族，Apex 当前的 v1 自动化范围如下：

- `channel`: `add`, `update`, `delete`, `show`, `list`
- `router`: `add`, `update`, `delete`, `list`
- `team`: `add`, `remove`, `list`

以下动作当前**不应被假定为可用**，除非后续版本显式增加：

- `router show`
- `team show`
- `team update`

当必需参数已经通过命令参数提供时，这些命令可以直接用于本地自动化或 AI skills，不需要依赖交互式输入。

如果同时传入 `--json`，`channel`、`router`、`team` 将返回稳定的机器可读结构。v1 顶层字段为：

- `ok`
- `command`
- `message`
- `data`
- `errors`
- `meta`

```bash
# 非交互式 channel 创建
apex channel add \
  --name openai-main \
  --provider openai \
  --api-key sk-xxx
```

```bash
# 非交互式 router 创建
apex router add \
  --name default-openai \
  --channels openai-main \
  --strategy round_robin
```

```bash
# 非交互式 team 创建
apex team add \
  --id demo-team \
  --routers default-openai
```

```bash
# 机器可读 JSON 输出
apex channel list --json
apex router add --name default-openai --channels openai-main --json
apex team remove demo-team --json
```

JSON 成功响应示例：

```json
{
  "ok": true,
  "command": "channel.list",
  "message": "Channels listed successfully.",
  "data": [],
  "errors": [],
  "meta": {
    "resource": "channel",
    "action": "list"
  }
}
```

JSON 错误响应示例：

```json
{
  "ok": false,
  "command": "team.remove",
  "message": "Team 'missing-team' not found",
  "data": null,
  "errors": [
    {
      "code": "not_found",
      "message": "Team 'missing-team' not found"
    }
  ],
  "meta": {
    "resource": "team",
    "action": "remove"
  }
}
```

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
- **删除团队**: `apex team remove <team-id>`

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
- `apex team remove <team-id>`: 删除团队
- `apex channel list`: 查看 Channel
- `apex channel show <name>`: 查看单个 Channel 详情
- `apex router list`: 查看 Router
- `apex status`: 查看服务状态
- `apex logs`: 查看日志

## 控制面说明

Apex 已不再提供 HTTP MCP 产品表面，`/mcp` 路由和 `global.enable_mcp` 配置项均已退役。

当前推荐的运维与自动化入口：

- 本地配置与脚本化操作：使用 `apex channel`、`apex router`、`apex team`
- AI/skills 自动化：优先使用 CLI 参数化输入和 `--json` 输出
- 远程管理能力：后续由 Admin Control Plane 承接
