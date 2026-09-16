//! Endpoints the control-plane SPA calls that are not entity CRUD: the
//! provider template catalog, pricing config, gateway info, and the live log
//! stream.
//!
//! Deliberately a flat module rather than `cp/mod.rs`: the embedded
//! `providers.json` is pulled in with a path relative to this source file, and
//! nesting would silently change that depth.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use serde_json::json;

use super::AppState;
use super::auth::enforce_global_auth;
use super::config_store::commit_config;
use super::errors::error_response;

// providers.json embedded at build time so the provider-templates endpoint
// always has data even when the file isn't present in the process CWD (e.g.
// installed deployments). A runtime providers.json, when present, takes
// precedence so operators can customize the default catalog.
const EMBEDDED_PROVIDERS_JSON: &str = include_str!("../../providers.json");

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
pub(super) async fn handle_cp_provider_templates(
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

/// Validate a global pricing table: positive `unit`; every rule has a non-empty,
/// unique name and a valid `type`; all rates finite and non-negative; subscription
/// rules have a sane `monthly_fee` and `billing_day`.
fn validate_pricing(pricing: &crate::config::Pricing) -> Result<(), Response<Body>> {
    if !pricing.unit.is_finite() || pricing.unit <= 0.0 {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "pricing.unit must be a number > 0",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for r in &pricing.rules {
        let name = r.name.trim();
        if name.is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                "each pricing rule needs a non-empty name",
            ));
        }
        if !seen.insert(name.to_string()) {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                &format!("duplicate pricing rule name: {name}"),
            ));
        }
        if r.kind != "payg" && r.kind != "subscription" {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                &format!("rule '{name}': type must be 'payg' or 'subscription'"),
            ));
        }
        // PAYG rate-card rows: each needs a valid `match` and non-negative rates.
        for p in &r.prices {
            if p.match_pattern.trim().is_empty() {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("rule '{name}': each price row needs a non-empty match"),
                ));
            }
            if glob::Pattern::new(&p.match_pattern).is_err() {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("rule '{name}': invalid match pattern '{}'", p.match_pattern),
                ));
            }
            for (label, value) in [
                ("input", p.input),
                ("output", p.output),
                ("cache_read", p.cache_read.unwrap_or(0.0)),
                ("cache_write", p.cache_write.unwrap_or(0.0)),
            ] {
                if !value.is_finite() || value < 0.0 {
                    return Err(error_response(
                        StatusCode::BAD_REQUEST,
                        &format!("rule '{name}': rate '{label}' must be a number >= 0"),
                    ));
                }
            }
        }
        if r.is_subscription() {
            if !r.monthly_fee.is_finite() || r.monthly_fee < 0.0 {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("rule '{name}': monthly_fee must be a number >= 0"),
                ));
            }
            if !(1..=31).contains(&r.billing_day) {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("rule '{name}': billing_day must be between 1 and 31"),
                ));
            }
        }
    }
    Ok(())
}

/// GET /admin/pricing — the model reference-price table. Returns an empty default
/// table when none is configured so the editor always has something to render.
pub(super) async fn handle_admin_get_pricing(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, _body) = req.into_parts();
    let config = state.config.read().unwrap().clone();
    if let Err(resp) = enforce_global_auth(&config, &parts.headers) {
        return resp;
    }
    let pricing = config
        .pricing
        .clone()
        .unwrap_or_else(|| crate::config::Pricing {
            currency: "USD".to_string(),
            unit: 1_000_000.0,
            rules: Vec::new(),
        });
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&pricing).unwrap_or_default(),
        ))
        .unwrap()
}

/// PUT /admin/pricing — replace the model reference-price table wholesale.
/// Applied live: `commit_config` swaps the in-memory config after persisting.
pub(super) async fn handle_admin_put_pricing(
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
    let pricing: crate::config::Pricing = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(err) => {
            return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {err}"));
        }
    };
    if let Err(resp) = validate_pricing(&pricing) {
        return resp;
    }
    if let Err(resp) = commit_config(&state, |cfg| {
        cfg.pricing = Some(pricing.clone());
        Ok(())
    }) {
        return resp;
    }
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&pricing).unwrap_or_default(),
        ))
        .unwrap()
}

