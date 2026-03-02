# Story 8.3: MCP 资源支持 (MCP Resources Support)

Status: completed

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a MCP 客户端用户,
I want 通过 MCP 协议访问 Apex 的配置资源 (Teams, Routers, Channels),
so that 我可以在 IDE 或其他 MCP 客户端中直接查看当前配置。

## Acceptance Criteria

1.  **Resources List**:
    *   实现 `resources/list` 接口。
    *   返回 `config://teams`, `config://routers`, `config://channels` 等资源。
    *   支持 `config://config.json` (完整配置)。
2.  **Resources Read**:
    *   实现 `resources/read` 接口。
    *   根据 URI 返回对应的配置内容（JSON 格式）。
    *   **必须应用密钥脱敏规则** (复用 Story 8.4 的逻辑)。
3.  **Updates**:
    *   当配置变更时，触发 `notifications/resources/list_changed` (已在 Story 8.2 实现，需验证集成)。

## Tasks / Subtasks

- [x] 资源处理器实现 (`src/mcp/server.rs`)
    - [x] 实现 `handle_resources_list`: 返回可用资源列表。
    - [x] 实现 `handle_resources_read`: 解析 URI 并返回脱敏后的配置数据。
- [x] 密钥脱敏逻辑复用
    - [x] 确保使用统一的脱敏逻辑 (Refactor `mask_secret` if needed to be accessible from MCP module).
- [x] 测试
    - [x] 单元/集成测试：验证 `resources/list` 返回预期资源。
    - [x] 单元/集成测试：验证 `resources/read` 返回脱敏数据。
    - [x] 验证 `config://config.json` 读取。

## Dev Notes

- 资源 URI 方案:
    - `config://teams`
    - `config://routers`
    - `config://channels`
    - `config://global`
- 数据源: `self.config` (Arc<RwLock<Config>>).
- 脱敏: 需确保不返回明文 API Key。

### References

- [E08-MCP-Server.md](file:///Users/shawn/workspace/code/apex/docs/epics/E08-MCP-Server.md)
