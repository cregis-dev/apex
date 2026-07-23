use glob::{MatchOptions, Pattern};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: String,
    pub global: Global,
    #[serde(default)]
    pub logging: Logging,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    // Legacy-only: filesystem asset override remains readable for old configs,
    // but should no longer be emitted in supported config files.
    #[serde(default = "default_web_dir", skip_serializing)]
    pub web_dir: String,
    #[serde(default)]
    pub channels: Arc<Vec<Channel>>,
    #[serde(default)]
    pub routers: Arc<Vec<Router>>,
    pub metrics: Metrics,
    pub hot_reload: HotReload,
    #[serde(default)]
    pub teams: Arc<Vec<Team>>,
    #[serde(default)]
    pub compliance: Option<Compliance>,
    #[serde(default)]
    pub retention: Retention,
    /// Optional per-model reference prices used to compute request cost in the
    /// dashboard. Absent ⇒ no cost is shown (graceful degradation).
    #[serde(default)]
    pub pricing: Option<Pricing>,
    /// Optional behavior-profiling / abuse-detection settings. Absent ⇒ feature
    /// off: no request fingerprinting, no rollup aggregation, no governance UI.
    #[serde(default)]
    pub profiling: Option<Profiling>,
}

/// Controls pruning of usage history and request/error/latency metrics so the
/// SQLite file stays bounded over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Retention {
    /// Days of history to keep. Rows older than this are pruned by a background
    /// task. `0` disables pruning entirely (keep forever).
    #[serde(default = "default_retention_days")]
    pub days: u64,
    /// How often the pruning task runs, in hours.
    #[serde(default = "default_retention_interval_hours")]
    pub interval_hours: u64,
}

fn default_retention_days() -> u64 {
    90
}

fn default_retention_interval_hours() -> u64 {
    24
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            days: default_retention_days(),
            interval_hours: default_retention_interval_hours(),
        }
    }
}

/// A set of named, independent pricing rules. A channel selects one by name
/// (see `Channel.pricing`) — pricing is per-channel, not matched by model.
/// Rates are per `unit` tokens (default 1M).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pricing {
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_price_unit")]
    pub unit: f64,
    #[serde(default)]
    pub rules: Vec<PricingRule>,
}

/// One named pricing rule. `type` is `"payg"` (a rate card: per-model rows,
/// first match wins) or `"subscription"` (a fixed monthly fee, no per-token rates).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingRule {
    pub name: String,
    #[serde(rename = "type", default = "default_rule_kind")]
    pub kind: String,
    /// PAYG: per-model price rows. A model may be priced differently within the
    /// same channel (e.g. DeepSeek V4-Flash vs V4-Pro). First match wins; put a
    /// `*` row last as a fallback.
    #[serde(default)]
    pub prices: Vec<ModelPrice>,
    /// Subscription only: fixed monthly fee.
    #[serde(default)]
    pub monthly_fee: f64,
    /// Subscription only: day of month the plan renews (for month-to-date proration).
    #[serde(default = "default_billing_day")]
    pub billing_day: u32,
    /// Subscription only: optional fair-use token ceiling.
    #[serde(default)]
    pub included_quota_tokens: Option<u64>,
}

/// One row in a PAYG rule's rate card. `match` is an exact (case-insensitive)
/// or glob model pattern, matched like team `allowed_models`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPrice {
    #[serde(rename = "match")]
    pub match_pattern: String,
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

fn default_currency() -> String {
    "USD".to_string()
}

fn default_price_unit() -> f64 {
    1_000_000.0
}

fn default_rule_kind() -> String {
    "payg".to_string()
}

fn default_billing_day() -> u32 {
    1
}

impl Pricing {
    /// The rule with the given name, if any.
    pub fn rule(&self, name: &str) -> Option<&PricingRule> {
        self.rules.iter().find(|r| r.name == name)
    }
}

impl PricingRule {
    pub fn is_subscription(&self) -> bool {
        self.kind == "subscription"
    }