pub(super) async fn handle_cp_info(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
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
        // Drives the control plane's conditional Governance nav (hidden when off).
        "profiling_enabled": config.profiling.as_ref().map(|p| p.enabled).unwrap_or(false),
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(info.to_string()))
        .unwrap()
}

#[derive(serde::Deserialize)]
pub(super) struct LogStreamQuery {
    /// Minimum severity to deliver (`TRACE`..`ERROR`). Note this narrows what
    /// the *global* `EnvFilter` already let through — it cannot raise verbosity
    /// beyond `logging.level` / `RUST_LOG`.
    level: Option<String>,
    /// Resume cursor: only entries with a greater `seq` are replayed.
    after_seq: Option<u64>,
    /// Backlog size to replay before switching to live push.
    limit: Option<usize>,
}

/// Server-sent stream of the gateway's own logs — the control plane's
/// equivalent of `apex logs`, but sourced from the in-process ring buffer so it
/// works regardless of how the gateway was started (see src/log_stream.rs).
///
/// Replays the recent backlog first, then pushes live entries as they arrive.
pub(super) async fn handle_cp_logs_stream(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<LogStreamQuery>,
    req: Request<Body>,
) -> Response<Body> {
    use axum::response::IntoResponse;
    use axum::response::sse::{Event, KeepAlive, Sse};

    let (parts, _body) = req.into_parts();
    let config = state.config.read().unwrap().clone();

    if let Err(resp) = enforce_global_auth(&config, &parts.headers) {
        return resp;
    }

    let logs = crate::log_stream::global();
    let min_level = query.level.as_deref().map(str::to_uppercase);
    let limit = query.limit.unwrap_or(500).min(2_000);
    let backlog = logs.recent(limit, min_level.as_deref(), query.after_seq);
    let receiver = logs.subscribe();

    fn to_event(entry: &crate::log_stream::LogEntry) -> Event {
        Event::default()
            .json_data(entry)
            .unwrap_or_else(|_| Event::default().data("{}"))
    }

    let live_filter = min_level.clone();
    let live = futures::stream::unfold(receiver, move |mut receiver| {
        let min_level = live_filter.clone();
        async move {
            loop {
                match receiver.recv().await {
                    Ok(entry) => {
                        if !entry.at_or_above(min_level.as_deref()) {
                            continue;
                        }
                        return Some((Ok(to_event(&entry)), receiver));
                    }
                    // This viewer fell behind; tell it how much it missed
                    // rather than dropping the gap silently.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                        let notice = crate::log_stream::LogEntry::lagged(dropped);
                        return Some((Ok(to_event(&notice)), receiver));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        }
    });

    let backlog = futures::stream::iter(
        backlog
            .into_iter()
            .map(|entry| Ok::<Event, std::convert::Infallible>(to_event(&entry)))
            .collect::<Vec<_>>(),
    );

    Sse::new(futures::StreamExt::chain(backlog, live))
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::test_fixtures::*;

    #[test]
    fn validate_pricing_checks_names_types_and_rates() {
        let ok = pricing_of(vec![
            payg_rule("gpt", 2.5, 10.0, Some(1.25)),
            sub_rule("plan", 200.0, None),
        ]);
        assert!(validate_pricing(&ok).is_ok());

        let mut bad_unit = ok.clone();
        bad_unit.unit = 0.0;
        assert!(validate_pricing(&bad_unit).is_err());

        let dup = pricing_of(vec![
            payg_rule("x", 1.0, 1.0, None),
            payg_rule("x", 2.0, 2.0, None),
        ]);
        assert!(validate_pricing(&dup).is_err());

        let neg = pricing_of(vec![payg_rule("x", -1.0, 0.0, None)]);
        assert!(validate_pricing(&neg).is_err());

        let empty_name = pricing_of(vec![payg_rule("  ", 1.0, 1.0, None)]);
        assert!(validate_pricing(&empty_name).is_err());

        let mut bad_day = sub_rule("s", 20.0, None);
        bad_day.billing_day = 40;
        assert!(validate_pricing(&pricing_of(vec![bad_day])).is_err());

        let bad_kind = crate::config::PricingRule {
            kind: "weird".to_string(),
            ..payg_rule("k", 1.0, 1.0, None)
        };
        assert!(validate_pricing(&pricing_of(vec![bad_kind])).is_err());
    }
}
