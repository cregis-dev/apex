//! Small helpers shared by the request pipeline: inspecting inbound bodies,
//! lifting correlation ids off headers, and rebuilding a response from what an
//! upstream returned.

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Response, StatusCode};
use axum::response::Response as HttpResponse;
use std::time::Duration;

use crate::config::{Channel, Timeouts};
use crate::providers::ResponseTimeouts;

use super::errors::error_response;

pub(super) fn anthropic_request_contains_tool_result(body: &Bytes) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };

    value
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .map(|messages| {
            messages.iter().any(|message| {
                message
                    .get("content")
                    .and_then(serde_json::Value::as_array)
                    .map(|parts| {
                        parts.iter().any(|part| {
                            part.get("type").and_then(serde_json::Value::as_str)
                                == Some("tool_result")
                        })
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub(super) fn summarize_anthropic_request(body: &Bytes) -> String {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return "invalid_json".to_string();
    };
    let Some(messages) = value.get("messages").and_then(serde_json::Value::as_array) else {
        return "messages=missing".to_string();
    };

    let mut referenced_tool_use_ids = Vec::new();
    let mut segments = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        let role = message
            .get("role")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let mut kinds = Vec::new();
        if let Some(content) = message.get("content").and_then(serde_json::Value::as_array) {
            for block in content {
                let kind = block
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown");
                kinds.push(kind.to_string());
                if kind == "tool_result"
                    && let Some(tool_use_id) =
                        block.get("tool_use_id").and_then(serde_json::Value::as_str)
                {
                    referenced_tool_use_ids.push(tool_use_id.to_string());
                }
            }
        }
        segments.push(format!("{index}:{role}[{}]", kinds.join(",")));
    }

    format!(
        "messages={} {} tool_result_ids={:?}",
        messages.len(),
        segments.join(" "),
        referenced_tool_use_ids
    )
}

pub(super) fn request_id_from_parts(parts: &axum::http::request::Parts) -> Option<String> {
    parts
        .extensions
        .get::<tower_http::request_id::RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .map(|id| id.to_string())
}

pub(super) fn provider_trace_id_from_headers(headers: &HeaderMap) -> Option<String> {
    const TRACE_HEADERS: [&str; 5] = [
        "x-request-id",
        "request-id",
        "x-trace-id",
        "trace-id",
        "cf-ray",
    ];

    TRACE_HEADERS.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string())
    })
}

pub(super) fn response_from_upstream_bytes(
    status: StatusCode,
    headers: &HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let mut builder = HttpResponse::builder().status(status);
    for (name, value) in headers {
        if crate::providers::should_forward_response_header(name) {
            builder = builder.header(name, value);
        }
    }

    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| error_response(StatusCode::BAD_GATEWAY, "invalid upstream response"))
}

pub(super) fn truncate_for_storage(input: &str, limit: usize) -> String {
    input.chars().take(limit).collect()
}

pub(super) fn response_timeouts_for(global: &Timeouts, channel: &Channel) -> ResponseTimeouts {
    let timeouts = channel.timeouts.as_ref().unwrap_or(global);
    ResponseTimeouts {
        idle: Duration::from_millis(timeouts.response_ms),
        // `response_ms` used to guard buffered bodies too, so operators raised it
        // to accommodate slow models. Taking the max keeps those configs working
        // instead of newly timing them out at a shorter `request_ms`.
        total: Duration::from_millis(timeouts.request_ms.max(timeouts.response_ms)),
    }
}
