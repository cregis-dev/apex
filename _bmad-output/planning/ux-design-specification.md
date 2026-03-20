---
stepsCompleted: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]
inputDocuments:
  - "_bmad-output/planning/prd.md"
  - "_bmad-output/stories/web-dashboard.md"
---

# UX Design Specification Apex Gateway

**Author:** Shawn
**Date:** 2026-03-10

---

## Executive Summary

### Project Vision

Apex Gateway 是一个基于 Rust 实现的轻量级 AI 网关服务，Web Dashboard 用于展示 Usage 使用记录与 Metrics 监控指标。

### Target Users

1. **平台管理员 (John/PM)** - 使用 CLI 管理配置、查看运行状态
2. **业务开发者 (Amelia/Dev)** - 通过标准 SDK 连接网关
3. **运维/分析人员 (Quinn/QA)** - 监控健康度、评估调用成本

### Key Design Challenges

- 数据展示的实时性与性能平衡
- 多维度筛选与分页体验
- 响应式设计适配不同设备

### Design Opportunities

- 通过可视化图表提升数据分析效率
- 通过告警机制提升异常发现能力

---

## User Research Findings

### User Persona Focus Group Feedback

#### John (平台管理员) 的需求：
- **一键导出数据** - 每周汇报需要导出 CSV
- **告警阈值设置** - 错误率超过 X% 时通知
- **多租户视图** - 同时看所有 Team 的汇总

#### Amelia (开发者) 的需求：
- **API 状态快速检查** - 快速确认服务是否正常
- **Model 使用统计** - 了解各模型的调用分布
- **调试模式** - 查看具体请求的详情

#### Quinn (运维/分析) 的需求：
- **实时刷新** - 页面自动更新最新数据
- **自定义时间范围** - 支持任意时间段查询
- **成本分析** - 按模型计算调用费用
- **异常高亮** - 错误率飙升时明显提示

### Feature Priority Matrix

| 功能 | 优先级 | 提出者 |
|------|--------|--------|
| 数据导出 (CSV) | 🔴 高 | John |
| 告警阈值设置 | 🔴 高 | John |
| 多租户视图 | 🔴 高 | John |
| 成本分析 | 🔴 高 | Quinn |
| 实时刷新 | 🔴 高 | Quinn |
| API 快速状态 | 🟡 中 | Amelia |
| Model 使用统计 | 🟡 中 | Amelia |
| 自定义时间 | 🟡 中 | Quinn |
| 调试详情 | 🟢 低 | Amelia |

---

## Core User Experience

### Defining Experience

**核心用户行为**: 查看数据看板和筛选过滤

- 用户打开 Dashboard 的首要目的是**快速了解系统使用状况**
- **一键获取关键指标**是最频繁的动作
- 查看趋势图表了解变化趋势

**关键动作**: 认证登录 → 查看指标

### Platform Strategy

| 平台 | 策略 |
|------|------|
| 主要平台 | Web 桌面端 |
| 交互方式 | 鼠标/键盘为主 |
| 响应式 | 需支持平板设备 |
| 离线功能 | 不需要（实时数据） |

### Effortless Interactions

| 应该完全自然的操作 | 当前痛点 |
|-------------------|----------|
| 输入 API Key 一键登录 | 需要手动输入 |
| 切换时间范围 | 步骤较多 |
| 查看趋势数据 | 需要等待加载 |
| 分页浏览记录 | 跳转不够流畅 |

### Critical Success Moments

- **首次登录成功** → 立即看到数据，建立信心
- **错误率异常** → 明显红色提示，快速发现问题
- **数据导出** → 一键下载，支持汇报

### Experience Principles

1. **数据优先** - 关键指标一目了然
2. **快速响应** - 操作即时反馈
3. **清晰可读** - 图表文字清晰可辨
4. **容错友好** - 网络错误不崩溃

---

## Desired Emotional Response

### Primary Emotional Goals

| 情感目标 | 描述 |
|---------|------|
| **掌控感** | 用户感到完全掌控数据，一目了然 |
| **高效感** | 快速完成任务，无需等待 |
| **信任感** | 数据准确可靠，错误及时预警 |

**用户感受**: *"我能快速了解系统状态，一切尽在掌握"*

### Emotional Journey Mapping

| 阶段 | 期望情感 |
|------|----------|
| 首次访问 | 好奇 → 惊喜（数据一目了然） |
| 日常使用 | 自信（快速获取信息） |
| 发现异常 | 警觉但从容（有明确提示） |
| 完成任务 | 成就感（数据导出成功） |

