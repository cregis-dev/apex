use anyhow::Context;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::net::SocketAddr;

#[derive(Clone)]
struct MockState {
    name: String,
    chat_status: StatusCode,
    message_status: StatusCode,
}

pub struct MockProvider {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl MockProvider {
    pub async fn spawn(name: impl Into<String>) -> anyhow::Result<Self> {
        Self::spawn_with_status(name, StatusCode::OK, StatusCode::OK).await
    }

    pub async fn spawn_failing_chat(
        name: impl Into<String>,
        status: StatusCode,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_status(name, status, StatusCode::OK).await
    }

    async fn spawn_with_status(
        name: impl Into<String>,
        chat_status: StatusCode,
        message_status: StatusCode,
    ) -> anyhow::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind mock provider listener")?;
        let addr = listener
            .local_addr()
            .context("failed to get mock provider addr")?;
        let state = MockState {
            name: name.into(),
            chat_status,
            message_status,
        };

        let app = Router::new()
            .route("/healthz", get(healthz))
            .route("/v1/models", get(models))
            .route("/models", get(models))
            .route("/openai/models", get(models))
            .route("/v1/chat/completions", post(chat_completions))
            .route("/chat/completions", post(chat_completions))
            .route("/openai/chat/completions", post(chat_completions))
            .route("/v1/messages", post(messages))
            .route("/messages", post(messages))
            .with_state(state);

        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Ok(Self { addr, handle })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

impl Drop for MockProvider {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn healthz() -> &'static str {
    "OK"
}

async fn models(State(state): State<MockState>) -> Json<Value> {
    let provider_name = state.name;
    Json(json!({
        "object": "list",
        "data": [
            {
                "id": format!("{}-model", provider_name),
                "object": "model",
                "created": 1_677_652_288,
                "owned_by": provider_name.clone(),
            }
        ]
    }))
}

async fn chat_completions(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
    if state.chat_status != StatusCode::OK {
        return (
            state.chat_status,
            Json(json!({
                "error": {
                    "message": format!("forced failure from {}", state.name),
                    "type": "upstream_error"
                }
            })),
        )
            .into_response();
    }

    if body.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        let mut response = Response::new(axum::body::Body::from(format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({
                "id": "chatcmpl-stream",
                "object": "chat.completion.chunk",
                "choices": [
                    {
                        "index": 0,
                        "delta": { "content": format!("stream from {}", state.name) },
                        "finish_reason": null
                    }
                ]
            })
        )));
        *response.status_mut() = StatusCode::OK;
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        return response;
    }

    Json(json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "created": 1_677_652_288,
        "model": body.get("model").cloned().unwrap_or_else(|| json!("mock-model")),
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": format!("response from {}", state.name),
                },
                "finish_reason": "stop",
            }
        ],
        "usage": {
            "prompt_tokens": 5,
            "completion_tokens": 5,
            "total_tokens": 10,
        }
    }))
    .into_response()
}

async fn messages(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
    if state.message_status != StatusCode::OK {
        return (
            state.message_status,
            Json(json!({
                "type": "error",
                "error": {
                    "type": "upstream_error",
                    "message": format!("forced failure from {}", state.name),
                }
            })),
        )
            .into_response();
    }

    if body.get("stream").and_then(Value::as_bool).unwrap_or(false) {
        let mut response = Response::new(axum::body::Body::from(format!(
            concat!(
                "event: message_start\n",
                "data: {{\"type\":\"message_start\",\"message\":{{\"id\":\"msg-mock\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[],\"model\":\"{}\",\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{{\"input_tokens\":0,\"output_tokens\":0}}}}}}\n\n",
                "event: content_block_start\n",
                "data: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n",
                "event: content_block_delta\n",
                "data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"stream from {}\"}}}}\n\n",
                "event: message_delta\n",
                "data: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\",\"stop_sequence\":null}},\"usage\":{{\"output_tokens\":4}}}}\n\n",
                "event: message_stop\n",
                "data: {{\"type\":\"message_stop\"}}\n\n"
            ),
            body.get("model")
                .and_then(Value::as_str)
                .unwrap_or("mock-model"),
            state.name
        )));
        *response.status_mut() = StatusCode::OK;
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        return response;
    }

    Json(json!({
        "id": "msg-mock",
        "type": "message",
        "role": "assistant",
        "model": body.get("model").cloned().unwrap_or_else(|| json!("mock-model")),
        "content": [
            {
                "type": "text",
                "text": format!("response from {}", state.name),
            }
        ],
        "stop_reason": "end_turn",
    }))
    .into_response()
}
