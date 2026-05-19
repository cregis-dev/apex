---
title: 'Apex CLI Operations, Service Management, and Upgrade UX'
slug: 'apex-cli-operations-service-management-and-upgrade-ux'
created: '2026-05-18'
status: 'implementation-complete'
stepsCompleted: [1, 2, 3, 4]
tech_stack: ['Rust 2024', 'clap derive', 'serde/serde_json', 'Tokio/Axum', 'Bash installer scripts', 'systemd', 'launchd', 'assert_cmd CLI tests']
files_to_modify:
  - '/Users/shawn/workspace/code/apex/src/main.rs'
  - '/Users/shawn/workspace/code/apex/src/config.rs'
  - '/Users/shawn/workspace/code/apex/src/service.rs'
  - '/Users/shawn/workspace/code/apex/src/upgrade.rs'
  - '/Users/shawn/workspace/code/apex/src/install_metadata.rs'
  - '/Users/shawn/workspace/code/apex/install-release.sh'
  - '/Users/shawn/workspace/code/apex/install.sh'
  - '/Users/shawn/workspace/code/apex/tests/cli.rs'
  - '/Users/shawn/workspace/code/apex/docs/current/guides/deployment.md'
  - '/Users/shawn/workspace/code/apex/docs/current/guides/operations.md'
  - '/Users/shawn/workspace/code/apex/RELEASE.md'
code_patterns:
  - 'CLI command definitions currently live in src/main.rs with clap Parser/Subcommand/Args derive macros.'
  - 'Config mutations and validation use config::load_config and config::save_config; load_config also performs compatibility migration for legacy router fields.'
  - 'Existing service-like behavior is daemonize-based gateway start --daemon with pid/log files; production service support should use native service managers instead of expanding this path.'
  - 'Release installer currently downloads a platform tarball, verifies checksums when available, and copies the binary directly to TARGET_DIR/apex.'
  - 'Release artifacts are named apex-x86_64-linux, apex-aarch64-linux, apex-x86_64-macos, and apex-aarch64-macos and include config.example.json.'
test_patterns:
  - 'CLI regression tests live in tests/cli.rs and use assert_cmd, predicates, serde_json, and tempfile.'
  - 'Existing CLI tests create temp config files with --config and inspect persisted JSON.'
  - 'Installer validation should include bash -n install-release.sh and bash -n install.sh.'
  - 'Platform service generation should be testable as pure string/path rendering without requiring systemctl or launchctl in unit tests.'
---

# Tech-Spec: Apex CLI Operations, Service Management, and Upgrade UX

**Created:** 2026-05-18

## Overview

### Problem Statement

Apex is operationally harder than it needs to be. Users are guided toward long `--config` commands, production service setup is manual, and upgrades require manually re-running scripts without a first-class rollback-safe flow. The current code already defaults to `~/.apex/config.json`, but the CLI surface, install script, and deployment docs do not consistently make the simple path obvious.

### Solution

Add a small, stable operator layer to the Apex CLI: simple config path resolution, explicit config diagnostics, service management backed by native service managers, and rollback-safe upgrades using versioned release directories. Align the UX with common open-source operator tools such as Caddy, Traefik, and Homebrew services while keeping Apex's implementation scope conservative.

### Scope

**In Scope:**
- Config resolution order only:
  1. `--config` / `-c`
  2. `APEX_CONFIG`
  3. `~/.apex/config.json`
- `apex config path`
- `apex config validate`
- `apex gateway run` as the clear foreground command, while keeping existing `apex gateway start` compatible
- `apex service install|uninstall|start|stop|restart|status|logs`
- Linux service management through systemd
- macOS service management through launchd
- `install-release.sh --service` support
- Install metadata to support later service and upgrade commands
- `apex upgrade --dry-run|--version|--restart`
- Rollback-safe upgrade layout using versioned release directories and a `current` symlink so upgrades do not overwrite an in-use binary
- Rollback on failed restart or health check
- Tests and operator documentation updates

**Out of Scope:**
- Current-directory `config.json` auto-discovery
- Multi-instance service management
- Windows service/self-upgrade support
- Automatic background updates
- TUI or broad CLI redesign unrelated to operations
- Changing provider, router, team, or request-routing semantics

## Context for Development

### Codebase Patterns

