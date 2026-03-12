# Web Dashboard 开发计划

## 概述

本文档描述 Apex Gateway Web Dashboard 功能的开发计划，包括用户故事、技术方案和任务拆分。

## 背景

当前 Apex Gateway 使用 CSV 文件记录 Usage 数据，Metrics 指标仅暴露在 Prometheus 端点。为了方便用户查看使用记录和指标，需要：
1. 使用 SQLite 数据库替代 CSV 文件存储 Usage 记录
2. 将 Metrics 指标也存入数据库
3. 提供 Web Dashboard 展示这些数据

## Dev Agent Record

### File List (变更文件)

| 文件 | 操作 | 说明 |
|------|------|------|
| Cargo.toml | 修改 | 添加 rusqlite 依赖 |
| Cargo.lock | 修改 | 自动更新 |
| src/lib.rs | 修改 | 添加 database 模块 |
| src/main.rs | 修改 | 添加 database 模块引用 |
| src/config.rs | 修改 | 添加 web_dir 配置支持 |
| src/server.rs | 修改 | 添加 API 端点、数据库集成和 Dashboard 静态文件服务 |
| src/usage.rs | 修改 | UsageLogger 改用 SQLite |
| src/database.rs | 新增 | 数据库模块，包含表结构和 CRUD 操作 |
| docs/PRD.md | 修改 | 更新 PRD 文档 |
| web/ | 新增 | Next.js + shadcn/ui 前端项目 |
| install.sh | 新增 | 安装脚本，一键部署到指定路径 |
| .gitignore | 修改 | 添加 test-config.json 等忽略规则 |

## 用户故事

### Story 1: 数据库存储改造
> 作为系统，我需要将 Usage 记录和 Metrics 指标存储到 SQLite 数据库，以便支持历史数据查询和统计分析。

**验收标准**：
- [x] 添加 rusqlite 依赖
- [x] 创建数据库模块，包含 usage_records, metrics_requests, metrics_errors, metrics_fallbacks, metrics_latency 表
- [x] UsageLogger 使用 SQLite 替代 CSV
- [x] Metrics 事件 (请求、错误、FallBack、延迟) 写入数据库

### Story 2: Usage API 端点
> 作为运维人员，我需要通过 API 获取 Usage 记录，以便在 Dashboard 中展示。

**验收标准**：
- [x] 实现 `GET /api/usage` 端点
- [x] 支持查询参数：`team_id`, `router`, `channel`, `model`, `limit`, `offset`
- [x] 返回 JSON 格式数据

### Story 3: Metrics API 端点
> 作为运维人员，我需要通过 API 获取 Metrics 汇总数据，以便在 Dashboard 中展示。

**验收标准**：
- [x] 实现 `GET /api/metrics` 端点
- [x] 返回总请求数、总错误数、总 FallBack 次数、平均延迟

### Story 4: Dashboard 前端页面 (MVP)
> 作为运维人员，我需要通过 Web 界面查看 Usage 记录和 Metrics 指标，以便直观了解系统使用情况。

**验收标准**：
- [x] 实现 `GET /dashboard` 端点
- [x] 展示 Metrics 汇总卡片
- [x] 展示 Usage 记录表格
- [x] 支持基础筛选和分页

### Story 5: 趋势分析图表
> 作为 Team 管理者，我需要查看使用量的时间趋势，以便了解增长或下降模式。

**验收标准**：
- [ ] 实现日/周请求量趋势图
- [ ] 实现 Token 消耗趋势图
- [ ] 支持时间范围选择

**UI 布局**:
```
┌─────────────────────────────────────┐
│  📈 趋势分析                         │
├─────────────────────────────────────┤
│  [今日] [本周] [本月] [自定义]       │
├─────────────────────────────────────┤
│  📊 请求量趋势 (折线图)              │
│     │_                              │
│     │  ‾‾‾‾‾‾                       │
│     └──────────────                 │
├─────────────────────────────────────┤
│  📊 Token 消耗趋势 (面积图)         │
│     ████▓▓▒▒░░                     │
│     └──────────────                 │
└─────────────────────────────────────┘
```

### Story 6: 排行榜统计
> 作为 Team 管理者，我需要了解哪些 Team/Model/Channel 使用最多，以便指导资源分配。

**验收标准**：
- [ ] 按 Team 显示使用量占比
- [ ] 按 Model 显示使用量排名
- [ ] 按 Channel 显示调用分布
- [ ] 支持点击查看详情

