---
title: 'Dashboard Sankey Tooltip 与文字显示增强'
slug: 'dashboard-sankey-tooltip-and-label-enhancement'
created: '2026-03-13T22:51:56+0800'
status: 'completed'
stepsCompleted: [1, 2, 3, 4]
tech_stack:
  - 'Rust (Axum server serialization)'
  - 'Next.js 16'
  - 'React 19'
  - 'TypeScript 5'
  - 'Recharts 2.15 Sankey'
  - 'Playwright end-to-end tests'
files_to_modify:
  - '/Users/shawn/workspace/code/apex/src/server.rs'
  - '/Users/shawn/workspace/code/apex/web/src/components/dashboard/types.ts'
  - '/Users/shawn/workspace/code/apex/web/src/components/dashboard/overview-tab.tsx'
  - '/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts'
  - '/Users/shawn/workspace/code/apex/web/tests/dashboard.backend.spec.ts'
code_patterns:
  - 'Dashboard analytics are aggregated in Rust and serialized into a single response payload consumed by the web client.'
  - 'The overview tab uses inline custom Recharts renderers for Sankey nodes, links, and tooltip instead of separate chart helper files.'
  - 'Topology nodes and links are minimal typed structures; link metrics beyond requests must be added explicitly to both Rust and TypeScript models.'
  - 'UI styling is composed with Tailwind utility classes inside the dashboard component.'
test_patterns:
  - 'Dashboard UI behavior is covered with Playwright route-mocked tests in web/tests/dashboard.spec.ts.'
  - 'Backend-integrated dashboard smoke coverage lives in web/tests/dashboard.backend.spec.ts and focuses on visible text and tab flows.'
---

# Tech-Spec: Dashboard Sankey Tooltip 与文字显示增强

**Created:** 2026-03-13T22:51:56+0800

## Overview

### Problem Statement

当前 dashboard 的 `Traffic Topology` Sankey 图虽然已经具备基本 tooltip，但图内没有固定文字显示，导致用户无法快速识别节点名称和主要流向；当节点或链路名称较长时，现有展示方式不足以支撑高效阅读和排障。

### Solution

在现有 dashboard Sankey 组件上增加固定显示文字能力，优先在图内直接展示节点或链路相关文字；对于空间不足而无法完整显示的内容，通过 hover tooltip 展示完整信息。tooltip 必须包含完整名称或路径、`requests` 和 `total_tokens`。

### Scope

**In Scope:**
- `Traffic Topology` Sankey 图的节点与链路文字显示策略
- hover tooltip 信息增强，至少覆盖完整名称或完整路径、`requests`、`total_tokens`
- 必要的数据结构扩展或映射调整，以支撑前端展示完整 tooltip 内容
- dashboard 相关自动化测试更新

**Out of Scope:**
- 其它 dashboard 图表或卡片的交互改版
- dashboard 整体视觉重构
- 新增独立 topology 页面或新的筛选维度

## Context for Development

### Codebase Patterns

- dashboard 概览页当前集中实现在 `web/src/components/dashboard/overview-tab.tsx`
- Sankey 使用 `recharts` 的自定义 `node`、`link` 与 `Tooltip` 渲染模式
- topology 数据由 Rust 后端在 `src/server.rs` 聚合后返回，前端类型定义位于 `web/src/components/dashboard/types.ts`
- 当前后端已返回 `topology.nodes`、`topology.links`、`topology.flows`，说明功能增强优先沿用现有 API 结构而不是重做接口
- `TopologyNode` 与 `TopologyLink` 目前都只画 SVG 图形，不输出文字；固定文字需要在自定义 renderer 中补 `<text>` 或同级 SVG 文本节点
- 当前 `TopologyTooltip` 只能可靠读取 `source.name`、`target.name` 和 `value(requests)`，拿不到 `total_tokens`，因此链路数据结构必须扩展
- 当前前端测试只断言 `Traffic Topology` 卡片存在，没有覆盖 tooltip 内容或标签显示逻辑
- 未发现 `project-context.md`，本次实现遵循现有 dashboard 代码和测试约定即可

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `/Users/shawn/workspace/code/apex/web/src/components/dashboard/overview-tab.tsx` | 当前 Sankey 图、自定义节点、自定义链路、tooltip 的主要实现位置 |
| `/Users/shawn/workspace/code/apex/web/src/components/dashboard/types.ts` | dashboard analytics 返回类型定义，包括 topology nodes、links、flows |
| `/Users/shawn/workspace/code/apex/src/server.rs` | topology 聚合构建逻辑，决定前端可用的节点、链路和 flow 字段 |
| `/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts` | dashboard 前端行为测试基线 |
| `/Users/shawn/workspace/code/apex/web/tests/dashboard.backend.spec.ts` | dashboard 后端联通与页面展示测试基线 |
| `/Users/shawn/workspace/code/apex/web/package.json` | 确认 web 技术栈与测试工具版本，包括 Next.js、React、Recharts、Playwright |

