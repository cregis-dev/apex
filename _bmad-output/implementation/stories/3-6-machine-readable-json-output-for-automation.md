# Story 3.6: Machine-Readable JSON Output for Automation

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As an AI skill or automation workflow,
I want `apex channel`, `apex router`, and `apex team` commands to return stable JSON,
so that I can parse success and failure results without scraping human-readable output.

## Acceptance Criteria

1. [AC1] The v1 JSON automation scope is explicitly limited to the existing `channel`, `router`, and `team` command families.
2. [AC2] Supported actions in the v1 scope provide a `--json` mode that returns one stable top-level response shape with `ok`, `command`, `message`, `data`, `errors`, and `meta`.
3. [AC3] JSON mode does not mix free-form human-readable status lines into stdout before or after the JSON payload.
4. [AC4] JSON success and JSON failure paths preserve the same top-level structure, and failure responses include at least one machine-readable error code.
5. [AC5] `command` and `meta.action` reflect the invoked verb as it exists today, including `team.remove`.
6. [AC6] Regression coverage proves representative success and failure payloads for `channel`, `router`, and `team` under `--json`.

## Tasks / Subtasks

- [ ] Task 1: Add a shared JSON response contract for v1 CLI automation (AC: #1, #2, #4, #5)
  - [ ] 1.1 Introduce a shared internal response shape in `src/main.rs` or a small adjacent helper module.
  - [ ] 1.2 Normalize top-level fields to:
    - `ok`
    - `command`
    - `message`
    - `data`
    - `errors`
    - `meta`
  - [ ] 1.3 Normalize `command` identifiers using the current verb names, such as `channel.add`, `router.list`, and `team.remove`.
- [ ] Task 2: Extend `--json` support across the v1 CLI surface (AC: #2, #3, #5)
  - [ ] 2.1 Keep existing `channel list --json` and `router list --json` behavior aligned with the new top-level contract rather than printing raw arrays.
  - [ ] 2.2 Add `--json` support to `channel add`, `channel update`, `channel delete`, and `channel show`.
  - [ ] 2.3 Add `--json` support to `router add`, `router update`, and `router delete`.
  - [ ] 2.4 Add `--json` support to `team add`, `team remove`, and `team list`.
- [ ] Task 3: Normalize error handling in JSON mode (AC: #3, #4)
  - [ ] 3.1 Ensure validation and not-found failures emit the same top-level contract as success responses.
  - [ ] 3.2 Introduce stable error codes for common failure categories such as missing required input, invalid value, already exists, or not found.
  - [ ] 3.3 Keep `data` present and use `null` when there is no result payload.
- [ ] Task 4: Document the JSON contract for operators and skills (AC: #1, #2, #5)
  - [ ] 4.1 Update `docs/current/guides/operations.md` with `--json` examples for `channel`, `router`, and `team`.
  - [ ] 4.2 Align examples with the v1 automation contract in `_bmad-output/planning/cli-ai-automation-contract.md`.
  - [ ] 4.3 Explicitly document that current verb names remain part of the machine contract for v1.
- [ ] Task 5: Add regression tests for JSON mode (AC: #6)
  - [ ] 5.1 Extend `tests/cli.rs` with representative JSON success assertions for `channel`, `router`, and `team`.
  - [ ] 5.2 Add representative JSON failure assertions, such as duplicate resource creation or not-found deletion.
  - [ ] 5.3 Validate response structure, not just string fragments.

## Dev Notes

- This story is about output contract, not prompt removal. Non-interactive invocation behavior belongs primarily to Story 3.5.
- The repo already has partial `--json` support, but current list commands print raw serialized entities rather than the contract required for AI skills. That behavior should be normalized, not duplicated.
- The v1 contract is intentionally conservative: it keeps current verbs and focuses on stable payload shape rather than broad CLI redesign.

### Project Structure Notes

- CLI parsing and handlers live in `src/main.rs`.
- Existing CLI regression coverage is already in `tests/cli.rs`.
- Operator-facing command documentation is concentrated in `docs/current/guides/operations.md`.

### Technical Requirements

- Follow the v1 automation contract in `_bmad-output/planning/cli-ai-automation-contract.md` as the source of truth for JSON shape. [Source: `_bmad-output/planning/cli-ai-automation-contract.md`]
- Keep top-level fields present across success and failure cases. [Source: `_bmad-output/implementation/tech-specs/cli-ai-friendly-automation-v1.md`]
- Do not emit extra human-readable lines in JSON mode. Parsing must not depend on ignoring banner text. [Source: `_bmad-output/implementation/tech-specs/cli-ai-friendly-automation-v1.md`]

### Architecture Compliance

- The PRD now defines the CLI as an AI-friendly automation surface for the v1 command families `channel`, `router`, and `team`. JSON mode is part of that contract, not a cosmetic enhancement. [Source: `_bmad-output/planning/prd.md`]
- The architecture treats CLI as the configuration and management entry surface; this story strengthens machine interoperability without changing gateway request flow. [Source: `_bmad-output/planning/architecture.md`]

### File Structure Requirements

- Expected primary touch points:
  - `src/main.rs`
  - `docs/current/guides/operations.md`
  - `tests/cli.rs`
- Possible secondary touch point:
  - a small internal helper extracted from `src/main.rs` if response formatting becomes too duplicated

### Testing Requirements

- Run `cargo test` at minimum.
- Because this changes operator-visible CLI behavior, also run `./scripts/test-local-e2e.sh` before marking the work merge-ready, per repo guidance.
- Use structured JSON assertions in tests, not plain substring matching.

### Previous Story Intelligence

- Story 3.5 establishes non-interactive invocation expectations for the same command families; this story should align to that command surface rather than expanding it.
- Existing `tests/cli.rs` already exercises command lifecycle flows and is the right regression harness to extend.

### References

- Epic story definition: [Source: `_bmad-output/planning/epics/e03-cli-management.md`]
- Cross-epic story list: [Source: `_bmad-output/planning/epics.md`]
- PRD CLI automation requirements: [Source: `_bmad-output/planning/prd.md`]
- Automation contract: [Source: `_bmad-output/planning/cli-ai-automation-contract.md`]
- Implementation spec: [Source: `_bmad-output/implementation/tech-specs/cli-ai-friendly-automation-v1.md`]
- Existing CLI code: [Source: `src/main.rs`]
- Existing CLI tests: [Source: `tests/cli.rs`]

## Dev Agent Record

### Agent Model Used

GPT-5

### Debug Log References

- Story created from Epic 3 JSON automation scope and CLI automation tech spec on 2026-03-29.

### Completion Notes List

- Implemented a shared JSON response envelope for `channel`, `router`, and `team` commands with stable top-level fields `ok`, `command`, `message`, `data`, `errors`, and `meta`.
- Preserved existing verb names in the machine contract, including `team.remove`.
- Added JSON success and failure regression coverage in `tests/cli.rs`.
- Addressed review findings so channel JSON responses no longer expose `api_key`, and common validation / missing-config failures now stay inside the JSON error contract.
- `cargo test --test cli` passed after implementation.
- Full suite execution is still environment-constrained where tests need local listener binds; that is tracked separately from this story's code changes.

### File List

- _bmad-output/implementation/stories/3-6-machine-readable-json-output-for-automation.md
- src/main.rs
- tests/cli.rs
- docs/current/guides/operations.md
