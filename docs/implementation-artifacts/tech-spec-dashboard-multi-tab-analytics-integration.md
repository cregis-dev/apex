---
title: 'Dashboard Multi-Tab Analytics Integration'
slug: 'dashboard-multi-tab-analytics-integration'
created: '2026-03-12T21:51:26+0800'
status: 'ready-for-dev'
stepsCompleted: [1, 2, 3, 4]
tech_stack:
  - 'Next.js 16 App Router static export'
  - 'React 19'
  - 'Tailwind CSS 4'
  - 'Recharts 2.x'
  - 'Base UI Select primitives'
  - 'Rust Axum dashboard asset serving'
  - 'SQLite analytics queries via rusqlite'
  - 'rust-embed embedded-web release packaging'
files_to_modify:
  - '/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx'
  - '/Users/shawn/workspace/code/apex/web/src/components/dashboard/'
  - '/Users/shawn/workspace/code/apex/web/src/app/globals.css'
  - '/Users/shawn/workspace/code/apex/web/src/components/ui/tabs.tsx'
  - '/Users/shawn/workspace/code/apex/web/src/components/ui/'
  - '/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts'
  - '/Users/shawn/workspace/code/apex/src/server.rs'
  - '/Users/shawn/workspace/code/apex/src/database.rs'
  - '/Users/shawn/workspace/code/apex/src/mcp/analytics.rs'
code_patterns:
  - 'Current dashboard is a single client component that owns auth bootstrap, URL state sync, API fetches, chart rendering, CSV export, and records drilldown.'
  - 'Dashboard is served as static-exported Next.js assets under /dashboard/ through Rust Axum routes and target/web output.'
  - 'Dashboard API calls use global auth via Authorization Bearer token and support token URL bootstrap.'
  - 'Recharts chart styling is centralized through ChartContainer and ChartTooltipContent helpers in ui/chart.tsx.'
  - 'Existing data filters are URL-backed and refresh the page model incrementally instead of navigating to separate routes.'
  - 'Database analytics currently mix direct SQL endpoints for dashboard with record-level aggregation helpers reused by MCP analytics.'
test_patterns:
  - 'Playwright dashboard tests validate token bootstrap, URL normalization, fetch mocking, refresh behavior, and export flows.'
  - 'Rust database tests assert usage ordering and query semantics near the database layer.'
---

# Tech-Spec: Dashboard Multi-Tab Analytics Integration

**Created:** 2026-03-12T21:51:26+0800

## Overview

### Problem Statement

当前 Apex dashboard 已经具备认证、基础 KPI、趋势图、筛选、导出和 usage records 表格，但页面仍以单屏监控总览为主，信息架构与用户提供的新 dashboard 原型存在明显差距。目标态要求引入侧边栏导航、全局筛选器、五个分析标签页、更多图表类型，以及以真实网关 API 数据驱动的团队、渠道、模型和拓扑分析；而用户给出的参考实现是一个 Vite/React 原型，使用的是 mock data，不能直接并入当前 `web/` 子应用或直接接入当前 Rust 静态导出链路。

### Solution

在现有 `web/` Next.js dashboard 中重构页面信息架构，将 Vite 原型的视觉层级、交互模式和图表模块迁移为 Apex 当前技术栈可维护的组件结构，并保留现有认证、URL 状态同步、静态导出和 Rust `/dashboard/` 挂载方式。实现过程中允许新增或扩展后端聚合接口，以便为团队排行、模型份额、渠道延迟和 Sankey 拓扑等模块提供真实数据，而不是将 mock data 直接搬入生产代码。

### Scope

**In Scope:**
- 将当前 dashboard 从单体监控页重构为包含 Sidebar、Global Filters、Tabs 主内容区的多模块工作台。
- 将用户提供原型中的五个 tabs 落到当前 `web/` 项目：`Overview`、`Team & Usage`、`System & Reliability`、`Model & Router`、`Records`。
- 保留并整合现有认证机制、`token` URL 导入、本地 token 存储、刷新、导出、分页和记录明细能力。
- 将现有与新增图表统一到 Recharts 下实现，包括双 Y 轴趋势图、水平/堆叠柱状图、面积图、饼图，以及可行的 Sankey 拓扑展示。
- 明确当前后端 API 与目标 UI 的数据差距，并在需要时扩展 Rust 聚合接口或响应字段以支撑真实图表。
- 保持 Next.js static export 输出到 `target/web`，继续由 Rust `/dashboard/` 路由提供静态资源。

