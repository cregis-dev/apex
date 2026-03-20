# Story 7.2: Compliance Middleware

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a platform security operator,
I want compliance enforcement to run as dedicated middleware on model request paths,
so that sensitive-data controls remain composable, deterministic, and consistent across gateway flows.

## Acceptance Criteria

1. [AC1] Compliance request processing is moved out of handler-level inline logic into a dedicated Axum middleware for model-serving routes, while preserving current mask/block behavior.
2. [AC2] The compliance middleware runs after team authentication and team policy checks, but before router selection and upstream forwarding for the OpenAI- and Anthropic-compatible model endpoints.
3. [AC3] The middleware reuses the existing compliance configuration and processing primitives (`Compliance`, `PiiProcessor`, `process_json_content`) instead of re-implementing regex, masking, or blocking logic.
4. [AC4] Blocked requests still return HTTP 403 with the existing JSON error envelope, and masked requests continue upstream with the rewritten request body while never logging raw sensitive values.
5. [AC5] When compliance is disabled or absent, request behavior remains unchanged. Existing non-compliance request flows must not regress.
6. [AC6] Regression coverage proves the middleware path for at least one OpenAI-compatible route and one Anthropic-compatible route, including mask, block, and disabled/no-op scenarios.

## Tasks / Subtasks

