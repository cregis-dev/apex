# Apex Release Guide

This document is the maintainer-facing release runbook for Apex.

## Scope

Use this guide when you are preparing a GitHub Release and packaged installable artifacts for Apex.

Current release automation:

- GitHub workflow: `.github/workflows/build-release.yml`
- Trigger: push tag matching `v*`
- Artifacts:
  - `apex-x86_64-linux.tar.gz`
  - `apex-aarch64-linux.tar.gz`
  - `apex-x86_64-macos.tar.gz`
  - `apex-aarch64-macos.tar.gz`
  - `checksums.txt`
- Installer entrypoint: `install-release.sh`

Linux packaging notes:

- `apex-x86_64-linux.tar.gz` should be built from `x86_64-unknown-linux-musl`
- this avoids runtime failures on older cloud images with outdated glibc
- do not switch x86_64 Linux release builds back to `gnu` unless you are intentionally narrowing compatibility

## Versioning

Apex uses Semantic Versioning, with conservative rules while still in `0.x`.

- Bug fix only: bump `patch`
  - Example: `v0.1.0` -> `v0.1.1`
- New backward-compatible feature: bump `minor`
  - Example: `v0.1.1` -> `v0.2.0`
- Breaking change during `0.x`: also bump `minor`
  - Example: `v0.2.3` -> `v0.3.0`

Rule: if you publish a new package, you should publish a new version.

## Pre-Release Checklist

Before tagging a release, make sure:

1. The target branch is `main`.
2. The working tree is clean.
3. `CHANGELOG.md` reflects the release content.
4. Release-related docs are up to date.
5. The packaged install flow still matches the workflow outputs.

Minimum validation:

```bash
cargo test --quiet
./scripts/test-local-e2e.sh
```

Additional validation when changes touch provider integration or critical request flow, especially:

- `src/providers.rs`
- `src/server.rs`
- `src/router_selector.rs`
- `src/config.rs`
- `src/e2e.rs`
- `tests/e2e/`

If `.env.e2e` is available, also run:

```bash
./scripts/test-real-smoke.sh
```

If web or packaging changed, also run:

```bash
cd web
npm run build
cd ..
bash -n install-release.sh
```

## Standard Release Flow

Example below uses `v0.1.1`. Replace with the actual target version.

### 1. Update release notes

Update:

- `CHANGELOG.md`
- any affected install or deployment docs

If the crate version itself needs to move, update:

- `Cargo.toml`

### 2. Verify branch state

Make sure you are releasing from `main` and that the tree is clean:

```bash
git checkout main
git status --short
```

Expected result: no modified or untracked files you do not intend to release.

### 3. Pull latest main

```bash
git fetch origin
git pull --ff-only origin main
```

Do not create a release tag from stale local `main`.

### 4. Run release validation

```bash
cargo test --quiet
./scripts/test-local-e2e.sh
```

When required and credentials are available:

```bash
./scripts/test-real-smoke.sh
```

### 5. Commit release prep changes

```bash
git add .
git commit -m "release: prepare v0.1.1"
git push origin main
```

### 6. Create and push the release tag

```bash
git tag v0.1.1
git push origin v0.1.1
```

This tag push triggers `.github/workflows/build-release.yml`.

## Post-Release Verification

After pushing the tag:

1. Open GitHub Actions and confirm `Build Release` succeeded for the tag.
2. Open GitHub Releases and verify the release exists.
3. Confirm expected assets are attached:
   - platform `tar.gz` files
   - `checksums.txt`
4. Smoke-check installer behavior:

```bash
curl -fsSL https://raw.githubusercontent.com/cregis-dev/apex/main/install-release.sh | \
  bash -s -- --version v0.1.1 /tmp/apex-smoke
```

5. Confirm the installed binary starts with the expected config path.

## Main Branch vs Release Tag

The release is built from the tag, not from a later `main` commit.

That means:

- `v0.1.0` can be valid even if `main` later receives follow-up CI-only or docs-only fixes
- do not retag a published version to a different commit

If a post-release fix is needed, create a new version:

- bad: force-move `v0.1.0`
- good: release `v0.1.1`

## Common Failure Cases

### CI fails on `main`, but tag release succeeded

This can happen when:

- the tag was pushed first
- the release workflow succeeded
- a later `main` push triggered a separate failing CI run

Action:

- fix `main`
- merge and rerun CI
- do not move the existing release tag

### `git push origin main` is rejected

This means remote `main` advanced.

Safe recovery:

```bash
git fetch origin
git merge --no-edit origin/main
git push origin main
```

Then create or push the tag from the intended release commit.

If you already pushed the tag, keep the tag where it is and only reconcile `main`.

### Release workflow fails

Check:

- `.github/workflows/build-release.yml`
- package naming matches `install-release.sh`
- `checksums.txt` generation step
- GitHub Release creation step permissions

Fix on `main`, then publish a new version tag.

### Installer pulls package but validation fails

Check:

- artifact names still match platform detection in `install-release.sh`
- `checksums.txt` contains the uploaded filenames
- release assets were attached to the correct tag

### Linux runtime fails with `GLIBC_x.y not found`

Cause:

- the Linux binary was built against a newer glibc than the server provides

Preferred fix:

- publish a new release where `apex-x86_64-linux.tar.gz` is built from `x86_64-unknown-linux-musl`

Short-term workaround:

- build from source on the target host, or
- run Apex in a newer base image with a compatible glibc

## Maintainer Notes

- Keep release commits focused and auditable.
- Do not include local generated secrets or test env files in releases.
- Do not force-push `main` or move published tags.
- If release automation changes, update this document in the same PR.
