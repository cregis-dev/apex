# Dashboard 页面结构说明

**Project:** Apex Gateway  
**Date:** 2026-03-11  
**Primary file baseline:** `/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx`  
**Related UX input:** `/Users/shawn/workspace/code/apex/docs/planning-artifacts/dashboard-ux-revision-plan-2026-03-11.md`

---

## 1. 文档目的

这份文档把 dashboard 修订方案继续推进到组件级结构说明。

目标不是描述视觉风格，而是回答 4 个实现问题：

1. 页面应该有哪些模块
2. 模块之间按什么优先级排列
3. 每个模块需要展示什么字段
4. 现有代码应该如何收敛，而不是继续堆功能

---

## 2. 页面定义

页面路径：`/dashboard`

页面角色：`监控总览页`

首屏目标：

- 5 秒内判断系统是否健康
- 15 秒内定位异常发生在哪个时间段或哪个维度
- 30 秒内进入 usage 明细验证问题

页面原则：

- 首页只服务“看状态”和“定位问题”
- 深度分析不是首页任务
- 首页不承担低频管理操作

---

## 3. 页面模块树

建议将页面结构固定为以下层级：

```text
DashboardPage
├─ AuthGate
│  └─ ApiKeyLoginCard
└─ DashboardShell
   ├─ DashboardHeader
   ├─ GlobalControlBar
   ├─ AlertBanner
   ├─ KpiSummarySection
   ├─ TrendSection
   ├─ FilterSection
   ├─ UsageTableSection
   └─ UsageDetailsDrawer
```

说明：

- `Rankings` 不再作为首页主模块
- `Team / Model / Channel Rankings` 从首页降级为二级分析模块
- 如果必须保留，建议折叠到页面底部的 `SecondaryInsightsSection`

---

## 4. 与现有实现的对照

当前 `dashboard-client.tsx` 已有这些模块：

- API Key 登录
- 页面标题与断开连接
- 时间范围
- KPI 卡片
- 趋势图
- Rankings
- Filters
- Usage Table

建议调整如下：

| 现有模块 | 处理建议 | 原因 |
|------|------|------|
| API Key 登录 | 保留 | 是访问入口 |
| 页面标题区 | 保留并增强 | 需要加入更新时间和状态摘要 |
| Time Range | 并入全局控制栏 | 不应独立占一大张卡片 |
| KPI 卡片 | 保留但重定义指标 | 需要统一口径 |
| 趋势图 | 保留但改为更清晰的双图结构 | 当前 token 图对监控价值偏弱 |
| Rankings | 从首页主区移除 | 视觉上抢空间，信息优先级偏低 |
| Filters | 保留但改成交互状态更明确的过滤条 | 当前仅是 4 个输入框 |
| Usage Table | 保留并强化 | 它是验证异常的关键落点 |

---

## 5. 模块级规格

## 5.1 AuthGate

### 角色

在未认证时阻断 dashboard 内容，展示 API Key 登录入口。

### 当前实现

已存在于 `/Users/shawn/workspace/code/apex/web/src/components/dashboard/dashboard-client.tsx`。

### 保留规则

- 保留本地存储 API Key 的能力
- 保留 URL `auth_token` 导入能力
- 登录失败时区分：
  - 无效 API Key
  - 服务暂时不可用

### 文案建议

- 标题：`Apex Gateway Dashboard`
- 辅助文案：`使用 API Key 访问实时网关指标与调用记录`

---

## 5.2 DashboardHeader

### 角色

作为已登录后的页面头部，负责建立“可信赖、可追踪”的第一印象。

### 展示内容

- 页面标题：`Gateway Overview`
- 子文案：当前数据范围说明
- 最近成功更新时间
- 连接状态
- `Disconnect` 按钮

### 交互规则

- 最近更新时间应来自最后一次成功取数时间，而不是页面加载时间
- 当数据拉取失败时，头部保留上次成功时间

### 建议组件

- `HeaderTitleBlock`
- `ConnectionStatusBadge`
- `LastUpdatedText`
- `DisconnectButton`

---

## 5.3 GlobalControlBar

### 角色

集中承载所有会影响全局数据视角的控制项。

### 包含控件

- 时间范围切换
- 自定义起止日期
- 自动刷新开关
- 导出按钮

### 结构建议