    /// First price row whose `match` matches `model` (exact case-insensitive,
    /// then glob). None when nothing matches (that model is then untracked).
    pub fn price_for(&self, model: &str) -> Option<&ModelPrice> {
        self.prices.iter().find(|p| {
            p.match_pattern.eq_ignore_ascii_case(model)
                || Pattern::new(&p.match_pattern).is_ok_and(|pat| {
                    pat.matches_with(
                        model,
                        MatchOptions {
                            case_sensitive: false,
                            require_literal_separator: false,
                            require_literal_leading_dot: false,
                        },
                    )
                })
        })
    }
}

impl ModelPrice {
    /// cache_read defaults to 0 (unset ⇒ not billed separately).
    pub fn cache_read_rate(&self) -> f64 {
        self.cache_read.unwrap_or(0.0)
    }

    /// cache_write (creation) defaults to the input rate when unset.
    pub fn cache_write_rate(&self) -> f64 {
        self.cache_write.unwrap_or(self.input)
    }
}

fn default_data_dir() -> String {
    dirs::home_dir()
        .map(|p| p.join(".apex/data").to_string_lossy().to_string())
        .unwrap_or_else(|| "~/.apex/data".to_string())
}

fn default_web_dir() -> String {
    "target/web".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub api_key: String,
    pub policy: TeamPolicy,
    /// Optional group label used by the control plane to organize teams in
    /// the UI. Free-form string (e.g. "engineering", "data-platform").
    /// Defaults to `None` (rendered as "Default").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Whether this team is currently allowed to make requests. `None`
    /// behaves like `Some(true)` for backward compatibility; `Some(false)`
    /// hard-pauses the team — all model requests are rejected before they
    /// reach the upstream provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

impl Team {
    /// True when the team is paused (`enabled == Some(false)`). Missing /
    /// `Some(true)` are both considered active.
    pub fn is_paused(&self) -> bool {
        matches!(self.enabled, Some(false))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamPolicy {
    pub allowed_routers: Vec<String>,
    #[serde(default)]
    pub allowed_models: Option<Vec<String>>,
    #[serde(default)]
    pub rate_limit: Option<TeamRateLimit>,
}

impl TeamPolicy {
    pub fn is_model_allowed(&self, model: &str) -> bool {
        match &self.allowed_models {
            None => true,
            Some(patterns) => {
                if patterns.is_empty() {
                    return true;
                }
                patterns.iter().any(|pattern_str| {
                    // 1. Exact match (case-insensitive)
                    if pattern_str.eq_ignore_ascii_case(model) {
                        return true;
                    }
                    // 2. Glob match (case-insensitive)
                    if let Ok(pattern) = Pattern::new(pattern_str) {
                        pattern.matches_with(
                            model,
                            MatchOptions {
                                case_sensitive: false,
                                require_literal_separator: false,
                                require_literal_leading_dot: false,
                            },
                        )
                    } else {
                        false
                    }
                })
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamRateLimit {
    pub rpm: Option<i32>,
    pub tpm: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_dir")]
    pub dir: Option<String>,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_dir() -> Option<String> {
    None
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            dir: default_log_dir(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Global {
    pub listen: String,
    #[serde(default)]
    pub auth_keys: Vec<String>,
    pub timeouts: Timeouts,
    pub retries: Retries,
    #[serde(default)]
    pub gemini_replay: GeminiReplay,
    #[serde(default)]
    pub cors_allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiReplay {
    #[serde(default = "default_gemini_replay_ttl_hours")]
    pub ttl_hours: u64,
}

fn default_gemini_replay_ttl_hours() -> u64 {
    24
}

impl Default for GeminiReplay {
    fn default() -> Self {
        Self {
            ttl_hours: default_gemini_replay_ttl_hours(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeouts {
    pub connect_ms: u64,
    pub request_ms: u64,
    pub response_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Retries {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub retry_on_status: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub name: String,
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: String,
    pub anthropic_base_url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub model_map: Option<HashMap<String, String>>,
    pub timeouts: Option<Timeouts>,
    /// Name of the `pricing` rule this channel bills under. `None` ⇒ untracked
    /// (no cost computed). The rule decides pay-as-you-go vs subscription.
    #[serde(default)]
    pub pricing: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    Openai,
    Anthropic,
    Gemini,
    CustomDual,
    Deepseek,
    Moonshot,
    Minimax,
    Ollama,
    Jina,
    Openrouter,
    Zai,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Router {
    pub name: String,

    // New unified rules configuration
    #[serde(default)]
    pub rules: Vec<RouterRule>,

    // Legacy fields (kept for backward compatibility, will be migrated to rules)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<TargetChannel>,
    #[serde(
        default = "default_strategy",
        skip_serializing_if = "is_default_strategy"
    )]
    pub strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<RouterMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterRule {
    #[serde(rename = "match")]
    pub match_spec: MatchSpec,
    pub channels: Vec<TargetChannel>,
    #[serde(default = "default_strategy")]
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchSpec {
    #[serde(default, deserialize_with = "string_or_vec", alias = "model")]
    pub models: Vec<String>,
}

fn string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::String(s) => Ok(vec![s]),
        StringOrVec::Vec(v) => Ok(v),
    }
}

fn default_strategy() -> String {
    "round_robin".to_string()
}

fn is_default_strategy(s: &String) -> bool {
    s == "round_robin"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetChannel {
    pub name: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
}

fn default_weight() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterMetadata {
    pub model_matcher: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    pub enabled: bool,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotReload {
    pub config_path: String,
    pub watch: bool,
}

/// PII action type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PiiAction {
    Mask,
    Block,
}

/// PII rule for detection and handling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiiRule {
    pub name: String,
    pub pattern: String,
    #[serde(default = "default_pii_action")]
    pub action: PiiAction,
    #[serde(default = "default_mask_char")]
    pub mask_char: char,
    #[serde(default)]
    pub replace_with: Option<String>,
}

fn default_pii_action() -> PiiAction {
    PiiAction::Mask
}

fn default_mask_char() -> char {
    '*'
}

/// Compliance configuration for PII masking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Compliance {
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<PiiRule>,
}

impl Compliance {
    /// Validate compliance configuration
    pub fn validate(&self) -> Result<(), String> {
        use regex::Regex;
        use std::collections::HashSet;

        // Check for duplicate rule names
        let mut seen_names = HashSet::new();
        for rule in &self.rules {
            if !seen_names.insert(&rule.name) {
                return Err(format!("Duplicate rule name: {}", rule.name));
            }

            // Validate regex pattern
            if let Err(e) = Regex::new(&rule.pattern) {
                return Err(format!("Invalid regex in rule '{}': {}", rule.name, e));
            }
        }

        Ok(())
    }
}

fn default_true() -> bool {
    true
}

/// Behavior-profiling / abuse-detection configuration. Targets *waste-type*
/// abuse — loops, idle spinning, retry storms, off-hours automation — using
/// request metadata plus a one-way request fingerprint (never the prompt text).
/// Optional and off by default; see `docs/design/behavior-profiling.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profiling {
    /// Master switch. When false the whole feature is inert.
    #[serde(default)]
    pub enabled: bool,
    /// Fingerprint request payloads into `usage_records.req_hash` (hash only,
    /// never the prompt text) to power repeat-rate detection. An independent
    /// privacy switch; defaults on when profiling is enabled.
    #[serde(default = "default_true")]
    pub hash_requests: bool,
    /// Pre-aggregation (rollup) job settings — the substrate baselines read from.
    #[serde(default)]
    pub rollup: RollupConfig,
    /// Detection thresholds. Parsed and validated now; consumed by the detection
    /// layer (a later phase). Present here so the wire shape is locked.
    #[serde(default)]
    pub thresholds: Thresholds,
}

impl Profiling {
    /// Reject nonsensical settings early: out-of-range ratios, zero/oversized
    /// intervals, and a lookback too short for the run interval (which would
    /// leave permanent holes in the rollup baseline).
    pub fn validate(&self) -> Result<(), String> {
        let r = &self.rollup;
        // Sane upper bounds so pathologically large values can't overflow the
        // `chrono::Duration` casts in the rollup task (which would silently kill
        // the tick). Generous ceilings — years, not a policy.
        if r.interval_minutes == 0 || r.interval_minutes > 43_200 {
            return Err("rollup.interval_minutes must be within [1, 43200] (30 days)".into());
        }
        if r.lookback_hours == 0 || r.lookback_hours > 8_784 {
            return Err("rollup.lookback_hours must be within [1, 8784] (1 year)".into());
        }
        if r.retention_days > 36_500 {
            return Err("rollup.retention_days must be <= 36500 (100 years)".into());
        }
        // The trailing window each run recomputes must cover the gap since the
        // previous run, or records in the tail of every cycle are never rolled up.
        if r.lookback_hours * 60 < r.interval_minutes {
            return Err(
                "rollup.lookback_hours * 60 must be >= rollup.interval_minutes so no window is skipped".into(),
            );
        }
        let t = &self.thresholds;
        if t.min_samples > 1_000_000_000 {
            return Err("thresholds.min_samples must be <= 1_000_000_000".into());
        }
        for (name, value) in [
            ("repeat_rate", t.repeat_rate),
            ("error_ratio", t.error_ratio),
            ("output_zero_ratio", t.output_zero_ratio),
            ("night_ratio", t.night_ratio),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return Err(format!("thresholds.{name} must be within [0, 1]"));
            }
        }
        for (name, value) in [
            ("rate_spike_z", t.rate_spike_z),
            ("spend_spike_z", t.spend_spike_z),
        ] {
            if value < 0.0 {
                return Err(format!("thresholds.{name} must be >= 0"));
            }
        }
        Ok(())
    }
}

/// Hourly rollup pre-aggregation job settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupConfig {
    /// How often the rollup job runs, in minutes.
    #[serde(default = "default_rollup_interval_minutes")]
    pub interval_minutes: u64,
    /// Trailing window re-aggregated on each run (hours) to absorb late writes.
    #[serde(default = "default_rollup_lookback_hours")]
    pub lookback_hours: u64,
    /// Days of rollup buckets to keep. Independent of raw-record retention so
    /// baselines can look back past the raw-record horizon. `0` = keep forever.
    #[serde(default = "default_rollup_retention_days")]
    pub retention_days: u64,
}

fn default_rollup_interval_minutes() -> u64 {
    15
}
fn default_rollup_lookback_hours() -> u64 {
    3
}
fn default_rollup_retention_days() -> u64 {
    400
}

impl Default for RollupConfig {
    fn default() -> Self {
        Self {
            interval_minutes: default_rollup_interval_minutes(),
            lookback_hours: default_rollup_lookback_hours(),
            retention_days: default_rollup_retention_days(),
        }
    }
}

/// Detection thresholds. All have documented defaults; the detection layer reads
/// them, this phase only parses and validates them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    /// Window share of requests that are repeats (same user, same fingerprint).
    #[serde(default = "default_repeat_rate")]
    pub repeat_rate: f64,
    /// Request-rate z-score vs the member's own rolling baseline.
    #[serde(default = "default_rate_spike_z")]
    pub rate_spike_z: f64,
    /// Window share of requests that errored.
    #[serde(default = "default_error_ratio")]
    pub error_ratio: f64,
    /// Window share of successful requests producing ~zero output tokens.
    #[serde(default = "default_output_zero_ratio")]
    pub output_zero_ratio: f64,
    /// Reference-cost z-score vs the member's own rolling baseline.
    #[serde(default = "default_spend_spike_z")]
    pub spend_spike_z: f64,
    /// Window share of requests landing in non-working (night) hours.
    #[serde(default = "default_night_ratio")]
    pub night_ratio: f64,
    /// Minimum sample count before a member is judged (suppresses false positives).
    #[serde(default = "default_min_samples")]
    pub min_samples: u64,
}

fn default_repeat_rate() -> f64 {
    0.5
}
fn default_rate_spike_z() -> f64 {
    3.0
}
fn default_error_ratio() -> f64 {
    0.3
}
fn default_output_zero_ratio() -> f64 {
    0.4
}
fn default_spend_spike_z() -> f64 {
    3.0
}
fn default_night_ratio() -> f64 {
    0.6
}
fn default_min_samples() -> u64 {
    50
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            repeat_rate: default_repeat_rate(),
            rate_spike_z: default_rate_spike_z(),
            error_ratio: default_error_ratio(),
            output_zero_ratio: default_output_zero_ratio(),
            spend_spike_z: default_spend_spike_z(),
            night_ratio: default_night_ratio(),
            min_samples: default_min_samples(),
        }
    }
}

pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    let content = fs::read_to_string(path)?;
    let mut config = serde_json::from_str::<Config>(&content)?;

    // Validate compliance configuration if present
    if let Some(ref compliance) = config.compliance {
        compliance
            .validate()
            .map_err(|e| anyhow::anyhow!("Invalid compliance config: {}", e))?;
    }

    // Validate profiling configuration if present
    if let Some(ref profiling) = config.profiling {
        profiling
            .validate()
            .map_err(|e| anyhow::anyhow!("Invalid profiling config: {}", e))?;
    }

    // Migrate legacy configuration to rules
    for router in std::sync::Arc::make_mut(&mut config.routers) {
        if router.rules.is_empty() {
            // 1. Convert metadata.model_matcher to rules
            if let Some(metadata) = &router.metadata {
                for (pattern, target_channel_name) in &metadata.model_matcher {
                    router.rules.push(RouterRule {
                        match_spec: MatchSpec {
                            models: vec![pattern.clone()],
                        },
                        channels: vec![TargetChannel {
                            name: target_channel_name.clone(),
                            weight: 1,
                        }],
                        strategy: "priority".to_string(), // Single channel implies priority/direct
                    });
                }
            }

            // 2. Convert top-level channels to a default wildcard rule
            if !router.channels.is_empty() {
                router.rules.push(RouterRule {
                    match_spec: MatchSpec {
                        models: vec!["*".to_string()],
                    },
                    channels: router.channels.clone(),
                    strategy: router.strategy.clone(),
                });
            }
        }
    }

    Ok(config)
}

/// Strings used as placeholder admin keys by `install-release.sh`, `install.sh`,
/// `config.example.json`, and the original v0.4.2 default config. These are
/// shipped verbatim; without this guard a user who never edits the file would
/// have a guessable preset key live on `0.0.0.0:12356`. See v0.4.4 changelog.
pub const PLACEHOLDER_AUTH_KEYS: &[&str] = &[
    "replace-with-admin-key",
    "replace-with-dashboard-admin-key",
    "REPLACE-WITH-YOUR-ADMIN-KEY",
    "sk-your-secret-key-here",
];

pub const PLACEHOLDER_TEAM_KEYS: &[&str] = &[
    "replace-with-team-api-key",
    "REPLACE-WITH-YOUR-TEAM-API-KEY",
    "sk-team-demo-key",
];

/// Returns Err with a multi-line, human-readable message if any auth key or
/// team api key matches one of the placeholder strings shipped in our install
/// templates. Called at gateway startup and on hot-reload so the server fails
/// closed instead of accepting the preset string as a valid credential.
pub fn check_no_placeholder_credentials(config: &Config) -> anyhow::Result<()> {
    let mut violations: Vec<String> = Vec::new();
    for (i, key) in config.global.auth_keys.iter().enumerate() {
        if PLACEHOLDER_AUTH_KEYS.contains(&key.as_str()) {
            violations.push(format!("global.auth_keys[{i}] = {key:?}"));
        }
    }
    for team in config.teams.iter() {
        if PLACEHOLDER_TEAM_KEYS.contains(&team.api_key.as_str()) {
            violations.push(format!(
                "teams[id={}].api_key = {:?}",
                team.id, team.api_key
            ));
        }
    }
    if violations.is_empty() {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "refusing to load config: placeholder credentials present (they would be accepted as real keys by auth middleware)\n  - {}\nedit the file printed by `apex config path` and replace these strings with real secrets",
        violations.join("\n  - ")
    ))
}

pub fn save_config(path: &Path, config: &Config) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config)?;
    fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Channel, Config, PLACEHOLDER_AUTH_KEYS, PLACEHOLDER_TEAM_KEYS, Pricing, ProviderType,
        check_no_placeholder_credentials,
    };

    fn parse_config(json: &str) -> Config {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn profiling_parses_with_documented_defaults() {
        use super::Profiling;
        // Only `enabled` provided — everything else fills from defaults.
        let p: Profiling = serde_json::from_str(r#"{"enabled": true}"#).unwrap();
        assert!(p.enabled);
        assert!(p.hash_requests, "hash_requests defaults on");
        assert_eq!(p.rollup.interval_minutes, 15);
        assert_eq!(p.rollup.lookback_hours, 3);
        assert_eq!(p.rollup.retention_days, 400);
        assert_eq!(p.thresholds.repeat_rate, 0.5);
        assert_eq!(p.thresholds.min_samples, 50);
        p.validate().expect("defaults are valid");
    }

    #[test]
    fn profiling_defaults_to_disabled_and_absent() {
        // Absent config section ⇒ feature off.
        let cfg = config_with(&[], &[]);
        assert!(cfg.profiling.is_none());
        // Present but `enabled` omitted ⇒ off, hashing still defaults on.
        let p: super::Profiling = serde_json::from_str("{}").unwrap();
        assert!(!p.enabled);
        assert!(p.hash_requests);
    }

    #[test]
    fn profiling_validate_rejects_bad_values() {
        use super::Profiling;
        let bad_ratio: Profiling =
            serde_json::from_str(r#"{"enabled":true,"thresholds":{"repeat_rate":1.5}}"#).unwrap();
        assert!(bad_ratio.validate().is_err());

        let bad_z: Profiling =
            serde_json::from_str(r#"{"enabled":true,"thresholds":{"rate_spike_z":-1.0}}"#).unwrap();
        assert!(bad_z.validate().is_err());

        let bad_interval: Profiling =
            serde_json::from_str(r#"{"enabled":true,"rollup":{"interval_minutes":0}}"#).unwrap();
        assert!(bad_interval.validate().is_err());
    }

    #[test]
    fn profiling_validate_rejects_lookback_shorter_than_interval() {
        use super::Profiling;
        // 1h lookback (60 min) < 240 min interval ⇒ the rollup would skip records
        // written in the 60min–240min tail of every cycle.
        let gap: Profiling = serde_json::from_str(
            r#"{"enabled":true,"rollup":{"interval_minutes":240,"lookback_hours":1}}"#,
        )
        .unwrap();
        assert!(gap.validate().is_err());
        // A pairing where the trailing window covers the interval passes.
        let ok: Profiling = serde_json::from_str(
            r#"{"enabled":true,"rollup":{"interval_minutes":60,"lookback_hours":3}}"#,
        )
        .unwrap();
        assert!(ok.validate().is_ok());
    }

    fn config_with(auth_keys: &[&str], teams: &[(&str, &str)]) -> Config {
        let auth_json = serde_json::to_string(auth_keys).unwrap();
        let teams_json: String = if teams.is_empty() {
            "[]".to_string()
        } else {
            let inner: Vec<String> = teams
                .iter()
                .map(|(id, key)| {
                    format!(
                        r#"{{"id":"{id}","api_key":"{key}","policy":{{"allowed_routers":[]}}}}"#
                    )
                })
                .collect();
            format!("[{}]", inner.join(","))
        };
        let json = format!(
            r#"{{
              "version": "1.0",
              "global": {{
                "listen": "127.0.0.1:12356",
                "auth_keys": {auth_json},
                "timeouts": {{"connect_ms":1000,"request_ms":1000,"response_ms":1000}},
                "retries": {{"max_attempts":1,"backoff_ms":100,"retry_on_status":[500]}},
                "cors_allowed_origins": []
              }},
              "logging": {{"level":"info","dir":null}},
              "data_dir": "/tmp/apex-data",
              "channels": [],
              "routers": [],
              "teams": {teams_json},
              "metrics": {{"enabled":true,"path":"/metrics"}},
              "hot_reload": {{"config_path":"config.json","watch":false}}
            }}"#
        );
        parse_config(&json)
    }

    #[test]
    fn placeholder_check_accepts_empty_auth_keys() {
        let cfg = config_with(&[], &[]);
        assert!(check_no_placeholder_credentials(&cfg).is_ok());
    }

    #[test]
    fn placeholder_check_accepts_real_keys() {
        let cfg = config_with(
            &["sk-real-admin-7f9d3a2e1c8b4f5a"],
            &[("acme", "sk-ap-realteamkey1234567890abcdef")],
        );
        assert!(check_no_placeholder_credentials(&cfg).is_ok());
    }

    #[test]
    fn placeholder_check_rejects_install_release_admin_placeholder() {
        let cfg = config_with(&["replace-with-admin-key"], &[]);
        let err = check_no_placeholder_credentials(&cfg).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("global.auth_keys[0]"), "{msg}");
        assert!(msg.contains("replace-with-admin-key"), "{msg}");
    }

    #[test]
    fn placeholder_check_rejects_install_sh_admin_placeholder() {
        let cfg = config_with(&["replace-with-dashboard-admin-key"], &[]);
        assert!(check_no_placeholder_credentials(&cfg).is_err());
    }

    #[test]
    fn placeholder_check_rejects_legacy_v042_admin_default() {
        // Anyone upgrading from v0.4.2 and never editing config carries this.
        let cfg = config_with(&["sk-your-secret-key-here"], &[]);
        assert!(check_no_placeholder_credentials(&cfg).is_err());
    }

    #[test]
    fn placeholder_check_rejects_uppercase_example_placeholder() {
        let cfg = config_with(&["REPLACE-WITH-YOUR-ADMIN-KEY"], &[]);
        assert!(check_no_placeholder_credentials(&cfg).is_err());
    }

    #[test]
    fn placeholder_check_rejects_team_placeholder() {
        let cfg = config_with(
            &["sk-real-admin-7f9d3a2e1c8b4f5a"],
            &[("demo-team", "replace-with-team-api-key")],
        );
        let err = check_no_placeholder_credentials(&cfg).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("teams[id=demo-team]"), "{msg}");
    }

    #[test]
    fn placeholder_check_rejects_legacy_v042_team_default() {
        let cfg = config_with(
            &["sk-real-admin-7f9d3a2e1c8b4f5a"],
            &[("demo-team", "sk-team-demo-key")],
        );
        assert!(check_no_placeholder_credentials(&cfg).is_err());
    }

    #[test]
    fn placeholder_check_real_key_alongside_placeholder_still_rejects() {
        // User added a real key but left the placeholder in place: still bad,
        // because the placeholder remains live too.
        let cfg = config_with(
            &["sk-real-admin-7f9d3a2e1c8b4f5a", "replace-with-admin-key"],
            &[],
        );
        let err = check_no_placeholder_credentials(&cfg).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("global.auth_keys[1]"), "{msg}");
    }

    #[test]
    fn placeholder_check_substring_match_does_not_trip_real_keys() {
        // A real key that happens to contain "replace" should NOT trip the
        // guard — we check exact equality, not substring.
        let cfg = config_with(&["sk-replace-this-with-rotation-policy"], &[]);
        assert!(check_no_placeholder_credentials(&cfg).is_ok());
    }

    #[test]
    fn placeholder_lists_cover_every_shipped_string() {
        // Belt-and-suspenders: if we ever add a new placeholder string to an
        // install template, this list should be updated too. Forces the dev
        // to look here.
        assert!(PLACEHOLDER_AUTH_KEYS.contains(&"replace-with-admin-key"));
        assert!(PLACEHOLDER_AUTH_KEYS.contains(&"replace-with-dashboard-admin-key"));
        assert!(PLACEHOLDER_AUTH_KEYS.contains(&"REPLACE-WITH-YOUR-ADMIN-KEY"));
        assert!(PLACEHOLDER_AUTH_KEYS.contains(&"sk-your-secret-key-here"));
        assert!(PLACEHOLDER_TEAM_KEYS.contains(&"replace-with-team-api-key"));
        assert!(PLACEHOLDER_TEAM_KEYS.contains(&"REPLACE-WITH-YOUR-TEAM-API-KEY"));
        assert!(PLACEHOLDER_TEAM_KEYS.contains(&"sk-team-demo-key"));
    }

    #[test]
    fn provider_type_zai_round_trips_as_snake_case() {
        let serialized = serde_json::to_string(&ProviderType::Zai).unwrap();
        assert_eq!(serialized, "\"zai\"");

        let parsed: ProviderType = serde_json::from_str("\"zai\"").unwrap();
        assert_eq!(parsed, ProviderType::Zai);
    }

    #[test]
    fn config_accepts_legacy_web_dir_but_does_not_serialize_it() {
        let content = r#"{
          "version": "1.0",
          "global": {
            "listen": "127.0.0.1:12356",
            "auth_keys": [],
            "timeouts": {
              "connect_ms": 1000,
              "request_ms": 1000,
              "response_ms": 1000
            },
            "retries": {
              "max_attempts": 1,
              "backoff_ms": 100,
              "retry_on_status": [500]
            },
            "cors_allowed_origins": []
          },
          "logging": {
            "level": "info",
            "dir": null
          },
          "data_dir": "/tmp/apex-data",
          "web_dir": "/tmp/legacy-web",
          "channels": [],
          "routers": [],
          "teams": [],
          "metrics": {
            "enabled": true,
            "path": "/metrics"
          },
          "hot_reload": {
            "config_path": "config.json",
            "watch": false
          }
        }"#;

        let config: Config = serde_json::from_str(content).unwrap();
        assert_eq!(config.web_dir, "/tmp/legacy-web");

        let serialized = serde_json::to_string(&config).unwrap();
        assert!(!serialized.contains("web_dir"));
    }

    // ---- cost/billing config ----

    #[test]
    fn config_without_pricing_still_parses() {
        // A pre-cost config must load unchanged: pricing absent.
        let cfg = config_with(&[], &[]);
        assert!(cfg.pricing.is_none());
    }

    #[test]
    fn channel_pricing_defaults_to_none_and_parses_rule_name() {
        let untracked: Channel = serde_json::from_str(
            r#"{"name":"c","provider_type":"openai","base_url":"u","api_key":"k"}"#,
        )
        .unwrap();
        assert!(untracked.pricing.is_none());

        let priced: Channel = serde_json::from_str(
            r#"{"name":"c","provider_type":"anthropic","base_url":"u","api_key":"k",
                "pricing":"claude-plan"}"#,
        )
        .unwrap();
        assert_eq!(priced.pricing.as_deref(), Some("claude-plan"));
    }

    #[test]
    fn pricing_rule_lookup_and_rate_card() {
        let pricing: Pricing = serde_json::from_str(
            r#"{"rules":[
                {"name":"deepseek","type":"payg","prices":[
                    {"match":"*flash*","input":0.14,"output":0.28,"cache_read":0.0028},
                    {"match":"*pro*","input":0.435,"output":0.87,"cache_read":0.003625},
                    {"match":"*","input":0.27,"output":1.1}
                ]},
                {"name":"claude-plan","type":"subscription","monthly_fee":200.0}
            ]}"#,
        )
        .unwrap();
        assert_eq!(pricing.currency, "USD"); // defaulted
        assert_eq!(pricing.unit, 1_000_000.0); // defaulted

        let ds = pricing.rule("deepseek").unwrap();
        assert!(!ds.is_subscription());
        // same channel/rule, different model → different price row (first match wins)
        assert_eq!(ds.price_for("deepseek-v4-flash").unwrap().input, 0.14);
        assert_eq!(ds.price_for("deepseek-v4-pro").unwrap().output, 0.87);
        assert_eq!(
            ds.price_for("deepseek-v4-pro").unwrap().cache_read_rate(),
            0.003625
        );
        // cache_write defaults to input when unset
        assert_eq!(
            ds.price_for("deepseek-v4-flash")
                .unwrap()
                .cache_write_rate(),
            0.14
        );
        // fallback row
        assert_eq!(ds.price_for("deepseek-chat").unwrap().input, 0.27);

        let plan = pricing.rule("claude-plan").unwrap();
        assert!(plan.is_subscription());
        assert_eq!(plan.monthly_fee, 200.0);
        assert_eq!(plan.billing_day, 1); // defaulted
        assert!(plan.prices.is_empty());
        assert!(pricing.rule("missing").is_none());
    }
}
