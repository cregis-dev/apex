use crate::config::Config;
use crate::converters::convert_openai_response_to_anthropic;
use crate::database::Database;
use crate::metrics::MetricsState;
use crate::middleware::auth::{TeamContext, global_auth, team_auth};
use crate::middleware::policy::team_policy;
use crate::middleware::ratelimit::TeamRateLimiter;
use crate::providers::{
    AccessAudit, NoOpAccessAudit, NoOpRateLimiter, ProviderRegistry, RateLimiter, RouteKind,
    prepare_request,
};
use crate::router_selector::RouterSelector;
use crate::usage::UsageLogger;
use crate::web_assets::{WebAssetError, load_web_asset};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, HeaderValue, Request, Response as HttpResponse, StatusCode, Uri};
use axum::response::{Redirect, Response};
use axum::routing::{get, post};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::json;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::{self, TraceLayer};
use tracing::{Level, error, info};

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
    pub mcp_server: Arc<McpServer>,
    pub database: Arc<Database>,
    pub web_dir: String,
}

use crate::mcp::server::{McpServer, streamable_http_handler};

// MCP is integrated into main server, controlled by global.enable_mcp config

pub async fn run_server(path: PathBuf) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(&path)?;
    let mut config: Config = serde_json::from_str(&content)?;

    // Store config path for potential hot reload
    config.hot_reload.config_path = path.to_string_lossy().to_string();

    let state = build_state(config.clone())?;
    let app = build_app(state.clone());

    // Start config watcher
    if config.hot_reload.watch {
        let path_clone = path.clone();
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = watch_config(path_clone, state_clone).await {
                error!("Config watcher failed: {}", e);
            }
        });
    }

    let addr: SocketAddr = config.global.listen.parse()?;
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn watch_config(path: PathBuf, state: Arc<AppState>) -> notify::Result<()> {
    // Watch parent directory for robust file replacement handling (atomic saves)
    let path = std::fs::canonicalize(&path)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path
        .file_name()
        .ok_or_else(|| {
            notify::Error::new(notify::ErrorKind::Generic("Invalid config path".into()))
        })?
        .to_os_string();

    let (tx, mut rx) = mpsc::channel(1);

    // Create a watcher that sends events to the channel
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                // We care about any event that affects our file
                let matches = event
                    .paths
                    .iter()
                    .any(|p| p.file_name().map(|n| n == filename).unwrap_or(false));

                if matches {
                    let _ = tx.blocking_send(());
                }
            }
        },
        NotifyConfig::default(),
    )?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(parent, RecursiveMode::NonRecursive)?;

    info!("Started watching config file: {:?}", path);

    // Debounce logic
    let debounce_duration = Duration::from_millis(500);

    loop {
        // Wait for an event
        if rx.recv().await.is_none() {
            break;
        }

        // Debounce: Wait for a short period to accumulate events
        // If more events come in, we just proceed after the timeout
        tokio::time::sleep(debounce_duration).await;

        // Drain any other pending events
        while rx.try_recv().is_ok() {}

        info!("Config file changed, reloading...");

        // Reload config
        match crate::config::load_config(&path) {
            Ok(new_config) => {
                // Update config
                {
                    let mut config_guard = state.config.write().unwrap();
                    // Preserve hot_reload config path if needed, or just overwrite
                    // The new config from file might not have the path set in hot_reload struct if it's not in JSON
                    // But we are reading from the same path.
                    // Ideally we merge or just replace.
                    // Let's replace but ensure critical internal fields are preserved if any.
                    // Actually Config is pure data.

                    // Note: If we use Arc for teams/routers/channels, deserialization creates new Arcs.
                    // This is exactly what we want.
                    *config_guard = new_config;
                    // Restore the path just in case
                    config_guard.hot_reload.config_path = path.to_string_lossy().to_string();
                }

                // Invalidate router cache
                state.selector.invalidate_cache();

                // Notify MCP clients about config change
                state.mcp_server.update_config().await;

                info!("Config reloaded successfully");
            }
            Err(e) => {
                error!("Failed to reload config: {}", e);
            }
        }
    }

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

    let database = Arc::new(Database::new(Some(config.data_dir.clone()))?);
    let usage_logger = Arc::new(UsageLogger::new(database.clone()));
    let web_dir = config.web_dir.clone();
    let config_arc = Arc::new(RwLock::new(config));
    let mcp_server = Arc::new(McpServer::new(config_arc.clone(), database.clone()));

    Ok(Arc::new(AppState {
        config: config_arc,
        metrics: Arc::new(MetricsState::new()?),
        providers: Arc::new(ProviderRegistry::new()),
        access_audit: Arc::new(NoOpAccessAudit),
        rate_limiter: Arc::new(NoOpRateLimiter),
        team_rate_limiter: Arc::new(TeamRateLimiter::new()),
        selector: Arc::new(RouterSelector::new()),
        client,
        usage_logger,
        mcp_server,
        database,
        web_dir,
    }))
}

