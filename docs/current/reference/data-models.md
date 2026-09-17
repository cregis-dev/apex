# Apex Gateway - Data Models

**Generated:** 2026-03-10
**Last verified:** 2026-08-03 (对照 `src/database.rs`)
**Scope:** SQLite 数据库模型和 Schema 设计

## 数据库概览

Apex Gateway 使用 SQLite 作为嵌入式数据库，用于持久化存储：
- Usage 使用记录 (`usage_records`)
- Metrics 性能指标 (`metrics_requests` / `metrics_errors` / `metrics_fallbacks` / `metrics_latency`)
- Usage 小时级预聚合 (`usage_rollup`)，用于行为画像基线
- Gemini 工具回放缓存 (`gemini_replay_turns`)

**数据库位置:** `~/.apex/data/apex.db` (可通过 `data_dir` 配置)

**唯一 Schema 来源:** 所有建表、索引与迁移语句都集中在 `src/database.rs` 的
`Database::new_db()` (`src/database.rs:52`) 中，没有独立的 `.sql` 迁移文件。
本文档中的 SQL 与该函数保持一致。

### 连接与 PRAGMA

打开数据库时按固定顺序设置以下 PRAGMA (`src/database.rs:74-79`)：

```sql
PRAGMA auto_vacuum=INCREMENTAL;  -- 必须在首次读写前设置，否则会被锁定为 NONE
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA busy_timeout=5000;
```

网关持有两个连接：
- **写连接** (`conn`)：所有 INSERT/DELETE，以及 Gemini 回放的「读后刷新」。
- **只读连接** (`read_conn`)：看板/分析类查询，额外设置 `PRAGMA query_only=ON`。
  WAL 模式下读取一致性快照，不会与写连接互相阻塞。

---

## Schema 设计

### 1. usage_records - Usage 使用记录表

存储每次 API 调用的详细记录。

**基础建表语句** (`src/database.rs:84-106`)：

```sql
CREATE TABLE IF NOT EXISTS usage_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    request_id TEXT,
    team_id TEXT NOT NULL,
    router TEXT NOT NULL,
    matched_rule TEXT,
    channel TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    latency_ms REAL,
    fallback_triggered INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'success',
    status_code INTEGER,
    error_message TEXT,
    provider_trace_id TEXT,
    provider_error_body TEXT,
    client TEXT,
    user_agent TEXT,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0
);
```

**迁移新增列** (`src/database.rs:190-234`)：

建表语句之后会执行一串 `ALTER TABLE ... ADD COLUMN`，全部以 `let _ = conn.execute(...)`
忽略错误。对**新库**而言，除 `req_hash` 和 `session_key` 外的列已在上面的 `CREATE TABLE` 中，这些语句会因
「列已存在」而静默失败；对**老库**而言，它们负责补齐历史版本缺失的列。

| 列 | 语句 | 说明 |
|----|------|------|
| `request_id` `matched_rule` `latency_ms` `fallback_triggered` `status` `status_code` `error_message` `provider_trace_id` `provider_error_body` `client` `user_agent` `cache_read_tokens` `cache_write_tokens` | `ALTER TABLE` + 已在 `CREATE TABLE` 中 | 老库升级路径；新库直接由建表语句创建 |
| `req_hash` | **仅** `ALTER TABLE usage_records ADD COLUMN req_hash TEXT` (`database.rs:234`) | 不在 `CREATE TABLE` 中，任何库都靠该迁移添加。可空，历史行为 NULL |
| `session_key` | **仅** `ALTER TABLE usage_records ADD COLUMN session_key TEXT` (`database.rs:248`) | 同上，v0.11.0 引入，不在 `CREATE TABLE` 中。可空 |

> **可空性说明：** 通过 `ALTER TABLE` 补加的列在老库的历史行上一律为 NULL 或取
> `DEFAULT`。`cache_read_tokens` / `cache_write_tokens` / `fallback_triggered` /
> `status` 带 `NOT NULL DEFAULT`，历史行会回填默认值；其余可空列（`request_id`、
> `matched_rule`、`client`、`user_agent`、`req_hash`、`session_key` 等）在历史行上为 NULL，
> 消费方需要按 NULL 处理。

