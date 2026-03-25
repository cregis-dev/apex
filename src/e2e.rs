use crate::config::{
    self, Channel, Config, Global, HotReload, Logging, MatchSpec, Metrics, ProviderType, Retries,
    Router as GatewayRouter, RouterRule, TargetChannel, Team, TeamPolicy, Timeouts,
};
use anyhow::{Context, bail};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct E2eEnv {
    pub listen: String,
    pub team_id: String,
    pub team_key: String,
    pub admin_key: Option<String>,
    pub router_name: String,
    pub router_strategy: String,
    pub test_model: String,
    pub enable_mcp: bool,
    pub metrics_path: String,
    pub upstreams: Vec<UpstreamConfig>,
}

#[derive(Debug, Clone)]
pub struct UpstreamConfig {
    pub name: String,
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: String,
    pub anthropic_base_url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub model_map: Option<HashMap<String, String>>,
    pub timeouts: Option<Timeouts>,
    pub weight: u32,
}

impl E2eEnv {
    pub fn from_env_file(path: &Path) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read env file: {}", path.display()))?;
        content.parse()
    }
}

impl FromStr for E2eEnv {
    type Err = anyhow::Error;

    fn from_str(content: &str) -> anyhow::Result<Self> {
        let values = parse_env_map(content)?;
        let listen = get_or_default(&values, "APEX_E2E_LISTEN", "127.0.0.1:12356");
        let team_id = get_or_default(&values, "APEX_E2E_TEAM_ID", "e2e-team");
        let team_key = require(&values, "APEX_E2E_TEAM_KEY")?;
        let admin_key = optional(&values, "APEX_E2E_ADMIN_KEY");
        let router_name = get_or_default(&values, "APEX_E2E_ROUTER_NAME", "e2e-default");
        let router_strategy =
            get_or_default(&values, "APEX_E2E_ROUTER_STRATEGY", "priority").to_lowercase();
        let test_model = get_or_default(&values, "APEX_E2E_TEST_MODEL", "apex-test-chat");
        let enable_mcp = parse_bool_with_default(&values, "APEX_E2E_ENABLE_MCP", true)?;
        let metrics_path = get_or_default(&values, "APEX_E2E_METRICS_PATH", "/metrics");
        let upstreams = parse_upstreams(&values, &test_model)?;

        if upstreams.is_empty() {
            bail!("no enabled upstreams found in .env");
        }

        Ok(Self {
            listen,
            team_id,
            team_key,
            admin_key,
            router_name,
            router_strategy,
            test_model,
            enable_mcp,
            metrics_path,
            upstreams,
        })
    }
}

pub fn build_config(env: &E2eEnv, config_path: &Path) -> Config {
    let data_dir = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("data")
        .to_string_lossy()
        .to_string();

    let channels = env
        .upstreams
        .iter()
        .map(|upstream| Channel {
            name: upstream.name.clone(),
            provider_type: upstream.provider_type.clone(),
            base_url: upstream.base_url.clone(),
            api_key: upstream.api_key.clone(),
            anthropic_base_url: upstream.anthropic_base_url.clone(),
            headers: upstream.headers.clone(),
            model_map: upstream.model_map.clone(),
            timeouts: upstream.timeouts.clone(),
        })
        .collect::<Vec<_>>();

    let target_channels = env
        .upstreams
        .iter()
        .map(|upstream| TargetChannel {
            name: upstream.name.clone(),
            weight: upstream.weight,
        })
        .collect::<Vec<_>>();

    let fallback_channels = env
        .upstreams
        .iter()
        .skip(1)
        .map(|upstream| upstream.name.clone())
        .collect::<Vec<_>>();

    Config {
        version: "1.0".to_string(),
        global: Global {
            listen: env.listen.clone(),
            auth_keys: env.admin_key.clone().into_iter().collect(),
            timeouts: Timeouts {
                connect_ms: 1_000,
                request_ms: 30_000,
                response_ms: 30_000,
            },
            retries: Retries {
                max_attempts: 2,
                backoff_ms: 200,
                retry_on_status: vec![429, 500, 502, 503, 504],
            },
            gemini_replay: crate::config::GeminiReplay::default(),
            enable_mcp: env.enable_mcp,
            cors_allowed_origins: vec![],
        },
        logging: Logging {
            level: "info".to_string(),
            dir: None,
        },
        data_dir,
        web_dir: "target/web".to_string(),
        channels: Arc::new(channels),
        routers: Arc::new(vec![GatewayRouter {
            name: env.router_name.clone(),
            rules: vec![RouterRule {
                match_spec: MatchSpec {
                    models: vec!["*".to_string()],
                },
                channels: target_channels,
                strategy: env.router_strategy.clone(),
            }],
            channels: vec![],
            strategy: env.router_strategy.clone(),
            metadata: None,
            fallback_channels,
        }]),
        metrics: Metrics {
            enabled: true,
            path: env.metrics_path.clone(),
        },
        hot_reload: HotReload {
            config_path: config_path.to_string_lossy().to_string(),
            watch: false,
        },
        teams: Arc::new(vec![Team {
            id: env.team_id.clone(),
            api_key: env.team_key.clone(),
            policy: TeamPolicy {
                allowed_routers: vec![env.router_name.clone()],
                allowed_models: Some(vec![env.test_model.clone()]),
                rate_limit: None,
            },
        }]),
        prompts: Arc::new(vec![]),
        compliance: None,
    }
}

