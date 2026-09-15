//! Error response bodies, in each upstream protocol's own shape.
//!
//! A client talking the Anthropic or Gemini protocol expects gateway errors to
//! look like that provider's errors, not like OpenAI's — `protocol_error_response`
//! picks the right shape for the route.

use axum::body::Body;
use axum::http::{Response, StatusCode};
use serde_json::json;

use crate::providers::RouteKind;

pub(crate) fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    let body = json!({
        "error": {
            "message": message,
            "type": "invalid_request_error",
            "param": null,
            "code": null
        }
    });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub(super) fn gemini_native_error_response(
    status: StatusCode,
    message: &str,
    gemini_status: &str,
) -> Response<Body> {
    let body = json!({
        "error": {
            "code": status.as_u16(),
            "message": message,
            "status": gemini_status,
        }
    });
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub(super) fn protocol_error_response(
    route: RouteKind,
    status: StatusCode,
    message: &str,
) -> Response<Body> {
    if matches!(route, RouteKind::GeminiNative) {
        let gemini_status = match status {
            StatusCode::NOT_FOUND => "NOT_FOUND",
            StatusCode::FORBIDDEN => "PERMISSION_DENIED",
            StatusCode::UNAUTHORIZED => "UNAUTHENTICATED",
            StatusCode::TOO_MANY_REQUESTS => "RESOURCE_EXHAUSTED",
            StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE => "UNAVAILABLE",
            _ => "INVALID_ARGUMENT",
        };
        gemini_native_error_response(status, message, gemini_status)
    } else if matches!(route, RouteKind::Anthropic) {
        let body = json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": message,
            }
        });
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    } else {
        error_response(status, message)
    }
}

pub(super) fn format_error_chain(error: &dyn std::error::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut current = error.source();
    while let Some(source) = current {
        parts.push(source.to_string());
        current = source.source();
    }
    parts.join(" | caused by: ")
}