**UI 布局**:
```
┌─────────────────┬─────────────────┐
│  🏢 Team 排行   │  🤖 Model 排行  │
├─────────────────┼─────────────────┤
│  Team A  ████ 40%│  gpt-4   ███ 35%│
│  Team B  ███ 30%│  claude  ████ 45%│
│  Team C  ██  20%│  gemini  ██  15%│
│  Other   █   10%│  other   █   5% │
└─────────────────┴─────────────────┘
```

### Story 7: 时间筛选增强
> 作为运维人员，我需要按不同时间维度筛选数据，以便分析特定时段的使用情况。

**验收标准**：
- [ ] 预设时间选项：今日、本周、本月
- [ ] 自定义日期范围选择器
- [ ] 时间筛选联动所有图表和表格

## 技术方案

### 后端技术栈
- **语言**: Rust
- **Web 框架**: Axum
- **数据库**: SQLite (rusqlite)
- **数据存储位置**: `data/apex.db`

### 数据库表设计

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

### API 设计

#### GET /api/usage
**Query Parameters**:
| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| team_id | string | 否 | 按团队筛选 |
| router | string | 否 | 按路由筛选 |
| channel | string | 否 | 按渠道筛选 |
| model | string | 否 | 按模型筛选 |
| limit | int | 否 | 默认 50，最大 100 |
| offset | int | 否 | 默认 0 |

**Response**:
```json
{
  "data": [
    {
      "id": 1,
      "timestamp": "2024-01-01 10:00:00",
      "team_id": "team1",
      "router": "router1",
      "channel": "openai",
      "model": "gpt-4",
      "input_tokens": 100,
      "output_tokens": 200
    }
  ],
  "total": 1000,
  "limit": 50,
  "offset": 0
}
```

#### GET /api/metrics
**Response**:
```json
{
  "total_requests": 10000,
  "total_errors": 50,
  "total_fallbacks": 20,
  "avg_latency_ms": 150.5
}
```

#### GET /api/metrics/trends
**Query Parameters**:
| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| period | string | 否 | `daily`, `weekly`, `monthly`，默认 `daily` |
| start_date | string | 否 | 开始日期 (YYYY-MM-DD) |
| end_date | string | 否 | 结束日期 (YYYY-MM-DD) |

**Response**:
```json
{
  "period": "daily",
  "data": [
    { "date": "2024-01-01", "requests": 1000, "input_tokens": 50000, "output_tokens": 80000 },
    { "date": "2024-01-02", "requests": 1200, "input_tokens": 60000, "output_tokens": 95000 }
  ]
}
```

#### GET /api/metrics/rankings
**Query Parameters**:
| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| by | string | 否 | `team`, `model`, `channel`，默认 `team` |
| limit | int | 否 | 返回数量，默认 10 |

**Response**:
```json
{
  "by": "team",
  "data": [
    { "name": "team-a", "requests": 5000, "input_tokens": 250000, "output_tokens": 400000, "percentage": 40 },
    { "name": "team-b", "requests": 3000, "input_tokens": 150000, "output_tokens": 250000, "percentage": 30 }
  ]
}
```

## 任务拆分

### Phase 1: 数据层改造 (已完成)
- [x] Task #1: 添加 rusqlite 依赖
- [x] Task #2: 创建数据库模块
- [x] Task #3: 更新 UsageLogger 使用 SQLite
- [x] Task #4: 添加 Metrics 事件存储

### Phase 2: API 开发
- [x] Task #7: 实现 Usage API 端点
- [x] Task #8: 实现 Metrics API 端点
- [x] Task #11: 实现 Trends API 端点
- [x] Task #12: 实现 Rankings API 端点

### Phase 3: Dashboard 前端 (MVP - 已完成)
- [x] Task #9: 实现 Dashboard 页面路由
- [x] Task #10: 开发 Dashboard 前端

### Phase 4: 趋势图表 (Story 5)
- [ ] Task #13: 添加趋势图组件 (shadcn/ui chart)
- [ ] Task #14: 实现 Trends API 后端
- [ ] Task #15: 前端集成趋势图表

### Phase 5: 排行榜统计 (Story 6)
- [ ] Task #16: 实现 Rankings API 后端
- [ ] Task #17: 前端展示排行榜组件

### Phase 7: 生产环境打包 (Story 打包部署)
- [x] Task #20: 配置 Next.js 静态导出到 target/web
- [x] Task #21: 添加 web_dir 配置支持
- [x] Task #22: 实现静态文件服务路由
- [x] Task #23: 创建 install.sh 安装脚本
- [x] Task #24: 添加 CORS 配置支持
- [x] Task #25: 添加路径遍历安全防护

