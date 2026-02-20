# 简易 AI Gateway PRD

## 背景与目标
- 企业内部统一出入口，屏蔽多家模型差异
- Rust 实现的轻量单体服务，配置驱动
- 支持热加载、路由 fallback、超时/重试、指标导出

## 范围
- 仅支持 JSON 配置文件
- 无 Web UI，仅 CLI 管理
- 支持 OpenAI/Anthropic 兼容入口与 Proxy 转发

## Provider 支持范围
- OpenAI
- Anthropic
- Gemini
- Deepseek
- moonshot
- minimax
- ollama
- jina
- OpenRouter
- 不支持：Azure OpenAI

## 用户与场景
- 平台管理员：通过 CLI 管理 channel/router 和配置
- 业务调用方：通过 vkey 请求 OpenAI/Anthropic 兼容接口
- 代理转发：使用 proxy 路由做原样转发

## 路由类型与对外路径
- OpenAI 兼容
  - Base URL：`https://<gateway-host>:12356`
  - 入口：`/v1/chat/completions`、`/v1/completions`、`/v1/embeddings`、`/v1/models`
- Anthropic 兼容
  - Base URL：`https://<gateway-host>:12356`
  - 入口：`/v1/messages`
- Proxy
  - Base URL：`https://<gateway-host>:12356`
  - 入口：`/proxy/{router_name}/*`

## 鉴权与密钥
- Router vkey 用于终端用户访问（OpenAI/Anthropic 入口）
- vkey 通过标准 Authorization (Bearer) 或 x-api-key 传入
- global.auth 用于 Gateway 入口鉴权，使用 Authorization (Bearer) 或 x-api-key
- Provider key 仅用于上游访问，保存在 channel 中

## 配置结构（合并版）
- providers 与 channels 合并为 channels
- 每个 channel 包含 provider_type、base_url、api_key、model_map、headers、timeouts
- routers 绑定 channel，可设置 fallback_channels
- global 包含 listen/auth/timeouts/retries
- metrics 包含 enabled/listen/path
- hot_reload 包含 config_path/watch

## 默认配置（init）
- listen：`0.0.0.0:12356`
- auth：none
- timeouts：connect 2000ms / request 30000ms / response 30000ms
- retries：max_attempts 2 / backoff_ms 200 / retry_on_status 429、500、502、503、504
- metrics：`0.0.0.0:9090` + `/metrics`
- hot_reload：watch true

## CLI 功能
- apex init 支持无参数默认向导
  - 默认配置路径：`~/.apex/config.json`
  - 默认监听：`0.0.0.0:12356`
  - 默认鉴权：none
- channel 与 router 支持 add/update/delete/list
- channel add 时 provider 采用选择式
- list 支持 `--json` 输出

## 请求处理与策略
- 路由匹配：按路由类型与 vkey / router_name 定位 router
- 转发顺序：主 channel → fallback_channels
- 单 channel 重试：同一 channel 内按 max_attempts 重试
- fallback 触发：当状态码命中 retry_on_status 且重试耗尽后进入下一个 channel
- 超时策略：connect / request / response 三级超时
- 头部透传：默认过滤 host、content-length、x-api-key、authorization
- 响应头过滤：移除 transfer-encoding 与 content-length

## Provider 兼容细节
- OpenAI：Authorization: Bearer {api_key}
- Anthropic：x-api-key: {api_key}，默认追加 anthropic-version: 2023-06-01
- Gemini：x-goog-api-key: {api_key}
- model_map：对请求体 model 字段做映射

## Proxy 规则
- proxy 路由通过 `/proxy/{router_name}/*` 绑定
- header 透传使用黑名单策略

## 指标
- apex_requests_total{route,router}
- apex_errors_total{route,router}
- apex_upstream_latency_ms{route,router,channel}
- apex_fallback_total{router,channel}

## 错误返回
- 400：请求体不可解析
- 401：缺失或无效的 vkey / api key
- 404：router 或 channel 不存在
- 502：上游失败或超时

## 其他规则
- vkey 由系统生成
- fallback 按错误类型与状态码触发
- 配置热加载失败时保持旧配置并记录错误

## 验收标准
- 支持 OpenAI/Anthropic 兼容路径与 Proxy 路由
- 支持 channel/router 的增删改查与 JSON 输出
- 支持多 channel fallback 与 retry_on_status
- 支持热加载失败保持旧配置并输出错误
- 指标可被 Prometheus 拉取并包含请求、错误、延迟、fallback
