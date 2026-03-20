# Product Requirements Document - Apex Gateway

## Document Status

- Status: active
- Audience: BMAD planning and solutioning workflows
- Scope: Apex Gateway backend, MCP capabilities, and supporting dashboard surfaces

## Product Summary

Apex Gateway is an internal AI API gateway that gives teams one governed entry point for LLM access. It normalizes multi-provider APIs, applies team-aware access control and rate limits, supports intelligent routing and failover, and exposes operational insight through metrics, usage records, and MCP capabilities.

The product serves two connected surfaces:

1. A Rust gateway runtime that brokers requests to upstream model providers.
2. A web dashboard that visualizes usage and operational metrics.

## Problem Statement

Internal teams need a consistent and governable way to access multiple model providers without embedding provider-specific behavior, credentials, and policy logic into every application. Without a gateway, teams face:

- fragmented provider integrations
- duplicated authentication and authorization logic
- inconsistent routing and fallback behavior
- limited observability into usage, cost, and failures
- weak controls for compliance and sensitive data handling

## Goals

- Provide a single API gateway for multi-provider LLM access.
- Preserve compatibility with common OpenAI- and Anthropic-style client integrations.
- Enforce team-based authentication, policy, and rate limits centrally.
- Support configurable routing, balancing, retries, and failover.
- Expose operational data through metrics, usage reporting, and MCP endpoints.
- Support enterprise controls such as data masking and audit-friendly behavior.

## Non-Goals

- Building a general-purpose consumer chat product.
- Replacing upstream provider model capabilities or billing systems.
- Implementing a standalone identity platform beyond gateway-level team and key controls.

## Users and Stakeholders

### Primary Users

- Internal application developers integrating LLM capabilities
- Platform and infrastructure engineers operating the gateway
- Security and governance stakeholders overseeing provider usage

### Secondary Users

- Product and operations teams reviewing usage and reliability trends
- Tooling clients consuming MCP prompts, tools, and resources

## Core Capabilities

### Multi-Provider Gateway

- Route requests to OpenAI, Anthropic, Gemini, DeepSeek, Moonshot, MiniMax, Ollama, Jina, and OpenRouter style providers.
- Normalize request and response handling across providers.
- Rewrite model names, headers, and protocol details where needed.

### Routing and Resilience

- Match requests by model and rule definitions.
- Support round-robin, random, priority, and weighted channel selection.
- Retry and fail over when upstream requests fail under configured conditions.

### Team Governance

- Authenticate requests using team API keys.
- Restrict access by router and model.
- Enforce request and token limits at team level.

### Observability and Reporting

- Export Prometheus metrics.
- Persist usage and metrics events for reporting.
- Provide dashboard views over usage, rankings, and trends.

### MCP Support

- Expose MCP-compatible resources, prompts, and tools.
- Support MCP session lifecycle and capability negotiation.
- Align MCP access control with gateway authentication and policy rules.

### Compliance

- Support masking and control of sensitive request content.
- Create a foundation for audit and policy enforcement flows.

## Functional Requirements

### FR1. Unified Gateway API

The system shall provide a stable gateway endpoint for supported LLM request flows and abstract upstream provider differences from clients.

### FR2. Config-Driven Operation

The system shall be configured primarily through JSON configuration and CLI-assisted management flows.

### FR3. Team-Based Access Control

The system shall authenticate requests using team credentials and enforce per-team access and rate-limit policy.

### FR4. Intelligent Routing

The system shall route requests according to configured rules, strategies, and provider/channel availability.

### FR5. Failure Handling

The system shall support retries, timeouts, and fallback behavior for upstream request failures.

### FR6. Operational Visibility

The system shall expose metrics and stored usage data sufficient for monitoring and operational analysis.

### FR7. MCP Surface

The system shall expose MCP resources, prompts, and tools over the supported MCP transport model used by the gateway implementation.

### FR8. Compliance Controls

The system shall support request-time data masking or blocking behavior for sensitive data scenarios.

## Non-Functional Requirements

- Performance: low-overhead request brokering suitable for internal production traffic
- Reliability: graceful handling of upstream failures with retries and fallback
- Security: centralized authentication, policy enforcement, and secret masking
- Operability: structured logs, metrics, and CLI-driven administration
- Extensibility: add providers, routers, and MCP capabilities without major redesign

## Success Indicators

- Internal teams can access multiple model providers through one gateway integration.
- Team-level policy violations are blocked consistently at the gateway layer.
- Operators can identify request volume, error rate, and usage trends through dashboard and metrics endpoints.
- MCP clients can discover and invoke supported prompts, tools, and resources using the gateway's MCP implementation.

## Delivery Themes

- Theme 1: Core gateway and provider interoperability
- Theme 2: Advanced routing and operational controls
- Theme 3: Team governance and compliance
- Theme 4: MCP server operations and analytics

## Relationship to Planning Artifacts

- Detailed epic breakdown lives in `./epics.md` and `./epics/`
- Architecture reference lives in `./architecture.md`
- UX and planning refinements live alongside this file in `_bmad-output/planning/`