> **注意：** 表中**没有** `created_at` 列，时间语义完全由 `timestamp` 承担
> (写入格式为本地时间 `%Y-%m-%d %H:%M:%S`，见 `log_usage`)。

**索引:**

```sql
-- 建表批次内 (src/database.rs:108-112)
CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_records(timestamp);
CREATE INDEX IF NOT EXISTS idx_usage_team ON usage_records(team_id);
CREATE INDEX IF NOT EXISTS idx_usage_router ON usage_records(router);
CREATE INDEX IF NOT EXISTS idx_usage_channel ON usage_records(channel);
CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records(model);

-- 迁移区内 (src/database.rs:236-252)
CREATE INDEX IF NOT EXISTS idx_usage_client ON usage_records(client);
CREATE INDEX IF NOT EXISTS idx_usage_req_hash ON usage_records(team_id, req_hash);
CREATE INDEX IF NOT EXISTS idx_usage_session_key ON usage_records(team_id, session_key);
```

`idx_usage_req_hash` 是联合索引，服务于重复率检测的 `GROUP BY team_id, req_hash`；
`idx_usage_session_key` 同理，服务于按会话分组 / 按对话查询。

**字段说明:**

| 字段 | 类型 | 可空 | 说明 |
|------|------|------|------|
| `id` | INTEGER | 否 | 主键，自增 |
| `timestamp` | TEXT | 否 | 请求时间戳，本地时间 `YYYY-MM-DD HH:MM:SS` |
| `request_id` | TEXT | 是 | 请求 ID，用于串联日志 |
| `team_id` | TEXT | 否 | 团队 (成员) ID |
| `router` | TEXT | 否 | 路由名称 |
| `matched_rule` | TEXT | 是 | 命中的路由规则 |
| `channel` | TEXT | 否 | 实际调用的上游通道名称 |
| `model` | TEXT | 否 | 使用的模型名称 (写入时统一转小写) |
| `input_tokens` | INTEGER | 否 | 输入 Token 数，默认 0 |
| `output_tokens` | INTEGER | 否 | 输出 Token 数，默认 0 |
| `latency_ms` | REAL | 是 | 上游延迟 (毫秒) |
| `fallback_triggered` | INTEGER | 否 | 是否触发了 fallback (0/1)，默认 0 |
| `status` | TEXT | 否 | 请求状态，默认 `'success'`；错误态为 `error` / `fallback_error` |
| `status_code` | INTEGER | 是 | HTTP 状态码 |
| `error_message` | TEXT | 是 | 错误信息摘要 |
| `provider_trace_id` | TEXT | 是 | 上游返回的追踪 ID |
| `provider_error_body` | TEXT | 是 | 上游错误响应体原文 |
| `client` | TEXT | 是 | 客户端/工具归因 (Claude Code、Codex、SDK…)，来自请求头 |
| `user_agent` | TEXT | 是 | 原始 User-Agent |
| `cache_read_tokens` | INTEGER | 否 | 缓存命中 Token，默认 0；与 `input_tokens` 分开计价 |
| `cache_write_tokens` | INTEGER | 否 | 缓存写入/创建 Token，默认 0 |
| `req_hash` | TEXT | 是 | 请求语义指纹 (blake3，128-bit hex)，**仅哈希不含 prompt 原文**；画像/`hash_requests` 关闭或历史行时为 NULL，检测逻辑跳过 NULL |
| `session_key` | TEXT | 是 | 对话指纹 (blake3 of `system` + 首条 message)——会话亲和路由用的同一个键，每个请求都记录，便于事后按多轮对话分组；请求体没有 conversation 数组时为 NULL |

**写入路径:** `Database::log_usage()` (`src/database.rs:460`) 一次性写入除 `id`
外的全部 22 列。

**读取路径:** 列顺序由常量 `Database::USAGE_RECORD_COLUMNS`
(`src/database.rs:633`) 统一定义，与 `map_usage_record()` 的位置索引严格对应
—— 增删列时必须同时改这两处。

---

### 2. metrics_requests - 请求指标表

记录请求数量统计 (`src/database.rs:114-120`)。

```sql
CREATE TABLE IF NOT EXISTS metrics_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    route TEXT NOT NULL,
    router TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 1
);
```

