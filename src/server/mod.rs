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
mod gemini_route;
mod pipeline;
mod proxy;
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
use cp::{
    handle_admin_get_pricing, handle_admin_put_pricing, handle_cp_info, handle_cp_logs_stream,
    handle_cp_provider_templates,
};
use dashboard::{dashboard_analytics_api_handler, dashboard_records_api_handler};
pub(crate) use errors::error_response;
use proxy::{handle_anthropic, handle_gemini_native, handle_models, handle_openai};

use crate::config::Config;
use crate::database::Database;
use crate::gemini_compat::GeminiAnthropicReplayCache;
use crate::metrics::MetricsState;
use crate::middleware::auth::{global_auth, team_auth};
use crate::middleware::compliance::compliance_middleware;
use crate::middleware::policy::team_policy;
use crate::middleware::ratelimit::TeamRateLimiter;
use crate::providers::{
    AccessAudit, NoOpAccessAudit, NoOpRateLimiter, ProviderRegistry, RateLimiter,
};
use crate::router_selector::RouterSelector;
use crate::usage::UsageLogger;
use crate::web_assets::{WebAssetError, load_web_asset};
use axum::Router;
use axum::body::Body;
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::{Redirect, Response};
use axum::routing::{delete, get, patch, post};
use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
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

#[cfg(test)]
mod tests {}
