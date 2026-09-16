//! Admin CRUD for teams (multi-tenant API keys, routing policy, rate limits).

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use serde_json::json;

use super::super::AppState;
use super::super::auth::enforce_global_auth;
use super::super::config_store::commit_config;
use super::super::errors::error_response;

pub(crate) async fn handle_admin_teams(
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

pub(crate) async fn handle_admin_create_team(
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

pub(crate) async fn handle_admin_update_team(
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

pub(crate) async fn handle_admin_delete_team(
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
