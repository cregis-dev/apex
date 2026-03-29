use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use rand::{Rng, distributions::Alphanumeric};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod compliance;
mod config;
mod converters;
mod database;
mod gemini_compat;
mod logs;
mod metrics;
mod middleware;
mod providers;
mod router_selector;
mod server;
mod usage;
mod utils;
mod web_assets;

use config::{
    Channel, Config, Global, HotReload, Metrics, ProviderType, Retries, Router, TargetChannel,
    Timeouts,
};

#[derive(Debug, Serialize)]
struct CliJsonResponse {
    ok: bool,
    command: String,
    message: String,
    data: Option<Value>,
    errors: Vec<CliJsonError>,
    meta: CliJsonMeta,
}

#[derive(Debug, Serialize)]
struct CliJsonError {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    field: Option<String>,
}

#[derive(Debug, Serialize)]
struct CliJsonMeta {
    resource: String,
    action: String,
}

fn print_json_success(
    resource: &str,
    action: &str,
    message: &str,
    data: Value,
) -> anyhow::Result<()> {
    let response = CliJsonResponse {
        ok: true,
        command: format!("{resource}.{action}"),
        message: message.to_string(),
        data: Some(data),
        errors: Vec::new(),
        meta: CliJsonMeta {
            resource: resource.to_string(),
            action: action.to_string(),
        },
    };
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

fn error_code_for_message(message: &str) -> (&'static str, Option<&'static str>) {
    let lower = message.to_lowercase();
    if lower.contains("already exists") {
        ("already_exists", None)
    } else if lower.contains("not found") {
        ("not_found", None)
    } else if lower.contains("invalid") || lower.contains("unsupported") {
        ("invalid_input", None)
    } else if lower.contains("required") {
        ("missing_required_field", None)
    } else {
        ("command_failed", None)
    }
}

fn exit_with_json_error(resource: &str, action: &str, err: &anyhow::Error) -> ! {
    let message = err.to_string();
    let (code, field) = error_code_for_message(&message);
    let response = CliJsonResponse {
        ok: false,
        command: format!("{resource}.{action}"),
        message: message.clone(),
        data: None,
        errors: vec![CliJsonError {
            code: code.to_string(),
            message,
            field: field.map(str::to_string),
        }],
        meta: CliJsonMeta {
            resource: resource.to_string(),
            action: action.to_string(),
        },
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&response).unwrap_or_else(|_| "{\"ok\":false}".to_string())
    );
    std::process::exit(1);
}

fn return_or_exit_json<T>(
    resource: &str,
    action: &str,
    json_output: bool,
    result: anyhow::Result<T>,
) -> anyhow::Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(err) => {
            if json_output {
                exit_with_json_error(resource, action, &err);
            }
            Err(err)
        }
    }
}

fn channel_public_json(channel: &Channel) -> Value {
    json!({
        "name": channel.name,
        "provider_type": channel.provider_type,
        "base_url": channel.base_url,
        "anthropic_base_url": channel.anthropic_base_url,
        "has_api_key": !channel.api_key.is_empty(),
        "headers": channel.headers,
        "model_map": channel.model_map,
        "timeouts": channel.timeouts,
    })
}

fn channels_public_json(channels: &[Channel]) -> Value {
    Value::Array(channels.iter().map(channel_public_json).collect())
}

#[derive(Parser)]
#[command(name = "apex", version)]
struct Cli {
    #[arg(long)]
    config: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init {
        config: Option<String>,
    },
    Channel {
        #[command(subcommand)]
        command: ChannelCommand,
    },
    Router {
        #[command(subcommand)]
        command: RouterCommand,
    },
    Gateway {
        #[command(subcommand)]
        command: GatewayCommand,
    },
    Team {
        #[command(subcommand)]
        command: TeamCommand,
    },
    Status,
    Logs,
}

#[derive(Subcommand)]
enum GatewayCommand {
    Start {
        #[arg(long, short = 'c')]
        config: String,
        #[arg(long, short = 'd')]
        daemon: bool,
    },
    Stop,
}

