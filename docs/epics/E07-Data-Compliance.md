# Epic 07: Data Compliance & PII Masking

## 目标 (Goal)
为企业团队提供数据安全保障，防止敏感信息（PII - Personally Identifiable Information）泄露给外部 LLM 提供商。

## 核心功能 (Core Features)

### 1. PII 识别与脱敏 (PII Detection & Masking)
*   **规则引擎**: 基于正则表达式 (Regex) 的匹配规则。
*   **内置规则**: 预置常见 PII 模式：
    *   Email
    *   Phone Number
    *   Credit Card Number
    *   IP Address
*   **自定义规则**: 允许用户在配置中添加自定义 Regex。
*   **脱敏策略**:
    *   `Mask`: 替换为 `***` 或 `[EMAIL]`.
    *   `Hash`: 替换为哈希值（用于保持上下文一致性，但在 LLM 场景下较少用）。
    *   `Block`: 发现敏感信息直接拒绝请求。

### 2. 审计日志 (Audit Logging)
*   记录触发脱敏的请求日志（记录“检测到 Email 并已脱敏”，不记录原始敏感数据）。

## 配置示例 (Configuration Example)

```json
{
  "compliance": {
    "enabled": true,
    "pii_rules": [
      {
        "name": "email",
        "pattern": "(?i)[a-z0-9._%+-]+@[a-z0-9.-]+\\.[a-z]{2,}",
        "action": "mask", // mask | block
        "mask_char": "*"  // jdoe@example.com -> ****@*******.**
      },
      {
        "name": "credit_card",
        "pattern": "\\d{4}-?\\d{4}-?\\d{4}-?\\d{4}",
        "action": "replace",
        "replace_with": "[CREDIT_CARD]"
      }
    ]
  }
}
```

## 技术实现 (Implementation Details)

### 1. 请求处理流程
*   在请求转发给 Provider **之前**，解析请求体 (Request Body)。
*   对于 Chat Completions API，遍历 `messages` 中的 `content` 字段。
*   对 `content` 应用正则替换。
*   重新序列化 Body 并更新 Content-Length。

### 2. 性能考量
*   Regex 匹配会有性能开销。
*   **优化**:
    *   预编译 Regex (`lazy_static` 或 `once_cell`).
    *   仅对文本类型的字段进行扫描。
    *   提供全局开关。

## 任务拆分 (Task Breakdown)
1.  **Config Schema**: 添加 `Compliance` 配置。
2.  **PII Processor**: 实现正则匹配与替换逻辑。
3.  **Request Middleware**: 拦截请求，解析 JSON，应用 PII Processor，重组请求。
4.  **Audit Logs**: 记录脱敏事件。
