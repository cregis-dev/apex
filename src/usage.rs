use crate::database::Database;
use crate::metrics::MetricsState;
use crate::providers::RouteKind;
use anyhow::Result;
use axum::body::{Body, Bytes};
use axum::http::StatusCode;
use axum::response::Response;
use futures::Stream;
use serde_json::Value;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

pub struct UsageLogger {
    db: Arc<Database>,
}

impl UsageLogger {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log(
        &self,
        request_id: Option<&str>,
        team_id: &str,
        router: &str,
        matched_rule: Option<&str>,
        channel: &str,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        latency_ms: Option<f64>,
        fallback_triggered: bool,
        client_info: &crate::utils::ClientInfo,
    ) {
        self.db.log_usage(
            request_id,
            team_id,
            router,
            matched_rule,
            channel,
            model,
            input_tokens as i64,
            output_tokens as i64,
            latency_ms,
            fallback_triggered,
            if fallback_triggered {
                "fallback"
            } else {
                "success"
            },
            Some(200),
            None,
            None,
            None,
            client_info.client.as_deref(),
            client_info.user_agent.as_deref(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_failure(
        &self,
        request_id: Option<&str>,
        team_id: &str,
        router: &str,
        matched_rule: Option<&str>,
        channel: &str,
        model: &str,
        latency_ms: Option<f64>,
        fallback_triggered: bool,
        status_code: i64,
        error_message: &str,
        provider_trace_id: Option<&str>,
        provider_error_body: Option<&str>,
        client_info: &crate::utils::ClientInfo,
    ) {
        self.db.log_usage(
            request_id,
            team_id,
            router,
            matched_rule,
            channel,
            model,
            0,
            0,
            latency_ms,
            fallback_triggered,
            if fallback_triggered {
                "fallback_error"
            } else {
                "error"
            },
            Some(status_code),
            Some(error_message),
            provider_trace_id,
            provider_error_body,
            client_info.client.as_deref(),
            client_info.user_agent.as_deref(),
        );
    }

    pub fn log_gateway_error(&self, route: &str, router: &str) {
        self.db.log_error(route, router);
    }
}

/// A single SSE line larger than this cannot be a usage event; drop it
/// instead of letting a newline-free stream grow the reassembly buffer
/// without bound.
const MAX_SSE_LINE_BYTES: usize = 1024 * 1024;

struct UsageTrackerState {
    request_id: Option<String>,
    team_id: String,
    router: String,
    matched_rule: Option<String>,
    channel: String,
    model: String,
    logger: Arc<UsageLogger>,
    metrics: Arc<MetricsState>,
    input_tokens: u64,
    output_tokens: u64,
    latency_ms: Option<f64>,
    fallback_triggered: bool,
    client_info: crate::utils::ClientInfo,
    accumulated_data: String,
}

impl UsageTrackerState {
    #[allow(clippy::too_many_arguments)]
    fn new(
        team_id: String,
        request_id: Option<String>,
        router: String,
        matched_rule: Option<String>,
        channel: String,
        model: String,
        logger: Arc<UsageLogger>,
        metrics: Arc<MetricsState>,
        latency_ms: Option<f64>,
        fallback_triggered: bool,
    ) -> Self {
        Self {
            request_id,
            team_id,
            router,
            matched_rule,
            channel,
            model,
            logger,
            metrics,
            input_tokens: 0,
            output_tokens: 0,
            latency_ms,
            fallback_triggered,
            client_info: crate::utils::ClientInfo::default(),
            accumulated_data: String::new(),
        }
    }

    fn process_chunk(&mut self, chunk: &[u8], is_sse: bool) {
        if let Ok(s) = std::str::from_utf8(chunk) {
            if is_sse {
                self.accumulated_data.push_str(s);
                let mut start = 0;
                while let Some(end) = self.accumulated_data[start..].find('\n') {
                    let line = self.accumulated_data[start..start + end].to_string();
                    self.process_sse_line(&line);
                    start += end + 1;
                }
                if start > 0 {
                    self.accumulated_data.drain(0..start);
                }
                if self.accumulated_data.len() > MAX_SSE_LINE_BYTES {
                    self.accumulated_data = String::new();
                }
            } else {
                // For non-SSE, we expect the whole body or chunks of JSON.
                // We'll accumulate everything and parse at the end,
                // but since we are in a stream wrapper, we can't easily know the end without state.
                // However, `wrap_response` handles non-SSE by reading the full body first.
                // So this method might only be called for SSE or if we implemented a buffering stream for non-SSE.
                // For simplicity, `wrap_response` handles non-SSE separately.
            }
        }
    }

    fn process_sse_line(&mut self, line: &str) {
        if let Some(data) = line.strip_prefix("data: ") {
            if data.trim() == "[DONE]" {
                return;
            }
            if let Ok(json) = serde_json::from_str::<Value>(data) {
                self.extract_usage(&json);
            }
        }
    }

    fn extract_usage(&mut self, json: &Value) {
        // OpenAI / Generic / Anthropic message_delta
        if let Some(usage) = json.get("usage") {
            if let Some(prompt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                self.input_tokens = prompt; // OpenAI sends cumulative or final
            }
            if let Some(completion) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                self.output_tokens = completion;
            }
            // Anthropic in message_start (sometimes nested differently) or message_delta
            if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                self.input_tokens += input;
            }
            if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                self.output_tokens += output;
            }
        }

        // Anthropic message_start (usage is inside message object)
        if let Some(message) = json.get("message")
            && let Some(usage) = message.get("usage")
        {
            if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                self.input_tokens += input;
            }
            if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                self.output_tokens += output;
            }
        }

        // Gemini native generateContent / streamGenerateContent.
        if let Some(usage) = json.get("usageMetadata") {
            if let Some(input) = usage
                .get("promptTokenCount")
                .or_else(|| usage.get("prompt_token_count"))
                .and_then(|v| v.as_u64())
            {
                self.input_tokens = input;
            }
            if let Some(output) = usage
                .get("candidatesTokenCount")
                .or_else(|| usage.get("candidates_token_count"))
                .and_then(|v| v.as_u64())
            {
                self.output_tokens = output;
            }
        }
    }

    fn flush(&self) {
        if self.input_tokens > 0 || self.output_tokens > 0 {
            let model_lower = self.model.to_lowercase();
            self.metrics
                .token_total
                .with_label_values(&[&self.router, &self.channel, &model_lower, "input"])
                .inc_by(self.input_tokens);
            self.metrics
                .token_total
                .with_label_values(&[&self.router, &self.channel, &model_lower, "output"])
                .inc_by(self.output_tokens);
        }

        self.logger.log(
            self.request_id.as_deref(),
            &self.team_id,
            &self.router,
            self.matched_rule.as_deref(),
            &self.channel,
            &self.model,
            self.input_tokens,
            self.output_tokens,
            self.latency_ms,
            self.fallback_triggered,
            &self.client_info,
        );
    }

    fn flush_failure(&self, route: RouteKind, provider_error_body: &str) {
        self.metrics
            .error_total
            .with_label_values(&[route_label(route), &self.router])
            .inc();
        self.logger
            .log_gateway_error(route_label(route), &self.router);
        self.logger.log_failure(
            self.request_id.as_deref(),
            &self.team_id,
            &self.router,
            self.matched_rule.as_deref(),
            &self.channel,
            &self.model,
            self.latency_ms,
            self.fallback_triggered,
            StatusCode::BAD_GATEWAY.as_u16() as i64,
            UPSTREAM_BODY_ERROR_MESSAGE,
            None,
            Some(provider_error_body),
            &self.client_info,
        );
    }
}