**索引:**
```sql
CREATE INDEX IF NOT EXISTS idx_metrics_timestamp ON metrics_requests(timestamp);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | INTEGER | 主键，自增 |
| `timestamp` | TEXT | 时间戳 |
| `route` | TEXT | API 路由路径 |
| `router` | TEXT | 路由名称 |
| `count` | INTEGER | 请求数量，默认 1 (每次调用写一行，汇总时用 `COUNT(*)`) |

---

### 3. metrics_errors - 错误指标表

记录错误请求数量 (`src/database.rs:122-128`)。

```sql
CREATE TABLE IF NOT EXISTS metrics_errors (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    route TEXT NOT NULL,
    router TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 1
);
```

**索引:**
```sql
CREATE INDEX IF NOT EXISTS idx_metrics_errors_timestamp ON metrics_errors(timestamp);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | INTEGER | 主键，自增 |
| `timestamp` | TEXT | 时间戳 |
| `route` | TEXT | API 路由路径 |
| `router` | TEXT | 路由名称 |
| `count` | INTEGER | 错误数量，默认 1 |

> 该表**不含** `error_code` 列；具体状态码与错误详情记录在
> `usage_records.status_code` / `error_message` 中。

---

### 4. metrics_fallbacks - Fallback 指标表

记录 Fallback 触发次数 (`src/database.rs:130-136`)。

```sql
CREATE TABLE IF NOT EXISTS metrics_fallbacks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    router TEXT NOT NULL,
    channel TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 1
);
```

**索引:**
```sql
CREATE INDEX IF NOT EXISTS idx_metrics_fallbacks_timestamp ON metrics_fallbacks(timestamp);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `id` | INTEGER | 主键，自增 |
| `timestamp` | TEXT | 时间戳 |
| `router` | TEXT | 路由名称 |
| `channel` | TEXT | Fallback 到的通道 |
| `count` | INTEGER | Fallback 次数，默认 1 |

> 该表**不含** `from_channel` / `reason` 列；原通道与失败原因需通过
> `usage_records` 中同一 `request_id` 的记录还原。

---

### 5. metrics_latency - 延迟指标表

记录请求延迟数据 (`src/database.rs:138-145`)。

```sql
CREATE TABLE IF NOT EXISTS metrics_latency (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    route TEXT NOT NULL,
    router TEXT NOT NULL,
    channel TEXT NOT NULL,
    latency_ms REAL NOT NULL
);
```

**索引:**
```sql
CREATE INDEX IF NOT EXISTS idx_metrics_latency_timestamp ON metrics_latency(timestamp);
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

---

### 6. gemini_replay_turns - Gemini 工具回放缓存表

缓存 Gemini 协议转换所需的助手轮次上下文 (`src/database.rs:147-157`)。

```sql
CREATE TABLE IF NOT EXISTS gemini_replay_turns (
    cache_key TEXT PRIMARY KEY,
    team_id TEXT NOT NULL,
    model TEXT NOT NULL,
    tool_use_id TEXT NOT NULL,
    assistant_content_json TEXT NOT NULL,
    prior_messages_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    last_accessed_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
```

