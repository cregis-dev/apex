use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceManager {
    Systemd,
    Launchd,
}

impl ServiceManager {
    pub fn detect() -> anyhow::Result<Self> {
        if cfg!(target_os = "linux") {
            Ok(Self::Systemd)
        } else if cfg!(target_os = "macos") {
            Ok(Self::Launchd)
        } else {
            bail!("native service management is only supported on Linux and macOS")
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::Launchd => "launchd",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServiceDefinition {
    pub install_dir: PathBuf,
    pub config_path: PathBuf,
    pub service_name: String,
    pub manager: ServiceManager,
}

impl ServiceDefinition {
    pub fn new(
        install_dir: PathBuf,
        config_path: PathBuf,
        service_name: String,
        manager: ServiceManager,
    ) -> Self {
        Self {
            install_dir,
            config_path,
            service_name,
            manager,
        }
    }

    pub fn binary_path(&self) -> PathBuf {
        self.install_dir.join("current").join("apex")
    }
}

pub fn default_service_name() -> &'static str {
    default_service_name_for(ServiceManager::detect().unwrap_or(ServiceManager::Systemd))
}

pub fn default_service_name_for(manager: ServiceManager) -> &'static str {
    match manager {
        ServiceManager::Systemd => "apex",
        ServiceManager::Launchd => "dev.cregis.apex",
    }
}

pub fn service_path(definition: &ServiceDefinition) -> PathBuf {
    match definition.manager {
        ServiceManager::Systemd => {
            PathBuf::from("/etc/systemd/system").join(systemd_unit(definition))
        }
        ServiceManager::Launchd => user_home_dir()
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", definition.service_name)),
    }
}

pub fn render_systemd_unit(definition: &ServiceDefinition) -> String {
    format!(
        "[Unit]\n\
         Description=Apex Gateway\n\
         After=network-online.target\n\
         Wants=network-online.target\n\n\
         [Service]\n\
         Type=simple\n\
         WorkingDirectory={install_dir}\n\
         Environment=APEX_CONFIG={config_path}\n\
         ExecStart={binary} gateway run\n\
         Restart=always\n\
         RestartSec=3\n\n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        install_dir = definition.install_dir.display(),
        config_path = definition.config_path.display(),
        binary = definition.binary_path().display()
    )
}

pub fn render_launchd_plist(definition: &ServiceDefinition) -> String {
    let stdout = definition.install_dir.join("logs").join("stdout.log");
    let stderr = definition.install_dir.join("logs").join("stderr.log");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string>
    <string>gateway</string>
    <string>run</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>APEX_CONFIG</key>
    <string>{config_path}</string>
  </dict>
  <key>WorkingDirectory</key>
  <string>{install_dir}</string>
  <key>StandardOutPath</key>
  <string>{stdout}</string>
  <key>StandardErrorPath</key>
  <string>{stderr}</string>
  <key>KeepAlive</key>
  <true/>
  <key>RunAtLoad</key>
  <true/>
</dict>
</plist>
"#,
        label = definition.service_name,
        binary = definition.binary_path().display(),
        config_path = definition.config_path.display(),
        install_dir = definition.install_dir.display(),
        stdout = stdout.display(),
        stderr = stderr.display()
    )
}

pub fn install_service(definition: &ServiceDefinition) -> anyhow::Result<()> {
    let path = service_path(definition);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create service directory {}", parent.display()))?;
    }
    std::fs::create_dir_all(definition.install_dir.join("logs")).with_context(|| {
        format!(
            "failed to create service log directory under {}",
            definition.install_dir.display()
        )
    })?;
    let content = match definition.manager {
        ServiceManager::Systemd => render_systemd_unit(definition),
        ServiceManager::Launchd => render_launchd_plist(definition),
    };
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write service definition {}", path.display()))?;
    match definition.manager {
        ServiceManager::Systemd => run_command(Command::new("systemctl").arg("daemon-reload"))?,
        ServiceManager::Launchd => {
            // If a previous registration is still loaded in launchd, bootout
            // the old in-memory plist so the next `service start` fresh-
            // bootstraps the file we just wrote. Otherwise launchctl keeps
            // the stale plist (ExecStart, env, paths) until the user reboots.
            let target = launchd_target(&launchd_domain(), &definition.service_name);
            if launchd_service_is_loaded(&target) {
                let _ = run_command(Command::new("launchctl").arg("bootout").arg(&target));
                println!(
                    "Unloaded previous launchd registration for {}; run `service start` to reload.",
                    definition.service_name
                );
            }
        }
    }
    println!("Wrote service definition: {}", path.display());
    Ok(())
}

