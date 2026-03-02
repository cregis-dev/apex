# Story 8.6: MCP Tools Support

Status: completed

## Story

As a MCP Client User,
I want to execute tools provided by the Apex Gateway,
so that I can perform diagnostic tasks or retrieve dynamic information.

## Acceptance Criteria

1.  **Tools List**:
    *   Implement `tools/list` interface.
    *   Return a list of available tools (e.g., `echo`, `list_models`).
2.  **Tools Call**:
    *   Implement `tools/call` interface.
    *   Execute the requested tool and return the result.
    *   Handle errors gracefully (e.g., tool not found, invalid arguments).
3.  **Built-in Tools**:
    *   `echo`: Returns the input argument `message`.
    *   `list_models`: Returns a list of all models available across all configured channels.

## Tasks / Subtasks

- [x] Protocol Definition (`src/mcp/protocol.rs`)
    - [x] Define `Tool`, `CallToolResult`, `ListToolsResult`, `CallToolRequest` types.
- [x] Handler Implementation (`src/mcp/server.rs`)
    - [x] Implement `handle_tools_list`.
    - [x] Implement `handle_tools_call`.
    - [x] Implement `echo` tool logic.
    - [x] Implement `list_models` tool logic.
- [x] Testing
    - [x] Unit/Integration tests for `tools/list` and `tools/call`.
