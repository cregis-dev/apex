# Epic 06: Team Governance & Multi-Tenancy

## 目标 (Goal)
实现“团队级”的多租户管理能力，支持基于 Team ID 的鉴权、限流（Rate Limiting）和配额（Budget Quotas）管理。

## 核心设计原则 (Design Principles)
1.  **配置驱动 (Configuration-Driven)**: 团队配置定义在 `config.json` 中。
2.  **单一密钥 (Single Key)**: 每个 Team 只有一个 API Key，自动生成，格式兼容 Anthropic (`sk-ant-xxxxx`)。
3.  **安全默认 (Secure Defaults)**: 必须显式配置允许访问的 Routers，否则无法访问任何资源。

## 功能需求 (Requirements)

### 1. 团队定义 (Team Definition)
*   **ID**: 唯一标识符 (e.g., `marketing-bot`).
*   **API Key**: 自动生成的唯一密钥 (e.g., `sk-ant-a1b2c3...`).
*   **Policy**:
    *   `allowed_routers` (Required): 允许访问的 Router 列表。若为空，则无法访问。
    *   `allowed_models` (Optional): 允许访问的模型列表。若为空或不配置，则允许该 Router 下的所有模型。
    *   `rate_limit` (Optional): RPM/TPM 限制。若不配置或设为负数，则不限制。

### 2. 鉴权逻辑 (Authentication)
*   网关接收请求时，提取 `Authorization: Bearer <key>` 或 `x-api-key: <key>`。
*   在内存中匹配 Key -> Team。
*   若 Key 无效，返回 401 Unauthorized。

### 3. 访问控制 (Access Control)
*   **Router Check**: 请求的 Router 必须在 `allowed_routers` 列表中。否则返回 403 Forbidden。此检查在路由解析阶段执行。
*   **Model Check**: 请求的 Model (从 Body 解析) 必须在 `allowed_models` 列表中 (如果配置了该列表)。否则返回 403 Forbidden。此检查在路由解析阶段执行。

### 4. 限流 (Rate Limiting)
*   基于 Token Bucket 算法。
*   仅当 `rate_limit` 配置且值 > 0 时生效。
*   在 Middleware 层执行 (RPM)。TPM 估算基于请求体大小或响应 token 计数。

## 配置示例 (Configuration Example)

```json
{
  "teams": [
    {
      "id": "marketing-bot",
      "api_key": "sk-ant-123456...", 
      "policy": {
        "allowed_routers": ["default-openai"],
        "rate_limit": {
          "rpm": 60,
          "tpm": 100000
        }
      }
    },
    {
      "id": "admin-team",
      "api_key": "sk-ant-admin...",
      "policy": {
        "allowed_routers": ["*"] // Special case: allow all
      }
    }
  ]
}
```

## 任务拆分 (Task Breakdown)
1.  **Config Schema**: Update `config.rs` to include `Team` struct. (Completed)
2.  **Key Generation**: Implement utility to generate `sk-ant-` keys. (Completed)
3.  **CLI**: Add `apex team add` command. (Completed)
4.  **Middleware**: Implement Auth and Policy middleware. (Completed)
