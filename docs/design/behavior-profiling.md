# Apex Gateway — 行为画像 / 统计分析(滥用与浪费识别)技术设计

**Status:** Draft / 待评审
**Scope:** 元数据行为采集、rollup 预聚合、读时异常检测、治理控制台
**Date:** 2026-07-23
**关联:** 成本计费设计见 [cost-billing.md](cost-billing.md)(需求一,已随 v0.8.0 发布);本设计是需求二。

---

## 1. 背景与目标

成本计费(需求一)回答了"谁花了多少钱";但花钱多 ≠ 有效工作。**有些用户因程序 bug 或不熟悉,在空转 / 无意义重复请求 / 刷量**——真金白银被浪费,却不体现在"是否超预算"里。团队管理者需要一套**行为画像**,把"浪费型滥用"从正常高用量里区分出来,形成治理动作(限流 / 停用)。

术语沿用需求一:**`team_id` = 用户(user / member)**,**`Team.group` = 团队(team)**。定位到人的前提**已具备**——每人一把 key。

### 设计边界(第一阶段只做"浪费型滥用",不碰语义)

| 能抓(本设计做) | 抓不到(留 v2) |
|---|---|
| 重复率、速率暴涨、错误-重试风暴、产出≈0、花费尖峰、作息画像 | 语义上"做无关的事"(需正文 + 分类器,重且敏感) |

只用**元数据 + 一个请求哈希**,**绝不落库请求正文**。语义级判定(是否在做正经事)需要读 prompt 正文并跑分类器,成本高且触及隐私红线,第一阶段明确不做。

### 设计原则(与需求一一致)

- **不破坏现状**:新列默认 NULL/0、新 config 段可选、响应字段全增量。DB / config / API / 老前端全兼容。
- **可选即降级**:`profiling` 未配置 ⇒ 不算哈希、不跑 rollup、不出治理 section(照 `pricing` 缺失即隐藏成本卡的先例)。
- **检测读时算,不落 flag**(用户已选):异常在分析 handler 里从预聚合 + 原始行实时算,随响应返回;不引入 flag 状态机。处置由人工确认后调用现成的 `rate_limit` / `enabled:false`。
- **隐私优先**:请求哈希是 blake3 截断,**单向、只存哈希**;且受 `profiling.hash_requests` 独立开关控制。

---

## 2. 检测信号与指标(第一阶段六项)

每项都是"**先规则阈值打 flag,再叠统计基线**"的两层结构(见 §4)。所有量都能从 `usage_records`(+ 一个 `req_hash` 列)+ rollup 预聚合导出。

| 信号 | 定义 / 公式 | 数据来源 | 抓什么 |
|---|---|---|---|
| **重复率 repeat_rate** | 窗口内 `同 team_id 同 req_hash` 的记录数占比:`1 − distinct(req_hash)/count`;或"最大单哈希重复次数" | `usage_records.req_hash` | 循环 / 空转 / 刷量:同一个请求打无数遍 |
| **速率暴涨 rate_spike** | 当前小时 RPM vs 该 member 滚动 7d 同时段基线的 z-score | rollup(每小时 requests) | 突然爆量,偏离自身常态 |
| **错误-重试风暴 error_storm** | 窗口内 `error_requests / requests` 超阈 **且** 错误集中在少数 `req_hash`(不退避重试同一失败请求) | rollup(error) + `req_hash` | 429/5xx 不退避的死循环重试 |
| **产出质量代理 output_zero** | `output_tokens≈0` 的成功请求占比(有输入无输出 = 大概率无意义调用) | `usage_records` | 空跑、探活式刷量、坏脚本 |
| **花费尖峰 spend_spike** | member 当窗参考成本 vs 自身 7d 基线 z-score(复用需求一 `reference_cost`) | rollup(tokens)+ `pricing` | 烧钱异常(与成本区联动) |
| **作息画像 activity_profile** | 24 小时活跃分布熵 / 夜间(非工作时段)请求占比;`24/7 均匀 ⇒ 机器`,`集中工作时段 ⇒ 人` | rollup(按 bucket 小时) | 区分自动化脚本 vs 真人 |