### Technical Decisions

- 保持当前 `Traffic Topology` 卡片、筛选行为和 `recharts` Sankey 技术选型不变
- 文字展示遵循“能固定显示则固定显示，显示不下则 hover 时完整显示”的原则
- tooltip 为本次需求的必需项，且必须展示 `total_tokens`
- 已确认需要扩展后端返回字段：`DashboardTopologyLink` 当前只有 `value`，若 tooltip 要稳定显示链路级 `total_tokens`，必须让 links 直接携带 `total_tokens` 或等价可映射字段
- 固定文字优先落在自定义 `TopologyNode` 和 `TopologyLink` renderer 中实现，而不是在 Sankey 外层额外叠加 HTML 层，以降低坐标同步复杂度
- 长文本不做强制压缩填充；渲染层应根据可用宽高决定是否显示完整/截断文本，完整内容统一由 hover tooltip 承担
- 测试优先补 Playwright 路由 mock 场景，验证 topology 可见文本与 tooltip 文案；后端联通测试只保留页面级冒烟断言，避免依赖过细的真实数据形状

## Implementation Plan

### Tasks

- [x] Task 1: 扩展 topology link 数据结构以支持 tooltip 显示 `total_tokens`
  - File: `/Users/shawn/workspace/code/apex/src/server.rs`
  - Action: 为 `DashboardTopologyLink` 增加链路级 `total_tokens` 字段，并在 `build_topology_section` 中按 `(source, target)` 聚合 `requests` 与 `total_tokens`。
  - Notes: 现有 `value` 继续保留并表示 `requests`，避免破坏 Recharts Sankey 以 `value` 控制链路宽度的现有行为；链路排序和 `render_mode` 行为保持不变。

- [x] Task 2: 同步前端 topology 类型定义
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/types.ts`
  - Action: 为前端 `DashboardTopologyLink` 增加 `total_tokens` 字段，并确保 `DashboardAnalyticsResponse` 的 topology 类型与后端序列化结构一致。
  - Notes: 不新增与当前需求无关的字段；若需要节点级聚合指标，优先在前端根据 `flows` 推导，避免扩大 API 面。

- [x] Task 3: 在 overview tab 中补充 topology 文本和 tooltip 数据映射
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/overview-tab.tsx`
  - Action: 新增 topology helper，用 `analytics.topology.flows` 生成节点级 `requests`/`total_tokens` 聚合映射，并让自定义 `TopologyTooltip` 同时支持 node/link 两类 payload。
  - Notes: node tooltip 至少显示节点名、节点类型、`requests`、`total_tokens`；link tooltip 至少显示 `source → target`、`requests`、`total_tokens`。数字格式保持 dashboard 当前风格，`requests` 用千分位，`total_tokens` 可复用 compact 或千分位格式，但同一 tooltip 内必须一致。