- CLI parsing and command handlers are centralized in `src/main.rs` using `clap` derive macros. The file is already large, so new service, install metadata, and upgrade logic should move into dedicated modules and keep `src/main.rs` as the command dispatch layer.
- The existing global `--config` flag resolves directly to `~/.apex/config.json` when omitted. It does not currently support `-c` or `APEX_CONFIG`.
- `expand_path()` is currently used by config and log paths, but an empty string maps to the default log directory. The config resolver should avoid relying on this behavior for environment-derived config paths.
- Current service-like behavior is implemented as `apex gateway start --daemon`, using `daemonize`, a pid file, and log files. This should remain compatible but should not become the production service path.
- `apex status` and `apex logs` currently inspect the daemon pid/log files, not systemd or launchd. New `apex service status/logs` should be separate so existing behavior does not silently change.
- `server::run_server(path)` reads config directly with `serde_json::from_str`, then starts file-watch hot reload if `hot_reload.watch` is true. `config::load_config()` performs additional validation and legacy migration, so `apex config validate` should use `config::load_config()`.
- There is no `/health` route today. Metrics exist at `/metrics` only when enabled and protected by global auth, so upgrade health checks should not depend on unauthenticated metrics. Either add a tiny unauthenticated health endpoint or make v1 upgrade health check process/service status only.
- Existing install flows live in `install-release.sh` for GitHub release artifacts and `install.sh` for local source-based installation. Both currently copy directly to `$TARGET_DIR/apex` and print long `--config` startup examples.
- Release artifacts are produced by `.github/workflows/build-release.yml`; tarballs contain `apex` and `config.example.json`, and `checksums.txt` is generated from the tarballs.
- Existing docs still frequently show long `apex gateway start --config /path/to/config.json` examples and manual systemd setup.

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `/Users/shawn/workspace/code/apex/src/main.rs` | Existing CLI command definitions, config path resolution, daemon handling, status, and logs commands |
| `/Users/shawn/workspace/code/apex/src/config.rs` | Config load/save behavior and validation-adjacent parsing |
| `/Users/shawn/workspace/code/apex/src/server.rs` | Server startup, existing hot reload watcher, metrics routes, and absence of a dedicated health route |
| `/Users/shawn/workspace/code/apex/src/service.rs` | New module to create for service definition rendering and platform service command execution |
| `/Users/shawn/workspace/code/apex/src/upgrade.rs` | New module to create for release download, checksum, versioned directory install, symlink switching, restart, and rollback |
| `/Users/shawn/workspace/code/apex/src/install_metadata.rs` | New module to create for reading/writing install metadata shared by service and upgrade commands |
| `/Users/shawn/workspace/code/apex/install-release.sh` | Release installer that should learn service setup and install metadata |
| `/Users/shawn/workspace/code/apex/install.sh` | Source-based local installer that should align final usage messaging |
| `/Users/shawn/workspace/code/apex/docs/current/guides/deployment.md` | Production deployment docs with current manual systemd setup |
| `/Users/shawn/workspace/code/apex/docs/current/guides/operations.md` | Operator guide for day-to-day CLI usage |
| `/Users/shawn/workspace/code/apex/RELEASE.md` | Release and artifact model that upgrade must respect |
| `/Users/shawn/workspace/code/apex/tests/cli.rs` | Existing CLI regression tests using `assert_cmd` |
| `/Users/shawn/workspace/code/apex/.github/workflows/build-release.yml` | Release artifact naming and packaged file contents that installer and upgrade logic must match |

### Technical Decisions

- Keep config resolution intentionally simple: flag, environment variable, then `~/.apex/config.json`.
- Add `-c` as the short form for the existing global `--config`.
- Do not discover `./config.json`; production stability is more important than surprising implicit behavior.
- Prefer native service managers for production (`systemd` on Linux, `launchd` on macOS) instead of expanding the built-in daemon mode.
- Keep `apex gateway start` compatible, but introduce `apex gateway run` as the preferred foreground command.
- Implement rollback-safe upgrades with versioned directories and a `current` symlink rather than overwriting a running binary. This avoids in-use binary replacement problems and gives clean rollback semantics.
- Use an install layout compatible with service execution:
  - `<install_dir>/releases/<version>/apex`
  - `<install_dir>/current -> releases/<version>`
  - `<install_dir>/config.json`
  - `<install_dir>/data`
  - `<install_dir>/logs`
  - `<install_dir>/install.json`
