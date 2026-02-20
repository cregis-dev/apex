use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: String,
    pub global: Global,
    pub channels: Vec<Channel>,
    pub routers: Vec<Router>,
    pub metrics: Metrics,
    pub hot_reload: HotReload,
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
    pub protocol: Option<String>,
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
    pub channel: String,
    pub fallback_channels: Vec<String>,
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
    let config = serde_json::from_str::<Config>(&content)?;
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
