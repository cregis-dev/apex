// Every HTTP handler in this module returns a full `Response<Body>`, and the
// admin write path threads that same type through `Result<_, Response<Body>>`
// (including the closures passed to `commit_config`). `result_large_err` is
// fundamentally at odds with that design, so allow it module-wide.
#![allow(clippy::result_large_err)]

use crate::config::Config;
use crate::converters::convert_openai_response_to_anthropic;
use crate::database::{
    Database, UsageAggregate, UsageRecord as DashboardUsageRecord, UsageRecordPage,
    UsageRecordQuery,
};
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
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response as HttpResponse, StatusCode};
use axum::response::{Redirect, Response};
use axum::routing::{delete, get, patch, post};
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
    pub database: Arc<Database>,
    pub web_dir: String,
}

pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 10 * 1024 * 1024;

pub async fn run_server(path: PathBuf) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(&path)?;
    let mut config: Config = serde_json::from_str(&content)?;

    // Store config path for potential hot reload
    config.hot_reload.config_path = path.to_string_lossy().to_string();

    // Refuse to bind if the config still carries the placeholder admin/team
    // keys we ship in install templates — those would be accepted verbatim
    // by the auth middleware. Fail closed so an unfinished setup never goes
    // live on 0.0.0.0:12356.
    crate::config::check_no_placeholder_credentials(&config)?;

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

    // Prune old usage/metrics rows in the background so the SQLite file stays
    // bounded. Runs once shortly after startup, then on a fixed interval.
    if config.retention.days > 0 {
        let db = state.database.clone();
        let retention = config.retention.clone();
        tokio::spawn(async move {
            let period = Duration::from_secs(retention.interval_hours.max(1) * 3600);
            // Delay the first sweep so a large prune on a freshly-started gateway
            // doesn't contend with the startup traffic ramp for the write lock.
            tokio::time::sleep(Duration::from_secs(60)).await;
            let mut ticker = tokio::time::interval(period);
            loop {
                ticker.tick().await;
                let db = db.clone();
                let days = retention.days;
                match tokio::task::spawn_blocking(move || db.cleanup_old_records(days)).await {
                    Ok(Ok(0)) => {}
                    Ok(Ok(n)) => info!(
                        "Retention: pruned {} usage/metrics rows older than {} days",
                        n, days
                    ),
                    Ok(Err(e)) => error!("Retention cleanup failed: {}", e),
                    Err(e) => error!("Retention task panicked: {}", e),
                }
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
                if let Err(e) = crate::config::check_no_placeholder_credentials(&new_config) {
                    error!("Refusing to apply reloaded config: {}", e);
                    continue;
                }
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
    let gemini_replay_ttl = Duration::from_secs(
        config
            .global
            .gemini_replay
            .ttl_hours
            .saturating_mul(60 * 60),
    );
    let usage_logger = Arc::new(UsageLogger::new(database.clone()));
    let web_dir = config.web_dir.clone();
    let config_arc = Arc::new(RwLock::new(config));

    Ok(Arc::new(AppState {
        config: config_arc,
        metrics: Arc::new(MetricsState::new()?),
        providers: Arc::new(ProviderRegistry::new()),
        access_audit: Arc::new(NoOpAccessAudit),
        rate_limiter: Arc::new(NoOpRateLimiter),
        team_rate_limiter: Arc::new(TeamRateLimiter::new()),
        selector: Arc::new(RouterSelector::new()),
        gemini_replay: Arc::new(GeminiAnthropicReplayCache::with_persistence(
            database.clone(),
            gemini_replay_ttl,
        )),
        client,
        usage_logger,
        database,
        web_dir,
    }))
}

pub fn build_app(state: Arc<AppState>) -> Router {
    let config = state.config.read().unwrap();
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

    let gemini_native_routes = Router::new()
        .route("/gemini/*path", get(handle_gemini_native))
        .route("/gemini/*path", post(handle_gemini_native))
        .route("/gemini/*path", delete(handle_gemini_native))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            team_policy,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            team_auth,
        ));

    // Admin/System Routes (no auth required)
    let admin_routes = Router::new()
        .route(
            "/admin/teams",
            get(handle_admin_teams).post(handle_admin_create_team),
        )
        // Order matters: this static path must be registered before the
        // `:team_id` route below so it isn't shadowed by the path-param match.
        .route("/admin/teams/api_keys", get(handle_admin_teams_api_keys))
        .route(
            "/admin/teams/:team_id",
            patch(handle_admin_update_team).delete(handle_admin_delete_team),
        )
        // Explicit single-key reveal: returns the *unmasked* api_key for one team.
        // Separate from the masked bulk list so reveals stay auditable.
        .route(
            "/admin/teams/:team_id/api_key",
            get(handle_admin_team_reveal_api_key),
        )
        .route(
            "/admin/routers",
            get(handle_admin_routers).post(handle_admin_create_router),
        )
        .route(
            "/admin/routers/:router_name",
            patch(handle_admin_update_router).delete(handle_admin_delete_router),
        )
        .route(
            "/admin/channels",
            get(handle_admin_channels).post(handle_admin_create_channel),
        )
        // Static path must be registered before the `:channel_name` path-param.
        .route(
            "/admin/channels/api_keys",
            get(handle_admin_channels_api_keys),
        )
        .route(
            "/admin/channels/:channel_name",
            patch(handle_admin_update_channel).delete(handle_admin_delete_channel),
        )
        .route(
            "/api/cp/provider-templates",
            get(handle_cp_provider_templates),
        )
        .route("/api/cp/info", get(handle_cp_info));

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
    let mut app = model_routes.merge(gemini_native_routes).merge(admin_routes);

    if let Some(metrics) = metrics_routes {
        app = app.merge(metrics);
    }

    // Root landing page (links to the Control Plane UI).
    // The legacy Next.js dashboard (`/dashboard`, `/_next/static/*`) has been
    // retired; the Control Plane at `/cp` is the sole web UI. The shared
    // `/api/dashboard/*` analytics endpoints stay in place — the Control Plane
    // consumes them.
    let root_routes = Router::new()
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

    // Control Plane UI (Vite + React, hash routing)
    // HTML/assets are public so browsers can load the SPA; API endpoints handle their own auth.
    let cp_routes = Router::new()
        .route(
            "/cp",
            get(|OriginalUri(uri): OriginalUri| async move {
                let target = match uri.query() {
                    Some(q) if !q.is_empty() => format!("/cp/?{q}"),
                    _ => "/cp/".to_string(),
                };
                Redirect::permanent(&target)
            }),
        )
        .route(
            "/cp/",
            get(move |State(state): State<Arc<AppState>>| async move {
                serve_web_asset(&state.web_dir, "cp/index.html", "Control plane not found")
            }),
        )
        .route(
            "/cp/favicon.svg",
            get(move |State(state): State<Arc<AppState>>| async move {
                let mut resp = serve_web_asset(&state.web_dir, "cp/favicon.svg", "Not found");
                resp.headers_mut().insert(
                    axum::http::header::CACHE_CONTROL,
                    axum::http::HeaderValue::from_static("public, max-age=86400"),
                );
                resp
            }),
        )
        .route(
            "/cp/assets/*path",
            get(
                move |State(state): State<Arc<AppState>>,
                      axum::extract::Path(path): axum::extract::Path<String>| async move {
                    let mut resp =
                        serve_web_asset(&state.web_dir, &format!("cp/assets/{path}"), "Not found");
                    resp.headers_mut().insert(
                        axum::http::header::CACHE_CONTROL,
                        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
                    );
                    resp
                },
            ),
        );

    app = app.merge(root_routes);
    app = app.merge(cp_routes);

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

async fn serve_index(_state: State<Arc<AppState>>) -> Response<Body> {
    // The legacy Next.js dashboard has been retired. The root page is now a
    // minimal landing page that points at the Control Plane UI (`/cp`). We no
    // longer serve the dashboard's `index.html`, so the old UI stays offline
    // even if its build artifacts remain on disk.
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
        <p><a href="/cp/">Go to Control Plane</a></p>
    </div>
</body>
</html>"#,
        ))
        .unwrap()
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
    clients: Vec<String>,
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
    client_usage: Vec<DashboardShareItem>,
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
        client: normalize_query_filter(params, "client"),
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

fn build_overview(
    current_records: &[DashboardUsageRecord],
    previous: &UsageAggregate,
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

    let previous_requests = previous.requests as f64;
    let previous_tokens = previous.total_tokens as f64;
    let previous_latency = previous.avg_latency_ms;
    let previous_errors = previous.error_count as f64;
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
    leaderboard.truncate(10);

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

/// Per-client (tool) usage breakdown. Records without a detected client are
/// bucketed under "Unknown" (e.g. failed requests, tools that send no UA).
fn build_client_usage_section(records: &[DashboardUsageRecord]) -> Vec<DashboardShareItem> {
    let total_requests = records.len() as f64;
    let mut map: HashMap<String, (i64, i64)> = HashMap::new();
    for record in records {
        let total_tokens = usage_record_total_tokens(record);
        let key = record
            .client
            .clone()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "Unknown".to_string());
        map.entry(key)
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
    }

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
    items.sort_by_key(|item| std::cmp::Reverse(item.requests));
    items
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
        items.sort_by_key(|item| std::cmp::Reverse(item.requests));
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
    flows.sort_by_key(|flow| std::cmp::Reverse(flow.requests));

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
    // Period-over-period deltas only need aggregates of the previous window, and
    // the filter dropdowns only need its distinct values — compute both in SQL
    // instead of loading every row of those windows into memory.
    let previous = match state.database.get_usage_aggregate(&previous_query) {
        Ok(aggregate) => aggregate,
        Err(err) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(err.to_string()))
                .unwrap();
        }
    };
    let filter_options = match state.database.get_filter_options(&options_query) {
        Ok(options) => DashboardFilterOptions {
            teams: options.teams,
            models: options.models,
            routers: options.routers,
            channels: options.channels,
            clients: options.clients,
        },
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
        filter_options,
        overview: build_overview(&current_records, &previous),
        topology: build_topology_section(&current_records),
        team_usage: build_team_usage_section(&current_records),
        system_reliability: build_system_reliability_section(&current_records, &trend),
        model_router: build_model_router_section(&current_records),
        client_usage: build_client_usage_section(&current_records),
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

    match state.database.get_usage_records_page(
        &query,
        limit as i64,
        offset as i64,
        since_timestamp,
        since_id,
    ) {
        Ok(UsageRecordPage {
            records,
            total,
            new_records,
            latest_cursor,
        }) => {
            let payload = DashboardRecordsResponse {
                data: records,
                total: total as usize,
                limit,
                offset,
                latest_cursor: latest_cursor
                    .map(|(id, timestamp)| DashboardRecordCursor { id, timestamp }),
                new_records: new_records as usize,
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

async fn handle_gemini_native(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(path): axum::extract::Path<String>,
    req: Request<Body>,
) -> Response<Body> {
    let route = match validate_gemini_native_route(req.method(), &path) {
        Some(route) => route,
        None => {
            return gemini_native_error_response(
                StatusCode::NOT_FOUND,
                "Gemini native endpoint is not allowlisted",
                "NOT_FOUND",
            );
        }
    };

    let (mut parts, body) = req.into_parts();
    let routing_model = route.routing_model;
    parts
        .extensions
        .insert(OriginalModelName(routing_model.clone()));
    let req = Request::from_parts(parts, body);
    if route.direct_pass {
        return process_gemini_native_direct_pass(state, req, routing_model).await;
    }
    process_request(state, req, RouteKind::GeminiNative, None, None).await
}

/// `GET /v1/models` (and `/models`). Returns the list of concrete model ids
/// the *team* associated with the inbound API key is allowed to call, in
/// OpenAI's list-models format. Admin / global keys are intentionally
/// rejected here — this endpoint exists to bootstrap end-user clients, not
/// to power admin tooling.
async fn handle_models(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response<Body> {
    let team_id = match req.extensions().get::<TeamContext>() {
        Some(ctx) => ctx.team_id.clone(),
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Team API Key required: /v1/models only resolves models for a specific team",
            );
        }
    };

    let config = state.config.read().unwrap().clone();
    let Some(team) = config.teams.iter().find(|t| t.id == team_id) else {
        return error_response(StatusCode::UNAUTHORIZED, "Team not found");
    };

    // -- 1. Collect candidate model ids ------------------------------------
    // Anything that is a literal model name in the rules of a router this
    // team is allowed to use. Glob patterns like "*" / "deepseek-*" are
    // skipped because OpenAI's list-models payload requires concrete ids.
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    for router_name in &team.policy.allowed_routers {
        let Some(router) = config.routers.iter().find(|r| &r.name == router_name) else {
            continue;
        };
        for rule in &router.rules {
            for pattern in &rule.match_spec.models {
                if !is_glob_pattern(pattern) {
                    candidates.insert(pattern.clone());
                }
            }
        }
    }

    // Augment with concrete model ids actually observed in the usage log for
    // this team. This covers the common case where the only router rule is
    // a glob (e.g. `deepseek-*`) — without history the team would otherwise
    // see an empty list.
    if let Ok(history) = state.database.distinct_models_for_team(&team_id) {
        candidates.extend(history);
    }

    // -- 2. Filter by team policy + verify a router can actually route it --
    let mut entries: Vec<serde_json::Value> = Vec::with_capacity(candidates.len());
    for model in candidates {
        if !team.policy.is_model_allowed(&model) {
            continue;
        }

        let Some((router_name, channel_name)) =
            resolve_model_to_router_channel(&config, &team.policy.allowed_routers, &model, &state)
        else {
            continue;
        };

        let owned_by = config
            .channels
            .iter()
            .find(|c| c.name == channel_name)
            .map(|c| format!("{:?}", c.provider_type).to_lowercase())
            .unwrap_or_else(|| "apex".to_string());

        entries.push(json!({
            "id": model,
            "object": "model",
            "created": 0,
            "owned_by": owned_by,
            "apex": {
                "router": router_name,
                "channel": channel_name,
            }
        }));
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "object": "list",
                "data": entries,
            })
            .to_string(),
        ))
        .unwrap()
}

