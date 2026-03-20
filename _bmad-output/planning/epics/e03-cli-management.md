# Epic: CLI Management (E03)

## Description
提供完善的命令行工具（CLI），用于管理网关配置、启动服务以及查看运行状态。

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
