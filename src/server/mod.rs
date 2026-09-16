// Every HTTP handler in this module returns a full `Response<Body>`, and the
// admin write path threads that same type through `Result<_, Response<Body>>`
// (including the closures passed to `commit_config`). `result_large_err` is
// fundamentally at odds with that design, so allow it module-wide.
#![allow(clippy::result_large_err)]

mod admin;
mod api;
mod auth;
mod config_store;
mod cp;
mod dashboard;
mod errors;
mod request_utils;
#[cfg(test)]
mod test_fixtures;

use admin::{
    handle_admin_channels, handle_admin_channels_api_keys, handle_admin_create_channel,
    handle_admin_create_router, handle_admin_create_team, handle_admin_delete_channel,
    handle_admin_delete_router, handle_admin_delete_team, handle_admin_routers,
    handle_admin_team_reveal_api_key, handle_admin_teams, handle_admin_teams_api_keys,
    handle_admin_update_channel, handle_admin_update_router, handle_admin_update_team,
};
use api::{metrics_api_handler, rankings_api_handler, trends_api_handler, usage_api_handler};
use auth::enforce_global_auth;
use cp::{
    handle_admin_get_pricing, handle_admin_put_pricing, handle_cp_info, handle_cp_logs_stream,
    handle_cp_provider_templates,
};
use dashboard::{dashboard_analytics_api_handler, dashboard_records_api_handler};
pub(crate) use errors::error_response;
use errors::{format_error_chain, gemini_native_error_response, protocol_error_response};
use request_utils::{
    anthropic_request_contains_tool_result, provider_trace_id_from_headers, request_id_from_parts,
    response_from_upstream_bytes, response_timeouts_for, summarize_anthropic_request,
    truncate_for_storage,
};

