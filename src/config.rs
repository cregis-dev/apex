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

pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    let content = fs::read_to_string(path)?;
    let mut config = serde_json::from_str::<Config>(&content)?;

    // Validate compliance configuration if present
    if let Some(ref compliance) = config.compliance {
        compliance
            .validate()
            .map_err(|e| anyhow::anyhow!("Invalid compliance config: {}", e))?;
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
        Config, PLACEHOLDER_AUTH_KEYS, PLACEHOLDER_TEAM_KEYS, ProviderType,
        check_no_placeholder_credentials,
    };

    fn parse_config(json: &str) -> Config {
        serde_json::from_str(json).unwrap()
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
}
