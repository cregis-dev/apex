//! Dedicated endpoints that reveal masked API keys.
//!
//! Separate from the list endpoints so those can stay secret-free.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use serde_json::json;

use crate::utils::mask_secret;

use super::super::AppState;
use super::super::auth::enforce_global_auth;
use super::super::errors::error_response;

// ---- masked api_key reveal endpoints --------------------------------------
//
// These are dedicated endpoints so the bulk list responses can stay
// secret-free. A separate request makes it easier to:
//   - audit who reads keys vs. who just browses the config,
//   - extend later to require an extra confirmation / scope per key, and
//   - keep the list endpoints cacheable.

pub(crate) async fn handle_admin_teams_api_keys(
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

pub(crate) async fn handle_admin_team_reveal_api_key(
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

pub(crate) async fn handle_admin_channels_api_keys(
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