- Keep a compatibility binary/symlink at `<install_dir>/apex` or `/usr/local/bin/apex` only as a convenience entrypoint. Service definitions should execute `<install_dir>/current/apex gateway run`.
- `install.json` should be the source of truth for upgrade/service defaults: install dir, current version, repo, config path, service name, service manager, binary path, and created-at/updated-at timestamps.
- Service install should write `APEX_CONFIG=<config_path>` into the native service definition instead of embedding `--config` into `ExecStart`/ProgramArguments.
- Linux service manager:
  - systemd unit name defaults to `apex.service`
  - `ExecStart=<install_dir>/current/apex gateway run`
  - `Environment=APEX_CONFIG=<config_path>`
  - `WorkingDirectory=<install_dir>`
  - `Restart=always`
  - `ExecReload` can be omitted in v1 unless a real reload signal is implemented
- macOS service manager:
  - launchd label defaults to `dev.cregis.apex`
  - plist uses `ProgramArguments` with `<install_dir>/current/apex`, `gateway`, `run`
  - plist includes `EnvironmentVariables` with `APEX_CONFIG`
  - logs go to `<install_dir>/logs/stdout.log` and `<install_dir>/logs/stderr.log`
  - prefer user agents under `~/Library/LaunchAgents` for non-root installs and document root/system daemon mode separately if implemented
- `apex service logs` should call `journalctl -u apex -f` on Linux and tail the launchd stdout/stderr files on macOS.
- `apex upgrade --dry-run` must not mutate install directories or services.
- `apex upgrade --restart` should switch `current`, restart through the detected service manager, verify process/service status, and roll back `current` if restart/verification fails.
- Treat Windows service management and Windows self-upgrade as explicitly unsupported in v1.
- Do not add a large new dependency unless the implementation cannot remain maintainable with `std`, `serde`, and existing dependencies. Network download may reuse `reqwest`, which is already present.

## Implementation Plan

### Tasks

- [x] Task 1: Add shared config path resolution with source tracking
  - File: `/Users/shawn/workspace/code/apex/src/main.rs`
  - Action:
    1. Change `Cli.config` from `Option<String>` to a field with `#[arg(long, short = 'c', global = true)]`.
    2. Introduce a small internal `ResolvedConfigPath` struct with `path: PathBuf` and `source: ConfigPathSource`.
    3. Implement `resolve_config_path_with_source(cli_config: Option<&str>) -> ResolvedConfigPath` with exact precedence: CLI flag, `APEX_CONFIG`, `~/.apex/config.json`.
    4. Keep a compatibility helper `resolve_config_path(...) -> PathBuf` only if needed by existing handlers, but route all callers through the shared resolver.
    5. Ensure empty `APEX_CONFIG` is treated as unset rather than resolving to the log directory through `expand_path()`.
  - Notes: Do not add current-directory discovery. Do not change provider/router/team config semantics.

- [x] Task 2: Add `apex config path` and `apex config validate`
  - File: `/Users/shawn/workspace/code/apex/src/main.rs`
  - Action:
    1. Add `Commands::Config { command: ConfigCommand }`.
    2. Add `ConfigCommand::Path` and `ConfigCommand::Validate`.
    3. `config path` prints the resolved absolute or expanded path and the source (`flag`, `env`, or `default`).
    4. `config validate` uses `config::load_config(&path)` and prints a concise success or validation failure.
    5. Return non-zero on missing or invalid config.
  - Notes: `config validate` should use `config::load_config`, not raw `serde_json::from_str`, so compliance validation and legacy router migration behavior are exercised.

- [x] Task 3: Introduce `apex gateway run` while keeping `gateway start` compatible
  - File: `/Users/shawn/workspace/code/apex/src/main.rs`
  - Action:
    1. Add `GatewayCommand::Run`.
    2. Dispatch `Run` to the same `server::run_server(path).await` path used by `Start`.
    3. Preserve `GatewayCommand::Start { daemon }` exactly for compatibility.
    4. Ensure daemon detection in `main()` only applies to `gateway start --daemon`, not `gateway run`.
  - Notes: Docs should promote `gateway run`; existing scripts/tests using `gateway start` must keep working.

- [x] Task 4: Create install metadata model and persistence helpers
  - File: `/Users/shawn/workspace/code/apex/src/install_metadata.rs`
  - Action:
    1. Create a serde-backed `InstallMetadata` struct.
    2. Include fields: `install_dir`, `current_version`, `repo`, `config_path`, `service_name`, `service_manager`, `current_link`, `releases_dir`, `created_at`, `updated_at`.
    3. Implement `metadata_path(install_dir)`, `read_metadata(path_or_install_dir)`, and `write_metadata(install_dir, metadata)`.
    4. Add unit tests for round-trip serialization and missing metadata errors.
  - Notes: Use plain JSON at `<install_dir>/install.json`. Avoid embedding secrets.

