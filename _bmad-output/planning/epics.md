# BMAD Compatibility Epics

This file restores the flat `epics.md` contract expected by BMAD workflows.
The detailed epic source files live under `_bmad-output/planning/epics/`.

## Epic 1: Core Gateway

Source: `_bmad-output/planning/epics/e01-core-gateway.md`

Description: Build the core gateway request pipeline, configuration loading, provider adapters, and basic request forwarding.

### Story 1.1: HTTP Server Basic Routing

- Implement the Axum server and standard API routes.

### Story 1.2: Provider Adapters OpenAI Anthropic

- Implement the initial provider adapter layer.

### Story 1.3: Extended Providers Support

- Add support for additional upstream providers.

### Story 1.4: Configuration System

- Implement config load, save, validation, and hot reload support.

### Story 1.5: Basic Authentication Rate Limiting

- Add gateway authentication and baseline rate limiting.

### Story 1.6: Basic Fallback Mechanism

- Add fallback channel handling and retry behavior.

## Epic 2: Advanced Routing

Source: `_bmad-output/planning/epics/e02-advanced-routing.md`

Description: Expand routing with multi-channel configuration, caching, routing strategies, and model matching.

### Story 2.1: Multi Channel Configuration Support

- Support richer multi-channel configuration and selection.

### Story 2.2: Router Rule Cache

- Add routing rule cache support.

### Story 2.3: Routing Strategy Implementation

- Implement round-robin, random, and weighted routing.

### Story 2.4: Content Based Routing Model Matcher

- Route requests using model and content matching rules.

## Epic 3: CLI Management

Source: `_bmad-output/planning/epics/e03-cli-management.md`

Description: Provide CLI workflows for project setup and gateway management.

### Story 3.1: CLI Infrastructure

- Establish the command structure and shared CLI plumbing.

### Story 3.2: Channel Management Commands

- Add CLI commands for channel lifecycle management.

### Story 3.3: Router Management Commands

- Add CLI commands for router lifecycle management.

### Story 3.4: Gateway Control

- Add CLI commands for gateway runtime control.

## Epic 4: Observability

Source: `_bmad-output/planning/epics/e04-observability.md`

Description: Add observability through metrics and structured logging.

### Story 4.1: Prometheus Metrics

- Expose metrics for gateway health and traffic.

### Story 4.2: Structured Logging

- Add structured tracing-based logging throughout the gateway.

## Epic 5: Rule Based Routing

Source: `_bmad-output/planning/epics/e05-rule-based-routing.md`

Description: Evolve routing to support rule-based matching with backward compatibility.

### Story 5.1: Configuration Schema Update

- Extend configuration for rule-based routing.

### Story 5.2: Backward Compatibility Migration

- Preserve existing behavior and migration safety.

### Story 5.3: Router Selector Logic Update

- Update routing resolution to evaluate rules.

### Story 5.4: CLI Documentation Update

- Reflect routing changes in CLI and documentation.

## Epic 6: Team Governance

Source: `_bmad-output/planning/epics/e06-team-governance.md`

Description: Add team-aware controls, authentication, and policy enforcement.

### Story 6.1: Team Configuration Support

- Add team definitions and policy controls to configuration.

### Story 6.2: Team CLI Management

- Add CLI support for managing teams.

### Story 6.3: Team Authentication Middleware

- Authenticate team-scoped access to the gateway.

### Story 6.4: Team Policy Middleware

- Enforce per-team policies during request handling.

## Epic 7: Data Compliance PII Masking

Source: `_bmad-output/planning/epics/e07-data-compliance.md`

Description: Protect sensitive user data before sending requests to upstream LLM providers.

### Story 7.1: PII Masking Engine

- Implement configurable PII detection, masking, blocking, and audit logging.

### Story 7.2: Compliance Middleware

- Apply compliance policies during request processing and gateway flow control.

## Epic 8: MCP Server Operations Analytics

Source: `_bmad-output/planning/epics/e08-mcp-server.md`

Description: Provide MCP transport, lifecycle, resources, prompts, tools, and analytics support in Apex Gateway.

### Story 8.1: MCP Protocol Transport

- Add MCP protocol handling and transport support.

### Story 8.2: Session Lifecycle

- Implement session lifecycle, capability negotiation, and cleanup.

### Story 8.3: MCP Resources

- Expose MCP resources backed by gateway configuration.

### Story 8.4: Resource Listing Key Masking

- Mask sensitive values in resource listings and config views.

### Story 8.5: Analytics Reporting

- Expose MCP analytics and reporting output.

### Story 8.6: MCP Prompts

- Expose prompt templates through MCP prompts APIs.

### Story 8.7: MCP Tools

- Expose tool discovery and execution through MCP tools APIs.