pub fn uninstall_service(definition: &ServiceDefinition) -> anyhow::Result<()> {
    let _ = stop_service(definition);
    let path = service_path(definition);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("failed to remove service definition {}", path.display()))?;
    }
    if definition.manager == ServiceManager::Systemd {
        run_command(Command::new("systemctl").arg("daemon-reload"))?;
    }
    println!("Service uninstalled: {}", definition.service_name);
    Ok(())
}

pub fn start_service(definition: &ServiceDefinition) -> anyhow::Result<()> {
    match definition.manager {
        ServiceManager::Systemd => run_command(
            Command::new("systemctl")
                .arg("start")
                .arg(systemd_unit(definition)),
        )?,
        ServiceManager::Launchd => {
            let path = service_path(definition);
            if !path.exists() {
                bail!(
                    "service definition not found at {}; run service install first",
                    path.display()
                );
            }

            let domain = launchd_domain();
            let target = launchd_target(&domain, &definition.service_name);
            if !launchd_service_is_loaded(&target) {
                run_command(
                    Command::new("launchctl")
                        .arg("bootstrap")
                        .arg(&domain)
                        .arg(&path),
                )?;
            }
            run_command(
                Command::new("launchctl")
                    .arg("kickstart")
                    .arg("-k")
                    .arg(target),
            )?;
        }
    }
    Ok(())
}

pub fn stop_service(definition: &ServiceDefinition) -> anyhow::Result<()> {
    match definition.manager {
        ServiceManager::Systemd => run_command(
            Command::new("systemctl")
                .arg("stop")
                .arg(systemd_unit(definition)),
        )?,
        ServiceManager::Launchd => {
            let target = launchd_target(&launchd_domain(), &definition.service_name);
            if launchd_service_is_loaded(&target) {
                run_command(Command::new("launchctl").arg("bootout").arg(&target))?;
            } else {
                println!(
                    "Service {} is not loaded; nothing to stop.",
                    definition.service_name
                );
            }
        }
    }
    Ok(())
}

pub fn restart_service(definition: &ServiceDefinition) -> anyhow::Result<()> {
    match definition.manager {
        ServiceManager::Systemd => run_command(
            Command::new("systemctl")
                .arg("restart")
                .arg(systemd_unit(definition)),
        )?,
        ServiceManager::Launchd => {
            let _ = stop_service(definition);
            start_service(definition)?;
        }
    }
    Ok(())
}

pub fn status_service(definition: &ServiceDefinition) -> anyhow::Result<()> {
    match definition.manager {
        ServiceManager::Systemd => run_command(
            Command::new("systemctl")
                .arg("status")
                .arg(systemd_unit(definition)),
        )?,
        ServiceManager::Launchd => {
            // `launchctl print` on an unloaded label fails with a bare
            // "Could not find service ... in domain for user gui: <uid>",
            // which reads like a broken install. Distinguish the two states
            // ourselves so a not-yet-started service reports as such, with
            // the command that would start it.
            let path = service_path(definition);
            if !path.exists() {
                bail!(
                    "service definition not found at {}; run service install first",
                    path.display()
                );
            }
            let target = launchd_target(&launchd_domain(), &definition.service_name);
            // Only a definitive "not loaded" becomes the idle status. If we
            // could not run launchctl at all, that is still an error — it was
            // one before this branch existed, and silently reporting "stopped"
            // would hide a broken environment.
            if launchd_probe_service_loaded(&target)? {
                run_command(Command::new("launchctl").arg("print").arg(&target))?;
            } else {
                println!("{}", launchd_not_loaded_message(definition, &path, &target));
            }
        }
    }
    Ok(())
}

/// Status text for a launchd service whose plist is installed but not loaded.
fn launchd_not_loaded_message(definition: &ServiceDefinition, path: &Path, target: &str) -> String {
    format!(
        "Service installed but not loaded: {target}\n\
         Service definition: {path}\n\
         Start it with: {binary} service start --install-dir {install_dir}",
        path = path.display(),
        binary = shell_quote(&definition.install_dir.join("apex").to_string_lossy()),
        install_dir = shell_quote(&definition.install_dir.to_string_lossy())
    )
}