/// A pattern is "glob-like" if it contains any of the meta-characters glob
/// would interpret. Bare literal model ids (which is what OpenAI's list
/// endpoint must return) contain none of these.
fn is_glob_pattern(pattern: &str) -> bool {
    pattern.chars().any(|c| matches!(c, '*' | '?' | '[' | ']'))
}

/// Try each of the team's allowed routers; if one can route the given model
/// to a concrete channel, return `(router_name, channel_name)`. We avoid
/// returning models the gateway would actually 404 on.
fn resolve_model_to_router_channel(
    config: &Config,
    allowed_routers: &[String],
    model: &str,
    state: &AppState,
) -> Option<(String, String)> {
    for router_name in allowed_routers {
        let Some(router) = config.routers.iter().find(|r| &r.name == router_name) else {
            continue;
        };
        if let Some(channel) = state.selector.select_channel(router, model) {
            return Some((router.name.clone(), channel));
        }
    }
    None
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
            // NOTE: api_key is intentionally NOT included in the list
            // response. Fetch it explicitly via GET /admin/teams/api_keys.
            json!({
                "id": team.id,
                "group": team.group,
                "enabled": team.enabled.unwrap_or(true),
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

// -------- Teams CRUD --------

#[derive(serde::Deserialize, Default)]
struct CreateTeamRequest {
    id: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    allowed_routers: Option<Vec<String>>,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
    #[serde(default)]
    rate_limit: Option<TeamRateLimitInput>,
}

#[derive(serde::Deserialize, Default)]
struct UpdateTeamRequest {
    #[serde(default)]
    group: Option<Option<String>>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    allowed_routers: Option<Vec<String>>,
    /// `Some(None)` means "clear" (no allowlist → all models). `Some(Some(_))` sets.
    /// `None` leaves the field unchanged.
    #[serde(default, deserialize_with = "deserialize_optional_optional_vec")]
    allowed_models: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "deserialize_optional_optional_rate_limit")]
    rate_limit: Option<Option<TeamRateLimitInput>>,
}

#[derive(serde::Deserialize, Default, Clone)]
struct TeamRateLimitInput {
    #[serde(default)]
    rpm: Option<i32>,
    #[serde(default)]
    tpm: Option<i32>,
}

fn deserialize_optional_optional_vec<'de, D>(
    deserializer: D,
) -> Result<Option<Option<Vec<String>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    // Accept null (= clear) or array.
    let value: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Some(None)),
        serde_json::Value::Array(items) => {
            let parsed = items
                .into_iter()
                .map(|item| match item {
                    serde_json::Value::String(s) => Ok(s),
                    other => Err(serde::de::Error::custom(format!(
                        "allowed_models entries must be strings, got {other:?}"
                    ))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Some(Some(parsed)))
        }
        other => Err(serde::de::Error::custom(format!(
            "allowed_models must be null or an array, got {other:?}"
        ))),
    }
}

fn deserialize_optional_optional_rate_limit<'de, D>(
    deserializer: D,
) -> Result<Option<Option<TeamRateLimitInput>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let value: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Some(None)),
        other => {
            let parsed: TeamRateLimitInput =
                serde_json::from_value(other).map_err(serde::de::Error::custom)?;
            Ok(Some(Some(parsed)))
        }
    }
}