**索引:**
```sql
CREATE INDEX IF NOT EXISTS idx_gemini_replay_expires_at ON gemini_replay_turns(expires_at);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `cache_key` | TEXT | 主键，缓存键 |
| `team_id` | TEXT | 团队 ID |
| `model` | TEXT | 模型名称 |
| `tool_use_id` | TEXT | 工具调用 ID |
| `assistant_content_json` | TEXT | 助手轮次内容 (JSON) |
| `prior_messages_json` | TEXT | 前置消息列表 (JSON) |
| `created_at` | INTEGER | 创建时间 (Unix 秒) |
| `last_accessed_at` | INTEGER | 最近访问时间 (Unix 秒) |
| `expires_at` | INTEGER | 过期时间 (Unix 秒) |

> 这是本库中唯一带 `created_at` 的表，且为 **Unix 时间戳整数**，不是
> `datetime('now')` 文本。写入走 `upsert_gemini_replay_turn()`
> (`ON CONFLICT(cache_key) DO UPDATE`)，并顺带删除已过期行；
> 读取走 `get_gemini_replay_turn()`，命中时顺延 `expires_at`。

---

### 7. usage_rollup - Usage 小时级预聚合表

按 `(小时, 成员, 模型, 通道)` 预聚合 `usage_records`，作为行为画像基线的底座
(`src/database.rs:164-179`)。

```sql
CREATE TABLE IF NOT EXISTS usage_rollup (
    bucket_start       TEXT    NOT NULL,
    team_id            TEXT    NOT NULL,
    model              TEXT    NOT NULL,
    channel            TEXT    NOT NULL,
    requests           INTEGER NOT NULL DEFAULT 0,
    error_requests     INTEGER NOT NULL DEFAULT 0,
    input_tokens       INTEGER NOT NULL DEFAULT 0,
    output_tokens      INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens  INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    zero_output_reqs   INTEGER NOT NULL DEFAULT 0,
    latency_sum_ms     REAL    NOT NULL DEFAULT 0,
    latency_count      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket_start, team_id, model, channel)
);
```

**索引:**
```sql
CREATE INDEX IF NOT EXISTS idx_rollup_team_bucket ON usage_rollup(team_id, bucket_start);
```

**字段说明:**

| 字段 | 类型 | 说明 |
|------|------|------|
| `bucket_start` | TEXT | 小时桶起点 `YYYY-MM-DD HH:00:00` (联合主键) |
| `team_id` | TEXT | 团队 (成员) ID (联合主键) |
| `model` | TEXT | 模型名称 (联合主键) |
| `channel` | TEXT | 通道名称 (联合主键) |
| `requests` | INTEGER | 桶内请求数 |
| `error_requests` | INTEGER | 状态为 `error` / `fallback_error` 的请求数 |
| `input_tokens` | INTEGER | 输入 Token 合计 (按行 `MAX(col, 0)` 截断负值后求和) |
| `output_tokens` | INTEGER | 输出 Token 合计 (同上) |
| `cache_read_tokens` | INTEGER | 缓存读 Token 合计 |
| `cache_write_tokens` | INTEGER | 缓存写 Token 合计 |
| `zero_output_reqs` | INTEGER | 非错误、有输入但零输出的请求数 |
| `latency_sum_ms` | REAL | 延迟求和 (`COALESCE(latency_ms, 0)`) |
| `latency_count` | INTEGER | 有延迟数据的行数；`latency_sum_ms / latency_count` 还原平均延迟 |

**设计说明:**
- 刻意**不含** `group` 字段：分组是可变的配置属性，读取时再解析，不固化进历史桶。
- 保留期与原始记录**相互独立** (见下方「数据清理策略」)，因此基线可以比原始行活得更久。
- 写入是幂等的：`rollup_usage(lookback_hours)` 用 `INSERT OR REPLACE` 按主键整桶重算，
  重复执行或窗口重叠都不会重复累加。
- `backfill_rollup_if_empty()` 仅在表为空时做一次全量回填，可无条件在启动时调用。

---

## Rust 数据模型

以下结构体定义在 `src/database.rs` 末尾。

### UsageRecord

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct UsageRecord {
    pub id: i64,
    pub timestamp: String,
    pub request_id: Option<String>,
    pub team_id: String,
    pub router: String,
    pub matched_rule: Option<String>,
    pub final_channel: String,
    pub channel: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub latency_ms: Option<f64>,
    pub fallback_triggered: bool,
    pub status: String,
    pub status_code: Option<i64>,
    pub error_message: Option<String>,
    pub provider_trace_id: Option<String>,
    pub provider_error_body: Option<String>,
    pub client: Option<String>,
    pub user_agent: Option<String>,
    #[serde(default)]
    pub cache_read_tokens: i64,
    #[serde(default)]
    pub cache_write_tokens: i64,
    #[serde(default)]
    pub req_hash: Option<String>,
    pub session_key: Option<String>,
}
```

> `final_channel` **不是数据库列** —— 它在 `map_usage_record()` 中由 `channel`
> 克隆而来，仅为兼容前端字段名。

### RollupRow

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct RollupRow {
    pub bucket_start: String,
    pub team_id: String,
    pub model: String,
    pub channel: String,
    pub requests: i64,
    pub error_requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub zero_output_reqs: i64,
    pub latency_sum_ms: f64,
    pub latency_count: i64,
}
```

### MetricsSummary

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSummary {
    pub total_requests: i64,
    pub total_errors: i64,
    pub total_fallbacks: i64,
    pub avg_latency_ms: f64,
    pub error_rate: f64,
    pub p95_latency_ms: f64,
}
```