#[derive(Subcommand)]
enum TeamCommand {
    Add(TeamAddArgs),
    Remove {
        id: String,
        #[arg(long)]
        json: bool,
    },
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
struct TeamAddArgs {
    #[arg(long)]
    id: String,
    #[arg(long, value_delimiter = ',', num_args = 1..)]
    routers: Vec<String>,
    #[arg(long, value_delimiter = ',', num_args = 0..)]
    models: Option<Vec<String>>,
    #[arg(long)]
    rpm: Option<i32>,
    #[arg(long)]
    tpm: Option<i32>,
    #[arg(long)]
    json: bool,
}

fn handle_team_command(cli: &Cli, command: &TeamCommand) -> anyhow::Result<()> {
    let config_path = resolve_config_path(cli.config.clone());

    match command {
        TeamCommand::Add(args) => {
            let mut config =
                return_or_exit_json("team", "add", args.json, load_config_or_exit(&config_path))?;
            if config.teams.iter().any(|t| t.id == args.id) {
                let err = anyhow::anyhow!("Team '{}' already exists", args.id);
                if args.json {
                    exit_with_json_error("team", "add", &err);
                }
                return Err(err);
            }

            // Generate API Key: sk-ap-xxxx
            let random_part: String = rand::thread_rng()
                .sample_iter(&Alphanumeric)
                .take(32)
                .map(char::from)
                .collect();
            let api_key = format!("sk-ap-{}", random_part);

            let team = config::Team {
                id: args.id.clone(),
                api_key: api_key.clone(),
                policy: config::TeamPolicy {
                    allowed_routers: args.routers.clone(),
                    allowed_models: args.models.clone(),
                    rate_limit: if args.rpm.is_some() || args.tpm.is_some() {
                        Some(config::TeamRateLimit {
                            rpm: args.rpm,
                            tpm: args.tpm,
                        })
                    } else {
                        None
                    },
                },
            };

            std::sync::Arc::make_mut(&mut config.teams).push(team.clone());
            return_or_exit_json(
                "team",
                "add",
                args.json,
                config::save_config(&config_path, &config),
            )?;
            if args.json {
                print_json_success(
                    "team",
                    "add",
                    "Team added successfully.",
                    serde_json::to_value(&team)?,
                )?;
            } else {
                println!("Team '{}' added successfully.", args.id);
                println!("API Key: {}", api_key);
            }
        }
        TeamCommand::Remove { id, json } => {
            let mut config =
                return_or_exit_json("team", "remove", *json, load_config_or_exit(&config_path))?;
            if let Some(index) = config.teams.iter().position(|t| t.id == *id) {
                let removed = std::sync::Arc::make_mut(&mut config.teams).remove(index);
                return_or_exit_json(
                    "team",
                    "remove",
                    *json,
                    config::save_config(&config_path, &config),
                )?;
                if *json {
                    print_json_success(
                        "team",
                        "remove",
                        "Team removed successfully.",
                        serde_json::to_value(&removed)?,
                    )?;
                } else {
                    println!("Team '{}' removed successfully.", id);
                }
            } else {
                let err = anyhow::anyhow!("Team '{}' not found", id);
                if *json {
                    exit_with_json_error("team", "remove", &err);
                }
                return Err(err);
            }
        }
        TeamCommand::List { json } => {
            let config =
                return_or_exit_json("team", "list", *json, load_config_or_exit(&config_path))?;
            if *json {
                print_json_success(
                    "team",
                    "list",
                    "Teams listed successfully.",
                    serde_json::to_value(&*config.teams)?,
                )?;
            } else if config.teams.is_empty() {
                println!("No teams configured.");
            } else {
                println!("{:<20} {:<45} {:<20}", "ID", "API Key", "Allowed Routers");
                println!("{:-<20} {:-<45} {:-<20}", "", "", "");
                for team in config.teams.iter() {
                    let routers = team.policy.allowed_routers.join(", ");
                    println!("{:<20} {:<45} {:<20}", team.id, team.api_key, routers);
                }
            }
        }
    }

    Ok(())
}

fn expand_path(path_str: &str) -> PathBuf {
    let trimmed = path_str.trim();

    // Empty string - use default
    if trimmed.is_empty() {
        return get_default_log_dir();
    }

    // Handle ~ expansion
    if trimmed.starts_with('~') {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        if trimmed == "~" {
            // ~ alone - use default log location
            return get_default_log_dir();
        }
        if let Some(stripped) = trimmed.strip_prefix("~/") {
            return home.join(stripped);
        }
    }

    PathBuf::from(trimmed)
}

fn get_default_log_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join("Library").join("Logs").join("apex")
    } else if cfg!(target_os = "linux") {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".local").join("share").join("apex").join("logs")
    } else {
        PathBuf::from("logs")
    }
}

fn get_log_dir(configured_dir: Option<String>) -> PathBuf {
    if let Some(dir) = configured_dir {
        let expanded = expand_path(&dir);
        // If expanded path is empty or just "~", use default
        if expanded.as_os_str().is_empty() || expanded.as_os_str() == "~" {
            return get_default_log_dir();
        }
        expanded
    } else {
        get_default_log_dir()
    }
}