### Micro-Emotions

| 正面情感 | 对立面 |
|---------|--------|
| 清晰明确 | 困惑迷茫 |
| 值得信赖 | 怀疑犹豫 |
| 快速响应 | 焦虑等待 |
| 满足成就 | 挫败沮丧 |

### Design Implications

| 情感 | 设计实现 |
|------|----------|
| 掌控感 | 关键指标卡片突出显示，一眼可见关键数据 |
| 高效感 | 页面加载即显示数据，无需多次点击 |
| 信任感 | 错误率红色高亮，数据来源明确标注 |
| 避免焦虑 | 加载状态明确显示 skeleton，避免空白等待 |

### Emotional Design Principles

1. **数据即安慰** - 清晰的数据展示本身就是安全感来源
2. **快即是爽** - 加载时间越短，用户越满意
3. **预警不恐慌** - 异常提示要明确但不吓人
4. **操作可逆** - 筛选错了可以轻松重置

---

## UX Pattern Analysis & Inspiration

### Inspiring Products Analysis

| 产品 | 值得借鉴的 UX |
|------|---------------|
| Datadog | 仪表板布局、实时指标卡片、告警系统 |
| Cloudflare | 简洁的登录流程、清晰的数据可视化 |
| Vercel | 快速加载、优雅的加载状态、清晰的错误提示 |

### Transferable UX Patterns

| 模式 | 应用场景 |
|------|----------|
| 关键指标卡片 | Dashboard 顶部突出显示 4 个核心指标 |
| Skeleton 加载 | 数据加载时显示骨架屏，避免空白 |
| 时间范围切换 | 顶部固定时间选择器，快速切换 |
| 横向滚动表格 | Usage 记录表格支持横向滚动 |
| 告警徽章 | 错误数超过阈值时红色徽章提示 |

### Anti-Patterns to Avoid

| 反模式 | 问题 |
|--------|------|
| 无限滚动 | 数据量大时性能差 → 使用分页 |
| 弹窗过多 | 干扰用户体验 → 用内联提示 |
| 密集恐惧症 | 一次性展示太多数据 → 分层/折叠 |
| 刷新按钮 | 让用户手动刷新 → 自动刷新 |

### Design Inspiration Strategy

**采用**:
- 顶部关键指标卡片布局
- 固定时间筛选器
- 骨架屏加载状态
- 红色告警高亮

**适配**:
- 简化 Datadog 的复杂性
- 保留 Cloudflare 的简洁风格

**避免**:
- 过于技术化的界面
- 需要培训才能使用

---

## Design System Foundation

### Design System Choice

**推荐选择**: shadcn/ui + Tailwind CSS

### Rationale for Selection

1. **已集成** - 项目已在使用
2. **高度可定制** - 通过 Tailwind 主题变量调整
3. **现代化** - 基于 Radix UI， accessibility 好
4. **开发者友好** - 代码复制而非 npm 包
5. **轻量** - 只引入需要的组件

### Implementation Approach

- 继续使用现有 shadcn/ui 组件
- 通过 Tailwind CSS 定制主题
- 使用 Recharts 进行数据可视化

### Customization Strategy

| 方面 | 定制内容 |
|------|----------|
| 颜色主题 | 沿用现有 shadcn/ui 主题 |
| 组件样式 | 按需修改组件样式 |
| 图表 | Recharts 已有，保持使用 |
| 字体 | 使用 Next.js font optimization |

---

## 2. Core User Experience

### 2.1 Defining Experience

**核心操作**: "一键登录，实时查看系统状态"

用户会这样描述：
> *"打开 Dashboard，输个 API Key，就能看到系统用得怎么样"*

### 2.2 User Mental Model

| 问题 | 当前方案 |
|------|----------|
| 如何看系统状态？ | 打开 Prometheus 面板 |
| 如何查具体调用？ | 看 CSV 日志文件 |
| 如何做汇报？ | 手动导出数据 |

**痛点**: 需要多个工具，割裂的使用体验

### 2.3 Success Criteria

| 标准 | 描述 |
|------|------|
| 3秒内 | 输入 API Key 后看到数据 |
| 一键操作 | 无需配置，立即可用 |
| 直观清晰 | 关键指标一眼可见 |
| 实时更新 | 数据状态保持最新 |

### 2.4 Novel UX Patterns

- 使用成熟模式 - 标准的数据看板布局
- 指标卡片 + 趋势图表 + 数据表格
- 无需教育用户，业界标准实践