**Out of Scope:**
- 实现 Sidebar 中 `API Keys`、`Routing Rules`、`Team Management`、`Settings` 等新页面。
- 将外部 Vite 工程整体迁入 monorepo 或保留其构建系统。
- 接入新的设计系统、状态管理框架或图表库替换当前 Next.js/Tailwind/Recharts 方案。
- 修改网关核心鉴权模型或将 dashboard 从全局 auth 改成 team auth。
- 实现与需求无关的深层管理工作流、复杂自定义布局、个性化保存等扩展能力。

## Context for Development

### Codebase Patterns

- 当前 dashboard 主实现集中在 [`/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx`](/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx)，由单个客户端组件维护时间范围、筛选、URL query、导出、表格、认证和图表数据。
- Dashboard 页面入口极薄，只在 [`/Users/shawn/workspace/code/apex/web/src/app/dashboard/page.tsx`](/Users/shawn/workspace/code/apex/web/src/app/dashboard/page.tsx) 中渲染 `DashboardClient`，因此重构优先方向应是拆分 `dashboard-client.tsx` 为更小的业务组件，而不是改 `page.tsx` 路由结构。
- 前端默认通过 `NEXT_PUBLIC_API_URL` 或 `window.location.origin` 访问 API，并用 `Authorization: Bearer <token>` 调用受 `global_auth` 保护的 `/api/metrics`、`/api/metrics/trends`、`/api/usage` 等接口。
- Rust 后端已固定 `/dashboard` 重定向和 `/dashboard/` 静态资源提供规则，且 Next.js 配置启用了 `output: "export"` 和 `trailingSlash: true`，因此任何新布局都必须兼容静态导出路径。
- 外部参考 dashboard 位于 `/Users/shawn/Downloads/apex-ai-gateway-dashboard/src/App.tsx`，其价值主要是信息架构、图表类型和交互密度，不是代码级直接复用目标。
- 当前 UI 基础组件包含 `button`、`card`、`input`、`select`、`table`、`chart`，但没有现成的 `tabs`、`badge`、`sidebar` 业务壳层，因此原型迁移会涉及新增 UI primitives 或业务组件。
- 当前 `dashboard-client.tsx` 已经有 records drawer、drilldown、CSV export、auto refresh、loading skeleton 和 inline error banner，这些状态管理不应重写成另一套并行逻辑，而应提升为共享状态层供多 tabs 复用。
- `src/database.rs` 已有 `get_usage_records_for_analytics` 和 `UsageRecordQuery`，而 [`/Users/shawn/workspace/code/apex/src/mcp/analytics.rs`](/Users/shawn/workspace/code/apex/src/mcp/analytics.rs) 已经基于它做记录级聚合，这为 dashboard 新增团队、渠道、模型、拓扑类统计提供了复用锚点。
- 发布链路已经满足“可打包进二进制”的要求：前端继续导出到 `target/web`，Rust 通过 `embedded-web` feature 从 [`/Users/shawn/workspace/code/apex/src/web_assets.rs`](/Users/shawn/workspace/code/apex/src/web_assets.rs) 嵌入并提供静态资源，因此本次允许重构前端架构，但不能破坏 `target/web` 输出和静态 export 约束。

### Files to Reference

