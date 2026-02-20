use anyhow::{bail, Context};
use clap::{Args, Parser, Subcommand};
use rand::{distributions::Alphanumeric, Rng};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod converters;
mod metrics;
mod providers;
mod server;

use config::{
    Auth, AuthMode, Channel, Config, Global, HotReload, Metrics, ProviderType, Retries, Router,
    Timeouts,
};

#[derive(Parser)]
#[command(name = "apex", version)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init { config: Option<String> },
    Channel { #[command(subcommand)] command: ChannelCommand },
    Router { #[command(subcommand)] command: RouterCommand },
    Gateway { #[command(subcommand)] command: GatewayCommand },
    Status,
}

#[derive(Subcommand)]
enum GatewayCommand {
    Start {
        config: Option<String>,
        #[arg(long, short = 'd')]
        daemon: bool,
    },
    Stop,
}

fn get_daemon_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join("Library").join("Logs").join("apex")
    } else {
        PathBuf::from("logs")
    }
}

#[derive(Subcommand)]
enum ChannelCommand {
    Add(ChannelAddArgs),
    Update(ChannelUpdateArgs),
    Delete { name: String },
    List { #[arg(long)] json: bool },
}

#[derive(Subcommand)]
enum RouterCommand {
    Add(RouterAddArgs),
    Update(RouterUpdateArgs),
    Delete { name: String },
    List { #[arg(long)] json: bool },
}

#[derive(Args)]
struct ChannelAddArgs {
    #[arg(long)]
    name: String,
    #[arg(long)]
    provider: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long)]
    protocol: Option<String>,
    #[arg(long)]
    anthropic_base_url: Option<String>,
    #[arg(long = "header")]
    headers: Vec<String>,
    #[arg(long = "model-map")]
    model_map: Vec<String>,
    #[arg(long)]
    connect_ms: Option<u64>,
    #[arg(long)]
    request_ms: Option<u64>,
    #[arg(long)]
    response_ms: Option<u64>,
}

#[derive(Args)]
struct ChannelUpdateArgs {
    #[arg(long)]
    name: String,
    #[arg(long)]
    provider: Option<String>,
    #[arg(long)]
    base_url: Option<String>,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long)]
    protocol: Option<String>,
    #[arg(long)]
    anthropic_base_url: Option<String>,
    #[arg(long = "header")]
    headers: Vec<String>,
    #[arg(long = "model-map")]
    model_map: Vec<String>,
    #[arg(long)]
    clear_headers: bool,
    #[arg(long)]
    clear_model_map: bool,
    #[arg(long)]
    clear_anthropic_base_url: bool,
    #[arg(long)]
    clear_timeouts: bool,
    #[arg(long)]
    connect_ms: Option<u64>,
    #[arg(long)]
    request_ms: Option<u64>,
    #[arg(long)]
    response_ms: Option<u64>,
}

#[derive(Args)]
struct RouterAddArgs {
    #[arg(long)]
    name: String,
    #[arg(long)]
    channel: Option<String>,
    #[arg(long = "fallback")]
    fallback_channels: Vec<String>,
    #[arg(long)]
    vkey: Option<String>,
}

#[derive(Args)]
struct RouterUpdateArgs {
    #[arg(long)]
    name: String,
    #[arg(long)]
    channel: Option<String>,
    #[arg(long = "fallback")]
    fallback_channels: Vec<String>,
    #[arg(long)]
    clear_fallbacks: bool,
    #[arg(long)]
    vkey: Option<String>,
    #[arg(long)]
    clear_vkey: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    // Check for daemon mode in Gateway Start command
    let is_daemon = matches!(cli.command, Commands::Gateway { command: GatewayCommand::Start { daemon: true, .. } });

