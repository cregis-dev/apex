use crate::config::Config;
use crate::converters::convert_openai_response_to_anthropic;
use crate::database::{Database, UsageRecord as DashboardUsageRecord, UsageRecordQuery};
use crate::gemini_compat::{GeminiAnthropicReplayCache, gemini_replay_missing_signature};
use crate::metrics::MetricsState;
use crate::middleware::auth::{TeamContext, global_auth, team_auth};
use crate::middleware::compliance::{OriginalModelName, compliance_middleware};
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
use chrono::{Duration as ChronoDuration, Local, NaiveDateTime};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    pub gemini_replay: Arc<GeminiAnthropicReplayCache>,
    pub client: reqwest::Client,
    pub usage_logger: Arc<UsageLogger>,
    pub mcp_server: Arc<McpServer>,
    pub database: Arc<Database>,
    pub web_dir: String,
}

pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 10 * 1024 * 1024;

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
        gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
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
            compliance_middleware,
        ))
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
                .route(
                    "/api/dashboard/analytics",
                    get(dashboard_analytics_api_handler),
                )
                .route("/api/dashboard/records", get(dashboard_records_api_handler))
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

#[derive(Debug, Clone)]
enum DashboardBucket {
    Hour,
    Day,
}

#[derive(Debug, Clone)]
struct DashboardWindow {
    range: String,
    bucket: DashboardBucket,
    current_start: NaiveDateTime,
    current_end: NaiveDateTime,
    previous_start: NaiveDateTime,
    previous_end: NaiveDateTime,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardRecordCursor {
    id: i64,
    timestamp: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardFilterOptions {
    teams: Vec<String>,
    models: Vec<String>,
    routers: Vec<String>,
    channels: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardOverviewDelta {
    total_requests: f64,
    total_tokens: f64,
    avg_latency_ms: f64,
    success_rate: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardOverview {
    total_requests: i64,
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    avg_latency_ms: f64,
    success_rate: f64,
    delta: DashboardOverviewDelta,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTrendPoint {
    bucket: String,
    label: String,
    requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    error_rate: f64,
    avg_latency_ms: f64,
    success_rate: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTrendSection {
    unit: String,
    points: Vec<DashboardTrendPoint>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTeamLeaderboardItem {
    team_id: String,
    total_requests: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTeamModelUsageItem {
    team_id: String,
    model: String,
    total_requests: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTeamUsageSection {
    leaderboard: Vec<DashboardTeamLeaderboardItem>,
    model_usage: Vec<DashboardTeamModelUsageItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardChannelLatencyItem {
    channel: String,
    total_requests: i64,
    avg_latency_ms: f64,
    p95_latency_ms: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardSystemReliabilitySection {
    error_rate_trend: Vec<DashboardTrendPoint>,
    channel_latency: Vec<DashboardChannelLatencyItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardShareItem {
    name: String,
    requests: i64,
    total_tokens: i64,
    percentage: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardModelRouterSection {
    model_share: Vec<DashboardShareItem>,
    router_summary: Vec<DashboardShareItem>,
    channel_summary: Vec<DashboardShareItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTopologyNode {
    name: String,
    kind: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTopologyLink {
    source: usize,
    target: usize,
    value: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardFlowSummary {
    team_id: String,
    router: String,
    channel: String,
    model: String,
    requests: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTopologySection {
    nodes: Vec<DashboardTopologyNode>,
    links: Vec<DashboardTopologyLink>,
    flows: Vec<DashboardFlowSummary>,
    render_mode: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardRecordsMeta {
    total: usize,
    latest_cursor: Option<DashboardRecordCursor>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardAnalyticsResponse {
    generated_at: String,
    range: String,
    filter_options: DashboardFilterOptions,
    overview: DashboardOverview,
    trend: DashboardTrendSection,
    topology: DashboardTopologySection,
    team_usage: DashboardTeamUsageSection,
    system_reliability: DashboardSystemReliabilitySection,
    model_router: DashboardModelRouterSection,
    records_meta: DashboardRecordsMeta,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardRecordsResponse {
    data: Vec<DashboardUsageRecord>,
    total: usize,
    limit: usize,
    offset: usize,
    latest_cursor: Option<DashboardRecordCursor>,
    new_records: usize,
}

#[derive(Default)]
struct TrendAccumulator {
    label: String,
    requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    error_count: i64,
    latency_sum: f64,
    latency_count: i64,
}

fn dashboard_window(range: Option<&str>) -> DashboardWindow {
    let now = Local::now().naive_local();
    let (range_key, bucket, duration) = match range.unwrap_or("24h") {
        "1h" => ("1h", DashboardBucket::Hour, ChronoDuration::hours(1)),
        "7d" => ("7d", DashboardBucket::Day, ChronoDuration::days(7)),
        "30d" => ("30d", DashboardBucket::Day, ChronoDuration::days(30)),
        _ => ("24h", DashboardBucket::Hour, ChronoDuration::hours(24)),
    };
    let current_start = now - duration;
    let previous_end = current_start - ChronoDuration::seconds(1);
    let previous_start = previous_end - duration + ChronoDuration::seconds(1);

    DashboardWindow {
        range: range_key.to_string(),
        bucket,
        current_start,
        current_end: now,
        previous_start,
        previous_end,
    }
}

fn format_dashboard_timestamp(value: NaiveDateTime) -> String {
    value.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn parse_dashboard_timestamp(value: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S"))
        .ok()
}

fn normalize_query_filter(params: &HashMap<String, String>, key: &str) -> Option<String> {
    params
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && *value != "all")
        .map(str::to_string)
}

fn build_dashboard_usage_query(
    params: &HashMap<String, String>,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> UsageRecordQuery {
    UsageRecordQuery {
        team_id: normalize_query_filter(params, "team_id"),
        router: normalize_query_filter(params, "router"),
        channel: normalize_query_filter(params, "channel"),
        model: normalize_query_filter(params, "model"),
        status: normalize_query_filter(params, "status"),
        start_time: Some(format_dashboard_timestamp(start)),
        end_time: Some(format_dashboard_timestamp(end)),
    }
}

fn usage_record_total_tokens(record: &DashboardUsageRecord) -> i64 {
    record.input_tokens.max(0) + record.output_tokens.max(0)
}

fn usage_record_is_error(record: &DashboardUsageRecord) -> bool {
    matches!(record.status.as_str(), "error" | "fallback_error")
}

fn percent_change(current: f64, previous: f64) -> f64 {
    if previous.abs() < f64::EPSILON {
        return if current.abs() < f64::EPSILON {
            0.0
        } else {
            100.0
        };
    }

    ((current - previous) / previous) * 100.0
}

fn percentile(values: &mut [f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    values.sort_by(f64::total_cmp);
    let rank = ((values.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).floor() as usize;
    values.get(rank).copied().unwrap_or(0.0)
}

fn build_filter_options(records: &[DashboardUsageRecord]) -> DashboardFilterOptions {
    let mut teams = BTreeSet::new();
    let mut models = BTreeSet::new();
    let mut routers = BTreeSet::new();
    let mut channels = BTreeSet::new();

    for record in records {
        teams.insert(record.team_id.clone());
        models.insert(record.model.clone());
        routers.insert(record.router.clone());
        channels.insert(record.final_channel.clone());
    }

    DashboardFilterOptions {
        teams: teams.into_iter().collect(),
        models: models.into_iter().collect(),
        routers: routers.into_iter().collect(),
        channels: channels.into_iter().collect(),
    }
}

fn build_overview(
    current_records: &[DashboardUsageRecord],
    previous_records: &[DashboardUsageRecord],
) -> DashboardOverview {
    let total_requests = current_records.len() as i64;
    let input_tokens = current_records
        .iter()
        .map(|record| record.input_tokens.max(0))
        .sum();
    let output_tokens = current_records
        .iter()
        .map(|record| record.output_tokens.max(0))
        .sum();
    let total_tokens = input_tokens + output_tokens;
    let current_errors = current_records
        .iter()
        .filter(|record| usage_record_is_error(record))
        .count();
    let success_rate = if total_requests > 0 {
        ((total_requests - current_errors as i64) as f64 / total_requests as f64) * 100.0
    } else {
        0.0
    };
    let avg_latency_ms = {
        let latencies = current_records
            .iter()
            .filter_map(|record| record.latency_ms)
            .filter(|latency| latency.is_finite())
            .collect::<Vec<_>>();
        if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        }
    };

    let previous_requests = previous_records.len() as f64;
    let previous_tokens = previous_records
        .iter()
        .map(usage_record_total_tokens)
        .sum::<i64>() as f64;
    let previous_latency_values = previous_records
        .iter()
        .filter_map(|record| record.latency_ms)
        .filter(|latency| latency.is_finite())
        .collect::<Vec<_>>();
    let previous_latency = if previous_latency_values.is_empty() {
        0.0
    } else {
        previous_latency_values.iter().sum::<f64>() / previous_latency_values.len() as f64
    };
    let previous_errors = previous_records
        .iter()
        .filter(|record| usage_record_is_error(record))
        .count() as f64;
    let previous_success_rate = if previous_requests > 0.0 {
        ((previous_requests - previous_errors) / previous_requests) * 100.0
    } else {
        0.0
    };

    DashboardOverview {
        total_requests,
        total_tokens,
        input_tokens,
        output_tokens,
        avg_latency_ms,
        success_rate,
        delta: DashboardOverviewDelta {
            total_requests: percent_change(total_requests as f64, previous_requests),
            total_tokens: percent_change(total_tokens as f64, previous_tokens),
            avg_latency_ms: percent_change(avg_latency_ms, previous_latency),
            success_rate: percent_change(success_rate, previous_success_rate),
        },
    }
}

fn bucket_key(bucket: &DashboardBucket, timestamp: NaiveDateTime) -> String {
    match bucket {
        DashboardBucket::Hour => timestamp.format("%Y-%m-%d %H:00:00").to_string(),
        DashboardBucket::Day => timestamp.format("%Y-%m-%d").to_string(),
    }
}

fn bucket_label(bucket: &DashboardBucket, timestamp: NaiveDateTime) -> String {
    match bucket {
        DashboardBucket::Hour => timestamp.format("%H:%M").to_string(),
        DashboardBucket::Day => timestamp.format("%m-%d").to_string(),
    }
}

fn iter_bucket_points(window: &DashboardWindow) -> Vec<(String, String)> {
    let mut current = window.current_start;
    let step = match window.bucket {
        DashboardBucket::Hour => ChronoDuration::hours(1),
        DashboardBucket::Day => ChronoDuration::days(1),
    };
    let mut items = Vec::new();

    while current <= window.current_end {
        items.push((
            bucket_key(&window.bucket, current),
            bucket_label(&window.bucket, current),
        ));
        current += step;
    }

    items
}

fn build_trend_section(
    current_records: &[DashboardUsageRecord],
    window: &DashboardWindow,
) -> DashboardTrendSection {
    let mut buckets = BTreeMap::new();
    for (key, label) in iter_bucket_points(window) {
        buckets.insert(
            key,
            TrendAccumulator {
                label,
                ..TrendAccumulator::default()
            },
        );
    }

    for record in current_records {
        let Some(timestamp) = parse_dashboard_timestamp(&record.timestamp) else {
            continue;
        };
        let key = bucket_key(&window.bucket, timestamp);
        let entry = buckets.entry(key).or_insert_with(|| TrendAccumulator {
            label: bucket_label(&window.bucket, timestamp),
            ..TrendAccumulator::default()
        });
        entry.requests += 1;
        entry.input_tokens += record.input_tokens.max(0);
        entry.output_tokens += record.output_tokens.max(0);
        if usage_record_is_error(record) {
            entry.error_count += 1;
        }
        if let Some(latency) = record.latency_ms.filter(|latency| latency.is_finite()) {
            entry.latency_sum += latency;
            entry.latency_count += 1;
        }
    }

    let points = buckets
        .into_iter()
        .map(|(bucket, item)| {
            let total_tokens = item.input_tokens + item.output_tokens;
            let error_rate = if item.requests > 0 {
                (item.error_count as f64 / item.requests as f64) * 100.0
            } else {
                0.0
            };
            let avg_latency_ms = if item.latency_count > 0 {
                item.latency_sum / item.latency_count as f64
            } else {
                0.0
            };

            DashboardTrendPoint {
                bucket,
                label: item.label,
                requests: item.requests,
                input_tokens: item.input_tokens,
                output_tokens: item.output_tokens,
                total_tokens,
                error_rate,
                avg_latency_ms,
                success_rate: if item.requests > 0 {
                    100.0 - error_rate
                } else {
                    0.0
                },
            }
        })
        .collect();

    DashboardTrendSection {
        unit: match window.bucket {
            DashboardBucket::Hour => "hour".to_string(),
            DashboardBucket::Day => "day".to_string(),
        },
        points,
    }
}

fn build_team_usage_section(records: &[DashboardUsageRecord]) -> DashboardTeamUsageSection {
    let mut team_totals: HashMap<String, (i64, i64)> = HashMap::new();
    let mut team_model: HashMap<(String, String), (i64, i64)> = HashMap::new();

    for record in records {
        let total_tokens = usage_record_total_tokens(record);
        let team_entry = team_totals.entry(record.team_id.clone()).or_insert((0, 0));
        team_entry.0 += 1;
        team_entry.1 += total_tokens;

        let model_entry = team_model
            .entry((record.team_id.clone(), record.model.clone()))
            .or_insert((0, 0));
        model_entry.0 += 1;
        model_entry.1 += total_tokens;
    }

    let mut leaderboard = team_totals
        .into_iter()
        .map(
            |(team_id, (total_requests, total_tokens))| DashboardTeamLeaderboardItem {
                team_id,
                total_requests,
                total_tokens,
            },
        )
        .collect::<Vec<_>>();
    leaderboard.sort_by(|left, right| {
        right
            .total_tokens
            .cmp(&left.total_tokens)
            .then_with(|| right.total_requests.cmp(&left.total_requests))
    });
    leaderboard.truncate(5);

    let mut model_usage = team_model
        .into_iter()
        .map(
            |((team_id, model), (total_requests, total_tokens))| DashboardTeamModelUsageItem {
                team_id,
                model,
                total_requests,
                total_tokens,
            },
        )
        .collect::<Vec<_>>();
    model_usage.sort_by(|left, right| {
        left.team_id
            .cmp(&right.team_id)
            .then_with(|| right.total_tokens.cmp(&left.total_tokens))
    });

    DashboardTeamUsageSection {
        leaderboard,
        model_usage,
    }
}

fn build_system_reliability_section(
    records: &[DashboardUsageRecord],
    trend: &DashboardTrendSection,
) -> DashboardSystemReliabilitySection {
    let mut channel_latency: HashMap<String, Vec<f64>> = HashMap::new();

    for record in records {
        if let Some(latency) = record.latency_ms.filter(|latency| latency.is_finite()) {
            channel_latency
                .entry(record.final_channel.clone())
                .or_default()
                .push(latency);
        }
    }

    let mut channel_items = channel_latency
        .into_iter()
        .map(|(channel, mut latencies)| {
            let total_requests = latencies.len() as i64;
            let avg_latency_ms = if latencies.is_empty() {
                0.0
            } else {
                latencies.iter().sum::<f64>() / latencies.len() as f64
            };
            let p95_latency_ms = percentile(&mut latencies, 0.95);
            DashboardChannelLatencyItem {
                channel,
                total_requests,
                avg_latency_ms,
                p95_latency_ms,
            }
        })
        .collect::<Vec<_>>();
    channel_items.sort_by(|left, right| {
        right
            .avg_latency_ms
            .total_cmp(&left.avg_latency_ms)
            .then_with(|| left.channel.cmp(&right.channel))
    });

    DashboardSystemReliabilitySection {
        error_rate_trend: trend.points.clone(),
        channel_latency: channel_items,
    }
}

fn build_model_router_section(records: &[DashboardUsageRecord]) -> DashboardModelRouterSection {
    let total_requests = records.len() as f64;
    let mut model_map: HashMap<String, (i64, i64)> = HashMap::new();
    let mut router_map: HashMap<String, (i64, i64)> = HashMap::new();
    let mut channel_map: HashMap<String, (i64, i64)> = HashMap::new();

    for record in records {
        let total_tokens = usage_record_total_tokens(record);
        model_map
            .entry(record.model.clone())
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
        router_map
            .entry(record.router.clone())
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
        channel_map
            .entry(record.final_channel.clone())
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
    }

    let to_items = |map: HashMap<String, (i64, i64)>| {
        let mut items = map
            .into_iter()
            .map(|(name, (requests, total_tokens))| DashboardShareItem {
                name,
                requests,
                total_tokens,
                percentage: if total_requests > 0.0 {
                    (requests as f64 / total_requests) * 100.0
                } else {
                    0.0
                },
            })
            .collect::<Vec<_>>();
        items.sort_by(|left, right| right.requests.cmp(&left.requests));
        items
    };

    DashboardModelRouterSection {
        model_share: to_items(model_map),
        router_summary: to_items(router_map),
        channel_summary: to_items(channel_map),
    }
}

fn build_topology_section(records: &[DashboardUsageRecord]) -> DashboardTopologySection {
    let mut flow_map: HashMap<(String, String, String, String), (i64, i64)> = HashMap::new();
    for record in records {
        let key = (
            record.team_id.clone(),
            record.router.clone(),
            record.final_channel.clone(),
            record.model.clone(),
        );
        let total_tokens = usage_record_total_tokens(record);
        flow_map
            .entry(key)
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
    }

    let mut flows = flow_map
        .into_iter()
        .map(
            |((team_id, router, channel, model), (requests, total_tokens))| DashboardFlowSummary {
                team_id,
                router,
                channel,
                model,
                requests,
                total_tokens,
            },
        )
        .collect::<Vec<_>>();
    flows.sort_by(|left, right| right.requests.cmp(&left.requests));

    let mut nodes = Vec::new();
    let mut node_index: HashMap<(String, String), usize> = HashMap::new();
    let mut ensure_node = |name: &str, kind: &str| -> usize {
        let key = (kind.to_string(), name.to_string());
        if let Some(index) = node_index.get(&key) {
            return *index;
        }

        let index = nodes.len();
        nodes.push(DashboardTopologyNode {
            name: name.to_string(),
            kind: kind.to_string(),
        });
        node_index.insert(key, index);
        index
    };

    let mut link_values: HashMap<(usize, usize), (i64, i64)> = HashMap::new();
    for flow in &flows {
        let team_idx = ensure_node(&flow.team_id, "team");
        let router_idx = ensure_node(&flow.router, "router");
        let channel_idx = ensure_node(&flow.channel, "channel");
        let model_idx = ensure_node(&flow.model, "model");

        for pair in [
            (team_idx, router_idx),
            (router_idx, channel_idx),
            (channel_idx, model_idx),
        ] {
            link_values
                .entry(pair)
                .and_modify(|value| {
                    value.0 += flow.requests;
                    value.1 += flow.total_tokens;
                })
                .or_insert((flow.requests, flow.total_tokens));
        }
    }

    let links = link_values
        .into_iter()
        .map(
            |((source, target), (value, total_tokens))| DashboardTopologyLink {
                source,
                target,
                value,
                total_tokens,
            },
        )
        .collect::<Vec<_>>();

    DashboardTopologySection {
        nodes,
        links,
        flows,
        render_mode: "sankey".to_string(),
    }
}

fn latest_cursor(records: &[DashboardUsageRecord]) -> Option<DashboardRecordCursor> {
    records.first().map(|record| DashboardRecordCursor {
        id: record.id,
        timestamp: record.timestamp.clone(),
    })
}

fn count_new_records(
    records: &[DashboardUsageRecord],
    since_timestamp: Option<&str>,
    since_id: Option<i64>,
) -> usize {
    let (Some(since_timestamp), Some(since_id)) = (since_timestamp, since_id) else {
        return 0;
    };

    let mut count = 0;
    for record in records {
        if record.timestamp == since_timestamp && record.id == since_id {
            break;
        }
        count += 1;
    }

    count
}

async fn dashboard_analytics_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response<Body> {
    let window = dashboard_window(params.get("range").map(String::as_str));
    let query = build_dashboard_usage_query(&params, window.current_start, window.current_end);
    let previous_query =
        build_dashboard_usage_query(&params, window.previous_start, window.previous_end);
    let options_query = UsageRecordQuery {
        start_time: Some(format_dashboard_timestamp(window.current_start)),
        end_time: Some(format_dashboard_timestamp(window.current_end)),
        ..UsageRecordQuery::default()
    };

    let current_records = match state.database.get_usage_records_for_analytics(&query) {
        Ok(records) => records,
        Err(err) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(err.to_string()))
                .unwrap();
        }
    };
    let previous_records = match state
        .database
        .get_usage_records_for_analytics(&previous_query)
    {
        Ok(records) => records,
        Err(err) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(err.to_string()))
                .unwrap();
        }
    };
    let option_records = match state
        .database
        .get_usage_records_for_analytics(&options_query)
    {
        Ok(records) => records,
        Err(err) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(err.to_string()))
                .unwrap();
        }
    };

    let trend = build_trend_section(&current_records, &window);
    let response = DashboardAnalyticsResponse {
        generated_at: format_dashboard_timestamp(Local::now().naive_local()),
        range: window.range,
        filter_options: build_filter_options(&option_records),
        overview: build_overview(&current_records, &previous_records),
        topology: build_topology_section(&current_records),
        team_usage: build_team_usage_section(&current_records),
        system_reliability: build_system_reliability_section(&current_records, &trend),
        model_router: build_model_router_section(&current_records),
        records_meta: DashboardRecordsMeta {
            total: current_records.len(),
            latest_cursor: latest_cursor(&current_records),
        },
        trend,
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&response).unwrap_or_default(),
        ))
        .unwrap()
}

async fn dashboard_records_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response<Body> {
    let window = dashboard_window(params.get("range").map(String::as_str));
    let query = build_dashboard_usage_query(&params, window.current_start, window.current_end);
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20)
        .min(100);
    let offset = params
        .get("offset")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let since_timestamp = params.get("since_timestamp").map(String::as_str);
    let since_id = params
        .get("since_id")
        .and_then(|value| value.parse::<i64>().ok());

    match state.database.get_usage_records_for_analytics(&query) {
        Ok(records) => {
            let total = records.len();
            let latest = latest_cursor(&records);
            let new_records = count_new_records(&records, since_timestamp, since_id);
            let data = records
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>();
            let payload = DashboardRecordsResponse {
                data,
                total,
                limit,
                offset,
                latest_cursor: latest,
                new_records,
            };

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&payload).unwrap_or_default(),
                ))
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

pub(crate) fn error_response(status: StatusCode, message: &str) -> Response<Body> {
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

fn protocol_error_response(route: RouteKind, status: StatusCode, message: &str) -> Response<Body> {
    if matches!(route, RouteKind::Anthropic) {
        let body = json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": message,
            }
        });
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    } else {
        error_response(status, message)
    }
}

fn format_error_chain(error: &dyn std::error::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut current = error.source();
    while let Some(source) = current {
        parts.push(source.to_string());
        current = source.source();
    }
    parts.join(" | caused by: ")
}

fn anthropic_request_contains_tool_result(body: &Bytes) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };

    value
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .map(|messages| {
            messages.iter().any(|message| {
                message
                    .get("content")
                    .and_then(serde_json::Value::as_array)
                    .map(|parts| {
                        parts.iter().any(|part| {
                            part.get("type").and_then(serde_json::Value::as_str)
                                == Some("tool_result")
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn summarize_anthropic_request(body: &Bytes) -> String {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return "invalid_json".to_string();
    };
    let Some(messages) = value.get("messages").and_then(serde_json::Value::as_array) else {
        return "messages=missing".to_string();
    };

    let mut referenced_tool_use_ids = Vec::new();
    let mut segments = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let role = message
            .get("role")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let mut kinds = Vec::new();
        if let Some(content) = message.get("content").and_then(serde_json::Value::as_array) {
            for block in content {
                let kind = block
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                kinds.push(kind.to_string());
                if kind == "tool_result"
                    && let Some(tool_use_id) =
                        block.get("tool_use_id").and_then(serde_json::Value::as_str)
                {
                    referenced_tool_use_ids.push(tool_use_id.to_string());
                }
            }
        }
        segments.push(format!("{index}:{role}[{}]", kinds.join(",")));
    }

    format!(
        "messages={} {} tool_result_ids={:?}",
        messages.len(),
        segments.join(" "),
        referenced_tool_use_ids
    )
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
    let bytes = match axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Request Failed: Failed to read body: {}", e);
            return error_response(StatusCode::BAD_REQUEST, &e.to_string());
        }
    };

    // 2. Parse Model
    let model_name = parts
        .extensions
        .get::<OriginalModelName>()
        .map(|model| model.0.clone())
        .or_else(|| {
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|json| {
                    json.get("model")
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string())
                })
        });
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
    let primary_selection = state
        .selector
        .select_channel_with_rule(router, model_name_str);
    let mut matched_rule = primary_selection
        .as_ref()
        .and_then(|selection| selection.matched_rule.clone());

    if let Some(selection) = primary_selection.as_ref()
        && let Some(ch_name) = Some(selection.channel_name.as_str())
        && let Some(ch) = config.channels.iter().find(|c| c.name == ch_name)
    {
        channels.push(ch);
        tracing::info!(
            "Channel Resolved: {} (strategy={}, model={}, matched_rule={})",
            ch.name,
            router.strategy,
            model_name_str,
            selection.matched_rule.as_deref().unwrap_or("n/a")
        );
    } else {
        if matched_rule.is_none() {
            matched_rule = Some("fallback".to_string());
        }
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
            matched_rule.as_deref(),
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

        let effective_bytes = if channel.provider_type == crate::config::ProviderType::Gemini
            && matches!(route, RouteKind::Anthropic)
        {
            state.gemini_replay.augment_request(&team_id, &bytes)
        } else {
            bytes.clone()
        };

        let effective_model = serde_json::from_slice::<serde_json::Value>(&effective_bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("model")
                    .and_then(|model| model.as_str())
                    .map(str::to_string)
            })
            .and_then(|model| {
                channel
                    .model_map
                    .as_ref()
                    .and_then(|map| map.get(&model))
                    .cloned()
                    .or(Some(model))
            });

        if channel.provider_type == crate::config::ProviderType::Gemini
            && matches!(route, RouteKind::Anthropic)
            && effective_model
                .as_deref()
                .is_some_and(|model| model.to_ascii_lowercase().starts_with("gemini-3"))
            && anthropic_request_contains_tool_result(&effective_bytes)
            && gemini_replay_missing_signature(&effective_bytes)
        {
            let reason = format!(
                "Gemini model '{}' requires Google thought_signature data on tool-result follow-up turns. Claude Code did not preserve the prior Gemini tool call state, and Apex could not reconstruct it from cache.",
                effective_model.as_deref().unwrap_or(model_name_str)
            );
            let request_summary = summarize_anthropic_request(&effective_bytes);
            tracing::warn!("Gemini replay rejection summary: {}", request_summary);
            tracing::warn!("Request Rejected: {}", reason);
            state
                .access_audit
                .audit(&channel.provider_type, route, false);
            state
                .metrics
                .error_total
                .with_label_values(&[route_label, &router_name])
                .inc();
            state.database.log_error(route_label, &router_name);
            state.usage_logger.log_failure(
                request_id.as_deref(),
                &team_id,
                &router_name,
                matched_rule.as_deref(),
                &channel.name,
                model_name_str,
                None,
                fallback_triggered,
                StatusCode::BAD_REQUEST.as_u16() as i64,
                &reason,
                None,
                None,
            );
            return protocol_error_response(route, StatusCode::BAD_REQUEST, &reason);
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
                &effective_bytes,
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
                        let mut response = adapter.handle_response(
                            route,
                            resp,
                            Duration::from_millis(config.global.timeouts.response_ms),
                        );
                        if channel.provider_type == crate::config::ProviderType::Gemini
                            && matches!(route, RouteKind::Anthropic)
                        {
                            response = state
                                .gemini_replay
                                .clone()
                                .wrap_response(team_id.clone(), effective_bytes.clone(), response)
                                .await;
                        }
                        return crate::usage::wrap_response(
                            response,
                            request_id.clone(),
                            team_id.clone(),
                            router_name.clone(),
                            matched_rule.clone(),
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
                        if !stored_error_body.is_empty() {
                            tracing::warn!("Upstream Error Body: {}", stored_error_body);
                        }
                        state.usage_logger.log_failure(
                            request_id.as_deref(),
                            &team_id,
                            &router_name,
                            matched_rule.as_deref(),
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
                    let error_chain = format_error_chain(&e);
                    tracing::error!(
                        is_connect = e.is_connect(),
                        is_timeout = e.is_timeout(),
                        is_request = e.is_request(),
                        is_body = e.is_body(),
                        is_decode = e.is_decode(),
                        error_chain = %error_chain,
                        "Upstream Error: {}",
                        e
                    );
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
        matched_rule.as_deref(),
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
    use tempfile::{TempDir, tempdir};

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

    fn create_test_database() -> (TempDir, Arc<Database>) {
        let dir = tempdir().unwrap();
        let db = Arc::new(Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap());
        (dir, db)
    }

    #[test]
    fn gemini_missing_signature_guard_triggers_only_for_tool_result_followups() {
        let followup = Bytes::from(
            serde_json::to_vec(&json!({
                "messages": [
                    {
                        "role": "assistant",
                        "content": [
                            {"type": "tool_use", "id": "toolu_1", "name": "run_command", "input": {"cmd": "pwd"}}
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            {"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"}
                        ]
                    }
                ]
            }))
            .unwrap(),
        );
        assert!(gemini_replay_missing_signature(&followup));

        let first_turn = Bytes::from(
            serde_json::to_vec(&json!({
                "messages": [
                    {
                        "role": "user",
                        "content": [{"type": "text", "text": "hello"}]
                    }
                ]
            }))
            .unwrap(),
        );
        assert!(!gemini_replay_missing_signature(&first_turn));
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks() {
        let config = create_test_config();
        let config_arc = Arc::new(RwLock::new(config));
        let audit_calls = Arc::new(Mutex::new(Vec::new()));

        let (_dir, database) = create_test_database();
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
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
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

        let (_dir, database) = create_test_database();
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
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
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

    #[test]
    fn topology_keeps_same_name_in_different_dimensions_separate() {
        let records = vec![DashboardUsageRecord {
            id: 1,
            timestamp: "2026-03-12 12:00:00".to_string(),
            request_id: Some("req-1".to_string()),
            team_id: "default".to_string(),
            router: "default".to_string(),
            matched_rule: Some("*".to_string()),
            final_channel: "openai".to_string(),
            channel: "openai".to_string(),
            model: "gpt-4o".to_string(),
            input_tokens: 10,
            output_tokens: 20,
            latency_ms: Some(100.0),
            fallback_triggered: false,
            status: "success".to_string(),
            status_code: Some(200),
            error_message: None,
            provider_trace_id: None,
            provider_error_body: None,
        }];

        let topology = build_topology_section(&records);
        let default_nodes = topology
            .nodes
            .iter()
            .filter(|node| node.name == "default")
            .collect::<Vec<_>>();

        assert_eq!(default_nodes.len(), 2);
        assert!(default_nodes.iter().any(|node| node.kind == "team"));
        assert!(default_nodes.iter().any(|node| node.kind == "router"));
    }

    #[test]
    fn topology_links_include_aggregated_total_tokens() {
        let records = vec![
            DashboardUsageRecord {
                id: 1,
                timestamp: "2026-03-12 12:00:00".to_string(),
                request_id: Some("req-1".to_string()),
                team_id: "team-alpha".to_string(),
                router: "default".to_string(),
                matched_rule: Some("*".to_string()),
                final_channel: "openai".to_string(),
                channel: "openai".to_string(),
                model: "gpt-4o".to_string(),
                input_tokens: 10,
                output_tokens: 20,
                latency_ms: Some(100.0),
                fallback_triggered: false,
                status: "success".to_string(),
                status_code: Some(200),
                error_message: None,
                provider_trace_id: None,
                provider_error_body: None,
            },
            DashboardUsageRecord {
                id: 2,
                timestamp: "2026-03-12 12:01:00".to_string(),
                request_id: Some("req-2".to_string()),
                team_id: "team-alpha".to_string(),
                router: "default".to_string(),
                matched_rule: Some("*".to_string()),
                final_channel: "openai".to_string(),
                channel: "openai".to_string(),
                model: "gpt-4o".to_string(),
                input_tokens: 15,
                output_tokens: 25,
                latency_ms: Some(120.0),
                fallback_triggered: false,
                status: "success".to_string(),
                status_code: Some(200),
                error_message: None,
                provider_trace_id: None,
                provider_error_body: None,
            },
        ];

        let topology = build_topology_section(&records);
        let team_link = topology
            .links
            .iter()
            .find(|link| {
                topology.nodes[link.source].name == "team-alpha"
                    && topology.nodes[link.target].name == "default"
            })
            .expect("team -> router link should exist");

        assert_eq!(team_link.value, 2);
        assert_eq!(team_link.total_tokens, 70);
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

        let (_dir, database) = create_test_database();
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
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
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

        let (_dir, database) = create_test_database();
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
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
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
