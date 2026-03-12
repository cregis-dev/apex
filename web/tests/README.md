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

## 测试结构

```
tests/
├── dashboard.spec.ts    # Dashboard 页面测试 (20 tests)
├── api.spec.ts         # API 测试 (5 tests, skip by default)
└── README.md          # 本文件
```

## 测试覆盖

### Dashboard 测试 (20 tests)

| 类别 | 测试数 | 覆盖内容 |
|------|--------|----------|
| 首页测试 | 2 | 登录表单、空提交 |
| 认证流程 | 2 | 无 auth 访问、存储 API Key |
| 页面测试 | 2 | 页面标题、auth 状态 |
| 组件测试 | 9 | 时间筛选、指标卡片、筛选器、表格、排行榜、图表、分页 |
| 错误处理 | 2 | 无效 API Key、断开连接 |
| 响应式测试 | 3 | 桌面、平板、手机 |

### API 测试 (5 tests, 默认跳过)

需要后端运行并设置 `RUN_API_TESTS=true`

## 配置

测试配置位于 `playwright.config.ts`:
- 测试目录: `./tests`
- 基础 URL: `http://localhost:3001`
- 浏览器: Chromium
- 报告器: HTML
- 超时: 30s

## CI 集成

在 CI 环境中运行:
```bash
CI=true npm test
```

## 注意事项

1. 测试需要 Next.js 开发服务器运行
2. Playwright 会自动启动开发服务器
3. 首次运行可能需要下载浏览器
4. API 测试需要后端服务运行，默认跳过
