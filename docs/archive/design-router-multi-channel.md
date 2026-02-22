# Apex Router 多 Channel 聚合与智能路由方案

## 1. 背景与目标

当前 Apex 的 Router 仅支持单一 Channel 作为主通道，Fallback Channel 仅用于故障转移。为了实现以下目标，需要升级 Router 的路由能力：

1.  **模型聚合 (Model Aggregation)**：下游客户端只需连接一个 Router，即可调用不同上游 Provider 的模型（如同时使用 OpenAI 和 Gemini）。
2.  **负载均衡 (Load Balancing)**：通过配置多个同类 Channel 分摊流量，突破单一 API Key 的 Rate Limit 限制。
3.  **精细化路由 (Content-Based Routing)**：根据请求的模型名称（Model Name）自动分发到指定的 Channel。

## 2. 核心设计：Router 结构升级

将 Router 的 `channel` 字段从单一字符串升级为支持多 Channel 配置的结构，引入 `channels` 列表和 `model_matcher` 路由规则。同时，为了保证高性能，引入 LRU 缓存机制来加速模型路由匹配。

### 2.1 配置结构变更

**旧结构：**
```rust
struct Router {
    name: String,
    vkey: Option<String>,
    channel: String,             // 单一主通道
    fallback_channels: Vec<String>, // 仅故障时使用
}
```

**新结构：**
```rust
struct Router {
    name: String,
    vkey: Option<String>,
    
    // 1. 目标通道列表（负载均衡池）
    // 定义该 Router 可用的所有 Channel 及其权重
    // 原 'clients' 改为 'channels'
    channels: Vec<TargetChannel>,
    
    // 2. 路由策略（默认分发逻辑）
    // 当没有命中具体模型规则时，如何从 channels 中选择 Channel
    // 支持: "round_robin" (轮询), "random" (随机), "priority" (按顺序)
    strategy: String, // default: "round_robin"

    // 3. 模型匹配规则（智能路由）
    // 根据请求体中的 model 字段，强制指定走哪个 Channel
    // 优先级高于默认 strategy
    metadata: Option<RouterMetadata>, 
}

struct TargetChannel {
    name: String,    // 引用 Channel 名称
    weight: u32,     // 权重 (默认为 1)，用于加权轮询
}

struct RouterMetadata {
    // 键为模型匹配模式 (glob pattern)，值为目标 channel 名称
    // 示例: "gpt-*": "openai-channel", "claude-*": "anthropic-channel"
    model_matcher: HashMap<String, String>,
}
```

### 2.2 配置示例 (JSON)

```json
{
  "routers": [
    {
      "name": "unified-api",
      "vkey": "vk_demo123",
      
      // 默认策略：轮询
      // 如果请求的模型未在 metadata 中匹配，则在 channels 中轮询分发
      "strategy": "round_robin",
      
      "channels": [
        { "name": "openai-primary", "weight": 3 },
        { "name": "openai-backup", "weight": 1 }
      ],
      
      "metadata": {
        "model_matcher": {
          // 精确匹配：Gemini 模型强制走 Google 通道
          "gemini-1.5-pro": "google-channel",
          
          // 通配符匹配：Claude 系列全部走 Anthropic 通道
          "claude-*": "anthropic-channel",
          
          // 特定模型走专用通道（如 Deepseek）
          "deepseek-coder": "deepseek-channel"
        }
      }
    }
  ]
}
```

### 2.3 性能优化：路由缓存机制

为了避免对每个请求都进行昂贵的正则/通配符匹配（O(N) 复杂度），我们引入 **Router Rule Cache**。

**机制设计：**

