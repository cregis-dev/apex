---
title: Apex AI Gateway Architecture
status: active
---

# Apex AI Gateway Architecture

## Goals
- Team-first, lightweight AI Gateway for internal enterprise use.
- Rust-based, high performance, JSON configuration driven.
- Unified OpenAI/Anthropic compatible interface.
- Support for Hot Reload, Timeouts/Retries, Fallbacks, and Prometheus Observability.
- Multi-provider support (OpenAI, Anthropic, DeepSeek, Ollama, etc.).

## Core Components

### 1. Configuration Management (CLI)
- Entry point: `apex` command.
- Subcommands: `init`, `channel`, `router`, `team`, `gateway`.
- Manages `config.json` with hot reload support.

### 2. Gateway Entry Layer
- HTTP Server (Axum) listening on `global.listen`.
- **Team Authentication**: Validates API Keys against configured Teams.
- **Team Policy**: Enforces Rate Limits (RPM/TPM) and Access Control (Allowed Routers/Models).

### 3. Router & Strategy
- **Router**: The logical endpoint for clients.
- **Rule-Based Routing**: Matches requests based on model names (glob patterns) or other criteria.
- **Load Balancing**: Distributes traffic across multiple Channels using strategies (Round Robin, Priority, Random).
- **Failover**: Automatically retries next available channel on failure.

### 4. Provider Adapters
- Abstracts differences between upstream providers.
- **Dual Protocol Support**: Handles both OpenAI and Anthropic protocols for providers that support both (e.g., DeepSeek, MiniMax).
- **Model Mapping**: Rewrites model names on the fly.
- **Header Injection**: Adds custom headers for upstream requests.

### 5. Observability
- Prometheus metrics exported at `/metrics`.
- Tracks: Request Volume, Latency, Error Rates, Token Usage (estimated).
- Labels: `router`, `model`, `team_id`.

## Configuration Structure

### Global
- `listen`: Gateway bind address.
- `auth`: Global authentication settings (optional).
- `timeouts`: Default timeouts.
- `retries`: Retry policy.

### Team (New in v0.2)
- `id`: Team identifier.
- `api_key`: Secret key for authentication (`sk-ant-xxx`).
- `policy`:
  - `allowed_routers`: List of accessible routers.
  - `allowed_models`: List of allowed models (glob).
  - `rate_limit`: RPM/TPM limits.

### Router
- `name`: Unique identifier.
- `rules`: List of routing rules.
  - `match`: Criteria (e.g., `model: "gpt-*"`).
  - `strategy`: `round_robin` | `priority`.
  - `channels`: List of target channels with weights.

### Channel
- `name`: Unique identifier.
- `provider_type`: `openai` | `anthropic` | `deepseek` | etc.
- `base_url`: Upstream API endpoint.
- `api_key`: Upstream credentials.

## Request Flow

1. **Ingress**: Client sends request with `Authorization: Bearer <Team-Key>`.
2. **Auth & Policy**: 
   - Gateway validates Team Key.
   - Checks Rate Limits (Token Bucket).
   - Verifies Router/Model access permissions.
3. **Router Selection**:
   - Matches request against Router Rules.
   - Selects target Channel based on Load Balancing Strategy.
4. **Adapter Processing**:
   - Rewrites request (headers, model name).
   - Handles protocol conversion if necessary.
5. **Upstream Request**:
   - Sends HTTP request to Provider.
   - Handles timeouts and retries.
6. **Response Processing**:
   - Streams response back to client.
   - Records metrics (Latency, Status).

## Key Technologies
- **Runtime**: Tokio (Async Rust).
- **Web Framework**: Axum.
- **Observability**: Prometheus / Metrics-rs.
- **Configuration**: Serde / JSON.
- **CLI**: Clap.

## Security
- **Authentication**: Bearer Token (Team API Key).
- **Access Control**: Role-based access to Routers/Models.
- **Audit**: (Planned) Request logging for compliance.