- [x] Task 5: Create platform service module for systemd and launchd
  - File: `/Users/shawn/workspace/code/apex/src/service.rs`
  - Action:
    1. Add platform detection for Linux, macOS, and unsupported OS.
    2. Implement pure rendering functions for systemd unit content and launchd plist content.
    3. Implement service path resolution:
       - Linux root/system mode: `/etc/systemd/system/apex.service`
       - macOS user mode: `~/Library/LaunchAgents/dev.cregis.apex.plist`
    4. Implement command helpers for `install`, `uninstall`, `start`, `stop`, `restart`, `status`, and `logs`.
    5. Use `std::process::Command` for `systemctl`, `journalctl`, and `launchctl`.
  - Notes: Keep rendering functions side-effect-free and covered by tests. The first implementation can require the user to run with appropriate permissions for system-level Linux installs.

- [x] Task 6: Wire `apex service` CLI commands
  - File: `/Users/shawn/workspace/code/apex/src/main.rs`
  - Action:
    1. Add `Commands::Service { command: ServiceCommand }`.
    2. Add subcommands: `install`, `uninstall`, `start`, `stop`, `restart`, `status`, `logs`.
    3. Support service install flags: `--install-dir`, `--config`, `--name`, and macOS/Linux mode defaults.
    4. Resolve service defaults from `install.json` when available.
    5. For install, write native service definition with `APEX_CONFIG=<config_path>` and `gateway run`.
  - Notes: Keep existing top-level `status` and `logs` untouched; they remain daemon/pid-file commands.

- [x] Task 7: Update release installer to use versioned layout and metadata
  - File: `/Users/shawn/workspace/code/apex/install-release.sh`
  - Action:
    1. Add `--service` option.
    2. Install binary to `$TARGET_DIR/releases/<version>/apex` instead of only `$TARGET_DIR/apex`.
    3. Maintain `$TARGET_DIR/current` symlink to the selected release.
    4. Create or update `$TARGET_DIR/apex` as a convenience symlink to `$TARGET_DIR/current/apex`.
    5. Write `$TARGET_DIR/install.json` with repo, version, install dir, config path, service defaults, and release layout.
    6. If `--service` is supplied, invoke the installed binary's service install path or write service files consistently with the Rust service module contract.
    7. Preserve checksum verification behavior.
  - Notes: If `VERSION=latest`, resolve and record the actual installed version when feasible. If GitHub latest resolution is too large for this task, record `latest` plus document the limitation and make `apex upgrade` capable of correcting metadata on first upgrade.

- [x] Task 8: Align source installer output with the new operator model
  - File: `/Users/shawn/workspace/code/apex/install.sh`
  - Action:
    1. Update final usage instructions to prefer `APEX_CONFIG=<path> <binary> gateway run` or `apex service install`.
    2. Avoid recommending long `--config ... gateway start` as the primary path.
    3. Optionally create the same local layout/metadata if practical without destabilizing the source install flow.
  - Notes: Keep this lower risk than release installer changes; source install can remain simpler as long as messaging aligns.

- [x] Task 9: Implement rollback-safe upgrade module
  - File: `/Users/shawn/workspace/code/apex/src/upgrade.rs`
  - Action:
    1. Read install metadata to determine install dir, repo, current version, config path, service manager, and current symlink.
    2. Resolve target version from `--version` or latest release metadata.
    3. Determine platform artifact name using the same OS/arch mapping as `install-release.sh`.
    4. Download tarball and checksums with existing `reqwest`.
    5. Verify checksum unless explicitly skipped by a future flag; v1 should default to verification.
    6. Extract into `<install_dir>/releases/<target_version>/`.
    7. Validate the new binary with `<new>/apex --version` and `<new>/apex config validate -c <config_path>`.
    8. Switch `<install_dir>/current` symlink to the new release.
    9. If `--restart` is set, restart via service module and verify service status.
    10. If restart/status verification fails, switch `current` back to the previous release and restart the previous version.
  - Notes: Do not overwrite the running binary. `--dry-run` must perform no filesystem mutation beyond temporary download cache if unavoidable; prefer no mutation.