- [ ] Task 1: Add dedicated compliance middleware module and route wiring (AC: #1, #2)
  - [ ] 1.1 Add `src/middleware/compliance.rs` following the existing Axum middleware pattern used by `auth.rs` and `policy.rs`.
  - [ ] 1.2 Export the new middleware from `src/middleware/mod.rs`.
  - [ ] 1.3 Wire the middleware into `build_app()` for model routes only, preserving current auth and policy layers.
  - [ ] 1.4 Keep MCP, metrics, and non-model routes out of scope unless the existing model route path already shares them.
- [ ] Task 2: Move request mutation and blocking behavior into middleware without changing semantics (AC: #1, #3, #4, #5)
  - [ ] 2.1 Extract the compliance body-read, block, and mask flow currently embedded in `server.rs::process_request()`.
  - [ ] 2.2 Rebuild the request body after masking and pass the mutated request downstream.
  - [ ] 2.3 Preserve request headers, URI, extensions, and team context when reconstructing the request.
  - [ ] 2.4 Reuse existing `PiiProcessor` and `process_json_content()` from `src/compliance.rs`; do not duplicate built-in rules or config validation.
  - [ ] 2.5 Reuse or centralize the current 403 JSON error response shape instead of introducing a second error contract.
- [ ] Task 3: Keep handler and gateway behavior stable after extraction (AC: #2, #4, #5)
  - [ ] 3.1 Remove the now-duplicated compliance block from `process_request()` once middleware is active.
  - [ ] 3.2 Preserve the current execution point relative to router resolution and upstream forwarding.
  - [ ] 3.3 Preserve current audit logging guarantees: log rule/action/count metadata only, never raw PII.
  - [ ] 3.4 Preserve current behavior for invalid JSON fallback and for requests with compliance disabled.
- [ ] Task 4: Add regression tests for middleware behavior and route coverage (AC: #6)
  - [ ] 4.1 Extend `tests/e07_compliance_test.rs` to prove compliance still works on OpenAI-compatible routes after extraction.
  - [ ] 4.2 Add at least one Anthropics-compatible request test (`/v1/messages`) to prove middleware coverage is route-layer based rather than handler-local coincidence.
  - [ ] 4.3 Add a regression test that verifies disabled or absent compliance leaves the request path untouched.
  - [ ] 4.4 Add a regression test ensuring blocked requests still return the expected 403 JSON envelope.

## Dev Notes

- `7-2` does not yet exist as a standalone planning story; this file resolves the overlap between Epic 7 and the completed `7-1` story by narrowing the new work to middleware extraction and route-layer hardening.
- Do not re-implement regex rules, config structs, or compliance validation. Those already exist and are covered by `7-1`.
- The existing code already performs compliance processing inside `server.rs::process_request()`. This story is about moving that behavior into the middleware layer cleanly, not adding a second compliance system.

### Project Structure Notes

- Existing middleware modules live under `src/middleware/` and are exported through `src/middleware/mod.rs`.
- Model routes are assembled in `src/server.rs::build_app()`, where team auth and team policy are already layered onto the model route tree.
- `src/compliance.rs` already owns the reusable processing primitives and should remain the single source of truth for masking/blocking logic.
- `error_response()` currently lives in `src/server.rs`; if middleware needs the same JSON envelope, prefer extracting a shared helper over duplicating the response contract.

### Technical Requirements

- Preserve the current request-path order from the architecture: auth/policy first, then compliance enforcement, then router/provider handling. [Source: `_bmad-output/planning/architecture.md` - Request Flow, Key Architectural Constraints]
- Keep team governance deterministic on the request path. Compliance middleware must not bypass or reorder team policy evaluation. [Source: `_bmad-output/planning/architecture.md` - Key Architectural Constraints]
- Reuse existing compliance types in `src/config.rs` and processing functions in `src/compliance.rs`. Do not add a parallel config surface. [Source: `src/config.rs`, `src/compliance.rs`]
- Preserve current JSON scanning semantics unless there is an explicit, tested reason to narrow scope. The current helper traverses all string values in JSON and falls back to plain-text processing on parse failure. [Source: `src/compliance.rs`]
- Avoid new dependencies unless strictly necessary. `regex` and `once_cell` are already in place through `7-1`.

### Architecture Compliance

- The architecture defines the gateway entry layer as Axum plus team auth/policy on the request path, with provider routing and normalization after that. Compliance belongs in that same governed request path, not buried in provider adapters. [Source: `_bmad-output/planning/architecture.md` - Gateway Entry Layer, Request Flow]
- PRD FR8 requires request-time masking or blocking for sensitive data scenarios, and success indicators require gateway-level policy enforcement consistency. The middleware extraction must keep that behavior centralized. [Source: `_bmad-output/planning/prd.md` - FR8, Success Indicators]

### File Structure Requirements

- Expected primary touch points:
  - `src/middleware/compliance.rs`
  - `src/middleware/mod.rs`
  - `src/server.rs`
  - `tests/e07_compliance_test.rs`
- Possible secondary touch point:
  - shared response helper location if `error_response()` is extracted from `src/server.rs`
- Avoid broad edits to providers, router selection, or config schema unless strictly needed for compile-time integration.

### Testing Requirements

- Run `cargo test` at minimum.
- Because this story touches `src/server.rs` and model request handling, also run `./scripts/test-local-e2e.sh` before marking merge-ready, per repo guidance.
- Reuse the existing E07 test style in `tests/e07_compliance_test.rs` instead of inventing a new harness.
- Ensure at least:
  - masked request still succeeds
  - blocked request still returns 403 with expected JSON shape
  - disabled compliance stays pass-through
  - Anthropic-compatible route is covered

### Previous Story Intelligence

- `7-1` already completed AC1-AC7 for config, processor logic, inline request processing, and audit logging. This story must build on that result rather than redoing it. [Source: `_bmad-output/implementation/stories/7-1-pii-masking-engine.md`]
- `7-1` established the core reusable pieces:
  - `Compliance`, `PiiRule`, `PiiAction`, and config validation in `src/config.rs`
  - `PiiProcessor::new`, `process`, `should_block`, and `process_json_content` in `src/compliance.rs`
  - existing request blocking/masking inside `src/server.rs::process_request()`
- The strongest implementation hazard is duplication: leaving compliance in `process_request()` and adding new middleware on top would double-process requests.

### Git Intelligence Summary

- Recent commits are unrelated to compliance and mostly touch e2e harness parsing and repo hygiene, so there is no new compliance-specific pattern to inherit from Git history here.

### Project Context Reference

- No `project-context.md` file was found in the repository during story creation.

### References

- Epic definition and task breakdown: [Source: `_bmad-output/planning/epics/e07-data-compliance.md`]
- Cross-epic story list: [Source: `_bmad-output/planning/epics.md`]
- Product and governance requirements: [Source: `_bmad-output/planning/prd.md`]
- Architecture request flow and constraints: [Source: `_bmad-output/planning/architecture.md`]
- Completed prior story and implementation notes: [Source: `_bmad-output/implementation/stories/7-1-pii-masking-engine.md`]
- Existing compliance engine: [Source: `src/compliance.rs`]
- Current inline compliance integration point: [Source: `src/server.rs`]
- Existing middleware patterns: [Source: `src/middleware/auth.rs`], [Source: `src/middleware/policy.rs`]
- Existing regression suite: [Source: `tests/e07_compliance_test.rs`]

## Dev Agent Record

### Agent Model Used

GPT-5

### Debug Log References

- Story created from Epic 7 plus current implementation analysis on 2026-03-21.

### Completion Notes List

- Story scope intentionally narrowed to middleware extraction and route-layer enforcement.
- Overlap with completed `7-1` captured explicitly to prevent duplicate implementation.
- Story prepared for `dev-story` execution with existing codebase references and regression guardrails.

### File List

- _bmad-output/implementation/stories/7-2-compliance-middleware.md
