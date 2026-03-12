# Apex Gateway - Backend Development Guide

**Generated:** 2026-03-10
**Scope:** Rust 后端开发环境设置和工作流

## 环境要求

### 必需工具

| 工具 | 版本 | 说明 |
|------|------|------|
| Rust | 1.75+ (edition 2024) | 编程语言 |
| Cargo | - | Rust 包管理器 |
| SQLite | 3.40+ | 嵌入式数据库 |
| Git | - | 版本控制 |

### 可选工具

| 工具 | 说明 |
|------|------|
| rust-analyzer | VS Code Rust 插件 |
| cargo-watch | 文件变更自动编译 |
| cargo-edit | Cargo.toml 编辑工具 |
| clippy | Rust Linter |
| rustfmt | Rust 格式化工具 |

---

## 快速开始

### 1. 克隆仓库

```bash
git clone https://github.com/cregis-dev/apex.git
cd apex
```

### 2. 构建项目

```bash
# Debug 构建
cargo build

# Release 构建
cargo build --release
```

### 3. 初始化配置

```bash
# 创建默认配置
cargo run -- init

# 配置文件位置：~/.apex/config.json
```

### 4. 启动服务

```bash
# 使用配置文件启动
cargo run -- --config config.json

# 调试模式 (详细日志)
RUST_LOG=debug cargo run -- --config config.json

# 守护进程模式
cargo run -- gateway start -d
```

### 5. 验证服务

```bash
# 发送测试请求
curl http://localhost:12356/v1/chat/completions \
  -H "Authorization: Bearer sk-ap-xxx" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# 查看 Prometheus 指标
curl http://localhost:12356/metrics

# 查看 Usage API
curl http://localhost:12356/api/usage
```

---

## 开发工作流

### 实时重载开发

```bash
# 安装 cargo-watch
cargo install cargo-watch

# 实时编译和运行
cargo watch -x run -- --config config.json
```

### 运行测试

```bash
# 运行所有测试
cargo test

# 运行特定测试
cargo test mcp

# 运行集成测试
cargo test --test gateway

# 显示输出
cargo test -- --nocapture
```

### 代码质量检查

```bash
# 格式化代码
cargo fmt

# 检查格式
cargo fmt -- --check

# 运行 Linter
cargo clippy

# 严格模式
cargo clippy -- -D warnings
```

---

## 项目结构

```
apex/
├── Cargo.toml              # 依赖配置
├── src/
│   ├── main.rs             # CLI 入口
│   ├── lib.rs              # 库入口
│   ├── server.rs           # HTTP 服务器
│   ├── config.rs           # 配置解析
│   ├── providers.rs        # LLM 提供商客户端
│   ├── router_selector.rs  # 路由选择
│   ├── database.rs         # 数据库操作
│   ├── usage.rs            # Usage 记录
│   ├── metrics.rs          # Prometheus 指标
│   ├── converters.rs       # 协议转换
│   ├── compliance.rs       # 合规性检查
│   ├── logs.rs             # 日志配置
│   ├── utils.rs            # 工具函数
│   ├── mcp/                # MCP 模块
│   │   ├── mod.rs
│   │   ├── server.rs
│   │   ├── protocol.rs
│   │   ├── session.rs
│   │   ├── transport.rs
│   │   └── capabilities.rs
│   └── middleware/         # 中间件
│       ├── mod.rs
│       ├── auth.rs
│       ├── ratelimit.rs
│       └── policy.rs
└── tests/                  # 集成测试
```

---

## 模块开发指南

### 添加新的 LLM Provider

1. **在 `src/config.rs` 添加 Provider 类型:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    OpenAI,
    Anthropic,
    // 添加新的 Provider
    NewProvider,
}
```

2. **在 `src/providers.rs` 实现 Provider 逻辑:**

```rust
impl ProviderType {
    pub fn default_base_url(&self) -> &str {
        match self {
            ProviderType::OpenAI => "https://api.openai.com",
            ProviderType::NewProvider => "https://api.newprovider.com",
            // ...
        }
    }
}
```

3. **在 `src/converters.rs` 添加协议转换 (如需要)**

### 添加新的 Middleware

1. **创建中间件文件 `src/middleware/new_middleware.rs`:**

```rust
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};

pub async fn new_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // 中间件逻辑
    Ok(next.run(request).await)
}
```

2. **在 `src/server.rs` 注册中间件:**

```rust
app = app.layer(new_middleware);
```

### 添加新的 API 端点

1. **在 `src/server.rs` 添加路由:**

```rust
let app = Router::new()
    .route("/api/new-endpoint", get(new_endpoint_handler))
    // ...
