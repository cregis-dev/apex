use crate::server::AppState;
use axum::{
    extract::{Request, State},
    http::HeaderMap,
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

    let team_id = {
        let config = state.config.read().unwrap();
        // 1. Extract API Key
        // If no key, proceed (downstream might require it or allow generic access)
        if let Some(api_key) = extract_api_key(&headers) {
            // 2. Find Team
            config
                .teams
                .iter()
                .find(|t| t.api_key == api_key)
                .map(|t| t.id.clone())
        } else {
            None
        }
    };

    if let Some(id) = team_id {
        // Inject Team Context into Request Extensions
        req.extensions_mut().insert(TeamContext {
            team_id: id.clone(),
        });

        // Record in tracing span
        tracing::Span::current().record("team_id", &id);
    }

    next.run(req).await
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
