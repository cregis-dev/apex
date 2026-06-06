# Current Release Model

**Status:** Current source of truth for build and release behavior  
**Date:** 2026-03-11

## Summary

Apex currently uses a two-stage web release model:

1. build the Control Plane UI (Vite + React) into `target/web/cp`
2. build the Rust binary with `--features embedded-web`

Release binaries are expected to serve the Control Plane from embedded assets, not from a separately deployed `cp/` directory.

## Development Mode

Development keeps filesystem-based behavior available:

- frontend source lives in `cp/`
- frontend build output lives in `target/web/cp/`
- backend reads static assets from the fixed `target/web` export when not using `embedded-web`

Default filesystem asset directory:

```text
target/web
```

## Release Mode

Release mode should use:

```bash
cd cp
pnpm install
pnpm build

cd ..
cargo build --release --features embedded-web
```

Result:

- release artifact is the Rust binary
- Control Plane assets are embedded into that binary
- shipping a separate `cp/` directory is no longer required

## Config Semantics

`web_dir` is no longer part of the supported config surface.

Interpretation:

- local filesystem serving uses the fixed `target/web` export directory
- embedded release binaries do not need any web asset path in config
- legacy configs that still contain `web_dir` are tolerated for compatibility, but the field is retired

## Canonical Rules

- `cp/` is source only
- `target/web/` is the only static export directory (Control Plane assets under `target/web/cp/`)
- release automation should build `embedded-web`
- install flows should not copy `target/web` into deployment targets

## Related Files

- [`README.md`](/Users/shawn/workspace/coding/apex/README.md)
- [`install.sh`](/Users/shawn/workspace/coding/apex/install.sh)
- [`Cargo.toml`](/Users/shawn/workspace/coding/apex/Cargo.toml)
- [`src/web_assets.rs`](/Users/shawn/workspace/coding/apex/src/web_assets.rs)
- [`docs/current/guides/deployment.md`](/Users/shawn/workspace/coding/apex/docs/current/guides/deployment.md)