/// POSIX single-quoting, so a copy-pasted path survives spaces and shell
/// metacharacters. Left bare when the value has nothing a shell would touch.
fn shell_quote(value: &str) -> String {
    let safe = !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '+' | ',')
        });
    if safe {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

pub fn service_is_active(definition: &ServiceDefinition) -> bool {
    match definition.manager {
        ServiceManager::Systemd => Command::new("systemctl")
            .arg("is-active")
            .arg("--quiet")
            .arg(systemd_unit(definition))
            .status()
            .map(|status| status.success())
            .unwrap_or(false),
        ServiceManager::Launchd => Command::new("launchctl")
            .arg("print")
            .arg(launchd_target(&launchd_domain(), &definition.service_name))
            .status()
            .map(|status| status.success())
            .unwrap_or(false),
    }
}

pub fn logs_service(definition: &ServiceDefinition) -> anyhow::Result<()> {
    match definition.manager {
        ServiceManager::Systemd => run_command(
            Command::new("journalctl")
                .arg("-u")
                .arg(systemd_unit(definition))
                .arg("-f"),
        )?,
        ServiceManager::Launchd => {
            let stdout = definition.install_dir.join("logs").join("stdout.log");
            let stderr = definition.install_dir.join("logs").join("stderr.log");
            run_command(Command::new("tail").arg("-f").arg(stdout).arg(stderr))?;
        }
    }
    Ok(())
}

fn systemd_unit(definition: &ServiceDefinition) -> String {
    if definition.service_name.ends_with(".service") {
        definition.service_name.clone()
    } else {
        format!("{}.service", definition.service_name)
    }
}

pub fn user_home_dir() -> PathBuf {
    user_home_dir_for(std::env::var("SUDO_USER").ok().as_deref(), dirs::home_dir())
}

fn user_home_dir_for(sudo_user: Option<&str>, fallback_home: Option<PathBuf>) -> PathBuf {
    if let Some(user) = sudo_user
        && !user.trim().is_empty()
        && user != "root"
    {
        return PathBuf::from("/Users").join(user);
    }
    fallback_home.unwrap_or_else(|| PathBuf::from("."))
}

fn launchd_domain() -> String {
    launchd_domain_for_uid(&launchd_uid())
}

fn launchd_uid() -> String {
    if let Some(uid) = launchd_uid_from_sudo_env(std::env::var("SUDO_UID").ok().as_deref()) {
        return uid;
    }

    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|uid| uid.trim().to_string())
        .filter(|uid| !uid.is_empty())
        .unwrap_or_else(|| "501".to_string())
}

fn launchd_uid_from_sudo_env(sudo_uid: Option<&str>) -> Option<String> {
    sudo_uid
        .map(str::trim)
        .filter(|uid| !uid.is_empty() && *uid != "0")
        .map(ToString::to_string)
}

fn launchd_domain_for_uid(uid: &str) -> String {
    format!("gui/{uid}")
}

fn launchd_target(domain: &str, service_name: &str) -> String {
    format!("{domain}/{service_name}")
}

/// `Ok(true/false)` for loaded / not loaded; `Err` only when launchctl could
/// not be run at all (missing binary, bad PATH). Callers that cannot act on
/// the distinction use [`launchd_service_is_loaded`].
fn launchd_probe_service_loaded(target: &str) -> anyhow::Result<bool> {
    let status = Command::new("launchctl")
        .arg("print")
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to run launchctl print {target}"))?;
    Ok(status.success())
}

fn launchd_service_is_loaded(target: &str) -> bool {
    launchd_probe_service_loaded(target).unwrap_or(false)
}

fn run_command(command: &mut Command) -> anyhow::Result<()> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to run service command {:?}", command))?;
    if output.status.success() {
        if !output.stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        return Ok(());
    }
    bail!(
        "service command failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    )
}

