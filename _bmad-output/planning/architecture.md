---
title: Apex AI Gateway Architecture
status: active
audience: bmad
---

# Apex AI Gateway Architecture

## Goals

- Provide a team-first AI gateway for internal enterprise use.
- Keep the gateway fast, lightweight, and configuration-driven.
- Expose a unified OpenAI- and Anthropic-compatible interface.
- Support routing, retries, fallback, observability, and MCP capabilities.
- Allow providers and policies to evolve without redesigning the gateway.

## Core Components

### 1. Configuration and CLI

- Entry point: `apex`
- Main concerns: configuration loading, validation, management commands, and hot reload

### 2. Gateway Entry Layer

- Axum HTTP server bound by `global.listen`
- Team authentication through API keys
- Team policy enforcement for router/model access and rate limits

### 3. Router and Strategy Layer

- Router rules select upstream channels based on model and request criteria
- Strategies include round-robin, random, priority, and weighted selection
- Failover and retry rules handle upstream degradation

### 4. Provider Adapter Layer

- Provider-specific request and response normalization
- Protocol compatibility for OpenAI- and Anthropic-style interfaces
- Model rewriting and header injection where required

### 5. Observability Layer

- Prometheus metrics exposed from the gateway
- Usage and metrics persistence for analysis and dashboard reporting

### 6. MCP Layer

- MCP resources, prompts, and tools exposed through the gateway implementation
- Session lifecycle and capability negotiation handled alongside gateway auth

## Configuration Model

### Global

- listen address
- auth defaults
- timeout and retry settings

### Teams

- team identity and API key
- allowed routers and models
- per-team RPM and TPM policy

### Routers

- named routing surfaces
- matching rules and strategy configuration

### Channels

- upstream provider definitions
- base URL, credentials, and provider type

## Request Flow

1. Client sends a gateway request using a team key.
2. Gateway authenticates the caller and evaluates policy.
3. Router rules select a target channel using the configured strategy.
4. Provider adapter normalizes the request for the upstream provider.
5. Gateway sends the upstream request with timeout, retry, and fallback handling.
6. Response is streamed or returned to the client and operational data is recorded.

## Key Architectural Constraints

- Configuration is JSON-driven and supports operational hot reload.
- Team policy enforcement sits on the request path and must remain deterministic.
- Provider behavior differences are isolated inside adapter logic.
- Observability must remain available without coupling clients to internal storage.
- MCP capabilities must inherit the same governance posture as standard gateway traffic.

## Primary Technology Choices

- Rust 2024
- Axum
- Tokio
- Reqwest
- Serde / JSON
- Prometheus-compatible metrics

## Security and Governance

- Bearer-token style team authentication
- Router/model level policy enforcement
- Secret masking and compliance-oriented controls
- Audit-friendly logging and metrics

## Relationship to Other Planning Artifacts

- Product scope and outcomes: `./prd.md`
- Epic and story breakdown: `./epics.md` and `./epics/`
- UX and planning refinements: other files under `_bmad-output/planning/`