use crate::config::Config;
use crate::converters::convert_openai_response_to_anthropic;
use crate::database::Database;
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
use axum::http::{HeaderValue, Method, Request, StatusCode};
use axum::response::{Redirect, Response};
use axum::routing::{delete, get, patch, post};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::json;
use std::collections::BTreeSet;
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

    // Behavior-profiling rollup: keep the hourly pre-aggregation fresh so the
    // detection layer can read baselines cheaply instead of re-scanning raw
    // rows. Backfills once on first run, then refreshes a trailing window and
    // prunes stale buckets on a fixed interval. Only runs when profiling is on.
    if let Some(profiling) = config.profiling.clone()
        && profiling.enabled
    {
        let db = state.database.clone();
        tokio::spawn(async move {
            let rollup = profiling.rollup;
            let period = Duration::from_secs(rollup.interval_minutes.max(1) * 60);

            // One-time backfill (no-op if the rollup already has rows).
            {
                let db = db.clone();
                match tokio::task::spawn_blocking(move || db.backfill_rollup_if_empty()).await {
                    Ok(Ok(0)) => {}
                    Ok(Ok(n)) => info!("Rollup: backfilled {} buckets from history", n),
                    Ok(Err(e)) => error!("Rollup backfill failed: {}", e),
                    Err(e) => error!("Rollup backfill task panicked: {}", e),
                }
            }

            let mut ticker = tokio::time::interval(period);
            loop {
                ticker.tick().await;
                let db = db.clone();
                let lookback = rollup.lookback_hours;
                let retention = rollup.retention_days;
                match tokio::task::spawn_blocking(move || {
                    let refreshed = db.rollup_usage(lookback)?;
                    let pruned = db.prune_rollup(retention)?;
                    anyhow::Ok((refreshed, pruned))
                })
                .await
                {
                    Ok(Ok((refreshed, pruned))) => {
                        if refreshed > 0 || pruned > 0 {
                            info!(
                                "Rollup: refreshed {} buckets, pruned {} stale",
                                refreshed, pruned
                            );
                        }
                    }
                    Ok(Err(e)) => error!("Rollup refresh failed: {}", e),
                    Err(e) => error!("Rollup task panicked: {}", e),
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
            "/admin/pricing",
            get(handle_admin_get_pricing).put(handle_admin_put_pricing),
        )
        .route(
            "/api/cp/provider-templates",
            get(handle_cp_provider_templates),
        )
        .route("/api/cp/info", get(handle_cp_info))
        // Live log stream for the control plane's Logs view. Handler enforces
        // the global auth key itself (like `/api/cp/info`) so it stays
        // available even when the metrics route group is disabled.
        .route("/api/cp/logs/stream", get(handle_cp_logs_stream));

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

// Helpers

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

    // Fingerprint the request payload for repeat/abuse detection — hash only,
    // never prompt text (see docs/design/behavior-profiling.md §3.2). Gated by
    // config; `None` ⇒ `usage_records.req_hash` stays NULL and detection skips it.
    let req_hash = config
        .profiling
        .as_ref()
        .filter(|profiling| profiling.enabled && profiling.hash_requests)
        .and_then(|_| crate::request_hash::request_hash(&bytes));

    // Conversation fingerprint — the stable prefix (`system` + first message)
    // that identifies a multi-turn conversation. Computed for every request so
    // usage rows can be grouped by session regardless of routing config; session
    // affinity reuses this same value when a matched rule opts in (see below).
    let session_key = crate::request_hash::session_key(&bytes);

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
    //
    // The matched rule yields an *ordered* candidate list (primary first, then
    // in-rule failovers). The retry loop below walks it, only reaching for the
    // router's `fallback_channels` once every candidate has failed. When any rule
    // opts into session affinity we derive a conversation-stable key so a
    // multi-turn conversation keeps hitting the same channel (prompt-cache
    // alignment); the key is computed only when needed.
    let mut channels: Vec<&crate::config::Channel> = Vec::new();
    let sticky_key = if router.rules.iter().any(|rule| rule.session_affinity) {
        session_key.clone()
    } else {
        None
    };
    let candidates =
        state
            .selector
            .select_candidates(router, model_name_str, sticky_key.as_deref());
    let mut matched_rule = candidates
        .as_ref()
        .and_then(|selection| selection.matched_rule.clone());

    if let Some(selection) = candidates.as_ref() {
        for name in &selection.channels {
            if let Some(ch) = config.channels.iter().find(|c| c.name == *name) {
                if !channels.iter().any(|c| c.name == ch.name) {
                    channels.push(ch);
                }
            } else {
                tracing::warn!("Rule channel not found: {}", name);
            }
        }
        tracing::info!(
            "Channels Resolved: [{}] (model={}, matched_rule={}, sticky={})",
            selection.channels.join(", "),
            model_name_str,
            selection.matched_rule.as_deref().unwrap_or("n/a"),
            sticky_key.is_some()
        );
    }

    if channels.is_empty() {
        // No rule matched (or none of its channels exist) — resolve directly to
        // the router's fallback channels.
        if matched_rule.is_none() {
            matched_rule = Some("fallback".to_string());
        }
        tracing::info!(
            "Fallback Triggered: No rule matched for model '{}' or its channels are missing. Trying fallback channels.",
            model_name_str
        );

        for fb_name in &router.fallback_channels {
            if let Some(channel) = config.channels.iter().find(|c| c.name == *fb_name) {
                tracing::info!("Channel Resolved (Fallback): {}", channel.name);
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
            session_key.as_deref(),
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
                session_key.as_deref(),
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
                session_key.as_deref(),
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
                        let mut response = adapter.handle_response(
                            route,
                            resp,
                            response_timeouts_for(&config.global.timeouts, channel),
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
                        let wrapped = crate::usage::wrap_response(
                            response,
                            route,
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
                            req_hash.clone(),
                            session_key.clone(),
                        )
                        .await;
                        if crate::usage::is_upstream_body_error_response(&wrapped) {
                            tracing::warn!(
                                "Upstream response body failed: channel={} attempt={}/{}",
                                channel.name,
                                attempt + 1,
                                max_attempts
                            );
                            state
                                .access_audit
                                .audit(&channel.provider_type, route, false);
                            if attempt + 1 < max_attempts
                                && crate::usage::is_upstream_body_error_retryable(&wrapped)
                            {
                                tokio::time::sleep(Duration::from_millis(
                                    config.global.retries.backoff_ms,
                                ))
                                .await;
                                continue;
                            }
                            if index == channels.len() - 1
                                && !fallback_triggered
                                && !router.fallback_channels.is_empty()
                            {
                                tracing::warn!(
                                    "Upstream body failed: Channel '{}' failed, trying fallback...",
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
                                break;
                            }
                            if index == channels.len() - 1 {
                                return wrapped;
                            }
                            break;
                        }
                        state
                            .access_audit
                            .audit(&channel.provider_type, route, true);
                        return wrapped;
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
                            session_key.as_deref(),
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
        session_key.as_deref(),
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
            None,
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
                None,
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
            None,
        );
        return response_from_upstream_bytes(status, &response_headers, error_body_bytes);
    }

    let adapter = state.providers.adapter_for(channel, route);
    let response = adapter.handle_response(
        route,
        resp,
        response_timeouts_for(&config.global.timeouts, channel),
    );
    let wrapped = crate::usage::wrap_response(
        response,
        route,
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
        // Native-Gemini direct-pass streams the request body without buffering
        // it, so there is nothing to fingerprint here; these rows get NULL for
        // both the request fingerprint and the conversation/session key.
        None,
        None,
    )
    .await;
    state.access_audit.audit(
        &channel.provider_type,
        route,
        !crate::usage::is_upstream_body_error_response(&wrapped),
    );
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Channel, ProviderType};

    use crate::server::test_fixtures::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

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
            pricing: None,
        });

        // Update router to match "gpt-4" to "ch2"
        let router = &mut Arc::make_mut(&mut config.routers)[0];
        router.rules.insert(
            0,
            crate::config::RouterRule {
                session_affinity: false,
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
            session_affinity: false,
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
            0,
            0,
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
}
