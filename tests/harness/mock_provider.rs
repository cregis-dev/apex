use anyhow::Context;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::net::SocketAddr;

#[derive(Clone)]
struct MockState {
    name: String,
    chat_status: StatusCode,
    message_status: StatusCode,
    embeddings_status: StatusCode,
}

pub struct MockProvider {
    addr: SocketAddr,
    handle: tokio::task::JoinHandle<()>,
}

impl MockProvider {
    pub async fn spawn(name: impl Into<String>) -> anyhow::Result<Self> {
        Self::spawn_with_status(name, StatusCode::OK, StatusCode::OK, StatusCode::OK).await
    }

    pub async fn spawn_failing_chat(
        name: impl Into<String>,
        status: StatusCode,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_status(name, status, StatusCode::OK, StatusCode::OK).await
    }

    async fn spawn_with_status(
        name: impl Into<String>,
        chat_status: StatusCode,
        message_status: StatusCode,
        embeddings_status: StatusCode,
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
            embeddings_status,
        };

        let app = Router::new()
            .route("/healthz", get(healthz))
            .route("/v1/models", get(models))
            .route("/models", get(models))
            .route("/openai/models", get(models))
            .route("/v1/chat/completions", post(chat_completions))
            .route("/chat/completions", post(chat_completions))
            .route("/openai/chat/completions", post(chat_completions))
            .route("/v1/embeddings", post(embeddings))
            .route("/embeddings", post(embeddings))
            .route("/openai/embeddings", post(embeddings))
            .route("/v1/messages", post(messages))
            .route("/messages", post(messages))
            .route("/v1beta/models", get(gemini_native_models))
            .route("/v1beta/models/*model_action", get(gemini_native_model))
            .route("/v1beta/models/*model_action", post(gemini_native_generate))
            .route("/v1beta/fileSearchStores", get(gemini_native_resource))
            .route("/v1beta/fileSearchStores", post(gemini_native_resource))
            .route(
                "/v1beta/fileSearchStores/*path",
                get(gemini_native_resource),
            )
            .route(
                "/v1beta/fileSearchStores/*path",
                post(gemini_native_resource),
            )
            .route(
                "/v1beta/fileSearchStores/*path",
                delete(gemini_native_resource),
            )
            .route(
                "/upload/v1beta/fileSearchStores/*path",
                post(gemini_native_resource),
            )
            .route(
                "/v1beta/interactions",
                post(gemini_native_interaction_create),
            )
            .route(
                "/v1beta/interactions/*interaction",
                get(gemini_native_interaction_get),
            )
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

async fn embeddings(State(state): State<MockState>, Json(body): Json<Value>) -> Response {
    if state.embeddings_status != StatusCode::OK {
        return (
            state.embeddings_status,
            Json(json!({
                "error": {
                    "message": format!("forced embeddings failure from {}", state.name),
                    "type": "upstream_error"
                }
            })),
        )
            .into_response();
    }

    let inputs = body
        .get("input")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![body.get("input").cloned().unwrap_or_else(|| json!(""))]);
    let data = inputs
        .into_iter()
        .enumerate()
        .map(|(index, _)| {
            json!({
                "object": "embedding",
                "index": index,
                "embedding": [0.01, 0.02, 0.03]
            })
        })
        .collect::<Vec<_>>();

    Json(json!({
        "object": "list",
        "data": data,
        "model": body.get("model").cloned().unwrap_or_else(|| json!("mock-model")),
        "usage": {
            "prompt_tokens": 3,
            "total_tokens": 3
        }
    }))
    .into_response()
}

async fn gemini_native_models(State(state): State<MockState>, headers: HeaderMap) -> Response {
    Json(json!({
        "models": [
            {
                "name": format!("models/{}-native-model", state.name),
                "supportedGenerationMethods": ["generateContent", "streamGenerateContent"]
            }
        ],
        "auth": gemini_native_auth_snapshot(&headers)
    }))
    .into_response()
}