pub fn build_app(state: Arc<AppState>) -> Router {
    let config = state.config.read().unwrap();
    let mcp_enabled = config.global.enable_mcp;
    let metrics_enabled = config.metrics.enabled;
    let cors_allowed_origins = config.global.cors_allowed_origins.clone();
    drop(config);

    // Model Routes (Protected by Team Auth)
    let model_routes = Router::new()
        .route("/v1/chat/completions", post(handle_openai))
        .route("/v1/completions", post(handle_openai))
        .route("/v1/embeddings", post(handle_openai))
        .route("/v1/models", get(handle_models))
        .route("/v1/messages", post(handle_anthropic))
        .route("/v1/responses", post(handle_openai))
        // Compatibility routes (no /v1 prefix)
        .route("/chat/completions", post(handle_openai))
        .route("/completions", post(handle_openai))
        .route("/embeddings", post(handle_openai))
        .route("/models", get(handle_models))
        .route("/messages", post(handle_anthropic))
        .route("/responses", post(handle_openai))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            team_policy,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            team_auth,
        ));

    // MCP Routes (Protected by Global API Key) - Streamable HTTP Transport (MCP 2025-11-25)
    let mcp_routes = if mcp_enabled {
        Some(
            Router::new()
                .route(
                    "/mcp",
                    get(streamable_http_handler)
                        .post(streamable_http_handler)
                        .delete(streamable_http_handler),
                )
                .layer(axum::middleware::from_fn_with_state(
                    state.mcp_server.as_ref().clone(),
                    crate::mcp::server::mcp_auth_guard,
                )),
        )
    } else {
        None
    };

    // Admin/System Routes (no auth required)
    let admin_routes = Router::new()
        .route("/admin/teams", get(handle_admin_teams))
        .route("/admin/routers", get(handle_admin_routers))
        .route("/admin/channels", get(handle_admin_channels));

    // Metrics (Protected by Global API Key)
    let metrics_routes = if metrics_enabled {
        Some(
            Router::new()
                .route("/metrics", get(metrics_handler))
                .route("/api/usage", get(usage_api_handler))
                .route("/api/metrics", get(metrics_api_handler))
                .route("/api/metrics/trends", get(trends_api_handler))
                .route("/api/metrics/rankings", get(rankings_api_handler))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    global_auth,
                )),
        )
    } else {
        None
    };

    // Combine all routes using merge (each has its own middleware)
    let mut app = model_routes.merge(admin_routes);

    if let Some(mcp) = mcp_routes {
        app = app.merge(mcp);
    }

    if let Some(metrics) = metrics_routes {
        app = app.merge(metrics);
    }

    // Next.js static resources (shared across all pages)
    let next_static_routes = Router::new().route(
        "/_next/static/*path",
        get(
            move |State(state): State<Arc<AppState>>,
                  axum::extract::Path(path): axum::extract::Path<String>| async move {
                serve_web_asset(&state.web_dir, &format!("_next/static/{path}"), "Not found")
            },
        ),
    );

    // Dashboard static files (Next.js static export)
    // Serve from web directory configured in config.json, default to "web"
    let dashboard_routes = Router::new()
        .route(
            "/dashboard",
            get(|OriginalUri(uri): OriginalUri| async move {
                Redirect::permanent(&dashboard_redirect_target(&uri))
            }),
        )
        .route(
            "/dashboard/",
            get(move |State(state): State<Arc<AppState>>| async move {
                serve_web_asset(
                    &state.web_dir,
                    "dashboard/index.html",
                    "Dashboard not found",
                )
            }),
        )
        .route(
            "/dashboard/*path",
            get(
                move |State(state): State<Arc<AppState>>,
                      axum::extract::Path(path): axum::extract::Path<String>| async move {
                    serve_web_asset(&state.web_dir, &format!("dashboard/{path}"), "Not found")
                },
            ),
        )
        .route(
            "/",
            get(move |State(state): State<Arc<AppState>>| async move {
                serve_index(State(state)).await
            }),
        )
        .route(
            "/index",
            get(move |State(state): State<Arc<AppState>>| async move {
                serve_index(State(state)).await
            }),
        );

    app = app.merge(next_static_routes);
    app = app.merge(dashboard_routes);

    app.layer(
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
                            team_id = tracing::field::Empty,
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
    // CORS layer for dashboard frontend
    .layer(
        CorsLayer::new()
            .allow_origin(build_cors_allow_origin(&cors_allowed_origins))
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any),
    )
    .with_state(state)
}