pub struct UsageStream<S> {
    inner: S,
    state: Arc<Mutex<UsageTrackerState>>,
    route: RouteKind,
    failed: bool,
}

impl<S, E> Stream for UsageStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.failed {
            return Poll::Ready(None);
        }

        let poll = Pin::new(&mut self.inner).poll_next(cx);
        match poll {
            Poll::Ready(Some(Ok(bytes))) => {
                if let Ok(mut state) = self.state.lock() {
                    state.process_chunk(&bytes, true);
                }
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(err))) => {
                if let Ok(state) = self.state.lock() {
                    state.flush_failure(self.route, &err.to_string());
                }
                self.failed = true;
                Poll::Ready(Some(Ok(stream_error_event(self.route))))
            }
            Poll::Ready(None) => {
                // Stream finished
                if let Ok(state) = self.state.lock() {
                    state.flush();
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub const UPSTREAM_BODY_ERROR_MESSAGE: &str = "failed to read upstream response body";
const UPSTREAM_BODY_ERROR_HEADER: &str = "x-apex-upstream-body-error";
/// Cap for upstream bodies buffered in gateway memory (success reads, error
/// reads, and SSE replay recording all share it).
pub const MAX_UPSTREAM_BODY_BYTES: usize = 10 * 1024 * 1024;

pub fn is_upstream_body_error_response(response: &Response<Body>) -> bool {
    response.headers().contains_key(UPSTREAM_BODY_ERROR_HEADER)
}

fn route_label(route: RouteKind) -> &'static str {
    match route {
        RouteKind::Openai => "openai",
        RouteKind::Anthropic => "anthropic",
        RouteKind::GeminiNative => "gemini_native",
    }
}

fn stream_error_event(route: RouteKind) -> Bytes {
    let body = match route {
        RouteKind::Anthropic => serde_json::json!({
            "type": "error",
            "error": {
                "type": "upstream_body_error",
                "message": UPSTREAM_BODY_ERROR_MESSAGE,
            }
        }),
        RouteKind::GeminiNative => serde_json::json!({
            "error": {
                "code": StatusCode::BAD_GATEWAY.as_u16(),
                "message": UPSTREAM_BODY_ERROR_MESSAGE,
                "status": "UNAVAILABLE",
            }
        }),
        RouteKind::Openai => serde_json::json!({
            "error": {
                "type": "upstream_body_error",
                "message": UPSTREAM_BODY_ERROR_MESSAGE,
            }
        }),
    };

    match route {
        RouteKind::Anthropic => Bytes::from(format!("event: error\ndata: {body}\n\n")),
        RouteKind::Openai | RouteKind::GeminiNative => Bytes::from(format!("data: {body}\n\n")),
    }
}

fn upstream_body_error_response(route: RouteKind) -> Response<Body> {
    let body = match route {
        RouteKind::Anthropic => serde_json::json!({
            "type": "error",
            "error": {
                "type": "upstream_body_error",
                "message": UPSTREAM_BODY_ERROR_MESSAGE,
            }
        }),
        RouteKind::GeminiNative => serde_json::json!({
            "error": {
                "code": StatusCode::BAD_GATEWAY.as_u16(),
                "message": UPSTREAM_BODY_ERROR_MESSAGE,
                "status": "UNAVAILABLE",
            }
        }),
        RouteKind::Openai => serde_json::json!({
            "error": {
                "type": "upstream_body_error",
                "message": UPSTREAM_BODY_ERROR_MESSAGE,
            }
        }),
    };
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .header("content-type", "application/json")
        .header(UPSTREAM_BODY_ERROR_HEADER, "1")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn read_non_sse_body(
    body: Body,
    limit: usize,
    timeout: Option<Duration>,
) -> Result<Bytes, String> {
    let read = axum::body::to_bytes(body, limit);
    let Some(timeout) = timeout.filter(|timeout| !timeout.is_zero()) else {
        return read.await.map_err(|err| err.to_string());
    };

    match tokio::time::timeout(timeout, read).await {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "response body timeout after {}ms",
            timeout.as_millis()
        )),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn wrap_response(
    response: Response<Body>,
    route: RouteKind,
    request_id: Option<String>,
    team_id: String,
    router: String,
    matched_rule: Option<String>,
    channel: String,
    model: String,
    logger: Arc<UsageLogger>,
    metrics: Arc<MetricsState>,
    latency_ms: Option<f64>,
    fallback_triggered: bool,
    client_info: crate::utils::ClientInfo,
    body_read_timeout: Option<Duration>,
) -> Response<Body> {
    let is_sse = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/event-stream"))
        .unwrap_or(false);

    let (parts, body) = response.into_parts();

    if is_sse {
        let mut tracker = UsageTrackerState::new(
            team_id,
            request_id,
            router,
            matched_rule,
            channel,
            model,
            logger,
            metrics,
            latency_ms,
            fallback_triggered,
        );
        tracker.client_info = client_info;
        let state = Arc::new(Mutex::new(tracker));
        let stream = body.into_data_stream();
        let usage_stream = UsageStream {
            inner: stream,
            state,
            route,
            failed: false,
        };
        Response::from_parts(parts, Body::from_stream(usage_stream))
    } else {
        // Non-SSE: read full body
        let bytes = match read_non_sse_body(body, MAX_UPSTREAM_BODY_BYTES, body_read_timeout).await
        {
            Ok(b) => b,
            Err(err) => {
                tracing::warn!("Failed to read upstream response body: {}", err);
                metrics
                    .error_total
                    .with_label_values(&[route_label(route), &router])
                    .inc();
                logger.log_gateway_error(route_label(route), &router);
                logger.log_failure(
                    request_id.as_deref(),
                    &team_id,
                    &router,
                    matched_rule.as_deref(),
                    &channel,
                    &model,
                    latency_ms,
                    fallback_triggered,
                    StatusCode::BAD_GATEWAY.as_u16() as i64,
                    UPSTREAM_BODY_ERROR_MESSAGE,
                    None,
                    Some(&err),
                    &client_info,
                );
                return upstream_body_error_response(route);
            }
        };

        // Process usage
        let mut state = UsageTrackerState::new(
            team_id,
            request_id,
            router,
            matched_rule,
            channel,
            model,
            logger,
            metrics,
            latency_ms,
            fallback_triggered,
        );
        state.client_info = client_info;

        if let Ok(json) = serde_json::from_slice::<Value>(&bytes) {
            state.extract_usage(&json);
            state.flush();
        }

        Response::from_parts(parts, Body::from(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use tempfile::{TempDir, tempdir};

    fn create_test_metrics() -> Arc<MetricsState> {
        Arc::new(MetricsState::new().unwrap())
    }

    fn create_test_logger() -> (TempDir, Arc<UsageLogger>) {
        let dir = tempdir().unwrap();
        let db = Arc::new(Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap());
        (dir, Arc::new(UsageLogger::new(db)))
    }

    #[test]
    fn test_extract_usage_openai() {
        let (_dir, logger) = create_test_logger();
        let metrics = create_test_metrics();

        let mut tracker = UsageTrackerState::new(
            "team1".to_string(),
            Some("req-1".to_string()),
            "r1".to_string(),
            Some("gpt-*".to_string()),
            "c1".to_string(),
            "m1".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
        );

        let json = serde_json::json!({
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 10
            }
        });

        tracker.extract_usage(&json);
        assert_eq!(tracker.input_tokens, 5);
        assert_eq!(tracker.output_tokens, 10);
    }

    #[test]
    fn test_extract_usage_anthropic_message_start() {
        let (_dir, logger) = create_test_logger();
        let metrics = create_test_metrics();

        let mut tracker = UsageTrackerState::new(
            "team1".to_string(),
            Some("req-1".to_string()),
            "r1".to_string(),
            Some("gpt-*".to_string()),
            "c1".to_string(),
            "m1".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
        );

        let json = serde_json::json!({
            "type": "message_start",
            "message": {
                "usage": {
                    "input_tokens": 15,
                    "output_tokens": 1
                }
            }
        });

        tracker.extract_usage(&json);
        assert_eq!(tracker.input_tokens, 15);
        assert_eq!(tracker.output_tokens, 1);
    }

    #[test]
    fn test_flush_logs_success_even_without_usage_tokens() {
        let (dir, logger) = create_test_logger();
        let metrics = create_test_metrics();

        let tracker = UsageTrackerState::new(
            "team1".to_string(),
            Some("req-1".to_string()),
            "r1".to_string(),
            Some("gemini-*".to_string()),
            "gemini_primary".to_string(),
            "gemini-3.1-pro-preview".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
        );

        tracker.flush();

        let db = Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap();
        let (records, total) = db
            .get_usage_records(None, None, None, None, None, None, None, 10, 0)
            .unwrap();

        assert_eq!(total, 1);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, "success");
        assert_eq!(records[0].channel, "gemini_primary");
        assert_eq!(records[0].model, "gemini-3.1-pro-preview");
        assert_eq!(records[0].input_tokens, 0);
        assert_eq!(records[0].output_tokens, 0);
    }

    #[tokio::test]
    async fn wrap_response_non_sse_body_error_returns_bad_gateway_body() {
        let (dir, logger) = create_test_logger();
        let metrics = create_test_metrics();
        let failing_stream = futures::stream::once(async {
            Err::<Bytes, std::io::Error>(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "response timeout",
            ))
        });
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from_stream(failing_stream))
            .unwrap();

        let wrapped = wrap_response(
            response,
            crate::providers::RouteKind::Openai,
            Some("req-1".to_string()),
            "team1".to_string(),
            "r1".to_string(),
            Some("m1-*".to_string()),
            "minimax".to_string(),
            "minimax-m3".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
            crate::utils::ClientInfo::default(),
            None,
        )
        .await;

        assert_eq!(wrapped.status(), StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(wrapped.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_text.contains("failed to read upstream response body"));

        let db = Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap();
        let (records, total) = db
            .get_usage_records(None, None, None, None, None, None, None, 10, 0)
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, "error");
        assert_eq!(records[0].status_code, Some(502));
        assert_eq!(db.get_metrics_summary().unwrap().total_errors, 1);
    }

    // start_paused: timers auto-advance in virtual time, so the test is
    // deterministic on loaded CI. The load-bearing assertion is the status —
    // if the deadline were per-chunk instead of total, the stream would
    // complete and return 200.
    #[tokio::test(start_paused = true)]
    async fn wrap_response_non_sse_body_timeout_is_total_duration() {
        let (dir, logger) = create_test_logger();
        let metrics = create_test_metrics();
        let slow_stream = futures::stream::unfold(0, |index| async move {
            if index >= 10 {
                None
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Some((
                    Ok::<Bytes, std::io::Error>(Bytes::from_static(b"x")),
                    index + 1,
                ))
            }
        });
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from_stream(slow_stream))
            .unwrap();

        let started = std::time::Instant::now();
        let wrapped = wrap_response(
            response,
            crate::providers::RouteKind::Anthropic,
            Some("req-1".to_string()),
            "team1".to_string(),
            "r1".to_string(),
            Some("m1-*".to_string()),
            "deepseek".to_string(),
            "deepseek-v4-flash".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
            crate::utils::ClientInfo::default(),
            Some(Duration::from_millis(35)),
        )
        .await;

        assert_eq!(wrapped.status(), StatusCode::BAD_GATEWAY);
        // Under the paused clock no real sleeping may happen at all.
        assert!(started.elapsed() < Duration::from_secs(5));
        let body = axum::body::to_bytes(wrapped.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body_json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["type"], "error");
        assert_eq!(body_json["error"]["type"], "upstream_body_error");

        let db = Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap();
        let (records, total) = db
            .get_usage_records(None, None, None, None, None, None, None, 10, 0)
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(records[0].status, "error");
        assert_eq!(records[0].status_code, Some(502));
        assert!(
            records[0]
                .provider_error_body
                .as_deref()
                .unwrap_or_default()
                .contains("response body timeout after 35ms")
        );
    }

    #[test]
    fn sse_buffer_is_bounded_for_newline_free_streams() {
        let (_dir, logger) = create_test_logger();
        let metrics = create_test_metrics();
        let mut state = UsageTrackerState::new(
            "team1".to_string(),
            None,
            "r1".to_string(),
            None,
            "ch".to_string(),
            "m".to_string(),
            logger,
            metrics,
            None,
            false,
        );

        let chunk = vec![b'a'; 512 * 1024];
        for _ in 0..8 {
            state.process_chunk(&chunk, true);
        }
        // 4MB of newline-free input must not pile up in the line buffer.
        assert!(state.accumulated_data.len() <= MAX_SSE_LINE_BYTES);

        // Later well-formed usage events still parse after the drop.
        state.process_chunk(
            b"\ndata: {\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":5}}\n",
            true,
        );
        assert_eq!(state.input_tokens, 3);
        assert_eq!(state.output_tokens, 5);
    }

    #[tokio::test]
    async fn wrap_response_anthropic_body_error_uses_anthropic_error_shape() {
        let (_dir, logger) = create_test_logger();
        let metrics = create_test_metrics();
        let failing_stream = futures::stream::once(async {
            Err::<Bytes, std::io::Error>(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "response timeout",
            ))
        });
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from_stream(failing_stream))
            .unwrap();

        let wrapped = wrap_response(
            response,
            crate::providers::RouteKind::Anthropic,
            Some("req-1".to_string()),
            "team1".to_string(),
            "r1".to_string(),
            Some("m1-*".to_string()),
            "minimax".to_string(),
            "minimax-m3".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
            crate::utils::ClientInfo::default(),
            None,
        )
        .await;

        assert_eq!(wrapped.status(), StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(wrapped.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body_json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body_json["type"], "error");
        assert_eq!(body_json["error"]["type"], "upstream_body_error");
        assert_eq!(
            body_json["error"]["message"],
            "failed to read upstream response body"
        );
    }

    #[tokio::test]
    async fn wrap_response_sse_body_error_emits_error_event_and_logs_failure() {
        let (dir, logger) = create_test_logger();
        let metrics = create_test_metrics();
        let failing_stream = futures::stream::once(async {
            Err::<Bytes, std::io::Error>(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "response timeout",
            ))
        });
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/event-stream")
            .body(Body::from_stream(failing_stream))
            .unwrap();

        let wrapped = wrap_response(
            response,
            crate::providers::RouteKind::Anthropic,
            Some("req-1".to_string()),
            "team1".to_string(),
            "r1".to_string(),
            Some("m1-*".to_string()),
            "minimax".to_string(),
            "minimax-m3".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
            crate::utils::ClientInfo::default(),
            Some(Duration::from_millis(1)),
        )
        .await;

        assert_eq!(wrapped.status(), StatusCode::OK);
        let body = axum::body::to_bytes(wrapped.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body_text = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_text.contains("event: error"));
        assert!(body_text.contains("upstream_body_error"));

        let db = Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap();
        let (records, total) = db
            .get_usage_records(None, None, None, None, None, None, None, 10, 0)
            .unwrap();
        assert_eq!(total, 1);
        assert_eq!(records[0].status, "error");
        assert_eq!(records[0].status_code, Some(502));
        assert_eq!(db.get_metrics_summary().unwrap().total_errors, 1);
    }

    #[test]
    fn test_extract_usage_anthropic_message_delta() {
        let (_dir, logger) = create_test_logger();
        let metrics = create_test_metrics();

        let mut tracker = UsageTrackerState::new(
            "team1".to_string(),
            Some("req-1".to_string()),
            "r1".to_string(),
            Some("gpt-*".to_string()),
            "c1".to_string(),
            "m1".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
        );

        let json = serde_json::json!({
            "type": "message_delta",
            "usage": {
                "output_tokens": 5
            }
        });

        tracker.extract_usage(&json);
        assert_eq!(tracker.input_tokens, 0);
        assert_eq!(tracker.output_tokens, 5);
    }

    #[test]
    fn test_extract_usage_gemini_native_usage_metadata() {
        let (_dir, logger) = create_test_logger();
        let metrics = create_test_metrics();

        let mut tracker = UsageTrackerState::new(
            "team1".to_string(),
            Some("req-1".to_string()),
            "gemini_native".to_string(),
            Some("gemini-*".to_string()),
            "c1".to_string(),
            "gemini-test".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
        );

        let json = serde_json::json!({
            "usageMetadata": {
                "promptTokenCount": 11,
                "candidatesTokenCount": 7,
                "totalTokenCount": 18
            }
        });

        tracker.extract_usage(&json);
        assert_eq!(tracker.input_tokens, 11);
        assert_eq!(tracker.output_tokens, 7);
    }

    #[test]
    fn test_extract_usage_gemini_native_snake_case_usage_metadata() {
        let (_dir, logger) = create_test_logger();
        let metrics = create_test_metrics();

        let mut tracker = UsageTrackerState::new(
            "team1".to_string(),
            Some("req-1".to_string()),
            "gemini_native".to_string(),
            Some("gemini-*".to_string()),
            "c1".to_string(),
            "gemini-test".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
        );

        let json = serde_json::json!({
            "usageMetadata": {
                "prompt_token_count": 13,
                "candidates_token_count": 8,
                "total_token_count": 21
            }
        });

        tracker.extract_usage(&json);
        assert_eq!(tracker.input_tokens, 13);
        assert_eq!(tracker.output_tokens, 8);
    }

    #[test]
    fn test_process_sse_line() {
        let (_dir, logger) = create_test_logger();
        let metrics = create_test_metrics();

        let mut tracker = UsageTrackerState::new(
            "team1".to_string(),
            Some("req-1".to_string()),
            "r1".to_string(),
            Some("gpt-*".to_string()),
            "c1".to_string(),
            "m1".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
        );

        let line = r#"data: {"usage": {"prompt_tokens": 3, "completion_tokens": 4}}"#;
        tracker.process_sse_line(line);

        assert_eq!(tracker.input_tokens, 3);
        assert_eq!(tracker.output_tokens, 4);
    }

    #[test]
    fn test_process_chunk_sse_partial() {
        let (_dir, logger) = create_test_logger();
        let metrics = create_test_metrics();

        let mut tracker = UsageTrackerState::new(
            "team1".to_string(),
            Some("req-1".to_string()),
            "r1".to_string(),
            Some("gpt-*".to_string()),
            "c1".to_string(),
            "m1".to_string(),
            logger,
            metrics,
            Some(42.0),
            false,
        );

        tracker.process_chunk(b"data: {\"usage\": {\"pro", true);
        tracker.process_chunk(b"mpt_tokens\": 2}}\n\n", true);

        assert_eq!(tracker.input_tokens, 2);
    }

    #[test]
    fn test_usage_logging_does_not_create_usage_csv() {
        let dir = tempdir().unwrap();
        let db = Arc::new(Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap());
        let logger = UsageLogger::new(db);

        logger.log(
            Some("req-1"),
            "team1",
            "router1",
            Some("gpt-*"),
            "channel1",
            "gpt-4",
            10,
            20,
            Some(12.0),
            false,
            &crate::utils::ClientInfo::default(),
        );

        assert!(
            !dir.path().join("usage.csv").exists(),
            "usage.csv should not be created anymore"
        );
        assert!(
            dir.path().join("apex.db").exists(),
            "usage should be persisted in SQLite"
        );
    }
}