async fn gemini_native_model(
    State(state): State<MockState>,
    headers: HeaderMap,
    axum::extract::Path(model_action): axum::extract::Path<String>,
) -> Response {
    Json(json!({
        "name": format!("models/{}", model_action),
        "provider": state.name,
        "auth": gemini_native_auth_snapshot(&headers)
    }))
    .into_response()
}

async fn gemini_native_generate(
    State(state): State<MockState>,
    headers: HeaderMap,
    axum::extract::Path(model_action): axum::extract::Path<String>,
    body: Bytes,
) -> Response {
    let body_json = serde_json::from_slice::<Value>(&body).unwrap_or_else(|_| json!(null));
    if model_action.ends_with(":streamGenerateContent") {
        let mut response = Response::new(axum::body::Body::from(format!(
            "data: {}\n\n",
            json!({
                "candidates": [
                    {
                        "content": {
                            "parts": [{"text": format!("stream from {}", state.name)}],
                            "role": "model"
                        },
                        "groundingMetadata": {
                            "searchEntryPoint": {"renderedContent": "mock-rendered"}
                        }
                    }
                ],
                "usageMetadata": {
                    "promptTokenCount": 11,
                    "candidatesTokenCount": 7,
                    "totalTokenCount": 18
                },
                "auth": gemini_native_auth_snapshot(&headers),
                "request": body_json
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
        "candidates": [
            {
                "content": {
                    "parts": [
                        {"text": format!("response from {}", state.name)},
                        {"executableCode": {"language": "PYTHON", "code": "print('ok')"}},
                        {"codeExecutionResult": {"outcome": "OUTCOME_OK", "output": "ok"}}
                    ],
                    "role": "model"
                },
                "groundingMetadata": {
                    "searchEntryPoint": {"renderedContent": "mock-rendered"}
                },
                "urlContextMetadata": {
                    "urlMetadata": [{"retrievedUrl": "https://example.com"}]
                }
            }
        ],
        "usageMetadata": {
            "promptTokenCount": 11,
            "candidatesTokenCount": 7,
            "totalTokenCount": 18
        },
        "auth": gemini_native_auth_snapshot(&headers),
        "modelAction": model_action,
        "request": body_json
    }))
    .into_response()
}

async fn gemini_native_resource(
    State(state): State<MockState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    Json(json!({
        "name": format!("fileSearchStores/{}", state.name),
        "done": true,
        "auth": gemini_native_auth_snapshot(&headers),
        "contentType": headers
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        "uploadProtocol": headers
            .get("x-goog-upload-protocol")
            .and_then(|value| value.to_str().ok()),
        "body": String::from_utf8_lossy(&body)
    }))
    .into_response()
}

async fn gemini_native_interaction_create(
    State(state): State<MockState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let body_json = serde_json::from_slice::<Value>(&body).unwrap_or_else(|_| json!(null));
    Json(json!({
        "id": format!("interactions/{}-research-1", state.name),
        "status": "in_progress",
        "agent": body_json.get("agent").cloned().unwrap_or_else(|| json!("deep-research-pro-preview")),
        "auth": gemini_native_auth_snapshot(&headers),
        "request": body_json
    }))
    .into_response()
}

async fn gemini_native_interaction_get(
    State(state): State<MockState>,
    headers: HeaderMap,
    axum::extract::Path(interaction): axum::extract::Path<String>,
) -> Response {
    Json(json!({
        "id": interaction,
        "status": "completed",
        "outputs": [{"type": "text", "text": format!("research from {}", state.name)}],
        "auth": gemini_native_auth_snapshot(&headers)
    }))
    .into_response()
}

fn gemini_native_auth_snapshot(headers: &HeaderMap) -> Value {
    json!({
        "xGoogApiKey": headers
            .get("x-goog-api-key")
            .and_then(|value| value.to_str().ok()),
        "authorization": headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        "xApiKey": headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok()),
        "customGoogHeader": headers
            .get("x-goog-fieldmask")
            .and_then(|value| value.to_str().ok())
    })
}
