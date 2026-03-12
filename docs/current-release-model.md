# Current Release Model

**Status:** Current source of truth for build and release behavior  
**Date:** 2026-03-11

## Summary

Apex currently uses a two-stage web release model:

1. build the Next.js dashboard as a static export into `target/web`
2. build the Rust binary with `--features embedded-web`

Release binaries are expected to serve the dashboard from embedded assets, not from a separately deployed `web/` directory.

## Development Mode

Development keeps filesystem-based behavior available:

- frontend source lives in `web/`
- frontend export output lives in `target/web/`
- backend can read static assets from `web_dir`

Default filesystem asset directory:

```text
target/web
```

## Release Mode

Release mode should use:

```bash
cd web
npm install
npm run build

cd ..
cargo build --release --features embedded-web
```

Result:

- release artifact is the Rust binary
- dashboard assets are embedded into that binary
- shipping a separate `web/` directory is no longer required

## Config Semantics

`web_dir` remains in config for filesystem mode compatibility.

Interpretation:

- required for local or fallback filesystem serving
- not required for embedded release binaries
- recommended value in filesystem mode: `target/web`

## Canonical Rules

- `web/` is source only
- `target/web/` is the only static export directory
- release automation should build `embedded-web`
- install flows should not copy `target/web` into deployment targets

## Related Files

- [`README.md`](/Users/shawn/workspace/code/apex/README.md)
- [`install.sh`](/Users/shawn/workspace/code/apex/install.sh)
- [`Cargo.toml`](/Users/shawn/workspace/code/apex/Cargo.toml)
- [`src/web_assets.rs`](/Users/shawn/workspace/code/apex/src/web_assets.rs)
- [`docs/deployment-guide.md`](/Users/shawn/workspace/code/apex/docs/deployment-guide.md)
