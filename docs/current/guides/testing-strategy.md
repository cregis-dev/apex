---
name: "Apex Gateway 测试架构（2026）"
version: "1.0"
date: "2026-03-17"
mode: "system-level"
---

# Apex Gateway 测试架构（2026）

## 1. 目标

本测试架构面向 Apex 这类由 AI 高参与开发、协议兼容要求高、路由规则复杂的网关项目。

设计目标：

- 把稳定行为固化为契约，降低 AI 驱动重构带来的隐性回归风险
- 把不稳定外部依赖隔离到最小范围，保证本地测试可重复、可快速执行
- 让本地开发、CI、真实 provider 验证三种场景使用同一套测试分层与配置模型
- 优先验证用户真实感知到的行为，而不是依赖内部实现细节

## 2. 为什么 Apex 不能只靠传统测试金字塔

Apex 是一个协议网关，而不是单纯的业务 CRUD 服务。它的核心风险集中在以下几个方面：

- OpenAI / Anthropic / MCP 等协议兼容性可能在重构中悄悄漂移
- 路由、fallback、限流、team policy、timeout、retry 等规则组合非常多
- provider adapter 对请求和响应的转换逻辑容易被局部修改破坏
- 真实 provider 有外网、费用、限流和可用性波动，不能作为默认回归基础

因此，Apex 的测试重点应该是：

- 重契约
- 重规则矩阵
- 重本地确定性全链路
- 轻真实外部依赖

## 3. 分层架构

Apex 推荐采用六层测试架构：

```text
L5  Real Provider Smoke
L4  Regression Baselines
L3  Local Full-Stack Blackbox
L2  In-Process Integration
L1  Provider Adapter Contracts
L0  Config and Rule Matrix
```

### L0: Config and Rule Matrix

目标：验证配置解析、默认值、规则匹配和策略判定，不依赖真实网络。

覆盖范围：

- `config` 解析、校验、默认值、坏配置报错
- `router_selector` 的 priority / round_robin / weighted / fallback
- team policy 的 allowed routers / allowed models / rate limit
- compliance 规则判定
- timeout / retry / fallback 选择条件

设计原则：

- 尽量使用表驱动测试
- 尽量让断言聚焦行为而非内部实现
- 尽量把边界值和组合值显式列出来

### L1: Provider Adapter Contracts

目标：验证每个 provider adapter 的请求转换和响应转换契约。

覆盖范围：

- 路径映射
- query 处理
- auth header 注入
- `model_map`
- OpenAI / Anthropic body conversion
- streaming 响应透传或转换

设计原则：

- 使用本地 mock upstream
- 不依赖真实 provider
- 每个 adapter 的契约单独可测

### L2: In-Process Integration

目标：验证网关应用内部组件组合后的行为，但仍在进程内完成。

覆盖范围：

- `build_state` / `build_app`
- auth middleware
- rate limit middleware
- metrics endpoint
- MCP prompts / tools / resources / session
- usage 记录与数据存储

设计原则：

- 复用统一测试支撑代码
- 保持运行速度快，适合 `cargo test` 默认执行

### L3: Local Full-Stack Blackbox

目标：从用户视角验证完整链路，但 upstream 仍然是本地可控 mock。

链路：

- 官方 SDK 或标准 HTTP client
- Apex 进程启动
- 配置加载
- 路由选择
- provider adapter
- mock upstream
- Apex 响应回写
- 客户端解析

必须覆盖的最小场景：

- OpenAI chat completions 非流式
- OpenAI chat completions 流式
- Anthropic messages 非流式
- Anthropic messages 流式
- `/v1/models`
- fallback
- hot reload
- metrics 和 usage smoke

设计原则：

- 默认可在本地离线运行
- 默认 deterministic
- 可作为 CI 主回归链路

### L4: Regression Baselines

目标：把关键响应结构和事件序列固化为基线，防止接口悄悄漂移。

建议固化的内容：

- JSON 响应结构
- 错误响应结构
- SSE streaming 事件序列
- metrics 输出中的关键字段

设计原则：