### 2.5 Experience Mechanics

| 阶段 | 操作 | 反馈 |
|------|------|------|
| 启动 | 输入 API Key | 即时验证，登录成功跳转 |
| 查看 | 页面加载 | Skeleton 骨架屏 |
| 筛选 | 选择时间/输入过滤 | 即时更新图表 |
| 完成 | 查看/导出 | 数据展示/下载完成 |

---

<!-- UX design content will be appended sequentially through collaborative workflow steps -->

---

## Visual Design Foundation

### Color System

#### Primary Palette

基于 shadcn/ui 默认主题，Dashboard 专用配色:

| 颜色 | 用途 | 色值 |
|------|------|------|
| 主色 | 按钮、链接、强调 | `#0EA5E9` (Sky-500) |
| 主色悬停 | 交互状态 | `#0284C7` (Sky-600) |
| 成功 | 正向指标、正常状态 | `#22C55E` (Green-500) |
| 警告 | 需要注意的状态 | `#F59E0B` (Amber-500) |
| 错误 | 异常、错误率飙升 | `#EF4444` (Red-500) |
| 背景 | 主背景 | `#09090B` (Zinc-950) |
| 卡片背景 | 组件容器 | `#18181B` (Zinc-900) |
| 边框 | 分割线 | `#27272A` (Zinc-800) |

#### Status Colors (Dashboard 专用)

| 状态 | 背景 | 文字 | 用途 |
|------|------|------|------|
| 健康 | `bg-green-500/10` | `text-green-500` | 正常指标 |
| 警告 | `bg-amber-500/10` | `text-amber-500` | 接近阈值 |
| 严重 | `bg-red-500/10` | `text-red-500` | 超过阈值 |
| 信息 | `bg-sky-500/10` | `text-sky-500` | 一般信息 |

#### Chart Colors

趋势图表配色方案 (Recharts):

```
#0EA5E9 (Sky-500) - 请求量
#22C55E (Green-500) - Token 消耗
#A855F7 (Purple-500) - 延迟
#F59E0B (Amber-500) - 成本
#EF4444 (Red-500) - 错误
```

### Typography

#### Font Stack

| 用途 | 字体 | 大小 | 字重 |
|------|------|------|------|
| 页面标题 | Inter | 24px | 600 |
| 卡片标题 | Inter | 18px | 600 |
| 指标数值 | Inter | 32px | 700 |
| 正文 | Inter | 14px | 400 |
| 标签 | Inter | 12px | 500 |
| 代码/数据 | JetBrains Mono | 13px | 400 |

#### Scale

```
xs: 12px    - 标签、徽章
sm: 14px    - 正文、输入框
base: 16px  - 卡片标题
lg: 18px    - Section 标题
xl: 24px    - 页面标题
2xl: 32px   - 指标数值
3xl: 48px   - Hero 数字 (如总请求)
```

### Spacing System

基于 4px 网格:

| Token | 值 | 用途 |
|-------|-----|------|
| `space-1` | 4px | 组件内部紧密间距 |
| `space-2` | 8px | 组件内元素间距 |
| `space-3` | 12px | 紧凑卡片内边距 |
| `space-4` | 16px | 标准间距 |
| `space-5` | 20px | Section 内间距 |
| `space-6` | 24px | 卡片间距 |
| `space-8` | 32px | 大区块间距 |
| `space-12` | 48px | 页面边距 |

### Layout Specifications

#### Dashboard Grid

```
┌─────────────────────────────────────────────────────────────┐
│ Header: Time Range Selector              [Auto-refresh]   │
├─────────────────────────────────────────────────────────────┤
│ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐        │
│ │Total    │ │Success  │ │Latency  │ │Cost     │  Cards  │
│ │Requests │ │Rate     │ │P99      │ │Today    │         │
│ └─────────┘ └─────────┘ └─────────┘ └─────────┘        │
├─────────────────────────────────────────────────────────────┤
│ ┌─────────────────────┐ ┌─────────────────────┐          │
│ │   Requests Trend    │ │   Token Trend       │  Charts  │
│ │   (Area Chart)      │ │   (Area Chart)      │          │
│ └─────────────────────┘ └─────────────────────┘          │
├─────────────────────────────────────────────────────────────┤
│ Filters: [Team] [Router] [Model] [Status]     [Export CSV]│
├─────────────────────────────────────────────────────────────┤
│ ┌─────────────────────────────────────────────────────┐   │
│ │ Usage Records Table (Pagination)                    │   │
│ │ Time | Team | Model | Tokens | Latency | Status    │   │
│ └─────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────┤
│ ┌───────────────┐ ┌───────────────┐ ┌───────────────┐     │
│ │ Top Teams     │ │ Top Models   │ │ Top Channels │     │
│ └───────────────┘ └───────────────┘ └───────────────┘     │
└─────────────────────────────────────────────────────────────┘
```

