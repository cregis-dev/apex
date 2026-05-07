---
title: 'CLI AI-Friendly Automation v1 for Channel, Router, and Team'
slug: 'cli-ai-friendly-automation-v1'
created: '2026-03-29'
status: 'ready-for-dev'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['Rust', 'clap', 'serde_json']
files_to_modify:
  - '/Users/shawn/workspace/code/apex/src/main.rs'
  - '/Users/shawn/workspace/code/apex/docs/current/guides/operations.md'
  - '/Users/shawn/workspace/code/apex/tests/'
  - '/Users/shawn/workspace/code/apex/_bmad-output/planning/cli-ai-automation-contract.md'
code_patterns:
  - 'CLI command definitions and handlers are centralized in src/main.rs using clap derive macros.'
  - 'Current CLI mixes flag-driven flows with inquire-based interactive prompts when required args are missing.'
  - 'Some list commands already expose --json, but add/update/delete flows mostly print human-readable text only.'
  - 'Team, router, and channel command families are not fully symmetric today; the implementation should improve automation without pretending unsupported verbs already exist.'
test_patterns:
  - 'cargo test'
  - 'Add CLI-focused regression coverage that validates JSON shape and non-interactive execution.'
---

# Tech-Spec: CLI AI-Friendly Automation v1 for Channel, Router, and Team

**Created:** 2026-03-29

## Overview

### Problem Statement

Apex is repositioning the local CLI as the primary automation surface for future AI skills and configuration workflows. The current CLI already supports `channel`, `router`, and `team` management, but it is not yet consistently automation-safe because:

- required inputs are not always fully expressible through arguments without falling back to prompts
- JSON output exists only in limited places and lacks a stable contract
- human-readable output is mixed into operations that future skills will need to parse programmatically
- command families are inconsistent today (`team remove` vs `channel delete`, missing `show` on some resources)

If this is not normalized enough for automation, skills will depend on brittle screen-scraping and prompt emulation. That is the wrong control surface.

### Solution

Upgrade the existing CLI implementation so the v1 automation scope, limited to `channel`, `router`, and `team`, can be invoked non-interactively and can return stable JSON responses. Use the planning contract in [`/Users/shawn/workspace/code/apex/_bmad-output/planning/cli-ai-automation-contract.md`](/Users/shawn/workspace/code/apex/_bmad-output/planning/cli-ai-automation-contract.md) as the response-shape source of truth.

### Scope

**In Scope:**
- Make `apex channel`, `apex router`, and `apex team` usable by skills without requiring TTY prompts when required arguments are provided.
- Add or extend `--json` support for the v1 command set.
- Standardize success and failure payloads around the agreed top-level JSON contract.
- Document which actions are supported in v1 and which are explicitly unavailable.
- Add regression coverage for CLI JSON shape and non-interactive behavior.

**Out of Scope:**
- Reworking unrelated commands such as `apex init`, `apex gateway`, `apex status`, or `apex logs`
- Introducing remote admin APIs or Admin Control Plane functionality
- Full CLI redesign or splitting command handlers out of `src/main.rs`
- Renaming all existing verbs for symmetry if that would create avoidable migration risk

## Context for Development

### Current Command Reality

The current CLI in [`/Users/shawn/workspace/code/apex/src/main.rs`](/Users/shawn/workspace/code/apex/src/main.rs) does not present one perfectly uniform CRUD surface:

- `channel`: `add`, `update`, `delete`, `show`, `list`
- `router`: `add`, `update`, `delete`, `list`
- `team`: `add`, `remove`, `list`

This matters. The implementation should support automation on the commands that exist today, and it should explicitly document unsupported verbs instead of implying false parity.

### Codebase Patterns

- CLI parsing and handler execution live in a single file: [`/Users/shawn/workspace/code/apex/src/main.rs`](/Users/shawn/workspace/code/apex/src/main.rs)
- `clap` derive macros define subcommands and args
- interactive behavior currently depends on `inquire` prompts inside handler code when args are omitted
- `channel list` and `router list` already support `--json`, which provides a starting pattern
- config changes are applied by loading config, mutating in-memory structs, and persisting via `config::save_config`

### Files to Reference

| File | Purpose |
| ---- | ------- |
| [/Users/shawn/workspace/code/apex/src/main.rs](/Users/shawn/workspace/code/apex/src/main.rs) | CLI command definitions, argument parsing, interactive prompts, and handler implementations |
| [/Users/shawn/workspace/code/apex/_bmad-output/planning/cli-ai-automation-contract.md](/Users/shawn/workspace/code/apex/_bmad-output/planning/cli-ai-automation-contract.md) | Agreed v1 automation scope and JSON response contract |
| [/Users/shawn/workspace/code/apex/docs/current/guides/operations.md](/Users/shawn/workspace/code/apex/docs/current/guides/operations.md) | Existing operator-facing CLI usage documentation |
| [/Users/shawn/workspace/code/apex/tests/](/Users/shawn/workspace/code/apex/tests/) | Existing test area; likely place to add CLI regression coverage or command-shape assertions |

### Technical Decisions

1. `--json` will be treated as the automation contract for v1 within `channel`, `router`, and `team`.
2. Existing command verbs may remain as-is for v1 to avoid unnecessary churn. That means `team remove` can remain `remove` instead of forcing a rename to `delete`.
3. JSON output should use the invoked action name in `command` and `meta.action`, for example:
   - `channel.add`
   - `router.list`
   - `team.remove`
