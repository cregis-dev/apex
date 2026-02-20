---
title: 简易 AI Gateway 技术架构
status: draft
---

# 简易 AI Gateway 技术架构

## 架构目标
- 面向企业内部使用的轻量 Gateway
- Rust 单体服务 + JSON 配置
- 提供 OpenAI/Anthropic 兼容接口与 Proxy 转发
- 支持热加载、超时/重试、fallback 与 Prometheus 指标

## 当前实现状态
- CLI 与配置结构已实现
- 运行时网关、路由转发、鉴权与指标进行中

## 组件划分
### CLI 配置管理
- 入口命令 apex
- 支持 init / channel / router 子命令
- 读写 JSON 配置文件

### 配置与热加载
- JSON 配置结构：global、channels、routers、metrics、hot_reload
- 热加载：文件变更触发重新加载，失败保持旧配置

### 网关入口层
- HTTP 服务监听 global.listen
- 统一鉴权入口：global.auth
- 路由到 OpenAI/Anthropic 兼容接口或 Proxy

### 路由与策略
- Router 绑定主 channel，可选 fallback_channels
- 按错误类型与状态码触发 fallback
- 超时/重试来自 global 或 channel 级别覆盖

### Provider 适配层
- 每个 channel 代表一个 provider 实例
- 适配请求/响应格式与基础 header
- 支持 model_map 进行模型映射

### Provider 扩展机制
- ProviderRegistry 维护 provider_type 到适配器的映射
- ProviderAdapter 定义路径映射与请求体转换扩展点
- 新增 provider 时仅需实现适配器并注册

### 观测与指标
- Prometheus 指标导出，metrics.listen + metrics.path
- 记录请求量、延迟、错误率、fallback 命中等指标

## 配置结构摘要
### Global
- listen: 网关监听地址
- auth: 鉴权模式与 key 列表
- timeouts: connect/request/response 超时
- retries: 最大重试次数与回退策略

### Channel
- name / provider_type / base_url / api_key
- headers / model_map / timeouts

### Router
- name / type / channel / fallback_channels / vkey

### Metrics
- enabled / listen / path

### HotReload
- config_path / watch

## 请求流程
1. 接收请求并做入口鉴权
2. 根据路径识别 OpenAI/Anthropic/Proxy 路由
3. 解析 router，选择主 channel
4. 发送上游请求，应用超时与重试
5. 触发 fallback 时切换备用 channel
6. 生成响应并记录指标

## 接口与路由约定
- OpenAI 兼容路由：/v1/chat/completions、/v1/completions、/v1/embeddings、/v1/models
- Anthropic 兼容路由：/v1/messages
- Proxy 路由：/proxy/{router_name}/{path...}

## 鉴权约定
- 入口鉴权：global.auth.mode=api_key 时读取 Authorization (Bearer) 或 x-api-key
- Router vkey：通过 Authorization (Bearer) 或 x-api-key 携带
- Proxy 路由不要求 vkey，仅通过 router_name 选择 router

## 转发与模型映射
- 上游请求基于 channel.base_url 与原路径拼接
- 读取 body.model 并按 channel.model_map 映射
- 默认写入 Authorization: Bearer {channel.api_key}
- channel.headers 作为补充 header 写入上游

## 重试与 fallback
- retry_on_status 命中或网络错误触发重试
- 重试次数受 global.retries.max_attempts 控制
- 重试耗尽后按 fallback_channels 顺序切换

## 指标设计
- /metrics 输出 Prometheus 文本
- 关键指标：请求总量、错误量、上游耗时、fallback 命中
- apex_requests_total{route,router}
- apex_errors_total{route,router}
- apex_upstream_latency_ms{route,router,channel}
- apex_fallback_total{router,channel}

## 关键技术选择
- HTTP 框架：axum
- 指标：Prometheus
- 序列化：serde + serde_json
- CLI：clap

## 风险与未决事项
- OpenAI/Anthropic 兼容字段映射细节未完全覆盖
- 指标名称与标签规范待细化
