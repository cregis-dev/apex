# Story 8.8: Remove HTTP MCP Surface

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a platform operator and product maintainer,
I want Apex to stop exposing the legacy HTTP MCP surface,
so that the product aligns with the future Admin Control Plane direction and no longer advertises a control path we intend to retire.

## Acceptance Criteria

1. [AC1] Apex no longer exposes `/mcp` as a supported HTTP route after the change.
2. [AC2] `global.enable_mcp` is no longer a supported product-facing configuration setting in the active schema or generated default config.
3. [AC3] Active operator and reference docs no longer describe HTTP MCP as a supported Apex capability.
4. [AC4] MCP-specific tests and helpers that only validate the retired HTTP surface are removed, retired, or updated so they do not represent active supported behavior.
5. [AC5] Active runtime server state no longer depends on HTTP MCP wiring after the route surface is removed.
6. [AC6] If any `src/mcp/` internals remain after the change, they have an explicit non-HTTP justification rather than lingering as accidental dead surface.
7. [AC7] Regression coverage still passes with `cargo test`, and because this story touches `src/server.rs`, `src/config.rs`, and `src/e2e.rs`, the local e2e suite is also run before merge-ready; if `.env.e2e` is present, real-smoke validation is additionally expected per repo policy.

## Tasks / Subtasks

