# Apex Gateway - Web Dashboard Architecture

**Generated:** 2026-03-10
**Scope:** Next.js Web Dashboard 架构设计

## 架构概览

Apex Web Dashboard 是一个使用 Next.js 16 构建的单页应用，用于展示 Apex Gateway 的使用记录和性能指标。

```
┌─────────────────────────────────────────────────────────────┐
│                    Browser Client                           │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Next.js 16 (App Router)                 │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌───────────┐ │   │
│  │  │   Pages      │  │  Components  │  │   API     │ │   │
│  │  │  (Routes)    │  │   (shadcn)   │  │  Client   │ │   │
│  │  └──────────────┘  └──────────────┘  └───────────┘ │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              │
                              │ HTTP (Fetch)
                              ▼
┌─────────────────────────────────────────────────────────────┐
│              Apex Gateway Backend (Rust)                    │
│  ┌──────────────┐  ┌──────────────┐  ┌─────────────────┐  │
│  │  /api/usage  │  │ /api/metrics │  │  /dashboard/*   │  │
│  │              │  │              │  │  (Static Files) │  │
│  └──────────────┘  └──────────────┘  └─────────────────┘  │
│  ┌───────────────────────────────────────────────────────┐ │
│  │                  SQLite Database                       │ │
│  └───────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## 技术栈

| 层级 | 技术 | 版本 |
|------|------|------|
| **框架** | Next.js | 16.1.6 |
| **语言** | TypeScript | 5.x |
| **UI 库** | React | 19.2.3 |
| **组件库** | shadcn/ui | 4.0.0 |
| **样式** | Tailwind CSS | 4.x |
| **图表** | Recharts | 2.15.4 |
| **测试** | Playwright | 1.58.2 |
| **日期处理** | date-fns | 4.1.0 |

---

## 目录结构

```
web/
├── src/
│   ├── app/                    # App Router 页面
│   │   ├── layout.tsx          # 根布局
│   │   ├── page.tsx            # 首页
│   │   ├── globals.css         # 全局样式
│   │   └── dashboard/          # Dashboard 页面
│   │       ├── layout.tsx      # Dashboard 布局
│   │       ├── page.tsx        # Dashboard 主页
│   │       └── loading.tsx     # 加载状态
│   └── components/
│       ├── ui/                 # shadcn/ui 基础组件
│       │   ├── button.tsx
│       │   ├── card.tsx
│       │   ├── table.tsx
│       │   ├── input.tsx
│       │   ├── select.tsx
│       │   └── ...
│       ├── dashboard/          # Dashboard 业务组件
│       │   ├── metrics-cards.tsx    # 指标卡片
│       │   ├── usage-table.tsx      # Usage 表格
│       │   ├── trend-chart.tsx      # 趋势图表
│       │   ├── ranking-list.tsx     # 排行榜
│       │   └── time-filter.tsx      # 时间筛选器
│       └── providers/          # Context Providers
│           └── api-client.tsx  # API 客户端 Provider
├── public/                     # 静态资源
├── tests/                      # Playwright 测试
├── target/web/                 # 构建输出 (由 backend 服务)
├── package.json                # 依赖配置
├── next.config.ts              # Next.js 配置
├── tsconfig.json               # TypeScript 配置
├── tailwind.config.ts          # Tailwind 配置
└── playwright.config.ts        # Playwright 配置
```

---

## 核心组件

### 1. Dashboard 主页面 (`app/dashboard/page.tsx`)

**职责**: 组合所有 Dashboard 子组件，管理全局状态

**状态管理**:
```typescript
const [timeRange, setTimeRange] = useState<'today' | 'week' | 'month' | 'custom'>('today');
const [filters, setFilters] = useState({
  teamId: '',
  router: '',
  channel: '',
  model: '',
});
const [metrics, setMetrics] = useState<MetricsSummary | null>(null);
const [usage, setUsage] = useState<UsageRecord[]>([]);
const [trends, setTrends] = useState<TrendData[]>([]);
const [rankings, setRankings] = useState<RankingData[]>([]);
```

**布局结构**:
```tsx
<div className="dashboard">
  <Header />
  <TimeFilter value={timeRange} onChange={setTimeRange} />
  <FilterBar filters={filters} onChange={setFilters} />

  <MetricsCards data={metrics} />

  <div className="charts-grid">
    <TrendChart data={trends} />
    <RankingList data={rankings} />
  </div>

  <UsageTable data={usage} />
  <Pagination />
