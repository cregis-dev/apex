# Apex Gateway - Data Models

**Generated:** 2026-03-10
**Scope:** SQLite 数据库模型和 Schema 设计

## 数据库概览

Apex Gateway 使用 SQLite 作为嵌入式数据库，用于持久化存储：
- Usage 使用记录
- Metrics 性能指标
- (未来) 配置历史和审计日志

**数据库位置:** `~/.apex/data/apex.db` (可通过 `data_dir` 配置)

---

## Schema 设计

### 1. usage_records - Usage 使用记录表

存储每次 API 调用的详细记录。

```sql
CREATE TABLE usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    team_id TEXT NOT NULL,
    router TEXT NOT NULL,
    channel TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now'))
);
```

**索引:**
```sql
CREATE INDEX idx_usage_timestamp ON usage_records(timestamp);
CREATE INDEX idx_usage_team_id ON usage_records(team_id);
CREATE INDEX idx_usage_router ON usage_records(router);
CREATE INDEX idx_usage_channel ON usage_records(channel);
CREATE INDEX idx_usage_model ON usage_records(model);
CREATE INDEX idx_usage_created_at ON usage_records(created_at);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | INTEGER | 主键，自增 |
| `timestamp` | TEXT | 请求时间戳 (ISO 8601) |
| `team_id` | TEXT | 团队 ID |
| `router` | TEXT | 路由名称 |
| `channel` | TEXT | 上游通道名称 |
| `model` | TEXT | 使用的模型名称 |
| `input_tokens` | INTEGER | 输入 Token 数 |
| `output_tokens` | INTEGER | 输出 Token 数 |
| `created_at` | TEXT | 记录创建时间 (自动) |

**示例数据:**
```sql
INSERT INTO usage_records (timestamp, team_id, router, channel, model, input_tokens, output_tokens)
VALUES ('2024-01-01T10:00:00Z', 'demo-team', 'default-router', 'openai-main', 'gpt-4', 100, 200);
```

---

### 2. metrics_requests - 请求指标表

记录请求数量统计。

```sql
CREATE TABLE metrics_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    route TEXT NOT NULL,
    router TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 1,
    created_at TEXT DEFAULT (datetime('now'))
);
```

**索引:**
```sql
CREATE INDEX idx_requests_timestamp ON metrics_requests(timestamp);
CREATE INDEX idx_requests_route ON metrics_requests(route);
CREATE INDEX idx_requests_router ON metrics_requests(router);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | INTEGER | 主键，自增 |
| `timestamp` | TEXT | 时间戳 |
| `route` | TEXT | API 路由路径 |
| `router` | TEXT | 路由名称 |
| `count` | INTEGER | 请求数量 |
| `created_at` | TEXT | 记录创建时间 (自动) |

---

### 3. metrics_errors - 错误指标表

记录错误请求数量。

```sql
CREATE TABLE metrics_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    route TEXT NOT NULL,
    router TEXT NOT NULL,
    error_code TEXT,
    count INTEGER NOT NULL DEFAULT 1,
    created_at TEXT DEFAULT (datetime('now'))
);
```

**索引:**
```sql
CREATE INDEX idx_errors_timestamp ON metrics_errors(timestamp);
CREATE INDEX idx_errors_route ON metrics_errors(route);
CREATE INDEX idx_errors_router ON metrics_errors(router);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | INTEGER | 主键，自增 |
| `timestamp` | TEXT | 时间戳 |
| `route` | TEXT | API 路由路径 |
| `router` | TEXT | 路由名称 |
| `error_code` | TEXT | 错误状态码 (如 429, 500) |
| `count` | INTEGER | 错误数量 |
| `created_at` | TEXT | 记录创建时间 (自动) |

---

### 4. metrics_fallbacks - Fallback 指标表

记录 Fallback 触发次数。

```sql
CREATE TABLE metrics_fallbacks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    router TEXT NOT NULL,
    channel TEXT NOT NULL,
    from_channel TEXT NOT NULL,
    reason TEXT,
    count INTEGER NOT NULL DEFAULT 1,
    created_at TEXT DEFAULT (datetime('now'))
);
```

**索引:**
```sql
CREATE INDEX idx_fallbacks_timestamp ON metrics_fallbacks(timestamp);
CREATE INDEX idx_fallbacks_router ON metrics_fallbacks(router);
CREATE INDEX idx_fallbacks_channel ON metrics_fallbacks(channel);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | INTEGER | 主键，自增 |
| `timestamp` | TEXT | 时间戳 |
| `router` | TEXT | 路由名称 |
| `channel` | TEXT | Fallback 到的通道 |
| `from_channel` | TEXT | 原通道 |
| `reason` | TEXT | Fallback 原因 (如 429, 500) |
| `count` | INTEGER | Fallback 次数 |
| `created_at` | TEXT | 记录创建时间 (自动) |

---

### 5. metrics_latency - 延迟指标表

记录请求延迟数据。

```sql
CREATE TABLE metrics_latency (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    route TEXT NOT NULL,
    router TEXT NOT NULL,
    channel TEXT NOT NULL,
    latency_ms REAL NOT NULL,
    created_at TEXT DEFAULT (datetime('now'))
);
```