```

2. **实现 Handler 函数:**

```rust
async fn new_endpoint_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RequestBody>,
) -> Result<Json<ResponseBody>, ApiError> {
    // Handler 逻辑
    Ok(Json(response))
}
```

---

## 调试技巧

### 日志调试

```bash
# 设置日志级别
RUST_LOG=apex=debug,tower_http=debug cargo run

# 只看特定模块
RUST_LOG=apex::mcp=debug cargo run
```

### 使用 cargo-watch 自动重载

```bash
# 监听 src 目录变化
cargo watch -w src -x run -- --config config.json
```

### 数据库调试

```bash
# 查看 SQLite 数据
sqlite3 ~/.apex/data/apex.db

# 查询 Usage 记录
sqlite3 ~/.apex/data/apex.db "SELECT * FROM usage_records LIMIT 10;"

# 查看表结构
sqlite3 ~/.apex/data/apex.db ".schema"
```

### 使用 curl 测试

```bash
# 测试 OpenAI 兼容接口
curl http://localhost:12356/v1/chat/completions \
  -H "Authorization: Bearer sk-ap-xxx" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"Hello"}]}'

# 测试 Anthropic 兼容接口
curl http://localhost:12356/v1/messages \
  -H "Authorization: Bearer sk-ap-xxx" \
  -H "Content-Type: application/json" \
  -H "x-api-key: sk-ap-xxx" \
  -d '{"model":"claude-3","messages":[{"role":"user","content":"Hello"}],"max_tokens":100}'

# 测试 Usage API
curl http://localhost:12356/api/usage?limit=5

# 测试 Metrics API
curl http://localhost:12356/api/metrics
```

---

## 配置说明

### 开发环境配置

```json
{
  "version": "1.0",
  "global": {
    "listen": "0.0.0.0:12356",
    "auth": {
      "mode": "api_key",
      "keys": ["sk-dev-key"]
    },
    "timeouts": {
      "connect_ms": 2000,
      "request_ms": 30000,
      "response_ms": 30000
    },
    "retries": {
      "max_attempts": 2,
      "backoff_ms": 200
    },
    "enable_mcp": true
  },
  "logging": {
    "level": "debug",
    "dir": "~/.apex/logs"
  },
  "data_dir": "~/.apex/data",
  "web_dir": "target/web",
  "channels": [
    {
      "name": "openai-dev",
      "provider": "openai",
      "base_url": "https://api.openai.com",
      "api_key": "sk-your-dev-key"
    }
  ],
  "routers": [
    {
      "name": "default-router",
      "rules": [
        {
          "match": { "model": "*" },
          "strategy": "round_robin",
          "channels": [{ "name": "openai-dev" }]
        }
      ]
    }
  ],
  "teams": [
    {
      "id": "dev-team",
      "api_key": "sk-ap-dev-key",
      "policy": {
        "allowed_routers": ["default-router"]
      }
    }
  ],
  "metrics": {
    "enabled": true,
    "path": "/metrics"
  },
  "hot_reload": {
    "config_path": "config.json",
    "watch": true
  }
}
```

---

## 常见问题

### Q: 如何处理 "config file not found" 错误？

A: 运行 `cargo run -- init` 创建默认配置。

### Q: 如何更改服务端口？

A: 修改配置文件中的 `global.listen` 字段，如 `"0.0.0.0:8080"`。

### Q: 如何启用详细日志？

A: 设置环境变量 `RUST_LOG=debug`。

### Q: 数据库文件在哪里？

A: 默认在 `~/.apex/data/apex.db`，可通过 `data_dir` 配置更改。

### Q: 如何重置数据库？

A: 删除数据库文件后重启服务：
```bash
rm ~/.apex/data/apex.db
cargo run -- --config config.json
```

---

## 性能分析

### 使用 cargo-flamegraph

```bash
# 安装
cargo install flamegraph

# 生成火焰图
cargo flamegraph --bin apex -- --config config.json
```

### 使用 tokio-console

```bash
# 安装
cargo install tokio-console
cargo add tokio-console

# 启用 console
RUST_LOG=tokio=trace cargo run

# 查看
tokio-console
```

---

## 发布流程

### 1. 更新版本号

编辑 `Cargo.toml`:
```toml
[package]
version = "0.2.0"
```

### 2. 运行完整测试

```bash
cargo test --all
cargo clippy -- -D warnings
cargo fmt -- --check
```

### 3. 构建 Release

```bash
cargo build --release
```

### 4. 生成多平台构建

```bash
# 使用 cross 进行交叉编译
cross build --target x86_64-unknown-linux-musl --release
cross build --target aarch64-apple-darwin --release
```

---

_Generated using BMAD Method `document-project` workflow_
