---
title: 'Remove HTTP MCP Surface from Apex'
slug: 'remove-http-mcp-surface'
created: '2026-03-29'
status: 'ready-for-dev'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['Rust', 'Axum', 'serde']
files_to_modify:
  - '/Users/shawn/workspace/code/apex/src/server.rs'
  - '/Users/shawn/workspace/code/apex/src/config.rs'
  - '/Users/shawn/workspace/code/apex/src/main.rs'
  - '/Users/shawn/workspace/code/apex/src/e2e.rs'
  - '/Users/shawn/workspace/code/apex/src/lib.rs'
  - '/Users/shawn/workspace/code/apex/docs/current/reference/api-contracts.md'
  - '/Users/shawn/workspace/code/apex/docs/current/guides/operations.md'
  - '/Users/shawn/workspace/code/apex/docs/current/overview.md'
  - '/Users/shawn/workspace/code/apex/docs/current/architecture/backend.md'
  - '/Users/shawn/workspace/code/apex/docs/current/architecture/integration.md'
  - '/Users/shawn/workspace/code/apex/tests/'
code_patterns:
  - 'HTTP MCP exposure is currently integrated into the main Axum server through /mcp route wiring in src/server.rs.'
  - 'global.enable_mcp currently gates route registration and is also present in config-building helpers and defaults.'
  - 'MCP-related code exists both as product surface and as internal Rust modules; removal of the HTTP surface does not automatically require deleting every internal module.'
  - 'Current docs describe /mcp as an active endpoint and global.enable_mcp as an operator-facing config toggle.'
test_patterns:
  - 'cargo test'
  - 'Remove or update MCP-specific tests that no longer reflect supported product behavior.'
---

# Tech-Spec: Remove HTTP MCP Surface from Apex

**Created:** 2026-03-29

## Overview

### Problem Statement

Apex is moving toward an Admin Control Plane for remote administrative workflows. The existing HTTP MCP surface no longer fits that product direction and creates an extra externally-visible control path that the product does not intend to keep supporting.

Today the HTTP MCP feature is still visible in several places:

- the main server exposes `/mcp`
- config includes a `global.enable_mcp` operator-facing toggle
- docs describe MCP as a supported capability
- tests and e2e helpers still construct configs with MCP enabled

Leaving those pieces in place would preserve a product surface that you have already decided to retire.

### Solution

Remove the HTTP MCP surface from Apex as a supported runtime capability. This includes route exposure, config-level enablement for that surface, operator documentation, and product-facing tests. Retain internal Rust modules only where they are still needed as non-product implementation details or shared logic; do not force broad deletion unless the code is truly dead.

### Scope

**In Scope:**
- Remove HTTP route exposure for MCP from the main server
- Remove or retire `global.enable_mcp` as a supported product configuration switch
- Update config defaults and e2e/config helpers so they no longer advertise MCP enablement
- Remove or rewrite docs that describe HTTP MCP as a supported Apex capability
- Remove or rewrite tests whose purpose is validating the retired HTTP MCP surface

**Out of Scope:**
- Delivering the future Admin Control Plane
- Replacing HTTP MCP with another remote administration interface in this change
- Forcing deletion of all `src/mcp/` internals if some code is still reused outside the retired product surface
- Reworking unrelated gateway features or CLI automation work

## Context for Development

### Current Implementation Footprint

The HTTP MCP surface is currently represented by:

- [`/Users/shawn/workspace/code/apex/src/server.rs`](/Users/shawn/workspace/code/apex/src/server.rs)
  - builds `McpServer`
  - stores `mcp_server` in `AppState`
  - conditionally registers `/mcp` routes based on `global.enable_mcp`
- [`/Users/shawn/workspace/code/apex/src/config.rs`](/Users/shawn/workspace/code/apex/src/config.rs)
  - defines `Global.enable_mcp`
- [`/Users/shawn/workspace/code/apex/src/main.rs`](/Users/shawn/workspace/code/apex/src/main.rs)
  - default config creation sets `enable_mcp: true`
- [`/Users/shawn/workspace/code/apex/src/e2e.rs`](/Users/shawn/workspace/code/apex/src/e2e.rs)
  - e2e environment and generated config still model MCP enablement
- multiple docs under `docs/current/`
  - describe MCP as active server capability and document `/mcp`
- MCP-focused tests in [`/Users/shawn/workspace/code/apex/tests/`](/Users/shawn/workspace/code/apex/tests/)
  - validate MCP prompts, resources, tools, and session behavior

### Architectural Constraint

There is an important distinction between:

1. removing the supported HTTP MCP product surface, and
2. deleting all MCP-related Rust code

Those are not automatically the same operation.

This matters because:

