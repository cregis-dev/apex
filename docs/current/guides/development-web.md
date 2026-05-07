# Apex Gateway - Web Dashboard Development Guide

**Generated:** 2026-03-10
**Scope:** Next.js 前端开发环境设置和工作流

---

## 环境要求

### 必需工具

| 工具 | 版本 | 说明 |
|------|------|------|
| Node.js | 18+ | JavaScript 运行时 |
| npm/pnpm/bun | - | 包管理器 |
| Git | - | 版本控制 |

### 可选工具

| 工具 | 说明 |
|------|------|
| VS Code | 推荐编辑器 |
| ESLint | 代码检查 |
| Prettier | 代码格式化 |

---

## 快速开始

### 1. 安装依赖

```bash
cd web
npm install
# 或
pnpm install
# 或
bun install
```

### 2. 启动开发服务器

```bash
# 开发模式 (热重载)
npm run dev

# 访问 http://localhost:3000
```

### 3. 构建生产版本

```bash
# 构建到 target/web 目录
npm run build

# 查看构建产物
ls -la ../target/web/
```

### 4. 本地预览生产构建

```bash
# 启动本地服务器预览
npm run start
```

---

## 开发工作流

### 1. 启动 Apex Gateway

在启动前端之前，确保后端服务正在运行：

```bash
# 在另一个终端启动后端
cd ..
cargo run -- --config config.json
```

### 2. 配置 API 代理

开发模式下，Next.js 需要代理到后端 API：

```typescript
// next.config.ts 中的代理配置
const nextConfig = {
  async rewrites() {
    return [
      {
        source: '/api/:path*',
        destination: 'http://localhost:12356/api/:path*',
      },
    ];
  },
};
```

### 3. 设置 API Key

首次访问 Dashboard 时，需要输入 API Key：

1. 打开 http://localhost:3000/dashboard
2. 在弹窗中输入 API Key (从 `config.json` 中的 `teams` 获取)
3. API Key 会存储在 `localStorage`

---

## 项目结构

```
web/
├── src/
│   ├── app/                    # App Router 页面
│   │   ├── layout.tsx          # 根布局
│   │   ├── page.tsx            # 首页
│   │   ├── globals.css         # 全局样式
│   │   └── dashboard/
│   │       ├── layout.tsx      # Dashboard 布局
│   │       ├── page.tsx        # Dashboard 主页
│   │       └── loading.tsx     # 加载状态
│   ├── components/
│   │   ├── ui/                 # shadcn/ui 组件
│   │   ├── dashboard/          # 业务组件
│   │   └── providers/          # Context Providers
│   ├── hooks/                  # Custom Hooks
│   │   ├── use-api.ts
│   │   └── use-dashboard.ts
│   └── lib/                    # 工具函数
│       ├── utils.ts
│       └── types.ts
├── public/                     # 静态资源
├── tests/                      # Playwright 测试
├── package.json                # 依赖配置
├── next.config.ts              # Next.js 配置
├── tsconfig.json               # TypeScript 配置
├── tailwind.config.ts          # Tailwind 配置
└── playwright.config.ts        # Playwright 配置
```

---

## 添加新组件

### 1. 使用 shadcn/ui 添加组件

```bash
# 添加 Button 组件
npx shadcn@latest add button

# 添加 Card 组件
npx shadcn@latest add card

# 添加 Table 组件
npx shadcn@latest add table
```

### 2. 创建业务组件

创建 `src/components/dashboard/my-component.tsx`:

```tsx
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';

interface MyComponentProps {
  title: string;
  data: any[];
}

export function MyComponent({ title, data }: MyComponentProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{title}</CardTitle>
      </CardHeader>
      <CardContent>
        {/* 组件内容 */}
      </CardContent>
    </Card>
  );
}
```

### 3. 在页面中使用

```tsx
// app/dashboard/page.tsx
import { MyComponent } from '@/components/dashboard/my-component';

export default function DashboardPage() {
  return (
    <div>
      <MyComponent title="我的组件" data={[]} />
    </div>
  );
}
```

---

## 添加新页面

### 1. 创建页面目录

```bash
mkdir -p src/app/reports
```

### 2. 创建 page.tsx

```tsx
// src/app/reports/page.tsx
export default function ReportsPage() {
  return (
    <div>
      <h1>报表页面</h1>
    </div>
  );
}
```

### 3. 添加布局 (可选)

```tsx
// src/app/reports/layout.tsx
export default function ReportsLayout({ children }) {
  return <div className="reports-layout">{children}</div>;
}
```

---

## 调用 API

### 使用 useApi Hook

```tsx
'use client';

import { useApi } from '@/hooks/use-api';

export function MyComponent() {
  const { fetchApi } = useApi();

  const handleClick = async () => {
    const data = await fetchApi('/metrics');
    console.log(data);
  };

  return <button onClick={handleClick}>获取数据</button>;
}
```

### 使用 React Query (推荐)

```bash
npm install @tanstack/react-query
```

