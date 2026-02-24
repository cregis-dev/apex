use crate::config::{AuthMode, Config};
use crate::converters::convert_openai_response_to_anthropic;
use crate::metrics::MetricsState;
use crate::providers::{
    AccessAudit, NoOpAccessAudit, NoOpRateLimiter, ProviderRegistry, RateLimiter, RouteKind,
    prepare_request,
};
use crate::router_selector::RouterSelector;
use crate::usage::UsageLogger;
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::{self, TraceLayer};
use tracing::Level;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
    pub metrics: Arc<MetricsState>,
    pub providers: Arc<ProviderRegistry>,
    pub access_audit: Arc<dyn AccessAudit>,
    pub rate_limiter: Arc<dyn RateLimiter>,
    pub selector: Arc<RouterSelector>,
    pub client: reqwest::Client,
    pub usage_logger: Arc<UsageLogger>,
}

pub async fn run_server(path: PathBuf) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(&path)?;
    let mut config: Config = serde_json::from_str(&content)?;

    // Store config path for potential hot reload
    config.hot_reload.config_path = path.to_string_lossy().to_string();

    let state = build_state(config.clone())?;
    let app = build_app(state);

    let addr: SocketAddr = config.global.listen.parse()?;
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

pub fn build_state(config: Config) -> Result<Arc<AppState>, anyhow::Error> {
    let mut builder = reqwest::Client::builder();
    if config.global.timeouts.connect_ms > 0 {
        builder = builder.connect_timeout(Duration::from_millis(config.global.timeouts.connect_ms));
    }
    // Default pool settings
    builder = builder
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_nodelay(true);

    let client = builder.build()?;

    let usage_logger = Arc::new(UsageLogger::new(config.logging.dir.clone())?);

    Ok(Arc::new(AppState {
        config: Arc::new(RwLock::new(config)),
        metrics: Arc::new(MetricsState::new()?),
        providers: Arc::new(ProviderRegistry::new()),
        access_audit: Arc::new(NoOpAccessAudit),
        rate_limiter: Arc::new(NoOpRateLimiter),
        selector: Arc::new(RouterSelector::new()),
        client,
        usage_logger,
    }))
}

pub fn build_app(state: Arc<AppState>) -> Router {
    Router::new()
        // Standard OpenAI/Anthropic routes
        .route("/v1/chat/completions", post(handle_openai))
        .route("/v1/completions", post(handle_openai))
        .route("/v1/embeddings", post(handle_openai))
        .route("/v1/models", get(handle_models))
        .route("/v1/messages", post(handle_anthropic))
        // Compatibility routes (no /v1 prefix) for clients that omit it
        .route("/chat/completions", post(handle_openai))
        .route("/completions", post(handle_openai))
        .route("/embeddings", post(handle_openai))
        .route("/models", get(handle_models))
        .route("/messages", post(handle_anthropic))
        .route("/metrics", get(metrics_handler))
        .layer(
            tower::ServiceBuilder::new()
                .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
                .layer(PropagateRequestIdLayer::x_request_id())
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(|request: &Request<Body>| {
                            let request_id = request
                                .extensions()
                                .get::<tower_http::request_id::RequestId>()
                                .map(|id| id.header_value().to_str().unwrap_or("unknown"))
                                .unwrap_or("unknown");
                            let client_ip = request
                                .headers()
                                .get("x-forwarded-for")
                                .and_then(|h| h.to_str().ok())
                                .unwrap_or("unknown");

                            tracing::info_span!("request",
                                request_id = %request_id,
                                client_ip = %client_ip,
                                method = %request.method(),
                                uri = %request.uri(),
                                version = ?request.version()
                            )
                        })
                        .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
                ),
        )
        .with_state(state)
}

async fn metrics_handler(state: State<Arc<AppState>>) -> Response<Body> {
    match state.metrics.render() {
        Ok(body) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain; version=0.0.4")
            .body(Body::from(body))
            .unwrap(),
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap(),
    }
}

async fn handle_openai(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response<Body> {
    process_request(state, req, RouteKind::Openai, None, None).await
}

async fn handle_anthropic(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    process_request(state, req, RouteKind::Anthropic, None, None).await
}

async fn handle_models(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let headers = &parts.headers;

    let config = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config, headers) {
        return resp;
    }
    // Try Authorization header first, then x-api-key (for Anthropic)
    let vkey =
        read_auth_token(headers, "authorization").or_else(|| read_auth_token(headers, "x-api-key"));

    let Some(vkey) = vkey else {
        return error_response(StatusCode::UNAUTHORIZED, "missing vkey");
    };
    let Some(_router) = find_router_by_vkey(&config, &vkey) else {
        return error_response(StatusCode::UNAUTHORIZED, "invalid vkey");
    };

    // Placeholder response for models
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"object":"list","data":[]}"#))
        .unwrap()
}