- 仅为高价值、易漂移接口建立 baseline
- fixture 需可读、可审查

当前仓库已实现的第一批 baseline：

- `tests/fixtures/regression/openai_chat_success.json`
- `tests/fixtures/regression/openai_error_upstream_500.json`
- `tests/fixtures/regression/anthropic_messages_stream.sse`

对应校验用例位于 `tests/e2e_regression_baseline_test.rs`。

### L5: Real Provider Smoke

目标：验证用户按 `.env` 填写真实 provider 后，Apex 能实际连通并完成最核心操作。

建议仅覆盖：

- 每个启用 provider 的 1 个非流式请求
- 每个启用 provider 的 1 个流式请求
- 1 个模型列表请求
- 1 个失败路径验证，例如错误 key、上游 5xx 或 timeout

设计原则：

- 必须 opt-in
- 不作为默认离线回归的一部分
- 只测可用性，不做全面业务回归

## 4. Harness 设计

为避免测试脚本与真实配置结构漂移，建议引入统一 harness。

建议目录：

```text
tests/
  harness/
    env.rs
    config_builder.rs
    gateway_process.rs
    mock_provider.rs
    fixtures/
  e2e/
    sdk_openai.py
    sdk_anthropic.py
    scenarios/
```

各组件职责：

- `env.rs`
  - 读取 `.env.e2e`
  - 校验字段完整性
  - 生成强类型测试输入

- `config_builder.rs`
  - 复用 Apex 自己的 `Config` 结构生成 `.run/e2e/generated.e2e.config.json`
  - 统一生成 `teams`、`routers`、`channels`、`metrics`、`hot_reload`

- `gateway_process.rs`
  - 启动 / 停止 Apex 进程
  - 等待服务端口就绪
  - 收集 stdout / stderr 日志

- `mock_provider.rs`
  - 提供成功、429、500、timeout、malformed JSON、streaming 等 upstream 行为

- `sdk_openai.py` 与 `sdk_anthropic.py`
  - 保持“真实用户客户端”视角
  - 不负责手写配置结构
  - 只负责发请求和断言协议体验

## 5. 配置策略

### 5.1 推荐原则

- `.env.e2e` 只承载用户输入
- 最终 `config.json` 由 harness 生成
- 测试代码不手写网关配置 schema

### 5.2 推荐原因

当前仓库中，`config.example.json` 已经出现 `${OPENAI_API_KEY}` 这类占位符，但 `load_config()` 仍然是直接 JSON 解析，并未内建 env 展开能力。与此同时，现有 Python E2E 仍混用旧字段模型，已经与当前 `Config` 结构产生漂移。

因此，推荐路径不是继续让 Python 直接拼 JSON，而是：

- Rust 负责生成正确配置
- Python 负责真实 SDK 客户端行为

当前 harness 的默认生成约定：

- 第 1 个启用 upstream 会进入主规则的 `channels`
- 第 2 个及之后的启用 upstream 会自动写入 `fallback_channels`
- 真实 smoke 会根据生成后 `channels[].provider_type` 自动决定运行 OpenAI、Anthropic 或双协议验证

### 5.3 `.env.e2e` 建议字段

```dotenv
APEX_E2E_LISTEN=127.0.0.1:12356
APEX_E2E_TEAM_ID=e2e-team
APEX_E2E_TEAM_KEY=sk-apex-e2e-team
APEX_E2E_ADMIN_KEY=sk-apex-e2e-admin
APEX_E2E_ROUTER_NAME=e2e-default
APEX_E2E_ROUTER_STRATEGY=priority
APEX_E2E_TEST_MODEL=apex-test-chat

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=openai_primary
APEX_UPSTREAM_1_TYPE=openai
APEX_UPSTREAM_1_BASE_URL=https://api.openai.com/v1
APEX_UPSTREAM_1_API_KEY=
APEX_UPSTREAM_1_MODEL_MAP_JSON={"apex-test-chat":"<user-fill-real-model>"}

APEX_UPSTREAM_2_ENABLED=false
APEX_UPSTREAM_2_NAME=anthropic_fallback
APEX_UPSTREAM_2_TYPE=anthropic
APEX_UPSTREAM_2_BASE_URL=https://api.anthropic.com
APEX_UPSTREAM_2_API_KEY=
APEX_UPSTREAM_2_MODEL_MAP_JSON={"apex-test-chat":"<user-fill-real-model>"}
```