fn build_cors_allow_origin(cors_allowed_origins: &[String]) -> tower_http::cors::AllowOrigin {
    if cors_allowed_origins.is_empty() {
        return tower_http::cors::Any.into();
    }

    let origins = cors_allowed_origins
        .iter()
        .filter_map(|origin| match origin.parse::<HeaderValue>() {
            Ok(value) => Some(value),
            Err(err) => {
                tracing::warn!("Ignoring invalid CORS origin '{}': {}", origin, err);
                None
            }
        })
        .collect::<Vec<_>>();

    if origins.is_empty() {
        tracing::warn!(
            "No valid CORS origins configured; browser cross-origin requests will be denied"
        );
    }

    origins.into()
}

fn dashboard_redirect_target(uri: &Uri) -> String {
    match uri.query() {
        Some(query) if !query.is_empty() => format!("/dashboard/?{query}"),
        _ => "/dashboard/".to_string(),
    }
}

async fn serve_index(state: State<Arc<AppState>>) -> Response<Body> {
    match load_web_asset(&state.web_dir, "index.html") {
        Ok(asset) => build_asset_response(StatusCode::OK, asset),
        Err(err) => {
            tracing::error!(
                "Failed to load index.html from {}: {:?}",
                state.web_dir,
                err
            );
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/html")
                .body(Body::from(
                    r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Apex Gateway</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }
        .container { text-align: center; }
        h1 { color: #333; }
        a { color: #0066cc; text-decoration: none; font-size: 18px; }
        a:hover { text-decoration: underline; }
    </style>
</head>
<body>
    <div class="container">
        <h1>Apex Gateway</h1>
        <p><a href="/dashboard/">Go to Dashboard</a></p>
    </div>
</body>
</html>"#,
                ))
                .unwrap()
        }
    }
}

fn serve_web_asset(web_dir: &str, relative_path: &str, not_found_body: &'static str) -> Response {
    match load_web_asset(web_dir, relative_path) {
        Ok(asset) => build_asset_response(StatusCode::OK, asset),
        Err(WebAssetError::Forbidden) => Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Body::from("Forbidden"))
            .unwrap(),
        Err(WebAssetError::NotFound) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from(not_found_body))
            .unwrap(),
        Err(WebAssetError::Internal) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("Internal error"))
            .unwrap(),
    }
}