</div>
```

### 2. Metrics Cards 组件 (`components/dashboard/metrics-cards.tsx`)

**职责**: 展示关键指标汇总

**指标卡片**:
| 卡片 | 数据源 | 格式 |
|------|--------|------|
| 总请求数 | `metrics.total_requests` | 数字 (千分位) |
| 总错误数 | `metrics.total_errors` | 数字 + 错误率% |
| Fallback 次数 | `metrics.total_fallbacks` | 数字 + 占比% |
| 平均延迟 | `metrics.avg_latency_ms` | 毫秒 (带趋势) |
| Token 消耗 | `metrics.total_input_tokens/output_tokens` | K/M 格式化 |

**设计**:
```tsx
<Card>
  <CardHeader>
    <CardTitle>总请求数</CardTitle>
  </CardHeader>
  <CardContent>
    <div className="metric-value">{formatNumber(metrics.total_requests)}</div>
    <div className="metric-trend">↑ 12% 较上周</div>
  </CardContent>
</Card>
```

### 3. Usage Table 组件 (`components/dashboard/usage-table.tsx`)

**职责**: 展示 Usage 记录表格

**列定义**:
| 列名 | 字段 | 格式化 |
|------|------|--------|
| 时间 | `timestamp` | 日期时间格式 |
| 团队 | `team_id` | 文本 |
| 路由 | `router` | 文本 |
| 渠道 | `channel` | 文本 |
| 模型 | `model` | 文本 |
| 输入 Token | `input_tokens` | 数字 (千分位) |
| 输出 Token | `output_tokens` | 数字 (千分位) |
| 总 Token | `input + output` | 数字 (千分位) |

**功能**:
- 分页 (服务端)
- 排序 (点击列头)
- 行高亮 (悬停)

### 4. Trend Chart 组件 (`components/dashboard/trend-chart.tsx`)

**职责**: 展示时间趋势图表

**使用 Recharts**:
```tsx
<LineChart data={trends}>
  <XAxis dataKey="date" />
  <YAxis />
  <Tooltip />
  <Legend />
  <Line dataKey="requests" stroke="#8884d8" />
  <Line dataKey="input_tokens" stroke="#82ca9d" />
</LineChart>
```

**图表类型**:
- 请求量趋势 (折线图)
- Token 消耗趋势 (面积图)
- 延迟趋势 (折线图)

### 5. Ranking List 组件 (`components/dashboard/ranking-list.tsx`)

**职责**: 展示排行榜 (Team/Model/Channel)

**Tab 切换**:
```tsx
<Tabs defaultValue="team">
  <TabsList>
    <TabsTrigger value="team">Team 排行</TabsTrigger>
    <TabsTrigger value="model">Model 排行</TabsTrigger>
    <TabsTrigger value="channel">Channel 排行</TabsTrigger>
  </TabsList>
  <TabsContent value="team">
    <RankingList data={teamRankings} />
  </TabsContent>
</Tabs>
```

**单项设计**:
```tsx
<div className="ranking-item">
  <div className="ranking-name">{item.name}</div>
  <div className="ranking-bar" style={{ width: `${item.percentage}%` }} />
  <div className="ranking-value">{formatNumber(item.requests)}</div>
</div>
```

### 6. Time Filter 组件 (`components/dashboard/time-filter.tsx`)

**职责**: 时间范围选择器

**预设选项**:
- 今日 (Today)
- 本周 (This Week)
- 本月 (This Month)
- 自定义 (Custom)

**自定义日期范围**:
使用 `react-day-picker` 组件：
```tsx
<Popover>
  <PopoverTrigger asChild>
    <Button>选择日期范围</Button>
  </PopoverTrigger>
  <PopoverContent>
    <DayPicker mode="range" selected={dateRange} onSelect={setDateRange} />
  </PopoverContent>
</Popover>
```

---

## API 客户端

### API Client Hook (`hooks/use-api.ts`)

```typescript
export function useApi() {
  const [apiKey, setApiKey] = useState<string>(() => {
    return localStorage.getItem('apex-api-key') || '';
  });

  const fetchApi = useCallback(async <T>(endpoint: string, options?: RequestInit): Promise<T> => {
    const response = await fetch(`/api${endpoint}`, {
      ...options,
      headers: {
        ...options?.headers,
        'Authorization': `Bearer ${apiKey}`,
      },
    });

    if (!response.ok) {
      throw new Error(`API Error: ${response.status}`);
    }

    return response.json();
  }, [apiKey]);

  return {
    apiKey,
    setApiKey,
    fetchApi,
    getUsage: (filters: UsageFilters) => fetchApi<UsageResponse>('/usage', { /* ... */ }),
    getMetrics: () => fetchApi<MetricsSummary>('/metrics'),
    getTrends: (period: string) => fetchApi<TrendResponse>('/metrics/trends', { /* ... */ }),
    getRankings: (by: string) => fetchApi<RankingResponse>('/metrics/rankings', { /* ... */ }),
  };
}
```

---

## 状态管理

### 使用 React Context

```typescript
const DashboardContext = createContext<DashboardState | null>(null);

