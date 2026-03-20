# Apex Gateway - Web Dashboard Component Inventory

**Generated:** 2026-03-10
**Scope:** Next.js 组件清单

---

## 组件层级结构

```
Dashboard (page.tsx)
├── Header
├── TimeFilter
│   └── DatePicker (Popover)
├── FilterBar
│   ├── TeamSelect
│   ├── RouterSelect
│   ├── ChannelSelect
│   └── ModelSelect
├── MetricsCards
│   ├── MetricCard (x5)
│   │   ├── RequestsCard
│   │   ├── ErrorsCard
│   │   ├── FallbacksCard
│   │   ├── LatencyCard
│   │   └── TokensCard
├── ChartsGrid
│   ├── TrendChart
│   │   └── Recharts (Line/Area)
│   └── RankingList
│       └── RankingItem (xN)
├── UsageTable
│   ├── TableHeader
│   ├── TableRow (xN)
│   └── Pagination
└── ApiKeyModal (if not set)
```

---

## 页面组件 (Pages)

### 1. `app/page.tsx` - 首页

**职责**: landing page，提供 Dashboard 入口

**依赖**:
- `components/ui/button`
- `components/ui/card`

**关键代码**:
```tsx
export default function HomePage() {
  return (
    <main className="min-h-screen">
      <Header />
      <HeroSection />
      <FeatureCards />
      <CTAButton href="/dashboard" />
    </main>
  );
}
```

---

### 2. `app/dashboard/page.tsx` - Dashboard 主页面

**职责**: 组合所有 Dashboard 组件，管理全局状态

**依赖**:
- `components/dashboard/*`
- `hooks/use-api`
- `components/ui/*`

**状态**:
```typescript
const [timeRange, setTimeRange] = useState<TimeRange>('today');
const [filters, setFilters] = useState<Filters>({});
const [data, setData] = useState<DashboardData>({});
const [loading, setLoading] = useState(true);
const [apiKey, setApiKey] = useState<string>('');
```

---

## Dashboard 业务组件

### 1. MetricsCards (`components/dashboard/metrics-cards.tsx`)

**职责**: 展示 5 个关键指标卡片

**子组件**:
| 组件 | 说明 | 数据源 |
|------|------|--------|
| `RequestsCard` | 总请求数 | `metrics.total_requests` |
| `ErrorsCard` | 总错误数 | `metrics.total_errors` |
| `FallbacksCard` | Fallback 次数 | `metrics.total_fallbacks` |
| `LatencyCard` | 平均延迟 | `metrics.avg_latency_ms` |
| `TokensCard` | Token 消耗 | `metrics.total_input/output_tokens` |

**Props**:
```typescript
interface MetricsCardsProps {
  data: MetricsSummary | null;
  loading?: boolean;
}
```

---

### 2. TrendChart (`components/dashboard/trend-chart.tsx`)

**职责**: 展示时间趋势图表

**图表类型**:
- 请求量趋势 (折线图)
- Token 消耗趋势 (面积图)
- 延迟趋势 (折线图)

**Props**:
```typescript
interface TrendChartProps {
  data: TrendData[];
  period: 'daily' | 'weekly' | 'monthly';
  loading?: boolean;
}
```

**依赖**:
- `recharts`: LineChart, Line, AreaChart, Area, XAxis, YAxis, Tooltip, Legend

**实现**:
```tsx
export function TrendChart({ data, period }: TrendChartProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>请求趋势</CardTitle>
      </CardHeader>
      <CardContent>
        <LineChart data={data} width={600} height={300}>
          <XAxis dataKey="date" />
          <YAxis />
          <Tooltip />
          <Legend />
          <Line dataKey="requests" stroke="#8884d8" name="请求数" />
          <Line dataKey="input_tokens" stroke="#82ca9d" name="输入 Token" />
        </LineChart>
      </CardContent>
    </Card>
  );
}
```

---

### 3. RankingList (`components/dashboard/ranking-list.tsx`)

**职责**: 展示排行榜 (Team/Model/Channel)

**Props**:
```typescript
interface RankingListProps {
  data: RankingData[];
  by: 'team' | 'model' | 'channel';
  loading?: boolean;
}
```

**子组件**:
- `Tabs` (shadcn/ui) - Tab 切换
- `RankingItem` - 单项展示

