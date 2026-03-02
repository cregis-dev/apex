# Story 8.1: MCP 协议与传输支持 (MCP Protocol & Transport Support)

Status: completed

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a 开发者,
I want 能够通过 stdio 或 HTTP/SSE 协议与 MCP Server 通信,
so that 我可以使用 Claude Desktop 或其他 MCP 客户端连接并使用 Apex 的能力.

## Acceptance Criteria

1.  **Transport Support**:
    *   支持 `stdio` 传输模式（默认），通过 stdin/stdout 进行 JSON-RPC 交互。
    *   支持 `http-sse` 传输模式，提供 `/mcp/sse` 和 `/mcp/messages` 端点。
2.  **Protocol Compliance**:
    *   遵循 JSON-RPC 2.0 规范（Request, Response, Notification, Error）。
    *   能够正确处理 `initialize` 请求，并返回服务器能力声明（Capabilities）。
    *   能够正确处理 `initialized` 通知。
3.  **Integration**:
    *   在 `apex` CLI 中增加 `mcp` 子命令启动服务。
    *   支持与现有配置系统集成（加载 `config.json`）。

## Tasks / Subtasks

- [x] CLI 集成
    - [x] 在 `src/main.rs` 中添加 `Mcp` 子命令。
    - [x] 实现 `apex mcp start` 命令入口。
- [x] 协议层实现 (`src/mcp/protocol.rs`)
    - [x] 定义 JSON-RPC 2.0 数据结构 (Request, Response, Error)。
    - [x] 实现消息序列化与反序列化。
- [x] 传输层实现 (`src/mcp/transport.rs`)
    - [x] 实现 `StdioTransport`：基于 tokio 的 stdin/stdout 读写循环。
    - [x] 实现 `SseTransport`：基于 Axum 的 SSE 处理（可选，优先 stdio）。
- [x] 核心服务实现 (`src/mcp/server.rs`)
    - [x] 实现 `initialize` 方法处理。
    - [x] 实现 `notifications/initialized` 处理。
    - [x] 搭建请求分发循环。

## Dev Notes

- 使用 `serde_json` 处理 JSON。
- 使用 `tokio` 处理异步 IO。
- 参考 [MCP Specification](https://modelcontextprotocol.io/specification) 关于 `initialize` 的定义。
- 优先完成 `stdio` 模式，这是 Claude Desktop 的主要连接方式。

### Project Structure Notes

- 新增模块 `src/mcp/`。
- 修改 `src/lib.rs` 暴露 `mcp` 模块。
- 修改 `Cargo.toml` (如果需要额外依赖，目前看 `serde_json` 和 `tokio` 已足够)。

### References

- [E08-MCP-Server.md](file:///Users/shawn/workspace/code/apex/docs/epics/E08-MCP-Server.md)