fn generate_team_api_key() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Hex-encoded timestamp + a small random suffix from process state.
    // Not cryptographically strong but unique per-team for in-config usage.
    let pid = std::process::id() as u128;
    let entropy = nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(pid);
    format!("sk-apex-{entropy:032x}")
}

fn persist_config(config: &Config) -> Result<(), String> {
    let path = PathBuf::from(&config.hot_reload.config_path);
    if path.as_os_str().is_empty() {
        return Err("hot_reload.config_path is empty".into());
    }
    crate::config::save_config(&path, config).map_err(|e| e.to_string())
}

/// Atomically apply a configuration mutation.
///
/// This is the single write path shared by every admin CRUD handler. It closes
/// two classes of bug that arise when validation, mutation and persistence are
/// done across separate lock acquisitions:
///
///   * **TOCTOU** — the closure runs while the write lock is held and validates
///     against a private `candidate` clone of the *current* config, so a
///     concurrent writer can't invalidate a check between validate and apply.
///   * **memory/disk divergence** — the candidate is persisted to disk *before*
///     it is committed to the in-memory `Config`. If the disk write fails, the
///     live config is left untouched and the handler returns an error, instead
///     of silently keeping an unpersisted change that vanishes on restart.
///
/// The closure receives `&mut Config` (the candidate) and returns either a
/// success value (used to build the response) or an error `Response` to abort
/// the whole operation with no change.
///
/// Note: the file write happens while the write lock is held. Admin mutations
/// are rare and the proxy hot-path only takes *read* locks, so the brief stall
/// is an acceptable tradeoff for atomicity.
fn commit_config<T>(
    state: &AppState,
    mutate: impl FnOnce(&mut Config) -> Result<T, Response<Body>>,
) -> Result<T, Response<Body>> {
    let mut guard = state.config.write().unwrap();
    let mut candidate = guard.clone();
    let value = mutate(&mut candidate)?;
    if let Err(err) = persist_config(&candidate) {
        tracing::error!("Failed to persist config change: {err}");
        return Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to persist config: {err}"),
        ));
    }
    *guard = candidate;
    Ok(value)
}

fn teams_json_response(team: &crate::config::Team) -> serde_json::Value {
    let rate_limit = team
        .policy
        .rate_limit
        .as_ref()
        .map(|l| json!({"rpm": l.rpm, "tpm": l.tpm}));
    // NOTE: api_key is intentionally NOT included in the standard response.
    // Create responses overlay it afterwards (only-time reveal); other reads
    // go through GET /admin/teams/api_keys.
    json!({
        "id": team.id,
        "group": team.group,
        "enabled": team.enabled.unwrap_or(true),
        "policy": {
            "allowed_routers": team.policy.allowed_routers,
            "allowed_models": team.policy.allowed_models,
            "rate_limit": rate_limit,
        }
    })
}

async fn handle_admin_create_team(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let config_snapshot = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config_snapshot, &parts.headers) {
        return resp;
    }

    let bytes = match axum::body::to_bytes(body, 64 * 1024).await {
        Ok(b) => b,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Failed to read body"),
    };
    let payload: CreateTeamRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {err}"));
        }
    };

    let id = payload.id.trim().to_string();
    if id.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "id must not be empty");
    }

    let api_key = payload
        .api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .unwrap_or_else(generate_team_api_key);

    let allowed_routers = payload.allowed_routers.unwrap_or_default();
    let rate_limit = payload.rate_limit.map(|r| crate::config::TeamRateLimit {
        rpm: r.rpm,
        tpm: r.tpm,
    });

    let new_team = crate::config::Team {
        id: id.clone(),
        api_key: api_key.clone(),
        group: payload.group.and_then(|g| {
            let g = g.trim().to_string();
            if g.is_empty() { None } else { Some(g) }
        }),
        enabled: payload.enabled.or(Some(true)),
        policy: crate::config::TeamPolicy {
            allowed_routers,
            allowed_models: payload.allowed_models,
            rate_limit,
        },
    };

    // Validate uniqueness + apply + persist atomically under the write lock.
    if let Err(resp) = commit_config(&state, |cfg| {
        if cfg.teams.iter().any(|t| t.id == id) {
            return Err(error_response(
                StatusCode::CONFLICT,
                "A team with this id already exists",
            ));
        }
        if cfg.teams.iter().any(|t| t.api_key == api_key) {
            return Err(error_response(
                StatusCode::CONFLICT,
                "A team with this api_key already exists",
            ));
        }
        Arc::make_mut(&mut cfg.teams).push(new_team.clone());
        Ok(())
    }) {
        return resp;
    }

    // For create only: return the *unmasked* api_key once, so the operator
    // can record it. Subsequent reads will be masked.
    let mut payload_value = teams_json_response(&new_team);
    if let Some(obj) = payload_value.as_object_mut() {
        obj.insert(
            "api_key".to_string(),
            serde_json::Value::String(api_key.clone()),
        );
        obj.insert(
            "api_key_revealed".to_string(),
            serde_json::Value::Bool(true),
        );
    }

    Response::builder()
        .status(StatusCode::CREATED)
        .header("content-type", "application/json")
        .body(Body::from(payload_value.to_string()))
        .unwrap()
}

async fn handle_admin_update_team(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(team_id): axum::extract::Path<String>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let config_snapshot = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config_snapshot, &parts.headers) {
        return resp;
    }

    let bytes = match axum::body::to_bytes(body, 64 * 1024).await {
        Ok(b) => b,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Failed to read body"),
    };
    let payload: UpdateTeamRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {err}"));
        }
    };

    let updated_team = match commit_config(&state, |cfg| {
        let teams = Arc::make_mut(&mut cfg.teams);
        let Some(team) = teams.iter_mut().find(|t| t.id == team_id) else {
            return Err(error_response(StatusCode::NOT_FOUND, "Team not found"));
        };

        if let Some(group) = payload.group {
            team.group = group.and_then(|g| {
                let trimmed = g.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            });
        }
        if let Some(enabled) = payload.enabled {
            team.enabled = Some(enabled);
        }
        if let Some(allowed_routers) = payload.allowed_routers {
            team.policy.allowed_routers = allowed_routers;
        }
        if let Some(allowed_models) = payload.allowed_models {
            team.policy.allowed_models = allowed_models;
        }
        if let Some(rate_limit) = payload.rate_limit {
            team.policy.rate_limit = rate_limit.map(|r| crate::config::TeamRateLimit {
                rpm: r.rpm,
                tpm: r.tpm,
            });
        }

        Ok(team.clone())
    }) {
        Ok(team) => team,
        Err(resp) => return resp,
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(teams_json_response(&updated_team).to_string()))
        .unwrap()
}

async fn handle_admin_delete_team(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(team_id): axum::extract::Path<String>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let config_snapshot = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config_snapshot, &parts.headers) {
        return resp;
    }

    if let Err(resp) = commit_config(&state, |cfg| {
        let teams = Arc::make_mut(&mut cfg.teams);
        let before = teams.len();
        teams.retain(|t| t.id != team_id);
        if teams.len() == before {
            return Err(error_response(StatusCode::NOT_FOUND, "Team not found"));
        }
        Ok(())
    }) {
        return resp;
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(json!({"deleted": team_id}).to_string()))
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
            // NOTE: api_key is intentionally NOT included in the list
            // response. Fetch it explicitly via GET /admin/channels/api_keys.
            json!({
                "name": channel.name,
                "provider_type": channel.provider_type,
                "base_url": channel.base_url,
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

// -------- Channels CRUD --------
//
// Channels write paths share the same shape as Teams: the in-memory `Config`
// is mutated under the write-lock, then `persist_config` writes the new JSON
// to disk so the change survives restart. Hot-reload picks it up too.
//
// Safety invariants:
//   * name uniqueness on create
//   * before delete, refuse if any router rule (or legacy fallback) still
//     references the channel — silent removal would break routing at runtime.

#[derive(serde::Deserialize)]
struct CreateChannelRequest {
    name: String,
    provider_type: crate::config::ProviderType,
    base_url: String,
    api_key: String,
    #[serde(default)]
    anthropic_base_url: Option<String>,
    #[serde(default)]
    headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    model_map: Option<std::collections::HashMap<String, String>>,
}

#[derive(serde::Deserialize, Default)]
struct UpdateChannelRequest {
    #[serde(default)]
    provider_type: Option<crate::config::ProviderType>,
    #[serde(default)]
    base_url: Option<String>,
    /// Bearer token / upstream secret. `None` leaves it unchanged.
    /// (No way to *clear* the key via PATCH — empty key would break the
    /// channel at runtime. Delete + recreate instead.)
    #[serde(default)]
    api_key: Option<String>,
    /// `Some(None)` clears the anthropic URL; `Some(Some(_))` sets it.
    #[serde(default, deserialize_with = "deserialize_optional_optional_string")]
    anthropic_base_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_optional_str_map")]
    headers: Option<Option<std::collections::HashMap<String, String>>>,
    #[serde(default, deserialize_with = "deserialize_optional_optional_str_map")]
    model_map: Option<Option<std::collections::HashMap<String, String>>>,
}

fn deserialize_optional_optional_string<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let value: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Some(None)),
        serde_json::Value::String(s) => Ok(Some(Some(s))),
        other => Err(serde::de::Error::custom(format!(
            "expected string or null, got {other:?}"
        ))),
    }
}