> 花费尖峰依赖需求一已落地的 `reference_cost`(`pricing` 未配则该信号自动关闭)。其余五项不依赖成本。

---

## 3. 数据层(第一阶段落地)

### 3.1 `usage_records` 加一列 `req_hash`(照 `src/database.rs:167-207` 幂等迁移范式)

```rust
let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN req_hash TEXT", []);
let _ = conn.execute(
    "CREATE INDEX IF NOT EXISTS idx_usage_req_hash ON usage_records(team_id, req_hash)", []);
```

- 默认 NULL ⇒ 历史行、以及 profiling 关闭时写入的行都是 NULL,重复率检测自动跳过它们,**零数据破坏**。
- 复合索引 `(team_id, req_hash)`:重复率检测是 `GROUP BY team_id, req_hash`,这条索引直接服务它。
- 同步改动:`USAGE_RECORD_COLUMNS`(`src/database.rs:445`)追加 `req_hash`;`map_usage_record`(`:449`)加 `req_hash: row.get(21)?`;`struct UsageRecord`(`:1132`)加字段;`log_usage`(`:264`)参数表 + INSERT 列表 + `params!` 各加一项。

### 3.2 请求哈希采集 —— 唯一有链路复杂度的部分

**采集点:`process_request`(`src/server.rs:3901`)。** 入站请求体在 `src/server.rs:3912` 已作为原始 `axum::body::Bytes` 读入(`bytes`),后续只机会性解析出 `model`。在这里算哈希:

```rust
// src/request_hash.rs (新)
/// 归一化后对"语义载荷"取 blake3,返回 16 字节截断的 hex(32 字符)。
/// 只用于同 user 的重复检测,单向不可逆,绝不携带正文。
pub fn request_hash(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    // 只哈希"会话内容"字段,对无关参数抖动(max_tokens/stream/temperature…)稳健:
    //   OpenAI/Anthropic → "messages";Anthropic 顶层 "system";Gemini → "contents";
    //   兜底 → "prompt";全无 → 整个规范化 body。
    let payload = v.get("messages")
        .or_else(|| v.get("contents"))
        .or_else(|| v.get("prompt"))
        .unwrap_or(&v);
    // serde_json 对 object 的键序不保证稳定 ⇒ 用 to_string 前先规范化。
    let canon = canonical_json(payload);
    let digest = blake3::hash(canon.as_bytes());
    Some(hex::encode(&digest.as_bytes()[..16]))
}
```

- **归一化**:`messages` 数组通常已是稳定序;object 的键序用 `canonical_json`(递归按 key 排序)消抖,避免"同内容不同键序 ⇒ 不同哈希"。
- **透传链路**:哈希在 `process_request` 算好后,作为 `Option<String>` 参数穿过 `wrap_response`(`src/usage.rs:484`)→ `UsageTrackerState::new`(`:138`)→ `flush`(`:306`)→ `UsageLogger::log`(`:25`)→ `db.log_usage`。**失败路径**(`log_failure` 及 server.rs 里的直接 `log_usage` 调用点)同样带上——错误-重试风暴要看失败行的 `req_hash`。这与需求一 cache token 的穿线方式完全一致。
- **开关**:仅当 `profiling.enabled && profiling.hash_requests` 时才解析+哈希(默认 on);否则传 `None`,零开销。
- **依赖**:新增 `blake3`(算哈希)、`hex`(编码)。进程内目前无直接哈希库(`sha2` 仅传递依赖,`upgrade.rs` 走 `sha256sum` 命令)。blake3 快、无 C 依赖。

### 3.3 rollup 预聚合表 + 定时聚合任务(基线的底座)

行为基线要按 member 看 7d/30d 滚动均值方差,现有 compute-on-read(`get_usage_records_for_analytics` 全量载入内存,`src/database.rs:560`)撑不住长窗口。引入按**小时**桶的预聚合表;日/周视图在读时从小时桶再上卷。

