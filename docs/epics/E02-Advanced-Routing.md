# Epic: Advanced Routing (E02)

## Description
实现基于多 Channel 的高级路由策略，包括负载均衡、内容路由（基于模型名称）以及高性能的路由匹配缓存。

## Stories

- [x] **S01: Multi-Channel Configuration Support**
  - Update `Router` struct to support `channels` list (Weighted).
  - Support `metadata` field for model matching rules.
  - Backward compatibility for legacy `channel` field.

- [x] **S02: Router Rule Cache (LRU)**
  - Implement `moka` based LRU cache for router rules.
  - Cache key: `router_name:model_name`.
  - Invalidate cache on config reload.

- [x] **S03: Routing Strategy Implementation**
  - Implement `round_robin` (Weighted) strategy.
  - Implement `random` strategy.
  - Implement `priority` strategy.

- [x] **S04: Content-Based Routing (Model Matcher)**
  - Implement Exact Match logic for model names.
  - Implement Glob Pattern matching (e.g., `gpt-*`).
  - Integrate with Router Selector.
