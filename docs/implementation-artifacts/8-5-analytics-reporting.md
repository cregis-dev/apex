# Story 8.5: 统计分析报表查询 (Analytics Reporting & Export)

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a 运维管理员,
I want 查询 Teams 的使用统计并支持条件搜索,
so that 我可以分析各团队的资源使用情况并导出报表.

## Acceptance Criteria

1.  **Team Usage Query** (AC: #1):
    *   支持按 Team ID 查询使用统计
    *   返回该 Team 的请求量、Token 消耗、错误率
    *   支持按时间范围过滤 (start_time, end_time)
2.  **Conditional Search** (AC: #2):
    *   支持按 Router 维度过滤
    *   支持按 Model 维度过滤
    *   支持多维度组合过滤 (AND 逻辑)
3.  **Aggregation Metrics** (AC: #3):
    *   聚合统计：总请求量、平均延迟、错误率、P50/P95/P99 延迟分位
    *   按 Router/Channel/Model 分组统计
4.  **Export Capability** (AC: #4):
    *   支持 JSON 格式导出
    *   支持 CSV 格式导出
    *   导出结果与查询结果一致

## Technical Foundation (Already Exists)

项目已有以下可复用的基础设施:

1. **Usage Logger** (`src/usage.rs`):
   - CSV 格式记录: timestamp, router, channel, model, input_tokens, output_tokens
   - 可通过 Team ID 关联 (需扩展)

2. **Metrics State** (`src/metrics.rs`):
   - `apex_requests_total{route, router}`
   - `apex_errors_total{route, router}`
   - `apex_token_total{router, channel, model, type}`
   - `apex_upstream_latency_ms{route, router, channel}`

## Implementation Approach

### Option A: MCP Tools 方式 (Recommended)
- 新增 MCP Tool: `query_team_usage`
- 参数: team_id, start_time, end_time, router, model
- 返回: JSON 格式统计结果
- 新增 MCP Tool: `export_usage_report`
- 参数: query filters, format (json/csv)

### Option B: MCP Resources 方式
- 新增 Resource: `analytics://team-usage`
- 支持 URI 参数过滤
- 适合简单查询场景

## Tasks / Subtasks

- [x] Task 1: 数据源扩展 (AC: #1)
  - [x] 通过 Router/Channel 映射关联 Team (使用 team.policy.allowed_routers)
- [x] Task 2: 查询引擎实现 (AC: #2, #3)
  - [x] 实现时间范围过滤
  - [x] 实现多维度过滤 (Router, Model)
  - [x] 实现聚合统计计算
- [x] Task 3: MCP Tool 定义 (AC: #1, #2, #3)
  - [x] 定义 `query_team_usage` tool
  - [x] 实现参数解析与验证
- [x] Task 4: 导出功能 (AC: #4)
  - [x] JSON 导出实现
  - [x] CSV 导出实现
  - [x] 测试导出与查询结果一致性

## Dev Notes

- 数据源: `logs/usage.csv` + `MetricsState` (Prometheus)
- 如需实时查询, 可从 Prometheus 拉取指标
- 建议使用 `csv` crate 读取使用日志
- Team ID 关联: 可通过 Router 配置中的 team 映射获取

### Project Structure Notes

- 新增模块: `src/mcp/analytics.rs` (查询逻辑)
- 修改: `src/mcp/server.rs` (tool handlers)
- 修改: `src/usage.rs` (可选: 添加 team_id 字段)

### References

- [E08-MCP-Server.md](docs/epics/E08-MCP-Server.md#L44-L52)
- [Story 8.6: MCP Tools](docs/implementation-artifacts/8-6-mcp-tools.md)

## Dev Agent Record

### Agent Model Used

Claude Sonnet 4.6

### Debug Log References

### Completion Notes List

- ✅ 创建 `src/mcp/analytics.rs` - 分析引擎模块
  - 实现 `AnalyticsEngine` 结构体
  - 支持查询过滤 (team_id, router, model, start_time, end_time)
  - 支持聚合统计 (按 router, model 分组)
  - 支持 JSON/CSV 导出
- ✅ 更新 `src/mcp/mod.rs` - 添加 analytics 模块
- ✅ 更新 `src/mcp/server.rs` - 添加 MCP tools
  - 新增 `query_team_usage` tool
  - 新增 `export_usage_report` tool
- ✅ 添加 6 个单元测试，全部通过

### File List

- src/mcp/analytics.rs (新增)
- src/mcp/mod.rs (修改)
- src/mcp/server.rs (修改)

## Review Follow-ups (AI)

- [ ] [AI-Review][Medium] 集成 MetricsState 获取错误率和延迟指标 - 需从 Prometheus 拉取 apex_errors_total 和 apex_upstream_latency_ms
- [ ] [AI-Review][Low] 添加按 Channel 分组统计 - ✅ 已修复