### TrendData

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct TrendData {
    pub date: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_errors: i64,
    pub total_fallbacks: i64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
}
```

### RankingItem

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct RankingItem {
    pub name: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub percentage: f64,
}
```

### 其他辅助结构

| 结构 | 用途 |
|------|------|
| `UsageRecordQuery` | 看板筛选条件 (team/router/channel/model/status/client/时间范围) |
| `UsageRecordPage` | 分页结果：`records` + `total` + `new_records` + `latest_cursor` |
| `UsageAggregate` | 窗口聚合：`requests` / `total_tokens` / `error_count` / `avg_latency_ms` |
| `FilterOptions` | 筛选下拉项的去重取值集合 |
| `GeminiReplayTurnRecord` | Gemini 回放缓存的读取结果 |

---

## CRUD 操作

所有数据库访问都是 `Database` 的方法 (`src/database.rs`)，写操作走写连接，
读操作走只读连接。

### 写入

| 方法 | 说明 |
|------|------|
| `log_usage(...)` | 写入一条 `usage_records`，含 21 个显式列 |
| `log_request(route, router)` | 写入 `metrics_requests` |
| `log_error(route, router)` | 写入 `metrics_errors` |
| `log_fallback(router, channel)` | 写入 `metrics_fallbacks` |
| `log_latency(route, router, channel, latency_ms)` | 写入 `metrics_latency` |
| `upsert_gemini_replay_turn(...)` | Upsert 回放缓存，并清理过期行 |
| `rollup_usage(lookback_hours)` | 增量重算尾部窗口的小时桶 |
| `backfill_rollup_if_empty()` | 表为空时做一次全量回填 |

写入路径全部使用 `let _ = conn.execute(...)` 吞掉错误 —— 指标落库失败不应影响请求链路。

### 查询

| 方法 | 返回 |
|------|------|
| `get_usage_records(team_id, router, channel, model, status, start_date, end_date, limit, offset)` | `(Vec<UsageRecord>, i64)` |
| `get_usage_records_page(query, limit, offset, since_timestamp, since_id)` | `UsageRecordPage` |
| `get_usage_records_for_analytics(...)` | 分析用的记录集合 |
| `get_usage_aggregate(query)` | `UsageAggregate` |
| `get_usage_summary(...)` | `UsageSummary` |
| `get_filter_options(query)` | `FilterOptions` |
| `get_metrics_summary()` | `MetricsSummary` |
| `get_trends(period, start_date, end_date)` | `Vec<TrendData>`，`period` 取 `daily`/`weekly`/`monthly` |
| `get_rankings(by, limit)` | `Vec<RankingItem>`，`by` 取 `team`(默认)/`model`/`channel`/`router` |
| `get_rollup_rows()` / `get_rollup_between(start, end)` | `Vec<RollupRow>` |
| `distinct_models_for_team(team_id)` | `Vec<String>` |
| `get_gemini_replay_turn(cache_key, ttl)` | `Option<GeminiReplayTurnRecord>` |

> 读取 `usage_records` 时统一使用 `USAGE_RECORD_COLUMNS` 常量拼接列名，
> 并由 `map_usage_record()` 按位置解析，避免 `SELECT *` 带来的列序漂移。

---

## 数据迁移

Schema 演进采用「建表语句写全量 + 追加幂等 `ALTER TABLE`」的模式，
没有版本号表，也没有独立迁移文件：

1. `CREATE TABLE IF NOT EXISTS` 描述当前的完整基础 Schema。
2. 之后逐条执行 `ALTER TABLE ... ADD COLUMN`，用 `let _ =` 忽略「列已存在」错误。
   - 新库：这些语句基本都会失败，属正常。
   - 老库：补齐缺失列，历史行取 `DEFAULT` 或 NULL。
3. 索引一律用 `CREATE INDEX IF NOT EXISTS`。

**新增一列的步骤:**

```rust
// 1. 加到 CREATE TABLE (供新库使用) —— req_hash 是例外，仅走 ALTER
// 2. 追加一条幂等迁移 (供老库升级)
let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN new_col TEXT", []);
// 3. 若参与查询，同步更新 USAGE_RECORD_COLUMNS 与 map_usage_record 的位置索引
// 4. 若参与写入，同步更新 log_usage 的列表与 params!
```

