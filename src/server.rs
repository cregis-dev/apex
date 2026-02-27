use crate::config::{AuthMode, Config};
use crate::converters::convert_openai_response_to_anthropic;
use crate::metrics::MetricsState;
use crate::middleware::auth::{TeamContext, team_auth};
use crate::middleware::policy::team_policy;
use crate::middleware::ratelimit::TeamRateLimiter;
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
    pub team_rate_limiter: Arc<TeamRateLimiter>,
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
        team_rate_limiter: Arc::new(TeamRateLimiter::new()),
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
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            team_policy,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            team_auth,
        ))
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

                            // Try to get team_id from extensions (if already set by auth middleware)
                            // Note: TraceLayer runs before auth middleware in the stack order defined below,
                            // but the span is created when the request arrives.
                            // The fields will be empty initially and populated later if we record them.
                            // However, since we want them in the span start, we might need to rely on
                            // the fact that we can't get team_id here yet.
                            // Instead, we can record it later in the handler or middleware.
                            // BUT, tracing::info_span! captures values at creation.
                            // Let's just include the fields we can get now.
                            // Actually, TraceLayer `make_span_with` is called when request arrives.
                            // Auth happens inside the service.
                            // So team_id won't be available here yet.
                            // We will add `team_id` field as Empty and populate it in a middleware wrapper or inside the handler.
                            // For now, let's just add the fields we have and make space for others.

                            tracing::info_span!("request",
                                request_id = %request_id,
                                client_ip = %client_ip,
                                team_id = tracing::field::Empty, // Will be populated by auth middleware
                                router_name = tracing::field::Empty,
                                channel_name = tracing::field::Empty,
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
    let _vkey =
        read_auth_token(headers, "authorization").or_else(|| read_auth_token(headers, "x-api-key"));

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

            let candidates = [
                read_auth_token(headers, "authorization"),
                read_auth_token(headers, "x-api-key"),
            ];

            for token in candidates.into_iter().flatten() {
                if keys.contains(&token) {
                    return Ok(());
                }
            }

            tracing::warn!(
                "Auth Failed: No valid token found in Authorization or x-api-key headers."
            );
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

fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": "invalid_request_error",
            "param": null,
            "code": null
        }
    });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
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

    // 1. Read Body
    let bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Request Failed: Failed to read body: {}", e);
            return error_response(StatusCode::BAD_REQUEST, &e.to_string());
        }
    };

    // 2. Parse Model
    let model_name = if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
        json.get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    };
    let model_name_str = model_name.as_deref().unwrap_or("default");

    // 3. Log Request with Context
    let team_context = parts.extensions.get::<TeamContext>();
    let auth_info = if let Some(_ctx) = team_context {
        format!("[Model: {}]", model_name_str)
    } else {
        let has_auth_header =
            parts.headers.contains_key("authorization") || parts.headers.contains_key("x-api-key");

        if has_auth_header {
            format!(
                "[Auth: Global (or Invalid Team Key), Model: {}]",
                model_name_str
            )
        } else {
            format!("[Auth: None, Model: {}]", model_name_str)
        }
    };

    tracing::info!(
        "Request Received: {} {} {}",
        parts.method,
        parts.uri,
        auth_info
    );

    let headers = parts.headers;
    let config = state.config.read().unwrap().clone();

    // 2. Resolve Router
    let router_name = if let Some(name) = router_name_override {
        name
    } else if let Some(ctx) = parts.extensions.get::<TeamContext>() {
        // Team Flow
        let team = config.teams.iter().find(|t| t.id == ctx.team_id);
        if team.is_none() {
            return error_response(StatusCode::UNAUTHORIZED, "Team not found");
        }
        let team = team.unwrap();

        // Check Allowed Models
        let policy = &team.policy;
        if !policy.is_model_allowed(model_name_str) {
            tracing::warn!(
                "Policy Failed: Model '{}' not allowed by team policy",
                model_name_str
            );
            return error_response(StatusCode::FORBIDDEN, "Model not allowed by team policy");
        }

        // Check Allowed Routers (Mandatory)
        let allowed_routers = &policy.allowed_routers;
        if allowed_routers.is_empty() {
            tracing::warn!(
                "Policy Failed: No allowed routers configured for team '{}'",
                ctx.team_id
            );
            return error_response(
                StatusCode::FORBIDDEN,
                "No allowed routers configured for team",
            );
        }

        let mut selected_router = None;
        for r_name in allowed_routers {
            if config
                .routers
                .iter()
                .find(|r| r.name == *r_name)
                .filter(|router| {
                    state
                        .selector
                        .select_channel(router, model_name_str)
                        .is_some()
                })
                .is_some()
            {
                selected_router = Some(r_name.clone());
                break;
            }
        }

        match selected_router {
            Some(name) => name,
            None => {
                tracing::warn!(
                    "Router Resolution Failed: No matching router found for model '{}' in allowed routers",
                    model_name_str
                );
                return error_response(
                    StatusCode::NOT_FOUND,
                    "No matching router found for model in allowed routers",
                );
            }
        }
    } else {
        // Global Auth Flow (Legacy/Admin)
        if let Err(resp) = enforce_global_auth(&config, &headers) {
            return resp;
        }

        // Try to find ANY router that handles the model
        let mut selected_router = None;
        for router in config.routers.iter() {
            if state
                .selector
                .select_channel(router, model_name_str)
                .is_some()
            {
                selected_router = Some(router.name.clone());
                break;
            }
        }

        match selected_router {
            Some(name) => name,
            None => {
                tracing::warn!(
                    "Router Resolution Failed: No matching router found for model '{}'",
                    model_name_str
                );
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "No matching router found for model",
                );
            }
        }
    };

    let Some(router) = config.routers.iter().find(|r| r.name == router_name) else {
        return error_response(StatusCode::NOT_FOUND, "router not found");
    };

    tracing::info!("Router Resolved: {}", router.name);
    tracing::Span::current().record("router_name", &router.name);

    // 3. Resolve Channels
    let mut channels = Vec::new();

    if let Some(ch_name) = state.selector.select_channel(router, model_name_str)
        && let Some(ch) = config.channels.iter().find(|c| c.name == ch_name)
    {
        channels.push(ch);
        tracing::info!(
            "Channel Resolved: {} (strategy={}, model={})",
            ch.name,
            router.strategy,
            model_name_str
        );
    } else {
        tracing::info!(
            "Fallback Triggered: No rule matched for model '{}' or all primary channels failed. Trying fallback channels.",
            model_name_str
        );

        // Fallback logic
        for fb_name in &router.fallback_channels {
            if let Some(channel) = config.channels.iter().find(|c| c.name == *fb_name) {
                tracing::info!("Channel Resolved (Fallback): {}", channel.name);
                // Avoid duplicates
                if !channels.iter().any(|c| c.name == channel.name) {
                    channels.push(channel);
                }
            } else {
                tracing::warn!("Fallback channel not found: {}", fb_name);
            }
        }

        if channels.is_empty() {
            tracing::error!(
                "Channel Resolution Failed: All Channels Failed for model '{}'",
                model_name_str
            );
        }
    }

    if channels.is_empty() {
        tracing::warn!(
            "Channel Resolution Failed: No channels configured or matched for router: {}",
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

    let mut index = 0;
    let mut fallback_triggered = false;

    while index < channels.len() {
        let channel = channels[index];
        tracing::Span::current().record("channel_name", &channel.name);

        if index > 0 {
            tracing::warn!(
                "Fallback Triggered: Switching to fallback channel: {}",
                channel.name
            );
            state
                .metrics
                .fallback_total
                .with_label_values(&[&router_name, &channel.name])
                .inc();
        }

        if !state.rate_limiter.check(&channel.provider_type) {
            tracing::warn!("Rate Limit Exceeded: Provider {:?}", channel.provider_type);
            index += 1;
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
                Err(e) => {
                    tracing::warn!("Upstream Request Build Failed: {}", e);
                    return error_response(StatusCode::BAD_REQUEST, &e.to_string());
                }
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
                Err(e) => {
                    tracing::warn!("Upstream Request Build Failed: {}", e);
                    return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string());
                }
            };

            tracing::info!(
                "Upstream Request: method={} url={} attempt={}/{}",
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
                        tracing::info!("Upstream Success: {} ({}ms)", status, elapsed);
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

                    tracing::warn!("Upstream Failed: {} ({}ms)", status, elapsed);
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
                                "Retry Triggered: attempt {}/{} due to status {}",
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
                        // Check if we can trigger fallback
                        if !fallback_triggered && !router.fallback_channels.is_empty() {
                            tracing::warn!(
                                "Upstream Failed: Channel '{}' failed, trying fallback...",
                                channel.name
                            );
                            fallback_triggered = true;
                            for fb_name in &router.fallback_channels {
                                if let Some(fb_ch) =
                                    config.channels.iter().find(|c| c.name == *fb_name).filter(
                                        |fb_ch| !channels.iter().any(|c| c.name == fb_ch.name),
                                    )
                                {
                                    channels.push(fb_ch);
                                }
                            }
                            break; // Break attempt loop, proceed to next channel
                        }

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
                    tracing::error!("Upstream Error: {}", e);
                    state
                        .access_audit
                        .audit(&channel.provider_type, route, false);
                    if attempt + 1 < max_attempts {
                        tracing::warn!(
                            "Retry Triggered: attempt {}/{} due to error",
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

        // If all attempts failed (network error), check fallback
        if index == channels.len() - 1
            && !fallback_triggered
            && !router.fallback_channels.is_empty()
        {
            tracing::warn!(
                "Upstream Failed: Channel '{}' failed (network), trying fallback...",
                channel.name
            );
            fallback_triggered = true;
            for fb_name in &router.fallback_channels {
                if let Some(fb_ch) = config
                    .channels
                    .iter()
                    .find(|c| c.name == *fb_name)
                    .filter(|fb_ch| !channels.iter().any(|c| c.name == fb_ch.name))
                {
                    channels.push(fb_ch);
                }
            }
        }

        index += 1;
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
            version: "1".to_string(),
            global: Global {
                listen: "0.0.0.0:0".to_string(),
                auth: Auth {
                    mode: AuthMode::None,
                    keys: None,
                },
                timeouts: Timeouts {
                    connect_ms: 100,
                    request_ms: 100,
                    response_ms: 100,
                },
                retries: Retries {
                    max_attempts: 1,
                    backoff_ms: 10,
                    retry_on_status: vec![],
                },
            },
            metrics: crate::config::Metrics {
                enabled: false,
                listen: "0.0.0.0:0".to_string(),
                path: "/metrics".to_string(),
            },
            hot_reload: crate::config::HotReload {
                config_path: "test.json".to_string(),
                watch: false,
            },
            logging: crate::config::Logging {
                level: "info".to_string(),
                dir: None,
            },
            teams: Arc::new(vec![]),
            channels: Arc::new(vec![
                crate::config::Channel {
                    name: "test-channel".to_string(),
                    provider_type: ProviderType::Openai,
                    base_url: "http://localhost:8080".to_string(),
                    api_key: "sk-test".to_string(),
                    anthropic_base_url: None,
                    headers: None,
                    model_map: None,
                    timeouts: None,
                },
                crate::config::Channel {
                    name: "test-channel-2".to_string(),
                    provider_type: ProviderType::Anthropic,
                    base_url: "http://localhost:8080".to_string(),
                    api_key: "sk-test".to_string(),
                    anthropic_base_url: None,
                    headers: None,
                    model_map: None,
                    timeouts: None,
                },
            ]),
            routers: Arc::new(vec![crate::config::Router {
                name: "test-router".to_string(),
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
                channels: vec![crate::config::TargetChannel {
                    name: "test-channel".to_string(),
                    weight: 1,
                }],
                strategy: "round_robin".to_string(),
                metadata: None,
                fallback_channels: vec![],
            }]),
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
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
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
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
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
        assert!(!calls[0].1); // Failed
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
        Arc::make_mut(&mut config.channels).push(Channel {
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
        let router = &mut Arc::make_mut(&mut config.routers)[0];
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
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
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

    #[tokio::test]
    async fn test_team_flow() {
        let mut config = create_test_config();

        // Add a team
        Arc::make_mut(&mut config.teams).push(crate::config::Team {
            id: "test-team".to_string(),
            api_key: "sk-ant-test".to_string(),
            policy: crate::config::TeamPolicy {
                allowed_routers: vec!["test-router".to_string()],
                allowed_models: Some(vec!["gpt-4".to_string()]),
                rate_limit: None,
            },
        });

        let state = Arc::new(AppState {
            config: Arc::new(RwLock::new(config)),
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: Arc::new(Mutex::new(Vec::new())),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(None).unwrap()),
        });

        // 1. Valid Request (Correct Key, Allowed Model)
        // Note: In unit test we manually inject TeamContext because middleware is bypassed
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer sk-ant-test")
            .extension(TeamContext {
                team_id: "test-team".to_string(),
            })
            .body(Body::from(r#"{"model": "gpt-4"}"#))
            .unwrap();

        let resp = handle_openai(State(state.clone()), req).await;
        // Should pass auth/policy checks and fail at upstream (BAD_GATEWAY) or "no channels"
        // Since "test-router" matches "*", it should find a channel.
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

        // 2. Invalid Model (Not in allowed_models)
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer sk-ant-test")
            .extension(TeamContext {
                team_id: "test-team".to_string(),
            })
            .body(Body::from(r#"{"model": "gpt-3.5"}"#))
            .unwrap();

        let resp = handle_openai(State(state.clone()), req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