    if is_daemon {
        let log_dir = get_daemon_dir();
        std::fs::create_dir_all(&log_dir).context("failed to create log dir")?;
        
        let stdout = std::fs::File::create(log_dir.join("stdout.log")).unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap());
        let stderr = std::fs::File::create(log_dir.join("stderr.log")).unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap());
        
        daemonize::Daemonize::new()
            .pid_file(log_dir.join("apex.pid"))
            .working_directory(".")
            .stdout(stdout)
            .stderr(stderr)
            .start()
            .context("failed to start daemon")?;
    }

    let _guard = if is_daemon {
        // Setup daemon logging
        let log_dir = get_daemon_dir();
        
        let file_appender = tracing_appender::rolling::daily(&log_dir, "apex.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "apex=info,tower_http=info".into()))
            .with(tracing_subscriber::fmt::layer().with_writer(non_blocking).with_ansi(false))
            .init();
            
        Some(guard)
    } else {
        // Setup standard logging
        tracing_subscriber::registry()
            .with(tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "apex=info,tower_http=info".into()))
            .with(tracing_subscriber::fmt::layer())
            .init();
        None
    };

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main(cli))
}

async fn async_main(cli: Cli) -> anyhow::Result<()> {
    match &cli.command {
        Commands::Init { config } => {
            let path = resolve_config_path(cli.config.clone().or_else(|| config.clone()));
            init_config(&path)?;
        }
        Commands::Channel { command } => handle_channel_command(&cli, command)?,
        Commands::Router { command } => handle_router_command(&cli, command)?,
        Commands::Gateway { command } => match command {
            GatewayCommand::Start { config, daemon: _ } => {
                let path = resolve_config_path(cli.config.clone().or_else(|| config.clone()));
                server::run_server(path).await?;
            }
            GatewayCommand::Stop => handle_stop_command()?,
        },
        Commands::Status => handle_status_command(&cli)?,
    }
    Ok(())
}

fn handle_status_command(cli: &Cli) -> anyhow::Result<()> {
    // Check daemon status
    let log_dir = get_daemon_dir();
    let pid_path = log_dir.join("apex.pid");
    let mut status = "Stopped";
    let mut pid_info = String::new();

    if pid_path.exists() {
        if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
            let pid_str = pid_str.trim();
            // Check if process exists (signal 0)
            if std::process::Command::new("kill").arg("-0").arg(pid_str).output().map(|o| o.status.success()).unwrap_or(false) {
                status = "Running";
                pid_info = format!(" (PID: {})", pid_str);
            }
        }
    }

    println!("Gateway Status: {}{}", status, pid_info);

    // Load config to show details
    let path = resolve_config_path(cli.config.clone());
    if path.exists() {
        println!("\nConfig File: {}", path.display());
        match config::load_config(&path) {
            Ok(config) => {
                println!("Listen Address: {}", config.global.listen);
                println!("\nChannels:");
                print_channel_table(&config.channels);
                println!("\nRouters:");
                print_router_table(&config.routers);
            }
            Err(e) => {
                println!("Error loading config: {}", e);
            }
        }
    } else {
        println!("\nConfig file not found at {}", path.display());
    }

    Ok(())
}

fn handle_stop_command() -> anyhow::Result<()> {
    let log_dir = get_daemon_dir();
    let pid_path = log_dir.join("apex.pid");
    
    if !pid_path.exists() {
        println!("⚠️  PID file not found at {}", pid_path.display());
        println!("Is the daemon running?");
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_path).context("failed to read pid file")?;
    let pid_str = pid_str.trim();
    
    // Validate PID format
    let pid: i32 = pid_str.parse().context("invalid pid in file")?;

    // Check if process exists (signal 0) using 'kill -0 <pid>'
    let check_status = std::process::Command::new("kill")
        .arg("-0")
        .arg(pid_str)
        .output();

    match check_status {
        Ok(output) if !output.status.success() => {
            println!("⚠️  Process {} not found. Cleaning up PID file.", pid);
            std::fs::remove_file(pid_path).ok();
            return Ok(());
        }
        Err(_) => {
            // If kill command fails to run, we proceed to try killing it anyway or error out
        }
        _ => {}
    }

    // Send SIGTERM
    let output = std::process::Command::new("kill")
        .arg(pid_str)
        .output()
        .context("failed to execute kill command")?;

    if output.status.success() {
        println!("✅ Stopped daemon (PID: {})", pid);
        std::fs::remove_file(pid_path).ok();
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("failed to stop daemon: {}", stderr);
    }
    Ok(())
}