#### Responsive Breakpoints

| 断点 | 宽度 | 布局变化 |
|------|------|----------|
| Mobile | < 640px | 单列，指标卡片 2x2 |
| Tablet | 640-1024px | 2 列，图表 1x2 |
| Desktop | 1024-1440px | 4 列指标，2 列图表 |
| Wide | > 1440px | 保持 Desktop，最大宽度 1600px |

### Component States

#### Interactive Elements

| 状态 | 样式变化 |
|------|----------|
| Default | `bg-sky-500 text-white` |
| Hover | `bg-sky-600` + `scale(1.02)` |
| Active | `bg-sky-700` + `scale(0.98)` |
| Disabled | `opacity-50 cursor-not-allowed` |
| Loading | `opacity-80` + spinner |

#### Data Loading

| 场景 | 显示 |
|------|------|
| 初始加载 | Skeleton 骨架屏 (与实际布局一致) |
| 刷新数据 | 脉冲动画 (pulse) |
| 无数据 | 空状态插图 + "暂无数据" 文字 |
| 加载错误 | 红色提示条 + 重试按钮 |

### Animations

| 动画 | 持续时间 | 用途 |
|------|----------|------|
| 淡入 | 150ms | 页面切换 |
| 滑入 | 200ms | 抽屉、弹窗 |
| 骨架脉冲 | 1.5s (循环) | 加载占位 |
| 数字递增 | 500ms | 指标数值变化 |
| 悬停缩放 | 150ms | 卡片悬停 |

---

## Step 9: Design Directions

### 9.1 Design Direction Options

基于用户研究和功能优先级，推荐以下设计方向:

#### Option A: 数据优先型 (Recommended)

**核心理念**: 关键指标最大化展示，一眼获取系统状态

- 4 个核心指标卡片顶部突出显示
- 趋势图表占据主视觉区域
- 排行榜快速识别 Top 使用者
- 筛选器固定在页面顶部

**适用场景**: Quinn (运维) 日常监控为主

#### Option B: 任务导向型

**核心理念**: 快速完成特定任务，减少点击

- 左侧快捷操作栏
- 支持自定义仪表板布局
- 一键导出功能突出
- 常用筛选条件保存

**适用场景**: John (PM) 每周汇报需求

#### Option C: 开发者友好型

**核心理念**: 面向技术用户的调试和排查

- 请求详情面板 (Drawer)
- 调试模式切换
- API 响应预览
- 错误堆栈友好展示

**适用场景**: Amelia (Dev) 调试排查

### 9.2 Selected Direction

**[A] 数据优先型** - 符合核心用户 Quinn 的监控需求，也是最常用场景

### 9.3 Key Design Decisions

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 指标卡片 | 4 列固定 | 业界标准，符合期望 |
| 图表位置 | 指标下方 | 信息层级清晰 |
| 时间选择器 | 固定顶部 | 快速切换，无需滚动 |
| 筛选器 | 内联展示 | 所见即所得 |
| 导出按钮 | 右侧固定 | 一键可达 |

---

## Step 10: User Journeys

### 10.1 Primary User Journey: 日常监控 (Quinn)

```
1. 打开 Dashboard
   ↓ (自动跳转或手动输入 API Key)
2. 立即看到核心指标 (请求量、成功率、延迟、成本)
   ↓
3. 查看趋势图表了解变化
   ↓
4. 切换时间范围 (Today/Week/Month)
   ↓
5. 应用筛选 (Team/Model)
   ↓
6. 查看 Usage 表格详情
```

**关键路径长度**: 6 步
**核心价值**: 3 秒内获取系统状态

### 10.2 Secondary Journey: 周报导出 (John)

```
1. 登录 Dashboard
   ↓
2. 切换到 "Week" 时间范围
   ↓
3. 选择 "All Teams" 视图
   ↓
4. 点击 "Export CSV" 按钮
   ↓
5. 下载数据文件
```

**关键路径长度**: 5 步
**核心价值**: 一键导出，无需手工整理