**索引:**
```sql
CREATE INDEX idx_latency_timestamp ON metrics_latency(timestamp);
CREATE INDEX idx_latency_route ON metrics_latency(route);
CREATE INDEX idx_latency_router ON metrics_latency(router);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | INTEGER | 主键，自增 |
| `timestamp` | TEXT | 时间戳 |
| `route` | TEXT | API 路由路径 |
| `router` | TEXT | 路由名称 |
| `channel` | TEXT | 上游通道名称 |
| `latency_ms` | REAL | 延迟 (毫秒) |
| `created_at` | TEXT | 记录创建时间 (自动) |

---

## Rust 数据模型

### UsageRecord

```rust
pub struct UsageRecord {
    pub id: i64,
    pub timestamp: String,
    pub team_id: String,
    pub router: String,
    pub channel: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}
```

### MetricsSummary

```rust
pub struct MetricsSummary {
    pub total_requests: i64,
    pub total_errors: i64,
    pub total_fallbacks: i64,
    pub avg_latency_ms: f64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}
```

### TrendData

```rust
pub struct TrendData {
    pub date: String,
    pub requests: i64,
    pub errors: i64,
    pub fallbacks: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub avg_latency_ms: f64,
}
```

### RankingData

```rust
pub struct RankingData {
    pub name: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub percentage: f64,
}
```

---

## CRUD 操作

### 插入 Usage 记录

```rust
pub fn record_usage(
    conn: &Connection,
    team_id: &str,
    router: &str,
    channel: &str,
    model: &str,
    input_tokens: i64,
    output_tokens: i64,
) -> Result<()> {
    let timestamp = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO usage_records (timestamp, team_id, router, channel, model, input_tokens, output_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![timestamp, team_id, router, channel, model, input_tokens, output_tokens],
    )?;
    Ok(())
}
```

### 查询 Usage 记录

```rust
pub fn get_usage_records(
    conn: &Connection,
    filters: &UsageFilters,
    limit: i64,
    offset: i64,
) -> Result<(Vec<UsageRecord>, i64)> {
    let mut sql = String::from("SELECT * FROM usage_records WHERE 1=1");
    let mut params: Vec<&dyn ToSql> = vec![];

    if let Some(team_id) = &filters.team_id {
        sql.push_str(" AND team_id = ?");
        params.push(team_id);
    }
    if let Some(router) = &filters.router {
        sql.push_str(" AND router = ?");
        params.push(router);
    }
    // ... 其他筛选条件

    sql.push_str(" ORDER BY timestamp DESC LIMIT ? OFFSET ?");
    params.push(&limit);
    params.push(&offset);

    let records: Vec<UsageRecord> = conn
        .prepare(&sql)?
        .query_map(params![params], |row| {
            Ok(UsageRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                team_id: row.get(2)?,
                router: row.get(3)?,
                channel: row.get(4)?,
                model: row.get(5)?,
                input_tokens: row.get(6)?,
                output_tokens: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // 获取总数
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM usage_records", [], |row| row.get(0))?;

    Ok((records, total))
}
```

### 获取 Metrics 汇总

```rust
pub fn get_metrics_summary(conn: &Connection) -> Result<MetricsSummary> {
    let total_requests: i64 = conn.query_row("SELECT SUM(count) FROM metrics_requests", [], |row| row.get(0))?;
    let total_errors: i64 = conn.query_row("SELECT SUM(count) FROM metrics_errors", [], |row| row.get(0))?;
    let total_fallbacks: i64 = conn.query_row("SELECT SUM(count) FROM metrics_fallbacks", [], |row| row.get(0))?;
    let avg_latency_ms: f64 = conn.query_row("SELECT AVG(latency_ms) FROM metrics_latency", [], |row| row.get(0))?;
    let total_input_tokens: i64 = conn.query_row("SELECT SUM(input_tokens) FROM usage_records", [], |row| row.get(0))?;
    let total_output_tokens: i64 = conn.query_row("SELECT SUM(output_tokens) FROM usage_records", [], |row| row.get(0))?;

    Ok(MetricsSummary {
        total_requests,
        total_errors,
        total_fallbacks,
        avg_latency_ms,
        total_input_tokens,
        total_output_tokens,
    })
}
```

### 获取趋势数据

```rust
pub fn get_metrics_trends(
    conn: &Connection,
    period: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<TrendData>> {
    let date_format = match period {
        "daily" => "%Y-%m-%d",
        "weekly" => "%Y-%W",
        "monthly" => "%Y-%m",
        _ => "%Y-%m-%d",
    };

    let sql = format!(
        r#"SELECT
            strftime(?1, timestamp) as date,
            SUM(r.count) as requests,
            SUM(COALESCE(e.count, 0)) as errors,
            SUM(COALESCE(f.count, 0)) as fallbacks,
            SUM(u.input_tokens) as input_tokens,
            SUM(u.output_tokens) as output_tokens,
            AVG(l.latency_ms) as avg_latency_ms
        FROM metrics_requests r
        LEFT JOIN metrics_errors e ON strftime(?1, r.timestamp) = strftime(?1, e.timestamp)
        LEFT JOIN metrics_fallbacks f ON strftime(?1, r.timestamp) = strftime(?1, f.timestamp)
        LEFT JOIN usage_records u ON strftime(?1, r.timestamp) = strftime(?1, u.timestamp)
        LEFT JOIN metrics_latency l ON strftime(?1, r.timestamp) = strftime(?1, l.timestamp)
        WHERE r.timestamp BETWEEN ?2 AND ?3
        GROUP BY date
        ORDER BY date"#
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![date_format, start_date, end_date], |row| {
        Ok(TrendData {
            date: row.get(0)?,
            requests: row.get(1)?,
            errors: row.get(2)?,
            fallbacks: row.get(3)?,
            input_tokens: row.get(4)?,
            output_tokens: row.get(5)?,
            avg_latency_ms: row.get(6)?,
        })
    })?;

    rows.collect()
}
```

### 获取排行榜数据

```rust
pub fn get_rankings(conn: &Connection, by: &str, limit: i64) -> Result<Vec<RankingData>> {
    let (group_by, name_field) = match by {
        "team" => ("team_id", "team_id"),
        "model" => ("model", "model"),
        "channel" => ("channel", "channel"),
        _ => ("team_id", "team_id"),
    };

    let total: i64 = conn.query_row("SELECT SUM(input_tokens + output_tokens) FROM usage_records", [], |row| row.get(0))?;

    let sql = format!(
        r#"SELECT
            {} as name,
            COUNT(*) as requests,
            SUM(input_tokens) as input_tokens,
            SUM(output_tokens) as output_tokens
        FROM usage_records
        GROUP BY {}
        ORDER BY requests DESC
        LIMIT ?"#,
        name_field, group_by
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], |row| {
        let input_tokens: i64 = row.get(2)?;
        let output_tokens: i64 = row.get(3)?;
        let total_tokens = input_tokens + output_tokens;
        let percentage = if total > 0 {
            (total_tokens as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        Ok(RankingData {
            name: row.get(0)?,
            requests: row.get(1)?,
            input_tokens,
            output_tokens,
            percentage,
        })
    })?;

    rows.collect()
}
```

---

## 数据迁移

### 初始化 Schema

```rust
pub fn init_database(db_path: &str) -> Result<Connection> {
    let conn = Connection::open(db_path)?;

    // 创建表
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS usage_records (...);
        CREATE TABLE IF NOT EXISTS metrics_requests (...);
        CREATE TABLE IF NOT EXISTS metrics_errors (...);
        CREATE TABLE IF NOT EXISTS metrics_fallbacks (...);
        CREATE TABLE IF NOT EXISTS metrics_latency (...);
        "#,
    )?;

    // 创建索引
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_records(timestamp);
        CREATE INDEX IF NOT EXISTS idx_usage_team_id ON usage_records(team_id);
        -- ... 其他索引
        "#,
    )?;

    Ok(conn)
}
```

---

## 数据清理策略

### 自动清理过期数据

```sql
-- 删除 30 天前的数据
DELETE FROM usage_records WHERE timestamp < datetime('now', '-30 days');
DELETE FROM metrics_requests WHERE timestamp < datetime('now', '-30 days');
DELETE FROM metrics_errors WHERE timestamp < datetime('now', '-30 days');
DELETE FROM metrics_fallbacks WHERE timestamp < datetime('now', '-30 days');
DELETE FROM metrics_latency WHERE timestamp < datetime('now', '-30 days');
```

### 数据聚合 (可选)

```sql
-- 按天聚合历史数据
INSERT INTO usage_daily (date, team_id, router, channel, model, total_input_tokens, total_output_tokens)
SELECT
    date(timestamp) as date,
    team_id, router, channel, model,
    SUM(input_tokens), SUM(output_tokens)
FROM usage_records
GROUP BY date(timestamp), team_id, router, channel, model;
```

---

## ER 图

```
┌─────────────────┐       ┌─────────────────┐
│  usage_records  │       │ metrics_requests│
├─────────────────┤       ├─────────────────┤
│ id (PK)         │       │ id (PK)         │
│ timestamp       │       │ timestamp       │
│ team_id         │       │ route           │
│ router          │       │ router          │
│ channel         │       │ count           │
│ model           │       └─────────────────┘
│ input_tokens    │
│ output_tokens   │       ┌─────────────────┐
└─────────────────┘       │ metrics_errors  │
                          ├─────────────────┤
┌─────────────────┐       │ id (PK)         │
│ metrics_latency │       │ timestamp       │
├─────────────────┤       │ route           │
│ id (PK)         │       │ router          │
│ timestamp       │       │ error_code      │
│ route           │       │ count           │
│ router          │       └─────────────────┘
│ channel         │
│ latency_ms      │       ┌─────────────────┐
└─────────────────┘       │metrics_fallbacks│
                          ├─────────────────┤
                          │ id (PK)         │
                          │ timestamp       │
                          │ router          │
                          │ channel         │
                          │ from_channel    │
                          │ reason          │
                          │ count           │
                          └─────────────────┘
```

---

_Generated using BMAD Method `document-project` workflow_
