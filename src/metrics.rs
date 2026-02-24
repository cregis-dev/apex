use anyhow::Context;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Registry, TextEncoder,
};

#[derive(Clone)]
pub struct MetricsState {
    registry: Registry,
    pub request_total: IntCounterVec,
    pub error_total: IntCounterVec,
    pub token_total: IntCounterVec,
    pub upstream_latency_ms: HistogramVec,
    pub fallback_total: IntCounterVec,
}

impl MetricsState {
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();
        let request_total = IntCounterVec::new(
            prometheus::Opts::new("apex_requests_total", "Gateway requests total"),
            &["route", "router"],
        )
        .context("create request_total")?;
        let error_total = IntCounterVec::new(
            prometheus::Opts::new("apex_errors_total", "Gateway errors total"),
            &["route", "router"],
        )
        .context("create error_total")?;
        let token_total = IntCounterVec::new(
            prometheus::Opts::new("apex_token_total", "Token usage total"),
            &["router", "channel", "model", "type"],
        )
        .context("create token_total")?;
        let upstream_latency_ms = HistogramVec::new(
            HistogramOpts::new("apex_upstream_latency_ms", "Upstream latency in ms"),
            &["route", "router", "channel"],
        )
        .context("create upstream_latency_ms")?;
        let fallback_total = IntCounterVec::new(
            prometheus::Opts::new("apex_fallback_total", "Gateway fallback total"),
            &["router", "channel"],
        )
        .context("create fallback_total")?;

        registry
            .register(Box::new(request_total.clone()))
            .context("register request_total")?;
        registry
            .register(Box::new(error_total.clone()))
            .context("register error_total")?;
        registry
            .register(Box::new(token_total.clone()))
            .context("register token_total")?;
        registry
            .register(Box::new(upstream_latency_ms.clone()))
            .context("register upstream_latency_ms")?;
        registry
            .register(Box::new(fallback_total.clone()))
            .context("register fallback_total")?;

        Ok(Self {
            registry,
            request_total,
            error_total,
            token_total,
            upstream_latency_ms,
            fallback_total,
        })
    }

    pub fn render(&self) -> anyhow::Result<String> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .context("encode metrics")?;
        String::from_utf8(buffer).context("metrics utf8")
    }
}
