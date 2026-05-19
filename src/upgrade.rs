use crate::install_metadata::{self, InstallMetadata};
use crate::service::{self, ServiceDefinition, ServiceManager};
use anyhow::{Context, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct UpgradeOptions {
    pub install_dir: Option<PathBuf>,
    pub target_version: Option<String>,
    pub restart: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradePlan {
    pub install_dir: PathBuf,
    pub current_version: String,
    pub target_version: String,
    pub repo: String,
    pub artifact_name: String,
    pub archive_name: String,
    pub current_link: PathBuf,
    pub previous_release: PathBuf,
    pub target_release: PathBuf,
    pub service_manager: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

pub async fn run_upgrade(options: UpgradeOptions) -> anyhow::Result<()> {
    let install_dir = options
        .install_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from("/opt/apex"));
    let metadata = install_metadata::read_metadata(&install_dir)?;
    let plan = build_plan(&metadata, options.target_version.clone()).await?;

    print_plan(&plan, options.dry_run, options.restart);
    if options.dry_run {
        return Ok(());
    }

    let temp_dir = make_temp_dir()?;
    let result = execute_upgrade(&metadata, &plan, &temp_dir, options.restart).await;
    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}

pub async fn build_plan(
    metadata: &InstallMetadata,
    target_version: Option<String>,
) -> anyhow::Result<UpgradePlan> {
    let target_version = match target_version {
        Some(version) => version,
        None => latest_release_tag(&metadata.repo).await?,
    };
    let artifact_name = platform_artifact_name()?;
    let install_dir = PathBuf::from(&metadata.install_dir);
    let current_link = PathBuf::from(&metadata.current_link);
    let releases_dir = PathBuf::from(&metadata.releases_dir);
    Ok(UpgradePlan {
        install_dir,
        current_version: metadata.current_version.clone(),
        target_release: releases_dir.join(&target_version),
        previous_release: releases_dir.join(&metadata.current_version),
        target_version,
        repo: metadata.repo.clone(),
        archive_name: format!("{artifact_name}.tar.gz"),
        artifact_name,
        current_link,
        service_manager: metadata.service_manager.clone(),
    })
}

async fn execute_upgrade(
    metadata: &InstallMetadata,
    plan: &UpgradePlan,
    temp_dir: &Path,
    restart: bool,
) -> anyhow::Result<()> {
    let archive_path = temp_dir.join(&plan.archive_name);
    let checksum_path = temp_dir.join("checksums.txt");
    let base_url = release_base_url(&plan.repo, &plan.target_version);
    download_file(&format!("{base_url}/{}", plan.archive_name), &archive_path).await?;
    if download_file(&format!("{base_url}/checksums.txt"), &checksum_path)
        .await
        .is_ok()
    {
        verify_checksum(&archive_path, &checksum_path)?;
    } else {
        println!("Warning: checksums.txt not found; skipping checksum verification");
    }

    if plan.target_release.exists() && plan.target_release.read_dir()?.next().is_some() {
        bail!(
            "target release directory already exists and is not empty: {}",
            plan.target_release.display()
        );
    }
    std::fs::create_dir_all(&plan.target_release)
        .with_context(|| format!("failed to create {}", plan.target_release.display()))?;
    extract_archive(&archive_path, &plan.target_release)?;
    validate_new_binary(
        &plan.target_release.join("apex"),
        Path::new(&metadata.config_path),
    )?;

    switch_current_symlink(&plan.current_link, &plan.target_release)?;

    let mut updated = metadata.clone();
    updated.current_version = plan.target_version.clone();
    install_metadata::write_metadata(&plan.install_dir, &updated)?;

    if restart {
        let definition = service_definition_from_metadata(metadata)?;
        if let Err(err) = restart_and_verify(&definition) {
            eprintln!("Restart verification failed: {err}");
            switch_current_symlink(&plan.current_link, &plan.previous_release)?;
            let mut rolled_back = metadata.clone();
            rolled_back.current_version = plan.current_version.clone();
            install_metadata::write_metadata(&plan.install_dir, &rolled_back)?;
            let _ = service::restart_service(&definition);
            bail!(
                "upgrade rolled back to {} after failed restart",
                plan.current_version
            );
        }
    }

    println!("Upgrade complete: {}", plan.target_version);
    Ok(())
}

fn print_plan(plan: &UpgradePlan, dry_run: bool, restart: bool) {
    println!("Apex upgrade plan");
    println!("  current version: {}", plan.current_version);
    println!("  target version: {}", plan.target_version);
    println!("  artifact: {}", plan.archive_name);
    println!("  install dir: {}", plan.install_dir.display());
    println!(
        "  service manager: {}",
        plan.service_manager.as_deref().unwrap_or("none")
    );
    println!("  restart: {}", restart);
    if dry_run {
        println!("Dry run: no files or services will be changed");
    }
}

async fn latest_release_tag(repo: &str) -> anyhow::Result<String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let release = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "apex-upgrade")
        .send()
        .await?
        .error_for_status()?
        .json::<GitHubRelease>()
        .await?;
    Ok(release.tag_name)
}

fn release_base_url(repo: &str, version: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{version}")
}

pub fn platform_artifact_name() -> anyhow::Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Ok("apex-x86_64-linux".to_string()),
        ("linux", "aarch64") => Ok("apex-aarch64-linux".to_string()),
        ("macos", "x86_64") => Ok("apex-x86_64-macos".to_string()),
        ("macos", "aarch64") => Ok("apex-aarch64-macos".to_string()),
        _ => bail!("unsupported platform for Apex release artifact: {os}/{arch}"),
    }
}

