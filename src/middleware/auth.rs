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
    let (api_key_opt, source_opt) = extract_api_key_with_source(&headers);

    let team_id = if let Some(api_key) = api_key_opt {
        let config = state.config.read().unwrap();
        // 1. Check Teams
        if let Some(team) = config.teams.iter().find(|t| t.api_key == api_key) {
            Some(team.id.clone())
        } else {
            // 2. Check Global Keys (if any)
            let is_global = if let Some(global_keys) = &config.global.auth.keys {
                global_keys.contains(&api_key)
            } else {
                false
            };

            if is_global {
                // Valid Global Key -> Pass
                None
            } else {
                // 3. Invalid Key -> Reject
                let source = source_opt.unwrap_or_else(|| "unknown".to_string());
                tracing::warn!(
                    "Auth Failed: Invalid API Key '{}' provided in {}",
                    api_key,
                    source
                );
                return Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"error": "Invalid API Key"}"#))
                    .unwrap();
            }
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