fn resolve_config_path(path: Option<String>) -> PathBuf {
    if let Some(path) = path {
        return PathBuf::from(path);
    }
    let mut home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.push(".apex");
    home.push("config.json");
    home
}

fn init_config(path: &PathBuf) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!("config already exists: {}", path.display());
    }
    let config = Config {
        version: "1".to_string(),
        global: Global {
            listen: "0.0.0.0:12356".to_string(),
            auth: Auth {
                mode: AuthMode::None,
                keys: None,
            },
            timeouts: Timeouts {
                connect_ms: 2000,
                request_ms: 30000,
                response_ms: 30000,
            },
            retries: Retries {
                max_attempts: 2,
                backoff_ms: 200,
                retry_on_status: vec![429, 500, 502, 503, 504],
            },
        },
        channels: Vec::new(),
        routers: Vec::new(),
        metrics: Metrics {
            enabled: true,
            listen: "0.0.0.0:9090".to_string(),
            path: "/metrics".to_string(),
        },
        hot_reload: HotReload {
            config_path: path.display().to_string(),
            watch: true,
        },
    };
    config::save_config(path, &config)
        .with_context(|| format!("failed to write config: {}", path.display()))?;
    println!("✅ 已写入 {}", path.display());
    Ok(())
}

fn load_config_or_exit(path: &PathBuf) -> anyhow::Result<Config> {
    config::load_config(path)
        .with_context(|| format!("failed to load config: {}", path.display()))
}

fn handle_channel_command(cli: &Cli, command: &ChannelCommand) -> anyhow::Result<()> {
    let path = resolve_config_path(cli.config.clone());
    match command {
        ChannelCommand::Add(args) => {
            let mut config = load_config_or_exit(&path)?;
            if config.channels.iter().any(|c| c.name == args.name) {
                bail!("channel already exists: {}", args.name);
            }
            
            // 1. Select Provider
            let provider_value = match &args.provider {
                Some(value) => value.clone(),
                None => prompt_provider_select()?,
            };
            let provider = parse_provider_type(&provider_value)?;
            
            // 2. Confirm Base URL
            let default_base_url = get_default_base_url(&provider);
            let base_url = match &args.base_url {
                Some(url) => url.clone(),
                None => inquire::Text::new("Base URL")
                    .with_default(default_base_url)
                    .prompt()?,
            };

            // 3. Input API Key
            let api_key = match &args.api_key {
                Some(key) => key.clone(),
                None => inquire::Text::new("API Key")
                    .with_help_message("Enter the API key for this provider")
                    .prompt()?,
            };

            let headers = parse_optional_map(&args.headers)?;
            let model_map = parse_optional_map(&args.model_map)?;
            let timeouts = build_timeouts(
                &config.global.timeouts,
                args.connect_ms,
                args.request_ms,
                args.response_ms,
            );
            let protocol = args.protocol.clone();
            let anthropic_base_url = args.anthropic_base_url.clone();

            let channel = Channel {
                name: args.name.clone(),
                provider_type: provider,
                base_url,
                api_key,
                protocol,
                anthropic_base_url,
                headers,
                model_map,
                timeouts,
            };
            config.channels.push(channel);
            config::save_config(&path, &config)?;
            println!("✅ 已添加 channel: {}", args.name);
        }
        ChannelCommand::Update(args) => {
            let mut config = load_config_or_exit(&path)?;
            let channel = config
                .channels
                .iter_mut()
                .find(|c| c.name == args.name)
                .ok_or_else(|| anyhow::anyhow!("channel not found: {}", args.name))?;
            if let Some(provider) = &args.provider {
                channel.provider_type = parse_provider_type(provider)?;
            }
            if let Some(base_url) = &args.base_url {
                channel.base_url = base_url.clone();
            }
            if let Some(api_key) = &args.api_key {
                channel.api_key = api_key.clone();
            }
            if let Some(protocol) = &args.protocol {
                channel.protocol = Some(protocol.clone());
            }
            if args.clear_anthropic_base_url {
                channel.anthropic_base_url = None;
            } else if let Some(url) = &args.anthropic_base_url {
                channel.anthropic_base_url = Some(url.clone());
            }
            if args.clear_headers {
                channel.headers = None;
            } else if !args.headers.is_empty() {
                channel.headers = parse_optional_map(&args.headers)?;
            }
            if args.clear_model_map {
                channel.model_map = None;
            } else if !args.model_map.is_empty() {
                channel.model_map = parse_optional_map(&args.model_map)?;
            }
            if args.clear_timeouts {
                channel.timeouts = None;
            } else if args.connect_ms.is_some()
                || args.request_ms.is_some()
                || args.response_ms.is_some()
            {
                let base = channel
                    .timeouts
                    .as_ref()
                    .unwrap_or(&config.global.timeouts);
                channel.timeouts = Some(merge_timeouts(
                    base,
                    args.connect_ms,
                    args.request_ms,
                    args.response_ms,
                ));
            }
            config::save_config(&path, &config)?;
            println!("✅ 已更新 channel: {}", args.name);
        }
        ChannelCommand::Delete { name } => {
            let mut config = load_config_or_exit(&path)?;
            let original_len = config.channels.len();
            config.channels.retain(|c| c.name != *name);
            if config.channels.len() == original_len {
                bail!("channel not found: {}", name);
            }
            config.routers.retain(|r| r.channel != *name);
            for router in &mut config.routers {
                router.fallback_channels.retain(|c| c != name);
            }
            config::save_config(&path, &config)?;
            println!("✅ 已删除 channel: {}", name);
        }
        ChannelCommand::List { json } => {
            let config = load_config_or_exit(&path)?;
            if *json {
                let output = serde_json::to_string_pretty(&config.channels)?;
                println!("{output}");
            } else {
                print_channel_table(&config.channels);
            }
        }
    }
    Ok(())
}