fn deserialize_optional_optional_str_map<'de, D>(
    deserializer: D,
) -> Result<Option<Option<std::collections::HashMap<String, String>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let value: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Some(None)),
        other => {
            let parsed: std::collections::HashMap<String, String> =
                serde_json::from_value(other).map_err(serde::de::Error::custom)?;
            Ok(Some(Some(parsed)))
        }
    }
}

fn channel_json_response(channel: &crate::config::Channel) -> serde_json::Value {
    json!({
        "name": channel.name,
        "provider_type": channel.provider_type,
        "base_url": channel.base_url,
        "anthropic_base_url": channel.anthropic_base_url,
    })
}

/// Collect router names + rule indices that reference the given channel.
/// Used by delete handlers to produce a helpful 409 instead of silently
/// leaving the gateway pointing at a deleted channel.
fn collect_channel_references(config: &Config, channel_name: &str) -> Vec<String> {
    let mut refs = Vec::new();
    for router in config.routers.iter() {
        for (idx, rule) in router.rules.iter().enumerate() {
            if rule.channels.iter().any(|c| c.name == channel_name) {
                refs.push(format!("router '{}' rule #{}", router.name, idx + 1));
            }
        }
        if router.channels.iter().any(|c| c.name == channel_name) {
            refs.push(format!("router '{}' legacy channels", router.name));
        }
        if router.fallback_channels.iter().any(|c| c == channel_name) {
            refs.push(format!("router '{}' fallback", router.name));
        }
    }
    refs
}

async fn handle_admin_create_channel(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let config_snapshot = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config_snapshot, &parts.headers) {
        return resp;
    }

    let bytes = match axum::body::to_bytes(body, 64 * 1024).await {
        Ok(b) => b,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Failed to read body"),
    };
    let payload: CreateChannelRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {err}"));
        }
    };

    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "name must not be empty");
    }
    if payload.base_url.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "base_url must not be empty");
    }
    if payload.api_key.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "api_key must not be empty");
    }

    let new_channel = crate::config::Channel {
        name: name.clone(),
        provider_type: payload.provider_type,
        base_url: payload.base_url.trim().to_string(),
        api_key: payload.api_key,
        anthropic_base_url: payload
            .anthropic_base_url
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        headers: payload.headers,
        model_map: payload.model_map,
        timeouts: None,
    };

    if let Err(resp) = commit_config(&state, |cfg| {
        if cfg.channels.iter().any(|c| c.name == name) {
            return Err(error_response(
                StatusCode::CONFLICT,
                "A channel with this name already exists",
            ));
        }
        Arc::make_mut(&mut cfg.channels).push(new_channel.clone());
        Ok(())
    }) {
        return resp;
    }

    Response::builder()
        .status(StatusCode::CREATED)
        .header("content-type", "application/json")
        .body(Body::from(channel_json_response(&new_channel).to_string()))
        .unwrap()
}

async fn handle_admin_update_channel(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(channel_name): axum::extract::Path<String>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let config_snapshot = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config_snapshot, &parts.headers) {
        return resp;
    }

    let bytes = match axum::body::to_bytes(body, 64 * 1024).await {
        Ok(b) => b,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Failed to read body"),
    };
    let payload: UpdateChannelRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {err}"));
        }
    };

    let snapshot = match commit_config(&state, |cfg| {
        let channels = Arc::make_mut(&mut cfg.channels);
        let Some(channel) = channels.iter_mut().find(|c| c.name == channel_name) else {
            return Err(error_response(StatusCode::NOT_FOUND, "Channel not found"));
        };

        if let Some(pt) = payload.provider_type {
            channel.provider_type = pt;
        }
        if let Some(base_url) = payload.base_url {
            let trimmed = base_url.trim().to_string();
            if trimmed.is_empty() {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "base_url must not be empty",
                ));
            }
            channel.base_url = trimmed;
        }
        if let Some(api_key) = payload.api_key {
            if api_key.trim().is_empty() {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "api_key must not be empty (omit field to keep current)",
                ));
            }
            channel.api_key = api_key;
        }
        if let Some(anthropic) = payload.anthropic_base_url {
            channel.anthropic_base_url = anthropic
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
        }
        if let Some(headers) = payload.headers {
            channel.headers = headers;
        }
        if let Some(model_map) = payload.model_map {
            channel.model_map = model_map;
        }

        Ok(channel.clone())
    }) {
        Ok(snapshot) => snapshot,
        Err(resp) => return resp,
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(channel_json_response(&snapshot).to_string()))
        .unwrap()
}

async fn handle_admin_delete_channel(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(channel_name): axum::extract::Path<String>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let config_snapshot = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config_snapshot, &parts.headers) {
        return resp;
    }

    // Reference check + delete + persist atomically: a concurrent router
    // create/update can't slip a new reference in between check and delete.
    if let Err(resp) = commit_config(&state, |cfg| {
        let refs = collect_channel_references(cfg, &channel_name);
        if !refs.is_empty() {
            return Err(Response::builder()
                .status(StatusCode::CONFLICT)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "error": format!(
                            "Channel '{channel_name}' is still referenced by: {}",
                            refs.join(", ")
                        ),
                        "references": refs,
                    })
                    .to_string(),
                ))
                .unwrap());
        }
        let channels = Arc::make_mut(&mut cfg.channels);
        let before = channels.len();
        channels.retain(|c| c.name != channel_name);
        if channels.len() == before {
            return Err(error_response(StatusCode::NOT_FOUND, "Channel not found"));
        }
        Ok(())
    }) {
        return resp;
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(json!({"deleted": channel_name}).to_string()))
        .unwrap()
}

// -------- Routers CRUD --------
//
// Routers are heavier than channels: a router carries an ordered list of
// `RouterRule` (match patterns + strategy + target channels) plus optional
// fallback channels. The write payload mirrors the on-disk JSON shape so
// the same blob can round-trip through `save_config`.
//
// Safety invariants:
//   * name uniqueness on create
//   * every channel referenced by any rule (or fallback / legacy channels)
//     must already exist
//   * before delete, refuse if any team.allowed_routers still references it

#[derive(serde::Deserialize, Default)]
struct RouterRuleInput {
    #[serde(default)]
    models: Vec<String>,
    #[serde(default)]
    channels: Vec<TargetChannelInput>,
    #[serde(default)]
    strategy: Option<String>,
}

#[derive(serde::Deserialize, Default, Clone)]
struct TargetChannelInput {
    name: String,
    #[serde(default = "default_target_weight")]
    weight: u32,
}

fn default_target_weight() -> u32 {
    1
}

#[derive(serde::Deserialize, Default)]
struct CreateRouterRequest {
    name: String,
    #[serde(default)]
    rules: Vec<RouterRuleInput>,
    #[serde(default)]
    fallback_channels: Vec<String>,
}

#[derive(serde::Deserialize, Default)]
struct UpdateRouterRequest {
    #[serde(default)]
    rules: Option<Vec<RouterRuleInput>>,
    #[serde(default)]
    fallback_channels: Option<Vec<String>>,
}

fn router_json_response(router: &crate::config::Router) -> serde_json::Value {
    serde_json::to_value(router).unwrap_or(serde_json::Value::Null)
}

fn build_rule(input: RouterRuleInput) -> Result<crate::config::RouterRule, String> {
    if input.channels.is_empty() {
        return Err("each rule must have at least one channel".into());
    }
    let strategy = input
        .strategy
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("round_robin")
        .to_string();
    match strategy.as_str() {
        "round_robin" | "random" | "priority" => {}
        other => return Err(format!("unknown strategy '{other}'")),
    }
    let channels = input
        .channels
        .into_iter()
        .map(|c| crate::config::TargetChannel {
            name: c.name,
            weight: c.weight.max(1),
        })
        .collect();
    Ok(crate::config::RouterRule {
        match_spec: crate::config::MatchSpec {
            models: input.models,
        },
        channels,
        strategy,
    })
}