async fn download_file(url: &str, dest: &Path) -> anyhow::Result<()> {
    let bytes = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "apex-upgrade")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    std::fs::write(dest, bytes).with_context(|| format!("failed to write {}", dest.display()))
}

fn verify_checksum(archive_path: &Path, checksum_path: &Path) -> anyhow::Result<()> {
    let archive_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("archive path has no filename")?;
    let checksums = std::fs::read_to_string(checksum_path)?;
    let expected = checksums
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let checksum = parts.next()?;
            let file = parts.next()?;
            (file == archive_name).then(|| checksum.to_string())
        })
        .next();
    let Some(expected) = expected else {
        println!("Warning: checksum entry not found for {archive_name}; skipping verification");
        return Ok(());
    };
    let actual = sha256_file(archive_path)?;
    if actual != expected {
        bail!("checksum mismatch for {archive_name}");
    }
    Ok(())
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let output = Command::new("sha256sum").arg(path).output().or_else(|_| {
        Command::new("shasum")
            .arg("-a")
            .arg("256")
            .arg(path)
            .output()
    })?;
    if !output.status.success() {
        bail!("failed to calculate sha256 for {}", path.display());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .split_whitespace()
        .next()
        .map(str::to_string)
        .context("checksum command returned empty output")
}

fn extract_archive(archive_path: &Path, release_dir: &Path) -> anyhow::Result<()> {
    let output = Command::new("tar")
        .arg("-xzf")
        .arg(archive_path)
        .arg("-C")
        .arg(release_dir)
        .arg("--strip-components")
        .arg("1")
        .output()
        .context("failed to run tar")?;
    if !output.status.success() {
        bail!(
            "failed to extract release archive: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn validate_new_binary(binary: &Path, config_path: &Path) -> anyhow::Result<()> {
    command_success(Command::new(binary).arg("--version"))?;
    command_success(
        Command::new(binary)
            .arg("-c")
            .arg(config_path)
            .arg("config")
            .arg("validate"),
    )
}

fn command_success(command: &mut Command) -> anyhow::Result<()> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }
    bail!(
        "command failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    )
}

fn switch_current_symlink(current_link: &Path, target_release: &Path) -> anyhow::Result<()> {
    if current_link.exists() || current_link.symlink_metadata().is_ok() {
        let metadata = current_link.symlink_metadata()?;
        if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            std::fs::remove_dir(current_link)?;
        } else {
            std::fs::remove_file(current_link)?;
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target_release, current_link)
            .with_context(|| format!("failed to update {}", current_link.display()))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = target_release;
        bail!("symlink upgrades are not supported on this platform")
    }
}

fn service_definition_from_metadata(
    metadata: &InstallMetadata,
) -> anyhow::Result<ServiceDefinition> {
    let manager = match metadata.service_manager.as_deref() {
        Some("systemd") => ServiceManager::Systemd,
        Some("launchd") => ServiceManager::Launchd,
        Some(other) => bail!("unsupported service manager in metadata: {other}"),
        None => ServiceManager::detect()?,
    };
    Ok(ServiceDefinition::new(
        PathBuf::from(&metadata.install_dir),
        PathBuf::from(&metadata.config_path),
        metadata
            .service_name
            .clone()
            .unwrap_or_else(|| service::default_service_name_for(manager).to_string()),
        manager,
    ))
}

fn restart_and_verify(definition: &ServiceDefinition) -> anyhow::Result<()> {
    service::restart_service(definition)?;
    if service::service_is_active(definition) {
        return Ok(());
    }
    bail!("service is not active after restart")
}

fn make_temp_dir() -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "apex-upgrade-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(temp: &Path) -> InstallMetadata {
        InstallMetadata {
            install_dir: temp.display().to_string(),
            current_version: "v0.1.0".to_string(),
            repo: "cregis-dev/apex".to_string(),
            config_path: temp.join("config.json").display().to_string(),
            service_name: Some("apex".to_string()),
            service_manager: Some("systemd".to_string()),
            current_link: temp.join("current").display().to_string(),
            releases_dir: temp.join("releases").display().to_string(),
            created_at: "2026-05-18T00:00:00Z".to_string(),
            updated_at: "2026-05-18T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn platform_artifact_name_matches_release_contract() {
        let artifact = platform_artifact_name().unwrap();
        if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            assert_eq!(artifact, "apex-x86_64-linux");
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            assert_eq!(artifact, "apex-aarch64-macos");
        } else {
            assert!(artifact.starts_with("apex-"));
        }
    }

    #[tokio::test]
    async fn build_plan_uses_versioned_release_paths() {
        let temp = tempfile::tempdir().unwrap();
        let plan = build_plan(&metadata(temp.path()), Some("v0.2.0".to_string()))
            .await
            .unwrap();

        assert_eq!(plan.current_version, "v0.1.0");
        assert_eq!(plan.target_version, "v0.2.0");
        assert_eq!(plan.current_link, temp.path().join("current"));
        assert_eq!(plan.previous_release, temp.path().join("releases/v0.1.0"));
        assert_eq!(plan.target_release, temp.path().join("releases/v0.2.0"));
        assert!(plan.archive_name.ends_with(".tar.gz"));
    }
}