#[derive(Subcommand)]
enum ChannelCommand {
    Add(ChannelAddArgs),
    Update(ChannelUpdateArgs),
    Delete {
        name: String,
        #[arg(long)]
        json: bool,
    },
    Show {
        name: String,
        #[arg(long)]
        json: bool,
    },
    List {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum RouterCommand {
    Add(RouterAddArgs),
    Update(RouterUpdateArgs),
    Delete {
        name: String,
        #[arg(long)]
        json: bool,
    },
    List {
        #[arg(long)]
        json: bool,
    },
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
    #[arg(long)]
    json: bool,
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
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct RouterAddArgs {
    #[arg(long)]
    name: String,
    #[arg(long = "channels", value_delimiter = ',', num_args = 0..)]
    channels: Vec<String>,
    #[arg(long, default_value = "round_robin")]
    strategy: String,
    #[arg(long = "match", value_delimiter = ',', num_args = 0..)]
    model_matchers: Vec<String>,
    #[arg(long = "fallback", value_delimiter = ',', num_args = 0..)]
    fallback_channels: Vec<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct RouterUpdateArgs {
    #[arg(long)]
    name: String,
    #[arg(long = "channels", value_delimiter = ',', num_args = 0..)]
    channels: Vec<String>,
    #[arg(long)]
    strategy: Option<String>,
    #[arg(long = "match", value_delimiter = ',', num_args = 0..)]
    model_matchers: Vec<String>,
    #[arg(long = "fallback", value_delimiter = ',', num_args = 0..)]
    fallback_channels: Vec<String>,
    #[arg(long)]
    clear_fallbacks: bool,
    #[arg(long)]
    json: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Check for daemon mode in Gateway Start command
    let (is_daemon, start_config) = if let Commands::Gateway {
        command: GatewayCommand::Start { daemon, config },
    } = &cli.command
    {
        (*daemon, Some(config.clone()))
    } else {
        (false, None)
    };

    // Load config to get log level and dir
    let config_path = resolve_config_path(cli.config.clone().or(start_config));
    let config = config::load_config(&config_path).ok();

    let log_level = config
        .as_ref()
        .map(|c| c.logging.level.clone())
        .unwrap_or_else(|| "info".to_string());
    let log_dir_override = config.as_ref().and_then(|c| c.logging.dir.clone());
    let log_dir = get_log_dir(log_dir_override);

    if is_daemon {
        std::fs::create_dir_all(&log_dir).context("failed to create log dir")?;

        let stdout = std::fs::File::create(log_dir.join("stdout.log"))
            .unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap());
        let stderr = std::fs::File::create(log_dir.join("stderr.log"))
            .unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap());

        daemonize::Daemonize::new()
            .pid_file(log_dir.join("apex.pid"))
            .working_directory(".")
            .stdout(stdout)
            .stderr(stderr)
            .start()
            .context("failed to start daemon")?;
    }

    let env_filter = format!("apex={},tower_http={}", log_level, log_level);

    let _guard = if is_daemon {
        // Setup daemon logging
        let file_appender = tracing_appender::rolling::daily(&log_dir, "apex.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| env_filter.into()),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(non_blocking)
                    .with_ansi(false),
            )
            .init();

        Some(guard)
    } else {
        // Setup standard logging
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| env_filter.into()),
            )
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
                let path = resolve_config_path(Some(config.clone()));
                server::run_server(path).await?;
            }
            GatewayCommand::Stop => handle_stop_command(&cli)?,
        },
        Commands::Status => handle_status_command(&cli)?,
        Commands::Logs => handle_logs_command(&cli)?,
        Commands::Team { command } => handle_team_command(&cli, command)?,
    }
    Ok(())
}

fn handle_logs_command(cli: &Cli) -> anyhow::Result<()> {
    let path = resolve_config_path(cli.config.clone());
    let config = config::load_config(&path).ok();
    let log_dir_override = config.as_ref().and_then(|c| c.logging.dir.clone());
    let log_dir = get_log_dir(log_dir_override);

    // Find the latest log file.
    // tracing_appender::rolling::daily creates files like "apex.log.YYYY-MM-DD"
    // But the symlink/current might be different.
    // Wait, rolling appender usually creates files with dates suffix.
    // Let's list files in log_dir and find the most recent one matching "apex.log.*"

    println!("Log directory: {}", log_dir.display());

    // For simplicity, we assume standard rolling naming.
    // However, tracing_appender doesn't create a "current" symlink by default unless configured?
    // Actually rolling appender creates `apex.log.YYYY-MM-DD`.
    // Let's try to find the newest file.

    let entries = std::fs::read_dir(&log_dir).context("failed to read log dir")?;
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                name.starts_with("apex.log")
            } else {
                false
            }
        })
        .collect();

    logs.sort();

    if let Some(latest) = logs.last() {
        println!("Tailing log file: {}", latest.display());
        // Use tail -f
        let mut child = std::process::Command::new("tail")
            .arg("-f")
            .arg(latest)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .context("failed to execute tail")?;

        if let Some(stdout) = child.stdout.take() {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        let colored_line = logs::highlight_line(&line);
                        println!("{}", colored_line);
                    }
                    Err(e) => {
                        eprintln!("Error reading log line: {}", e);
                        break;
                    }
                }
            }
        }

        // Wait for child process if loop exits
        let _ = child.wait();
    } else {
        println!("No log files found in {}", log_dir.display());
    }

    Ok(())
}

