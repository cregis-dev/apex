# Story 8.4: 资源列表与密钥脱敏

Status: completed

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a 运维管理员,
I want 查询 team/router/channel 的列表且密钥被脱敏,
so that 能安全审计与排查配置问题而不暴露敏感信息.

## Acceptance Criteria

1. 列表接口返回稳定分页顺序。
2. key 仅展示前缀与后四位，其他字符脱敏。

## Tasks / Subtasks

- [x] 资源列表接口与鉴权对齐 (AC: #1)
  - [x] 确保 /admin/teams、/admin/routers、/admin/channels 需要全局鉴权
  - [x] 响应结构保持 object=list 与 data 数组
- [x] 密钥脱敏规则实现 (AC: #2)
  - [x] 保留前缀与后四位，其他字符脱敏
  - [x] 短 key（长度 ≤ 7）全部脱敏
  - [x] 统一应用于 team.api_key 与 channel.api_key
- [x] 测试覆盖 (AC: #1, #2)
  - [x] 更新 admin_list_masks_keys 断言为后四位
  - [x] 增加短 key 脱敏用例或覆盖现有最短路径

## Dev Notes

- 管理接口与脱敏逻辑集中在 [server.rs](file:///Users/shawn/workspace/coding/apex/src/server.rs)。
- admin 列表测试集中在 [gateway.rs](file:///Users/shawn/workspace/coding/apex/tests/gateway.rs)。
- 项目测试入口为 cargo test，涉及路由/鉴权/关键流程需有端到端测试覆盖。
- Rust 版本为 edition 2024。

### Project Structure Notes

- 服务器路由注册位于 src/server.rs::build_app。
- admin handler 使用 enforce_global_auth 进行鉴权。
- 脱敏函数 mask_secret 在 src/server.rs。

### References

- [E08-MCP-Server.md](file:///Users/shawn/workspace/coding/apex/_bmad-output/planning/epics/e08-mcp-server.md#L36-L43)
- [project-development-constraints.md](file:///Users/shawn/workspace/coding/apex/.trae/rules/project-development-constraints.md#L15-L20)

## Dev Agent Record

### Agent Model Used

GPT-5.2-Codex

### Debug Log References

### Completion Notes List

### File List