**实现**:
```tsx
export function RankingList({ data, by }: RankingListProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{by === 'team' ? 'Team 排行' : by === 'model' ? 'Model 排行' : 'Channel 排行'}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="space-y-4">
          {data.map((item, index) => (
            <RankingItem key={item.name} item={item} rank={index + 1} />
          ))}
        </div>
      </CardContent>
    </Card>
  );
}
```

---

### 4. UsageTable (`components/dashboard/usage-table.tsx`)

**职责**: 展示 Usage 记录表格，支持分页和排序

**Props**:
```typescript
interface UsageTableProps {
  data: UsageRecord[];
  total: number;
  limit: number;
  offset: number;
  onPageChange: (offset: number) => void;
  onSortChange: (field: string) => void;
  loading?: boolean;
}
```

**列定义**:
```typescript
const columns: ColumnDef<UsageRecord>[] = [
  { accessorKey: 'timestamp', header: '时间' },
  { accessorKey: 'team_id', header: '团队' },
  { accessorKey: 'router', header: '路由' },
  { accessorKey: 'channel', header: '渠道' },
  { accessorKey: 'model', header: '模型' },
  { accessorKey: 'input_tokens', header: '输入 Token' },
  { accessorKey: 'output_tokens', header: '输出 Token' },
  {
    accessorKey: 'total_tokens',
    header: '总 Token',
    cell: ({ row }) => row.original.input_tokens + row.original.output_tokens,
  },
];
```

**依赖**:
- `@tanstack/react-table` - 表格逻辑
- `components/ui/table` - 表格样式

---

### 5. TimeFilter (`components/dashboard/time-filter.tsx`)

**职责**: 时间范围选择器

**Props**:
```typescript
interface TimeFilterProps {
  value: TimeRange;
  onChange: (value: TimeRange) => void;
}
```

**预设选项**:
- `today` - 今日
- `week` - 本周
- `month` - 本月
- `custom` - 自定义 (打开 DatePicker)

**实现**:
```tsx
export function TimeFilter({ value, onChange }: TimeFilterProps) {
  return (
    <Tabs value={value} onValueChange={(v) => onChange(v as TimeRange)}>
      <TabsList>
        <TabsTrigger value="today">今日</TabsTrigger>
        <TabsTrigger value="week">本周</TabsTrigger>
        <TabsTrigger value="month">本月</TabsTrigger>
        <TabsTrigger value="custom">自定义</TabsTrigger>
      </TabsList>
      {value === 'custom' && (
        <DatePicker selected={dateRange} onSelect={handleDateSelect} />
      )}
    </Tabs>
  );
}
```

---

### 6. FilterBar (`components/dashboard/filter-bar.tsx`)

**职责**: 多维度筛选器

**Props**:
```typescript
interface FilterBarProps {
  filters: Filters;
  onChange: (filters: Filters) => void;
}
```

**筛选项**:
| 字段 | 组件 | 数据源 |
|------|------|--------|
| `team_id` | Select | API `/api/usage?group=team` |
| `router` | Select | 配置中的 routers |
| `channel` | Select | 配置中的 channels |
| `model` | Select | API `/v1/models` |

**实现**:
```tsx
export function FilterBar({ filters, onChange }: FilterBarProps) {
  return (
    <div className="flex gap-4">
      <Select value={filters.teamId} onValueChange={(v) => onChange({ ...filters, teamId: v })}>
        <SelectTrigger><SelectValue placeholder="选择团队" /></SelectTrigger>
        <SelectContent>
          {teams.map(team => (
            <SelectItem key={team.id} value={team.id}>{team.id}</SelectItem>
          ))}
        </SelectContent>
      </Select>
      {/* 其他筛选项类似 */}
    </div>
  );
}
```

---

### 7. ApiKeyModal (`components/dashboard/api-key-modal.tsx`)

**职责**: API Key 输入弹窗 (首次访问时显示)

**状态管理**:
- 使用 `localStorage` 存储 API Key
- 验证 API Key 有效性

**实现**:
```tsx
export function ApiKeyModal({ open, onOpenChange, onSave }) {
  const [apiKey, setApiKey] = useState('');
  const [error, setError] = useState('');

  const handleSave = async () => {
    try {
      await fetch('/api/metrics', {
        headers: { 'Authorization': `Bearer ${apiKey}` }
      });
      localStorage.setItem('apex-api-key', apiKey);
      onSave(apiKey);
    } catch {
      setError('无效的 API Key');
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>输入 API Key</DialogTitle>
        </DialogHeader>
        <Input value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="sk-ap-..." />
        {error && <p className="text-red-500">{error}</p>}
        <Button onClick={handleSave}>保存</Button>
      </DialogContent>
    </Dialog>
  );
}
```