fn handle_router_command(cli: &Cli, command: &RouterCommand) -> anyhow::Result<()> {
    let path = resolve_config_path(cli.config.clone());
    match command {
        RouterCommand::Add(args) => {
            let mut config = load_config_or_exit(&path)?;
            if config.routers.iter().any(|r| r.name == args.name) {
                bail!("router already exists: {}", args.name);
            }
            
            // Interactive Channel
            let channel_name = match &args.channel {
                Some(c) => c.clone(),
                None => prompt_channel_select(&config.channels, None)?,
            };

            ensure_channels_exist(&config, &[channel_name.clone()])?;
            ensure_channels_exist(&config, &args.fallback_channels)?;
            let vkey = Some(args.vkey.clone().unwrap_or_else(generate_vkey));
            let router = Router {
                name: args.name.clone(),
                vkey,
                channel: channel_name,
                fallback_channels: args.fallback_channels.clone(),
            };
            config.routers.push(router);
            config::save_config(&path, &config)?;
            println!("✅ 已添加 router: {}", args.name);
        }
        RouterCommand::Update(args) => {
            let mut config = load_config_or_exit(&path)?;
            
            let router_idx = config.routers.iter().position(|r| r.name == args.name)
                .ok_or_else(|| anyhow::anyhow!("router not found: {}", args.name))?;

            // Check if we are in "interactive mode" (no explicit updates)
            let is_interactive = args.channel.is_none() 
                && args.fallback_channels.is_empty() 
                && !args.clear_fallbacks 
                && args.vkey.is_none() 
                && !args.clear_vkey;

            let mut new_channel = args.channel.clone();

            if is_interactive {
                println!("进入交互式更新模式 (按 Ctrl+C 取消)...");
                let current_router = &config.routers[router_idx];
                
                // Channel
                let current_channel = &current_router.channel;
                let selection = prompt_channel_select(&config.channels, Some(current_channel))?;
                if selection != *current_channel {
                    new_channel = Some(selection);
                }
            }

            if let Some(channel) = &new_channel {
                ensure_channels_exist(&config, &[channel.clone()])?;
            }
            if !args.fallback_channels.is_empty() {
                ensure_channels_exist(&config, &args.fallback_channels)?;
            }

            let router = &mut config.routers[router_idx];
            
            if let Some(ch) = new_channel {
                router.channel = ch;
            }
            if args.clear_fallbacks {
                router.fallback_channels = Vec::new();
            } else if !args.fallback_channels.is_empty() {
                router.fallback_channels = args.fallback_channels.clone();
            }
            if args.clear_vkey {
                router.vkey = None;
            } else if let Some(vkey) = &args.vkey {
                router.vkey = Some(vkey.clone());
            } else if router.vkey.is_none() {
                router.vkey = Some(generate_vkey());
            }
            config::save_config(&path, &config)?;
            println!("✅ 已更新 router: {}", args.name);
        }
        RouterCommand::Delete { name } => {
            let mut config = load_config_or_exit(&path)?;
            let original_len = config.routers.len();
            config.routers.retain(|r| r.name != *name);
            if config.routers.len() == original_len {
                bail!("router not found: {}", name);
            }
            config::save_config(&path, &config)?;
            println!("✅ 已删除 router: {}", name);
        }
        RouterCommand::List { json } => {
            let config = load_config_or_exit(&path)?;
            if *json {
                let output = serde_json::to_string_pretty(&config.routers)?;
                println!("{output}");
            } else {
                print_router_table(&config.routers);
            }
        }
    }
    Ok(())
}

