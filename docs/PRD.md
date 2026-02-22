# Apex AI Gateway PRD

## 1. 背景与目标 (Background & Goals)
Apex 是一个基于 Rust 实现的轻量级 AI 网关服务，旨在统一企业内部的大模型访问入口，屏蔽多家模型提供商（OpenAI, Anthropic, Gemini 等）的接口差异，提供统一的鉴权、路由、重试与监控能力。

### 核心目标
1.  **统一入口**：屏蔽底层 Provider 差异，提供兼容 OpenAI/Anthropic 协议的统一接口。
2.  **高可用**：支持多 Channel 负载均衡、故障转移（Fallback）、超时重试。
3.  **配置驱动**：纯 JSON 配置驱动，支持热加载，无需重启服务。
4.  **轻量高效**：无依赖单体二进制，低资源占用。
5.  **智能路由**：支持基于模型名称的内容路由（Content-Based Routing）和多通道聚合。

## 2. 范围 (Scope)
- **支持协议**：HTTP/HTTPS
- **配置格式**：JSON
- **管理方式**：CLI (命令行工具)
- **部署模式**：单机部署 / Sidecar
- **不支持**：Web UI 管理界面（当前版本）

## 3. 用户角色与场景 (User Personas & Scenarios)
### 3.1 平台管理员 (Platform Admin)
- 使用 CLI 工具初始化网关配置。
- 管理 Channel（上游供应商配置）和 Router（路由规则）。
- 配置全局鉴权策略和监控指标。
- 查看运行状态和日志。

### 3.2 业务开发者 (Developer)
- 获取网关分配的 Router `vkey`。
- 使用标准的 OpenAI SDK 或 Anthropic SDK 连接网关地址。
- 无需关心底层具体使用的是哪个 Provider 账号或 API Key。

## 4. 功能需求 (Functional Requirements)

### 4.1 核心网关 (Core Gateway)
#### 4.1.1 Provider 支持
网关需支持对接以下 Provider，并进行协议转换：
- **OpenAI** (标准协议)
- **Anthropic** (Claude 系列)
- **Gemini** (Google)
- **Deepseek** (深度求索)
- **Moonshot** (Kimi)
- **Minimax**
- **Ollama** (本地模型)
- **Jina** (Embedding)
- **OpenRouter** (聚合平台)

#### 4.1.2 路由与转发 (Routing & Proxy)
- **兼容入口**：
    - OpenAI 兼容路径：`/v1/chat/completions`, `/v1/embeddings`, `/v1/models`
    - Anthropic 兼容路径：`/v1/messages`
- **智能路由 (Smart Routing)**：
    - **基于模型匹配**：根据请求体中的 `model` 字段（支持精确匹配和 Glob 通配符），将请求转发到指定 Channel。
    - **负载均衡**：支持 `round_robin` (轮询), `random` (随机), `priority` (优先级) 策略。
    - **多 Channel 聚合**：一个 Router 可关联多个 Channel。
- **故障转移 (Fallback)**：
    - 当主 Channel 失败（返回特定状态码如 429, 500, 502）时，自动切换到 Fallback Channel。
- **重试机制 (Retries)**：
    - 支持配置最大重试次数 (`max_attempts`) 和退避时间 (`backoff_ms`)。

#### 4.1.3 鉴权 (Authentication)
- **Router 鉴权**：通过 `vkey` 验证客户端请求（Bearer Token 或 x-api-key）。
- **Global 鉴权**：可选的网关全局访问控制。
- **上游鉴权**：自动注入上游 Provider 的 API Key。

### 4.2 CLI 管理工具 (CLI Management)
提供 `apex` 命令行工具管理配置：
- `apex init`: 初始化配置向导。
- `apex gateway start/stop`: 启动/停止网关服务（支持 Daemon 模式）。
- `apex channel add/update/delete/list`: 管理上游渠道。
- `apex router add/update/delete/list`: 管理路由规则。
- `apex status`: 查看服务状态。

### 4.3 可观测性 (Observability)
- **Prometheus 指标**：
    - `apex_requests_total`: 请求总量
    - `apex_errors_total`: 错误总量
    - `apex_upstream_latency_ms`: 上游响应延迟
    - `apex_fallback_total`: Fallback 触发次数
- **日志 (Logging)**：
    - 结构化日志输出，支持文件和标准输出。

### 4.4 配置热加载 (Hot Reload)
- 监听配置文件变化，自动重载配置。
- 如果新配置非法，保持旧配置生效并记录错误。

## 5. 技术需求 (Technical Requirements)

### 5.1 架构设计
- **语言**：Rust (基于 Axum, Tokio)
- **配置结构**：
    ```rust
    struct Config {
        global: Global,
        channels: Vec<Channel>,
        routers: Vec<Router>,
        metrics: Metrics,
    }
    ```
- **Router 结构升级**（合并自 Design Doc）：
    - 支持 `channels: Vec<TargetChannel>` (带权重)。
    - 支持 `metadata.model_matcher` (模型路由规则)。
    - 引入 **Router Rule Cache** (LRU) 优化路由匹配性能。

### 5.2 性能要求
- 极低的额外延迟 (< 5ms)。
- 高并发处理能力。
- 内存占用可控。

## 6. 验收标准 (Acceptance Criteria)
1.  成功启动网关并响应 OpenAI/Anthropic 格式请求。
2.  CLI 可以正确增删改查 Channel 和 Router，配置实时生效。
3.  配置多个 Channel 时，能按权重分发流量。
4.  当主 Channel 返回 500/429 时，能自动切换到 Fallback Channel。
5.  Prometheus 能抓取到正确的监控指标。
