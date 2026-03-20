## Summary

<!-- What changed and why? Keep this to a few short paragraphs or bullets. -->

## Scope

- [ ] `src/config.rs`
- [ ] `src/server.rs`
- [ ] `src/router_selector.rs`
- [ ] `src/providers.rs`
- [ ] `src/e2e.rs`
- [ ] `src/mcp/`
- [ ] `tests/e2e/`
- [ ] Docs / examples / scripts only

## Risk Check

- [ ] This change affects request routing
- [ ] This change affects authentication or authorization
- [ ] This change affects provider protocol compatibility
- [ ] This change affects config generation or hot reload
- [ ] This change affects MCP behavior
- [ ] This change requires migration or rollout notes

## Validation

- [ ] `cargo test`
- [ ] `./scripts/test-local-e2e.sh`
- [ ] `./scripts/test-real-smoke.sh` if this PR touches `src/providers.rs`, `src/server.rs`, `src/router_selector.rs`, `src/config.rs`, `src/e2e.rs`, `tests/e2e/`, or real upstream integration

## Real Provider Smoke

- [ ] Not needed for this PR
- [ ] Ran against local `.env.e2e`
- [ ] Ran in GitHub Actions `Real Provider Smoke`
- [ ] Could not run because credentials or `.env.e2e` were unavailable

## Notes For Reviewers

<!-- Any known tradeoffs, follow-ups, rollout cautions, or areas you want reviewed closely. -->

## Docs

- [ ] No doc changes needed
- [ ] Updated docs
- [ ] Updated config examples
- [ ] Updated `.env.e2e.example`