---

## 数据清理策略

### 原始记录与指标 (按 `retention_days`)

`Database::cleanup_old_records(retention_days)` (`src/database.rs:262`)，
`retention_days == 0` 表示永久保留：

```sql
-- 对以下 5 张表逐一执行，cutoff = now - retention_days，格式 'YYYY-MM-DD HH:MM:SS'
DELETE FROM usage_records     WHERE timestamp < :cutoff;
DELETE FROM metrics_requests  WHERE timestamp < :cutoff;
DELETE FROM metrics_errors    WHERE timestamp < :cutoff;
DELETE FROM metrics_fallbacks WHERE timestamp < :cutoff;
DELETE FROM metrics_latency   WHERE timestamp < :cutoff;

-- 归还空闲页并截断 WAL
PRAGMA incremental_vacuum;
PRAGMA wal_checkpoint(TRUNCATE);
```

> `incremental_vacuum` 依赖 `auto_vacuum=INCREMENTAL`。该设置只对新库或
> 执行过一次全量 `VACUUM` 的老库生效。
>
> 该方法持有写连接锁并可能全表扫描，需通过 `spawn_blocking` 在异步运行时之外执行。

### 预聚合桶 (独立保留期)

`Database::prune_rollup(retention_days)`，按 `bucket_start` 而非 `timestamp` 裁剪，
与原始记录保留期互不影响：

```sql
DELETE FROM usage_rollup WHERE bucket_start < :cutoff;  -- cutoff 对齐到整点
```

### Gemini 回放缓存 (按 TTL 自清理)

无需定时任务：每次 `upsert_gemini_replay_turn()` 写入后顺带执行

```sql
DELETE FROM gemini_replay_turns WHERE expires_at <= :now;
```

---

## ER 图

各表相互独立，没有外键约束；跨表关联依赖 `timestamp` / `team_id` / `request_id`
等业务字段。

```
┌──────────────────────┐        ┌─────────────────┐   ┌─────────────────┐
│    usage_records     │        │metrics_requests │   │ metrics_errors  │
├──────────────────────┤        ├─────────────────┤   ├─────────────────┤
│ id (PK)              │        │ id (PK)         │   │ id (PK)         │
│ timestamp            │        │ timestamp       │   │ timestamp       │
│ request_id           │        │ route           │   │ route           │
│ team_id              │        │ router          │   │ router          │
│ router               │        │ count           │   │ count           │
│ matched_rule         │        └─────────────────┘   └─────────────────┘
│ channel              │
│ model                │        ┌─────────────────┐   ┌─────────────────┐
│ input_tokens         │        │metrics_fallbacks│   │ metrics_latency │
│ output_tokens        │        ├─────────────────┤   ├─────────────────┤
│ latency_ms           │        │ id (PK)         │   │ id (PK)         │
│ fallback_triggered   │        │ timestamp       │   │ timestamp       │
│ status               │        │ router          │   │ route           │
│ status_code          │        │ channel         │   │ router          │
│ error_message        │        │ count           │   │ channel         │
│ provider_trace_id    │        └─────────────────┘   │ latency_ms      │
│ provider_error_body  │                              └─────────────────┘
│ client               │
│ user_agent           │        ┌──────────────────────┐
│ cache_read_tokens    │        │  gemini_replay_turns │
│ cache_write_tokens   │        ├──────────────────────┤
│ req_hash (迁移新增)   │        │ cache_key (PK)       │
└──────────┬───────────┘        │ team_id              │
           │                    │ model                │
           │ 小时级聚合          │ tool_use_id          │
           │ (rollup_usage)     │ assistant_content_json│
           ▼                    │ prior_messages_json  │
┌──────────────────────┐        │ created_at           │
│     usage_rollup     │        │ last_accessed_at     │
├──────────────────────┤        │ expires_at           │
│ bucket_start ┐       │        └──────────────────────┘
│ team_id      │ PK    │
│ model        │       │
│ channel      ┘       │
│ requests             │
│ error_requests       │
│ input_tokens         │
│ output_tokens        │
│ cache_read_tokens    │
│ cache_write_tokens   │
│ zero_output_reqs     │
│ latency_sum_ms       │
│ latency_count        │
└──────────────────────┘
```

---

_Generated using BMAD Method `document-project` workflow_
