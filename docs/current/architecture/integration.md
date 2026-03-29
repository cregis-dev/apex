# Apex Gateway - Integration Architecture

**Generated:** 2026-03-10
**Scope:** Backend 与 Web Dashboard 的集成架构

---

## 整体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                          Client Layer                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │  OpenAI SDK  │  │Anthropic SDK │  │   Web Browser        │  │
│  │  (HTTP)      │  │  (HTTP)      │  │   (Dashboard UI)     │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ HTTP/HTTPS (Port 12356)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      Apex Gateway (Rust)                         │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                    Axum Router                             │  │
│  │  /v1/*        → LLM Proxy Handlers                        │  │
│  │  /api/*       → Observability API Handlers                │  │
│  │  /dashboard/* → Static Asset Layer                        │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   Middleware Chain                         │  │
│  │   Auth → RateLimit → Policy → Metrics → Logger            │  │
│  └───────────────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                   SQLite Database                          │  │
│  │   - usage_records                                         │  │
│  │   - metrics_requests                                      │  │
│  │   - metrics_errors                                        │  │
│  │   - metrics_fallbacks                                     │  │
│  │   - metrics_latency                                       │  │
│  └───────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ File System / Embedded Assets
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Static Files (target/web)                     │
│  - index.html                                                   │
│  - dashboard/index.html                                         │
│  - _next/static/* (JS, CSS)                                     │
│  - assets/*                                                     │
└─────────────────────────────────────────────────────────────────┘
```

---

## 集成点

### 1. 静态文件服务

**Backend 职责**: 通过统一静态资源访问层提供 Dashboard 静态资源

**配置**:
```json
{
  "web_dir": "target/web"
}
```

说明：

- 文件系统模式下从 `web_dir` 指向的 `target/web` 目录读取资源
- 启用 `embedded-web` 时从二进制内嵌资源读取
- 发布态推荐使用 `embedded-web`

**前端构建输出**:
```bash
web/
└── target/web/
    ├── index.html
    ├── dashboard/
    │   └── index.html
    ├── _next/
    │   └── static/
    │       ├── css/
    │       └── chunks/
    └── assets/
```

---

### 2. API 通信

**前端调用后端 API**:

```typescript
// hooks/use-api.ts
export function useApi() {
  const { apiKey } = useAuth();

  const fetchApi = async <T>(endpoint: string): Promise<T> => {
    const response = await fetch(`/api${endpoint}`, {
      headers: {
        'Authorization': `Bearer ${apiKey}`,
      },
    });
    return response.json();
  };

  return { fetchApi };
}
```

**后端 API 端点** (`src/server.rs`):
```rust
let api_routes = Router::new()
    .route("/usage", get(usage_api_handler))
    .route("/metrics", get(metrics_api_handler))
    .route("/metrics/trends", get(trends_api_handler))
    .route("/metrics/rankings", get(rankings_api_handler));
```

---

### 3. 认证集成

**前端存储 API Key**:
```typescript
// localStorage 持久化
const saveApiKey = (key: string) => {
  localStorage.setItem('apex-api-key', key);
};

const getApiKey = (): string => {
  return localStorage.getItem('apex-api-key') || '';
};
```

**后端验证 API Key** (`src/middleware/auth.rs`):
```rust
pub async fn auth_middleware(
    request: Request,
    next: Next,
    state: Arc<AppState>,
) -> Result<Response, StatusCode> {
    let api_key = extract_api_key(&request)?;

    // 验证 Team API Key
    let team = state.config.teams.iter()
        .find(|t| t.api_key == api_key)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // 将 Team 信息注入请求上下文
    request.extensions_mut().insert(team.clone());

    Ok(next.run(request).await)
}
```

---

### 4. 数据流

### Usage 数据流

```
1. Client → POST /v1/chat/completions
   ↓
2. [Auth Middleware] → 验证 API Key, 提取 Team 信息
   ↓
3. [Router Selector] → 匹配路由规则
   ↓
4. [Provider Adapter] → 调用上游 LLM API
   ↓
5. [Usage Logger] → 记录到 SQLite
   sql: INSERT INTO usage_records (...)
   ↓
6. Response → Client

---

7. Browser → GET /dashboard
   ↓
8. [Static File Server] → 返回 dashboard/index.html
   ↓
9. Browser → GET /api/usage
   ↓
10. [Usage API Handler] → 查询 SQLite
    sql: SELECT * FROM usage_records ...
    ↓
11. Response → Browser (Dashboard UI 展示)
```

---

### 5. 构建流程

```
┌─────────────────────────────────────────────────────────────┐
│ Step 1: Build Frontend                                      │
├─────────────────────────────────────────────────────────────┤
│ $ cd web && npm run build                                   │
│ → Output: target/web/                                       │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ Step 2: Build Backend                                       │
├─────────────────────────────────────────────────────────────┤
│ $ cargo build --release                                     │
│ → Output: target/release/apex                               │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ Step 3: Deploy                                              │
├─────────────────────────────────────────────────────────────┤
│ $ ./install.sh /opt/apex                                    │
│ → Copy: apex binary + web/ to /opt/apex                     │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ Step 4: Run                                                 │
├─────────────────────────────────────────────────────────────┤
│ $ /opt/apex/apex gateway start --config /opt/apex/config.json│
│ → Serves: API + Dashboard from same port (12356)            │
└─────────────────────────────────────────────────────────────┘
```

---

## 部署架构

### 单机部署

```
┌─────────────────────────────────────┐
│         Single Server               │
│  ┌───────────────────────────────┐  │
│  │   Apex Gateway (Rust)         │  │
│  │   Port: 12356                 │  │
│  │                               │  │
│  │   - API Endpoints             │  │
│  │   - Static Files (Dashboard)  │  │
│  │   - SQLite Database           │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

### Docker 部署

```yaml
# docker-compose.yml
version: '3.8'
services:
  apex:
    image: apex-gateway:latest
    ports:
      - "12356:12356"
    volumes:
      - ./config.json:/app/config.json
      - apex-data:/root/.apex/data
    restart: unless-stopped

volumes:
  apex-data:
```

### Sidecar 部署 (Kubernetes)

```yaml
# k8s/apex-sidecar.yaml
apiVersion: v1
kind: Pod
metadata:
  name: my-app
spec:
  containers:
  - name: app
    image: my-app:latest
  - name: apex
    image: apex-gateway:latest
    ports:
    - containerPort: 12356
    volumeMounts:
    - name: config
      mountPath: /app/config.json
      subPath: config.json
  volumes:
  - name: config
    configMap:
      name: apex-config
```

---

## 通信协议

### HTTP API

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | OpenAI 兼容接口 |
| `/v1/messages` | POST | Anthropic 兼容接口 |
| `/api/usage` | GET | Usage 记录查询 |
| `/api/metrics` | GET | Metrics 汇总 |
| `/dashboard/*` | GET | 静态文件 |

### 控制与配置自动化

远程 HTTP MCP 控制面已经退役。当前配置自动化以本地 CLI 为主，后续远程管理能力由 Admin Control Plane 承接。

---

## 配置同步

### 热重载机制

**配置文件监控** (`src/config.rs`):
```rust
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

pub fn start_hot_reload(config_path: &str, tx: Sender<Config>) {
    let mut watcher = RecommendedWatcher::new(move |event| {
        if let Ok(e) = event {
            if e.kind.is_modify() {
                // 重新加载配置
                let new_config = load_config(config_path).unwrap();
                tx.send(new_config).ok();
            }
        }
    })
    .unwrap();

    watcher.watch(Path::new(config_path), RecursiveMode::NonRecursive).unwrap();
}
```

## 安全模型

### 认证层级

```
┌─────────────────────────────────────────────────────────────┐
│ Layer 1: Global Auth                                        │
│ - Config: global.auth.keys                                  │
│ - Purpose: 保护整个网关                                     │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ Layer 2: Team Auth                                          │
│ - Config: teams[].api_key                                   │
│ - Purpose: 多租户隔离                                       │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ Layer 3: Policy Enforcement                                 │
│ - Config: teams[].policy                                    │
│ - Purpose: 限制允许的路由和模型                             │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ Layer 4: Rate Limiting                                      │
│ - Config: teams[].policy.rate_limit                         │
│ - Purpose: RPM/TPM 限制                                     │
└─────────────────────────────────────────────────────────────┘
```

### CORS 配置

```json
{
  "global": {
    "cors_allowed_origins": [
      "http://localhost:3000",
      "https://dashboard.example.com"
    ]
  }
}
```

**后端实现** (`src/server.rs`):
```rust
use tower_http::cors::{CorsLayer, Any};
use http::header;

let cors = CorsLayer::new()
    .allow_origin(allowed_origins.iter().map(|o| o.parse::<HeaderValue>().unwrap()).collect::<Vec<_>>())
    .allow_methods(Any)
    .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

let app = app.layer(cors);
```

---

## 日志和监控

### 日志流

```
Apex Gateway
    │
    ├─> stdout (开发模式)
    │
    ├─> ~/.apex/logs/apex.log (守护进程模式)
    │
    └─> SQLite (Usage/Metrics 事件)
        ↓
        Web Dashboard → /api/usage → 展示
```

### 监控指标

```
Apex Gateway
    │
    ├─> /metrics (Prometheus 格式)
    │   - apex_requests_total
    │   - apex_errors_total
    │   - apex_fallbacks_total
    │   - apex_upstream_latency_ms
    │
    └─> SQLite (聚合数据)
        ↓
        Web Dashboard → /api/metrics → 展示
```

---

## 故障排查

### 常见问题

**Q: Dashboard 无法加载静态资源？**

A: 先确认发布二进制是否使用 `embedded-web` 构建；仅文件系统模式下才需要检查 `web_dir` 和 `target/web`。

**Q: API 请求返回 401？**

A: 确保前端 localStorage 中存储了有效的 API Key。

**Q: CORS 错误？**

A: 在配置文件中添加 `cors_allowed_origins`。

---

_Generated using BMAD Method `document-project` workflow_