pub fn write_config(env: &E2eEnv, config_path: &Path) -> anyhow::Result<Config> {
    let config = build_config(env, config_path);
    config::save_config(config_path, &config)?;
    Ok(config)
}

fn parse_upstreams(
    values: &HashMap<String, String>,
    test_model: &str,
) -> anyhow::Result<Vec<UpstreamConfig>> {
    let mut indexes = BTreeSet::new();
    for key in values.keys() {
        if let Some(index) = upstream_index(key) {
            indexes.insert(index);
        }
    }

    let mut upstreams = Vec::new();
    for index in indexes {
        let enabled_key = format!("APEX_UPSTREAM_{index}_ENABLED");
        if !parse_bool_with_default(values, &enabled_key, false)? {
            continue;
        }

        let name = require(values, &format!("APEX_UPSTREAM_{index}_NAME"))?;
        let provider_type =
            parse_provider_type(&require(values, &format!("APEX_UPSTREAM_{index}_TYPE"))?)?;
        let base_url = require(values, &format!("APEX_UPSTREAM_{index}_BASE_URL"))?;
        let api_key = get_or_default(values, &format!("APEX_UPSTREAM_{index}_API_KEY"), "");
        let anthropic_base_url =
            optional(values, &format!("APEX_UPSTREAM_{index}_ANTHROPIC_BASE_URL"));
        let headers =
            parse_optional_json_object(values, &format!("APEX_UPSTREAM_{index}_HEADERS_JSON"))?;
        let model_map = parse_model_map(values, index, test_model)?;
        let timeouts = parse_timeouts(values, index)?;
        let weight = parse_u32_with_default(values, &format!("APEX_UPSTREAM_{index}_WEIGHT"), 1)?;

        upstreams.push(UpstreamConfig {
            name,
            provider_type,
            base_url,
            api_key,
            anthropic_base_url,
            headers,
            model_map,
            timeouts,
            weight,
        });
    }

    Ok(upstreams)
}

fn parse_model_map(
    values: &HashMap<String, String>,
    index: usize,
    test_model: &str,
) -> anyhow::Result<Option<HashMap<String, String>>> {
    let map_key = format!("APEX_UPSTREAM_{index}_MODEL_MAP_JSON");
    if let Some(raw) = optional(values, &map_key) {
        return parse_json_object(&raw, &map_key);
    }

    let model_key = format!("APEX_UPSTREAM_{index}_MODEL");
    if let Some(model) = optional(values, &model_key) {
        let mut map = HashMap::new();
        map.insert(test_model.to_string(), model);
        return Ok(Some(map));
    }

    Ok(None)
}

fn parse_timeouts(
    values: &HashMap<String, String>,
    index: usize,
) -> anyhow::Result<Option<Timeouts>> {
    let connect_key = format!("APEX_UPSTREAM_{index}_CONNECT_MS");
    let request_key = format!("APEX_UPSTREAM_{index}_REQUEST_MS");
    let response_key = format!("APEX_UPSTREAM_{index}_RESPONSE_MS");

    let connect_ms = optional(values, &connect_key)
        .map(|value| {
            value
                .parse::<u64>()
                .with_context(|| format!("invalid value for {connect_key}: {value}"))
        })
        .transpose()?;
    let request_ms = optional(values, &request_key)
        .map(|value| {
            value
                .parse::<u64>()
                .with_context(|| format!("invalid value for {request_key}: {value}"))
        })
        .transpose()?;
    let response_ms = optional(values, &response_key)
        .map(|value| {
            value
                .parse::<u64>()
                .with_context(|| format!("invalid value for {response_key}: {value}"))
        })
        .transpose()?;

    if connect_ms.is_none() && request_ms.is_none() && response_ms.is_none() {
        return Ok(None);
    }

    Ok(Some(Timeouts {
        connect_ms: connect_ms.unwrap_or(1_000),
        request_ms: request_ms.unwrap_or(30_000),
        response_ms: response_ms.unwrap_or(30_000),
    }))
}

