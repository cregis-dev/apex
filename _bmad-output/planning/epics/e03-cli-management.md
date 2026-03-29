# Epic: CLI Management (E03)

## Description
提供完善的命令行工具（CLI），用于管理网关配置、启动服务、查看运行状态，并作为 AI skills 与本地自动化的主要操作入口。

Reference: `../cli-ai-automation-contract.md`

## Stories

- [x] **S01: CLI Infrastructure**
  - Use `clap` to define command structure.
  - Support global `--config` flag.
  - Implement `apex init` wizard (default config generation).

- [x] **S02: Channel Management Commands**
  - `apex channel add`: Interactive/Flag-based channel creation.
  - `apex channel update`: Modify existing channels.
  - `apex channel delete`: Remove channels.
  - `apex channel list/show`: Display channel info (support JSON output).

- [x] **S03: Router Management Commands**
  - `apex router add`: Create routers with strategies and matchers.
  - `apex router update`: Modify router settings.
  - `apex router delete`: Remove routers.
  - `apex router list`: Display router info.

- [x] **S04: Gateway Control**
  - `apex gateway start`: Run server (foreground).
  - `apex gateway start --daemon`: Run in background (Unix/macOS).
  - `apex gateway stop`: Stop daemon process.

- [ ] **S05: AI-Friendly Non-Interactive CLI Inputs**
  - Scope:
    - Target v1 command set: `apex channel`, `apex router`, and `apex team`.
    - Target actions for v1 automation: `add`, `update`, `delete`, `list`, and `show`.
    - Ensure required inputs for the target v1 command set can be supplied fully through flags and arguments without interactive prompts.
    - Define behavior when both flags and interactive mode are possible, with flags taking precedence.
  - Acceptance criteria:
    - `apex channel`, `apex router`, and `apex team` can run end-to-end without TTY interaction when arguments are provided.
    - `add`, `update`, `delete`, `list`, and `show` workflows in the target v1 command set are explicitly supported or explicitly documented as unavailable.
    - Help text clearly documents required and optional parameters for automation use.
    - Interactive prompting remains optional rather than required for the target v1 command set.

- [ ] **S06: Machine-Readable JSON Output for Automation**
  - Scope:
    - Add JSON output support to the target v1 command set: `apex channel`, `apex router`, and `apex team`.
    - Standardize success and error payload structure for skill consumption.
    - Ensure human-readable output and JSON mode do not conflict.
    - Follow the v1 contract defined in `../cli-ai-automation-contract.md`.
  - Acceptance criteria:
    - `apex channel`, `apex router`, and `apex team` support a consistent JSON output mode.
    - JSON responses preserve the top-level fields `ok`, `command`, `message`, `data`, `errors`, and `meta`.
    - Error responses in JSON mode are machine-readable and stable enough for automation parsing.
    - Documentation includes examples of JSON-based invocation for skill workflows.
