---
name: "Apex Gateway 测试架构设计"
version: "1.0"
date: "2026-03-09"
mode: "system-level"
---

# Apex Gateway 系统级测试架构

## 1. 项目概述

**项目**: Apex Gateway - AI API 网关
**类型**: 全栈应用 (Rust 后端 + Next.js 前端)
**核心功能**: 多提供商路由、负载均衡、认证鉴权、MCP 协议支持、Web Dashboard

## 2. 当前测试状态

### 2.1 后端测试 (Rust)

| 测试类型 | 位置 | 框架 | 状态 |
|----------|------|------|------|
| 单元测试 | 各模块 | 内置 `#[test]` | ✅ 活跃 |
| 集成测试 | `tests/*.rs` | 内置 | ✅ 活跃 |
| E2E 测试 | `tests/e2e/` | Python pytest | ✅ 活跃 |

### 2.2 前端测试 (Next.js)

| 测试类型 | 位置 | 框架 | 状态 |
|----------|------|------|------|
| 单元测试 | - | - | ❌ 未配置 |
| 组件测试 | - | - | ❌ 未配置 |
| E2E 测试 | - | Playwright | ❌ 未配置 |

**关键发现**: 前端项目 `web/` 缺少测试框架，需要初始化。

## 3. 测试架构设计

### 3.1 测试金字塔

```
           ┌─────────────┐
           │   E2E Tests │  ← Playwright (前端)
           ├─────────────┤
           │  API Tests  │  ← Python pytest (后端 E2E)
           ├─────────────┤
           │ Integration │  ← Rust 集成测试
           ├─────────────┤
           │ Unit Tests  │  ← Rust 单元测试
           └─────────────┘
```

### 3.2 推荐的测试策略

#### 后端 (保持现有)
- **单元测试**: Rust 内置测试 - 覆盖核心逻辑
- **集成测试**: Rust 集成测试 - 覆盖 API 端点
- **API E2E**: Python pytest - 覆盖完整请求链路

#### 前端 (新增 Playwright)
- **E2E 测试**: Playwright - 覆盖关键用户流程
- **组件测试**: 可选 (Vitest + Testing Library)

### 3.3 前端测试优先级

| 优先级 | 场景 | 原因 |
|--------|------|------|
| P0 | Dashboard 页面加载 | 核心功能 |
| P0 | API Key 认证流程 | 安全关键 |
| P1 | 数据筛选功能 | 常用功能 |
| P1 | 趋势图表渲染 | 核心展示 |
| P2 | 响应式布局 | 辅助功能 |

## 4. 实施计划

### Phase 1: 初始化 Playwright (当前)
- [ ] 安装 Playwright 依赖
- [ ] 配置 playwright.config.ts
- [ ] 创建基础测试结构

### Phase 2: 编写 E2E 测试
- [ ] Dashboard 页面加载测试
- [ ] 认证流程测试
- [ ] 数据展示测试

### Phase 3: CI 集成
- [ ] 添加 GitHub Actions 工作流
- [ ] 配置测试报告

## 5. 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 前端无测试覆盖 | 高 | 优先初始化 Playwright |
| E2E 测试不稳定 | 中 | 使用 Playwright 可靠等待 |
| 测试数据准备 | 中 | 使用 fixtures/data factories |

## 6. 实施的测试框架配置

### 前端 (Playwright) - ✅ 已完成
```json
{
  "devDependencies": {
    "@playwright/test": "^1.58.2",
    "playwright": "^1.58.2"
  }
}
```

### 测试文件
- `web/tests/dashboard.spec.ts` - 前端 E2E 测试
- `web/tests/api.spec.ts` - API 集成测试

### 运行环境
- Node.js: 与 web/package.json 兼容
- Browser: Chromium (已安装)
- CI: GitHub Actions (配置就绪)

## 7. 执行测试

```bash
cd web

# 安装 Playwright 浏览器 (已完成)
npx playwright install chromium

# 运行所有测试
npm test

# UI 模式
npm run test:ui

# 查看报告
npm run test:report
```

---

## 8. CI/CD 集成

### GitHub Actions 工作流
已创建 `.github/workflows/frontend-tests.yml`

```yaml
on:
  push:
    branches: [main, dev]
  pull_request:
```

### 运行测试

```bash
# 本地运行
cd web && npm test

# 带 API 测试 (需要后端运行)
RUN_API_TESTS=true npm test
```

---

**状态**: ✅ Playwright 测试框架已初始化，前端 E2E 测试已创建，CI 工作流已配置