fn handle_status_command(cli: &Cli) -> anyhow::Result<()> {
    // Load config to find log dir
    let path = resolve_config_path(cli.config.clone());
    let config = config::load_config(&path).ok();
    let log_dir_override = config.as_ref().and_then(|c| c.logging.dir.clone());
    let log_dir = get_log_dir(log_dir_override);

    // Check daemon status
    let pid_path = log_dir.join("apex.pid");
    let mut status = "Stopped";
    let mut pid_info = String::new();

    if pid_path.exists()
        && let Ok(pid_str) = std::fs::read_to_string(&pid_path)
    {
        let pid_str = pid_str.trim();
        // Check if process exists (signal 0)
        if std::process::Command::new("kill")
            .arg("-0")
            .arg(pid_str)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            status = "Running";
            pid_info = format!(" (PID: {})", pid_str);
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

fn handle_stop_command(cli: &Cli) -> anyhow::Result<()> {
    let path = resolve_config_path(cli.config.clone());
    let config = config::load_config(&path).ok();
    let log_dir_override = config.as_ref().and_then(|c| c.logging.dir.clone());
    let log_dir = get_log_dir(log_dir_override);

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
        return expand_path(&path);
    }
    let mut home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.push(".apex");
    home.push("config.json");
    home
}

fn init_config(path: &std::path::Path) -> anyhow::Result<()> {
    if path.exists() {
        anyhow::bail!("config already exists: {}", path.display());
    }
    let config = Config {
        version: "1".to_string(),
        global: Global {
            listen: "0.0.0.0:12356".to_string(),
            auth_keys: vec![],
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
            gemini_replay: crate::config::GeminiReplay::default(),
            cors_allowed_origins: vec![],
        },
        channels: std::sync::Arc::new(Vec::new()),
        routers: std::sync::Arc::new(Vec::new()),
        metrics: Metrics {
            enabled: true,
            path: "/metrics".to_string(),
        },
        hot_reload: HotReload {
            config_path: path.display().to_string(),
            watch: true,
        },
        logging: config::Logging {
            level: "info".to_string(),
            dir: None,
        },
        data_dir: dirs::home_dir()
            .map(|p| p.join(".apex/data").to_string_lossy().to_string())
            .unwrap_or_else(|| "~/.apex/data".to_string()),
        web_dir: "web".to_string(),
        teams: std::sync::Arc::new(Vec::new()),
        compliance: None,
    };
    config::save_config(path, &config)
        .with_context(|| format!("failed to write config: {}", path.display()))?;
    println!("✅ 已写入 {}", path.display());
    Ok(())
}

fn load_config_or_exit(path: &std::path::Path) -> anyhow::Result<Config> {
    if !path.exists() {
        bail!(
            "Config file not found at {}. Run 'apex init' to create a default configuration.",
            path.display()
        );
    }
    config::load_config(path).with_context(|| format!("failed to load config: {}", path.display()))
}

fn handle_channel_command(cli: &Cli, command: &ChannelCommand) -> anyhow::Result<()> {
    let path = resolve_config_path(cli.config.clone());
    match command {
        ChannelCommand::Add(args) => {
            let mut config =
                return_or_exit_json("channel", "add", args.json, load_config_or_exit(&path))?;
            if config.channels.iter().any(|c| c.name == args.name) {
                let err = anyhow::anyhow!("channel already exists: {}", args.name);
                if args.json {
                    exit_with_json_error("channel", "add", &err);
                }
                return Err(err);
            }

            let templates = load_provider_templates().unwrap_or_default();

            // 1. Select Provider
            let provider_value = match &args.provider {
                Some(value) => value.clone(),
                None => prompt_provider_select()?,
            };
            let provider = return_or_exit_json(
                "channel",
                "add",
                args.json,
                parse_provider_type(&provider_value),
            )?;

            let template = templates.iter().find(|t| t.provider_type == provider_value);

            // 2. Base URL
            let default_base_url = template
                .map(|t| t.base_url.clone())
                .unwrap_or_else(|| get_default_base_url(&provider).to_string());

            let base_url = args
                .base_url
                .clone()
                .unwrap_or_else(|| default_base_url.clone());

            // 3. Anthropic Base URL
            let anthropic_base_url = args.anthropic_base_url.clone().or_else(|| {
                template
                    .and_then(|t| t.anthropic_base_url.clone())
                    .or_else(|| get_default_anthropic_base_url(&provider).map(|s| s.to_string()))
            });

            // 4. Input API Key
            let api_key = match &args.api_key {
                Some(key) => key.clone(),
                None => inquire::Text::new("API Key")
                    .with_help_message("Enter the API key for this provider")
                    .prompt()?,
            };

            let headers = return_or_exit_json(
                "channel",
                "add",
                args.json,
                parse_optional_map(&args.headers),
            )?;
            let model_map = return_or_exit_json(
                "channel",
                "add",
                args.json,
                parse_optional_map(&args.model_map),
            )?;
            let timeouts = build_timeouts(
                &config.global.timeouts,
                args.connect_ms,
                args.request_ms,
                args.response_ms,
            );

            let channel = Channel {
                name: args.name.clone(),
                provider_type: provider,
                base_url,
                api_key,
                anthropic_base_url,
                headers,
                model_map,
                timeouts,
            };
            std::sync::Arc::make_mut(&mut config.channels).push(channel.clone());
            return_or_exit_json(
                "channel",
                "add",
                args.json,
                config::save_config(&path, &config),
            )?;
            if args.json {
                print_json_success(
                    "channel",
                    "add",
                    "Channel added successfully.",
                    channel_public_json(&channel),
                )?;
            } else {
                println!("✅ 已添加 channel: {}", args.name);
            }
        }
        ChannelCommand::Update(args) => {
            let mut config =
                return_or_exit_json("channel", "update", args.json, load_config_or_exit(&path))?;
            let channel_idx = config
                .channels
                .iter()
                .position(|c| c.name == args.name)
                .ok_or_else(|| anyhow::anyhow!("channel not found: {}", args.name));
            let channel_idx = match channel_idx {
                Ok(index) => index,
                Err(err) => {
                    if args.json {
                        exit_with_json_error("channel", "update", &err);
                    }
                    return Err(err);
                }
            };

            let templates = load_provider_templates().unwrap_or_default();
            let mut new_provider_value: Option<String> = None;

            if let Some(provider_val) = &args.provider {
                let p = return_or_exit_json(
                    "channel",
                    "update",
                    args.json,
                    parse_provider_type(provider_val),
                )?;
                std::sync::Arc::make_mut(&mut config.channels)[channel_idx].provider_type = p;
                new_provider_value = Some(provider_val.clone());
            }

            // If provider changed, we might want to prompt for URLs if not provided
            if let Some(provider_val) = new_provider_value {
                let template = templates.iter().find(|t| t.provider_type == provider_val);

                // Base URL
                if args.base_url.is_none() {
                    let default_base_url =
                        template.map(|t| t.base_url.clone()).unwrap_or_else(|| {
                            get_default_base_url(&config.channels[channel_idx].provider_type)
                                .to_string()
                        });
                    std::sync::Arc::make_mut(&mut config.channels)[channel_idx].base_url =
                        default_base_url;
                }

                // Anthropic Base URL
                if args.anthropic_base_url.is_none() && !args.clear_anthropic_base_url {
                    let default_anthropic = template
                        .and_then(|t| t.anthropic_base_url.clone())
                        .or_else(|| {
                            get_default_anthropic_base_url(
                                &config.channels[channel_idx].provider_type,
                            )
                            .map(|s| s.to_string())
                        });

                    if let Some(default) = default_anthropic {
                        std::sync::Arc::make_mut(&mut config.channels)[channel_idx]
                            .anthropic_base_url = Some(default);
                    }
                }
            }

            let channel = &mut std::sync::Arc::make_mut(&mut config.channels)[channel_idx];

            if let Some(base_url) = &args.base_url {
                channel.base_url = base_url.clone();
            }
            if let Some(api_key) = &args.api_key {
                channel.api_key = api_key.clone();
            }
            if args.clear_anthropic_base_url {
                channel.anthropic_base_url = None;
            } else if let Some(url) = &args.anthropic_base_url {
                channel.anthropic_base_url = Some(url.clone());
            }
            if args.clear_headers {
                channel.headers = None;
            } else if !args.headers.is_empty() {
                channel.headers = return_or_exit_json(
                    "channel",
                    "update",
                    args.json,
                    parse_optional_map(&args.headers),
                )?;
            }
            if args.clear_model_map {
                channel.model_map = None;
            } else if !args.model_map.is_empty() {
                channel.model_map = return_or_exit_json(
                    "channel",
                    "update",
                    args.json,
                    parse_optional_map(&args.model_map),
                )?;
            }
            if args.clear_timeouts {
                channel.timeouts = None;
            } else if args.connect_ms.is_some()
                || args.request_ms.is_some()
                || args.response_ms.is_some()
            {
                let base = channel.timeouts.as_ref().unwrap_or(&config.global.timeouts);
                channel.timeouts = Some(merge_timeouts(
                    base,
                    args.connect_ms,
                    args.request_ms,
                    args.response_ms,
                ));
            }
            return_or_exit_json(
                "channel",
                "update",
                args.json,
                config::save_config(&path, &config),
            )?;
            let updated = config.channels[channel_idx].clone();
            if args.json {
                print_json_success(
                    "channel",
                    "update",
                    "Channel updated successfully.",
                    channel_public_json(&updated),
                )?;
            } else {
                println!("✅ 已更新 channel: {}", args.name);
            }
        }
        ChannelCommand::Delete { name, json } => {
            let mut config =
                return_or_exit_json("channel", "delete", *json, load_config_or_exit(&path))?;
            let removed = config.channels.iter().find(|c| c.name == *name).cloned();
            let removed = match removed {
                Some(channel) => channel,
                None => {
                    let err = anyhow::anyhow!("channel not found: {}", name);
                    if *json {
                        exit_with_json_error("channel", "delete", &err);
                    }
                    return Err(err);
                }
            };
            std::sync::Arc::make_mut(&mut config.channels).retain(|c| c.name != *name);

            // Remove channel from all routers' channel lists
            for router in std::sync::Arc::make_mut(&mut config.routers) {
                router.channels.retain(|c| c.name != *name);
                router.fallback_channels.retain(|c| c != name);
            }
            // Remove routers that have no channels left
            std::sync::Arc::make_mut(&mut config.routers).retain(|r| !r.channels.is_empty());

            return_or_exit_json(
                "channel",
                "delete",
                *json,
                config::save_config(&path, &config),
            )?;
            if *json {
                print_json_success(
                    "channel",
                    "delete",
                    "Channel deleted successfully.",
                    channel_public_json(&removed),
                )?;
            } else {
                println!("✅ 已删除 channel: {}", name);
            }
        }
        ChannelCommand::Show { name, json } => {
            let config = return_or_exit_json("channel", "show", *json, load_config_or_exit(&path))?;
            let channel = config.channels.iter().find(|c| c.name == *name).cloned();
            let channel = match channel {
                Some(channel) => channel,
                None => {
                    let err = anyhow::anyhow!("channel not found: {}", name);
                    if *json {
                        exit_with_json_error("channel", "show", &err);
                    }
                    return Err(err);
                }
            };

            if *json {
                print_json_success(
                    "channel",
                    "show",
                    "Channel shown successfully.",
                    channel_public_json(&channel),
                )?;
            } else {
                println!("Channel Details:");
                println!("  Name:               {}", channel.name);
                println!("  Provider:           {:?}", channel.provider_type);
                println!("  Base URL:           {}", channel.base_url);
                println!(
                    "  Anthropic Base URL: {}",
                    channel.anthropic_base_url.as_deref().unwrap_or("N/A")
                );
                println!("  Has API Key:        {}", !channel.api_key.is_empty());

                if let Some(headers) = &channel.headers {
                    println!("  Headers:            {:?}", headers);
                }
                if let Some(models) = &channel.model_map {
                    println!("  Model Map:          {:?}", models);
                }
                if let Some(timeouts) = &channel.timeouts {
                    println!("  Timeouts:           {:?}", timeouts);
                }
            }
        }
        ChannelCommand::List { json } => {
            let config = return_or_exit_json("channel", "list", *json, load_config_or_exit(&path))?;
            if *json {
                print_json_success(
                    "channel",
                    "list",
                    "Channels listed successfully.",
                    channels_public_json(&config.channels),
                )?;
            } else {
                print_channel_table(&config.channels);
            }
        }
    }
    Ok(())
}

fn parse_target_channels(inputs: &[String]) -> anyhow::Result<Vec<TargetChannel>> {
    let mut channels = Vec::new();
    for input in inputs {
        let parts: Vec<&str> = input.splitn(2, ':').collect();
        let name = parts[0].trim().to_string();
        let weight = if parts.len() > 1 {
            parts[1].parse::<u32>().context("invalid weight")?
        } else {
            1
        };
        channels.push(TargetChannel { name, weight });
    }
    Ok(channels)
}

fn handle_router_command(cli: &Cli, command: &RouterCommand) -> anyhow::Result<()> {
    let path = resolve_config_path(cli.config.clone());
    match command {
        RouterCommand::Add(args) => {
            let mut config =
                return_or_exit_json("router", "add", args.json, load_config_or_exit(&path))?;
            if config.routers.iter().any(|r| r.name == args.name) {
                let err = anyhow::anyhow!("router already exists: {}", args.name);
                if args.json {
                    exit_with_json_error("router", "add", &err);
                }
                return Err(err);
            }

            let mut target_channels = return_or_exit_json(
                "router",
                "add",
                args.json,
                parse_target_channels(&args.channels),
            )?;

            // If no explicit channels list, prompt
            if target_channels.is_empty() {
                let channel_name = prompt_channel_select(&config.channels, None)?;
                target_channels.push(TargetChannel {
                    name: channel_name,
                    weight: 1,
                });
            }

            // Verify all channels exist
            let channel_names: Vec<String> =
                target_channels.iter().map(|c| c.name.clone()).collect();
            return_or_exit_json(
                "router",
                "add",
                args.json,
                ensure_channels_exist(&config, &channel_names),
            )?;
            return_or_exit_json(
                "router",
                "add",
                args.json,
                ensure_channels_exist(&config, &args.fallback_channels),
            )?;

            // Build rules from args
            let mut rules = Vec::new();

            // 1. Model matchers -> Specific rules
            if !args.model_matchers.is_empty() {
                for matcher in &args.model_matchers {
                    let parts: Vec<&str> = matcher.splitn(2, '=').collect();
                    if parts.len() != 2 {
                        let err = anyhow::anyhow!("invalid matcher format: {}", matcher);
                        if args.json {
                            exit_with_json_error("router", "add", &err);
                        }
                        return Err(err);
                    }
                    let pattern = parts[0].to_string();
                    let channel_name = parts[1].to_string();

                    return_or_exit_json(
                        "router",
                        "add",
                        args.json,
                        ensure_channels_exist(&config, std::slice::from_ref(&channel_name)),
                    )?;
                    rules.push(config::RouterRule {
                        match_spec: config::MatchSpec {
                            models: vec![pattern],
                        },
                        channels: vec![config::TargetChannel {
                            name: channel_name,
                            weight: 1,
                        }],
                        strategy: "round_robin".to_string(),
                    });
                }
            }

            // 2. Default channels -> Catch-all rule
            if !target_channels.is_empty() {
                rules.push(config::RouterRule {
                    match_spec: config::MatchSpec {
                        models: vec!["*".to_string()],
                    },
                    channels: target_channels.clone(),
                    strategy: args.strategy.clone(),
                });
            }

            let router = Router {
                name: args.name.clone(),
                rules,
                // Legacy fields - don't set channels when using rules-based config
                channels: Vec::new(),
                strategy: args.strategy.clone(),
                metadata: None,
                fallback_channels: args.fallback_channels.clone(),
            };
            std::sync::Arc::make_mut(&mut config.routers).push(router.clone());
            return_or_exit_json(
                "router",
                "add",
                args.json,
                config::save_config(&path, &config),
            )?;
            if args.json {
                print_json_success(
                    "router",
                    "add",
                    "Router added successfully.",
                    serde_json::to_value(&router)?,
                )?;
            } else {
                println!("✅ 已添加 router: {}", args.name);
            }
        }
        RouterCommand::Update(args) => {
            let mut config =
                return_or_exit_json("router", "update", args.json, load_config_or_exit(&path))?;

            let router_idx = config
                .routers
                .iter()
                .position(|r| r.name == args.name)
                .ok_or_else(|| anyhow::anyhow!("router not found: {}", args.name));
            let router_idx = match router_idx {
                Ok(index) => index,
                Err(err) => {
                    if args.json {
                        exit_with_json_error("router", "update", &err);
                    }
                    return Err(err);
                }
            };

            // Check if we are in "interactive mode" (no explicit updates)
            let is_interactive = args.channels.is_empty()
                && args.fallback_channels.is_empty()
                && !args.clear_fallbacks
                && args.strategy.is_none()
                && args.model_matchers.is_empty();

            let mut new_channels = return_or_exit_json(
                "router",
                "update",
                args.json,
                parse_target_channels(&args.channels),
            )?;

            if is_interactive {
                println!("进入交互式更新模式 (按 Ctrl+C 取消)...");
                let current_router = &config.routers[router_idx];

                // Channel
                let current_channel = current_router
                    .channels
                    .first()
                    .map(|c| c.name.as_str())
                    .unwrap_or("");
                let current_channel_string = current_channel.to_string();

                let selection =
                    prompt_channel_select(&config.channels, Some(&current_channel_string))?;
                if selection != current_channel_string {
                    new_channels.push(TargetChannel {
                        name: selection,
                        weight: 1,
                    });
                }
            }

            // Merge logic: if explicit channels given, use them.
            // If interactive selection made, use it.
            if !new_channels.is_empty() {
                // verify
                let names: Vec<String> = new_channels.iter().map(|c| c.name.clone()).collect();
                return_or_exit_json(
                    "router",
                    "update",
                    args.json,
                    ensure_channels_exist(&config, &names),
                )?;
            }

            if !args.fallback_channels.is_empty() {
                return_or_exit_json(
                    "router",
                    "update",
                    args.json,
                    ensure_channels_exist(&config, &args.fallback_channels),
                )?;
            }

            // Verify matcher targets
            if let Some(map) = return_or_exit_json(
                "router",
                "update",
                args.json,
                parse_optional_map(&args.model_matchers),
            )? {
                let targets: Vec<String> = map.values().cloned().collect();
                return_or_exit_json(
                    "router",
                    "update",
                    args.json,
                    ensure_channels_exist(&config, &targets),
                )?;
            }

            let router = &mut std::sync::Arc::make_mut(&mut config.routers)[router_idx];

            if !new_channels.is_empty() {
                router.channels = new_channels;
            }

            if let Some(strategy) = &args.strategy {
                router.strategy = strategy.clone();
            }

            if let Some(map) = return_or_exit_json(
                "router",
                "update",
                args.json,
                parse_optional_map(&args.model_matchers),
            )? {
                let mut current_matchers = router
                    .metadata
                    .as_ref()
                    .map(|m| m.model_matcher.clone())
                    .unwrap_or_default();
                current_matchers.extend(map);
                router.metadata = Some(config::RouterMetadata {
                    model_matcher: current_matchers,
                });
            }

            if args.clear_fallbacks {
                router.fallback_channels = Vec::new();
            } else if !args.fallback_channels.is_empty() {
                router.fallback_channels = args.fallback_channels.clone();
            }
            return_or_exit_json(
                "router",
                "update",
                args.json,
                config::save_config(&path, &config),
            )?;
            let updated = config.routers[router_idx].clone();
            if args.json {
                print_json_success(
                    "router",
                    "update",
                    "Router updated successfully.",
                    serde_json::to_value(&updated)?,
                )?;
            } else {
                println!("✅ 已更新 router: {}", args.name);
            }
        }
        RouterCommand::Delete { name, json } => {
            let mut config =
                return_or_exit_json("router", "delete", *json, load_config_or_exit(&path))?;
            let removed = config.routers.iter().find(|r| r.name == *name).cloned();
            let removed = match removed {
                Some(router) => router,
                None => {
                    let err = anyhow::anyhow!("router not found: {}", name);
                    if *json {
                        exit_with_json_error("router", "delete", &err);
                    }
                    return Err(err);
                }
            };
            std::sync::Arc::make_mut(&mut config.routers).retain(|r| r.name != *name);
            return_or_exit_json(
                "router",
                "delete",
                *json,
                config::save_config(&path, &config),
            )?;
            if *json {
                print_json_success(
                    "router",
                    "delete",
                    "Router deleted successfully.",
                    serde_json::to_value(&removed)?,
                )?;
            } else {
                println!("✅ 已删除 router: {}", name);
            }
        }
        RouterCommand::List { json } => {
            let config = return_or_exit_json("router", "list", *json, load_config_or_exit(&path))?;
            if *json {
                print_json_success(
                    "router",
                    "list",
                    "Routers listed successfully.",
                    serde_json::to_value(&*config.routers)?,
                )?;
            } else {
                print_router_table(&config.routers);
            }
        }
    }
    Ok(())
}

fn print_channel_table(channels: &[Channel]) {
    println!(
        "{:<20} {:<12} {:<11} {:<10}",
        "NAME", "PROVIDER", "HAS_API_KEY", "MODEL_MAP"
    );
    for channel in channels {
        let has_key = !channel.api_key.is_empty();
        let model_map_count = channel.model_map.as_ref().map(|m| m.len()).unwrap_or(0);
        println!(
            "{:<20} {:<12} {:<11} {:<10}",
            channel.name,
            format!("{:?}", channel.provider_type).to_lowercase(),
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
        "custom_dual" => Ok(ProviderType::CustomDual),
        "deepseek" => Ok(ProviderType::Deepseek),
        "moonshot" => Ok(ProviderType::Moonshot),
        "minimax" => Ok(ProviderType::Minimax),
        "ollama" => Ok(ProviderType::Ollama),
        "jina" => Ok(ProviderType::Jina),
        "openrouter" => Ok(ProviderType::Openrouter),
        "zai" => Ok(ProviderType::Zai),
        _ => bail!("unsupported provider: {}", value),
    }
}

fn provider_choices() -> Vec<&'static str> {
    vec![
        "openai",
        "anthropic",
        "gemini",
        "custom_dual",
        "deepseek",
        "moonshot",
        "minimax",
        "ollama",
        "jina",
        "openrouter",
        "zai",
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
    if let Some(d) = default
        && let Some(idx) = choices.iter().position(|x| x == d)
    {
        select = select.with_starting_cursor(idx);
    }
    let selection = select.prompt()?;
    Ok(selection)
}

fn get_default_base_url(provider: &ProviderType) -> &'static str {
    match provider {
        ProviderType::Openai => "https://api.openai.com/v1",
        ProviderType::Anthropic => "https://api.anthropic.com/v1",
        ProviderType::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai/",
        ProviderType::CustomDual => "https://api.example.com/v1",
        ProviderType::Deepseek => "https://api.deepseek.com",
        ProviderType::Moonshot => "https://api.moonshot.cn/v1",
        ProviderType::Minimax => "https://api.minimax.io/v1",
        ProviderType::Ollama => "http://localhost:11434",
        ProviderType::Jina => "https://api.jina.ai/v1",
        ProviderType::Openrouter => "https://openrouter.ai/api/v1",
        ProviderType::Zai => "https://api.z.ai/api/coding/paas/v4",
    }
}

fn get_default_anthropic_base_url(provider: &ProviderType) -> Option<&'static str> {
    match provider {
        ProviderType::CustomDual => Some("https://api.example.com/anthropic"),
        ProviderType::Deepseek => Some("https://api.deepseek.com/anthropic"),
        ProviderType::Moonshot => Some("https://api.moonshot.cn/anthropic"),
        ProviderType::Minimax => Some("https://api.minimax.io/anthropic"),
        ProviderType::Ollama => Some("http://localhost:11434"),
        ProviderType::Anthropic => Some("https://api.anthropic.com/v1"),
        ProviderType::Zai => Some("https://api.z.ai/api/anthropic"),
        _ => None,
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

#[derive(Debug, Deserialize)]
struct ProviderFile {
    provider_templates: Vec<ProviderTemplate>,
}

#[derive(Debug, Deserialize, Clone)]
struct ProviderTemplate {
    provider_type: String,
    base_url: String,
    anthropic_base_url: Option<String>,
}

fn load_provider_templates() -> anyhow::Result<Vec<ProviderTemplate>> {
    let path = std::env::current_dir()?.join("providers.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(path)?;
    let config: ProviderFile = serde_json::from_str(&content)?;
    Ok(config.provider_templates)
}

#[cfg(test)]
fn generate_vkey() -> String {
    let rand: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect();
    format!("vk_{rand}")
}

#[cfg(test)]
mod tests_vkey {
    use super::*;

    #[test]
    fn test_generate_vkey() {
        let vkey = generate_vkey();
        assert!(vkey.starts_with("vk_"));
        assert_eq!(vkey.len(), 27);
    }
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
    println!("{:<20} {:<20} {:<20}", "NAME", "CHANNELS", "FALLBACKS");
    for router in routers {
        let channels_display = if !router.channels.is_empty() {
            router
                .channels
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        } else {
            String::new()
        };

        println!(
            "{:<20} {:<20} {:<20}",
            router.name,
            channels_display,
            router.fallback_channels.join(",")
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
        assert_eq!(choices.len(), 11);
    }

    #[test]
    fn parse_provider_type_ok() {
        let provider = parse_provider_type("openai").unwrap();
        assert_eq!(provider, ProviderType::Openai);
        let provider = parse_provider_type("custom_dual").unwrap();
        assert_eq!(provider, ProviderType::CustomDual);
        let provider = parse_provider_type("zai").unwrap();
        assert_eq!(provider, ProviderType::Zai);
    }

    #[test]
    fn provider_choices_contains_zai() {
        let choices = provider_choices();
        assert!(choices.contains(&"zai"));
    }

    #[test]
    fn zai_defaults_to_single_base_url_only() {
        assert_eq!(
            get_default_base_url(&ProviderType::Zai),
            "https://api.z.ai/api/coding/paas/v4"
        );
        assert_eq!(
            get_default_anthropic_base_url(&ProviderType::Zai),
            Some("https://api.z.ai/api/anthropic")
        );
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
}

#[cfg(test)]
mod tests_timeouts {
    use super::*;

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
}