fn build_asset_response(status: StatusCode, asset: crate::web_assets::WebAsset) -> Response {
    let mut builder = Response::builder()
        .status(status)
        .header("content-type", asset.content_type);

    if let Some(cache_control) = asset.cache_control {
        builder = builder.header("cache-control", cache_control);
    }

    builder.body(Body::from(asset.bytes.into_owned())).unwrap()
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

async fn usage_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response<Body> {
    let team_id = params.get("team_id").map(|s| s.as_str());
    let router = params.get("router").map(|s| s.as_str());
    let channel = params.get("channel").map(|s| s.as_str());
    let model = params.get("model").map(|s| s.as_str());
    let status = params.get("status").map(|s| s.as_str());
    let start_date = params.get("start_date").map(|s| s.as_str());
    let end_date = params.get("end_date").map(|s| s.as_str());
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(50)
        .min(100);
    let offset = params
        .get("offset")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);

    match state.database.get_usage_records(
        team_id, router, channel, model, status, start_date, end_date, limit, offset,
    ) {
        Ok((records, total)) => {
            let json = serde_json::json!({
                "data": records,
                "total": total,
                "limit": limit,
                "offset": offset
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap()
        }
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap(),
    }
}

async fn metrics_api_handler(state: State<Arc<AppState>>) -> Response<Body> {
    match state.database.get_metrics_summary() {
        Ok(summary) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&summary).unwrap_or_default(),
            ))
            .unwrap(),
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap(),
    }
}

async fn trends_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response<Body> {
    let period = params.get("period").map(|s| s.as_str()).unwrap_or("daily");
    let start_date = params.get("start_date").map(|s| s.as_str());
    let end_date = params.get("end_date").map(|s| s.as_str());

    match state.database.get_trends(period, start_date, end_date) {
        Ok(trends) => {
            let json = serde_json::json!({
                "period": period,
                "data": trends
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap()
        }
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap(),
    }
}

async fn rankings_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response<Body> {
    let by = params.get("by").map(|s| s.as_str()).unwrap_or("team_id");
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(10);

    match state.database.get_rankings(by, limit) {
        Ok(rankings) => {
            let json = serde_json::json!({
                "by": by,
                "data": rankings
            });
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(json.to_string()))
                .unwrap()
        }
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

    // Check Team Context or global auth required
    if parts.extensions.get::<TeamContext>().is_none() && !config.global.auth_keys.is_empty() {
        return error_response(StatusCode::UNAUTHORIZED, "Team API Key Required");
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

async fn handle_admin_teams(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let headers = &parts.headers;

    let config = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config, headers) {
        return resp;
    }

    let data = config
        .teams
        .iter()
        .map(|team| {
            let rate_limit = team.policy.rate_limit.as_ref().map(|l| {
                json!({
                    "rpm": l.rpm,
                    "tpm": l.tpm
                })
            });
            json!({
                "id": team.id,
                "api_key": mask_secret(&team.api_key),
                "policy": {
                    "allowed_routers": team.policy.allowed_routers,
                    "allowed_models": team.policy.allowed_models,
                    "rate_limit": rate_limit
                }
            })
        })
        .collect::<Vec<_>>();

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "object": "list",
                "data": data
            })
            .to_string(),
        ))
        .unwrap()
}

async fn handle_admin_routers(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let headers = &parts.headers;

    let config = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config, headers) {
        return resp;
    }

    let data = config
        .routers
        .iter()
        .filter_map(|router| serde_json::to_value(router).ok())
        .collect::<Vec<_>>();

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "object": "list",
                "data": data
            })
            .to_string(),
        ))
        .unwrap()
}

async fn handle_admin_channels(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let headers = &parts.headers;

    let config = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config, headers) {
        return resp;
    }

    let data = config
        .channels
        .iter()
        .map(|channel| {
            json!({
                "name": channel.name,
                "provider_type": channel.provider_type,
                "base_url": channel.base_url,
                "api_key": mask_secret(&channel.api_key),
                "anthropic_base_url": channel.anthropic_base_url
            })
        })
        .collect::<Vec<_>>();

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "object": "list",
                "data": data
            })
            .to_string(),
        ))
        .unwrap()
}

// Helpers