#[allow(dead_code)]
fn _path_exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchd_not_loaded_message_names_plist_and_start_command() {
        let definition = ServiceDefinition::new(
            PathBuf::from("/Users/alice/.apex"),
            PathBuf::from("/Users/alice/.apex/config.json"),
            "dev.cregis.apex".to_string(),
            ServiceManager::Launchd,
        );
        let path = PathBuf::from("/Users/alice/Library/LaunchAgents/dev.cregis.apex.plist");
        let message = launchd_not_loaded_message(&definition, &path, "gui/501/dev.cregis.apex");

        // The three things a user needs to act on: that it is installed but
        // idle, where the plist lives, and how to start it.
        assert!(message.contains("Service installed but not loaded: gui/501/dev.cregis.apex"));
        assert!(message.contains("/Users/alice/Library/LaunchAgents/dev.cregis.apex.plist"));
        assert!(
            message
                .contains("/Users/alice/.apex/apex service start --install-dir /Users/alice/.apex")
        );
    }

    #[test]
    fn shell_quote_leaves_ordinary_paths_bare() {
        assert_eq!(shell_quote("/Users/alice/.apex"), "/Users/alice/.apex");
        assert_eq!(shell_quote("/opt/apex-2.0_beta"), "/opt/apex-2.0_beta");
    }

    #[test]
    fn shell_quote_protects_spaces_and_metacharacters() {
        assert_eq!(
            shell_quote("/Users/Alice Smith/.apex"),
            "'/Users/Alice Smith/.apex'"
        );
        assert_eq!(shell_quote("/tmp/a;rm -rf b"), "'/tmp/a;rm -rf b'");
        assert_eq!(shell_quote("/tmp/$(whoami)"), "'/tmp/$(whoami)'");
        // An embedded single quote has to close, escape, and reopen.
        assert_eq!(shell_quote("/tmp/it's"), r"'/tmp/it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn launchd_not_loaded_message_quotes_paths_with_spaces() {
        let definition = ServiceDefinition::new(
            PathBuf::from("/Users/Alice Smith/.apex"),
            PathBuf::from("/Users/Alice Smith/.apex/config.json"),
            "dev.cregis.apex".to_string(),
            ServiceManager::Launchd,
        );
        let path = PathBuf::from("/Users/Alice Smith/Library/LaunchAgents/dev.cregis.apex.plist");
        let message = launchd_not_loaded_message(&definition, &path, "gui/501/dev.cregis.apex");

        assert!(message.contains(
            "'/Users/Alice Smith/.apex/apex' service start --install-dir '/Users/Alice Smith/.apex'"
        ));
    }

    #[test]
    fn renders_systemd_unit_for_gateway_run() {
        let definition = ServiceDefinition::new(
            PathBuf::from("/opt/apex"),
            PathBuf::from("/opt/apex/config.json"),
            "apex".to_string(),
            ServiceManager::Systemd,
        );
        let unit = render_systemd_unit(&definition);
        let binary = definition.binary_path().display().to_string();
        let config_path = definition.config_path.display().to_string();
        let install_dir = definition.install_dir.display().to_string();

        assert!(unit.contains(&format!("ExecStart={binary} gateway run")));
        assert!(unit.contains(&format!("Environment=APEX_CONFIG={config_path}")));
        assert!(unit.contains(&format!("WorkingDirectory={install_dir}")));
        assert!(unit.contains("Restart=always"));
    }

    #[test]
    fn systemd_service_path_does_not_double_suffix() {
        let definition = ServiceDefinition::new(
            PathBuf::from("/opt/apex"),
            PathBuf::from("/opt/apex/config.json"),
            "apex.service".to_string(),
            ServiceManager::Systemd,
        );

        assert_eq!(
            service_path(&definition),
            PathBuf::from("/etc/systemd/system/apex.service")
        );
    }

    #[test]
    fn renders_launchd_plist_for_gateway_run() {
        let definition = ServiceDefinition::new(
            PathBuf::from("/opt/apex"),
            PathBuf::from("/opt/apex/config.json"),
            "dev.cregis.apex".to_string(),
            ServiceManager::Launchd,
        );
        let plist = render_launchd_plist(&definition);
        let binary = definition.binary_path().display().to_string();
        let config_path = definition.config_path.display().to_string();
        let stdout = definition.install_dir.join("logs").join("stdout.log");
        let stderr = definition.install_dir.join("logs").join("stderr.log");

        assert!(plist.contains(&format!("<string>{binary}</string>")));
        assert!(plist.contains("<string>gateway</string>"));
        assert!(plist.contains("<string>run</string>"));
        assert!(plist.contains("<key>APEX_CONFIG</key>"));
        assert!(plist.contains(&format!("<string>{config_path}</string>")));
        assert!(plist.contains(&format!("<string>{}</string>", stdout.display())));
        assert!(plist.contains(&format!("<string>{}</string>", stderr.display())));
    }

    #[test]
    fn launchd_domain_prefers_sudo_uid_for_user_agent() {
        assert_eq!(
            launchd_uid_from_sudo_env(Some("501")).as_deref(),
            Some("501")
        );
        assert_eq!(launchd_uid_from_sudo_env(Some("0")), None);
        assert_eq!(launchd_domain_for_uid("501"), "gui/501");
        assert_eq!(
            launchd_target("gui/501", "dev.cregis.apex"),
            "gui/501/dev.cregis.apex"
        );
    }

    #[test]
    fn user_home_prefers_sudo_user_home() {
        assert_eq!(
            user_home_dir_for(Some("alice"), Some(PathBuf::from("/var/root"))),
            PathBuf::from("/Users/alice")
        );
        assert_eq!(
            user_home_dir_for(Some("root"), Some(PathBuf::from("/var/root"))),
            PathBuf::from("/var/root")
        );
        assert_eq!(
            user_home_dir_for(None, Some(PathBuf::from("/Users/bob"))),
            PathBuf::from("/Users/bob")
        );
        assert_eq!(
            user_home_dir_for(Some(""), Some(PathBuf::from("/Users/bob"))),
            PathBuf::from("/Users/bob")
        );
    }
}
