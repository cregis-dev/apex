# Epic: Core Gateway (E01)

## Description
实现网关的核心请求处理流程，包括 HTTP 服务器搭建、配置加载、Provider 适配器实现以及基本的请求转发能力。

## Stories

- [x] **S01: HTTP Server & Basic Routing**
  - Implement Axum server listening on configured port.
  - Support `apex init` generated default config.
  - Handle `/v1/chat/completions` and other standard routes.

- [x] **S02: Provider Adapters (OpenAI & Anthropic)**
  - Implement `ProviderAdapter` trait.
  - Support OpenAI protocol forwarding.
  - Support Anthropic protocol forwarding (with version header).

- [x] **S03: Extended Providers Support**
  - Implement adapters for Gemini, Deepseek, Moonshot, Minimax, Ollama, Jina.
  - Handle specific header requirements for each provider.

- [x] **S04: Configuration System**
  - Define `Config` struct with Serde.
  - Implement `load_config` and `save_config`.
  - Support Hot Reload (watch file changes).

- [x] **S05: Basic Authentication & Rate Limiting**
  - Implement `vkey` verification (Bearer/x-api-key).
  - Implement Global Auth (Gateway level).
  - Define RateLimiter trait (NoOp implementation initially).

- [x] **S06: Basic Fallback Mechanism**
  - Support `fallback_channels` configuration.
  - Retry on specific error codes (429, 50x).