// Helpers

#[allow(clippy::result_large_err)]
fn enforce_global_auth(config: &Config, headers: &HeaderMap) -> Result<(), Response<Body>> {
    match config.global.auth.mode {
        AuthMode::None => Ok(()),
        AuthMode::ApiKey => {
            let Some(keys) = &config.global.auth.keys else {
                return Ok(());
            };
            let token = read_auth_token(headers, "authorization")
                .or_else(|| read_auth_token(headers, "x-api-key"));

            if let Some(token) = token {
                if keys.contains(&token) {
                    return Ok(());
                }
            }
            Err(error_response(StatusCode::UNAUTHORIZED, "unauthorized"))
        }
    }
}

fn read_auth_token(headers: &HeaderMap, key: &str) -> Option<String> {
    if let Some(val) = headers.get(key)
        && let Ok(s) = val.to_str()
    {
        if key == "authorization" && s.starts_with("Bearer ") {
            return Some(s[7..].to_string());
        }
        return Some(s.to_string());
    }
    None
}

fn find_router_by_vkey(config: &Config, vkey: &str) -> Option<String> {
    for router in &config.routers {
        if let Some(ref k) = router.vkey
            && k == vkey
        {
            return Some(router.name.clone());
        }
    }
    None
}

fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(Body::from(message.to_string()))
        .unwrap()
}

