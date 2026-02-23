# Epic: Rule-Based Routing (E05)

## Description
Refactor the router configuration and logic to use a unified "Rule-Based" approach. This replaces the split `model_matcher` + `channels` design with a linear list of rules, where each rule contains its own matching criteria and execution strategy (including load balancing and failover).

## Goals
- **Unified Logic**: Merge "matching" and "load balancing" into a single `Rule` concept.
- **Enhanced Flexibility**: Allow different load balancing strategies per model (e.g., Round Robin for `gpt-4`, Priority for `gemini`).
- **In-Rule Failover**: Support automatic failover between channels within the same rule.
- **Backward Compatibility**: Automatically migrate existing configurations to the new Rule format.

## Stories

- [ ] **S01: Configuration Schema Update**
  - Define `RouterRule` and `MatchSpec` structs in `config.rs`.
  - Add `rules` field to `Router` struct.
  - Implement serde logic to load `rules` from JSON/YAML.

- [ ] **S02: Backward Compatibility & Migration**
  - Implement a migration function that converts legacy `metadata.model_matcher` and `channels` into `RouterRule`s.
  - Ensure old config files still work without modification.

- [ ] **S03: Router Selector Logic Update**
  - Update `RouterSelector` to iterate through `rules` instead of `model_matcher`.
  - Implement "match first" logic (stop on first match).
  - Update caching logic to support rule-based results.

- [ ] **S04: CLI & Documentation Update**
  - Update `apex router` CLI commands to support adding/listing rules (or at least display them correctly).
  - Update `operations.md` and architecture docs with new configuration examples.
