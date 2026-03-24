use crate::compliance::{PiiProcessor, process_json_content};
use crate::server::{AppState, MAX_REQUEST_BODY_BYTES, error_response};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{Method, StatusCode, header::CONTENT_LENGTH},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct OriginalModelName(pub String);

pub async fn compliance_middleware(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    if !matches!(*req.method(), Method::POST | Method::PUT | Method::PATCH) {
        return next.run(req).await;
    }

    let compliance = {
        let config = state.config.read().unwrap();
        config.compliance.clone()
    };

    let Some(compliance) = compliance else {
        return next.run(req).await;
    };

    if !compliance.enabled {
        return next.run(req).await;
    }

    let processor = PiiProcessor::new(&Some(compliance));
    let (mut parts, body) = req.into_parts();

    let bytes = match axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!(
                "Request Failed: Failed to read body in compliance middleware: {}",
                err
            );
            return error_response(StatusCode::BAD_REQUEST, &err.to_string());
        }
    };

    let body_str = String::from_utf8_lossy(&bytes);

    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes)
        && let Some(model) = json.get("model").and_then(|value| value.as_str())
    {
        parts
            .extensions
            .insert(OriginalModelName(model.to_string()));
    }

    if let Some(detection) = processor.should_block(&body_str) {
        tracing::warn!(
            "Request Blocked: PII detected (rule={}, action=block)",
            detection.rule_name
        );
        return error_response(
            StatusCode::FORBIDDEN,
            &format!("Request blocked: {} detected", detection.rule_name),
        );
    }

    let (processed_body, detections) = process_json_content(&processor, &body_str);

    if !detections.is_empty() {
        tracing::info!(
            "PII Masking Applied: {} detections in request",
            detections.len()
        );
    }

    parts.headers.remove(CONTENT_LENGTH);

    let req = Request::from_parts(parts, Body::from(processed_body));
    next.run(req).await
}
