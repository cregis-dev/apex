# Story 7.1: PII Masking Engine

Status: done

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
  - [x] 3.4 Integrated PII processing in server.rs::process_request
    - Fixed: Added PII masking and blocking logic before forwarding to upstream
    - Processes request body after config read and before router resolution
    - Block action returns 403 Forbidden with rule name
    - Mask action logs detection count and continues with masked body
- [x] Task 4: Audit Logging (AC: #7)
  - [x] 4.1 Log detection events using tracing (without original sensitive data)
  - [x] 4.2 Include rule name, action taken via structured logging
  - [x] 4.3 Added audit logging in process() method for mask actions
  - [x] 4.4 Added audit logging in should_block() method for block actions

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

- Epic: _bmad-output/planning/epics/e07-data-compliance.md
- Configuration schema参考: docs/current/reference/config-reference.md
- 现有中间件参考: src/middleware/auth.rs, src/middleware/ratelimit.rs

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List

- src/compliance.rs (new) - PII processing module with PiiProcessor
  - Added audit logging in process() and should_block() methods
  - Improved email regex pattern for better accuracy
  - Added Compliance::validate() method for config validation
  - Added edge case unit tests (11 tests total)
- src/config.rs - Added Compliance, PiiRule, PiiAction structs
  - Added Compliance::validate() impl
  - Added validation call in load_config()
- src/server.rs - Integrated PII processing in process_request()
  - Added imports for PiiProcessor and process_json_content
  - Added PII masking logic after config read, before router resolution
  - Block action returns 403 with descriptive error
- src/main.rs - Added compliance module declaration
- src/lib.rs - Already had compliance module declaration
- Cargo.toml - Added once_cell dependency
- tests/common/mod.rs - Added compliance: None to test configs
- tests/mcp_*.rs - Added compliance: None to test configs
- tests/e07_compliance_test.rs (new) - E2E integration tests (5 tests)
  - test_pii_masking_email
  - test_pii_blocking_credit_card
  - test_pii_disabled
  - test_pii_custom_rule
  - test_pii_multiple_detections

### Dev Agent Record

### Implementation Notes

**Code Review Fixes Applied (2026-03-12):**

1. **AC6 Implementation** - Integrated PII processing in server.rs
   - Added PII checking and masking in process_request() after config read
   - Block action now returns 403 Forbidden with rule name in error message
   - Mask action processes body and logs detection count

2. **AC7 Completion** - Added comprehensive audit logging
   - Added warn! logging in process() method when PII detected
   - Added warn! logging in should_block() method for block actions
   - Logs include: rule name, action type, detection count
   - Logs do NOT include original sensitive data (compliant)

3. **Quality Improvements**
   - Improved email regex pattern (more restrictive, fewer false positives)
   - Added Compliance::validate() for config validation
   - Added 6 additional edge case tests: disabled compliance, empty text, multiple PII, invalid JSON fallback, replace_with option, validation tests
   - Created E07 integration test suite with 5 end-to-end tests
   - Fixed module declaration in main.rs

**Test Coverage:**
- Unit tests: 11/11 passed (compliance.rs)
- Integration tests: 5/5 passed (e07_compliance_test.rs)
- All other tests: passing (no regressions)

**Original Notes:**
- PII Processor provides should_block() and process() methods
- process_json_content() handles JSON parsing and masking
- Built-in rules: email, phone, credit_card, ip_address