```sql
CREATE TABLE IF NOT EXISTS usage_rollup (
    bucket_start       TEXT    NOT NULL,   -- 'YYYY-MM-DD HH:00:00' 本地时,与 usage_records.timestamp 同格式
    team_id            TEXT    NOT NULL,
    model              TEXT    NOT NULL,
    channel            TEXT    NOT NULL,
    requests           INTEGER NOT NULL DEFAULT 0,
    error_requests     INTEGER NOT NULL DEFAULT 0,
    input_tokens       INTEGER NOT NULL DEFAULT 0,
    output_tokens      INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens  INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    zero_output_reqs   INTEGER NOT NULL DEFAULT 0,   -- output≈0 的成功请求数(output_zero 信号)
    latency_sum_ms     REAL    NOT NULL DEFAULT 0,
    latency_count      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (bucket_start, team_id, model, channel)
);
CREATE INDEX IF NOT EXISTS idx_rollup_team_bucket ON usage_rollup(team_id, bucket_start);
```

- **不含 `group`**:`group` 是 config 字段、会变;rollup 只按 `team_id` 聚,`group→成员`映射在读时从活配置解析(团队数很少),这样改组不影响历史桶。
- **聚合任务**(幂等,重算尾窗):后台任务每 `interval_minutes` 跑一次

  ```sql
  INSERT OR REPLACE INTO usage_rollup
  SELECT strftime('%Y-%m-%d %H:00:00', timestamp) AS bucket_start,
         team_id, model, channel,
         COUNT(*),
         SUM(CASE WHEN status IN ('error','fallback_error') THEN 1 ELSE 0 END),
         SUM(MAX(input_tokens,0)),  SUM(MAX(output_tokens,0)),
         SUM(cache_read_tokens),    SUM(cache_write_tokens),
         SUM(CASE WHEN status='success' AND output_tokens<=0 AND input_tokens>0 THEN 1 ELSE 0 END),
         SUM(COALESCE(latency_ms,0)),
         SUM(CASE WHEN latency_ms IS NOT NULL THEN 1 ELSE 0 END)
  FROM usage_records
  WHERE timestamp >= ?1   -- 尾窗起点(now − lookback,对齐到整点)
  GROUP BY bucket_start, team_id, model, channel;
  ```

  - **INSERT OR REPLACE + PK** ⇒ 天然幂等;每次只重算尾窗(默认 lookback=3h)覆盖迟到写入。
  - **冷启动全量回填**:表空时首跑一次 `WHERE 1=1` 全量;之后只跑尾窗。回填持写连接锁扫全表 `usage_records`(与 retention 任务同范式,在 `spawn_blocking` 里、非阻塞 bind);首次对超大历史库启用时可能短暂阻塞请求日志写入——一次性代价,可接受。
  - **连接**:走写连接 `self.conn`(`read_conn` 是 `query_only=ON`,`src/database.rs:216`);任务在 `run_server` 里用 `spawn_blocking` 起,照 retention 任务(`cleanup_old_records`,`src/database.rs:230`)的范式。
  - **保留策略**:rollup 独立于原始行保留期。原始 `usage_records` 默认留 90d(`Retention.days`),而 rollup 体积极小(members×models×channels×hours),默认留更久(见 §3.4 `rollup.retention_days`,默认 400d),**让基线能看过 90d 的长窗**。故 rollup **不**并进 `cleanup_old_records`(`src/database.rs:241`)的删表清单,单独按自身保留期裁剪。

### 3.4 新 config 段 `profiling`(`src/config.rs`,可选)

照 `Option<Pricing>`(`src/config.rs:32-35`)/ `Option<Compliance>`(带 `validate()`,`:485-512`)的可选段范式:

```rust
#[serde(default)] pub profiling: Option<Profiling>,   // None ⇒ 功能整体关闭

pub struct Profiling {
    #[serde(default)] pub enabled: bool,
    #[serde(default = "default_true")]  pub hash_requests: bool,       // req_hash 独立隐私开关
    #[serde(default)] pub rollup: RollupConfig,
    #[serde(default)] pub thresholds: Thresholds,                      // 供第二阶段检测读取
}
pub struct RollupConfig {
    #[serde(default = "d_rollup_interval")] pub interval_minutes: u64, // 默认 15
    #[serde(default = "d_rollup_lookback")] pub lookback_hours: u64,   // 默认 3
    #[serde(default = "d_rollup_retention")] pub retention_days: u64,  // 默认 400;0=永久
}
pub struct Thresholds {   // 全部有默认值,第二阶段检测用;本期只解析+校验,不消费
    pub repeat_rate: f64,       // 默认 0.5 —— 窗口内 >50% 请求是重复
    pub rate_spike_z: f64,      // 默认 3.0
    pub error_ratio: f64,       // 默认 0.3
    pub output_zero_ratio: f64, // 默认 0.4
    pub spend_spike_z: f64,     // 默认 3.0
    pub night_ratio: f64,       // 默认 0.6 —— 非工作时段请求占比
    pub min_samples: u64,       // 默认 50 —— 样本不足不判定,压假阳
}
```

`Profiling::validate()`:比率∈[0,1]、z≥0、interval/lookback>0;并额外校验 **`lookback_hours*60 ≥ interval_minutes`**(否则每轮尾窗盖不住运行间隔 ⇒ rollup 永久留洞)及各值的**合理上限**(interval≤30d、lookback≤1y、retention≤100y、min_samples≤1e9,防超大值 `as i64` 溢出 chrono `Duration` 静默打死 rollup tick)。非法即 `load_config`(`src/config.rs:514`)报错返回(照 `compliance.validate()` 的挂点)。

---

## 4. 检测层(第二阶段;读时无状态)—— 已实现

> **已落地**于 `src/server.rs::build_behavior_section`,由 `dashboard_analytics_api_handler` 挂载。
> **实现要点/与原设计的偏差:**
> - **无状态**:每信号一个布尔判定 + z-score 叠加,随 `behavior` 响应段返回,不落 flag 表。
> - **作息定义**:工作时段固定 `[08:00, 20:00)` 本地(`WORK_HOUR_START/END`),之外记夜间;非当前可配。
> - **baseline**:取窗口前 `7d`(`BEHAVIOR_BASELINE_DAYS`)的 rollup,`get_rollup_between` 半开区间**排除当前窗**——两端 bucket 边界**向下取整到整点**后再比较(桶 `bucket_start` 本就是整点;不取整则 end="14:37" 会漏放行窗口自身的 "14:00" 桶,把窗口内流量算进基线)。基线桶数 < 12(`BEHAVIOR_MIN_BASELINE_BUCKETS`)或方差为 0 ⇒ 该 z 记 `None` 不判。
>   - **已知语义局限(保守取舍)**:z 比较的是"窗口**平均**每小时速率(`requests/window_hours`)vs 基线**活跃**小时桶分布"(rollup 只存有流量的小时,空闲小时不入分布)。故 (a) 长窗口(24h/7d)里一次集中爆发会被平均稀释、可能不触发;(b) 基线均值只含活跃小时、偏高,进一步降 z。二者都是**假阴**方向——刻意偏保守以压假阳(与"别误伤自动化"一致)。z 信号对**短窗(1h)最灵敏**(此时 `current_rate ≈ 当窗请求数`,与小时桶同尺度)。真正的"同时段(hour-of-day/星期)对齐 + 空闲小时零填充"留后续(零填充会给稀疏用户引入假阳,需配合门槛谨慎设计)。
> - **peer 离群(同 group MAD)本期未做**——六个 per-member 信号已覆盖 §2 全部;peer 叠加留后续。
> - `min_samples` 门槛按 member 窗口请求数;`repeat_rate` 额外要求 `hashed >= min_samples`。
> - **处置建议**:每 flag 带 `suggested_action`;member 级取最强,**≥2 个"可执行"(非 observe)的 critical ⇒ disable**,否则最强单项(rate_limit/observe)。按"可执行 critical"而非裸 critical 数升级,避免夜间 cron(critical `off_hours`+`output_zero`,两者本身仅 observe)被误升到 disable。

两层,都在分析 handler 里从 rollup + 原始行**实时算**,不落 flag 表:

1. **规则阈值 → 打 flag**:每信号一个布尔判定(§2 公式 vs §3.4 阈值),`min_samples` 不足直接跳过(压假阳)。
2. **统计基线叠加**:每 member 从 rollup 取滚动 7d 同时段序列,算均值/方差 → 当前值 z-score;同 `group` peer 间用 MAD/IQR 找离群点。z 超 `*_z` 阈值 ⇒ 该 flag 置信度升级。

产出一个 `flags: Vec<{member, signal, severity, evidence, suggested_action}>`,`evidence` 携带触发该 flag 的量(重复次数、z 值、错误率、夜间占比…),`suggested_action` ∈ `{observe, rate_limit(rpm/tpm), disable}`。

---

## 5. 计算层与 API(第二阶段)—— 已实现

照 cost section 的挂法:

- `DashboardAnalyticsResponse` 加 `behavior: Option<DashboardBehaviorSection>` + `#[serde(skip_serializing_if = "Option::is_none")]`。**响应形状**:`{ window_secs, evaluated, flagged, members: [{ id, group, severity, suggested_action, profile{requests,repeat_rate,error_ratio,output_zero_ratio,night_ratio,rate_z?,spend_z?,reference_cost?}, flags: [{ signal, severity, value, threshold, detail, suggested_action }] }] }`,members 仅含被 flag 者、按 severity→requests 排序。
- `build_behavior_section(current, baseline, thresholds, teams, pricing, channels, window)`:每 member 窗口画像 + 触发的 flags。spend_spike 复用 §4.1 的 `reference_cost`(rollup 桶用 `rollup_reference_cost`);无 pricing 时 spend 信号自动关闭。
- 仅当 `profiling.enabled` 时构建,否则 `None`(前端据此隐藏治理页)。
- **本期直接挂在 analytics 主响应**,未另开 `/api/dashboard/profiling`;治理页要"按 member 拉 7d 序列"的重量视图时再加该端点。

---

## 6. 处置(第二阶段;检测→人工确认→执行)

用户已选:**告警 + 建议,人工确认后执行**,复用现成处置端:

- **限流**:写回 `Team.policy.rate_limit`(`TeamRateLimit{rpm,tpm}`,`src/config.rs:256`)。
- **停用**:`Team.enabled = Some(false)`(`is_paused()`,`src/config.rs:207`)硬暂停该 user。
- 治理页每条 flag 一个"采纳建议"按钮 → 预填限流值/停用 → 二次确认 → 走既有 Teams 写配置通道热更新。检测端只给建议,**不自动执行**。

---

## 7. 前端(第三阶段;独立治理页)—— 已实现

- **独立页** `cp/src/pages/GovernancePage.tsx`,路由 `/governance`(`App.tsx`),导航在 **Access** 段(`Sidebar.tsx`,`shield` 图标)。
- **导航门控**:`/api/cp/info` 加 `profiling_enabled`(后端 `config.profiling.enabled`);Sidebar 据此**隐藏/显示** Governance 项(nav item `requiresProfiling`)。直接敲 URL 且 `behavior` 缺失时,页面渲染 **"Profiling not enabled"** 空态。
- **页面结构**:汇总(evaluated / flagged / critical 三 stat)→ 被 flag 用户卡列表(按 severity→requests 排,后端已排):每卡 = 头部(severity 点 + user + group chip + flag 数 + 建议动作)+ 画像指标条(requests / repeat / errors / zero-out / night / rate z / spend z,超阈染色)+ flag 明细行(signal + severity pill + evidence)+ **处置区**。
- **处置(人工确认)**:每卡两键 **Set rate limit** / **Disable user**,建议项高亮为 primary。限流复用现成 `RateLimitEditor`(与 Rate Limits 页同一编辑器,带近 24h 参考流量);停用走确认 Modal。二者均调既有 `PATCH /admin/teams/{id}`(`api.updateTeam`)写 `rate_limit` / `enabled:false`,成功后 invalidate `['teams']`+`['analytics']` + toast。已 disable 的 user 显示 "User disabled";非受管 id(如 `global`)无处置按钮。
- 类型镜像 `cp/src/lib/types.ts`(`BehaviorSection/Member/Flag/Profile` + `AnalyticsResponse.behavior?`,照 `CostSection` 增量范式);无新 API 方法(`behavior` 随 `api.analytics` 主响应返回)。
- **降级**:`behavior` 缺失 ⇒ 导航项隐藏 + 页面空态,不显示误导内容。
- **偏差**:证据用画像指标 + flag 明细,**未做 24 格作息热力**(需按小时分布,与"按 member 拉 7d 序列"的重量端点一并留后续);Rust↔TS 线格式由后端单测 `behavior_section_serializes_to_the_wire_shape_the_cp_expects` 锁住。