```tsx
'use client';

import { useQuery } from '@tanstack/react-query';
import { useApi } from '@/hooks/use-api';

export function MetricsCards() {
  const { fetchApi } = useApi();

  const { data, isLoading, error } = useQuery({
    queryKey: ['metrics'],
    queryFn: () => fetchApi('/metrics'),
    staleTime: 30000, // 30 秒
  });

  if (isLoading) return <div>加载中...</div>;
  if (error) return <div>错误：{error.message}</div>;

  return <div>{data.total_requests}</div>;
}
```

---

## 样式开发

### Tailwind CSS 使用

```tsx
export function MyComponent() {
  return (
    <div className="flex items-center justify-between p-4 bg-white rounded-lg shadow-md">
      <h2 className="text-lg font-semibold text-gray-900">标题</h2>
      <button className="px-4 py-2 text-white bg-blue-600 rounded hover:bg-blue-700">
        按钮
      </button>
    </div>
  );
}
```

### 使用 cn 工具函数

```tsx
import { cn } from '@/lib/utils';

export function MyComponent({ className, ...props }) {
  return (
    <div
      className={cn(
        'flex items-center p-4 bg-white rounded-lg',
        className
      )}
      {...props}
    />
  );
}
```

---

## 测试

### 运行 Playwright 测试

```bash
# 运行所有测试
npm test

# 运行特定测试
npm test -- dashboard

# UI 模式运行测试
npm run test:ui

# 有头模式 (显示浏览器)
npm run test:headed
```

### 编写测试

```tsx
// tests/dashboard.spec.ts
import { test, expect } from '@playwright/test';

test.describe('Dashboard', () => {
  test('should display metrics', async ({ page }) => {
    await page.goto('/dashboard');

    // 等待数据加载
    await page.waitForSelector('[data-testid=metrics-cards]');

    // 断言指标卡片存在
    await expect(page.locator('text=总请求数')).toBeVisible();
    await expect(page.locator('text=总错误数')).toBeVisible();
  });
});
```

---

## 调试技巧

### 1. React DevTools

安装 [React Developer Tools](https://react.dev/learn/react-developer-tools) 浏览器扩展。

### 2. 调试 Hook

```tsx
import { useEffect } from 'react';

export function MyComponent() {
  const { data, loading } = useDashboard();

  useEffect(() => {
    console.log('Dashboard data changed:', data);
  }, [data]);

  return <div>...</div>;
}
```

### 3. Network 调试

打开浏览器 DevTools 的 Network 面板，查看 API 请求。

---

## 常见问题

### Q: 开发模式下 API 请求失败？

A: 确保配置了正确的代理目标：
```typescript
// next.config.ts
async rewrites() {
  return [
    {
      source: '/api/:path*',
      destination: 'http://localhost:12356/api/:path*',
    },
  ];
}
```

### Q: 构建后样式丢失？

A: 确保 Tailwind 配置正确：
```typescript
// tailwind.config.ts
export default {
  content: ['./src/**/*.{ts,tsx}'],
  // ...
};
```

### Q: 如何重置 localStorage 中的 API Key？

A: 在浏览器控制台运行：
```javascript
localStorage.removeItem('apex-api-key');
location.reload();
```

### Q: 如何处理跨域问题？

A: 在后端配置 CORS：
```json
{
  "global": {
    "cors_allowed_origins": ["http://localhost:3000"]
  }
}
```

---

## 构建和部署

### 开发构建

```bash
npm run build
# 输出到 ../target/web/
```

### 生产部署

```bash
# 1. 构建前端
cd web
npm install
npm run build

# 2. 构建内嵌 Dashboard 资源的二进制
cd ..
cargo build --release --features embedded-web

# 3. 启动服务
./target/release/apex gateway start --config config.json
```

### 使用 install.sh

```bash
# 一键安装
./install.sh /opt/apex
```

---

## 代码规范

### ESLint 配置

```bash
# 运行检查
npm run lint

# 自动修复
npm run lint -- --fix
```

### Prettier 配置

```bash
# 格式化代码
npx prettier --write src/
```

### Git Hooks (可选)

```bash
# 安装 Husky
npm install -D husky
npx husky install

# 添加 pre-commit hook
npx husky add .husky/pre-commit "npm run lint && npm run build"
```

---

## 性能优化

### 1. 代码分割

Next.js App Router 自动进行代码分割。

### 2. 图片优化

```tsx
import Image from 'next/image';

<Image
  src="/logo.png"
  alt="Logo"
  width={100}
  height={100}
  priority
/>
```

### 3. 字体优化

```tsx
import { Inter } from 'next/font/google';

const inter = Inter({ subsets: ['latin'] });

export default function RootLayout({ children }) {
  return (
    <html className={inter.className}>
      <body>{children}</body>
    </html>
  );
}
```

### 4. 图表懒加载

```tsx
import dynamic from 'next/dynamic';

const TrendChart = dynamic(() => import('./trend-chart'), {
  ssr: false,
  loading: () => <Skeleton />
});
```

---

_Generated using BMAD Method `document-project` workflow_
