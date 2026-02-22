# 操作手册：简易 AI Gateway

## 适用范围

适用于本项目的本地部署与基础使用，包含安装、配置、启动、调用与运维检查。

## 安装

### 环境要求

- Rust（edition 2024）
- 网络可访问目标 provider 的 API

### 获取代码

```bash
git clone <your-repo>
cd apex
```

### 构建与安装

```bash
cargo install --path .
apex --version
```

## 使用

### 1. 初始化配置

```bash
apex init
```

默认配置路径：`~/.apex/config.json`

### 2. 添加 channel

```bash
# 交互式添加（推荐）
apex channel add --name openai-main
```

系统将引导您选择 Provider、确认 Base URL 并输入 API Key。

```bash
# 完整参数方式
apex channel add \
  --name openai-main \
  --provider openai \
  --base-url https://api.openai.com \
  --api-key sk-xxx
```

可选参数：

- `--header key=value`：追加上游请求头（可多次传入）
- `--model-map old=new`：模型映射（可多次传入）
- `--connect-ms` / `--request-ms` / `--response-ms`：通道级超时

### 3. 添加 router

```bash
apex router add \
  --name default-openai \
  --channel openai-main
```

如需指定 vkey：

```bash
apex router add \
  --name default-openai \
  --channel openai-main \
  --vkey vk_xxxxx
```

### 4. 启动服务

```bash
# 开发调试
cargo run -- gateway start

# 生产环境（后台运行）
apex gateway start -d
```

### 5. 停止服务

```bash
apex gateway stop
```

### 6. 查看 channel 详情

```bash
apex channel show --name openai-main
```

### 7. 查看列表

```bash
apex channel list
apex router list
```

默认 channel list 不显示 Base URL，如需查看完整信息请使用 `show` 命令。

## 调用示例

### OpenAI 兼容

```bash
curl http://localhost:12356/v1/chat/completions \
  -H "content-type: application/json" \
  -H "Authorization: Bearer <router-vkey>" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"hello"}]}'
```

### Anthropic 兼容

```bash
curl http://localhost:12356/v1/messages \
  -H "content-type: application/json" \
  -H "x-api-key: <router-vkey>" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-3-5-sonnet-20240620","messages":[{"role":"user","content":"hello"}]}'
```

## 鉴权

### 认证方式

Apex 支持通过标准请求头传递凭证：

1. **OpenAI 协议**：
   - `Authorization: Bearer <key>`
2. **Anthropic 协议**：
   - `x-api-key: <key>`

### 鉴权层级

- **全局鉴权**：保护整个网关，需在 `config.json` 中配置 `global.auth`。
- **路由鉴权**：保护特定路由，需在 Router 配置中设置 `vkey`。
- **Provider Key**：仅用于访问上游服务，保存在 Channel 配置中，不对外暴露。

> 提示：在使用 Chatbox、NextChat 等标准 OpenAI 客户端时，直接将 Router VKey 填入 API Key 字段即可。

## 配置结构

配置文件为 JSON，主要结构如下：

- `global.listen`：监听地址
- `global.auth`：全局鉴权
- `global.timeouts`：全局超时
- `global.retries`：重试策略
- `channels`：上游通道集合
- `routers`：路由集合
- `metrics`：指标导出
- `hot_reload`：热加载开关

### 示例

```json
{
  "version": "1",
  "global": {
    "listen": "0.0.0.0:12356",
    "auth": { "mode": "none", "keys": null },
    "timeouts": { "connect_ms": 2000, "request_ms": 30000, "response_ms": 30000 },
    "retries": { "max_attempts": 2, "backoff_ms": 200, "retry_on_status": [429, 500, 502, 503, 504] }
  },
  "channels": [
    {
      "name": "openai-main",
      "provider_type": "openai",
      "base_url": "https://api.openai.com",
      "api_key": "sk-xxx",
      "headers": null,
      "model_map": null,
      "timeouts": null
    }
  ],
  "routers": [
    {
      "name": "default-openai",
      "vkey": "vk_xxxxx",
      "channel": "openai-main",
      "fallback_channels": []
    }
  ],
  "metrics": { "enabled": true, "listen": "0.0.0.0:9090", "path": "/metrics" },
  "hot_reload": { "config_path": "~/.apex/config.json", "watch": true }
}
```

## 运维检查

### 指标

默认地址：`http://localhost:9090/metrics`

- apex_requests_total{route,router}
- apex_errors_total{route,router}
- apex_upstream_latency_ms{route,router,channel}
- apex_fallback_total{router,channel}

### 常见问题

- 401：检查 `x-apex-api-key` / `x-apex-vkey`
- 404：检查 router 名称与 channel 绑定
- 502：检查上游 base_url 与网络连通性

## 测试

```bash
cargo test
```
