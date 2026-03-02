# Story 8.5: MCP Prompts 支持 (MCP Prompts Support)

Status: done

## Story

As a MCP 客户端用户,
I want 通过 MCP 协议访问 Apex 提供的 Prompt 模板,
so that 我可以在 IDE 中直接使用预定义的 Prompts (例如 "Explain Team Policy", "Review Config").

## Acceptance Criteria

1.  **Config Support**:
    *   在 `config.json` 中支持 `prompts` 配置项。
    *   允许定义 Prompt 的名称、描述、参数和模板内容。
2.  **Prompts List**:
    *   实现 `prompts/list` 接口。
    *   返回配置中定义的所有 Prompts 及其元数据 (arguments)。
3.  **Prompts Get**:
    *   实现 `prompts/get` 接口。
    *   根据 Prompt 名称和参数，渲染并返回消息列表 (Messages)。
    *   支持简单变量替换 (e.g. `{{ arg_name }}`).

## Configuration Example

```json
{
  "prompts": [
    {
      "name": "explain-policy",
      "description": "Explain the policy for a specific team",
      "arguments": [
        {
          "name": "team_id",
          "description": "The ID of the team",
          "required": true
        }
      ],
      "messages": [
        {
          "role": "user",
          "content": {
            "type": "text",
            "text": "Please explain the policy for team {{ team_id }}."
          }
        }
      ]
    }
  ]
}
```

## Tasks / Subtasks

- [x] 配置扩展 (`src/config.rs`)
    - [x] 定义 `Prompt` 和 `PromptArgument` 结构体。
    - [x] 在 `Config` 中添加 `prompts` 字段。
- [x] 协议定义 (`src/mcp/protocol.rs`)
    - [x] 定义 `Prompt`, `PromptMessage`, `GetPromptResult`, `ListPromptsResult` 等类型。
- [x] 处理器实现 (`src/mcp/server.rs`)
    - [x] 实现 `handle_prompts_list`.
    - [x] 实现 `handle_prompts_get`.
    - [x] 实现简单的模板替换逻辑。
- [x] 测试
    - [x] 单元/集成测试：验证配置加载。
    - [x] 单元/集成测试：验证 `prompts/list` 和 `prompts/get`。