/// Verify every channel referenced by a router exists. Returns the list of
/// missing channel names (empty if all OK).
fn missing_channels(
    config: &Config,
    rules: &[crate::config::RouterRule],
    fallback: &[String],
) -> Vec<String> {
    let known: std::collections::HashSet<&str> =
        config.channels.iter().map(|c| c.name.as_str()).collect();
    let mut missing = std::collections::BTreeSet::new();
    for rule in rules {
        for tc in &rule.channels {
            if !known.contains(tc.name.as_str()) {
                missing.insert(tc.name.clone());
            }
        }
    }
    for name in fallback {
        if !known.contains(name.as_str()) {
            missing.insert(name.clone());
        }
    }
    missing.into_iter().collect()
}

async fn handle_admin_create_router(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let config_snapshot = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config_snapshot, &parts.headers) {
        return resp;
    }

    let bytes = match axum::body::to_bytes(body, 256 * 1024).await {
        Ok(b) => b,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Failed to read body"),
    };
    let payload: CreateRouterRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {err}"));
        }
    };

    let name = payload.name.trim().to_string();
    if name.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "name must not be empty");
    }
    if payload.rules.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "rules must contain at least one rule",
        );
    }

    let mut built_rules = Vec::with_capacity(payload.rules.len());
    for rule in payload.rules {
        match build_rule(rule) {
            Ok(r) => built_rules.push(r),
            Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
        }
    }

    let new_router = crate::config::Router {
        name: name.clone(),
        rules: built_rules,
        channels: vec![],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: payload.fallback_channels,
    };

    // Name uniqueness + channel-existence validation + persist, all atomic.
    if let Err(resp) = commit_config(&state, |cfg| {
        if cfg.routers.iter().any(|r| r.name == name) {
            return Err(error_response(
                StatusCode::CONFLICT,
                "A router with this name already exists",
            ));
        }
        let missing = missing_channels(cfg, &new_router.rules, &new_router.fallback_channels);
        if !missing.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                &format!("Unknown channels: {}", missing.join(", ")),
            ));
        }
        Arc::make_mut(&mut cfg.routers).push(new_router.clone());
        Ok(())
    }) {
        return resp;
    }

    Response::builder()
        .status(StatusCode::CREATED)
        .header("content-type", "application/json")
        .body(Body::from(router_json_response(&new_router).to_string()))
        .unwrap()
}

async fn handle_admin_update_router(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(router_name): axum::extract::Path<String>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let config_snapshot = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config_snapshot, &parts.headers) {
        return resp;
    }

    let bytes = match axum::body::to_bytes(body, 256 * 1024).await {
        Ok(b) => b,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Failed to read body"),
    };
    let payload: UpdateRouterRequest = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {err}"));
        }
    };

    let mut built_rules = None;
    if let Some(rules) = payload.rules {
        if rules.is_empty() {
            return error_response(
                StatusCode::BAD_REQUEST,
                "rules must contain at least one rule",
            );
        }
        let mut tmp = Vec::with_capacity(rules.len());
        for rule in rules {
            match build_rule(rule) {
                Ok(r) => tmp.push(r),
                Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
            }
        }
        built_rules = Some(tmp);
    }

    let snapshot = match commit_config(&state, |cfg| {
        // Compute the resulting rules/fallback first, then validate channel
        // references against the *live* config before mutating.
        let (final_rules, final_fallback) = {
            let Some(router) = cfg.routers.iter().find(|r| r.name == router_name) else {
                return Err(error_response(StatusCode::NOT_FOUND, "Router not found"));
            };
            let final_rules = built_rules.clone().unwrap_or_else(|| router.rules.clone());
            let final_fallback = payload
                .fallback_channels
                .clone()
                .unwrap_or_else(|| router.fallback_channels.clone());
            (final_rules, final_fallback)
        };

        let missing = missing_channels(cfg, &final_rules, &final_fallback);
        if !missing.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                &format!("Unknown channels: {}", missing.join(", ")),
            ));
        }

        let routers = Arc::make_mut(&mut cfg.routers);
        let router = routers
            .iter_mut()
            .find(|r| r.name == router_name)
            .expect("router existence already checked above under the same lock");
        if let Some(rules) = built_rules {
            router.rules = rules;
        }
        if let Some(fallback) = payload.fallback_channels {
            router.fallback_channels = fallback;
        }
        Ok(router.clone())
    }) {
        Ok(snapshot) => snapshot,
        Err(resp) => return resp,
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(router_json_response(&snapshot).to_string()))
        .unwrap()
}

async fn handle_admin_delete_router(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(router_name): axum::extract::Path<String>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let config_snapshot = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config_snapshot, &parts.headers) {
        return resp;
    }

    if let Err(resp) = commit_config(&state, |cfg| {
        let referring_teams: Vec<String> = cfg
            .teams
            .iter()
            .filter(|t| t.policy.allowed_routers.iter().any(|r| r == &router_name))
            .map(|t| t.id.clone())
            .collect();
        if !referring_teams.is_empty() {
            return Err(Response::builder()
                .status(StatusCode::CONFLICT)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "error": format!(
                            "Router '{router_name}' is still in allowed_routers of: {}",
                            referring_teams.join(", ")
                        ),
                        "references": referring_teams,
                    })
                    .to_string(),
                ))
                .unwrap());
        }
        let routers = Arc::make_mut(&mut cfg.routers);
        let before = routers.len();
        routers.retain(|r| r.name != router_name);
        if routers.len() == before {
            return Err(error_response(StatusCode::NOT_FOUND, "Router not found"));
        }
        Ok(())
    }) {
        return resp;
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(json!({"deleted": router_name}).to_string()))
        .unwrap()
}

// ---- masked api_key reveal endpoints --------------------------------------
//
// These are dedicated endpoints so the bulk list responses can stay
// secret-free. A separate request makes it easier to:
//   - audit who reads keys vs. who just browses the config,
//   - extend later to require an extra confirmation / scope per key, and
//   - keep the list endpoints cacheable.

async fn handle_admin_teams_api_keys(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let config = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config, &parts.headers) {
        return resp;
    }

    let data = config
        .teams
        .iter()
        .map(|team| {
            json!({
                "id": team.id,
                "api_key": mask_secret(&team.api_key),
            })
        })
        .collect::<Vec<_>>();

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "object": "list", "data": data }).to_string(),
        ))
        .unwrap()
}

async fn handle_admin_team_reveal_api_key(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(team_id): axum::extract::Path<String>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let config = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config, &parts.headers) {
        return resp;
    }

    match config.teams.iter().find(|team| team.id == team_id) {
        Some(team) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({ "id": team.id, "api_key": team.api_key }).to_string(),
            ))
            .unwrap(),
        None => error_response(StatusCode::NOT_FOUND, "Team not found"),
    }
}

async fn handle_admin_channels_api_keys(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let config = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config, &parts.headers) {
        return resp;
    }

    let data = config
        .channels
        .iter()
        .map(|channel| {
            json!({
                "name": channel.name,
                "api_key": mask_secret(&channel.api_key),
            })
        })
        .collect::<Vec<_>>();

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "object": "list", "data": data }).to_string(),
        ))
        .unwrap()
}

// providers.json embedded at build time so the provider-templates endpoint
// always has data even when the file isn't present in the process CWD (e.g.
// installed deployments). A runtime providers.json, when present, takes
// precedence so operators can customize the default catalog.
const EMBEDDED_PROVIDERS_JSON: &str = include_str!("../providers.json");

#[derive(serde::Deserialize)]
struct CpProviderFile {
    #[serde(default)]
    provider_templates: Vec<CpProviderTemplate>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct CpProviderTemplate {
    provider_type: String,
    base_url: String,
    #[serde(default)]
    anthropic_base_url: Option<String>,
}

fn load_cp_provider_templates() -> Vec<CpProviderTemplate> {
    // Prefer a runtime providers.json next to the working directory.
    if let Ok(cwd) = std::env::current_dir() {
        let path = cwd.join("providers.json");
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Ok(file) = serde_json::from_str::<CpProviderFile>(&content)
        {
            return file.provider_templates;
        }
    }
    // Fall back to the catalog embedded at build time.
    serde_json::from_str::<CpProviderFile>(EMBEDDED_PROVIDERS_JSON)
        .map(|f| f.provider_templates)
        .unwrap_or_default()
}

/// `GET /api/cp/provider-templates` — the default base_url / anthropic_base_url
/// per provider type, used by the control plane to pre-fill the channel form.
async fn handle_cp_provider_templates(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let config = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config, &parts.headers) {
        return resp;
    }