## 6. 执行入口

建议把执行方式显式拆分：

```bash
# L0-L2
cargo test

# L3
./scripts/test-local-e2e.sh

# L5
./scripts/test-real-smoke.sh

# 默认全套（不含真实 provider）
./scripts/test-all.sh

# 全套 + 真实 provider 冒烟
./scripts/test-all.sh --real
```

当前仓库已实现的入口：

- `cargo test --test e2e_harness_test --test e2e_local_blackbox_test`
- `./scripts/test-local-e2e.sh`
- `./scripts/test-real-smoke.sh`
- `./scripts/test-all.sh`
- `cargo run --bin apex-e2e-config -- --env-file .env.e2e --output .run/e2e/generated.e2e.config.json`

其中：

- `./scripts/test-local-e2e.sh` 会同时执行本地黑盒和 regression baseline
- `./scripts/test-real-smoke.sh` 复用同一套配置生成链路，只把 upstream 切到用户填写的真实 provider
- `tests/e2e/test_chat_cli.py --mode auto` 会按生成配置自动识别需要验证的协议，而不是硬编码全部都跑

当前本地黑盒已覆盖：

- OpenAI chat completions
- Anthropic messages 与 streaming
- `/v1/models`
- fallback
- metrics / usage smoke
- hot reload channel 切换

执行约束：

- `cargo test` 必须默认离线、稳定、可重复
- 真实 provider 冒烟必须显式开启
- 任何需要费用或公网的测试都不得作为默认回归前提

## 7. 迁移原则

迁移现有测试时，采用“分层迁移而非全部重写”：

- Rust 规则层与进程内集成测试优先保留
- 重复的 config 和 upstream 构造逐步沉淀到统一 harness
- Python E2E 保留“官方 SDK 做客户端”的价值，但重写配置来源和启动方式
- 手写旧 schema 的脚本不再继续扩展

## 8. 当前仓库测试资产的处理原则

### 8.1 优先保留并增强

- `tests/common/mod.rs`
- `tests/gateway.rs`
- `tests/system.rs`
- `tests/e05_routing_test.rs`
- `tests/hot_reload_test.rs`
- `tests/mcp_*`
- `tests/team_test.rs`
- `tests/e04_observability_test.rs`
- `tests/e07_compliance_test.rs`

这些测试整体符合 L0-L3 的方向，主要问题不是思路错误，而是：

- 重复构造较多
- 场景覆盖仍不够矩阵化
- 缺少统一 harness 复用

### 8.2 建议重构而非继续补丁

- `tests/e2e/run_e2e.py`
- `tests/e2e/test_chat_cli.py`
- `tests/e2e/test_router_strategy.py`

这些脚本仍在承载旧配置模型或手写配置行为，容易继续与真实代码漂移。

## 9. 第一阶段落地计划

### Phase 1

- 建立 `tests/harness`
- 建立 `.env.e2e.example`
- 建立 config builder
- 统一启动 / 停止 Apex 与 mock provider

### Phase 2

- 重构 Python E2E，使其只负责官方 SDK 客户端行为
- 跑通 OpenAI / Anthropic 的本地黑盒链路
- 补齐 `/v1/models`、fallback、hot reload smoke

### Phase 3

- 增加 regression baselines
- 增加真实 provider smoke
- 将执行入口脚本纳入 CI 分层执行

## 10. 结论

对于 Apex 这样的 AI API Gateway，最佳测试架构不是单纯堆积单元测试或 E2E，而是：

- 用 L0-L2 稳住规则和契约
- 用 L3 建立本地可重复的全链路回归
- 用 L4 防止接口结构漂移
- 用 L5 验证真实 provider 接入可用性

一句话总结：

`规则层很厚 + 契约层很稳 + 本地全链路可重复 + 真实 provider 只做最小冒烟`