### 10.3 Tertiary Journey: 问题排查 (Amelia)

```
1. 登录 Dashboard
   ↓
2. 注意到错误率告警徽章
   ↓
3. 切换到问题时间段
   ↓
4. 筛选特定 Model/Channel
   ↓
5. 查看 Usage 表格定位异常请求
   ↓
6. 点击查看请求详情
```

**关键路径长度**: 6 步
**核心价值**: 快速定位问题根因

### 10.4 Journey Map Summary

| Journey | 频率 | 步骤 | 关键痛点 |
|---------|------|------|----------|
| 日常监控 | 多次/天 | 6 | 加载速度 |
| 周报导出 | 1次/周 | 5 | 手动整理 |
| 问题排查 | 偶尔 | 6 | 详情不足 |

---

## Step 11: Component Strategy

### 11.1 Core Components

| 组件 | 用途 | 状态 |
|------|------|------|
| `TimeRangeSelector` | 时间范围切换 | 需开发 |
| `MetricCard` | 指标展示卡片 | 需开发 |
| `TrendChart` | 趋势图表 | 已有 (Recharts) |
| `UsageTable` | 使用记录表格 | 需开发 |
| `FilterBar` | 筛选工具栏 | 需开发 |
| `RankingList` | 排行榜 | 需开发 |
| `Pagination` | 分页控件 | 已有 (shadcn) |
| `AlertBadge` | 告警徽章 | 需开发 |
| `ExportButton` | 导出按钮 | 需开发 |
| `SkeletonLoader` | 骨架屏 | 已有 (shadcn) |

### 11.2 Component Specifications

#### MetricCard

```
Props:
- title: string (指标名称)
- value: number | string (数值)
- change: number (变化百分比)
- status: 'positive' | 'negative' | 'neutral'
- icon: ReactNode (图标)
- loading: boolean

Variants:
- default: 标准卡片
- highlight: 高亮状态 (告警时)
- compact: 移动端压缩版
```

#### UsageTable

```
Props:
- data: UsageRecord[]
- pagination: PaginationState
- onPageChange: (page: number) => void
- onSort: (field: string, dir: 'asc' | 'desc') => void

Features:
- 列: Time, Team, Router, Model, Channel, Input Tokens, Output Tokens, Latency, Status
- 横向滚动支持
- 行悬停高亮
- 点击展开详情
```

#### FilterBar

```
Props:
- teams: string[]
- routers: string[]
- models: string[]
- channels: string[]
- onFilterChange: (filters: FilterState) => void

Elements:
- Select: Team 筛选
- Select: Router 筛选
- Input: Model 搜索
- Select: Channel 筛选
- Select: Status 筛选
- Button: 重置筛选
- Button: 导出 CSV
```

### 11.3 Layout Components

| 组件 | 描述 |
|------|------|
| `DashboardHeader` | 顶部导航 + 时间选择器 |
| `MetricsGrid` | 4 列指标卡片网格 |
| `ChartsSection` | 2 列图表容器 |
| `FilterSection` | 筛选工具栏 + 导出 |
| `TableSection` | 使用记录表格 |
| `RankingsSection` | 3 列排行榜 |

### 11.4 Responsive Behavior

| 断点 | 指标卡片 | 图表 | 表格 |
|------|----------|------|------|
| Mobile | 2x2 | 1 列 | 横向滚动 |
| Tablet | 2x2 | 1 列 | 横向滚动 |
| Desktop | 4 列 | 2 列 | 完整显示 |

---

## Step 12: UX Patterns

### 12.1 Navigation Patterns

#### Primary Navigation

- **位置**: 页面顶部固定
- **内容**: Logo + 时间范围选择器 + 刷新开关 + 用户菜单
- **行为**: 始终可见，滚动不隐藏

#### Secondary Navigation

- **位置**: 筛选工具栏下方 (固定)
- **内容**: Tab 切换 (Overview / Usage / Analytics)
- **行为**: 滚动时可考虑吸附

### 12.2 Data Patterns

#### Loading States

| 场景 | 模式 |
|------|------|
| 初始加载 | Skeleton 骨架屏 (与实际布局一致) |
| 刷新数据 | 局部脉冲动画 |
| 分页加载 | 表格行 skeleton |
| 图表加载 | 图表区域 skeleton |

#### Error States

```
┌─────────────────────────────────────┐
│ ⚠️ 连接断开                          │
│ 无法获取最新数据                      │
│ [重试]                              │
└─────────────────────────────────────┘

- 显示位置: 对应数据区域
- 自动重试: 30 秒后
- 手动重试: 按钮点击
```

