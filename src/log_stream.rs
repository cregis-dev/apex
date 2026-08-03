//! In-process capture of the gateway's own log stream.
//!
//! `apex logs` tails `apex.log.*` on disk, which only exists when the gateway
//! was started with `gateway start -d`; the installed systemd/launchd service
//! runs `gateway run` in the foreground and writes no such file. This module
//! makes the log stream available regardless of how the process was started: a
//! [`LogLayer`] fans every event that passes the global filter into
//!
//!   * a bounded ring buffer — the backlog a freshly-opened viewer replays, and
//!   * a broadcast channel — the live push to already-connected viewers.
//!
//! Both live in one process-global [`LogStream`], matching the lifetime of the
//! (also global) tracing subscriber the layer is installed into.
//!
//! Only what the `fmt` layer would already have written to disk passes through
//! here — the global `EnvFilter` runs first, so raising verbosity still means
//! raising `logging.level` (or `RUST_LOG`).

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// Ring-buffer depth — how much history a freshly-opened viewer can replay.
const BUFFER_CAPACITY: usize = 2_000;
/// Per-viewer queue depth. A viewer that falls further behind than this gets a
/// `Lagged` notice rather than stalling the gateway; log capture never blocks
/// the request path.
const BROADCAST_CAPACITY: usize = 512;

/// One captured log event, shaped for the control plane's log view.
#[derive(Clone, Debug, serde::Serialize)]
pub struct LogEntry {
    /// Monotonic per-process sequence number. A reconnecting viewer passes the
    /// last one it saw as `after_seq` to resume without gaps or duplicates.
    pub seq: u64,
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
    /// Pulled off the enclosing `request` span, so a log line can be matched to
    /// a row in Live Tail / Records.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
}

impl LogEntry {
    /// True when this entry is at or above `min_level`; no filter accepts all.
    pub fn at_or_above(&self, min_level: Option<&str>) -> bool {
        match min_level {
            Some(min) => level_rank(&self.level) >= level_rank(min),
            None => true,
        }
    }

    /// Synthetic entry telling a viewer the server dropped lines because that
    /// viewer could not keep up. Surfaced in-band so the gap is visible rather
    /// than silently swallowed.
    pub fn lagged(count: u64) -> Self {
        Self {
            seq: 0,
            timestamp: chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string(),
            level: "WARN".to_string(),
            target: "apex::log_stream".to_string(),
            message: format!("[stream lagged — {count} lines dropped]"),
            request_id: None,
            team_id: None,
        }
    }
}

/// Severity rank for filtering; higher is more severe.
fn level_rank(level: &str) -> u8 {
    match level {
        "TRACE" => 0,
        "DEBUG" => 1,
        "INFO" => 2,
        "WARN" => 3,
        "ERROR" => 4,
        _ => 2,
    }
}

/// Ring buffer + broadcast fan-out. Cloneable handle semantics via `Arc`.
pub struct LogStream {
    buffer: Mutex<VecDeque<LogEntry>>,
    tx: broadcast::Sender<LogEntry>,
    next_seq: AtomicU64,
}

impl LogStream {
    fn new() -> Self {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(BUFFER_CAPACITY)),
            tx,
            next_seq: AtomicU64::new(1),
        }
    }

    /// Live feed of entries captured from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<LogEntry> {
        self.tx.subscribe()
    }

    /// Buffered history, oldest first: at most `limit` entries, keeping the
    /// **newest** when the backlog is longer, optionally narrowed to entries
    /// at or above `min_level` and newer than `after_seq`.
    pub fn recent(
        &self,
        limit: usize,
        min_level: Option<&str>,
        after_seq: Option<u64>,
    ) -> Vec<LogEntry> {
        let Ok(buffer) = self.buffer.lock() else {
            return Vec::new();
        };
        let matching = buffer
            .iter()
            .filter(|e| e.at_or_above(min_level))
            .filter(|e| after_seq.is_none_or(|seq| e.seq > seq));
        // Keep the tail: on first load the newest lines are the interesting ones.
        let count = matching.clone().count();
        matching
            .skip(count.saturating_sub(limit))
            .cloned()
            .collect()
    }

    fn push(&self, mut entry: LogEntry) {
        entry.seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut buffer) = self.buffer.lock() {
            if buffer.len() == BUFFER_CAPACITY {
                buffer.pop_front();
            }
            buffer.push_back(entry.clone());
        }
        // Errs only when nobody is watching, which is the common case.
        let _ = self.tx.send(entry);
    }
}

/// The process-wide log stream, created on first use.
pub fn global() -> &'static Arc<LogStream> {
    static GLOBAL: OnceLock<Arc<LogStream>> = OnceLock::new();
    GLOBAL.get_or_init(|| Arc::new(LogStream::new()))
}

/// Span fields worth carrying onto the events recorded inside that span.
/// Populated at span creation and topped up by later `record` calls (the
/// `request` span declares `team_id` as `Empty` and fills it in after auth).
#[derive(Default, Clone)]
struct SpanFields {
    request_id: Option<String>,
    team_id: Option<String>,
}

