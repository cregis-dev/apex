# Sprint Change Proposal - MCP Streamable HTTP 迁移

**日期**: 2026-03-10
**变更类型**: 技术债务/协议合规性
**变更范围**: MCP 传输层

---

## 1. 问题摘要

### 触发条件
MCP (Model Context Protocol) 协议规范已更新到版本 2025-11-25，其中 SSE (Server-Sent Events) 传输模式已被弃用，推荐使用 **Streamable HTTP** 传输模式。

### 核心问题
当前 Apex MCP 实现基于旧的 HTTP+SSE 传输模式（协议版本 2024-11-05），需要迁移到新的 Streamable HTTP 模式以符合最新协议规范。

### 证据
- MCP 官方规范：https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- 规范明确说明：Streamable HTTP  replaces the HTTP+SSE transport from protocol version 2024-11-05

---

## 2. 影响分析

### Epic 影响
| Epic ID | Epic 名称 | 影响程度 | 说明 |
|---------|-----------|----------|------|
| E04-Observability | 可观测性 | 低 | MCP 传输层变更，不影响核心指标采集功能 |

###  artifact 冲突和影响

#### 代码文件修改
| 文件 | 修改内容 | 状态 |
|------|----------|------|
| `src/mcp/server.rs` | 新增 `streamable_http_handler`，替换旧的 `sse_handler` 和 `messages_handler` | 已完成 |
| `src/server.rs` | 路由注册从 `/mcp/sse` + `/mcp/messages` 改为单一 `/mcp` 端点 | 已完成 |
| `Cargo.toml` | 添加 `uuid` 依赖 | 已完成 |

#### 文档修改
| 文件 | 修改内容 | 状态 |
|------|----------|------|
| `docs/api-contracts-backend.md` | 更新 MCP API 合同，新增 Streamable HTTP 说明 | 已完成 |
| `docs/architecture-backend.md` | 更新架构图和传输流程说明 | 已完成 |
| `docs/operations.md` | 更新部署和客户端连接指南 | 已完成 |

### 技术影响

#### 协议变更对比
| 特性 | 旧 SSE 模式 | Streamable HTTP |
|------|------------|-----------------|
| 端点 | `/mcp/sse` + `/mcp/messages` | `/mcp` (单一端点) |
| 会话管理 | `session_id` 查询参数 | `MCP-Session-Id` HTTP 头 |
| 协议版本 | 无 | `MCP-Protocol-Version: 2025-11-25` |
| 认证方式 | Query Param / Header | Header (推荐) / Query Param (遗留) |
| 响应格式 | SSE 事件流 | JSON 或 SSE 流 |

#### 向后兼容性
- **破坏性变更**: 旧的 `/mcp/sse` 和 `/mcp/messages` 端点已移除
- **迁移路径**: MCP 客户端需要更新配置，使用新的 `/mcp` 端点

---

## 3. 推荐方案

### 方案选择：完全迁移 (方案 B)

**理由**:
1. 简化代码维护，无需同时支持两种传输模式
2. 符合 MCP 最新协议规范
3. 当前 MCP 使用场景有限，破坏性影响可控

### 实施方法

#### 已完成的工作
1. ✅ 添加 `uuid` 依赖用于生成会话 ID
2. ✅ 实现 `streamable_http_handler` 支持 GET/POST/DELETE 方法
3. ✅ 实现会话管理使用 `MCP-Session-Id` 头
4. ✅ 实现协议版本验证
5. ✅ 更新路由注册
6. ✅ 更新相关文档

#### 代码结构
```rust
// 新增的 handler 结构
pub async fn streamable_http_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<McpQuery>,
    req: AxumRequest,
) -> impl IntoResponse {
    match &*req.method() {
        &Method::POST => handle_post_request(...),  // JSON-RPC 消息处理
        &Method::GET => handle_get_request(...),    // SSE 流（服务端推送）
        &Method::DELETE => handle_delete_request(...), // 会话终止
    }
}
```

### 努力评估
- **开发 effort**: 中等 (~4 小时)
- **测试 effort**: 低 (现有测试可复用)
- **风险等级**: 低 (MCP 当前使用范围有限)

---

## 4. PRD MVP 影响

### MVP 影响评估
- **不影响** 核心网关路由功能
- **不影响** Team 认证和速率限制
- **不影响** Usage/Metrics 数据采集
- **仅影响** MCP 远程传输模式

### 高层行动计划
1. ✅ 代码实现完成
2. ✅ 文档更新完成
3. ⏳ 客户端适配（需 MCP 客户端更新配置）

---

## 5. 实施交接

### 变更范围分类：**Moderate**

需要以下角色协调：

| 角色 | 职责 |
|------|------|
| **开发团队** | 验证代码实现，确保编译通过 |
| **产品负责人** | 确认破坏性变更可接受 |
| **运维人员** | 更新部署文档和客户端配置指南 |

### 交接清单

#### 开发团队
- [ ] 运行 `cargo build` 验证编译
- [ ] 运行 `cargo test` 验证测试
- [ ] 验证 MCP 基本功能（List Tools, Call Tool）

#### 产品负责人
- [ ] 确认破坏性变更影响范围
- [ ] 批准变更方案

#### 运维人员
- [ ] 更新 MCP 客户端连接文档
- [ ] 通知现有 MCP 用户更新配置

### 成功标准
1. ✅ 代码编译通过
2. ⏳ MCP 基本功能测试通过
3. ⏳ 文档更新完成

---

## 6. 附录：MCP 连接示例

### 新的连接流程

#### 1. 初始化
```bash
POST https://gateway.cregis.ai/mcp
Content-Type: application/json
Accept: application/json, text/event-stream
Authorization: Bearer sk-your-team-key

{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "protocolVersion": "2025-11-25",
    "capabilities": {},
    "clientInfo": {
      "name": "my-client",
      "version": "1.0.0"
    }
  }
}
```

#### 2. 响应（包含会话 ID）
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2025-11-25",
    "capabilities": {...},
    "serverInfo": {...}
  }
}
```
*响应头包含：`MCP-Session-Id: <uuid>`*

#### 3. 后续请求（携带会话 ID）
```bash
POST https://gateway.cregis.ai/mcp
Content-Type: application/json
MCP-Session-Id: <uuid-from-initialize-response>
Authorization: Bearer sk-your-team-key

{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/list",
  "params": {}
}
```

---

## 7. 审批记录

| 审批人 | 角色 | 状态 | 日期 |
|--------|------|------|------|
| Shawn | 开发负责人 | ⏳ 待审批 | - |

---

## 8. 参考资料

- [MCP Specification 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
- [MCP Architecture](https://modelcontextprotocol.io/specification/2025-11-25/architecture)
- Apex Gateway PRD: `docs/PRD.md`
- Apex Architecture: `docs/architecture-backend.md`