1.  **Cache Key**: `router_name` + `model_name` (e.g., `"unified-api:claude-3-opus"`)
2.  **Cache Value**: `Option<String>` (匹配到的 Channel 名称，或 None)
3.  **数据结构**: 使用 `moka` 或 `lru` crate 实现线程安全的 LRU Cache。
4.  **容量**: 默认 10,000 条记录，足够覆盖常见模型组合。
5.  **生命周期**:
    *   **Write**: 首次请求某个模型时，遍历 `model_matcher` 规则，计算匹配结果并写入缓存。
    *   **Read**: 后续请求直接查缓存 (O(1))，命中即返回。
    *   **Invalidate**: 当 Router 配置更新（Hot Reload）时，清空该 Router 相关的缓存。

```rust
// 伪代码示意
struct RouterState {
    // ... 其他字段
    rule_cache: Cache<String, Option<String>>, 
}

impl RouterState {
    fn find_target_channel(&self, router: &Router, model: &str) -> Option<String> {
        let cache_key = format!("{}:{}", router.name, model);
        
        // 1. 查缓存
        if let Some(cached) = self.rule_cache.get(&cache_key) {
            return cached;
        }

        // 2. 缓存未命中，执行匹配逻辑
        let target = self.match_rule(router, model);
        
        // 3. 写入缓存
        self.rule_cache.insert(cache_key, target.clone());
        
        target
    }
}
```

## 3. 请求处理流程

当一个请求到达 Router 时，Apex 将执行以下逻辑：

1.  **解析请求体**：
    *   读取 HTTP Body，提取 `model` 字段（如 `"model": "claude-3-5-sonnet"`）。
    *   注意：对于非 JSON Body 或无 model 字段的请求，跳过匹配步骤。

2.  **模型路由 (Model Routing with Cache)**：
    *   调用 `find_target_channel(router, model)`。
    *   优先查 LRU 缓存。
    *   若未命中，则遍历 `metadata.model_matcher`：
        *   **精确匹配**：查找是否存在 key == model 的规则。
        *   **通配符匹配**：查找是否存在 key 匹配 model 的规则（如 `claude-*`）。
    *   将结果（Target Channel 或 None）写入缓存。

3.  **分发决策 (Dispatch Decision)**：
    *   **命中规则**：如果 `find_target_channel` 返回了具体的 Channel，直接转发给该 Channel（忽略负载均衡策略）。
    *   **未命中 (Default)**：根据 `strategy` 从 `channels` 列表中选择一个 Channel。
        *   **Round Robin**：按权重轮询选择。
        *   **Random**：随机选择。
        *   **Priority**：始终尝试第一个健康的 Channel。

4.  **执行请求**：
    *   使用选定的 Channel 处理请求。
    *   如果请求失败（网络错误/5xx），且配置了全局 `fallback_channels`（保留现有字段作为最后兜底），则尝试 Fallback。

## 4. 兼容性设计

为了保证向后兼容，我们将支持两种配置格式：

1.  **Legacy Mode (当前模式)**：
    *   配置中仅包含 `channel: "xxx"` 字段。
    *   系统自动将其转换为 `channels: [{ name: "xxx", weight: 1 }]`，策略为 `priority`。

2.  **Advanced Mode (新模式)**：
    *   配置中包含 `channels` 列表。
    *   此时忽略旧的 `channel` 字段。

## 5. 开发计划

1.  **配置结构升级**：
    *   修改 `Config` 和 `Router` 结构体，添加 `channels` (Vec<TargetChannel>) 和 `metadata` 字段。
    *   实现配置加载时的兼容性转换逻辑。

2.  **引入缓存依赖**：
    *   添加 `moka` 或 `lru` 依赖到 `Cargo.toml`。
    *   在 `AppState` 中初始化全局路由缓存。

3.  **路由逻辑实现**：
    *   在 `server.rs` 中引入 `RouterSelector` 模块。
    *   实现带缓存的模型匹配逻辑。
    *   实现基于权重的轮询算法 (Weighted Round-Robin)。

4.  **CLI 支持**：
    *   更新 `apex router add/update` 命令，支持 `--add-channel "name=openai,weight=1"` 和 `--match "gpt-*=openai"` 参数。

