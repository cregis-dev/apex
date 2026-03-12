# Test Automation Summary

## 测试框架

- **框架**: Rust 内置测试 (#[tokio::test] + #[test])
- **依赖**: assert_cmd, predicates, tempfile, tower
- **测试目录**: `/Users/shawn/workspace/code/apex/tests`

## 已生成的测试

### MCP 协议测试

| 测试文件 | 状态 | 描述 |
|---------|------|------|
| [tests/mcp_session_test.rs](tests/mcp_session_test.rs) | ✅ 通过 | 会话生命周期管理 |
| [tests/mcp_tools_test.rs](tests/mcp_tools_test.rs) | ✅ 通过 | 工具列表与调用 |
| [tests/mcp_resources_test.rs](tests/mcp_resources_test.rs) | ✅ 通过 | 资源列表与读取 |
| [tests/mcp_prompts_test.rs](tests/mcp_prompts_test.rs) | ✅ 通过 | 提示词列表与获取 |

### 其他现有测试

| 测试文件 | 状态 | 描述 |
|---------|------|------|
| [tests/gateway.rs](tests/gateway.rs) | ✅ 通过 | 网关功能测试 |
| [tests/cli.rs](tests/cli.rs) | ✅ 通过 | CLI 命令测试 |
| [tests/system.rs](tests/system.rs) | ✅ 通过 | 系统集成测试 |
| [tests/hot_reload_test.rs](tests/hot_reload_test.rs) | ✅ 通过 | 热重载功能测试 |
| [tests/e04_observability_test.rs](tests/e04_observability_test.rs) | ✅ 通过 | 可观测性测试 |

## 覆盖率

### MCP 功能覆盖率

| 功能 | 测试状态 | 备注 |
|------|---------|------|
| Initialize 请求 | ✅ 已测试 | 返回协议版本和能力声明 |
| Resources/List | ✅ 已测试 | 返回 4 个资源配置 |
| Resources/Read | ✅ 已测试 | 支持 teams/routers/channels/config.json |
| 密钥脱敏 | ✅ 已测试 | mask_secret 正确应用于密钥字段 |
| Prompts/List | ✅ 已测试 | 返回配置的提示词 |
| Prompts/Get | ✅ 已测试 | 参数替换功能正常 |
| Tools/List | ✅ 已测试 | 返回 echo| Tools/ 和 list_models |
Call | ✅ 已测试 | 工具调用和错误处理 |
| 会话生命周期 | ✅ 已测试 | 添加/获取/删除会话 |
| 配置变更通知 | ✅ 已测试 | list_changed 通知广播 |

## 修复的问题

1. **mcp_resources_test 失败**: 修复了 `mask_secret` 函数被错误应用于整个 JSON 内容的问题。添加了 `mask_json_secrets` 函数来递归处理 JSON 并仅掩码密钥字段。

2. **mcp_session_test 编译错误**: 修复了以下问题:
   - 添加了 `sessions()` 公开方法访问会话管理器
   - 修正了 `update_config()` 方法签名 (无参数)
   - 更新了测试以匹配实际 API

## 运行测试

```bash
# 运行所有 MCP 测试
cargo test mcp_

# 运行所有测试
cargo test
```

## 下一步

- 添加更多边界情况测试
- 添加 HTTP/SSE 传输层集成测试
- 添加 E2E 测试验证完整 MCP 流程

---

# Dashboard E2E 回归摘要（2026-03-12）

## 测试框架

- **框架**: Playwright
- **目录**: `/Users/shawn/workspace/code/apex/web/tests`
- **执行命令**: `npx playwright test tests/dashboard.spec.ts`

## 本次回归覆盖

### Dashboard 页面

| 测试文件 | 状态 | 覆盖内容 |
|---------|------|------|
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | 根路径认证入口与空提交校验 |
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | URL `token` 引导、地址清洗、本地存储恢复 |
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | 无效 `token` 回退到连接页并清理本地凭证 |
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | 已存储 token 自动登录与断开连接 |
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | 时间范围、团队、模型筛选与 URL/API 参数同步 |
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | Overview、Team、System、Model、Records 五个 tabs 渲染 |
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | Request ID 复制反馈与 records 详情抽屉 |
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | 翻页后刷新提示新记录并跳回最新页 |
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | 刷新失败 banner 与旧快照保留 |
| [web/tests/dashboard.spec.ts](/Users/shawn/workspace/code/apex/web/tests/dashboard.spec.ts) | ✅ 通过 | CSV 导出文件名与内容 |

## 执行结果

- **通过数**: 10/10
- **失败数**: 0
- **总耗时**: 35.3s

## 备注

- 本次测试全部通过 mocked dashboard API 完成，重点验证前端状态机、URL 状态、交互路径和导出行为。
- 尚未覆盖真实后端联调、跨浏览器矩阵和移动端视口专项回归。

## Dashboard 真实后端联调（2026-03-12）

- **Seed 数据**: 写入 `/tmp/apex-dashboard-integration/data/apex.db`，共 25 条近 24 小时 `usage_records`
- **后端配置**: 使用临时 config 监听 `127.0.0.1:12356`，`web_dir` 指向 `target/web`
- **API 验证**: `GET /api/dashboard/analytics?range=24h` 返回 200，概览口径为 `25 requests / 7450 tokens / 84.0% success rate`
- **页面验证**: `GET /dashboard/` 返回 200，静态资源由真实 Apex 后端提供
- **真实浏览器 smoke**: `RUN_REAL_DASHBOARD_TESTS=true BASE_URL=http://127.0.0.1:12356 DASHBOARD_API_KEY=sk-dashboard-admin-key npx playwright test tests/dashboard.backend.spec.ts --config playwright.real.config.ts`
- **复用脚本**:
  - `scripts/dashboard/setup_real_backend_fixture.sh`
  - `scripts/dashboard/run_real_backend_smoke.sh`
- **执行结果**: 2/2 通过
