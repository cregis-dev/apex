//! Admin CRUD for routers (model-match rules onto target channels).

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use serde_json::json;

use crate::config::Config;

use super::super::AppState;
use super::super::auth::enforce_global_auth;
use super::super::config_store::commit_config;
use super::super::errors::error_response;

pub(crate) async fn handle_admin_routers(
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
    #[serde(default)]
    session_affinity: bool,
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
        session_affinity: input.session_affinity,
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

pub(crate) async fn handle_admin_create_router(
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

pub(crate) async fn handle_admin_update_router(
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

pub(crate) async fn handle_admin_delete_router(
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