function DashboardProvider({ children }: { children: React.ReactNode }) {
  const [timeRange, setTimeRange] = useState<TimeRange>('today');
  const [filters, setFilters] = useState<Filters>({});
  const [data, setData] = useState<DashboardData>({});

  // 数据获取逻辑
  useEffect(() => {
    const fetchData = async () => {
      const [metrics, usage, trends, rankings] = await Promise.all([
        api.getMetrics(),
        api.getUsage(filters),
        api.getTrends(timeRange),
        api.getRankings('team'),
      ]);
      setData({ metrics, usage, trends, rankings });
    };
    fetchData();
  }, [timeRange, filters]);

  return (
    <DashboardContext.Provider value={{ timeRange, setTimeRange, filters, setFilters, data }}>
      {children}
    </DashboardContext.Provider>
  );
}
```

---

## 样式系统

### Tailwind CSS 配置

```typescript
// tailwind.config.ts
export default {
  content: ['./src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        border: 'hsl(var(--border))',
        input: 'hsl(var(--input))',
        ring: 'hsl(var(--ring))',
        background: 'hsl(var(--background))',
        foreground: 'hsl(var(--foreground))',
        primary: {
          DEFAULT: 'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))',
        },
        // ... shadcn/ui 主题色
      },
    },
  },
  plugins: [],
}
```

### CSS 变量主题

```css
/* src/app/globals.css */
@layer base {
  :root {
    --background: 0 0% 100%;
    --foreground: 222.2 84% 4.9%;
    --primary: 222.2 47.4% 11.2%;
    --primary-foreground: 210 40% 98%;
    /* ... */
  }

  .dark {
    --background: 222.2 84% 4.9%;
    --foreground: 210 40% 98%;
    /* ... */
  }
}
```

---

## 构建和部署

### Next.js 配置

```typescript
// next.config.ts
const nextConfig = {
  output: 'export',  // 静态导出
  trailingSlash: true,
  distDir: '../target/web',  // 输出到 target/web
};

export default nextConfig;
```

### 构建命令

```bash
# 开发模式
npm run dev

# 生产构建
npm run build

# 构建输出到 target/web/
# 由 Apex Gateway backend 读取并提供静态导出资源
```

### 安装脚本

```bash
#!/bin/bash
# install.sh

# 构建前端
cd web
npm install
npm run build

# 构建内嵌 Dashboard 资源的发布二进制
cd ..
cargo build --release --features embedded-web
```

---

## 测试策略

### Playwright 测试配置

```typescript
// playwright.config.ts
export default {
  testDir: './tests',
  timeout: 30000,
  use: {
    baseURL: 'http://localhost:12356',
    headless: true,
  },
};
```

### 测试用例

```typescript
// tests/dashboard.spec.ts
import { test, expect } from '@playwright/test';

test.describe('Dashboard', () => {
  test('should display metrics cards', async ({ page }) => {
    await page.goto('/dashboard');

    await expect(page.locator('text=总请求数')).toBeVisible();
    await expect(page.locator('text=总错误数')).toBeVisible();
    await expect(page.locator('text=平均延迟')).toBeVisible();
  });

  test('should filter by time range', async ({ page }) => {
    await page.goto('/dashboard');

    await page.click('text=本周');
    await expect(page.locator('[data-testid=metrics-cards]')).toBeVisible();
  });

  test('should display usage table', async ({ page }) => {
    await page.goto('/dashboard');

    const table = page.locator('table');
    await expect(table).toBeVisible();

    const rows = table.locator('tbody tr');
    await expect(rows).toHaveCount.greaterThan(0);
  });
});
```

---

## 性能优化

### 1. 代码分割

Next.js App Router 自动进行代码分割，每个路由独立打包。

### 2. 静态导出

使用 `output: 'export'` 生成纯静态文件，由 Rust backend 的统一静态资源层提供服务；发布态推荐嵌入到二进制。

### 3. 图表懒加载

```typescript
const TrendChart = dynamic(() => import('./trend-chart'), {
  ssr: false,
  loading: () => <Skeleton />
});
```

### 4. 数据缓存

使用 React Query 进行数据缓存和重新验证：

```typescript
const { data: metrics } = useQuery({
  queryKey: ['metrics', filters],
  queryFn: () => api.getMetrics(filters),
  staleTime: 30000, // 30 秒
});
```

---

## 安全考虑

### 1. API Key 存储

- 存储在 `localStorage` (加密)
- 不存储在 Cookie (避免 CSRF)

### 2. XSS 防护

- 使用 React 的自动转义
- 不使用 `dangerouslySetInnerHTML`

### 3. CSP 配置

```html
<meta http-equiv="Content-Security-Policy"
      content="default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'">
```

---

## 未来功能

### Trend 图表增强
- [ ] 支持更多图表类型 (散点图、热力图)
- [ ] 导出图表为图片/PDF
- [ ] 自定义时间粒度

### 筛选器增强
- [ ] 保存筛选预设
- [ ] 多条件组合筛选
- [ ] 高级搜索语法

### 数据导出
- [ ] 导出 CSV/Excel
- [ ] 定时报表生成
- [ ] 邮件推送

---

_Generated using BMAD Method `document-project` workflow_