---

## shadcn/ui 基础组件

### 已使用的组件

| 组件 | 路径 | 说明 |
|------|------|------|
| `Button` | `components/ui/button` | 按钮 |
| `Card` | `components/ui/card` | 卡片容器 |
| `Table` | `components/ui/table` | 表格 |
| `Select` | `components/ui/select` | 下拉选择 |
| `Tabs` | `components/ui/tabs` | Tab 切换 |
| `Dialog` | `components/ui/dialog` | 对话框 |
| `Popover` | `components/ui/popover` | 弹出框 |
| `Input` | `components/ui/input` | 输入框 |
| `Calendar` | `components/ui/calendar` | 日历 (react-day-picker) |
| `Skeleton` | `components/ui/skeleton` | 加载骨架屏 |
| `Badge` | `components/ui/badge` | 徽章标签 |
| `Label` | `components/ui/label` | 标签 |

### 组件使用示例

```tsx
// Button 变体
<Button variant="default">Default</Button>
<Button variant="destructive">Delete</Button>
<Button variant="outline">Outline</Button>
<Button variant="ghost">Ghost</Button>
<Button variant="link">Link</Button>
<Button size="sm">Small</Button>
<Button size="lg">Large</Button>
```

---

## Hooks

### 1. `useApi` (`hooks/use-api.ts`)

**职责**: API 客户端封装

**返回值**:
```typescript
{
  apiKey: string;
  setApiKey: (key: string) => void;
  fetchApi: <T>(endpoint: string, options?: RequestInit) => Promise<T>;
  getUsage: (filters: UsageFilters) => Promise<UsageResponse>;
  getMetrics: () => Promise<MetricsSummary>;
  getTrends: (period: string) => Promise<TrendResponse>;
  getRankings: (by: string) => Promise<RankingResponse>;
}
```

---

### 2. `useDashboard` (`hooks/use-dashboard.ts`)

**职责**: Dashboard 状态管理

**返回值**:
```typescript
{
  timeRange: TimeRange;
  setTimeRange: (range: TimeRange) => void;
  filters: Filters;
  setFilters: (filters: Filters) => void;
  data: DashboardData;
  loading: boolean;
  error: Error | null;
  refresh: () => void;
}
```

---

### 3. `useUsageTable` (`hooks/use-usage-table.ts`)

**职责**: Usage 表格状态管理

**返回值**:
```typescript
{
  data: UsageRecord[];
  total: number;
  page: number;
  pageSize: number;
  sortField: string;
  sortDirection: 'asc' | 'desc';
  onPageChange: (page: number) => void;
  onSortChange: (field: string) => void;
  loading: boolean;
}
```

---

## 工具函数

### `lib/utils.ts`

```typescript
import { type ClassValue, clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatNumber(num: number): string {
  if (num >= 1000000) {
    return `${(num / 1000000).toFixed(1)}M`;
  }
  if (num >= 1000) {
    return `${(num / 1000).toFixed(1)}K`;
  }
  return num.toString();
}

export function formatDateTime(timestamp: string): string {
  return format(new Date(timestamp), 'yyyy-MM-dd HH:mm:ss');
}

export function formatDuration(ms: number): string {
  if (ms < 1000) {
    return `${ms}ms`;
  }
  return `${(ms / 1000).toFixed(2)}s`;
}
```

---

## 类型定义

### `lib/types.ts`

```typescript
export interface UsageRecord {
  id: number;
  timestamp: string;
  team_id: string;
  router: string;
  channel: string;
  model: string;
  input_tokens: number;
  output_tokens: number;
}

export interface MetricsSummary {
  total_requests: number;
  total_errors: number;
  total_fallbacks: number;
  avg_latency_ms: number;
  total_input_tokens: number;
  total_output_tokens: number;
}

export interface TrendData {
  date: string;
  requests: number;
  errors: number;
  fallbacks: number;
  input_tokens: number;
  output_tokens: number;
  avg_latency_ms: number;
}

export interface RankingData {
  name: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  percentage: number;
}

export type TimeRange = 'today' | 'week' | 'month' | 'custom';

export interface Filters {
  teamId?: string;
  router?: string;
  channel?: string;
  model?: string;
}
```

---

_Generated using BMAD Method `document-project` workflow_