- `src/mcp/analytics.rs` is already referenced as a potential shared analytics implementation anchor in other planning artifacts
- internal modules may remain temporarily useful during transition or code reuse

So the correct implementation stance is:

- remove the HTTP product surface completely
- delete dead code when clearly unused
- keep shared internals only if they still serve a non-retired path

### Files to Reference

| File | Purpose |
| ---- | ------- |
| [/Users/shawn/workspace/code/apex/src/server.rs](/Users/shawn/workspace/code/apex/src/server.rs) | Current HTTP route wiring and MCP server initialization |
| [/Users/shawn/workspace/code/apex/src/config.rs](/Users/shawn/workspace/code/apex/src/config.rs) | Product-facing config schema, including `global.enable_mcp` |
| [/Users/shawn/workspace/code/apex/src/main.rs](/Users/shawn/workspace/code/apex/src/main.rs) | Default config generation still enabling MCP |
| [/Users/shawn/workspace/code/apex/src/e2e.rs](/Users/shawn/workspace/code/apex/src/e2e.rs) | E2E config generation still models MCP enablement |
| [/Users/shawn/workspace/code/apex/src/lib.rs](/Users/shawn/workspace/code/apex/src/lib.rs) | Library export surface; may need cleanup if MCP becomes fully private or partially removed |
| [/Users/shawn/workspace/code/apex/docs/current/reference/api-contracts.md](/Users/shawn/workspace/code/apex/docs/current/reference/api-contracts.md) | Current `/mcp` API contract documentation |
| [/Users/shawn/workspace/code/apex/docs/current/guides/operations.md](/Users/shawn/workspace/code/apex/docs/current/guides/operations.md) | Operator documentation still instructs users to enable and connect to MCP |
| [/Users/shawn/workspace/code/apex/docs/current/overview.md](/Users/shawn/workspace/code/apex/docs/current/overview.md) | Product overview still describes Apex as including MCP Server |
| [/Users/shawn/workspace/code/apex/docs/current/architecture/backend.md](/Users/shawn/workspace/code/apex/docs/current/architecture/backend.md) | Architecture documentation still treats `/mcp` and MCP module as active |
| [/Users/shawn/workspace/code/apex/docs/current/architecture/integration.md](/Users/shawn/workspace/code/apex/docs/current/architecture/integration.md) | Integration diagrams and flow descriptions still include MCP |
| [/Users/shawn/workspace/code/apex/tests/mcp_tools_test.rs](/Users/shawn/workspace/code/apex/tests/mcp_tools_test.rs) | Example MCP-focused test that should no longer represent supported product behavior |

### Technical Decisions

1. Apex should no longer expose `/mcp` as a supported route.
2. `global.enable_mcp` should no longer be part of the supported product configuration surface.
3. Default config generation must stop advertising MCP.
4. E2E helpers and test config builders must stop treating MCP enablement as a normal configuration knob.
5. MCP-specific docs should be removed from active operator/reference docs or replaced with a short retirement note where historical context is necessary.
6. MCP-specific tests that only validate the retired product surface should be removed or retired from the supported regression suite.
7. Internal `src/mcp/` code may remain temporarily only if it has proven non-HTTP reuse; otherwise it should be removed as dead code.

## Implementation Plan

### Tasks

- [ ] Task 1: Remove HTTP MCP route registration from the server
  - File: [/Users/shawn/workspace/code/apex/src/server.rs](/Users/shawn/workspace/code/apex/src/server.rs)
  - Action:
    1. Remove `/mcp` route exposure from `build_app`
    2. Remove the `mcp_enabled` branch and related route assembly
    3. Remove `mcp_auth_guard` route dependency from the main server path
  - Notes: The goal is zero supported HTTP MCP endpoints, not a hidden route behind a disabled flag.

- [ ] Task 2: Remove product-facing MCP enablement from config and defaults
  - File: [/Users/shawn/workspace/code/apex/src/config.rs](/Users/shawn/workspace/code/apex/src/config.rs)
  - Action:
    1. Remove `Global.enable_mcp` from the supported config schema, or make it an ignored legacy field only if migration safety requires one short compatibility bridge
    2. Update serde/default behavior accordingly
  - Notes: Prefer not to preserve a fake config toggle for a retired surface.
  - File: [/Users/shawn/workspace/code/apex/src/main.rs](/Users/shawn/workspace/code/apex/src/main.rs)
  - Action: Remove `enable_mcp: true` from generated default config.
  - File: [/Users/shawn/workspace/code/apex/src/e2e.rs](/Users/shawn/workspace/code/apex/src/e2e.rs)
  - Action: Remove MCP enablement from generated e2e config or treat it as legacy input that no longer affects the output.