- [x] Task 4: 为 Sankey 节点和链路增加固定文字显示策略
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/overview-tab.tsx`
  - Action: 在 `TopologyNode` renderer 中添加节点名称常驻文本；在 `TopologyLink` renderer 中为可容纳的主要链路添加常驻文字，优先显示链路名称或数值摘要。
  - Notes: 文本策略遵循“能显示则固定显示、显示不下则不强行挤压并依赖 hover tooltip”。实现时使用 SVG `<text>` 与现有 Sankey 坐标系对齐，避免额外 HTML overlay。链路文字渲染必须加可用空间判断，避免大面积重叠或越界。

- [x] Task 5: 调整 tooltip 与文本显示的交互细节
  - File: `/Users/shawn/workspace/code/apex/web/src/components/dashboard/overview-tab.tsx`
  - Action: 让固定文本被隐藏或截断的节点/链路在 hover 时仍能显示完整 tooltip；保留现有 `Tooltip` 容器挂载方式，不改变 card 布局和空状态文案。
  - Notes: tooltip 内容需要以“完整内容”为准，不能复用截断后的字符串；若链路固定文字只显示摘要，则 tooltip 仍需展示完整 `source → target` 与完整数值。

- [x] Task 6: 更新 dashboard 自动化测试夹具和断言
  - File: `/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts`
  - Action: 将 mocked analytics topology 从空 `nodes/links` 改为可渲染 Sankey 的数据，补充至少一条高流量链路和一条较小链路，并新增对固定文字与 hover tooltip 内容的断言。
  - Notes: 测试需覆盖 `requests` 与 `total_tokens` 同时出现；hover 断言应针对实际可交互的 SVG 文本或图形节点，避免仅断言标题存在。

- [x] Task 7: 评估并按需补充真实后端冒烟覆盖
  - File: `/Users/shawn/workspace/code/apex/web/tests/dashboard.backend.spec.ts`
  - Action: 仅在真实后端测试稳定可控的前提下，补充一条对 topology 可见文本的宽松断言；否则保持现有页面级 smoke test，不绑定具体 tooltip 内容。
  - Notes: 真实数据波动较大时，不应将该测试设计为依赖特定 `total_tokens` 文案，以免引入脆弱性。

### Acceptance Criteria

- [x] AC 1: Given dashboard analytics 返回包含 topology links，when overview tab 渲染 `Traffic Topology`，then 每条 link payload 都包含 `value=requests` 与 `total_tokens` 两个可供 tooltip 使用的字段。
- [x] AC 2: Given topology 中存在可见节点，when Sankey 图渲染完成，then 节点名称在可用空间足够时固定显示在图内且不依赖 hover 才能读取。
- [x] AC 3: Given topology 中存在主要链路，when 链路可用空间足够承载文本，then 链路上固定显示文字摘要；when 空间不足，then 不强行显示导致重叠，而由 hover tooltip 承担完整信息展示。
- [x] AC 4: Given 用户 hover 任一 topology 节点，when tooltip 打开，then tooltip 显示节点完整名称、节点类型、`requests` 和 `total_tokens`。
- [x] AC 5: Given 用户 hover 任一 topology 链路，when tooltip 打开，then tooltip 显示完整 `source → target` 路径、`requests` 和 `total_tokens`。
- [x] AC 6: Given 节点或链路名称过长而无法在图内完整显示，when 用户 hover 对应元素，then tooltip 仍展示未截断的完整文本与完整数值。
- [x] AC 7: Given 当前筛选条件下没有 topology 数据，when overview tab 渲染，then 仍显示现有空状态文案且不引入报错或空 tooltip。
- [x] AC 8: Given dashboard route-mocked Playwright 测试运行，when topology mock 数据包含 Sankey 节点和链路，then 测试能够验证固定文字与 tooltip 中 `requests`、`total_tokens` 的存在。
- [x] AC 9: Given dashboard 其它卡片与 tab 已存在，when 本需求完成，then `Traffic Topology` 之外的 dashboard 布局、筛选参数和 tab 切换行为保持不变。

## Additional Context

### Dependencies

- `recharts` Sankey 自定义渲染能力
- 现有 dashboard analytics API / Rust topology 聚合返回结构
- `web/package.json` 中的 `recharts@^2.15.4`、`next@16.1.6`、`react@19.2.3`、`typescript@^5`
- topology tooltip 数值依赖 `src/server.rs` 输出的链路级 `total_tokens`

### Testing Strategy

- 在 `web/tests/dashboard.spec.ts` 中补 mocked analytics topology 数据，覆盖固定文字渲染和 hover tooltip 中的 `requests`、`total_tokens`
- 若 topology mock 数据结构扩展，保持测试夹具与 TypeScript 类型同步，避免接口漂移
- 在 `web/tests/dashboard.backend.spec.ts` 中维持页面级存在性验证，不将真实后端测试绑定到具体 tooltip 文案
- 手动验证步骤至少包括：进入 `/dashboard?tab=overview`、观察主要节点/链路固定文字、hover 长名称元素确认 tooltip 显示完整名称与 `total_tokens`

### Notes

- 用户已确认：文字尽量固定显示；显示不下的内容通过 hover tooltip 展示。
- 用户已确认：tooltip 必须显示 `total_tokens`。
- 深入调查结论：本需求不是纯前端样式调整，至少涉及 Rust topology link 序列化结构和前端类型同步。
- 高风险项：Sankey 自定义 link renderer 的文字可读性高度依赖曲线宽度和位置，若直接在所有链路上绘制文字，极易产生重叠，因此必须把“可用空间判断”作为实现约束而不是可选优化。
- 已知限制：Recharts Sankey 的自定义 renderer payload 结构不如普通图表稳定，实施时应先以 route-mocked 数据验证 node/link tooltip 的 payload 形状，再收敛到最终实现。
- 后续考虑：如果将来需要更复杂的 topology 标注策略，可再抽离独立 `topology-chart` 组件；本次 quick spec 保持在 `overview-tab.tsx` 内完成。

## Review Notes

- Adversarial review completed
- Findings: 12 total, 10 addressed, 2 skipped
- Resolution approach: auto-fix
- Skipped findings:
  - 与本次需求无关、且涉及预先存在测试基础设施改动的后端测试数据库整理项
  - 关于 hover state 重渲染成本的低置信度性能担忧，当前数据规模与测试结果下未发现实际问题