impl Visit for SpanFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "request_id" => self.request_id = Some(value.to_string()),
            "team_id" => self.team_id = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // `%foo` / `?foo` fields arrive here rather than as `record_str`.
        match field.name() {
            "request_id" | "team_id" => {
                let rendered = format!("{value:?}");
                // Debug-formatted strings arrive quoted; unwrap for display.
                let cleaned = rendered.trim_matches('"').to_string();
                if field.name() == "request_id" {
                    self.request_id = Some(cleaned);
                } else {
                    self.team_id = Some(cleaned);
                }
            }
            _ => {}
        }
    }
}

/// Collects an event's `message` plus any other fields as `key=value` tail.
#[derive(Default)]
struct EventVisitor {
    message: String,
    fields: String,
}

impl EventVisitor {
    fn finish(mut self) -> String {
        if !self.fields.is_empty() {
            if self.message.is_empty() {
                return self.fields;
            }
            let _ = write!(self.message, " {}", self.fields);
        }
        self.message
    }

    fn record(&mut self, name: &str, value: &str) {
        if name == "message" {
            self.message = value.to_string();
        } else {
            if !self.fields.is_empty() {
                self.fields.push(' ');
            }
            let _ = write!(self.fields, "{name}={value}");
        }
    }
}

impl Visit for EventVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record(field.name(), value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.record(field.name(), &format!("{value:?}"));
    }
}

/// `tracing` layer that mirrors events into the process-global [`LogStream`].
pub struct LogLayer;

impl<S> Layer<S> for LogLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        ctx: Context<'_, S>,
    ) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = SpanFields::default();
        attrs.record(&mut fields);
        span.extensions_mut().insert(fields);
    }

    fn on_record(&self, id: &tracing::Id, values: &tracing::span::Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut extensions = span.extensions_mut();
        if let Some(fields) = extensions.get_mut::<SpanFields>() {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        // Nearest enclosing span that carries request context wins.
        let mut request_id = None;
        let mut team_id = None;
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope {
                if let Some(fields) = span.extensions().get::<SpanFields>() {
                    if request_id.is_none() {
                        request_id.clone_from(&fields.request_id);
                    }
                    if team_id.is_none() {
                        team_id.clone_from(&fields.team_id);
                    }
                }
                if request_id.is_some() && team_id.is_some() {
                    break;
                }
            }
        }

        let metadata = event.metadata();
        global().push(LogEntry {
            seq: 0, // assigned under the buffer lock so it stays monotonic
            timestamp: chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string(),
            level: metadata.level().to_string(),
            target: metadata.target().to_string(),
            message: visitor.finish(),
            request_id,
            team_id,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seq_hint: &str, level: &str) -> LogEntry {
        LogEntry {
            seq: 0,
            timestamp: "2026-08-03 12:00:00.000".to_string(),
            level: level.to_string(),
            target: "apex::test".to_string(),
            message: seq_hint.to_string(),
            request_id: None,
            team_id: None,
        }
    }

    #[test]
    fn push_assigns_monotonic_sequence_numbers() {
        let stream = LogStream::new();
        stream.push(entry("a", "INFO"));
        stream.push(entry("b", "INFO"));
        let recent = stream.recent(10, None, None);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].message, "a");
        assert_eq!(recent[1].message, "b");
        assert!(recent[1].seq > recent[0].seq);
    }

    #[test]
    fn recent_keeps_the_newest_when_over_limit() {
        let stream = LogStream::new();
        for i in 0..10 {
            stream.push(entry(&i.to_string(), "INFO"));
        }
        let recent = stream.recent(3, None, None);
        let messages: Vec<&str> = recent.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["7", "8", "9"]);
    }

    #[test]
    fn recent_filters_by_level_and_cursor() {
        let stream = LogStream::new();
        stream.push(entry("info-1", "INFO"));
        stream.push(entry("warn-1", "WARN"));
        stream.push(entry("error-1", "ERROR"));

        let warn_and_up = stream.recent(10, Some("WARN"), None);
        let messages: Vec<&str> = warn_and_up.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["warn-1", "error-1"]);

        // Resume after the first entry.
        let after_first = stream.recent(10, None, Some(1));
        let messages: Vec<&str> = after_first.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["warn-1", "error-1"]);
    }

    #[test]
    fn ring_buffer_is_bounded() {
        let stream = LogStream::new();
        for i in 0..(BUFFER_CAPACITY + 50) {
            stream.push(entry(&i.to_string(), "INFO"));
        }
        assert_eq!(stream.buffer.lock().unwrap().len(), BUFFER_CAPACITY);
        // Oldest entries were evicted, newest retained.
        let recent = stream.recent(1, None, None);
        assert_eq!(recent[0].message, (BUFFER_CAPACITY + 49).to_string());
    }

    #[test]
    fn subscribers_receive_pushed_entries() {
        let stream = LogStream::new();
        let mut rx = stream.subscribe();
        stream.push(entry("live", "INFO"));
        let got = rx.try_recv().expect("entry delivered to subscriber");
        assert_eq!(got.message, "live");
    }
}