- [x] Task 10: Wire `apex upgrade` CLI command
  - File: `/Users/shawn/workspace/code/apex/src/main.rs`
  - Action:
    1. Add `Commands::Upgrade(UpgradeArgs)`.
    2. Add flags: `--version`, `--restart`, `--dry-run`.
    3. Dispatch to `upgrade::run_upgrade(args)`.
    4. Emit clear operator output for current version, target version, artifact, install dir, service manager, and rollback result.
  - Notes: If metadata is missing, fail with a clear message telling users to reinstall with `install-release.sh` or provide future explicit flags; do not guess arbitrary install directories.

- [x] Task 11: Add CLI, service rendering, metadata, and upgrade planning tests
  - File: `/Users/shawn/workspace/code/apex/tests/cli.rs`
  - Action:
    1. Add tests for `-c`, `APEX_CONFIG`, and flag-over-env precedence.
    2. Add tests for `config path` source reporting and `config validate` success/failure.
    3. Add a parser-level or subprocess test proving `gateway run --help` or command parsing is accepted without breaking `gateway start`.
  - File: `/Users/shawn/workspace/code/apex/src/service.rs`
  - Action:
    1. Add unit tests for systemd unit rendering.
    2. Add unit tests for launchd plist rendering.
  - File: `/Users/shawn/workspace/code/apex/src/install_metadata.rs`
  - Action:
    1. Add metadata round-trip tests.
  - File: `/Users/shawn/workspace/code/apex/src/upgrade.rs`
  - Action:
    1. Add tests for artifact naming, release path calculation, dry-run plan, and rollback plan.

- [x] Task 12: Update operator and release documentation
  - File: `/Users/shawn/workspace/code/apex/docs/current/guides/operations.md`
  - Action:
    1. Document config resolution order.
    2. Add examples for `apex config path`, `apex config validate`, `apex gateway run`, `apex service ...`, and `apex upgrade --restart`.
    3. Replace primary long `--config` examples with simpler defaults and `APEX_CONFIG` examples.
  - File: `/Users/shawn/workspace/code/apex/docs/current/guides/deployment.md`
  - Action:
    1. Replace manual systemd setup as the primary path with `install-release.sh --service`.
    2. Document Linux systemd and macOS launchd behavior.
    3. Replace manual binary replacement upgrade instructions with `apex upgrade --restart`.
  - File: `/Users/shawn/workspace/code/apex/RELEASE.md`
  - Action:
    1. Document versioned install layout expectations.
    2. Add release verification for `install-release.sh --service` and upgrade dry-run.
  - Notes: Some older docs may still mention `gateway start --config`; update the high-traffic docs first and leave broader architecture docs for a follow-up if needed.

- [x] Task 13: Run required validation
  - File: repository root
  - Action:
    1. Run `cargo test`.
    2. Run `bash -n install-release.sh`.
    3. Run `bash -n install.sh`.
    4. Run `./scripts/test-local-e2e.sh` before merge-ready delivery if local listener permissions allow it.
  - Notes: If local environment blocks listener binding, record the exact failure and the subset of validation completed.

### Acceptance Criteria

