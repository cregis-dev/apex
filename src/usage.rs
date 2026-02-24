use std::path::PathBuf;
use std::fs::OpenOptions;
use std::sync::{Arc, Mutex};
use chrono::Local;
use anyhow::Result;
use axum::body::{Body, Bytes};
use axum::response::Response;
use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use serde_json::Value;
use crate::metrics::MetricsState;

pub struct UsageLogger {
    writer: Mutex<csv::Writer<std::fs::File>>,
}

impl UsageLogger {
    pub fn new(log_dir: Option<String>) -> Result<Self> {
        let dir = if let Some(d) = log_dir {
            PathBuf::from(d)
        } else {
            PathBuf::from("logs")
        };

        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        
        let file_path = dir.join("usage.csv");
        let file_exists = file_path.exists();
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;

        let mut writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(file);

        if !file_exists {
            writer.write_record(&[
                "timestamp",
                "router",
                "channel",
                "model",
                "input_tokens",
                "output_tokens",
            ])?;
            writer.flush()?;
        }

        Ok(Self {
            writer: Mutex::new(writer),
        })
    }

    pub fn log(&self, router: &str, channel: &str, model: &str, input_tokens: u64, output_tokens: u64) {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_record(&[
                &timestamp,
                router,
                channel,
                model,
                &input_tokens.to_string(),
                &output_tokens.to_string(),
            ]);
            let _ = w.flush();
        }
    }
}

struct UsageTrackerState {
    router: String,
    channel: String,
    model: String,
    logger: Arc<UsageLogger>,
    metrics: Arc<MetricsState>,
    input_tokens: u64,
    output_tokens: u64,
    accumulated_data: String,
}

impl UsageTrackerState {
    fn new(router: String, channel: String, model: String, logger: Arc<UsageLogger>, metrics: Arc<MetricsState>) -> Self {
        Self {
            router,
            channel,
            model,
            logger,
            metrics,
            input_tokens: 0,
            output_tokens: 0,
            accumulated_data: String::new(),
        }
    }

    fn process_chunk(&mut self, chunk: &[u8], is_sse: bool) {
        if let Ok(s) = std::str::from_utf8(chunk) {
            if is_sse {
                self.accumulated_data.push_str(s);
                let mut start = 0;
                while let Some(end) = self.accumulated_data[start..].find('\n') {
                    let line = self.accumulated_data[start..start+end].to_string();
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
            if data.trim() == "[DONE]" { return; }
            if let Ok(json) = serde_json::from_str::<Value>(data) {
                self.extract_usage(&json);
            }
        }
    }

    fn extract_usage(&mut self, json: &Value) {
        // OpenAI / Generic
        if let Some(usage) = json.get("usage") {
            if let Some(prompt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                self.input_tokens = prompt; // OpenAI sends cumulative or final
            }
            if let Some(completion) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                self.output_tokens = completion;
            }
            // Anthropic in message_start
            if let Some(input) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                 self.input_tokens += input;
            }
            // Anthropic in message_delta
            if let Some(output) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                 self.output_tokens += output;
            }
        }
    }

    fn flush(&self) {
        if self.input_tokens > 0 || self.output_tokens > 0 {
             self.metrics.token_total
                .with_label_values(&[&self.router, &self.channel, &self.model, "input"])
                .inc_by(self.input_tokens);
             self.metrics.token_total
                .with_label_values(&[&self.router, &self.channel, &self.model, "output"])
                .inc_by(self.output_tokens);
             
             self.logger.log(&self.router, &self.channel, &self.model, self.input_tokens, self.output_tokens);
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

pub async fn wrap_response(
    response: Response<Body>,
    router: String,
    channel: String,
    model: String,
    logger: Arc<UsageLogger>,
    metrics: Arc<MetricsState>,
) -> Response<Body> {
    let is_sse = response.headers().get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/event-stream"))
        .unwrap_or(false);

    let (parts, body) = response.into_parts();

    if is_sse {
        let state = Arc::new(Mutex::new(UsageTrackerState::new(
            router, channel, model, logger, metrics
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
            router, channel, model, logger, metrics
        );
        
        if let Ok(json) = serde_json::from_slice::<Value>(&bytes) {
             state.extract_usage(&json);
             state.flush();
        }

        Response::from_parts(parts, Body::from(bytes))
    }
}