4. Unsupported actions should be documented, not silently implied. For example:
   - `router show`: unavailable unless added in this change
   - `team show`: unavailable unless added in this change
   - `team update`: unavailable unless added in this change
5. When all required arguments are present, commands in scope must not open `inquire` prompts.
6. Human-readable output can remain for default mode, but `--json` mode must not mix in extra free-form lines.
7. Error handling in JSON mode must use stable error codes and preserve the agreed top-level shape:
   - `ok`
   - `command`
   - `message`
   - `data`
   - `errors`
   - `meta`

## Implementation Plan

### Tasks

- [ ] Task 1: Audit and annotate the v1 CLI automation surface
  - File: [/Users/shawn/workspace/code/apex/src/main.rs](/Users/shawn/workspace/code/apex/src/main.rs)
  - Action:
    1. Confirm each supported action under `channel`, `router`, and `team`
    2. Identify where interactive prompts still trigger
    3. Identify where JSON output already exists and where it does not
  - Notes: This should produce an explicit matrix of supported and unsupported verbs before changing behavior.

- [ ] Task 2: Add JSON mode consistently to the v1 command set
  - File: [/Users/shawn/workspace/code/apex/src/main.rs](/Users/shawn/workspace/code/apex/src/main.rs)
  - Action:
    1. Extend `--json` support beyond current list commands
    2. Ensure add/update/delete-or-remove/show/list commands emit structured JSON where supported
    3. Ensure default text mode remains usable for humans
  - Notes: JSON mode must not print extra status lines outside the response body.

- [ ] Task 3: Introduce a shared CLI response formatter
  - File: [/Users/shawn/workspace/code/apex/src/main.rs](/Users/shawn/workspace/code/apex/src/main.rs)
  - Action:
    1. Add a shared helper or internal response struct for success and failure output
    2. Centralize `serde_json` serialization for the v1 command set
    3. Normalize `command` and `meta` values based on resource and invoked action
  - Notes: The goal is to avoid hand-building slightly different JSON payloads in each branch.

- [ ] Task 4: Remove prompt dependency when explicit args are sufficient
  - File: [/Users/shawn/workspace/code/apex/src/main.rs](/Users/shawn/workspace/code/apex/src/main.rs)
  - Action:
    1. Refactor `channel add` and `channel update` flows so prompt fallback only happens when required fields are missing
    2. Refactor `router add` and `router update` flows with the same rule
    3. Confirm `team add` already behaves non-interactively and keep that path clean
  - Notes: This task is about predictable automation behavior, not removing interactive UX entirely.

- [ ] Task 5: Make unsupported actions explicit in docs and help text
  - File: [/Users/shawn/workspace/code/apex/docs/current/guides/operations.md](/Users/shawn/workspace/code/apex/docs/current/guides/operations.md)
  - Action:
    1. Document `--json` examples for `channel`, `router`, and `team`
    2. Document the supported action set for each command family
    3. Call out that some verbs are intentionally unavailable in v1 where applicable
  - Notes: Skills and operators both need a visible contract.

- [ ] Task 6: Add CLI regression coverage
  - File: [/Users/shawn/workspace/code/apex/tests/](/Users/shawn/workspace/code/apex/tests/)
  - Action:
    1. Add tests that execute representative CLI commands against temp config fixtures
    2. Validate JSON success shape and JSON error shape
    3. Validate that fully-parameterized invocations do not depend on TTY input
  - Notes: Without tests, the contract will drift.

## Acceptance Criteria

- [ ] AC 1: Given `apex channel add` is invoked with all required arguments and `--json`, when the command succeeds, then it returns a single JSON payload with top-level fields `ok`, `command`, `message`, `data`, `errors`, and `meta`, and it does not prompt for missing input.
- [ ] AC 2: Given `apex router add` is invoked with all required arguments and `--json`, when the command succeeds, then it returns the same stable top-level JSON shape and does not enter interactive selection.
- [ ] AC 3: Given `apex team add` is invoked with all required arguments and `--json`, when the command succeeds, then it returns the same stable top-level JSON shape including the created team result and generated API key data required by automation.
- [ ] AC 4: Given a validation or not-found failure occurs in `channel`, `router`, or `team` commands under `--json`, when the command exits, then the response still preserves the same top-level JSON shape and includes at least one machine-readable error code.
- [ ] AC 5: Given a supported `list` or `show` style command in the v1 scope is invoked with `--json`, when it succeeds, then `data` contains the requested entity or collection and `errors` is an empty array.
- [ ] AC 6: Given a command family does not support a verb in v1, when the user consults help or operations documentation, then the unsupported action is explicit rather than implied.
- [ ] AC 7: When `cargo test` is run, then existing tests continue to pass and new CLI-focused regression tests cover JSON mode and non-interactive execution for the v1 scope.

## Additional Context

### Dependencies

- No new product dependency is required.
- Implementation may add internal helper structs or small utility functions in `src/main.rs` or a nearby module if that reduces duplication.

### Testing Strategy

- Run `cargo test`
- Prefer fixture-based CLI tests over manual-only verification
- Include at least one success and one failure case for each command family in v1 scope

### Risks

- Mixing human-readable status lines into JSON mode will break skills immediately.
- Trying to force perfect CRUD symmetry in one pass may expand scope beyond what is needed for v1.
- Keeping all logic in `src/main.rs` may increase complexity unless shared response helpers are introduced early.

### Notes

- This spec intentionally does not include the HTTP MCP removal work; that should be handled as a separate implementation change stream even though both changes serve the same product direction.
- The product goal is not “make CLI pretty.” The goal is “make CLI usable as a stable control surface for skills.”