- [ ] AC 1: Given both `--config/-c` and `APEX_CONFIG` are absent, when any config-aware command resolves configuration, then it uses `~/.apex/config.json` and does not inspect `./config.json`.
- [ ] AC 2: Given `APEX_CONFIG` is set and no `--config/-c` is supplied, when `apex config path` is run, then it reports the env-provided path and source `env`.
- [ ] AC 3: Given `APEX_CONFIG` is set and `-c /tmp/apex.json` is supplied, when `apex config path` is run, then it reports `/tmp/apex.json` and source `flag`.
- [ ] AC 4: Given a valid config file exists, when `apex config validate -c <path>` is run, then the command exits successfully and reports that the config is valid.
- [ ] AC 5: Given a missing or invalid config file, when `apex config validate -c <path>` is run, then the command exits non-zero and prints a clear validation error.
- [ ] AC 6: Given an existing workflow uses `apex gateway start`, when the command is run with a valid config, then it remains accepted and starts the gateway through the existing path.
- [ ] AC 7: Given an operator runs `apex gateway run`, when the command is run with a valid config, then it starts the gateway through the same foreground server path as `gateway start`.
- [ ] AC 8: Given Linux service install inputs, when the systemd unit is rendered, then it uses `<install_dir>/current/apex gateway run`, sets `APEX_CONFIG=<config_path>`, sets `WorkingDirectory=<install_dir>`, and includes restart behavior.
- [ ] AC 9: Given macOS service install inputs, when the launchd plist is rendered, then it uses ProgramArguments for `<install_dir>/current/apex gateway run`, includes `APEX_CONFIG`, and writes stdout/stderr paths under `<install_dir>/logs`.
- [ ] AC 10: Given `install-release.sh --service --config-path <path> <install_dir>` succeeds, when installation completes, then `<install_dir>/releases/<version>/apex`, `<install_dir>/current`, `<install_dir>/apex`, and `<install_dir>/install.json` exist.
- [ ] AC 11: Given a service has been installed, when `apex service start|stop|restart|status` is run, then Apex delegates to `systemctl` on Linux or `launchctl` on macOS using metadata-derived defaults.
- [ ] AC 12: Given install metadata exists, when `apex upgrade --dry-run` is run, then it reports the current version, target version, artifact, install dir, and service manager without changing `current` or writing a new release directory.
- [ ] AC 13: Given install metadata exists and a target release is available, when `apex upgrade --version <tag>` is run without `--restart`, then Apex downloads, verifies, extracts, validates, switches `current`, updates metadata, and does not restart the service.
- [ ] AC 14: Given `apex upgrade --restart` switches to a new release and service restart verification fails, when rollback executes, then `current` points back to the previous release and Apex attempts to restart the previous version.
- [ ] AC 15: Given the platform is not Linux or macOS, when `apex service ...` or `apex upgrade --restart` requires service control, then the command fails with an explicit unsupported-platform message.
- [ ] AC 16: Given docs are updated, when an operator follows the primary install/service/upgrade path, then they do not need to hand-write a systemd unit or pass long `--config` arguments for normal use.

## Additional Context

### Dependencies

- No new dependency is required for config path resolution, service file generation, install metadata, or platform command execution.
- `reqwest` already exists and can be reused for release download in `apex upgrade`.
- `serde_json` already exists and should be used for `install.json`.
- Native platform commands are expected for service operations:
  - Linux: `systemctl`, `journalctl`
  - macOS: `launchctl`
- Installer scripts require existing shell dependencies plus optional service command availability:
  - existing: `curl` or `wget`, `tar`, `mktemp`, checksum command
  - service mode: `systemctl` on Linux or `launchctl` on macOS

### Testing Strategy

- Run `cargo test` at minimum.
- Add/extend `tests/cli.rs` for:
  - `--config` still wins
  - `-c` works as short config flag
  - `APEX_CONFIG` is used when no flag is passed
  - `--config` overrides `APEX_CONFIG`
  - missing default config still errors clearly
  - `apex config path` reports both path and source
  - `apex config validate` succeeds/fails against temp config files
  - `apex gateway run` is accepted by clap and aliases the same startup path as `gateway start`
- Add unit tests in new service module for pure rendering:
  - Linux systemd unit contains `ExecStart=<install_dir>/current/apex gateway run` and `Environment=APEX_CONFIG=...`
  - macOS launchd plist contains correct label, program arguments, environment, and log paths
- Add unit tests in new install metadata module for read/write round trip and missing metadata behavior.
- Add unit tests in new upgrade module for versioned path calculation, `current` symlink target selection, and rollback plan construction without performing network or service operations.
- Validate scripts with:
  - `bash -n install-release.sh`
  - `bash -n install.sh`
- Before merge-ready delivery, also run `./scripts/test-local-e2e.sh` per repo guidance if local listener permissions allow it.

### Notes

- The existing working tree has unrelated modifications. Implementation must preserve user changes and avoid broad rewrites.
- This spec is intentionally separate from the already completed CLI automation work for `channel`, `router`, and `team`.
- The largest implementation risk is upgrade safety. The implementation must not directly overwrite an in-use binary; use versioned release directories and symlink switching.
- The second largest risk is accidentally changing existing `gateway start --daemon`, `apex status`, or `apex logs` behavior. Keep new native service behavior under `apex service`.
- There is no unauthenticated health route today. V1 rollback verification should use native service status unless a small `/health` endpoint is intentionally added and covered by tests.
- macOS launchd has user-agent and system-daemon modes with different paths and privilege expectations. V1 should pick one default clearly and document it; user LaunchAgents are the least surprising for non-root installs.
- Future work may add multi-instance service management, Windows service support, signed update metadata, and authenticated health checks.