```text
GlobalControlBar
├─ TimeRangeTabs
├─ CustomDateRange
├─ AutoRefreshToggle
└─ ExportButton
```

### 规则

- 预设时间范围：`24H`、`7D`、`30D`、`Custom`
- 默认：`7D` 可接受，但推荐改为 `24H`
- 选择 `Custom` 时才显示日期输入
- 自动刷新默认开启，间隔 30 秒
- `Custom` 模式下自动刷新自动关闭
- 导出默认导出当前筛选结果

### 对现有代码的影响

当前 `Time Range` 独立卡片应取消，改并入头部下方的横向控制栏。

---

## 5.4 AlertBanner

### 角色

作为异常信息的最高优先级承载区，避免异常只散落在卡片或 toast 中。

### 触发场景

- 数据拉取失败
- 错误率明显升高
- Fallback 激增
- 仅展示部分数据

### 展示内容

- 异常级别
- 简短原因
- 关联时间范围
- 快速操作：`Retry` / `Reset filters`

### 规则

- `requestError` 不应只在普通 Card 中出现
- 应升级为页面顶部横幅

---

## 5.5 KpiSummarySection

### 角色

首屏第一视线区，只服务“现在健康不健康”的判断。

### 卡片数量

固定 4 张，不增加。

### 卡片定义

1. `Total Requests`
2. `Error Rate`
3. `P95 Latency`
4. `Fallbacks`

### 卡片字段

每张卡片应包含：

- 标题
- 当前值
- 与上一周期对比的 delta
- 状态色
- 简短说明 tooltip

### 状态规则

- 正常：中性或成功色
- 关注：警告色
- 危险：错误色

### 对现有代码的影响

当前实现是：

- `Total Requests`
- `Total Errors`
- `Total Fallbacks`
- `Avg Latency`

建议替换为：

- `Total Requests`
- `Error Rate`
- `P95 Latency` 或临时 `Avg Latency`
- `Fallbacks`

原因：

- `Total Errors` 的业务解释力不如 `Error Rate`
- `Avg Latency` 对异常感知不如 `P95`

---

## 5.6 TrendSection

### 角色

帮助用户从总体状态进入时间维度观察，是首屏第二层信息。

### 模块数量

固定 2 张图。

### 图表定义

#### 图 1: Request Volume Trend

用途：

- 看请求量变化
- 判断流量波动是否异常

字段：

- x 轴：时间
- y 轴：requests

建议图型：

- 折线图或面积图

#### 图 2: Reliability Trend

用途：

- 看错误率和延迟在同一时间段的波动

字段：

- x 轴：时间
- y 轴左：error_rate
- y 轴右：latency

建议图型：

- 双轴折线图

### 对现有代码的影响

当前是：

- `Requests Trend`
- `Token Consumption`

建议改为：

- `Requests Trend`
- `Error Rate / Latency Trend`

原因：

- `Token Consumption` 更偏分析型，不是首页监控第一优先级
- 监控用户更需要直接看到稳定性波动

### 可选下沉模块

`Token Consumption` 可以在二级分析区保留，或作为可切换视图。

---

## 5.7 FilterSection

### 角色

让用户缩小问题范围，而不是承担高级搜索器的职责。

### 展示结构

```text
FilterSection
├─ TeamFilter
├─ RouterFilter
├─ ChannelFilter
├─ ModelFilter
├─ ActiveFilterChips
└─ ResetAllButton
```

### 输入形式建议

- 如果选项数量有限，用 `Select`
- 如果需要模糊输入，保留 `Input`
- 优先避免全是自由输入框

### 规则

- 输入后即时生效
- 所有已生效筛选以 chips 形式显示
- 点击 chip 可单独移除
- 点击 `Reset all` 清空全部筛选
- 筛选状态应同步 URL query

### 对现有代码的影响

当前只有 4 个 `Input`，缺少：

- 生效状态回显
- 重置操作
- URL 同步

---

## 5.8 UsageTableSection

### 角色

这是首页最重要的验证区。用户在这里确认异常是否真实发生、发生在哪些记录上。

### 表格字段

建议列顺序：

1. `Timestamp`
2. `Team`
3. `Router`
4. `Channel`
5. `Model`
6. `Total Tokens`
7. `Latency`
8. `Status`

### 字段规则

