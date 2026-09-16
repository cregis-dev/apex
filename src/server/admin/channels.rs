//! Admin CRUD for channels (upstream provider connections).

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

pub(crate) async fn handle_admin_channels(
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
                "anthropic_base_url": channel.anthropic_base_url,
                "model_map": channel.model_map,
                "pricing": channel.pricing
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
    /// Name of the pricing rule this channel bills under. Omit / `null` ⇒ untracked.
    #[serde(default)]
    pricing: Option<String>,
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
    /// `None` leaves the pricing rule unchanged; `Some("")` clears it (untracked);
    /// `Some(name)` sets it. The name must be an existing pricing rule.
    #[serde(default)]
    pricing: Option<String>,
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
        "model_map": channel.model_map,
        "pricing": channel.pricing,
    })
}

/// A channel's `pricing` must name an existing rule (empty string clears it).
/// Validates against the candidate config so it runs inside `commit_config`.
fn validate_channel_pricing(cfg: &Config, name: Option<&str>) -> Result<(), Response<Body>> {
    if let Some(rule_name) = name
        && !rule_name.is_empty()
    {
        let exists = cfg
            .pricing
            .as_ref()
            .is_some_and(|p| p.rule(rule_name).is_some());
        if !exists {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                &format!("pricing rule '{rule_name}' does not exist"),
            ));
        }
    }
    Ok(())
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

pub(crate) async fn handle_admin_create_channel(
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
        pricing: payload.pricing.filter(|v| !v.is_empty()),
    };

    if let Err(resp) = commit_config(&state, |cfg| {
        if cfg.channels.iter().any(|c| c.name == name) {
            return Err(error_response(
                StatusCode::CONFLICT,
                "A channel with this name already exists",
            ));
        }
        validate_channel_pricing(cfg, new_channel.pricing.as_deref())?;
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

pub(crate) async fn handle_admin_update_channel(
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
        // Validate the pricing rule exists before taking the mutable channel borrow.
        validate_channel_pricing(cfg, payload.pricing.as_deref())?;
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
        if let Some(pricing) = payload.pricing {
            channel.pricing = if pricing.is_empty() {
                None
            } else {
                Some(pricing)
            };
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

pub(crate) async fn handle_admin_delete_channel(
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
