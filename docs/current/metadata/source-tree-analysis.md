# Apex Gateway - Source Tree Analysis

**Generated:** 2026-03-10
**Scope:** Full repository structure with annotated descriptions

## Root Directory

```
apex/
├── Cargo.toml              # Rust 项目配置和依赖定义
├── Cargo.lock              # 依赖锁定文件 (自动更新)
├── config.example.json     # 配置文件示例
├── test-config.json        # 测试用配置文件
├── docker-compose.yml      # Docker 编排配置
├── Dockerfile              # Docker 镜像构建配置
├── install.sh              # 一键安装/部署脚本
├── LICENSE                 # MIT 许可证
├── README.md               # 项目 README (英文)
├── README_zh-CN.md         # 项目 README (中文)
├── CONTRIBUTING.md         # 贡献指南
├── CODE_OF_CONDUCT.md      # 行为准则
├── SECURITY.md             # 安全政策
└── CLAUDE.md               # AI 助手项目指南
```

## 核心目录

### `/src` - Rust 源代码

```
src/
├── main.rs                 # CLI 入口和命令处理
├── lib.rs                  # 库入口
├── server.rs               # HTTP 服务器和路由处理
├── config.rs               # 配置解析和验证
├── providers.rs            # LLM 提供商客户端封装
├── router_selector.rs      # 路由选择和负载均衡
├── database.rs             # SQLite 数据库操作
├── usage.rs                # Usage 记录和查询
├── metrics.rs              # Prometheus 指标注册和导出
├── converters.rs           # 协议转换 (OpenAI ↔ Anthropic)
├── compliance.rs           # 数据合规性检查
├── logs.rs                 # 日志配置和高亮
├── utils.rs                # 通用工具函数
└── middleware/             # HTTP 中间件
    ├── mod.rs              # 模块入口
    ├── auth.rs             # API Key 认证中间件
    ├── ratelimit.rs        # 速率限制中间件
    └── policy.rs           # Team Policy 检查
```

### `/tests` - Rust 集成测试

```
tests/
├── system.rs               # 系统级集成测试
├── gateway.rs              # 网关功能测试
├── cli.rs                  # CLI 命令测试
├── team_test.rs            # Team 管理测试
├── e04_observability_test.rs  # 可观测性测试
├── e05_routing_test.rs     # 路由功能测试
├── benchmark_test.rs       # 性能基准测试
├── hot_reload_test.rs      # 热重载测试
└── common/                 # 测试工具
```

### `/web` - Next.js 前端

```
web/
├── package.json            # Node.js 依赖配置
├── next.config.ts          # Next.js 配置
├── tsconfig.json           # TypeScript 配置
├── tailwind.config.ts      # Tailwind CSS 配置
├── playwright.config.ts    # Playwright 测试配置
├── components.json         # shadcn/ui 配置
├── eslint.config.mjs       # ESLint 配置
├── src/                    # 源代码
│   ├── app/                # App Router 页面
│   │   ├── layout.tsx      # 根布局
│   │   ├── page.tsx        # 首页
│   │   └── dashboard/      # Dashboard 页面
│   └── components/         # React 组件
│       ├── ui/             # shadcn/ui 基础组件
│       └── ...             # 业务组件
├── dashboard/              # 构建后的 dashboard 页面
├── public/                 # 静态资源
├── tests/                  # Playwright E2E 测试
└── target/web/             # 生产构建输出 (由 backend 服务)
```

### `/docs` - 项目文档

```
docs/
├── index.md                # 文档索引 (新生成)
├── current/                # 当前项目事实文档
│   ├── overview.md
│   ├── architecture/
│   ├── reference/
│   ├── guides/
│   └── metadata/
_bmad-output/
├── planning/               # BMAD 规划产物
│   ├── epics.md
│   ├── epics/
│   ├── ux-design-specification.md
│   └── research/
├── implementation/         # BMAD 实施产物
│   ├── sprint-status.yaml
│   ├── stories/
│   │   ├── 7-1-pii-masking-engine.md
│   │   ├── 8-1-mcp-protocol-transport.md
│   │   ├── 8-2-session-lifecycle.md
│   │   ├── 8-3-mcp-resources.md
│   │   ├── 8-4-resource-listing-key-masking.md
│   │   ├── 8-5-analytics-reporting.md
│   │   ├── 8-6-mcp-prompts.md
│   │   └── 8-7-mcp-tools.md
│   ├── tech-specs/
│   ├── tests/
│   └── retrospectives/
└── test-artifacts/         # 历史测试设计产物
    └── test-design-architecture.md
```