#[allow(clippy::result_large_err)]
fn enforce_global_auth(config: &Config, headers: &HeaderMap) -> Result<(), Response<Body>> {
    let keys = &config.global.auth_keys;

    // If no auth_keys configured, skip validation
    if keys.is_empty() {
        return Ok(());
    }

    let candidates = [
        read_auth_token(headers, "authorization"),
        read_auth_token(headers, "x-api-key"),
    ];

    for token in candidates.into_iter().flatten() {
        if keys.contains(&token) {
            return Ok(());
        }
    }

    tracing::warn!("Auth Failed: No valid token found in Authorization or x-api-key headers.");
    Err(error_response(StatusCode::UNAUTHORIZED, "unauthorized"))
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

use crate::utils::mask_secret;

fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    let body = json!({
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

fn request_id_from_parts(parts: &axum::http::request::Parts) -> Option<String> {
    parts
        .extensions
        .get::<tower_http::request_id::RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .map(|id| id.to_string())
}

fn provider_trace_id_from_headers(headers: &HeaderMap) -> Option<String> {
    const TRACE_HEADERS: [&str; 5] = [
        "x-request-id",
        "request-id",
        "x-trace-id",
        "trace-id",
        "cf-ray",
    ];

    TRACE_HEADERS.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string())
    })
}

fn response_from_upstream_bytes(
    status: StatusCode,
    headers: &HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let mut builder = HttpResponse::builder().status(status);
    for (name, value) in headers {
        if crate::providers::should_forward_response_header(name) {
            builder = builder.header(name, value);
        }
    }

    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| error_response(StatusCode::BAD_GATEWAY, "invalid upstream response"))
}

