# Epic: Observability (E04)

## Description
构建网关的可观测性体系，包括 Prometheus 监控指标导出和结构化日志记录，以便于运维排查和性能分析。

## Stories

- [x] **S01: Prometheus Metrics**
  - Implement `MetricsState` registry.
  - Export standard HTTP metrics (requests, latency, errors).
  - Expose `/metrics` endpoint.
  - Track `apex_fallback_total` and specific error codes.

- [x] **S02: Structured Logging**
  - Use `tracing` and `tracing-subscriber`.
  - Support log levels (INFO, DEBUG, ERROR).
  - Output logs to stdout/file (Daemon mode).
  - Include Request ID (UUID) in all logs.
