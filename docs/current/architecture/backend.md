# Apex Gateway - Backend Architecture

**Generated:** 2026-03-10
**Scope:** Rust 后端核心服务架构设计

## 架构概览

Apex Gateway 后端采用分层架构设计，以 Axum 作为 Web 框架，Tokio 作为异步运行时，实现了高性能、可扩展的 AI API 网关。

```
┌─────────────────────────────────────────────────────────────┐
│                      Client Applications                     │
│              (OpenAI SDK / Anthropic SDK / HTTP)             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Axum Router Layer                       │
│  /v1/chat/completions │ /v1/messages │ /api/* │ /dashboard/* │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                    Middleware Chain                          │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐   │
│  │   Auth      │ │ Rate Limit  │ │    Policy Check     │   │
│  │  Middleware │ │  Middleware │ │     Middleware      │   │
│  └─────────────┘ └─────────────┘ └─────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                   Router Selector                            │
│         (Model Matching + Load Balancing Strategy)           │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                   Provider Adapters                          │
│  ┌──────────┐ ┌────────────┐ ┌─────────┐ ┌──────────────┐  │
│  │ OpenAI   │ │ Anthropic  │ │ Gemini  │ │ DeepSeek/... │  │
│  │ Adapter  │ │  Adapter   │ │ Adapter │ │   Adapters   │  │
│  └──────────┘ └────────────┘ └─────────┘ └──────────────┘  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  Cross-Cutting Concerns                      │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────────────┐   │
│  │   Metrics   │ │    Usage    │ │      Compliance     │   │
│  │  (Prometheus│ │  (SQLite)   │ │   (PII Masking)     │   │
│  └─────────────┘ └─────────────┘ └─────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

## 核心模块

### 1. Server 模块 (`src/server.rs`)

**职责**: HTTP 服务器启动、路由注册、中间件组装

**关键结构**:
```rust
pub struct AppState {
    pub config: Arc<Config>,
    pub metrics: Option<Arc<MetricsState>>,
    pub database: Arc<DatabaseState>,
}
```

**主要路由**:
| 路径 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | OpenAI 兼容聊天接口 |
| `/v1/messages` | POST | Anthropic 兼容接口 |
| `/v1/models` | GET | 模型列表 |
| `/api/usage` | GET | Usage 记录 API |
| `/api/metrics` | GET | Metrics 汇总 API |
| `/api/metrics/trends` | GET | 趋势数据 API |
| `/api/metrics/rankings` | GET | 排行榜 API |
| `/dashboard/*` | GET | Web Dashboard 静态文件 |
| `/metrics` | GET | Prometheus 指标 |

### 2. Config 模块 (`src/config.rs`)

**职责**: JSON 配置解析、验证、热重载

**配置结构**:
```rust
pub struct Config {
    pub version: String,
    pub global: Global,
    pub logging: Logging,
    pub data_dir: String,
    pub web_dir: String, // optional override for filesystem-served web assets
    pub channels: Arc<Vec<Channel>>,
    pub routers: Arc<Vec<Router>>,
    pub teams: Arc<Vec<Team>>,
    pub metrics: Metrics,
    pub hot_reload: HotReload,
    pub compliance: Option<Compliance>,
}
```

**热重载机制**:
- 使用 `notify` crate 监听配置文件变化
- 配置变更时自动重新加载
- 非法配置时保持旧配置并记录错误

### 3. Provider 模块 (`src/providers.rs`)

**职责**: LLM 提供商客户端封装、协议转换

**支持的提供商**:
| 提供商 | 协议 | Base URL |
|--------|------|----------|
| OpenAI | OpenAI | https://api.openai.com |
| Anthropic | Anthropic | https://api.anthropic.com |
| Gemini | OpenAI | https://generativelanguage.googleapis.com |
| DeepSeek | Dual | https://api.deepseek.com |
| Moonshot | Dual | https://api.moonshot.cn |
| Minimax | Dual | https://api.minimax.io |
| Ollama | Dual | http://localhost:11434 |
| Jina | OpenAI | https://api.jina.ai |
| OpenRouter | Dual | https://openrouter.ai |

**协议转换**:
- OpenAI → Anthropic: `convert_openai_to_anthropic()`
- Anthropic → OpenAI: `convert_anthropic_to_openai()`

### 4. Router Selector 模块 (`src/router_selector.rs`)

**职责**: 路由匹配、负载均衡、故障转移

**路由匹配流程**:
```
1. 提取请求中的 model 字段
2. 遍历 Router 规则，匹配 model patterns
3. 根据策略 (round_robin/random/priority/weighted) 选择 Channel
4. 如果主 Channel 失败，尝试 Fallback Channel
```

**负载均衡策略**:
| 策略 | 说明 |
|------|------|
| `round_robin` | 轮询，均匀分配流量 |
| `random` | 随机选择 |
| `priority` | 按优先级顺序，失败时降级 |
| `weighted` | 按权重分配 |

### 5. Middleware 模块 (`src/middleware/`)

#### Auth Middleware (`auth.rs`)

**职责**: API Key 验证

**验证流程**:
```rust
1. 从 Header (Authorization / X-API-Key) 或 Query Param 提取 Key
2. 检查 Global Keys (如果配置)
3. 检查 Team Keys
4. 验证 Team Policy (allowed_routers, allowed_models)
5. 将 Team 信息注入请求上下文
```

#### Rate Limit Middleware (`ratelimit.rs`)

**职责**: RPM/TPM 限制

**实现**:
- 使用滑动窗口算法
- 支持 per-team 限流
- 超限返回 429 Too Many Requests

#### Policy Middleware (`policy.rs`)

**职责**: Team Policy 检查

**检查项**:
- 允许的 Routers
- 允许的 Models (支持通配符)
- 速率限制 (RPM/TPM)

### 6. Database 模块 (`src/database.rs`)

**职责**: SQLite 数据库操作

**表结构**:

```sql
-- Usage 记录表
CREATE TABLE usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    team_id TEXT NOT NULL,
    router TEXT NOT NULL,
    channel TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0
);

-- 请求指标表
CREATE TABLE metrics_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    route TEXT NOT NULL,
    router TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 1
);

-- 错误指标表
CREATE TABLE metrics_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    route TEXT NOT NULL,
    router TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 1
);

-- Fallback 指标表
CREATE TABLE metrics_fallbacks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    router TEXT NOT NULL,
    channel TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 1
);

-- 延迟指标表
CREATE TABLE metrics_latency (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    route TEXT NOT NULL,
    router TEXT NOT NULL,
    channel TEXT NOT NULL,
    latency_ms REAL NOT NULL
);
```

**核心方法**:
```rust
- fn init_database() -> Result<DatabaseState>
- fn record_usage() -> Result<()>
- fn record_request() -> Result<()>
- fn record_error() -> Result<()>
- fn record_fallback() -> Result<()>
- fn record_latency() -> Result<()>
- fn get_usage_records() -> Result<(Vec<UsageRecord>, i64)>
- fn get_metrics_summary() -> Result<MetricsSummary>
- fn get_metrics_trends() -> Result<Vec<TrendData>>
- fn get_rankings() -> Result<Vec<RankingData>>
```

### 7. Metrics 模块 (`src/metrics.rs`)

**职责**: Prometheus 指标注册和导出

**指标定义**:
```rust
pub struct MetricsState {
    pub requests_total: IntCounter,           // 请求总量
    pub errors_total: IntCounter,             // 错误总量
    pub fallbacks_total: IntCounter,          // Fallback 总量
    pub upstream_latency_ms: Histogram,       // 上游延迟
}
```

**Prometheus 指标**:
- `apex_requests_total` - 请求总量
- `apex_errors_total` - 错误总量
- `apex_fallbacks_total` - Fallback 触发次数
- `apex_upstream_latency_ms` - 上游响应延迟

### 8. Usage 模块 (`src/usage.rs`)

**职责**: Usage 记录采集和查询

**记录字段**:
```rust
pub struct UsageRecord {
    pub id: i64,
    pub timestamp: String,
    pub team_id: String,
    pub router: String,
    pub channel: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}
```

### 9. Compliance 模块 (`src/compliance.rs`)

**职责**: 数据合规性检查

**功能**:
- PII (Personal Identifiable Information) 检测
- 敏感数据脱敏
- 审计日志记录

### 10. Converters 模块 (`src/converters.rs`)

**职责**: 协议转换

**转换函数**:
```rust
- fn openai_to_anthropic() -> AnthropicRequest
- fn anthropic_to_openai() -> OpenAIRequest
- fn normalize_response() -> UnifiedResponse
```

## 数据流

### 请求处理完整流程

```
1. Client 发送请求到 /v1/chat/completions
   ↓
2. Axum Router 匹配路径
   ↓
3. 执行 Middleware 链:
   - Auth: 验证 API Key，提取 Team 信息
   - RateLimit: 检查 RPM/TPM 限制
   - Policy: 检查 Team Policy
   ↓
4. Router Selector:
   - 提取 model 字段
   - 匹配 Router 规则
   - 根据策略选择 Channel
   ↓
5. Provider Adapter:
   - 构建上游请求
   - 发送 HTTP 请求
   - 处理响应
   ↓
6. 后处理:
   - Metrics 记录 (请求、延迟)
   - Usage 记录 (tokens)
   - 如有错误，记录 Error/Fallback
   ↓
7. 返回响应给 Client
```

### 错误处理流程

```
1. 上游返回错误状态码 (429/500/502/503/504)
   ↓
2. 检查 Fallback Channel 是否可用
   ↓
3. 如可用，尝试 Fallback Channel
   ↓
4. 记录 Fallback 事件
   ↓
5. 如所有 Channel 失败，返回错误响应
   ↓
6. 记录 Error 事件
```

## 配置示例

### 完整配置

```json
{
  "version": "1.0",
  "global": {
    "listen": "0.0.0.0:12356",
    "auth": {
      "mode": "api_key",
      "keys": ["sk-global-key"]
    },
    "timeouts": {
      "connect_ms": 2000,
      "request_ms": 30000,
      "response_ms": 30000
    },
    "retries": {
      "max_attempts": 2,
      "backoff_ms": 200,
      "retry_on_status": [429, 500, 502, 503, 504]
    },
    "cors_allowed_origins": []
  },
  "logging": {
    "level": "info",
    "dir": "~/.apex/logs"
  },
  "data_dir": "~/.apex/data",
  "web_dir": "target/web",
  "channels": [
    {
      "name": "openai-main",
      "provider": "openai",
      "base_url": "https://api.openai.com",
      "api_key": "sk-xxx"
    }
  ],
  "routers": [
    {
      "name": "default-router",
      "rules": [
        {
          "match": { "model": "*" },
          "strategy": "round_robin",
          "channels": [{ "name": "openai-main" }]
        }
      ]
    }
  ],
  "teams": [
    {
      "id": "demo-team",
      "api_key": "sk-ap-xxx",
      "policy": {
        "allowed_routers": ["default-router"]
      }
    }
  ],
  "metrics": {
    "enabled": true,
    "path": "/metrics"
  },
  "hot_reload": {
    "config_path": "config.json",
    "watch": true
  }
}
```

## 关键设计决策

### 1. 为什么选择 Axum？

- **Tokio 原生**: 与 Tokio 异步运行时深度集成
- **Tower 生态**: 直接使用 Tower 中间件生态
- **类型安全**: 强大的类型系统和错误处理
- **性能**: 基于 Tokio，性能优异

### 2. 为什么使用 SQLite？

- **轻量**: 无外部依赖，单文件数据库
- **嵌入**: 适合网关场景，无需单独部署
- **功能完整**: 支持 SQL 查询，便于分析

### 3. 为什么支持多协议？

- **用户友好**: 客户端可使用熟悉的 SDK
- **迁移便利**: 便于从 OpenAI/Anthropic 迁移
- **灵活性**: 适应不同客户端需求

## 性能优化

### 1. 连接池

- 使用 `reqwest::Client` 连接池
- 复用 TCP 连接，减少握手延迟

### 2. 异步 IO

- 全链路异步非阻塞
- Tokio 多运行时支持

### 3. 缓存

- 使用 Moka 作为本地缓存
- Router 规则缓存

### 4. 并发控制

- 信号量限制并发请求数
- 防止资源耗尽

## 安全考虑

### 1. API Key 管理

- Key 脱敏显示
- 支持全局 Key 和 Team Key

### 2. 输入验证

- Model 名称白名单
- 请求大小限制

### 3. 审计日志

- 所有请求记录日志
- 支持 Trace ID 追踪

---

_Generated using BMAD Method `document-project` workflow_