- [ ] Task 3: Simplify server state and initialization
  - File: [/Users/shawn/workspace/code/apex/src/server.rs](/Users/shawn/workspace/code/apex/src/server.rs)
  - Action:
    1. Remove `mcp_server` from `AppState` if no longer needed by active runtime paths
    2. Remove unconditional `McpServer::new(...)` construction if HTTP exposure was its only active consumer
    3. Keep or extract any still-needed shared internals only if another active feature depends on them
  - Notes: This is the point where dead code should be pruned carefully rather than reflexively.

- [ ] Task 4: Retire product-facing docs
  - File: [/Users/shawn/workspace/code/apex/docs/current/reference/api-contracts.md](/Users/shawn/workspace/code/apex/docs/current/reference/api-contracts.md)
  - Action: Remove `/mcp` API contract from active reference docs.
  - File: [/Users/shawn/workspace/code/apex/docs/current/guides/operations.md](/Users/shawn/workspace/code/apex/docs/current/guides/operations.md)
  - Action: Remove enable/connect/operate MCP guidance and any mention of `global.enable_mcp`.
  - File: [/Users/shawn/workspace/code/apex/docs/current/overview.md](/Users/shawn/workspace/code/apex/docs/current/overview.md)
  - Action: Remove MCP Server from product overview positioning.
  - File: [/Users/shawn/workspace/code/apex/docs/current/architecture/backend.md](/Users/shawn/workspace/code/apex/docs/current/architecture/backend.md)
  - Action: Remove `/mcp` runtime route and active MCP module descriptions, or replace them with a retirement note if needed.
  - File: [/Users/shawn/workspace/code/apex/docs/current/architecture/integration.md](/Users/shawn/workspace/code/apex/docs/current/architecture/integration.md)
  - Action: Remove MCP integration diagrams and flows from active architecture docs.

- [ ] Task 5: Retire MCP-specific tests and helpers
  - File: [/Users/shawn/workspace/code/apex/tests/](/Users/shawn/workspace/code/apex/tests/)
  - Action:
    1. Remove or archive MCP surface tests that no longer reflect a supported product capability
    2. Update shared test config builders that still set `enable_mcp`
  - Notes: Do not keep a green test suite for a feature the product no longer supports.

- [ ] Task 6: Decide final disposition of `src/mcp/`
  - File: [/Users/shawn/workspace/code/apex/src/lib.rs](/Users/shawn/workspace/code/apex/src/lib.rs)
  - Action:
    1. Remove `pub mod mcp` if the module is no longer part of supported code paths
    2. If parts of `src/mcp/` remain for internal reuse, narrow exports and remove unsupported public exposure
  - Notes: This is the only task that should delete deeper MCP internals, and only after confirming actual remaining dependencies.

## Acceptance Criteria

- [ ] AC 1: Given Apex is started after this change, when HTTP routes are registered, then `/mcp` is no longer exposed as a supported endpoint.
- [ ] AC 2: Given an operator reviews the active config schema and generated default config, when looking for HTTP MCP enablement, then `global.enable_mcp` is no longer a supported product-facing setting.
- [ ] AC 3: Given an operator reads active docs, when reviewing API contracts, operations guidance, overview, or architecture pages, then HTTP MCP is no longer described as a supported Apex capability.
- [ ] AC 4: Given the supported regression suite is run, when tests execute, then MCP-specific tests for the retired HTTP surface are removed, retired, or updated so they do not represent active supported behavior.
- [ ] AC 5: Given implementation completes, when shared runtime state is inspected, then no active server path still depends on HTTP MCP wiring.
- [ ] AC 6: Given some `src/mcp/` code remains after the change, when reviewing the result, then the remaining code has an explicit non-HTTP justification rather than lingering as accidental dead surface.
- [ ] AC 7: When `cargo test` is run, then the remaining supported test suite passes.

## Additional Context

### Dependencies

- No new dependency is required.
- This change may unlock follow-up cleanup of dead MCP code, but that should happen only after verifying reuse boundaries.

### Testing Strategy

- Run `cargo test`
- Prefer removing obsolete MCP surface tests over leaving them as ignored historical drift unless there is a clear archive convention
- Add focused regression around route absence or config migration behavior only if the codebase already has a clean place for that check

### Risks

- Leaving `enable_mcp` in config after route removal creates operator confusion and future maintenance debt.
- Deleting all `src/mcp/` code immediately could break unrelated internal reuse if assumptions are wrong.
- Updating planning docs without updating active operator docs would leave the repo internally contradictory.

### Notes

- This spec is intentionally about removing the HTTP MCP product surface, not about deleting every MCP-related artifact in one sweep.
- Product intent is clear: remote administrative direction moves to Admin Control Plane, while local automation moves to CLI.