    let data = load_cp_provider_templates();
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "object": "list", "data": data }).to_string(),
        ))
        .unwrap()
}

async fn handle_cp_info(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let config = state.config.read().unwrap().clone();

    if let Err(resp) = enforce_global_auth(&config, &parts.headers) {
        return resp;
    }

    let info = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "listen": config.global.listen,
        "auth_required": !config.global.auth_keys.is_empty(),
        "auth_key_count": config.global.auth_keys.len(),
        "cors_origins": config.global.cors_allowed_origins,
        "timeouts": {
            "connect_ms": config.global.timeouts.connect_ms,
            "request_ms": config.global.timeouts.request_ms,
            "response_ms": config.global.timeouts.response_ms,
        },
        "retries": {
            "max_attempts": config.global.retries.max_attempts,
            "backoff_ms": config.global.retries.backoff_ms,
        },
        "channels": config.channels.len(),
        "routers": config.routers.len(),
        "teams": config.teams.len(),
        "metrics_enabled": config.metrics.enabled,
        "hot_reload": config.hot_reload.watch,
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(info.to_string()))
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

struct GeminiNativeRoute {
    routing_model: String,
    direct_pass: bool,
}

fn validate_gemini_native_route(method: &Method, path: &str) -> Option<GeminiNativeRoute> {
    let path = path.trim_start_matches('/');
    let segments = path.split('/').collect::<Vec<_>>();

    match (method, segments.as_slice()) {
        (&Method::GET, ["v1beta", "models"]) => Some(gemini_native_resource_route()),
        (&Method::GET, ["v1beta", "models", model]) if !model.contains(':') => {
            Some(gemini_native_model_route(model))
        }
        (&Method::POST, ["v1beta", "models", model_action])
            if model_action.ends_with(":generateContent")
                || model_action.ends_with(":streamGenerateContent") =>
        {
            model_action
                .split_once(':')
                .filter(|(model, _)| !model.is_empty())
                .map(|(model, _)| gemini_native_model_route(model))
        }
        (&Method::GET, ["v1beta", "fileSearchStores"])
        | (&Method::POST, ["v1beta", "fileSearchStores"]) => Some(gemini_native_resource_route()),
        (&Method::GET, ["v1beta", "fileSearchStores", store])
        | (&Method::DELETE, ["v1beta", "fileSearchStores", store])
            if !store.contains(':') =>
        {
            Some(gemini_native_resource_route())
        }
        (&Method::POST, ["v1beta", "fileSearchStores", store_action])
        | (&Method::POST, ["upload", "v1beta", "fileSearchStores", store_action])
            if store_action.ends_with(":uploadToFileSearchStore") =>
        {
            Some(GeminiNativeRoute {
                routing_model: "gemini-native".to_string(),
                direct_pass: true,
            })
        }
        (&Method::GET, ["v1beta", "fileSearchStores", store, "operations", operation])
        | (
            &Method::GET,
            [
                "v1beta",
                "fileSearchStores",
                store,
                "upload",
                "operations",
                operation,
            ],
        ) if !store.is_empty() && !operation.is_empty() => Some(gemini_native_resource_route()),
        (&Method::POST, ["v1beta", "interactions"]) => Some(gemini_native_direct_pass_route()),
        (&Method::GET, ["v1beta", "interactions", interaction]) if !interaction.is_empty() => {
            Some(gemini_native_direct_pass_route())
        }
        _ => None,
    }
}

fn gemini_native_resource_route() -> GeminiNativeRoute {
    GeminiNativeRoute {
        routing_model: "gemini-native".to_string(),
        direct_pass: false,
    }
}

fn gemini_native_direct_pass_route() -> GeminiNativeRoute {
    GeminiNativeRoute {
        routing_model: "gemini-native".to_string(),
        direct_pass: true,
    }
}

fn gemini_native_model_route(model: &str) -> GeminiNativeRoute {
    GeminiNativeRoute {
        routing_model: model.to_string(),
        direct_pass: false,
    }
}

fn gemini_native_resource_router_is_deterministic(
    router: &crate::config::Router,
    model: &str,
) -> bool {
    router.rules.iter().any(|rule| {
        crate::config::TeamPolicy {
            allowed_routers: Vec::new(),
            allowed_models: Some(rule.match_spec.models.clone()),
            rate_limit: None,
        }
        .is_model_allowed(model)
            && rule.strategy == "priority"
            && rule.channels.len() == 1
    })
}

fn gemini_native_error_response(
    status: StatusCode,
    message: &str,
    gemini_status: &str,
) -> Response<Body> {
    let body = json!({
        "error": {
            "code": status.as_u16(),
            "message": message,
            "status": gemini_status,
        }
    });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn protocol_error_response(route: RouteKind, status: StatusCode, message: &str) -> Response<Body> {
    if matches!(route, RouteKind::GeminiNative) {
        let gemini_status = match status {
            StatusCode::NOT_FOUND => "NOT_FOUND",
            StatusCode::FORBIDDEN => "PERMISSION_DENIED",
            StatusCode::UNAUTHORIZED => "UNAUTHENTICATED",
            StatusCode::TOO_MANY_REQUESTS => "RESOURCE_EXHAUSTED",
            StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE => "UNAVAILABLE",
            _ => "INVALID_ARGUMENT",
        };
        gemini_native_error_response(status, message, gemini_status)
    } else if matches!(route, RouteKind::Anthropic) {
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
    let client_info = crate::utils::classify_client(&parts.headers);

    // 1. Read Body
    let bytes = match axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Request Failed: Failed to read body: {}", e);
            return protocol_error_response(route, StatusCode::BAD_REQUEST, &e.to_string());
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
            return protocol_error_response(route, StatusCode::UNAUTHORIZED, "Team not found");
        }
        let team = team.unwrap();

        // Check Allowed Models
        let policy = &team.policy;
        if !policy.is_model_allowed(model_name_str) {
            tracing::warn!(
                "Policy Failed: Model '{}' not allowed by team policy",
                model_name_str
            );
            return protocol_error_response(
                route,
                StatusCode::FORBIDDEN,
                "Model not allowed by team policy",
            );
        }

        // Check Allowed Routers (Mandatory)
        let allowed_routers = &policy.allowed_routers;
        if allowed_routers.is_empty() {
            tracing::warn!(
                "Policy Failed: No allowed routers configured for team '{}'",
                ctx.team_id
            );
            return protocol_error_response(
                route,
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
                return protocol_error_response(
                    route,
                    StatusCode::NOT_FOUND,
                    "No matching router found for model in allowed routers",
                );
            }
        }
    } else {
        // Global Auth Flow (Legacy/Admin)
        if let Err(resp) = enforce_global_auth(&config, &headers) {
            return if matches!(route, RouteKind::GeminiNative) {
                protocol_error_response(route, StatusCode::UNAUTHORIZED, "unauthorized")
            } else {
                resp
            };
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
                return protocol_error_response(
                    route,
                    StatusCode::BAD_REQUEST,
                    "No matching router found for model",
                );
            }
        }
    };

    let Some(router) = config.routers.iter().find(|r| r.name == router_name) else {
        return protocol_error_response(route, StatusCode::NOT_FOUND, "router not found");
    };
    if matches!(route, RouteKind::GeminiNative)
        && model_name_str == "gemini-native"
        && !gemini_native_resource_router_is_deterministic(router, model_name_str)
    {
        return protocol_error_response(
            route,
            StatusCode::BAD_REQUEST,
            "Gemini native resource routes require a priority router rule with exactly one channel",
        );
    }

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
            &client_info,
        );
        return protocol_error_response(
            route,
            StatusCode::BAD_GATEWAY,
            "no channels configured or matched",
        );
    }

    let route_label = match route {
        RouteKind::Openai => "openai",
        RouteKind::Anthropic => "anthropic",
        RouteKind::GeminiNative => "gemini_native",
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

    // Extract path and query for preparation
    let path = path_override.unwrap_or_else(|| parts.uri.path().to_string());
    let query = parts.uri.query().map(|s| s.to_string());
    let is_gemini_native_upload = matches!(route, RouteKind::GeminiNative)
        && (path.contains(":uploadToFileSearchStore") || path.starts_with("/gemini/upload/"));
    let max_attempts = if is_gemini_native_upload {
        1
    } else {
        config.global.retries.max_attempts.max(1)
    };

    let mut index = 0;
    let mut fallback_triggered = false;

    while index < channels.len() {
        let channel = channels[index];
        tracing::Span::current().record("channel_name", &channel.name);

        if matches!(route, RouteKind::GeminiNative)
            && channel.provider_type != crate::config::ProviderType::Gemini
        {
            let message = format!(
                "Gemini native route resolved to non-Gemini channel '{}'",
                channel.name
            );
            tracing::warn!("Request Rejected: {}", message);
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
                StatusCode::BAD_GATEWAY.as_u16() as i64,
                &message,
                None,
                None,
                &client_info,
            );
            return protocol_error_response(route, StatusCode::BAD_GATEWAY, &message);
        }

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
                &client_info,
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
                    return protocol_error_response(route, StatusCode::BAD_REQUEST, &e.to_string());
                }
            };

            let adapter = state.providers.adapter_for(channel, route);

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
                    return protocol_error_response(
                        route,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &e.to_string(),
                    );
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
                            client_info.clone(),
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
                            &client_info,
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
        &client_info,
    );

    protocol_error_response(route, StatusCode::BAD_GATEWAY, "all channels failed")
}