- `Total Tokens = input_tokens + output_tokens`
- `Status` 需要映射：
  - `Success`
  - `Error`
  - `Fallback`
- 时间列使用等宽字体
- 数值列右对齐

### 排序规则

- 默认按时间倒序
- 第一版可以只支持默认排序
- 如果后续开放排序，优先支持 `Timestamp` 和 `Latency`

### 状态规则

- 加载中：使用 skeleton row，不用纯文本 `Loading...`
- 空数据：区分“系统无数据”和“筛选无结果”
- 错误态：保留最近成功结果并提示刷新失败

### 分页规则

- 保留服务端分页
- 分页文案要处理 `0` 条记录情况
- `Showing 0-0 of 0` 不应出现

### 对现有代码的影响

当前表格缺少：

- `Total Tokens`
- `Latency`
- `Status`
- 行点击行为
- 更细致的空状态

---

## 5.9 UsageDetailsDrawer

### 角色

承接首页进入记录级验证的需求，不把复杂信息直接堆进表格。

### 打开方式

- 点击表格行
- 键盘 focus 后回车也可打开

### 内容结构

```text
UsageDetailsDrawer
├─ MetaSummary
├─ TokenBreakdown
├─ RoutingInfo
├─ RequestStatus
└─ CloseAction
```

### 第一版展示字段

- Timestamp
- Team
- Router
- Channel
- Model
- Input Tokens
- Output Tokens
- Total Tokens
- Latency
- Status

### 第二版可扩展

- Trace ID
- Request ID
- Raw payload preview
- Error detail

---

## 6. SecondaryInsightsSection

这是可选区，不是首页主干。

若业务仍坚持展示排行榜，建议：

- 放在 Usage Table 下方
- 默认折叠
- 标记为 `Secondary insights`

包含：

- Top Teams
- Top Models
- Top Channels

这样可以保留分析价值，但不打断首页主路径。

---

## 7. 数据与状态管理建议

### 7.1 页面状态分层

建议至少区分：

- `auth state`
- `global loading state`
- `kpi/trend request state`
- `table request state`
- `drawer selection state`

### 7.2 刷新策略

- KPI 和趋势图可按全局时间范围联动刷新
- 表格刷新不应打断用户当前分页和筛选
- Drawer 打开时，列表刷新不应强制关闭 drawer

### 7.3 URL 同步

建议同步这些参数：

- `range`
- `start_date`
- `end_date`
- `team_id`
- `router`
- `channel`
- `model`
- `page`

---

## 8. 响应式布局建议

### Desktop

- Header 与全局控制栏分两层
- KPI 四列
- 趋势图两列
- 筛选一行
- 表格全宽

### Tablet

- KPI 两列
- 趋势图单列堆叠
- 筛选分两行

### Mobile

首页不追求完整信息密度，优先保留：

- Header
- 时间范围
- KPI
- 一张趋势图
- 表格摘要

移动端可弱化：

- 二级分析区
- 宽字段表格

---

## 9. 组件拆分建议

基于当前单文件实现，建议逐步拆成：

- `dashboard-header.tsx`
- `global-control-bar.tsx`
- `alert-banner.tsx`
- `kpi-summary-section.tsx`
- `trend-section.tsx`
- `filter-section.tsx`
- `usage-table-section.tsx`
- `usage-details-drawer.tsx`

这样做的价值：

- 页面结构更清楚
- 状态边界更清楚
- 后续加热力图或二级分析区不会继续把 `dashboard-client.tsx` 堆大

---

## 10. 实现优先级

### P1

- 合并 Time Range 到 GlobalControlBar
- 重定义 KPI 卡片
- 重构趋势图区
- 强化 FilterSection
- 补齐 UsageTable 字段和状态

### P2

- 增加 AlertBanner
- 增加 UsageDetailsDrawer
- URL query 同步
- 自动刷新策略

### P3

- SecondaryInsightsSection
- Token heatmap
- 导出反馈增强

---

## 11. 实施建议

如果下一步进入开发，不建议直接在现有页面上零散补丁式添加功能。

更稳妥的顺序是：

1. 先按本说明重排模块层级
2. 再改 KPI 和趋势图口径
3. 再补筛选与表格交互
4. 最后接 drawer 和二级分析区

这样可以避免首页继续演变成“功能越来越多，但主路径越来越模糊”的状态。