fn parse_provider_type(value: &str) -> anyhow::Result<ProviderType> {
    match value.trim().to_ascii_lowercase().as_str() {
        "openai" => Ok(ProviderType::Openai),
        "anthropic" => Ok(ProviderType::Anthropic),
        "gemini" => Ok(ProviderType::Gemini),
        "deepseek" => Ok(ProviderType::Deepseek),
        "moonshot" => Ok(ProviderType::Moonshot),
        "minimax" => Ok(ProviderType::Minimax),
        "ollama" => Ok(ProviderType::Ollama),
        "jina" => Ok(ProviderType::Jina),
        "openrouter" => Ok(ProviderType::Openrouter),
        other => bail!("unsupported provider type in .env: {other}"),
    }
}

fn parse_optional_json_object(
    values: &HashMap<String, String>,
    key: &str,
) -> anyhow::Result<Option<HashMap<String, String>>> {
    match optional(values, key) {
        Some(raw) => parse_json_object(&raw, key),
        None => Ok(None),
    }
}

fn parse_json_object(raw: &str, key: &str) -> anyhow::Result<Option<HashMap<String, String>>> {
    let value: Value = serde_json::from_str(raw)
        .with_context(|| format!("invalid JSON object for {key}: {raw}"))?;
    let object = value
        .as_object()
        .with_context(|| format!("{key} must be a JSON object"))?;

    let mut map = HashMap::new();
    for (field, value) in object {
        let string_value = value
            .as_str()
            .with_context(|| format!("{key}.{field} must be a string"))?;
        map.insert(field.clone(), string_value.to_string());
    }

    Ok(Some(map))
}

fn parse_env_map(content: &str) -> anyhow::Result<HashMap<String, String>> {
    let mut values = HashMap::new();

    for (line_no, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, raw_value)) = line.split_once('=') else {
            bail!("invalid .env entry at line {}: {}", line_no + 1, raw_line);
        };

        let key = key.trim();
        if key.is_empty() {
            bail!("empty key in .env at line {}", line_no + 1);
        }

        values.insert(key.to_string(), parse_env_value(raw_value.trim()));
    }

    Ok(values)
}

fn parse_env_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0] as char;
        let last = trimmed.as_bytes()[trimmed.len() - 1] as char;
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }

    if let Some((value, _)) = trimmed.split_once(" #") {
        return value.trim().to_string();
    }

    trimmed.to_string()
}

fn require(values: &HashMap<String, String>, key: &str) -> anyhow::Result<String> {
    optional(values, key).with_context(|| format!("missing required env key: {key}"))
}

fn optional(values: &HashMap<String, String>, key: &str) -> Option<String> {
    values
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn get_or_default(values: &HashMap<String, String>, key: &str, default: &str) -> String {
    optional(values, key).unwrap_or_else(|| default.to_string())
}

fn parse_bool_with_default(
    values: &HashMap<String, String>,
    key: &str,
    default: bool,
) -> anyhow::Result<bool> {
    let Some(raw) = optional(values, key) else {
        return Ok(default);
    };

    match raw.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("invalid boolean value for {key}: {raw}"),
    }
}

fn parse_u32_with_default(
    values: &HashMap<String, String>,
    key: &str,
    default: u32,
) -> anyhow::Result<u32> {
    let Some(raw) = optional(values, key) else {
        return Ok(default);
    };

    raw.parse::<u32>()
        .with_context(|| format!("invalid integer value for {key}: {raw}"))
}

fn upstream_index(key: &str) -> Option<usize> {
    let suffix = key.strip_prefix("APEX_UPSTREAM_")?;
    let (index, _) = suffix.split_once('_')?;
    index.parse::<usize>().ok()
}
