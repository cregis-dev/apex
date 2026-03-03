# Story 7.1: PII Masking Engine

Status: review

> **Note**: This story is **partially complete**. AC6 (server integration) requires a fix for Rust Edition 2024 + axum compatibility issue. The core PII processing logic is implemented and tested.

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a enterprise team administrator,
I want to configure PII (Personally Identifiable Information) detection and masking rules,
so that sensitive data is automatically detected and masked before being sent to external LLM providers.

## Acceptance Criteria

1. [AC1] Gateway supports enabling/disabling PII masking via configuration
2. [AC2] Built-in PII patterns are available for: Email, Phone Number, Credit Card, IP Address
3. [AC3] Users can define custom Regex patterns for PII detection
4. [AC4] Mask action replaces sensitive data with configurable characters (e.g., `****@*****.com`)
5. [AC5] Block action rejects requests containing detected PII
6. [AC6] PII masking is applied before forwarding requests to providers
7. [AC7] Audit log records when PII is detected (without logging original sensitive data)

## Tasks / Subtasks

- [x] Task 1: Config Schema - Add Compliance configuration (AC: #1)
  - [x] 1.1 Add `compliance` section to Config struct
  - [x] 1.2 Define `PiiRule` struct with name, pattern, action, mask_char
  - [x] 1.3 Support built-in rules and custom rules
- [x] Task 2: PII Processor - Implement regex matching and replacement (AC: #2, #3, #4, #5)
  - [x] 2.1 Implement PiiProcessor struct with pre-compiled regex patterns
  - [x] 2.2 Implement `mask()` method for replacement
  - [x] 2.3 Implement `block()` method for rejection
  - [x] 2.4 Support custom rule configuration
- [x] Task 3: Request Processing Integration (AC: #6)
  - [x] 3.1 Added compliance module with PII processing functions
  - [x] 3.2 Process JSON content in requests
  - [x] 3.3 Added process_json_content function for use in server
  - [ ] 3.4 Integration in server.rs (blocked by Rust edition 2024 + axum compatibility issue)
    - Note: All PII processing functions are implemented and tested
    - Integration requires fixing axum middleware compatibility with Rust edition 2024
- [x] Task 4: Audit Logging (AC: #7)
  - [x] 4.1 Log detection events using tracing (without original sensitive data)
  - [x] 4.2 Include rule name, action taken via structured logging

## Dev Notes

- Relevant architecture patterns and constraints
- Source tree components to touch
- Testing standards summary

### Project Structure Notes

- Alignment with unified project structure (paths, modules, naming)
- Detected conflicts or variances (with rationale)

### Technical Implementation Notes

- Use `regex` crate for pattern matching
- Use `once_cell` or `lazy_static` for pre-compiled regex patterns
- Middleware should be placed in `src/middleware/` directory
- Config should follow existing pattern in `src/config.rs`
- Consider JSON parsing for Chat Completions API format

### References

- Epic: docs/epics/E07-Data-Compliance.md
- Configuration schema参考: docs/config-example.md
- 现有中间件参考: src/middleware/auth.rs, src/middleware/ratelimit.rs

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List

- src/compliance.rs (new) - PII processing module with PiiProcessor
- src/config.rs - Added Compliance, PiiRule, PiiAction structs
- Cargo.toml - Added once_cell dependency
- tests/common/mod.rs, tests/mcp_*.rs - Added compliance: None to test configs

### Dev Agent Record

### Implementation Notes

- PII Processor provides should_block() and process() methods
- process_json_content() handles JSON parsing and masking
- Integration in server.rs requires modifying process_request to handle async properly
- Built-in rules: email, phone, credit_card, ip_address
