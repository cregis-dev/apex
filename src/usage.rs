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
        cache_read_tokens: u64,
        cache_write_tokens: u64,
        req_hash: Option<&str>,
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
            cache_read_tokens as i64,
            cache_write_tokens as i64,
            req_hash,
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
            0,
            0,
            // Failure rows carry no request fingerprint yet; the retry-storm
            // signal that consumes it lands with the detection layer.
            None,
        );
    }

    pub fn log_gateway_error(&self, route: &str, router: &str) {
        self.db.log_error(route, router);
    }
}

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
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    latency_ms: Option<f64>,
    fallback_triggered: bool,
    client_info: crate::utils::ClientInfo,
    /// One-way fingerprint of the request payload (set post-construction, like
    /// `client_info`). `None` when profiling/hash_requests is off.
    req_hash: Option<String>,
    accumulated_data: Vec<u8>,
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
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            latency_ms,
            fallback_triggered,
            client_info: crate::utils::ClientInfo::default(),
            req_hash: None,
            accumulated_data: Vec::new(),
        }
    }

    fn process_chunk(&mut self, chunk: &[u8], is_sse: bool) {
        // Buffer at the byte level: network chunk boundaries can fall in the
        // middle of a multi-byte UTF-8 character (common with Chinese content),
        // so decoding each raw chunk as UTF-8 would fail and drop it — losing the
        // trailing `usage` SSE line and recording 0 tokens. Instead we accumulate
        // raw bytes and only decode complete lines (split on '\n', which never
        // bisects a UTF-8 code point).
        if is_sse {
            self.accumulated_data.extend_from_slice(chunk);
            let mut start = 0;
            while let Some(offset) = self.accumulated_data[start..]
                .iter()
                .position(|&b| b == b'\n')
            {
                let line = String::from_utf8_lossy(&self.accumulated_data[start..start + offset])
                    .into_owned();
                self.process_sse_line(&line);
                start += offset + 1;
            }
            if start > 0 {
                self.accumulated_data.drain(0..start);
            }
        }
        // For non-SSE, `wrap_response` reads the full body and parses it directly,
        // so nothing to accumulate here.
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
                // OpenAI-compatible providers count cached tokens *inside* prompt_tokens;
                // peel them out so input_tokens stays "full-price input" and cache is
                // billed separately. Providers report the cached count under different
                // keys: OpenAI/most → prompt_tokens_details.cached_tokens; DeepSeek →
                // prompt_cache_hit_tokens. Take whichever is present.
                let cached = usage
                    .get("prompt_tokens_details")
                    .and_then(|d| d.get("cached_tokens"))
                    .and_then(|v| v.as_u64())
                    .or_else(|| {
                        usage
                            .get("prompt_cache_hit_tokens")
                            .and_then(|v| v.as_u64())
                    })
                    .or_else(|| usage.get("cached_tokens").and_then(|v| v.as_u64()))
                    .unwrap_or(0);
                self.input_tokens = prompt.saturating_sub(cached); // OpenAI sends cumulative or final
                self.cache_read_tokens = cached;
            }
            if let Some(completion) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                self.output_tokens = completion;
            }
            // Anthropic in message_start (sometimes nested differently) or message_delta.
            // Anthropic's input_tokens already excludes cache, so add cache separately.
            if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                self.input_tokens += input;
            }
            if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                self.output_tokens += output;
            }
            if let Some(cr) = usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
            {
                self.cache_read_tokens += cr;
            }
            if let Some(cw) = usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
            {
                self.cache_write_tokens += cw;
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
            if let Some(cr) = usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
            {
                self.cache_read_tokens += cr;
            }
            if let Some(cw) = usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
            {
                self.cache_write_tokens += cw;
            }
        }

        // Gemini native generateContent / streamGenerateContent.
        if let Some(usage) = json.get("usageMetadata") {
            if let Some(input) = usage
                .get("promptTokenCount")
                .or_else(|| usage.get("prompt_token_count"))
                .and_then(|v| v.as_u64())
            {
                // Gemini's promptTokenCount includes cached content; split it out.
                let cached = usage
                    .get("cachedContentTokenCount")
                    .or_else(|| usage.get("cached_content_token_count"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                self.input_tokens = input.saturating_sub(cached);
                self.cache_read_tokens = cached;
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
            self.cache_read_tokens,
            self.cache_write_tokens,
            self.req_hash.as_deref(),
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
    request_hash: Option<String>,
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
        tracker.req_hash = request_hash;
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
        let bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
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
                    Some(&err.to_string()),
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
        state.req_hash = request_hash;

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

    fn tracker_for_cache_test() -> UsageTrackerState {
        let (_dir, logger) = create_test_logger();
        UsageTrackerState::new(
            "team1".to_string(),
            Some("req-1".to_string()),
            "r1".to_string(),
            None,
            "c1".to_string(),
            "m1".to_string(),
            logger,
            create_test_metrics(),
            Some(1.0),
            false,
        )
    }

    #[test]
    fn cache_openai_cached_tokens_split_out_of_prompt() {
        let mut t = tracker_for_cache_test();
        // OpenAI: cached is a subset of prompt_tokens → input must exclude it.
        t.extract_usage(&serde_json::json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_tokens_details": { "cached_tokens": 30 }
            }
        }));
        assert_eq!(t.input_tokens, 70);
        assert_eq!(t.cache_read_tokens, 30);
        assert_eq!(t.cache_write_tokens, 0);
        assert_eq!(t.output_tokens, 20);
    }

    #[test]
    fn cache_deepseek_hit_tokens_split_out_of_prompt() {
        let mut t = tracker_for_cache_test();
        // DeepSeek reports cache hits as `prompt_cache_hit_tokens` (subset of prompt).
        t.extract_usage(&serde_json::json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "prompt_cache_hit_tokens": 30,
                "prompt_cache_miss_tokens": 70
            }
        }));
        assert_eq!(t.input_tokens, 70);
        assert_eq!(t.cache_read_tokens, 30);
        assert_eq!(t.output_tokens, 20);
    }

    #[test]
    fn cache_anthropic_tokens_are_separate_from_input() {
        let mut t = tracker_for_cache_test();
        // Anthropic: input_tokens already excludes cache; cache is additive.
        t.extract_usage(&serde_json::json!({
            "usage": {
                "input_tokens": 40,
                "output_tokens": 5,
                "cache_read_input_tokens": 200,
                "cache_creation_input_tokens": 60
            }
        }));
        assert_eq!(t.input_tokens, 40);
        assert_eq!(t.cache_read_tokens, 200);
        assert_eq!(t.cache_write_tokens, 60);
    }

    #[test]
    fn cache_gemini_cached_content_split_out_of_prompt() {
        let mut t = tracker_for_cache_test();
        t.extract_usage(&serde_json::json!({
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 12,
                "cachedContentTokenCount": 25
            }
        }));
        assert_eq!(t.input_tokens, 75);
        assert_eq!(t.cache_read_tokens, 25);
        assert_eq!(t.output_tokens, 12);
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
            None,
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
    fn test_process_chunk_sse_multibyte_split() {
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

        // A realistic OpenAI stream: a content delta carrying a Chinese char,
        // then the final chunk carrying usage.
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"好\"}}],\"usage\":null}\n\ndata: {\"choices\":[],\"usage\":{\"prompt_tokens\":54,\"completion_tokens\":26}}\n\n";
        let bytes = sse.as_bytes();
        // Split the network chunk boundary in the MIDDLE of the 3-byte '好'.
        let split = sse.find('好').unwrap() + 1;
        tracker.process_chunk(&bytes[..split], true);
        tracker.process_chunk(&bytes[split..], true);

        assert_eq!(tracker.input_tokens, 54, "input tokens lost");
        assert_eq!(tracker.output_tokens, 26, "output tokens lost");
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
            0,
            0,
            None,
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