#### Empty States

```
┌─────────────────────────────────────┐
│ 📊 暂无数据                          │
│ 在选定时间范围内没有使用记录           │
│ 尝试: 扩大时间范围                   │
└─────────────────────────────────────┘
```

### 12.3 Interaction Patterns

#### 筛选即时反馈

```
用户操作 → 300ms 延迟防抖 → 请求数据 → 更新UI
         ↓
      显示加载指示器
```

#### 时间范围切换

```
Today ←→ Week ←→ Month ←→ Custom
  ↓
请求新数据 (保留筛选状态)
```

#### 数据导出

```
点击 Export → 显示进度 → 下载文件 → 成功提示
```

### 12.4 Feedback Patterns

#### 成功反馈

- 指标更新: 数字递增动画
- 导出完成: Toast 通知 "导出成功"

#### 警告反馈

- 错误率 > 5%: 卡片红色边框 + 徽章
- 错误率 > 10%: 页面顶部警告条
- API Key 无效: 重定向登录 + 提示

#### 操作反馈

- 按钮点击: 轻微缩放 (scale 0.98)
- 筛选重置: 按钮状态即时更新

---

## Step 13: Responsive & Accessibility

### 13.1 Responsive Strategy

#### Mobile (< 640px)

| 区域 | 适配方案 |
|------|----------|
| 指标卡片 | 2x2 网格，每行 2 个 |
| 图表 | 全宽，单列堆叠 |
| 表格 | 横向滚动，固定首列 |
| 筛选器 | 折叠为 "筛选" 按钮 |
| 排行榜 | 垂直堆叠，每项 1 列 |

#### Tablet (640px - 1024px)

| 区域 | 适配方案 |
|------|----------|
| 指标卡片 | 2x2 网格 |
| 图表 | 1x2 网格 |
| 表格 | 横向滚动 |
| 筛选器 | 部分展开 |

#### Desktop (> 1024px)

| 区域 | 适配方案 |
|------|----------|
| 指标卡片 | 4 列网格 |
| 图表 | 2 列网格 |
| 表格 | 完整显示 |
| 筛选器 | 完全展开 |

### 13.2 Accessibility Requirements

#### WCAG 2.1 AA 合规

| 要求 | 实现 |
|------|------|
| 颜色对比度 | 文本 ≥ 4.5:1，大文本 ≥ 3:1 |
| 焦点可见 | 所有交互元素有明确 focus 样式 |
| 键盘导航 | 支持 Tab 导航，Enter 确认 |
| 屏幕阅读 | ARIA 标签，语义化 HTML |
| 动态内容 | ARIA live regions 通知更新 |

#### 特定组件 Accessibility

| 组件 | ARIA 要求 |
|------|-----------|
| 时间选择器 | `role="radiogroup"`, `aria-label` |
| 表格 | `role="table"`, `aria-sort` |
| 图表 | `aria-label` 描述数据趋势 |
| 筛选器 | `aria-label` 描述筛选条件 |
| 告警 | `role="alert"`, `aria-live="polite"` |

### 13.3 Keyboard Shortcuts

| 快捷键 | 动作 |
|--------|------|
| `T` | 切换时间范围 |
| `R` | 刷新数据 |
| `/` | 聚焦筛选器 |
| `E` | 导出 CSV |
| `Esc` | 关闭弹窗 |

---

## Step 14: Complete

### Design Deliverables Summary

| 阶段 | 内容 | 状态 |
|------|------|------|
| Step 1-7 | 用户研究 & 需求分析 | ✅ |
| Step 8 | 视觉设计基础 | ✅ |
| Step 9 | 设计方向 | ✅ |
| Step 10 | 用户旅程 | ✅ |
| Step 11 | 组件策略 | ✅ |
| Step 12 | UX 模式 | ✅ |
| Step 13 | 响应式 & 无障碍 | ✅ |

### Next Steps

1. **创建实现 Stories**: 将设计规格拆分为可开发的任务
2. **技术验证**: 确认现有技术栈支持所有设计决策
3. **原型验证**: 可选 - 创建交互原型进行用户测试

### Design Files

- **UX Spec**: `_bmad-output/planning/ux-design-specification.md`
- **PRD**: `_bmad-output/planning/prd.md`
- **Story**: `_bmad-output/stories/web-dashboard.md`

---

**UX Design Status**: ✅ Complete - 2026-03-10
