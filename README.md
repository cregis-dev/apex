# Apex AI Gateway

Simple, High Performance, High Availability AI Gateway.
面向企业内部的轻量 AI Gateway，基于 Rust 实现，使用 JSON 配置驱动，支持 OpenAI/Anthropic 兼容入口，提供热加载、超时/重试、fallback 与 Prometheus 指标导出。

详细操作指南请参考 [docs/operations.md](docs/operations.md)。

## 功能概览

- **双协议支持**: 同时兼容 OpenAI 与 Anthropic 协议，支持 MiniMax/DeepSeek 等双协议 Provider。
- **多通道路由**: 支持权重负载均衡 (Round Robin/Priority/Random) 与模型名称路由。
- **高可用**: 支持 Connect/Request/Response 三级超时与自动故障转移 (Fallback)。
- **安全**: 全局鉴权与 Router VKey 鉴权。
- **可观测**: 内置 Prometheus 指标导出。
- **易用**: CLI 交互式管理配置。

## 安装

```bash
cargo install --path .
```

## 快速开始

### 1) 初始化配置

```bash
apex init
```

### 2) 添加 channel

```bash
# 交互式引导添加
apex channel add --name openai-main
```

### 3) 添加 router

```bash
# 交互式引导添加
apex router add --name default-openai
```

### 4) 启动服务

```bash
# 前台运行
apex gateway start

# 后台运行 (Daemon)
apex gateway start -d
```

### 5) 验证调用

**OpenAI 兼容客户端**:
```bash
curl http://localhost:12356/v1/chat/completions \
  -H "Authorization: Bearer <router-vkey>" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"hello"}]}'
```

**Anthropic 兼容客户端**:
```bash
curl http://localhost:12356/v1/messages \
  -H "x-api-key: <router-vkey>" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-3-5-sonnet-20240620","messages":[{"role":"user","content":"hello"}]}'
```

## 更多文档

- [操作手册 (Operations Guide)](docs/operations.md): 详细的 CLI 使用说明、配置参数详解、高级路由策略配置及双协议支持说明。
- [架构文档 (Architecture)](docs/architecture.md): 架构设计说明。

## 客户端兼容性

为了更好地支持各类 AI 客户端（如 Chatbox, NextChat, Vercel AI SDK 等），Apex 提供了以下兼容性支持：

1.  **标准鉴权头**：支持使用 `Authorization: Bearer <key>`（OpenAI）或 `x-api-key: <key>`（Anthropic）。
2.  **路径兼容**：同时支持 `/v1/chat/completions` 和 `/chat/completions`（无 `/v1` 前缀）等路径。
3.  **模型列表**：支持 `GET /v1/models` 接口。

## 运维

默认指标地址：`http://localhost:9090/metrics`
