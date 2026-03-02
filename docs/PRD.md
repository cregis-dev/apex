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
- **扩展能力**：MCP server 操作管理与统计分析（阶段性引入）
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

### 3.3 运维与分析人员 (Ops & Analytics)
- 关注网关与 MCP server 的运行健康度、可用性与故障处置。
- 通过统计指标与报表评估调用成本、失败率与性能瓶颈。
- 基于审计与操作日志回溯变更影响。

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

### 4.5 MCP Server 操作管理与统计分析
#### 4.5.1 协议与传输
- **协议层**：遵循 MCP/JSON-RPC 2.0 规范。
- **传输层**：支持 stdio 与 HTTP/SSE 远程模式。
- **生命周期**：支持初始化、能力协商、会话管理与通知更新。

#### 4.5.2 管理与控制面
- **配置管理**：支持 MCP server 的启停、参数调整与热更新策略。
- **权限控制**：支持基于 OAuth 2.0 或 API Key 的访问控制。
- **审计能力**：记录管理操作与关键变更事件。
- **资源清单**：提供 team/router/channel 的 list 能力，并对 key 做脱敏展示。

#### 4.5.3 统计分析与报表
- **指标体系**：请求量、错误率、P95/P99 延迟、模型与通道分布、调用成本。
- **报表能力**：支持按时间、租户、模型、路由策略维度查询与导出。

#### 4.5.4 集成方式
- **网关入口**：通过 API Gateway 统一接入，集中鉴权、限流与审计。
- **读写分离**：统计查询与写入分离，提升报表查询性能。

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

### 5.3 可观测性与分析要求
- 支持指标聚合与统计查询的可扩展存储方案。
- 支持基础运维指标与业务指标的分层看板。

## 6. 验收标准 (Acceptance Criteria)
1.  成功启动网关并响应 OpenAI/Anthropic 格式请求。
2.  CLI 可以正确增删改查 Channel 和 Router，配置实时生效。
3.  配置多个 Channel 时，能按权重分发流量。
4.  当主 Channel 返回 500/429 时，能自动切换到 Fallback Channel。
5.  Prometheus 能抓取到正确的监控指标。
6.  MCP server 可按协议完成初始化与能力协商，并可稳定提供远程通信。
7.  管理面能够完成 MCP server 启停、配置变更与审计记录。
8.  统计面可输出关键指标与报表查询，满足基础运营分析需求。

## 7. 用户故事与流程 (User Stories & Flows)
### 7.1 平台管理员
- 作为平台管理员，我希望通过 CLI 初始化与更新配置，确保网关与 MCP server 能稳定运行。
- 作为平台管理员，我希望能够快速定位某个路由或通道的配置问题，并回滚到上一个可用版本。

### 7.2 业务开发者
- 作为业务开发者，我希望通过统一接口调用模型，不关心底层 Provider 的差异。
- 作为业务开发者，我希望在调用失败时能得到稳定的错误响应与可追踪的请求标识。

### 7.3 运维与分析人员
- 作为运维人员，我希望能看到 MCP server 的健康度与告警，快速定位故障来源。
- 作为分析人员，我希望查看按模型与路由维度的调用成本与成功率趋势。

## 8. 非功能指标 (Non-Functional Metrics)
### 8.1 性能与可用性
- 网关额外延迟目标：P95 < 5ms，P99 < 15ms。
- 关键接口可用性目标：月度可用性 99.9%。

### 8.2 稳定性与可靠性
- 失败请求需包含可追踪请求标识，便于定位问题。
- 关键配置变更必须记录审计日志并可回溯。

### 8.3 可扩展性
- 支持横向扩展与多实例部署。
- 支持统计查询的独立扩缩与读写分离。

## 9. 里程碑计划 (Milestones)
- **M1 基础能力**：MCP server 协议接入、基础管理与审计能力、核心指标采集。
- **M2 扩展能力**：统计报表与告警能力、读写分离与基础聚合查询。
- **M3 优化能力**：高级分析能力、成本治理与资源优化策略。

## 10. 依赖与约束 (Dependencies & Constraints)
- 依赖现有 Apex 网关的配置与鉴权体系。
- 远程传输需满足内网安全策略与 TLS 加密要求。

## 11. 风险与对策 (Risks & Mitigations)
- **协议兼容风险**：严格对齐 MCP/JSON-RPC 规范并通过集成测试保障兼容性。
- **运维复杂度增加**：引入统一监控与告警策略，预置运行手册与回滚机制。
- **数据一致性风险**：统计与写入分离时明确最终一致性口径与报表时效。

## 12. 详细功能拆分 (Detailed Requirements)
### 12.1 MCP Server 生命周期管理
- 初始化：支持版本协商与能力声明。
- 运行：支持会话保持与能力变更通知。
- 关闭：支持安全终止与资源释放。

### 12.2 管理面操作集
- 启停控制：支持单实例与集群级启停。
- 配置变更：支持灰度发布与回滚。
- 审计记录：记录操作人、时间、变更内容与结果状态。
- 资源列表：支持 team/router/channel 查询与列表展示，key 脱敏处理。

### 12.3 统计分析能力
- 实时指标：请求量、错误率、延迟分位数、模型与通道分布。
- 成本指标：按模型、路由与租户维度的调用成本与预算消耗。
- 报表导出：支持 CSV/JSON 导出与定时生成。

## 13. 数据口径与指标定义 (Metrics Definitions)
- **请求量**：统计周期内进入网关的有效请求总数。
- **错误率**：错误请求数 / 总请求数。
- **延迟分位**：P95/P99 基于完整请求链路耗时统计。
- **成功率**：成功响应数 / 总请求数。
- **成本**：单位时间内各模型调用的计费总量与均值。

## 14. 权限与安全模型 (Security Model)
- **访问层级**：平台管理员、运维人员、分析人员、业务开发者。
- **权限粒度**：按租户、路由、模型维度进行授权控制。
- **凭证策略**：支持 OAuth 2.0 与 API Key 并行，支持轮换与失效。

## 15. 运维流程与变更管理 (Operations)
- **变更流程**：变更申请 → 评审 → 灰度发布 → 验证 → 全量推广。
- **告警流程**：异常告警 → 关联链路 → 故障定位 → 复盘结论。
- **回滚策略**：配置回滚优先，版本回滚为兜底。

## 16. 验收细则 (Acceptance Details)
- 管理与统计接口在模拟高负载下无明显性能退化。
- 关键指标与报表与审计日志可形成闭环追溯。
- MCP server 会话与能力变更在版本升级后保持向后兼容。
