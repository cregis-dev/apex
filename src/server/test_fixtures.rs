//! Pricing-config fixtures shared by the dashboard cost tests and the
//! `validate_pricing` test that still lives beside the admin pricing endpoint.
//! Kept in one place so neither side has to duplicate them.

use crate::config::Channel;

pub(crate) fn price_row(
    match_pattern: &str,
    input: f64,
    output: f64,
    cache_read: Option<f64>,
) -> crate::config::ModelPrice {
    crate::config::ModelPrice {
        match_pattern: match_pattern.to_string(),
        input,
        output,
        cache_read,
        cache_write: None,
    }
}

/// A PAYG rule with a single `*` rate-card row (the flat case).
pub(crate) fn payg_rule(
    name: &str,
    input: f64,
    output: f64,
    cache_read: Option<f64>,
) -> crate::config::PricingRule {
    crate::config::PricingRule {
        name: name.to_string(),
        kind: "payg".to_string(),
        prices: vec![price_row("*", input, output, cache_read)],
        monthly_fee: 0.0,
        billing_day: 1,
        included_quota_tokens: None,
    }
}

pub(crate) fn payg_card(
    name: &str,
    rows: Vec<crate::config::ModelPrice>,
) -> crate::config::PricingRule {
    crate::config::PricingRule {
        name: name.to_string(),
        kind: "payg".to_string(),
        prices: rows,
        monthly_fee: 0.0,
        billing_day: 1,
        included_quota_tokens: None,
    }
}

pub(crate) fn sub_rule(
    name: &str,
    monthly_fee: f64,
    quota: Option<u64>,
) -> crate::config::PricingRule {
    crate::config::PricingRule {
        name: name.to_string(),
        kind: "subscription".to_string(),
        prices: vec![],
        monthly_fee,
        billing_day: 1,
        included_quota_tokens: quota,
    }
}

pub(crate) fn pricing_of(rules: Vec<crate::config::PricingRule>) -> crate::config::Pricing {
    crate::config::Pricing {
        currency: "USD".to_string(),
        unit: 1_000_000.0,
        rules,
    }
}

/// A channel named `name` that bills under pricing rule `rule`.
pub(crate) fn priced_channel(name: &str, rule: &str) -> Channel {
    Channel {
        name: name.to_string(),
        provider_type: crate::config::ProviderType::Openai,
        base_url: "x".to_string(),
        api_key: "x".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
        pricing: Some(rule.to_string()),
    }
}

// ---- gateway test doubles and AppState construction --------------------

use std::sync::{Arc, Mutex, RwLock};
use tempfile::{TempDir, tempdir};

use crate::config::{Config, Global, ProviderType, Retries, Timeouts};
use crate::database::Database;
use crate::gemini_compat::GeminiAnthropicReplayCache;
use crate::metrics::MetricsState;
use crate::middleware::ratelimit::TeamRateLimiter;
use crate::providers::{AccessAudit, ProviderRegistry, RateLimiter, RouteKind};
use crate::router_selector::RouterSelector;
use crate::server::AppState;
use crate::usage::UsageLogger;

pub(crate) struct MockAccessAudit {
    pub(crate) calls: Arc<Mutex<Vec<(ProviderType, bool)>>>,
}

impl AccessAudit for MockAccessAudit {
    fn audit(&self, provider: &ProviderType, _route: RouteKind, success: bool) {
        self.calls.lock().unwrap().push((provider.clone(), success));
    }
}

pub(crate) struct MockRateLimiter {
    pub(crate) allow: bool,
}

impl RateLimiter for MockRateLimiter {
    fn check(&self, _provider: &ProviderType) -> bool {
        self.allow
    }
}

pub(crate) fn create_test_config() -> Config {
    Config {
        version: "1".to_string(),
        global: Global {
            listen: "0.0.0.0:0".to_string(),
            auth_keys: vec![],
            timeouts: Timeouts {
                connect_ms: 100,
                request_ms: 100,
                response_ms: 100,
            },
            retries: Retries {
                max_attempts: 1,
                backoff_ms: 10,
                retry_on_status: vec![],
            },
            gemini_replay: crate::config::GeminiReplay::default(),
            cors_allowed_origins: vec![],
        },
        data_dir: "/tmp".to_string(),
        web_dir: "target/web".to_string(),
        metrics: crate::config::Metrics {
            enabled: false,
            path: "/metrics".to_string(),
        },
        hot_reload: crate::config::HotReload {
            config_path: "test.json".to_string(),
            watch: false,
        },
        logging: crate::config::Logging {
            level: "info".to_string(),
            dir: None,
        },
        teams: Arc::new(vec![]),
        compliance: None,
        retention: Default::default(),
        pricing: None,
        profiling: None,
        channels: Arc::new(vec![
            crate::config::Channel {
                name: "test-channel".to_string(),
                provider_type: ProviderType::Openai,
                base_url: "http://localhost:8080".to_string(),
                api_key: "sk-test".to_string(),
                anthropic_base_url: None,
                headers: None,
                model_map: None,
                timeouts: None,
                pricing: None,
            },
            crate::config::Channel {
                name: "test-channel-2".to_string(),
                provider_type: ProviderType::Anthropic,
                base_url: "http://localhost:8080".to_string(),
                api_key: "sk-test".to_string(),
                anthropic_base_url: None,
                headers: None,
                model_map: None,
                timeouts: None,
                pricing: None,
            },
        ]),
        routers: Arc::new(vec![crate::config::Router {
            name: "test-router".to_string(),
            rules: vec![crate::config::RouterRule {
                session_affinity: false,
                match_spec: crate::config::MatchSpec {
                    models: vec!["*".to_string()],
                },
                channels: vec![crate::config::TargetChannel {
                    name: "test-channel".to_string(),
                    weight: 1,
                }],
                strategy: "round_robin".to_string(),
            }],
            channels: vec![crate::config::TargetChannel {
                name: "test-channel".to_string(),
                weight: 1,
            }],
            strategy: "round_robin".to_string(),
            metadata: None,
            fallback_channels: vec![],
        }]),
    }
}

pub(crate) fn create_test_database() -> (TempDir, Arc<Database>) {
    let dir = tempdir().unwrap();
    let db = Arc::new(Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap());
    (dir, db)
}

pub(crate) fn state_with_config(config: Config) -> (Arc<AppState>, TempDir) {
    let (dir, database) = create_test_database();
    let state = Arc::new(AppState {
        config: Arc::new(RwLock::new(config)),
        metrics: Arc::new(MetricsState::new().unwrap()),
        providers: Arc::new(ProviderRegistry::new()),
        access_audit: Arc::new(MockAccessAudit {
            calls: Arc::new(Mutex::new(Vec::new())),
        }),
        rate_limiter: Arc::new(MockRateLimiter { allow: true }),
        team_rate_limiter: Arc::new(TeamRateLimiter::new()),
        selector: Arc::new(RouterSelector::new()),
        gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
        client: reqwest::Client::new(),
        usage_logger: Arc::new(UsageLogger::new(database.clone())),
        database,
        web_dir: "target/web".to_string(),
    });
    (state, dir)
}