fn print_channel_table(channels: &[Channel]) {
    println!(
        "{:<20} {:<12} {:<30} {:<11} {:<10}",
        "NAME", "PROVIDER", "BASE_URL", "HAS_API_KEY", "MODEL_MAP"
    );
    for channel in channels {
        let has_key = !channel.api_key.is_empty();
        let model_map_count = channel.model_map.as_ref().map(|m| m.len()).unwrap_or(0);
        println!(
            "{:<20} {:<12} {:<30} {:<11} {:<10}",
            channel.name,
            format!("{:?}", channel.provider_type).to_lowercase(),
            channel.base_url,
            if has_key { "yes" } else { "no" },
            model_map_count
        );
    }
}

fn parse_provider_type(value: &str) -> anyhow::Result<ProviderType> {
    match value.to_lowercase().as_str() {
        "openai" => Ok(ProviderType::Openai),
        "anthropic" => Ok(ProviderType::Anthropic),
        "gemini" => Ok(ProviderType::Gemini),
        "deepseek" => Ok(ProviderType::Deepseek),
        "moonshot" => Ok(ProviderType::Moonshot),
        "minimax" => Ok(ProviderType::Minimax),
        "ollama" => Ok(ProviderType::Ollama),
        "jina" => Ok(ProviderType::Jina),
        "openrouter" => Ok(ProviderType::Openrouter),
        _ => bail!("unsupported provider: {}", value),
    }
}

fn provider_choices() -> Vec<&'static str> {
    vec![
        "openai",
        "anthropic",
        "gemini",
        "deepseek",
        "moonshot",
        "minimax",
        "ollama",
        "jina",
        "openrouter",
    ]
}

fn prompt_provider_select() -> anyhow::Result<String> {
    let choices = provider_choices();
    let selection = inquire::Select::new("请选择 provider:", choices).prompt()?;
    Ok(selection.to_string())
}





fn prompt_channel_select(channels: &[Channel], default: Option<&str>) -> anyhow::Result<String> {
    let choices: Vec<String> = channels.iter().map(|c| c.name.clone()).collect();
    if choices.is_empty() {
        bail!("没有可用的 channel，请先创建 channel。");
    }
    let mut select = inquire::Select::new("请选择 channel:", choices.clone());
    if let Some(d) = default {
        if let Some(idx) = choices.iter().position(|x| x == d) {
            select = select.with_starting_cursor(idx);
        }
    }
    let selection = select.prompt()?;
    Ok(selection)
}

fn get_default_base_url(provider: &ProviderType) -> &'static str {
    match provider {
        ProviderType::Openai => "https://api.openai.com/v1",
        ProviderType::Anthropic => "https://api.anthropic.com/v1",
        ProviderType::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai/",
        ProviderType::Deepseek => "https://api.deepseek.com/anthropic",
        ProviderType::Moonshot => "https://api.moonshot.cn/anthropic",
        ProviderType::Minimax => "https://api.minimax.io/anthropic",
        ProviderType::Ollama => "http://localhost:11434",
        ProviderType::Jina => "https://api.jina.ai/v1",
        ProviderType::Openrouter => "https://openrouter.ai/api/v1",
    }
}