- [ ] Task 1: Remove HTTP MCP route exposure from the main server (AC: #1, #5)
  - [ ] 1.1 Remove `/mcp` route registration from `src/server.rs::build_app()`.
  - [ ] 1.2 Remove the `mcp_enabled` route branch and related route assembly.
  - [ ] 1.3 Remove route-layer dependency on `crate::mcp::server::mcp_auth_guard`.
- [ ] Task 2: Remove product-facing MCP enablement from config and generated defaults (AC: #2)
  - [ ] 2.1 Remove or retire `Global.enable_mcp` from `src/config.rs`.
  - [ ] 2.2 Remove `enable_mcp: true` from generated default config in `src/main.rs`.
  - [ ] 2.3 Remove MCP enablement from generated e2e config in `src/e2e.rs`, or treat it as legacy input that no longer affects output if one short migration bridge is necessary.
- [ ] Task 3: Simplify server state after HTTP MCP removal (AC: #5, #6)
  - [ ] 3.1 Remove `mcp_server` from `AppState` if no active runtime path still needs it.
  - [ ] 3.2 Remove unconditional `McpServer::new(...)` construction if HTTP exposure was its only active consumer.
  - [ ] 3.3 Keep or extract any still-needed internals only where there is a proven non-HTTP reuse path.
- [ ] Task 4: Retire product-facing MCP documentation (AC: #3)
  - [ ] 4.1 Remove `/mcp` API contract sections from `docs/current/reference/api-contracts.md`.
  - [ ] 4.2 Remove MCP enable/connect/operate guidance from `docs/current/guides/operations.md`.
  - [ ] 4.3 Remove MCP Server positioning from `docs/current/overview.md`.
  - [ ] 4.4 Remove or revise active MCP route/module descriptions in `docs/current/architecture/backend.md` and `docs/current/architecture/integration.md`.
- [ ] Task 5: Retire MCP-specific tests and shared fixtures (AC: #4, #7)
  - [ ] 5.1 Remove or archive MCP-focused tests under `tests/` that only validate the retired HTTP product surface.
  - [ ] 5.2 Update shared config builders such as `tests/common/mod.rs` so they no longer set `enable_mcp`.
  - [ ] 5.3 Review any test summaries or metadata artifacts that still present MCP surface tests as active coverage.
- [ ] Task 6: Decide final disposition of `src/mcp/` exports (AC: #6)
  - [ ] 6.1 Remove `pub mod mcp` from `src/lib.rs` if the module is no longer part of supported code paths.
  - [ ] 6.2 If some `src/mcp/` internals remain, narrow exports and document why they still exist.

## Dev Notes

- This story is about removing the HTTP MCP product surface, not blindly deleting every MCP-related Rust file. That distinction matters because some MCP-adjacent internals may still be reusable, especially analytics-related logic.
- The product decision is already made: Admin Control Plane will own future remote administrative workflows, while local automation is moving to the CLI.
- Do not preserve a fake `enable_mcp` toggle after route removal. That would leave the product surface contradictory.

### Project Structure Notes

- Runtime HTTP route assembly lives in `src/server.rs`.
- Product-facing config schema lives in `src/config.rs`.
- Default config generation lives in `src/main.rs`.
- E2E config generation lives in `src/e2e.rs`.
- MCP-focused tests currently live in `tests/mcp_*.rs`.

### Technical Requirements

- Remove the supported HTTP route surface completely rather than hiding it behind a disabled flag. [Source: `_bmad-output/implementation/tech-specs/remove-http-mcp-surface.md`]
- Remove or retire `global.enable_mcp` from the supported product config surface. [Source: `_bmad-output/implementation/tech-specs/remove-http-mcp-surface.md`]
- Keep shared internals only where there is a real non-HTTP dependency; otherwise treat them as dead code. [Source: `_bmad-output/implementation/tech-specs/remove-http-mcp-surface.md`]

### Architecture Compliance

- The current architecture artifact still mentions MCP capabilities, but product planning has now retired the HTTP MCP surface and repositioned Apex around gateway runtime, dashboard, and AI-friendly CLI operations. Implementation should follow the updated planning direction rather than preserving stale architecture language. [Source: `_bmad-output/planning/prd.md`, `_bmad-output/planning/epics/e08-mcp-server.md`]
- Request-path governance for normal gateway traffic must remain intact while removing MCP-specific route wiring. [Source: `_bmad-output/planning/architecture.md`]

### File Structure Requirements

- Expected primary touch points:
  - `src/server.rs`
  - `src/config.rs`
  - `src/main.rs`
  - `src/e2e.rs`
  - `src/lib.rs`
  - `docs/current/reference/api-contracts.md`
  - `docs/current/guides/operations.md`
  - `docs/current/overview.md`
  - `docs/current/architecture/backend.md`
  - `docs/current/architecture/integration.md`
  - `tests/`
- Avoid broad unrelated refactors outside HTTP MCP retirement.

### Testing Requirements

- Run `cargo test`.
- Because this story touches `src/server.rs`, `src/config.rs`, and `src/e2e.rs`, also run `./scripts/test-local-e2e.sh` before marking the work merge-ready.
- If `.env.e2e` exists locally, run `./scripts/test-real-smoke.sh` before merge-ready, per repo guidance.

### Previous Story Intelligence

- Epic 8 was previously delivered as MCP server functionality and has now been superseded in planning. This story exists to retire that surface cleanly rather than leaving the codebase and docs in a contradictory state.
- The main server currently constructs `McpServer`, stores it in `AppState`, and conditionally mounts `/mcp` behind `global.enable_mcp`; those are the primary runtime removal points.

### References

- Superseded Epic 8 planning context: [Source: `_bmad-output/planning/epics/e08-mcp-server.md`]
- Cross-epic story list: [Source: `_bmad-output/planning/epics.md`]
- PRD change in scope direction: [Source: `_bmad-output/planning/prd.md`]
- Implementation spec: [Source: `_bmad-output/implementation/tech-specs/remove-http-mcp-surface.md`]
- Existing runtime wiring: [Source: `src/server.rs`]
- Existing config schema: [Source: `src/config.rs`]
- Existing e2e config model: [Source: `src/e2e.rs`]
- Existing MCP tests: [Source: `tests/mcp_tools_test.rs`], [Source: `tests/mcp_resources_test.rs`], [Source: `tests/mcp_prompts_test.rs`], [Source: `tests/mcp_session_test.rs`]

## Dev Agent Record

### Agent Model Used

GPT-5

### Debug Log References

- Story created from MCP retirement tech spec and superseded Epic 8 context on 2026-03-29.

### Completion Notes List

- Removed the HTTP MCP product surface from runtime wiring, product-facing config, generated defaults, docs, and test fixtures.
- Deleted the remaining `src/mcp/` module files and retired MCP-specific tests once no supported runtime path remained.
- Follow-up cleanup removed the stale `prompts` config surface and related active documentation after review confirmed there was no runtime consumer left.
- `cargo test` compiles and the main test suites reach the existing environment limit at local listener bind permissions, not MCP compilation or route errors.
- `./scripts/test-local-e2e.sh` remains blocked in this environment by mock listener bind permissions.
- `.env.e2e` was present and `./scripts/test-real-smoke.sh` was run; OpenAI and Anthropic base smoke requests passed, while the final Claude Code style tool-history scenario failed and remains to be triaged separately.

### File List

- _bmad-output/implementation/stories/8-8-remove-http-mcp-surface.md
- src/server.rs
- src/config.rs
- src/main.rs
- src/e2e.rs
- src/lib.rs
- docs/current/reference/api-contracts.md
- docs/current/guides/operations.md
- docs/current/overview.md
- docs/current/architecture/backend.md
- docs/current/architecture/integration.md
- tests/common/mod.rs
