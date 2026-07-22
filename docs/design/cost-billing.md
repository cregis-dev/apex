# Apex Gateway — 成本计费技术设计(PAYG + Coding Plan 统一)

**Status:** Draft / 待评审
**Scope:** 后端用量入库、分析聚合、控制台成本视图
**Date:** 2026-07-21

---

## 1. 背景与目标

当前 Insight 面板只有请求数、token 数、延迟、成功率,**没有任何成本/金额维度**(全库搜 `cost/price/usd/budget` 零命中)。团队管理者无法回答"这个月花了多少钱、该砍谁、订阅值不值",token 多 ≠ 花钱多(不同模型单价差 10~100 倍),现有排行榜会误导决策。

上游订阅有两类,计费必须同时支持:

1. **Pay-as-you-go(PAYG)** —— 按 token 计价。
2. **Coding Plan** —— 固定月费,边际成本 ≈ 0,但需要:① 把固定费按用量**分摊**到用户/团队(内部记账);② 看**利用率**(额度有没有用满、值不值得续)。

术语前提(已在控制台纠正,见 [terminology 说明](#附术语)):**`team_id` = 用户(user)**,**`group` = 团队(team)**。本设计沿用此口径:分摊到 `member`(=user=team_id),上卷到 `team`(=group)。

### 设计原则

- **不破坏现状**:新列默认 0、新 config 段可选、响应字段全部增量。DB/config/API/老前端全兼容。
- **成本不落库,读时算**:只存原始 token(含 cache),成本用价格表在分析时计算 ⇒ 改价可重算历史。
- **provider 无关**:入库前把 token 归一化,让成本公式对所有上游统一。

---

## 2. 核心模型:参考成本(reference)+ 实付成本(actual)

> **v2 重构(已实现):** 定价改成**一组命名规则**,channel 各自选一条,不再按 model 全局匹配
> ——因为同一个 model 在不同 channel 价格可能不同。订阅也是一种规则。

每条记录按其 **channel 选中的规则** 算**参考成本**(通用可比尺子)。再按规则类型决定**实付**:

| 规则 `type` | 实付逻辑 | 管理产出 |
|---|---|---|
| `payg`(一口价) | 实付 = 参考成本(规则的 flat 费率 × token) | 真金白银 |
| `subscription` | 单条边际实付 = 0;**月费按窗口内各 member 的参考成本占比分摊**(多 channel 共用一条订阅 ⇒ 月费计一次) | 分摊账 + 利用率 |

- channel 没选规则 ⇒ **不计费**(该流量不进成本区)。
- **member 总成本** = Σ(PAYG 参考成本) + Σ(订阅月费分摊)
- **利用率** = 订阅规则参考价值 ÷ 已计提月费(>1 划算,<1 在为没用完的额度买单)
- **有效折扣** = 1 − 实付 ÷ 参考价值(整体拿到几折)

---

## 3. 数据层

### 3.1 定价规则 —— 新 config 段 `pricing`(命名规则表)

```jsonc
"pricing": {
  "currency": "USD",
  "unit": 1000000,                      // 费率按每 1M token 计
  "rules": [
    // PAYG 规则 = 费率表:规则内按 model 分档(首个命中生效,* 兜底)。
    // 解决"同一 channel 不同 model 不同价"(如 DeepSeek V4-Flash vs V4-Pro)。
    { "name": "deepseek", "type": "payg", "prices": [
      { "match": "*flash*", "input": 0.14,  "output": 0.28, "cache_read": 0.0028 },
      { "match": "*pro*",   "input": 0.435, "output": 0.87, "cache_read": 0.003625 },
      { "match": "*",       "input": 0.27,  "output": 1.10, "cache_read": 0.07 }
    ]},
    // 订阅规则:固定月费,无 per-token 费率。
    { "name": "claude-max", "type": "subscription", "monthly_fee": 200, "billing_day": 1 }
  ]
}
```

- 规则 `name` 唯一;`type` = `payg` | `subscription`。
- **cache miss = `input`(全价输入),cache hit = `cache_read`,output = `output`**;`cache_write` 缺省 = `input`。
- **不按 model 全局匹配**——channel 通过 `channel.pricing` 选规则;规则**内部**才按 model 分档。

### 3.2 通道选规则 —— `Channel.pricing` 字段(`src/config.rs`)

```rust
// Channel struct:删掉旧的 billing/Billing enum,改为按名引用一条规则
#[serde(default)] pub pricing: Option<String>,   // 规则名;None ⇒ 不计费
```

规则结构:
```rust
pub struct PricingRule {
    pub name: String,
    #[serde(rename = "type", default = "default_rule_kind")] pub kind: String,
    #[serde(default)] pub prices: Vec<ModelPrice>,          // PAYG 费率表(首个命中生效)
    pub monthly_fee: f64,                                   // 仅 subscription
    #[serde(default = "default_billing_day")] pub billing_day: u32,
    pub included_quota_tokens: Option<u64>,                 // 仅 subscription
}
pub struct ModelPrice {                                     // 费率表一行
    #[serde(rename = "match")] pub match_pattern: String,   // 精确或通配符 model
    pub input: f64, pub output: f64,
    pub cache_read: Option<f64>, pub cache_write: Option<f64>,
}
```
`PricingRule::price_for(model)` 在规则内按 model 找命中行(复用 team `allowed_models` 的通配符匹配)。

校验:规则名唯一、每行 `match` 可解析、费率≥0、订阅月费/日合法;`channel.pricing` 必须指向存在的规则(否则 400)。UI 在 **Configure ▸ Pricing** 管理(只读表 + 弹窗编辑,PAYG 弹窗内是小费率表),**Channels 编辑器**用下拉选规则。

### 3.3 usage_records 加两列(照 `src/database.rs:165-194` 的幂等迁移范式)

```rust
let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN cache_read_tokens  INTEGER NOT NULL DEFAULT 0", []);
let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN cache_write_tokens INTEGER NOT NULL DEFAULT 0", []);
```

默认 0 ⇒ 历史行成本照算(缓存部分记 0),**零数据破坏**。

### 3.4 采集 cache token —— 改 `src/usage.rs` `extract_usage`(:198)

> **现状:cache token 完全没采集** —— `extract_usage` 只取 `prompt/completion_tokens`(OpenAI)与 `input/output_tokens`(Anthropic/Gemini)。这是本需求唯一有 provider 适配复杂度的部分。

`UsageTrackerState`(:110)加 `cache_read_tokens / cache_write_tokens` 字段,`extract_usage` 追加各家格式解析:

| 上游 | 缓存字段 | 归一化规则(关键) |
|---|---|---|
| Anthropic | `usage.cache_read_input_tokens` / `cache_creation_input_tokens`(`usage` 与 `message.usage` 两处) | `input_tokens` **本就不含**缓存,直接分列存 |
| OpenAI | `usage.prompt_tokens_details.cached_tokens` | cached 是 prompt 的**子集**,存库前 `input = prompt − cached`,避免双算 |
| Gemini | `usageMetadata.cachedContentTokenCount` | 从 prompt 中扣除 |

> 归一化后成本公式对所有 provider 统一:`input` 恒为"全价计费的输入",`cache_read/write` 独立。

新值透传链路:`extract_usage → flush(:247) → UsageLogger.log(:25) → db.log_usage(:276) INSERT`,各函数参数表各加两项。

---

## 4. 计算层(分析 handler,`src/server.rs`)

原始行由 `get_usage_records_for_analytics`(`src/database.rs:541`)全量载入内存,逐条算成本**零额外查询开销**。

### 4.1 逐条参考成本

```rust
fn reference_cost(rec: &UsageRecord, p: &ModelPrice) -> f64 {
    (p.input       * rec.input_tokens as f64
   + p.output      * rec.output_tokens as f64
   + p.cache_read  * rec.cache_read_tokens as f64
   + p.cache_write * rec.cache_write_tokens as f64) / p.unit
}
```

### 4.2 PAYG 求和 / 订阅分摊

对窗口 `[start, end]`,每个 `subscription` 通道 C:

```
accrued_fee_C = monthly_fee × (窗口时长 / 30天)              // 简洁版;billing_day 的 month-to-date 作精化
total_ref_C   = Σ reference_cost(rec)   rec ∈ C, 窗口内
member 分摊    = accrued_fee_C × ref_C[member] / total_ref_C  // total_ref_C=0 ⇒ 不分摊,记为闲置浪费 idle_fee
utilization_C = total_ref_C / accrued_fee_C
```

`member.actual = Σ_payg reference_cost + Σ_订阅 member分摊`

overview 汇总:
- `actual_spend = Σ_payg reference_cost + Σ_C accrued_fee_C`
- `reference_value = Σ_all reference_cost`
- `effective_discount = 1 − actual_spend / reference_value`

### 4.3 新增/扩展响应字段(全部**增量**,老前端忽略即可)

- `DashboardOverview` 追加 `actual_cost / reference_cost / currency` + `delta.actual_cost`
- 新 section `cost`:
  ```jsonc
  "cost": {
    "currency": "USD",
    "by_member": [{ "id": "alice", "group": "eng", "actual": 12.3, "reference": 40.1 }],
    "by_model":  [{ "name": "gpt-4o", "actual": 8.0, "reference": 8.0 }],
    "subscriptions": [
      { "channel": "claude-plan", "monthly_fee": 200, "accrued_fee": 6.6,
        "reference_value": 42.0, "utilization": 6.36, "idle_fee": 0 }
    ]
  }
  ```
- `team_usage.leaderboard` 项追加 `actual_cost`(供"按花费"排序)

---

## 5. 前端(cp)

- `cp/src/lib/types.ts`:镜像上述新字段(增量,不破坏反序列化)。
- `cp/src/pages/OverviewPage.tsx`:
  - 新增 **Spend 卡片**(`actual_cost` + delta,副标 "list value $X · N% off");
  - Rankings 的 metric 从 `requests|tokens` 扩到 **`cost`**(`RankCard` :401 已是通用组件,加一档即可),花费榜按 **Team(group)→User(member)** 展示;
  - 新 **Subscriptions 卡片**:每个订阅通道一个利用率仪表(参考价值 vs 已计提月费)+ 省/亏金额 + 闲置额度告警。
- **降级**:`pricing` 未配置 ⇒ 成本全 0,前端据 `currency` 缺失**隐藏成本卡片**(不显示误导的 $0)。

---

## 6. 零破坏保证 & 边界取舍

- 新列默认 0、新 config 段可选、响应字段增量 ⇒ DB / config / API / 老前端**全兼容**。
- **价格随时间变**:`pricing` 可扩 `effective_from`(按记录日期选价);v1 先用当前单价近似历史,注释标注。
- **proration**:v1 用 30 天匀摊;v2 用 `billing_day` 做真实 month-to-date。
- **超额(overage)**:v1 只在 `included_quota_tokens` 上做"逼近额度"告警(复用行为画像告警管道);超额转按量留 v2。
- **无价格模型**:命中兜底 `*`(0 价)并在响应标 `priced:false`,前端提示"N 个模型缺单价"。

---

## 7. 验证 & 工作量

**验证**
1. 单测:`reference_cost` 各 provider 归一化、订阅分摊(含 `total_ref=0`)、利用率;`cargo test`。
2. `cd cp && pnpm build` 类型通过;e2e 冒烟(`tests/e2e`)。
3. 造一条订阅通道 + 一条 PAYG 通道,核对 overview 实付 = PAYG求和 + 计提月费,利用率符合手算。

**工作量分层**

| 步骤 | 内容 | 估时 |
|---|---|---|
| ① | cache 采集(usage.rs,含各家格式 + 单测) | ~半天 |
| ② | 加列 + 迁移 | ~1h |
| ③ | 价格表 / billing 配置解析 | ~半天 |
| ④ | 成本聚合 + 订阅分摊(server.rs) | ~1天 |
| ⑤ | 前端卡片 | ~1天 |

**建议先做 ①(cache 采集)并用单测锁住**,它是唯一有 provider 适配复杂度的部分。

---

## 8. 未决问题 / v2

- 单价来源:手工维护 config,还是从 `providers.json` / 外部价格源同步?
- 参考价 vs 通道议价:PAYG 通道若有折扣(如 OpenRouter 加价、私有折扣),是否需要 `channel.billing` 覆盖实付费率(参考价仍走全局表)。
- 预算与告警:按 team/member 设月度预算上限 + 超支告警(与行为画像告警共用管道)。
- 货币与汇率:多货币上游是否需要统一折算。

---

## 附:术语

`usage_records.team_id` 是**一把 API key = 一个用户(user)**;`Team.group` 是**真实团队(team)**。控制台已按 User / Team 展示,wire 字段名(`team_id` / `group`)保持不变。详见 `cp/src/lib/types.ts` 注释。