fn truncate_for_storage(input: &str, limit: usize) -> String {
    input.chars().take(limit).collect()
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

    let request_id = request_id_from_parts(&parts);
    let headers = parts.headers;

    // Extract team_id for usage logging
    let team_id = parts
        .extensions
        .get::<crate::middleware::auth::TeamContext>()
        .map(|ctx| ctx.team_id.clone())
        .unwrap_or_else(|| "global".to_string());
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
        state.usage_logger.log_failure(
            request_id.as_deref(),
            &team_id,
            &router_name,
            "unresolved",
            model_name_str,
            None,
            false,
            StatusCode::BAD_GATEWAY.as_u16() as i64,
            "no channels configured or matched",
            None,
            None,
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

    // Log request to database
    state.database.log_request(route_label, &router_name);

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

            // Log fallback to database
            state.database.log_fallback(&router_name, &channel.name);
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

                    // Log latency to database
                    state
                        .database
                        .log_latency(route_label, &router_name, &channel.name, elapsed);

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
                            request_id.clone(),
                            team_id.clone(),
                            router_name.clone(),
                            channel.name.clone(),
                            model_name_str.to_string(),
                            state.usage_logger.clone(),
                            state.metrics.clone(),
                            Some(elapsed),
                            fallback_triggered,
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

                        // Log error to database
                        state.database.log_error(route_label, &router_name);
                        let provider_trace_id = provider_trace_id_from_headers(resp.headers());
                        let response_headers = resp.headers().clone();
                        let error_body_bytes = resp.bytes().await.unwrap_or_default();
                        let provider_error_body = String::from_utf8_lossy(&error_body_bytes);
                        let stored_error_body = truncate_for_storage(&provider_error_body, 4000);
                        state.usage_logger.log_failure(
                            request_id.as_deref(),
                            &team_id,
                            &router_name,
                            &channel.name,
                            model_name_str,
                            Some(elapsed),
                            fallback_triggered,
                            status.as_u16() as i64,
                            status
                                .canonical_reason()
                                .unwrap_or("upstream request failed"),
                            provider_trace_id.as_deref(),
                            Some(stored_error_body.as_str()),
                        );

                        // Convert error if needed (e.g. for Anthropic)
                        if matches!(route, RouteKind::Anthropic) {
                            let body = convert_openai_response_to_anthropic(error_body_bytes);
                            return Response::builder()
                                .status(status)
                                .body(Body::from(body))
                                .unwrap();
                        }
                        return response_from_upstream_bytes(
                            status,
                            &response_headers,
                            error_body_bytes,
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

    // Log error to database
    state.database.log_error(route_label, &router_name);
    let last_channel = channels
        .last()
        .map(|channel| channel.name.as_str())
        .unwrap_or("unresolved");
    state.usage_logger.log_failure(
        request_id.as_deref(),
        &team_id,
        &router_name,
        last_channel,
        model_name_str,
        None,
        fallback_triggered,
        StatusCode::BAD_GATEWAY.as_u16() as i64,
        "all channels failed",
        None,
        None,
    );

    error_response(StatusCode::BAD_GATEWAY, "all channels failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Channel, Global, ProviderType, Retries, Timeouts};
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
                auth_keys: vec![],
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
                enable_mcp: true,
                cors_allowed_origins: vec![],
            },
            data_dir: "/tmp".to_string(),
            web_dir: "target/web".to_string(),
            metrics: crate::config::Metrics {
                enabled: false,
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
            prompts: Arc::new(vec![]),
            compliance: None,
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
        let config_arc = Arc::new(RwLock::new(config));
        let audit_calls = Arc::new(Mutex::new(Vec::new()));

        let database = Arc::new(Database::new(None).unwrap());
        let mcp_server = Arc::new(McpServer::new(config_arc.clone(), database.clone()));
        let state = Arc::new(AppState {
            config: config_arc,
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: audit_calls.clone(),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: false }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            mcp_server,
            database,
            web_dir: "target/web".to_string(),
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
        let config_arc = Arc::new(RwLock::new(config));
        let audit_calls = Arc::new(Mutex::new(Vec::new()));

        let database = Arc::new(Database::new(None).unwrap());
        let mcp_server = Arc::new(McpServer::new(config_arc.clone(), database.clone()));
        let state = Arc::new(AppState {
            config: config_arc,
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: audit_calls.clone(),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            mcp_server,
            database,
            web_dir: "target/web".to_string(),
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

    #[test]
    fn test_dashboard_redirect_target_preserves_query() {
        let uri: Uri = "/dashboard?auth_token=goodluck".parse().unwrap();
        assert_eq!(
            dashboard_redirect_target(&uri),
            "/dashboard/?auth_token=goodluck"
        );

        let uri: Uri = "/dashboard".parse().unwrap();
        assert_eq!(dashboard_redirect_target(&uri), "/dashboard/");
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
        let config_arc = Arc::new(RwLock::new(config));

        let database = Arc::new(Database::new(None).unwrap());
        let mcp_server = Arc::new(McpServer::new(config_arc.clone(), database.clone()));
        let state = Arc::new(AppState {
            config: config_arc,
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: audit_calls.clone(),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            mcp_server,
            database,
            web_dir: "target/web".to_string(),
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
            api_key: "sk-ap-test".to_string(),
            policy: crate::config::TeamPolicy {
                allowed_routers: vec!["test-router".to_string()],
                allowed_models: Some(vec!["gpt-4".to_string()]),
                rate_limit: None,
            },
        });

        let config_arc = Arc::new(RwLock::new(config));

        let database = Arc::new(Database::new(None).unwrap());
        let mcp_server = Arc::new(McpServer::new(config_arc.clone(), database.clone()));
        let state = Arc::new(AppState {
            config: config_arc,
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: Arc::new(Mutex::new(Vec::new())),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            mcp_server,
            database,
            web_dir: "target/web".to_string(),
        });

        // 1. Valid Request (Correct Key, Allowed Model)
        // Note: In unit test we manually inject TeamContext because middleware is bypassed
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer sk-ap-test")
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
            .header("Authorization", "Bearer sk-ap-test")
            .extension(TeamContext {
                team_id: "test-team".to_string(),
            })
            .body(Body::from(r#"{"model": "gpt-3.5"}"#))
            .unwrap();

        let resp = handle_openai(State(state.clone()), req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
