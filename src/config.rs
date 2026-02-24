use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: String,
    pub global: Global,
    #[serde(default)]
    pub logging: Logging,
    pub channels: Vec<Channel>,
    pub routers: Vec<Router>,
    pub metrics: Metrics,
    pub hot_reload: HotReload,
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
    pub auth: Auth,
    pub timeouts: Timeouts,
    pub retries: Retries,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auth {
    pub mode: AuthMode,
    pub keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    ApiKey,
    None,
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
    Deepseek,
    Moonshot,
    Minimax,
    Ollama,
    Jina,
    Openrouter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Router {
    pub name: String,
    pub vkey: Option<String>,

    // New unified rules configuration
    #[serde(default)]
    pub rules: Vec<RouterRule>,

    // Legacy fields (kept for backward compatibility, will be migrated to rules)
    #[serde(default)]
    pub channels: Vec<TargetChannel>,
    #[serde(default = "default_strategy")]
    pub strategy: String,
    pub metadata: Option<RouterMetadata>,
    #[serde(default)]
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
    pub listen: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotReload {
    pub config_path: String,
    pub watch: bool,
}

pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    let content = fs::read_to_string(path)?;
    let mut config = serde_json::from_str::<Config>(&content)?;

    // Migrate legacy configuration to rules
    for router in &mut config.routers {
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

pub fn save_config(path: &Path, config: &Config) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(config)?;
    fs::write(path, content)?;
    Ok(())
}