---

## 8. 零破坏保证 & 边界取舍

- 新列默认 NULL、新表 `IF NOT EXISTS`、新 config 段可选、响应字段增量 ⇒ DB / config / API / 老前端全兼容。
- **哈希稳健性**:只哈希会话内容字段 + 键序归一化;仍抓不到"内容微调的近重复"(改一个字 ⇒ 不同哈希)——第一阶段接受,近重复留 v2(需 minhash/simhash)。
- **假阳压制**:`min_samples` 门槛 + 双层(规则∧基线)+ 建议而非自动执行。
- **时区**:沿用 `usage_records.timestamp` 的本地、秒精度、字符串格式;rollup `strftime` 与 `build_usage_record_filters`(`src/database.rs:777`)口径一致。

---

## 9. 分阶段交付

| 阶段 | 内容 | 本次? |
|---|---|---|
| **一期(地基)** | ① `req_hash` 列 + 迁移 + 索引 ② 请求哈希采集(`request_hash.rs` + 穿线)③ rollup 表 + 定时聚合任务 + 冷启动回填 ④ `profiling` config 段 + `validate()` ⑤ 单测 | ✅ **已完成** |
| **二期(检测)** | 六信号规则阈值 + rate/spend z-score 基线 → `build_behavior_section` + `behavior: Option<...>` 响应段(挂 analytics 主响应)+ 单测 | ✅ **已完成** |
| **三期(前端+处置)** | 独立 Governance 页(`profiling_enabled` 门控导航)+ 证据视图 + 人工确认联动 `rate_limit`/`enabled`(复用 `RateLimitEditor` + `PATCH /admin/teams`)+ 线格式单测 | ✅ **已完成** |
| 后续(可选) | peer-离群叠加 · 24 格作息热力 + `/api/dashboard/profiling` 重量端点 · 近重复(minhash) · 语义级判定(v2) · 作息时段可配 · 失败行 req_hash(error-storm 哈希收敛) | ⛔ 待排期 |

**一期即"最高杠杆新增":** req_hash 一击命中循环/空转,rollup 是所有基线的底座——两者备好,二/三期是纯读时计算 + UI。

---

## 10. 验证(一期)

1. **单测**:
   - `request_hash`:同内容(含键序打乱)→ 同哈希;不同内容 → 不同哈希;非 JSON body 兜底不 panic;输出是 32 字符 hex、不含正文子串。
   - rollup 聚合:造多行 `usage_records`(跨小时/多 member/含 error/含 output=0)→ 跑聚合 → 断言各桶 requests/error/tokens/zero_output 正确;重跑幂等(INSERT OR REPLACE 不翻倍)。
   - config:`profiling` 段解析 + 默认值 + `validate()` 拒非法阈值。
2. `cargo test` 全绿;`cargo clippy` 无新警告。
3. 手验:配一个 `profiling.enabled=true` 的 config 跑网关,发几个重复请求 + 几个失败请求,确认 `usage_records.req_hash` 落库、`usage_rollup` 有桶且重跑不翻倍;`profiling` 缺失时一切照旧(无哈希、无 rollup)。

---

## 11. 未决问题 / v2

- **近重复**:内容微调的刷量(minhash/simhash) vs 精确哈希;第一阶段只做精确。
- **语义滥用**:读正文 + 分类器判"是否做正经事"——重且敏感,需单独隐私评审。
- **flag 生命周期**:v1 读时无状态;若要"已确认/已忽略"记忆,需 flag 表(用户当前选无状态)。
- **rollup 粒度**:小时桶是否够;超高频 member 是否要分钟桶。
- **告警外发**:是否接 webhook/邮件周报(与需求一"预算告警"共管道)。
