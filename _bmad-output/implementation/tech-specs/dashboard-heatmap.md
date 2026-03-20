---
title: 'Dashboard 热力图组件'
slug: 'dashboard-heatmap'
created: '2026-03-10'
status: 'ready-for-dev'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['React 19', 'Recharts 2.15', 'TypeScript', 'Tailwind CSS 4', 'Next.js 16']
files_to_modify: ['web/src/app/dashboard/page.tsx', 'web/src/components/heatmap-chart.tsx (新建)']
code_patterns: ['shadcn/ui 组件模式', 'CSS Grid 热力图', 'Tailwind CSS 响应式']
test_patterns: ['Playwright E2E']
---

# Tech-Spec: Dashboard 热力图组件

**Created:** 2026-03-10

## Overview

### Problem Statement

用户希望在 Dashboard 中添加一个热力图组件，以可视化方式展示每日 input tokens 和 output tokens 的使用量分布，帮助快速识别使用高峰和低谷时段。

### Solution

在现有 Dashboard 页面中添加一个新的热力图组件，展示最近 30 天的每日 token 使用量。热力图使用颜色深浅表示使用量大小，支持 input/output 两种数据类型切换。

### Scope

**In Scope:**
- 创建热力图组件 (HeatmapChart)
- 集成到 Dashboard 页面
- 支持 Input/Output Tokens 切换
- 数据来源于现有的 `/api/metrics/trends` API

**Out of Scope:**
- 后端 API 新增 (复用现有 API)
- 其他可视化类型
- 导出功能

## Context for Development

### Codebase Patterns

| 模式 | 位置 | 说明 |
|------|------|------|
| Chart 组件 | `web/src/components/ui/chart.tsx` | 基于 Recharts 的封装 |
| Dashboard | `web/src/app/dashboard/page.tsx` | 主页面，使用现有 API |
| API 调用 | `fetchTrends()` | 获取 daily period 数据 |
| 工具函数 | `web/src/lib/utils.ts` | `cn()` 用于类名合并 |

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `web/src/components/ui/chart.tsx` | 参考现有 Chart 封装模式 |
| `web/src/app/dashboard/page.tsx` | 页面结构和数据获取 |
| `web/src/lib/utils.ts` | `cn()` 工具函数 |

### Technical Decisions

- **热力图实现**: 使用 CSS Grid + 颜色映射实现简单热力图 (避免 Recharts 热力图复杂性)
- **颜色方案**:
  - Input Tokens: 绿色系 `#22C55E` (浅) → `#15803D` (深)
  - Output Tokens: 紫色系 `#A855F7` (浅) → `#7E22CE` (深)
- **数据格式**: 复用 `TrendData[]` (已有 date, input_tokens, output_tokens 字段)
- **组件位置**: 在 `web/src/components/` 下创建 `heatmap-chart.tsx`

## Implementation Plan

### Tasks

- [ ] Task 1: 创建热力图组件 HeatmapChart
  - File: `web/src/components/heatmap-chart.tsx` (新建)
  - Action: 创建新组件，使用 CSS Grid 实现热力图
  - Notes:
    - Props: `data: TrendData[]`, `type: 'input' | 'output'`
    - 使用 7 列网格 (一周) x 5 行布局
    - 颜色使用 Tailwind: input 用 green-500 渐变, output 用 purple-500 渐变
    - 悬停显示具体数值 (title tooltip)

- [ ] Task 2: 在 Dashboard 页面集成热力图
  - File: `web/src/app/dashboard/page.tsx`
  - Action: 在 Trend Charts 区域添加热力图组件
  - Notes:
    - 在现有 LineChart/BarChart 下方或旁边添加热力图
    - 位置: Trend Charts section 内

- [ ] Task 3: 添加 Input/Output 切换按钮
  - File: `web/src/app/dashboard/page.tsx`
  - Action: 添加状态 `heatmapType` 和切换按钮
  - Notes:
    - 使用 Button 组件 (已有 shadcn/ui)
    - 状态: `heatmapType: 'input' | 'output' = 'input'`
    - 传递给 HeatmapChart 组件

### Acceptance Criteria

- [ ] AC 1: Given 用户打开 Dashboard, when 页面加载完成, then 热力图显示最近 30 天的数据
- [ ] AC 2: Given 用户点击 "Input" 按钮, when 切换数据源, then 热力图显示 input_tokens 数据并使用绿色系
- [ ] AC 3: Given 用户点击 "Output" 按钮, when 切换数据源, then 热力图显示 output_tokens 数据并使用紫色系
- [ ] AC 4: Given 热力图单元格, when 用户悬停, then 显示日期和具体 token 数量
- [ ] AC 5: Given 热力图数据, when 使用量高, then 颜色更深; 使用量低则颜色更浅
- [ ] AC 6: Given 移动设备, when 屏幕宽度 < 640px, then 热力图自动调整列数以适应屏幕

## Additional Context

### Dependencies

- React 19 (已有)
- Tailwind CSS 4 (已有)
- shadcn/ui Button 组件 (已有)
- 无需额外安装依赖

### Testing Strategy

- **手动测试**:
  1. 打开 Dashboard，确认热力图显示
  2. 点击 Input/Output 切换，确认颜色变化
  3. 悬停单元格，确认 tooltip 显示
  4. 调整浏览器宽度，确认响应式布局

- **Playwright E2E 测试**:
  - 在 `web/tests/dashboard.spec.ts` 添加热力图相关测试

### Notes

- 热力图数据来自 `fetchTrends()` API 返回的 `TrendData[]`
- 需要确保 API 返回足够的历史数据 (至少 28 天)
- 颜色映射算法: 将 token 数量归一化到 0-1 范围，然后映射到颜色梯度
- 网格布局: 每周一行 (7 列)，最多 5 行 (35 天)