### Phase 6: 时间筛选增强 (Story 7)
- [ ] Task #18: 时间选择器组件
- [ ] Task #19: 筛选逻辑联动

#### 前端技术栈
- **框架**: Next.js 16 (App Router)
- **UI 组件**: shadcn/ui
- **样式**: Tailwind CSS
- **目录**: `web/`

#### 页面结构
- `/` - 首页，提供 Dashboard 链接
- `/dashboard` - 主 Dashboard 页面
  - API Key 输入 (需要认证)
  - 时间筛选器 (今日/本周/本月/自定义)
  - Metrics 汇总卡片 (总请求、Token、错误率、延迟)
  - 📈 趋势图表 (请求量、Token 消耗)
  - 🏆 排行榜 (Team / Model / Channel)
  - 筛选表单 (Team ID, Router, Channel, Model)
  - Usage 记录表格
  - 分页控件

## 进度追踪

| 阶段 | 状态 | 说明 |
|------|------|------|
| Phase 1: 数据层改造 | ✅ 完成 | SQLite 数据库存储 |
| Phase 2: API 开发 | ✅ 完成 | Usage + Metrics API |
| Phase 3: Dashboard 前端 (MVP) | ✅ 完成 | 基础展示页面 |
| Phase 4: 趋势图表 | ✅ 完成 | 趋势折线图/面积图 |
| Phase 5: 排行榜统计 | ✅ 完成 | Team/Model/Channel 排行 |
| Phase 6: 时间筛选增强 | ✅ 完成 | 时间选择器 |
| Phase 7: 生产环境打包 | ✅ 完成 | target/web 输出、install.sh 脚本 |

## 注意事项

1. **向后兼容**: 原有的 Prometheus 端点 `/metrics` 保持不变
2. **性能考虑**: 数据库写入使用异步方式，不阻塞请求处理
3. **数据量**: 初期不做数据清理，后续可根据需要添加自动清理机制

## Change Log

### 2026-03-07 - 趋势图表与排行榜实现
- **实现 Trends API**: `/api/metrics/trends` 支持 daily/weekly/monthly
- **实现 Rankings API**: `/api/metrics/rankings` 支持 team/model/channel
- **更新前端 Dashboard**:
  - 时间筛选器 (今日/本周/本月/自定义)
  - 请求量趋势折线图
  - Token 消耗柱状图
  - Team/Model/Channel 排行榜

### 2026-03-07 - Party Mode 讨论更新
- **新增 Story 5**: 趋势分析图表 (日/周/月请求量、Token 消耗)
- **新增 Story 6**: 排行榜统计 (Team/Model/Channel 排行)
- **新增 Story 7**: 时间筛选增强 (今日/本周/本月/自定义)
- **新增 API**: `/api/metrics/trends` - 趋势数据
- **新增 API**: `/api/metrics/rankings` - 排行榜数据
- **更新 Phase 4-6**: 新增趋势图表、排行榜、时间筛选任务

### 2026-03-07 - Code Review Fixes
- **修复 1**: 更新 Story 2 和 Story 3 的验收标准为已完成状态
- **修复 2**: 添加 Dev Agent Record → File List 记录所有变更文件
- **修复 3**: API 响应添加 `total` 字段支持分页显示总记录数
  - 更新 `src/database.rs` 的 `get_usage_records` 返回 `(Vec<UsageRecord>, i64)` 元组
  - 更新 `src/server.rs` 的 `usage_api_handler` 返回 total 字段
  - 更新 `web/src/app/dashboard/page.tsx` 前端接口和分页显示

### 2026-03-09 - 生产环境 Web Dashboard 打包
- **Next.js 静态导出配置**: 配置 `output: "export"` 和 `trailingSlash: true`
- **构建输出目录**: 改为输出到 `target/web` 目录，避免污染源代码
- **Config 配置**: 添加 `web_dir` 字段，支持从配置文件指定 Web 目录
- **安装脚本**: 创建 `install.sh` 脚本，一键部署 apex 二进制和 web 内容到指定路径
- **静态文件服务**: 在 `server.rs` 中添加 `/dashboard/*path` 路由，使用 `ServeDir` 提供静态文件
- **CORS 配置**: 添加 `cors_allowed_origins` 全局配置支持
- **路径遍历防护**: 添加安全检查防止目录遍历攻击