async fn process_request(
    state: Arc<AppState>,
    req: Request<Body>,
    route: RouteKind,
    router_name_override: Option<String>,
    path_override: Option<String>,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let headers = parts.headers;
    let bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
    };

    let config = state.config.read().unwrap().clone();

    // 1. Global Auth
    if let Err(resp) = enforce_global_auth(&config, &headers) {
        return resp;
    }

    // 2. Resolve Router
    let router_name = if let Some(name) = router_name_override {
        name
    } else {
        // Try Authorization header first, then x-api-key (standard Anthropic)
        let vkey = read_auth_token(&headers, "authorization")
            .or_else(|| read_auth_token(&headers, "x-api-key"));

        if let Some(key) = vkey {
            if let Some(name) = find_router_by_vkey(&config, &key) {
                name
            } else {
                return error_response(StatusCode::UNAUTHORIZED, "invalid vkey");
            }
        } else {
            return error_response(StatusCode::UNAUTHORIZED, "missing vkey");
        }
    };

    let Some(router) = config.routers.iter().find(|r| r.name == router_name) else {
        return error_response(StatusCode::NOT_FOUND, "router not found");
    };

    tracing::info!("Router resolved: {}", router.name);

    // 3. Resolve Channels
    let mut channels = Vec::new();

    // Parse model from body to use in routing
    let model_name = if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
        json.get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    };
    let model_name_str = model_name.as_deref().unwrap_or("default");

    if let Some(ch_name) = state.selector.select_channel(router, model_name_str)
        && let Some(ch) = config.channels.iter().find(|c| c.name == ch_name)
    {
        channels.push(ch);
        tracing::info!(
            "Channel selected: {} (strategy={}, model={})",
            ch.name,
            router.strategy,
            model_name_str
        );
    }

    // Add fallbacks
    for fb_name in &router.fallback_channels {
        if let Some(ch) = config.channels.iter().find(|c| c.name == *fb_name) {
            // Avoid duplicates
            if !channels.iter().any(|c| c.name == ch.name) {
                channels.push(ch);
            }
        }
    }

    if channels.is_empty() {
        tracing::warn!(
            "No channels configured or matched for router: {}",
            router_name
        );
        return error_response(StatusCode::BAD_GATEWAY, "no channels configured or matched");
    }

    let route_label = match route {
        RouteKind::Openai => "openai",
        RouteKind::Anthropic => "anthropic",
    };
    state
        .metrics
        .request_total
        .with_label_values(&[route_label, &router_name])
        .inc();

    // 4. Loop channels
    let retry_on = &config.global.retries.retry_on_status;
    let max_attempts = config.global.retries.max_attempts.max(1);

    // Extract path and query for preparation
    let path = path_override.unwrap_or_else(|| parts.uri.path().to_string());
    let query = parts.uri.query().map(|s| s.to_string());

    for (index, channel) in channels.iter().enumerate() {
        if index > 0 {
            tracing::warn!("Switching to fallback channel: {}", channel.name);
            state
                .metrics
                .fallback_total
                .with_label_values(&[&router_name, &channel.name])
                .inc();
        }

        if !state.rate_limiter.check(&channel.provider_type) {
            tracing::warn!(
                "Rate limit exceeded for provider: {:?}",
                channel.provider_type
            );
            continue;
        }

        for attempt in 0..max_attempts {
            let prepared = match prepare_request(
                &state.providers,
                channel,
                route,
                &channel.base_url,
                &path,
                query.as_deref(),
                &headers,
                &bytes,
            ) {
                Ok(p) => p,
                Err(e) => return error_response(StatusCode::BAD_REQUEST, &e.to_string()),
            };

            let adapter = state.providers.adapter(channel);

            let start = std::time::Instant::now();

            let req_future = state
                .client
                .request(parts.method.clone(), prepared.url)
                .headers(prepared.headers)
                .body(prepared.body)
                .build();

            let req_built = match req_future {
                Ok(r) => r,
                Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
            };

            tracing::info!(
                "Upstream request: method={} url={} attempt={}/{}",
                req_built.method(),
                req_built.url(),
                attempt + 1,
                max_attempts
            );

            let resp_result = state.client.execute(req_built).await;

            match resp_result {
                Ok(resp) => {
                    let elapsed = start.elapsed().as_millis() as f64;

                    state
                        .metrics
                        .upstream_latency_ms
                        .with_label_values(&[route_label, &router_name, &channel.name])
                        .observe(elapsed);

                    let status = resp.status();
                    if status.is_success() {
                        tracing::info!("Upstream success: {} ({}ms)", status, elapsed);
                        state
                            .access_audit
                            .audit(&channel.provider_type, route, true);
                        let response = adapter.handle_response(
                            route,
                            resp,
                            Duration::from_millis(config.global.timeouts.response_ms),
                        );
                        return crate::usage::wrap_response(
                            response,
                            router_name.clone(),
                            channel.name.clone(),
                            model_name_str.to_string(),
                            state.usage_logger.clone(),
                            state.metrics.clone(),
                        )
                        .await;
                    }

                    tracing::warn!("Upstream failed: {} ({}ms)", status, elapsed);
                    state
                        .access_audit
                        .audit(&channel.provider_type, route, false);

                    // Check if retryable
                    if attempt + 1 < max_attempts {
                        // Check retry on status
                        let status_code = status.as_u16();
                        // Assuming retry_on is Vec<u16>
                        if retry_on.contains(&status_code) {
                            tracing::warn!(
                                "Retrying (attempt {}/{}) due to status {}",
                                attempt + 1,
                                max_attempts,
                                status_code
                            );
                            tokio::time::sleep(Duration::from_millis(
                                config.global.retries.backoff_ms,
                            ))
                            .await;
                            continue;
                        }
                    }

                    // If last channel and last attempt, return error
                    if index == channels.len() - 1 && attempt == max_attempts - 1 {
                        state
                            .metrics
                            .error_total
                            .with_label_values(&[route_label, &router_name])
                            .inc();
                        // Convert error if needed (e.g. for Anthropic)
                        if matches!(route, RouteKind::Anthropic) {
                            let bytes = resp.bytes().await.unwrap_or_default();
                            let body = convert_openai_response_to_anthropic(bytes);
                            return Response::builder()
                                .status(status)
                                .body(Body::from(body))
                                .unwrap();
                        }
                        return adapter.handle_response(
                            route,
                            resp,
                            Duration::from_millis(config.global.timeouts.response_ms),
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Upstream request error: {}", e);
                    state
                        .access_audit
                        .audit(&channel.provider_type, route, false);
                    if attempt + 1 < max_attempts {
                        tracing::warn!(
                            "Retrying (attempt {}/{}) due to error",
                            attempt + 1,
                            max_attempts
                        );
                        tokio::time::sleep(Duration::from_millis(config.global.retries.backoff_ms))
                            .await;
                        continue;
                    }
                }
            }
        }
    }

    state
        .metrics
        .error_total
        .with_label_values(&[route_label, &router_name])
        .inc();
    error_response(StatusCode::BAD_GATEWAY, "all channels failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Auth, Channel, Global, ProviderType, Retries, Timeouts};
    use crate::providers::{AccessAudit, RateLimiter, RouteKind};
    use std::sync::Mutex;

    struct MockAccessAudit {
        calls: Arc<Mutex<Vec<(ProviderType, bool)>>>,
    }

    impl AccessAudit for MockAccessAudit {
        fn audit(&self, provider: &ProviderType, _route: RouteKind, success: bool) {
            self.calls.lock().unwrap().push((provider.clone(), success));
        }
    }

    struct MockRateLimiter {
        allow: bool,
    }

    impl RateLimiter for MockRateLimiter {
        fn check(&self, _provider: &ProviderType) -> bool {
            self.allow
        }
    }

    fn create_test_config() -> Config {
        Config {
            version: "1.0".to_string(),
            global: Global {
                listen: "127.0.0.1:0".to_string(),
                auth: Auth {
                    mode: AuthMode::None,
                    keys: None,
                },
                retries: Retries {
                    max_attempts: 1,
                    backoff_ms: 0,
                    retry_on_status: vec![],
                },
                timeouts: Timeouts {
                    connect_ms: 100,
                    request_ms: 100,
                    response_ms: 100,
                },
            },
            metrics: crate::config::Metrics {
                enabled: false,
                listen: "127.0.0.1:0".to_string(),
                path: "/metrics".to_string(),
            },
            hot_reload: crate::config::HotReload {
                watch: false,
                config_path: "/tmp/config.json".to_string(),
            },
            logging: crate::config::Logging {
                level: "info".to_string(),
                dir: None,
            },
            channels: vec![Channel {
                name: "test-channel".to_string(),
                provider_type: ProviderType::Openai,
                base_url: "http://example.com".to_string(),
                api_key: "test-key".to_string(),
                anthropic_base_url: None,
                headers: None,
                model_map: None,
                timeouts: None,
            }],
            routers: vec![crate::config::Router {
                name: "test-router".to_string(),
                vkey: Some("test-vkey".to_string()),
                channels: vec![crate::config::TargetChannel {
                    name: "test-channel".to_string(),
                    weight: 1,
                }],
                strategy: "round_robin".to_string(),
                metadata: None,
                fallback_channels: vec![],
                rules: vec![crate::config::RouterRule {
                    match_spec: crate::config::MatchSpec {
                        models: vec!["*".to_string()],
                    },
                    channels: vec![crate::config::TargetChannel {
                        name: "test-channel".to_string(),
                        weight: 1,
                    }],
                    strategy: "round_robin".to_string(),
                }],
            }],
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks() {
        let config = create_test_config();
        let audit_calls = Arc::new(Mutex::new(Vec::new()));

        let state = Arc::new(AppState {
            config: Arc::new(RwLock::new(config)),
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: audit_calls.clone(),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: false }), // Block everything
            selector: Arc::new(RouterSelector::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(None).unwrap()),
        });

        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer test-vkey")
            .body(Body::from("{}"))
            .unwrap();

        let resp = handle_openai(State(state), req).await;

        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn test_access_audit_logged() {
        let config = create_test_config();
        let audit_calls = Arc::new(Mutex::new(Vec::new()));

        let state = Arc::new(AppState {
            config: Arc::new(RwLock::new(config)),
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: audit_calls.clone(),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            selector: Arc::new(RouterSelector::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(None).unwrap()),
        });

        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer test-vkey")
            .body(Body::from("{}"))
            .unwrap();

        let _ = handle_openai(State(state), req).await;

        let calls = audit_calls.lock().unwrap();
        assert!(!calls.is_empty());
        assert_eq!(calls[0].0, ProviderType::Openai);
        assert_eq!(calls[0].1, false); // Failed
    }

    #[test]
    fn test_read_auth_token() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "secret".parse().unwrap());
        assert_eq!(
            read_auth_token(&headers, "x-api-key"),
            Some("secret".to_string())
        );

        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer token".parse().unwrap());
        assert_eq!(
            read_auth_token(&headers, "authorization"),
            Some("token".to_string())
        );
    }

    #[tokio::test]
    async fn test_routing_logic() {
        let mut config = create_test_config();

        // Add another channel
        config.channels.push(Channel {
            name: "ch2".to_string(),
            provider_type: ProviderType::Anthropic, // Distinct provider
            base_url: "http://example.com".to_string(),
            api_key: "k2".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
            timeouts: None,
        });

        // Update router to match "gpt-4" to "ch2"
        let router = &mut config.routers[0];
        router.rules.insert(
            0,
            crate::config::RouterRule {
                match_spec: crate::config::MatchSpec {
                    models: vec!["gpt-4".to_string()],
                },
                channels: vec![crate::config::TargetChannel {
                    name: "ch2".to_string(),
                    weight: 1,
                }],
                strategy: "priority".to_string(),
            },
        );

        let audit_calls = Arc::new(Mutex::new(Vec::new()));

        let state = Arc::new(AppState {
            config: Arc::new(RwLock::new(config)),
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: audit_calls.clone(),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            selector: Arc::new(RouterSelector::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(None).unwrap()),
        });

        // Request with model "gpt-4" -> should go to ch2 (Anthropic)
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer test-vkey")
            .body(Body::from(r#"{"model": "gpt-4"}"#))
            .unwrap();

        let _ = handle_openai(State(state.clone()), req).await;

        let calls = audit_calls.lock().unwrap();
        assert!(!calls.is_empty(), "should have made a call");
        // The last call should be Anthropic because that's ch2's provider type
        assert_eq!(calls.last().unwrap().0, ProviderType::Anthropic);
    }
}
