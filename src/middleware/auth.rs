use crate::server::AppState;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct TeamContext {
    pub team_id: String,
}

pub async fn team_auth(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let headers = req.headers().clone();
    let (mut api_key_opt, mut source_opt) = extract_api_key_with_source(&headers);

    // If not found in headers, try query parameter (common for SSE)
    if api_key_opt.is_none()
        && let Some(query) = req.uri().query()
    {
        for pair in query.split('&') {
            if let Some((key, value)) = pair.split_once('=')
                && (key == "api_key" || key == "auth_token")
            {
                api_key_opt = Some(value.to_string());
                source_opt = Some("Query Parameter (auth_token)".to_string());
                break;
            }
        }
    }

    let team_id = if let Some(api_key) = api_key_opt {
        let config = state.config.read().unwrap();
        // 1. Check Teams
        if let Some(team) = config.teams.iter().find(|t| t.api_key == api_key) {
            Some(team.id.clone())
        } else {
            // 2. Invalid Key -> Reject (Global keys are NOT allowed for model requests)
            let source = source_opt.unwrap_or_else(|| "unknown".to_string());
            tracing::warn!(
                "Auth Failed: Invalid Team API Key '{}' provided in {}",
                api_key,
                source
            );
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"error": "Invalid Team API Key"}"#))
                .unwrap();
        }
    } else {
        None
    };

    if let Some(id) = team_id {
        // Inject Team Context into Request Extensions
        req.extensions_mut().insert(TeamContext {
            team_id: id.clone(),
        });

        // Record in tracing span
        tracing::Span::current().record("team_id", &id);
        tracing::info!("Team Resolved: {}", id);
    }

    next.run(req).await
}

pub async fn global_auth(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let api_key_opt = extract_api_key(headers);

    // If not found in headers, try query parameter (common for SSE/Metrics)
    let mut api_key = api_key_opt;
    if api_key.is_none()
        && let Some(query) = req.uri().query()
    {
        for pair in query.split('&') {
            if let Some((key, value)) = pair.split_once('=')
                && (key == "api_key" || key == "token" || key == "auth_token")
            {
                api_key = Some(value.to_string());
                break;
            }
        }
    }

    let auth_keys = {
        let config = state.config.read().unwrap();
        config.global.auth_keys.clone()
    };

    // If no auth_keys configured, skip validation
    if auth_keys.is_empty() {
        return next.run(req).await;
    }

    // Check if provided key is authorized
    let authorized = api_key.map(|k| auth_keys.contains(&k)).unwrap_or(false);

    if authorized {
        next.run(req).await
    } else {
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"error": "Unauthorized: Global Access Required"}"#,
            ))
            .unwrap()
    }
}

fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    // Try Authorization: Bearer <token>
    if let Some(auth_val) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(stripped) = auth_val.strip_prefix("Bearer ") {
            return Some(stripped.to_string());
        }
        return Some(auth_val.to_string());
    }

    // Try x-api-key (Anthropic style)
    if let Some(key_val) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(key_val.to_string());
    }

    None
}

fn extract_api_key_with_source(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    // Try Authorization: Bearer <token>
    if let Some(auth_val) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(stripped) = auth_val.strip_prefix("Bearer ") {
            return (
                Some(stripped.to_string()),
                Some("Authorization (Bearer)".to_string()),
            );
        }
        return (
            Some(auth_val.to_string()),
            Some("Authorization".to_string()),
        );
    }

    // Try x-api-key (Anthropic style)
    if let Some(key_val) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return (Some(key_val.to_string()), Some("x-api-key".to_string()));
    }

    (None, None)
}