### `/.github` - GitHub 配置

```
.github/
├── ISSUE_TEMPLATE/         # Issue 模板
└── workflows/              # GitHub Actions 工作流
    ├── ci.yml              # CI 构建和测试
    ├── frontend-tests.yml  # 前端测试
    └── build-release.yml   # 多平台构建发布
```

### `/_bmad` - BMAD Method 配置

```
_bmad/
├── bmm/                    # BMAD 方法配置
│   ├── config.yaml         # 项目配置
│   └── workflows/          # 工作流定义
│       └── document-project/ # 文档生成工作流
├── core/                   # 核心任务和工作流
└── tea/                    # TEA (Test Engineering Architecture)
```

### `/.claude` - Claude 配置

```
.claude/
├── commands/               # 自定义命令
│   ├── bmad-*.md           # BMAD 方法命令
│   └── ...
└── skills/                 # 技能定义
```

### `/target` - 构建产物

```
target/
├── debug/                  # Debug 构建
├── release/                # Release 构建
├── flycheck0/              # Flycheck 增量检查
├── tmp/                    # 临时文件
└── web/                    # Web 前端构建输出
```

## 关键文件详解

### `src/main.rs` (约 800 行)

**职责:**
- CLI 命令解析 (使用 Clap)
- 命令分发 (init, channel, router, gateway, team, status, logs)
- 守护进程模式支持
- 日志系统初始化

**主要结构:**
```rust
- struct Cli              # CLI 参数定义
- enum Commands           # 命令枚举
- fn main()               # 入口函数
- fn async_main()         # 异步主逻辑
- fn handle_*_command()   # 各命令处理器
```

### `src/server.rs` (约 1400 行)

**职责:**
- HTTP 服务器启动和配置
- 路由注册 (OpenAI, Anthropic, Dashboard)
- 中间件链组装
- API 端点实现

**主要路由:**
```
/v1/chat/completions  → OpenAI 兼容接口
/v1/messages          → Anthropic 兼容接口
/v1/models            → 模型列表
/api/usage            → Usage API
/api/metrics          → Metrics API
/dashboard/*          → Web Dashboard 静态文件
/metrics              → Prometheus 指标
```

### `src/providers.rs` (约 900 行)

**职责:**
- LLM 提供商客户端封装
- 请求/响应协议转换
- 错误处理和重试

**支持的提供商:**
- OpenAI, Anthropic, Gemini
- DeepSeek, Moonshot, Minimax
- Ollama, Jina, OpenRouter

### `src/config.rs` (约 400 行)

**职责:**
- JSON 配置文件解析
- 热重载支持
- 配置验证

**核心结构:**
```rust
- struct Config         # 顶层配置
- struct Global         # 全局设置
- struct Channel        # 上游通道
- struct Router         # 路由规则
- struct Team           # 团队配置
- struct Metrics        # 指标配置
- struct HotReload      # 热重载配置
```

## 数据流分析

### 请求处理流程

```
Client Request
    ↓
[Axum Router]
    ↓
[Auth Middleware] → API Key 验证
    ↓
[Rate Limit Middleware] → RPM/TPM 检查
    ↓
[Policy Middleware] → Team Policy 验证
    ↓
[Router Selector] → 匹配路由规则
    ↓
[Provider Adapter] → 协议转换
    ↓
[Upstream LLM API]
    ↓
[Response Converter] → 统一响应格式
    ↓
[Metrics Logger] → 指标采集
    ↓
[Usage Logger] → SQLite 持久化
    ↓
Client Response
```

### 说明

该分析文档原先包含 HTTP MCP 模块与专项测试清单。当前产品方向已移除该表面能力，自动化入口以 CLI 和后续 Admin Control Plane 为准。

## 代码规模统计

| 模块 | 文件数 | 代码行数 (约) |
|------|--------|---------------|
| src/ | 15 | 5,500 |
| tests/ | 12 | 2,000 |
| web/src/ | ~20 | 3,000 |
| docs/ | 30+ | 10,000+ |

---

_Generated using BMAD Method `document-project` workflow_
