use crate::database::Database;
use crate::metrics::MetricsState;
use anyhow::Result;
use axum::body::{Body, Bytes};
use axum::response::Response;
use futures::Stream;
use serde_json::Value;
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
        );
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
    latency_ms: Option<f64>,
    fallback_triggered: bool,
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
            );
        }
    }
}

pub struct UsageStream<S> {
    inner: S,
    state: Arc<Mutex<UsageTrackerState>>,
}

impl<S, E> Stream for UsageStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let poll = Pin::new(&mut self.inner).poll_next(cx);
        match poll {
            Poll::Ready(Some(Ok(bytes))) => {
                if let Ok(mut state) = self.state.lock() {
                    state.process_chunk(&bytes, true);
                }
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(None) => {
                // Stream finished
                if let Ok(state) = self.state.lock() {
                    state.flush();
                }
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn wrap_response(
    response: Response<Body>,
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
) -> Response<Body> {
    let is_sse = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/event-stream"))
        .unwrap_or(false);

    let (parts, body) = response.into_parts();

    if is_sse {
        let state = Arc::new(Mutex::new(UsageTrackerState::new(
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
        )));
        let stream = body.into_data_stream();
        let usage_stream = UsageStream {
            inner: stream,
            state,
        };
        Response::from_parts(parts, Body::from_stream(usage_stream))
    } else {
        // Non-SSE: read full body
        let bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => return Response::from_parts(parts, Body::empty()), // Should not happen often
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
    use tempfile::tempdir;

    fn create_test_metrics() -> Arc<MetricsState> {
        Arc::new(MetricsState::new().unwrap())
    }

    #[test]
    fn test_extract_usage_openai() {
        let db = Arc::new(Database::new(None).unwrap());
        let logger = Arc::new(UsageLogger::new(db));
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
        let db = Arc::new(Database::new(None).unwrap());
        let logger = Arc::new(UsageLogger::new(db));
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
    fn test_extract_usage_anthropic_message_delta() {
        let db = Arc::new(Database::new(None).unwrap());
        let logger = Arc::new(UsageLogger::new(db));
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
    fn test_process_sse_line() {
        let db = Arc::new(Database::new(None).unwrap());
        let logger = Arc::new(UsageLogger::new(db));
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
        let db = Arc::new(Database::new(None).unwrap());
        let logger = Arc::new(UsageLogger::new(db));
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
