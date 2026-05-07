# Story 3.5: AI-Friendly Non-Interactive CLI Inputs

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As an AI skill or automation workflow,
I want `apex channel`, `apex router`, and `apex team` commands to run fully from command arguments,
so that I can manage Apex configuration without relying on TTY prompts or interactive selections.

## Acceptance Criteria

1. [AC1] The v1 automation scope is explicitly limited to the existing `channel`, `router`, and `team` command families. The implementation must not silently expand into unrelated commands.
2. [AC2] For supported actions in the v1 scope, when all required inputs are supplied through flags or arguments, the CLI does not open `inquire` prompts or require TTY interaction.
3. [AC3] Explicit arguments take precedence over any existing interactive fallback behavior.
4. [AC4] The supported action set for each command family is explicit in docs/help text, and unsupported verbs are not implied as available.
5. [AC5] Existing human-friendly interactive flows may remain as fallback when required inputs are missing, but they are optional rather than mandatory for the v1 scope.
6. [AC6] Regression coverage proves that representative `channel`, `router`, and `team` commands succeed non-interactively when invoked with complete arguments.

## Tasks / Subtasks

- [ ] Task 1: Establish the v1 automation support matrix in the CLI surface (AC: #1, #4)
  - [ ] 1.1 Confirm the real command/action surface in `src/main.rs`:
    - `channel`: `add`, `update`, `delete`, `show`, `list`
    - `router`: `add`, `update`, `delete`, `list`
    - `team`: `add`, `remove`, `list`
  - [ ] 1.2 Keep existing verbs intact for v1; do not force symmetry such as renaming `team remove` to `delete` in this story.
  - [ ] 1.3 Make unsupported verbs explicit in operator-facing documentation rather than implying full CRUD parity.
- [ ] Task 2: Remove prompt dependency when explicit args are sufficient for `channel` flows (AC: #2, #3, #5)
  - [ ] 2.1 Refactor `handle_channel_command()` so `add` does not prompt for provider/base URL/API key when those required values are already supplied.
  - [ ] 2.2 Refactor `handle_channel_command()` so `update` only falls back to prompt-based behavior when required values are missing and interactive fallback is intended.
  - [ ] 2.3 Preserve current validation and config-save behavior when moving prompt boundaries.
- [ ] Task 3: Remove prompt dependency when explicit args are sufficient for `router` flows (AC: #2, #3, #5)
  - [ ] 3.1 Refactor `handle_router_command()` so `add` does not open channel selection when `--channels` is already supplied.
  - [ ] 3.2 Refactor `handle_router_command()` so `update` remains deterministic when explicit arguments are supplied, and only enters the existing interactive path when no explicit updates are provided.
  - [ ] 3.3 Preserve current channel existence validation and rule construction.
- [ ] Task 4: Keep `team` flows clean for automation (AC: #2, #3, #5)
  - [ ] 4.1 Confirm `team add`, `team remove`, and `team list` remain fully argument-driven and do not regress into prompt-driven behavior.
  - [ ] 4.2 Keep generated API key output available for downstream automation consumers in later JSON-focused work.
- [ ] Task 5: Document the automation-safe command surface (AC: #4)
  - [ ] 5.1 Update `docs/current/guides/operations.md` to document non-interactive examples for `channel`, `router`, and `team`.
  - [ ] 5.2 Explicitly document the supported action set for each command family.
  - [ ] 5.3 Avoid promising unsupported actions such as `team update` or `router show` unless they are actually added.
- [ ] Task 6: Add regression coverage for non-interactive execution (AC: #6)
  - [ ] 6.1 Extend `tests/cli.rs` with representative fully-parameterized `channel` and `router` flows that prove no prompt is required.
  - [ ] 6.2 Extend CLI or related tests for `team` flows as needed.
  - [ ] 6.3 Ensure tests validate successful config mutation rather than only process exit status.

## Dev Notes

- This story is about deterministic invocation behavior, not about JSON output. JSON response contract work belongs to Story 3.6 and should not be smeared into this story unless needed for compile-time integration.
- The current CLI implementation is centralized in `src/main.rs`, and `inquire` prompts are embedded directly in command handlers. That means the safest implementation path is to narrow prompt entry points rather than attempting a broad CLI refactor.
- The command surface is not symmetric today. That is acceptable for v1. The requirement is “automation-safe for the commands that exist,” not “invent new verbs everywhere.”

### Project Structure Notes

- CLI parsing and handlers live in `src/main.rs`.
- Existing CLI regression coverage already lives in `tests/cli.rs`.
- Team policy and gateway tests live elsewhere and should not be used as a substitute for direct CLI behavior assertions.

### Technical Requirements

- Preserve the configuration-driven architecture: CLI changes should continue to mutate persisted JSON config through existing `config::load_config` and `config::save_config` flows. [Source: `_bmad-output/planning/architecture.md` - Configuration and CLI]
- Preserve current validation behavior for channels, routers, and teams; the story is about invocation mode, not relaxed validation. [Source: `src/main.rs`]
- Keep interactive fallback available when required inputs are absent, but do not let prompt code run when explicit args already make the operation deterministic. [Source: `_bmad-output/planning/cli-ai-automation-contract.md`]

### Architecture Compliance

- The planning architecture already identifies CLI as part of the core configuration surface. This story strengthens that surface for AI skills without changing gateway request-path architecture. [Source: `_bmad-output/planning/architecture.md`]
- PRD FR7 now defines the AI-friendly CLI surface as non-interactive and automation-usable for the v1 scope. [Source: `_bmad-output/planning/prd.md`]

### File Structure Requirements

- Expected primary touch points:
  - `src/main.rs`
  - `docs/current/guides/operations.md`
  - `tests/cli.rs`
- Avoid broad edits to gateway runtime, provider adapters, or dashboard code for this story.

### Testing Requirements

- Run `cargo test` at minimum.
- Because this changes CLI behavior that operators depend on, also run `./scripts/test-local-e2e.sh` before marking the work merge-ready, per repo guidance.
- Reuse the existing `assert_cmd`-based CLI test style in `tests/cli.rs`.

### Previous Story Intelligence

- Stories `3.1` through `3.4` already established the basic CLI surface. This story is an extension of that work, not a redesign.
- Current `channel add/update` and `router add/update` handlers still contain direct `inquire` calls in `src/main.rs`; those are the primary risk points for non-interactive automation.

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

- Story created from Epic 3 CLI expansion and CLI automation tech spec on 2026-03-29.

### Completion Notes List

- Implemented non-interactive argument-driven flows for the v1 scope `channel`, `router`, and `team` without forcing new verbs.
- CLI regression coverage was extended in `tests/cli.rs` for fully-parameterized non-interactive channel, router, and team flows.
- `cargo test --test cli` passed after implementation.
- Follow-up review findings were addressed so `--json` mode now also covers missing-config and validation-error paths used by automation.
- Full `cargo test` and `./scripts/test-local-e2e.sh` remain partially blocked in this environment by local listener bind permission failures unrelated to the story logic.

### File List

- _bmad-output/implementation/stories/3-5-ai-friendly-non-interactive-cli-inputs.md
- src/main.rs
- tests/cli.rs
- docs/current/guides/operations.md
