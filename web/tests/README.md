# Apex Gateway 前端测试

## 测试框架

- **Playwright** - E2E 测试框架
- **@playwright/test** - 测试运行器

## 运行测试

```bash
# 安装依赖后运行测试
npm test

# UI 模式 (可视化)
npm run test:ui

# 有头模式 (显示浏览器)
npm run test:headed

# 查看测试报告
npm run test:report
```

真实后端 Dashboard smoke:

```bash
../scripts/dashboard/run_real_backend_smoke.sh
```

仅生成 fixture config + seed SQLite：

```bash
../scripts/dashboard/setup_real_backend_fixture.sh
```

## 测试结构

```
tests/
├── dashboard.spec.ts            # Dashboard mock API 回归
├── dashboard.backend.spec.ts    # Dashboard 真实后端联调 smoke
├── api.spec.ts                  # API 测试 (skip by default)
└── README.md                    # 本文件
```

## 测试覆盖

### Dashboard 测试

| 类别 | 测试数 | 覆盖内容 |
|------|--------|----------|
| Mock UI 回归 | 10 | 认证、URL 状态、tabs、records 抽屉、刷新、导出 |
| 真实后端 smoke | 2 | SQLite seeded 数据联调、真实 `/api/dashboard/*` 与页面渲染 |

### API 测试 (5 tests, 默认跳过)

需要后端运行并设置 `RUN_API_TESTS=true`

## 配置

测试配置位于 `playwright.config.ts`:
- 测试目录: `./tests`
- 基础 URL: `http://localhost:3001`
- 浏览器: Chromium
- 报告器: HTML
- 超时: 30s

真实后端联调使用 `playwright.real.config.ts`，该配置会禁用内置 `webServer`，直接连接已启动的 Apex 后端。

## CI 集成

在 CI 环境中运行:
```bash
CI=true npm test
```

## 注意事项

1. 默认 `dashboard.spec.ts` 仍使用 mock API 回归
2. `dashboard.backend.spec.ts` 需要先启动真实 Apex 后端并准备 seeded SQLite 数据
3. 默认配置会自动启动静态开发服务器；真实后端联调请使用 `playwright.real.config.ts`
4. 首次运行可能需要下载浏览器
5. API 测试需要后端服务运行，默认跳过