| File | Purpose |
| ---- | ------- |
| [/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx](/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx) | 当前 dashboard 核心实现，包含认证、筛选、图表、records 表格、导出和 URL 状态 |
| [/Users/shawn/workspace/code/apex/web/src/app/dashboard/page.tsx](/Users/shawn/workspace/code/apex/web/src/app/dashboard/page.tsx) | Dashboard 页面入口 |
| [/Users/shawn/workspace/code/apex/web/src/app/globals.css](/Users/shawn/workspace/code/apex/web/src/app/globals.css) | 全局主题变量与基础样式，需要承载新的 Slate/Emerald/Orange 视觉方向 |
| [/Users/shawn/workspace/code/apex/web/src/components/ui/select.tsx](/Users/shawn/workspace/code/apex/web/src/components/ui/select.tsx) | 当前可直接复用的筛选器 primitive |
| [/Users/shawn/workspace/code/apex/web/src/components/ui/chart.tsx](/Users/shawn/workspace/code/apex/web/src/components/ui/chart.tsx) | 统一图表容器、tooltip 和图例样式能力 |
| [/Users/shawn/workspace/code/apex/web/package.json](/Users/shawn/workspace/code/apex/web/package.json) | 确认 Next.js、Recharts、Playwright 与构建输出脚本 |
| [/Users/shawn/workspace/code/apex/web/next.config.ts](/Users/shawn/workspace/code/apex/web/next.config.ts) | 静态导出与 trailing slash 约束 |
| [/Users/shawn/workspace/code/apex/src/server.rs](/Users/shawn/workspace/code/apex/src/server.rs) | Dashboard 静态资源挂载与 API 路由整合点 |
| [/Users/shawn/workspace/code/apex/src/middleware/auth.rs](/Users/shawn/workspace/code/apex/src/middleware/auth.rs) | Dashboard 使用的全局鉴权模型 |
| [/Users/shawn/workspace/code/apex/src/database.rs](/Users/shawn/workspace/code/apex/src/database.rs) | 现有 usage、metrics、trends、rankings SQL 以及 analytics 查询锚点 |
| [/Users/shawn/workspace/code/apex/src/mcp/analytics.rs](/Users/shawn/workspace/code/apex/src/mcp/analytics.rs) | 已存在的记录聚合逻辑，可作为 dashboard analytics API 的复用来源 |
| [/Users/shawn/workspace/code/apex/src/web_assets.rs](/Users/shawn/workspace/code/apex/src/web_assets.rs) | `embedded-web` 和文件系统双模式静态资源加载 |
| [/Users/shawn/workspace/code/apex/Cargo.toml](/Users/shawn/workspace/code/apex/Cargo.toml) | `embedded-web` feature 定义 |
| [/Users/shawn/workspace/code/apex/docs/current-release-model.md](/Users/shawn/workspace/code/apex/docs/current-release-model.md) | 当前发布模型：先构建 `target/web`，再用 `--features embedded-web` 打包 |
| [/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | 当前 dashboard E2E 回归覆盖模式 |
| [/Users/shawn/workspace/code/apex/docs/planning-artifacts/dashboard-page-structure-spec-2026-03-11.md](/Users/shawn/workspace/code/apex/docs/planning-artifacts/dashboard-page-structure-spec-2026-03-11.md) | 现有 dashboard 结构建议，便于识别与新需求冲突点 |
| [/Users/shawn/Downloads/apex-ai-gateway-dashboard/src/App.tsx](/Users/shawn/Downloads/apex-ai-gateway-dashboard/src/App.tsx) | 外部原型的 tabs、KPI、Sankey、records refresh/copy 参考 |

### Technical Decisions

- 以当前 monorepo 的 `web/` Next.js 应用为唯一前端实现载体，不引入第二套 Vite runtime。
- 以“迁移原型能力到当前工程”而非“照搬原型代码”为实现原则，优先复用现有 auth、records、export、URL sync 逻辑。
- 页面结构将从单一 `DashboardClient` 逐步拆分为壳层、控制条、tab sections、图表卡片、records table 等业务组件，以降低单文件复杂度。
- 图表仍使用 Recharts；若 Sankey 在当前版本可用，则直接实现，否则以兼容当前版本的方式封装或降级处理，但对外规格仍保持流量拓扑能力。
- 全局筛选器保留 URL 可分享能力，并把新的 `1h/24h/7d/30d` 时间范围映射到现有日期范围计算逻辑；这意味着现有 `today/week/month/custom` 模型需要重命名或适配。
- `1h` 不再复用纯日期粒度查询，必须使用时间戳级过滤参数，例如 `start_time/end_time`，而 `24h/7d/30d` 可继续使用日期或自动聚合粒度。
- 新 UI 以真实 API 数据为主，外部原型中的 mock 字段仅用作数据结构设计参考，不进入最终业务逻辑。
- 若现有 `/api/metrics`、`/api/metrics/trends`、`/api/metrics/rankings` 无法支撑 tabs 所需指标，允许新增或扩展聚合 API，但接口必须继续受 `global_auth` 保护。
- 前端认证可以简化，但后端鉴权语义不改。优先方向是保留现有 `token` URL bootstrap 和本地缓存能力，将独立登录页弱化为轻量入口或内联连接态，而不是引入 cookie/session 登录系统。
- 无论 URL 中的 `token` 最终是否有效，前端在初始化读取后都必须立即从地址栏移除该参数，避免泄露到历史记录、分享链接或截图。
- 为保障后续二进制打包，本次前端架构调整仅限 `web/` 源码组织、组件拆分和 API 调用层；不得引入依赖运行时服务端渲染、动态路由重写或非静态导出兼容能力。
- `rankings` 现有接口只支持不带筛选的简单 group by，无法直接满足“团队模型分布”“渠道延迟对比”“Sankey topology”这类组合分析；这些模块预计需要新增 dashboard analytics API 或复用 `mcp/analytics.rs` 聚合模式下沉到 HTTP 层。
- Dashboard 展示型统计默认以 `usage_records` 为主聚合源，避免与 `metrics_*` 表产生口径漂移；`metrics_*` 仅保留监控或对照用途，不作为 dashboard 主来源。
- `Route Info` 相关展示必须落到明确字段，至少包括 `matched_rule`、`router`、`final_channel`，否则 Records 与 Topology 不得宣称展示完整链路。
- 若 Sankey 在当前 Recharts 版本下不能稳定实现，降级方案必须是保留 topology 模块并展示结构化 flow summary，而不是直接删除该模块。

## Implementation Plan

### Tasks

- [ ] Task 1: 建立新的 dashboard 页面骨架与组件边界
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx`
  - Action: 从当前单体渲染树中抽离共享状态层，仅保留认证、筛选、URL 状态、数据加载和 tab 选择等顶层状态。
  - Notes: 这一步不直接重写全部 UI，而是先把 `Sidebar`、`GlobalFilters`、`TabPanels`、`RecordsSection`、`UsageDetailsDrawer` 的挂载点定义清楚，避免后续边改边拆。

- [ ] Task 2: 补齐 dashboard 所需的 UI primitives 和业务容器
  - File: `/Users/shawn/workspace/code/apex/web/src/components/ui/tabs.tsx`
  - Action: 新增可复用 tabs primitive，支持 `TabsList`、`TabsTrigger`、`TabsContent`。
  - Notes: 当前 `web/src/components/ui` 没有 tabs，不能直接照搬外部 Vite 原型。

- [ ] Task 3: 新增 dashboard 业务组件目录结构
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/`
  - Action: 新增并拆分业务组件，例如 `dashboard-shell.tsx`、`dashboard-sidebar.tsx`、`dashboard-filters.tsx`、`overview-tab.tsx`、`team-usage-tab.tsx`、`system-reliability-tab.tsx`、`model-router-tab.tsx`、`records-tab.tsx`、`usage-details-drawer.tsx`。
  - Notes: `records` 和 `drawer` 逻辑应从现有实现迁移，而不是重造一份不兼容状态的新版本。

- [ ] Task 4: 将时间范围和筛选模型适配为新需求
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx`
  - Action: 把现有 `today/week/month/custom` 状态模型调整为 `1h/24h/7d/30d` 与可选自定义日期映射，并统一作用于所有 tabs 数据请求。
  - Notes: 需要继续保留 URL query 恢复能力；若保留自定义日期，应将其作为扩展态而不是主筛选项。

- [ ] Task 5: 简化前端认证入口但保持后端全局鉴权
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx`
  - Action: 将现有独立 auth gate 简化为更轻量的连接入口或内联连接态，继续支持 `token` query 导入与 localStorage 恢复。
  - Notes: 不引入 cookie/session；当存在有效 token 时应直接进入 dashboard，失败时才暴露连接入口；无论 token 是否有效，都要在初始化后从 URL 中立即清除。

- [ ] Task 6: 先定义 dashboard analytics API contract 与共享字段
  - File: `/Users/shawn/workspace/code/apex/src/server.rs`
  - Action: 明确新增 analytics API 的端点、查询参数、响应结构和字段命名，并同步到前端 TypeScript 类型。
  - Notes: 至少覆盖 `overview`、`team_usage`、`system_reliability`、`model_router`、`topology`、`records_meta/filter_options`；必须写清 `1h` 的时间戳级参数方案。

- [ ] Task 7: 扩展后端 dashboard analytics API
  - File: `/Users/shawn/workspace/code/apex/src/server.rs`
  - Action: 新增或扩展 HTTP 接口，用于返回 tabs 需要的聚合数据，例如 overview 总览、team-model 分布、channel latency、model share、topology/sankey 数据。
  - Notes: 接口必须继续挂在当前 dashboard API 路由组下，并保持 `global_auth` 保护。

- [ ] Task 8: 复用数据库 analytics 聚合能力并补足缺口
  - File: `/Users/shawn/workspace/code/apex/src/database.rs`
  - Action: 为 dashboard 需要的组合分析增加查询函数，优先复用 `UsageRecordQuery` 和 `get_usage_records_for_analytics` 过滤模型。
  - Notes: 现有 `get_rankings` 不支持筛选和多维聚合，不能直接满足需求；需要引入基于 records 的 group-by 聚合或针对性 SQL；避免“全量拉出后内存聚合”成为默认实现。

- [ ] Task 9: 评估并抽取 MCP analytics 的复用逻辑
  - File: `/Users/shawn/workspace/code/apex/src/mcp/analytics.rs`
  - Action: 识别 `aggregate_records`、按 team/channel/model/router 聚合的通用逻辑，决定是直接复用、抽公共模块，还是复制必要算法到 dashboard 层。
  - Notes: 目标是减少 dashboard 与 MCP analytics 之间的统计口径漂移，并落实“dashboard 以 `usage_records` 为主口径”。

- [ ] Task 10: 在 Overview tab 接入 KPI、双轴趋势图与流量拓扑图
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/overview-tab.tsx`
  - Action: 使用真实 API 数据渲染 `Total Requests`、`Total Tokens`、`Avg Latency`、`Success Rate`、全局趋势图和 Sankey 拓扑图。
  - Notes: `Success Rate` 可由 `100 - error_rate` 推导；同时实现用户要求的环比变化；若 Recharts 当前版本对 Sankey 支持受限，需要落地为约定好的 flow summary 降级视图，而不是删除 topology。

- [ ] Task 11: 在 Team/System/Model tabs 接入剩余分析图表
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/team-usage-tab.tsx`
  - Action: 实现 Team Leaderboard 与 Model Usage by Team。
  - Notes: 团队排行按 tokens 排序，模型分布采用堆叠柱状图，必须受全局 filters 影响。
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/system-reliability-tab.tsx`
  - Action: 实现 Error Rate Trend 与 Channel Latency Comparison。
  - Notes: 错误率曲线与 Overview 指标口径一致；渠道延迟需要展示均值，必要时预留 p95 tooltip。
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/model-router-tab.tsx`
  - Action: 实现 Model Share 饼图与必要的 router/channel 补充摘要。
  - Notes: 悬浮态要显示真实值与占比，不只是颜色块。

- [ ] Task 12: 重构 Records tab 并保留现有可操作能力
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/records-tab.tsx`
  - Action: 将现有 usage table、分页、刷新、drawer、状态色、高延迟标红、CSV 导出、request ID 一键复制迁移为独立 records 模块。
  - Notes: 必须明确定义刷新语义：在第一页时追加新记录到顶部；非第一页时显示 “N new records” 横幅，用户确认后跳回第一页；避免在前端伪造 mock 记录。

- [ ] Task 13: 更新主题与视觉层
  - File: `/Users/shawn/workspace/code/apex/web/src/app/globals.css`
  - Action: 调整全局变量与 dashboard 局部样式，使其符合 Slate/Emerald/Orange 的视觉方向，并兼容现有组件变量体系。
  - Notes: 不要求重建完整设计系统，但要避免当前实现与新 dashboard 视觉语言冲突。

- [ ] Task 14: 扩展前端回归测试
  - File: `/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts`
  - Action: 为 tabs 切换、全局筛选、简化认证入口、request ID copy、records refresh、图表渲染和新 analytics API mock 增加 Playwright 用例。
  - Notes: 现有测试已经覆盖 token URL bootstrap、刷新、导出和 drawer，可在此基础上演进而不是推翻；要新增“有效 token 初始化后也会从 URL 清除”的断言。

- [ ] Task 15: 补充后端查询与接口测试，并验证打包链路
  - File: `/Users/shawn/workspace/code/apex/src/database.rs`
  - Action: 为新增 analytics 查询补单测，覆盖筛选条件、空数据、百分比与排序稳定性。
  - Notes: 至少要验证 team/model/channel 聚合、latency 聚合口径、`matched_rule/final_channel` 字段和 `1h` 时间窗口行为。
  - File: `/Users/shawn/workspace/code/apex/src/server.rs`
  - Action: 为新增 dashboard API handler 补路由级测试或请求级断言。
  - Notes: 完成后需通过 `web` 构建到 `target/web`，并保持 `embedded-web` 可继续作为发布模式。

### Acceptance Criteria

- [ ] AC 1: Given 用户访问 `/dashboard/` 且浏览器中已有有效全局 API key 或 URL 带有 `token`, when 页面初始化完成, then dashboard 直接进入主工作台并展示 Sidebar、Global Filters 与默认 `Overview` tab。
- [ ] AC 2: Given 用户没有可用 token, when 打开 `/dashboard/`, then 页面展示简化后的连接入口而不是独立管理页，并允许输入全局 API key 进入 dashboard。
- [ ] AC 3: Given 用户提供了无效 token, when dashboard 首次验证 `/api/metrics` 或新的 analytics API, then 前端清理失效 token、移除 URL 中的 `token`，并回退到连接入口显示错误信息。
- [ ] AC 4: Given 用户通过 URL 提供了有效 token, when dashboard 初始化完成, then 连接状态保留，但 URL 中的 `token` 也会被立即移除。
- [ ] AC 5: Given 用户切换时间范围为 `1h`、`24h`、`7d` 或 `30d`, when 选择生效, then 所有 KPI、图表和 records 使用一致的数据窗口刷新，且 URL query 能恢复当前视图。
- [ ] AC 6: Given 用户选择 `1h`, when dashboard 生成查询条件, then 前后端使用时间戳级窗口而不是仅按日期过滤。
- [ ] AC 7: Given 用户切换 Team 或 Model 筛选器, when 筛选生效, then `Overview`、`Team & Usage`、`System & Reliability`、`Model & Router`、`Records` 五个 tabs 展示的数据全部基于同一筛选条件。
- [ ] AC 8: Given 用户进入 `Overview` tab, when 数据加载成功, then 页面展示 `Total Requests`、`Total Tokens`、`Avg Latency`、`Success Rate` 四个 KPI、环比变化、请求量/Token 双轴趋势图和流量拓扑图，且数据来源于真实接口。
- [ ] AC 9: Given 用户进入 `Team & Usage` tab, when analytics 数据返回, then 页面展示按 Token 排序的 Top 5 Team Leaderboard 和按团队拆分的模型使用堆叠柱状图。
- [ ] AC 10: Given 用户进入 `System & Reliability` tab, when analytics 数据返回, then 页面展示错误率趋势面积图和按渠道聚合的平均延迟对比图。
- [ ] AC 11: Given 用户进入 `Model & Router` tab, when analytics 数据返回, then 页面展示模型份额占比图，并支持悬浮查看请求量或占比的具体数值。
- [ ] AC 12: Given 用户进入 `Records` tab, when usage records 已加载, then 表格显示 Time、Request ID、Team、Route Info、Model、Tokens In/Out、Latency、Status，并支持分页浏览。
- [ ] AC 13: Given records 数据返回, when 表格渲染 `Route Info`, then 至少显示 `matched_rule` 与最终渠道 `final_channel`，而不是仅回显 router 名称。
- [ ] AC 14: Given 用户点击 `Records` tab 中的刷新按钮且当前位于第一页, when 请求成功, then 最新记录出现在表格顶部、按钮展示加载动画，且当前筛选状态不会丢失。
- [ ] AC 15: Given 用户点击 `Records` tab 中的刷新按钮且当前不在第一页, when 检测到有更新记录, then 页面显示 “N new records” 提示，并允许用户跳回第一页查看新增项。
- [ ] AC 16: Given 某条记录的 `latency_ms` 超过约定阈值, when 该记录出现在表格中, then 其延迟值以高风险视觉样式高亮显示。
- [ ] AC 17: Given 用户点击某条记录的 Request ID 复制入口, when 浏览器支持 clipboard API, then 对应 request ID 被复制，且界面给出可见反馈；若不支持，则提供降级反馈。
- [ ] AC 18: Given 用户点击某条 records 行, when 详情抽屉打开, then 页面展示请求、路由、token、状态、错误和 provider diagnostics 等完整明细。
- [ ] AC 19: Given dashboard analytics 查询返回空结果, when 当前筛选无匹配数据, then 各 tab 显示明确的 empty state，而不是空白卡片或前端异常。
- [ ] AC 20: Given 后端 analytics API 出错, when 前端请求失败, then 页面保留上一次成功数据并以内联错误提示告知当前 tab 或全局数据同步失败。
- [ ] AC 21: Given Sankey 在当前 Recharts 版本不可稳定渲染, when topology 模块加载, then 页面自动降级为结构化 flow summary 视图，而不是移除 topology 模块。
- [ ] AC 22: Given dashboard 统计数据在多个 tabs 中复用, when 同一筛选条件下查看 KPI、趋势和排行, then 统计口径以 `usage_records` 聚合结果为准，不出现明显互相冲突的数值。
- [ ] AC 23: Given 前端执行 `npm run build`, when 构建完成, then 静态导出产物仍写入 `target/web`，并保持 Rust `embedded-web` 发布模式可用。
- [ ] AC 24: Given 后端使用 `cargo build --release --features embedded-web` 构建, when 二进制运行, then dashboard 仍可通过内嵌静态资源在 `/dashboard/` 正常打开。

## Additional Context

### Dependencies

- 前端现有依赖已覆盖 `recharts`、`lucide-react`、`@base-ui/react/select`、Tailwind CSS 4 与 Next.js static export，不需要新增第二套前端框架。
- 若要实现 `Tabs`、`Badge` 或更强的 sidebar 交互，可能需要在 `web/src/components/ui` 内补本地组件，而不是新增大型第三方 UI 框架。
- 后端依赖已经具备 `rusqlite`、`chrono` 和 `rust-embed`，足以支持 analytics 查询与 `embedded-web` 打包。
- 新 analytics API 依赖真实 usage/latency/error/fallback 数据完整写入 SQLite；若历史数据字段不足，部分图表可能只能对新数据准确生效。
- filter 选择器若要做成真正的下拉选项，需要明确数据来源：可来自 analytics API 返回的 `filter_options`，或从 usage records 去重生成，但必须在 contract 中写明。
- Sankey 拓扑图依赖团队、router、channel、model 四段链路数据同时存在；如果某段历史记录缺失，需要定义降级展示策略。

### Testing Strategy

- 前端：
  - 使用 Playwright 继续做 dashboard 主流程回归。
  - 为 `Overview`、`Team & Usage`、`System & Reliability`、`Model & Router`、`Records` 五个 tabs 分别补至少一个渲染断言。
  - 为简化认证入口补有效 token、无效 token、无 token 三种路径测试。
  - 为 request ID copy、刷新追加、分页切换、空数据提示和 API 失败保留旧数据补交互回归。
- 后端：
  - 在 `src/database.rs` 增加 analytics 查询测试，覆盖筛选条件、排序、百分比和空数据情况。
  - 如在 `src/server.rs` 新增 handler，增加 API 路由/响应结构测试。
  - 若复用 `mcp/analytics.rs` 聚合逻辑，增加共享统计口径的验证，避免 dashboard 与 MCP 返回不一致。
- 手工验证：
  - `cd web && npm run lint`
  - `cd web && npm run build`
  - `cargo test`
  - `cargo build --release --features embedded-web`
  - 运行后手工访问 `/dashboard/` 验证构建产物、认证入口、tabs、图表和 records。

### Notes

- 高风险项 1：如果直接在现有 `dashboard-client.tsx` 中继续堆叠 tabs 与图表，复杂度会快速失控；必须优先拆分组件与状态职责。
- 高风险项 2：若 dashboard 统计继续同时使用 `metrics_*` 表和 `usage_records` 表的不同口径，而不统一聚合逻辑，图表之间会出现数字不一致。
- 高风险项 3：Sankey 图对数据完整性和组件兼容性要求最高，应尽早验证 Recharts 当前版本可行性，避免最后阶段返工。
- 高风险项 4：认证“简化”只能是前端体验简化，不应误改成绕过后端 `global_auth`；否则会影响安全边界。
- 高风险项 5：如果 `Route Info` 仍拿不到 `matched_rule` 和 `final_channel`，用户点名的全链路可观测性目标将无法兑现。
- 高风险项 6：如果 analytics 实现默认走“全量 records 拉出后再内存聚合”，随着数据规模增长会引发响应时间和内存压力问题。
- 已知限制：本次不实现 Sidebar 中其他业务模块的真实页面，因此 Sidebar 默认是 dashboard 内导航壳层与未来入口占位。
- 已知限制：若历史 usage 记录中缺失 team/router/channel/model 某一维数据，相关图表只能展示现有可用维度或用 `unknown` 分组降级。
- 后续可选方向：如果本次多 tabs 工作台落地后仍然过重，可在下一轮再把 `Records` 或 `Model & Router` 拆成二级页面，而不是在本次 quick spec 中提前扩大范围。