fn parse_optional_map(values: &[String]) -> anyhow::Result<Option<HashMap<String, String>>> {
    if values.is_empty() {
        return Ok(None);
    }
    let mut map = HashMap::new();
    for item in values {
        let mut parts = item.splitn(2, '=');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if key.is_empty() || value.is_empty() {
            bail!("invalid key=value pair: {}", item);
        }
        map.insert(key.to_string(), value.to_string());
    }
    Ok(Some(map))
}

fn build_timeouts(
    base: &Timeouts,
    connect_ms: Option<u64>,
    request_ms: Option<u64>,
    response_ms: Option<u64>,
) -> Option<Timeouts> {
    if connect_ms.is_none() && request_ms.is_none() && response_ms.is_none() {
        return None;
    }
    Some(merge_timeouts(base, connect_ms, request_ms, response_ms))
}

fn merge_timeouts(
    base: &Timeouts,
    connect_ms: Option<u64>,
    request_ms: Option<u64>,
    response_ms: Option<u64>,
) -> Timeouts {
    Timeouts {
        connect_ms: connect_ms.unwrap_or(base.connect_ms),
        request_ms: request_ms.unwrap_or(base.request_ms),
        response_ms: response_ms.unwrap_or(base.response_ms),
    }
}

fn generate_vkey() -> String {
    let rand: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect();
    format!("vk_{rand}")
}

fn ensure_channels_exist(config: &Config, channels: &[String]) -> anyhow::Result<()> {
    for name in channels {
        if !config.channels.iter().any(|c| c.name == *name) {
            bail!("channel not found: {}", name);
        }
    }
    Ok(())
}

fn print_router_table(routers: &[Router]) {
    println!(
        "{:<20} {:<20} {:<20} {:<15}",
        "NAME", "CHANNEL", "FALLBACKS", "VKEY"
    );
    for router in routers {
        let vkey = router.vkey.clone().unwrap_or_default();
        println!(
            "{:<20} {:<20} {:<20} {:<15}",
            router.name,
            router.channel,
            router.fallback_channels.join(","),
            if vkey.is_empty() { "" } else { "vk_****" }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_choices_contains_openai() {
        let choices = provider_choices();
        assert!(choices.contains(&"openai"));
    }

    #[test]
    fn provider_choices_count() {
        let choices = provider_choices();
        assert_eq!(choices.len(), 9);
    }

    #[test]
    fn parse_provider_type_ok() {
        let provider = parse_provider_type("openai").unwrap();
        assert_eq!(provider, ProviderType::Openai);
    }

    #[test]
    fn parse_provider_type_err() {
        assert!(parse_provider_type("unknown").is_err());
    }



    #[test]
    fn parse_optional_map_ok() {
        let input = vec!["a=b".to_string(), "c=d".to_string()];
        let map = parse_optional_map(&input).unwrap().unwrap();
        assert_eq!(map.get("a").unwrap(), "b");
        assert_eq!(map.get("c").unwrap(), "d");
    }

    #[test]
    fn parse_optional_map_err() {
        let input = vec!["a=".to_string()];
        assert!(parse_optional_map(&input).is_err());
    }

    #[test]
    fn build_timeouts_none_when_empty() {
        let base = Timeouts {
            connect_ms: 1,
            request_ms: 2,
            response_ms: 3,
        };
        let merged = build_timeouts(&base, None, None, None);
        assert!(merged.is_none());
    }

    #[test]
    fn merge_timeouts_overrides() {
        let base = Timeouts {
            connect_ms: 1,
            request_ms: 2,
            response_ms: 3,
        };
        let merged = merge_timeouts(&base, Some(10), None, Some(30));
        assert_eq!(merged.connect_ms, 10);
        assert_eq!(merged.request_ms, 2);
        assert_eq!(merged.response_ms, 30);
    }

    #[test]
    fn generate_vkey_format() {
        let vkey = generate_vkey();
        assert!(vkey.starts_with("vk_"));
        assert_eq!(vkey.len(), 27);
    }
}