async fn process_gemini_native_direct_pass(
    state: Arc<AppState>,
    req: Request<Body>,
    routing_model: String,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let route = RouteKind::GeminiNative;
    let route_label = "gemini_native";
    let request_id = request_id_from_parts(&parts);
    let team_id = parts
        .extensions
        .get::<crate::middleware::auth::TeamContext>()
        .map(|ctx| ctx.team_id.clone())
        .unwrap_or_else(|| "global".to_string());
    let headers = parts.headers.clone();
    let client_info = crate::utils::classify_client(&headers);
    let config = state.config.read().unwrap().clone();

    let router_name = if let Some(ctx) = parts.extensions.get::<TeamContext>() {
        let Some(team) = config.teams.iter().find(|team| team.id == ctx.team_id) else {
            return protocol_error_response(route, StatusCode::UNAUTHORIZED, "Team not found");
        };

        if !team.policy.is_model_allowed(&routing_model) {
            return protocol_error_response(
                route,
                StatusCode::FORBIDDEN,
                "Model not allowed by team policy",
            );
        }

        team.policy
            .allowed_routers
            .iter()
            .find(|router_name| {
                config
                    .routers
                    .iter()
                    .find(|router| router.name == **router_name)
                    .and_then(|router| state.selector.select_channel(router, &routing_model))
                    .is_some()
            })
            .cloned()
    } else {
        if let Err(_resp) = enforce_global_auth(&config, &headers) {
            return protocol_error_response(route, StatusCode::UNAUTHORIZED, "unauthorized");
        }
        config
            .routers
            .iter()
            .find(|router| {
                state
                    .selector
                    .select_channel(router, &routing_model)
                    .is_some()
            })
            .map(|router| router.name.clone())
    };

    let Some(router_name) = router_name else {
        return protocol_error_response(
            route,
            StatusCode::NOT_FOUND,
            "No matching router found for model",
        );
    };
    let Some(router) = config
        .routers
        .iter()
        .find(|router| router.name == router_name)
    else {
        return protocol_error_response(route, StatusCode::NOT_FOUND, "router not found");
    };
    if !gemini_native_resource_router_is_deterministic(router, &routing_model) {
        return protocol_error_response(
            route,
            StatusCode::BAD_REQUEST,
            "Gemini native resource routes require a priority router rule with exactly one channel",
        );
    }
    let Some(selection) = state
        .selector
        .select_channel_with_rule(router, &routing_model)
    else {
        return protocol_error_response(
            route,
            StatusCode::BAD_GATEWAY,
            "no channels configured or matched",
        );
    };
    let matched_rule = selection.matched_rule.clone();
    let Some(channel) = config
        .channels
        .iter()
        .find(|channel| channel.name == selection.channel_name)
    else {
        return protocol_error_response(
            route,
            StatusCode::BAD_GATEWAY,
            "no channels configured or matched",
        );
    };

    if channel.provider_type != crate::config::ProviderType::Gemini {
        let message = format!(
            "Gemini native route resolved to non-Gemini channel '{}'",
            channel.name
        );
        state.usage_logger.log_failure(
            request_id.as_deref(),
            &team_id,
            &router_name,
            matched_rule.as_deref(),
            &channel.name,
            &routing_model,
            None,
            false,
            StatusCode::BAD_GATEWAY.as_u16() as i64,
            &message,
            None,
            None,
            &client_info,
        );
        return protocol_error_response(route, StatusCode::BAD_GATEWAY, &message);
    }

    state
        .metrics
        .request_total
        .with_label_values(&[route_label, &router_name])
        .inc();
    state.database.log_request(route_label, &router_name);

    if !state.rate_limiter.check(&channel.provider_type) {
        return protocol_error_response(route, StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }

    let path = parts.uri.path().to_string();
    let query = parts.uri.query().map(|value| value.to_string());
    let prepared = match crate::providers::prepare_gemini_native_request(
        channel,
        &channel.base_url,
        &path,
        query.as_deref(),
        &headers,
        &Bytes::new(),
    ) {
        Ok(prepared) => prepared,
        Err(err) => {
            return protocol_error_response(route, StatusCode::BAD_REQUEST, &err.to_string());
        }
    };

    let start = std::time::Instant::now();
    let reqwest_body = reqwest::Body::wrap_stream(body.into_data_stream());
    let req_built = match state
        .client
        .request(parts.method.clone(), prepared.url)
        .headers(prepared.headers)
        .body(reqwest_body)
        .build()
    {
        Ok(request) => request,
        Err(err) => {
            return protocol_error_response(
                route,
                StatusCode::INTERNAL_SERVER_ERROR,
                &err.to_string(),
            );
        }
    };

    let resp = match state.client.execute(req_built).await {
        Ok(resp) => resp,
        Err(err) => {
            let message = format_error_chain(&err);
            state.usage_logger.log_failure(
                request_id.as_deref(),
                &team_id,
                &router_name,
                matched_rule.as_deref(),
                &channel.name,
                &routing_model,
                None,
                false,
                StatusCode::BAD_GATEWAY.as_u16() as i64,
                &message,
                None,
                None,
                &client_info,
            );
            return protocol_error_response(route, StatusCode::BAD_GATEWAY, &message);
        }
    };

    let elapsed = start.elapsed().as_millis() as f64;
    state
        .metrics
        .upstream_latency_ms
        .with_label_values(&[route_label, &router_name, &channel.name])
        .observe(elapsed);
    state
        .database
        .log_latency(route_label, &router_name, &channel.name, elapsed);

    let status = resp.status();
    if !status.is_success() {
        state.database.log_error(route_label, &router_name);
        let provider_trace_id = provider_trace_id_from_headers(resp.headers());
        let response_headers = resp.headers().clone();
        let error_body_bytes = resp.bytes().await.unwrap_or_default();
        let stored_error_body =
            truncate_for_storage(&String::from_utf8_lossy(&error_body_bytes), 4000);
        state.usage_logger.log_failure(
            request_id.as_deref(),
            &team_id,
            &router_name,
            matched_rule.as_deref(),
            &channel.name,
            &routing_model,
            Some(elapsed),
            false,
            status.as_u16() as i64,
            status
                .canonical_reason()
                .unwrap_or("upstream request failed"),
            provider_trace_id.as_deref(),
            Some(stored_error_body.as_str()),
            &client_info,
        );
        return response_from_upstream_bytes(status, &response_headers, error_body_bytes);
    }

    state
        .access_audit
        .audit(&channel.provider_type, route, true);
    let adapter = state.providers.adapter_for(channel, route);
    let response = adapter.handle_response(
        route,
        resp,
        Duration::from_millis(config.global.timeouts.response_ms),
    );
    crate::usage::wrap_response(
        response,
        request_id,
        team_id,
        router_name,
        matched_rule,
        channel.name.clone(),
        routing_model,
        state.usage_logger.clone(),
        state.metrics.clone(),
        Some(elapsed),
        false,
        client_info.clone(),
    )
    .await
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
                gemini_replay: crate::config::GeminiReplay::default(),
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
            compliance: None,
            retention: Default::default(),
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
            client: None,
            user_agent: None,
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
                client: None,
                user_agent: None,
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
                client: None,
                user_agent: None,
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

    #[test]
    fn team_usage_leaderboard_is_capped_at_top_ten() {
        let records = (0..12)
            .map(|index| DashboardUsageRecord {
                id: index + 1,
                timestamp: "2026-03-12 12:00:00".to_string(),
                request_id: Some(format!("req-{index}")),
                team_id: format!("team-{index:02}"),
                router: "default".to_string(),
                matched_rule: Some("*".to_string()),
                final_channel: "openai".to_string(),
                channel: "openai".to_string(),
                model: "gpt-4o".to_string(),
                input_tokens: 10,
                output_tokens: 100 - index,
                latency_ms: Some(100.0),
                fallback_triggered: false,
                status: "success".to_string(),
                status_code: Some(200),
                error_message: None,
                provider_trace_id: None,
                provider_error_body: None,
                client: None,
                user_agent: None,
            })
            .collect::<Vec<_>>();

        let team_usage = build_team_usage_section(&records);

        assert_eq!(team_usage.leaderboard.len(), 10);
        assert_eq!(
            team_usage
                .leaderboard
                .first()
                .map(|item| item.team_id.as_str()),
            Some("team-00")
        );
        assert_eq!(
            team_usage
                .leaderboard
                .last()
                .map(|item| item.team_id.as_str()),
            Some("team-09")
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
        let config_arc = Arc::new(RwLock::new(config));

        let (_dir, database) = create_test_database();
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
            group: None,
            enabled: None,
        });

        let config_arc = Arc::new(RwLock::new(config));

        let (_dir, database) = create_test_database();
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

    // ----- /v1/models -------------------------------------------------

    fn build_models_state(
        rules: Vec<crate::config::RouterRule>,
        allowed_models: Option<Vec<String>>,
    ) -> (Arc<AppState>, TempDir) {
        let mut config = create_test_config();

        Arc::make_mut(&mut config.routers).clear();
        Arc::make_mut(&mut config.routers).push(crate::config::Router {
            name: "test-router".to_string(),
            rules,
            channels: vec![],
            strategy: "round_robin".to_string(),
            metadata: None,
            fallback_channels: vec![],
        });

        Arc::make_mut(&mut config.teams).push(crate::config::Team {
            id: "test-team".to_string(),
            api_key: "sk-ap-test".to_string(),
            policy: crate::config::TeamPolicy {
                allowed_routers: vec!["test-router".to_string()],
                allowed_models,
                rate_limit: None,
            },
            group: None,
            enabled: None,
        });

        let (dir, database) = create_test_database();
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
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            database,
            web_dir: "target/web".to_string(),
        });
        (state, dir)
    }

    async fn fetch_models(
        state: Arc<AppState>,
        team_ctx: Option<TeamContext>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method("GET").uri("/v1/models");
        if let Some(ctx) = team_ctx {
            builder = builder.extension(ctx);
        }
        let req = builder.body(Body::empty()).unwrap();
        let resp = handle_models(State(state), req).await;
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, body)
    }

    fn rule_matching(models: &[&str]) -> crate::config::RouterRule {
        crate::config::RouterRule {
            match_spec: crate::config::MatchSpec {
                models: models.iter().map(|s| s.to_string()).collect(),
            },
            channels: vec![crate::config::TargetChannel {
                name: "test-channel".to_string(),
                weight: 1,
            }],
            strategy: "round_robin".to_string(),
        }
    }

    fn ids_in(body: &serde_json::Value) -> Vec<String> {
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn handle_models_requires_team_context() {
        let (state, _dir) = build_models_state(vec![rule_matching(&["*"])], None);
        let (status, _) = fetch_models(state, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn handle_models_lists_literal_models_from_router_rules() {
        let (state, _dir) = build_models_state(
            vec![rule_matching(&["deepseek-v4-pro", "deepseek-v4-flash"])],
            None,
        );
        let (status, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let ids = ids_in(&body);
        assert!(ids.contains(&"deepseek-v4-pro".to_string()));
        assert!(ids.contains(&"deepseek-v4-flash".to_string()));
    }

    #[tokio::test]
    async fn handle_models_skips_pure_glob_patterns() {
        let (state, _dir) = build_models_state(
            vec![rule_matching(&["*", "deepseek-*", "claude-3-haiku"])],
            None,
        );
        let (_, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        let ids = ids_in(&body);
        assert!(ids.contains(&"claude-3-haiku".to_string()));
        // glob patterns must never leak into the OpenAI list payload
        assert!(!ids.iter().any(|id| id.contains('*')));
        assert!(!ids.iter().any(|id| id.contains('?')));
    }

    #[tokio::test]
    async fn handle_models_includes_history_distinct_models() {
        let (state, _dir) = build_models_state(vec![rule_matching(&["*"])], None);
        state.database.log_usage(
            Some("req-1"),
            "test-team",
            "test-router",
            Some("*"),
            "test-channel",
            "gpt-4o-mini", // observed in traffic
            10,
            5,
            Some(120.0),
            false,
            "success",
            Some(200),
            None,
            None,
            None,
            None,
            None,
        );

        let (status, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let ids = ids_in(&body);
        // log_usage lower-cases the model name; the canonical OpenAI id is
        // whatever the gateway has actually seen, so that's what we return.
        assert!(ids.contains(&"gpt-4o-mini".to_string()));
    }

    #[tokio::test]
    async fn handle_models_respects_team_allowed_models() {
        let (state, _dir) = build_models_state(
            vec![rule_matching(&["gpt-4", "gpt-4o", "claude-3-haiku"])],
            Some(vec!["gpt-4*".to_string()]),
        );

        let (_, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        let ids = ids_in(&body);
        assert!(ids.contains(&"gpt-4".to_string()));
        assert!(ids.contains(&"gpt-4o".to_string()));
        // claude-3-haiku exists in the router rule but is filtered out by
        // the team's allowed_models glob.
        assert!(!ids.contains(&"claude-3-haiku".to_string()));
    }

    #[tokio::test]
    async fn handle_models_marks_owned_by_with_provider_type() {
        let (state, _dir) = build_models_state(vec![rule_matching(&["gpt-4"])], None);
        let (_, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        let entry = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "gpt-4")
            .unwrap();
        assert_eq!(entry["owned_by"], "openai"); // test-channel's provider_type
        assert_eq!(entry["apex"]["router"], "test-router");
        assert_eq!(entry["apex"]["channel"], "test-channel");
    }

    // ----- commit_config atomicity ------------------------------------

    #[test]
    fn commit_config_persists_and_commits_on_success() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("config.json");
        let mut config = create_test_config();
        config.hot_reload.config_path = cfg_path.to_string_lossy().to_string();
        let (state, _db_dir) = state_with_config(config);

        let result = commit_config(&state, |cfg| {
            Arc::make_mut(&mut cfg.channels).push(crate::config::Channel {
                name: "added".to_string(),
                provider_type: ProviderType::Openai,
                base_url: "http://x".to_string(),
                api_key: "sk-x".to_string(),
                anthropic_base_url: None,
                headers: None,
                model_map: None,
                timeouts: None,
            });
            Ok::<_, Response<Body>>(())
        });
        assert!(result.is_ok());

        // In-memory committed
        assert!(
            state
                .config
                .read()
                .unwrap()
                .channels
                .iter()
                .any(|c| c.name == "added")
        );
        // Disk persisted
        let on_disk = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(on_disk.contains("\"added\""));
    }

    #[test]
    fn commit_config_does_not_commit_when_persist_fails() {
        let mut config = create_test_config();
        // Empty path makes persist_config fail deterministically.
        config.hot_reload.config_path = String::new();
        let before = config.channels.len();
        let (state, _db_dir) = state_with_config(config);

        let result = commit_config(&state, |cfg| {
            Arc::make_mut(&mut cfg.channels).push(crate::config::Channel {
                name: "ghost".to_string(),
                provider_type: ProviderType::Openai,
                base_url: "http://x".to_string(),
                api_key: "sk-x".to_string(),
                anthropic_base_url: None,
                headers: None,
                model_map: None,
                timeouts: None,
            });
            Ok::<_, Response<Body>>(())
        });
        assert!(result.is_err(), "persist failure should surface as Err");

        // In-memory MUST be untouched — no divergence from disk.
        let after = state.config.read().unwrap().channels.len();
        assert_eq!(after, before);
        assert!(
            !state
                .config
                .read()
                .unwrap()
                .channels
                .iter()
                .any(|c| c.name == "ghost")
        );
    }

    #[test]
    fn commit_config_aborts_without_change_when_closure_errors() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("config.json");
        let mut config = create_test_config();
        config.hot_reload.config_path = cfg_path.to_string_lossy().to_string();
        let before = config.channels.len();
        let (state, _db_dir) = state_with_config(config);

        let result = commit_config(&state, |cfg| {
            Arc::make_mut(&mut cfg.channels).clear();
            Err::<(), _>(error_response(StatusCode::CONFLICT, "nope"))
        });
        assert!(result.is_err());
        // Closure error => no persist, no commit.
        assert_eq!(state.config.read().unwrap().channels.len(), before);
        assert!(
            !cfg_path.exists(),
            "must not have written config on closure error"
        );
    }

    fn state_with_config(config: Config) -> (Arc<AppState>, TempDir) {
        let (dir, database) = create_test_database();
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
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            database,
            web_dir: "target/web".to_string(),
        });
        (state, dir)
    }
}
