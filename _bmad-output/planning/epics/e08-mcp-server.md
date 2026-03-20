# Epic 08: MCP Server Operations & Analytics

## Description
为 MCP Server 提供协议接入、管理与统计能力，覆盖生命周期管理、资源列表与安全脱敏、基础统计报表，以及与现有网关的一致鉴权与接入方式。

## Stories

- [ ] **S01: MCP 协议与传输支持**
  - Scope:
    - JSON-RPC 2.0 请求/响应/通知处理。
    - stdio 与 HTTP/SSE 传输通道。
    - 与现有网关鉴权机制的对齐接入。
  - Acceptance criteria:
    - 支持 initialize 请求并返回服务端能力声明。
    - stdio 与 HTTP/SSE 均可完成基础调用链路。
    - 错误返回遵循 JSON-RPC 2.0 错误结构。

- [ ] **S02: 会话生命周期与能力协商**
  - Scope:
    - 会话状态机与超时管理。
    - 能力注册表与版本化能力声明。
    - 能力变更通知与订阅机制。
  - Acceptance criteria:
    - 会话在超时或关闭时可正确清理资源。
    - 能力变更通知可被客户端接收并更新本地能力缓存。

- [ ] **S03: 管理与控制面能力**
  - Scope:
    - 管理 API 提供启停与配置更新。
    - 变更审计记录与查询。
    - 配置变更回滚机制。
  - Acceptance criteria:
    - 每次变更均记录操作人、时间与变更内容。
    - 回滚可恢复到最近一次稳定配置。

- [ ] **S04: 资源列表与密钥脱敏**
  - Scope:
    - team/router/channel 的列表与查询接口。
    - key 脱敏规则与输出一致性。
  - Acceptance criteria:
    - 列表接口返回稳定分页顺序。
    - key 仅展示前缀与后四位，其他字符脱敏。

- [ ] **S05: 统计分析与报表查询**
  - Scope:
    - 请求量、错误率、延迟分位、成本指标的聚合。
    - 按时间、团队、路由、模型维度查询。
    - CSV/JSON 导出能力。
  - Acceptance criteria:
    - 查询支持时间范围与维度过滤。
    - 导出结果与查询结果一致。
