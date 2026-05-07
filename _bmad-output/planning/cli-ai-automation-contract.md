# CLI AI Automation Contract (v1)

## Purpose

Define the first stable automation contract for Apex CLI so AI skills and scripts can invoke configuration workflows deterministically.

## v1 Command Scope

The initial AI-oriented command set is limited to:

- `apex channel`
- `apex router`
- `apex team`

The following command families are explicitly out of scope for this contract version:

- `apex init`
- `apex gateway`
- any future diagnostic, dashboard, or provider-specific commands not listed above

## v1 Automation Principles

1. Commands in scope must support non-interactive execution when required arguments are provided.
2. When both flags and interactive prompting are available, explicit arguments take precedence.
3. `--json` is the machine contract for v1 automation workflows.
4. Human-readable output may vary, but JSON output should remain stable across minor revisions.

## v1 Action Scope

The expected automation-facing actions are:

- `add`
- `update`
- `delete`
- `list`
- `show`

If a command family does not yet support one of these actions, the gap should be explicit in help text and planning, not hidden behind interactive behavior.

## Input Contract Expectations

For `channel`, `router`, and `team` commands:

1. All fields required for a successful operation must be expressible through flags or arguments.
2. Commands must not require TTY prompts when all required inputs are already supplied.
3. Validation failures should identify the invalid field or missing input when possible.
4. Help text should distinguish:
   - required parameters for non-interactive use
   - optional parameters
   - parameters that are only meaningful in interactive mode, if any remain

## JSON Output Contract (v1)

### Success Shape

```json
{
  "ok": true,
  "command": "channel.add",
  "message": "Channel created.",
  "data": {},
  "errors": [],
  "meta": {
    "resource": "channel",
    "action": "add"
  }
}
```

### Error Shape

```json
{
  "ok": false,
  "command": "channel.add",
  "message": "Validation failed.",
  "data": null,
  "errors": [
    {
      "code": "missing_required_field",
      "message": "base_url is required.",
      "field": "base_url"
    }
  ],
  "meta": {
    "resource": "channel",
    "action": "add"
  }
}
```

## Field Definitions

- `ok`: Boolean success indicator for simple automation branching.
- `command`: Stable command identifier in `<resource>.<action>` form.
- `message`: Short summary intended for logs and operator readability.
- `data`: Operation result payload. For list operations this should contain the returned collection; for show/add/update it should contain the resulting entity; for delete it may contain the deleted identifier or a minimal confirmation object.
- `errors`: Array of structured error entries. Empty on success.
- `meta`: Stable metadata describing the command context.

### Error Entry Fields

- `code`: Stable machine-readable error code.
- `message`: Human-readable explanation.
- `field`: Optional field name for validation or input-related failures.

## JSON Stability Rules

1. Top-level keys must remain consistent for all v1 in-scope commands.
2. `errors` must always be present, even on success.
3. `data` must always be present. Use `null` when no payload is available.
4. `meta.resource` and `meta.action` must always be present.
5. New fields may be added in the future only if existing fields remain backward-compatible.

## Command Examples

### Channel Add

```bash
apex channel add --id openai-prod --type openai --base-url https://api.openai.com/v1 --api-key env:OPENAI_API_KEY --json
```

### Router List

```bash
apex router list --json
```

### Team Show

```bash
apex team show --id marketing-bot --json
```

## Acceptance Guidance

This contract should be considered satisfied only when:

1. A skill can create, inspect, update, and remove `channel`, `router`, and `team` entities without interactive prompts.
2. Success and failure paths both return parseable JSON with the same top-level structure.
3. Validation and missing-input failures can be interpreted programmatically without scraping free-form text.
