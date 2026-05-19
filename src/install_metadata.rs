use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstallMetadata {
    pub install_dir: String,
    pub current_version: String,
    pub repo: String,
    pub config_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_manager: Option<String>,
    pub current_link: String,
    pub releases_dir: String,
    pub created_at: String,
    pub updated_at: String,
}

impl InstallMetadata {
    pub fn new(
        install_dir: PathBuf,
        current_version: String,
        repo: String,
        config_path: PathBuf,
        service_name: Option<String>,
        service_manager: Option<String>,
    ) -> Self {
        let now = Utc::now().to_rfc3339();
        let current_link = install_dir.join("current");
        let releases_dir = install_dir.join("releases");
        Self {
            install_dir: install_dir.display().to_string(),
            current_version,
            repo,
            config_path: config_path.display().to_string(),
            service_name,
            service_manager,
            current_link: current_link.display().to_string(),
            releases_dir: releases_dir.display().to_string(),
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

pub fn metadata_path(path_or_install_dir: impl AsRef<Path>) -> PathBuf {
    let path = path_or_install_dir.as_ref();
    if path.file_name().and_then(|name| name.to_str()) == Some("install.json") {
        path.to_path_buf()
    } else {
        path.join("install.json")
    }
}

pub fn read_metadata(path_or_install_dir: impl AsRef<Path>) -> anyhow::Result<InstallMetadata> {
    let path = metadata_path(path_or_install_dir);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("install metadata not found at {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse install metadata at {}", path.display()))
}

pub fn write_metadata(
    install_dir: impl AsRef<Path>,
    metadata: &InstallMetadata,
) -> anyhow::Result<()> {
    let install_dir = install_dir.as_ref();
    std::fs::create_dir_all(install_dir)
        .with_context(|| format!("failed to create install dir {}", install_dir.display()))?;
    let mut metadata = metadata.clone();
    if metadata.created_at.trim().is_empty() {
        metadata.created_at = Utc::now().to_rfc3339();
    }
    metadata.updated_at = Utc::now().to_rfc3339();
    let path = metadata_path(install_dir);
    let content = serde_json::to_string_pretty(&metadata)?;
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write install metadata at {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trips() {
        let temp = tempfile::tempdir().unwrap();
        let metadata = InstallMetadata::new(
            temp.path().to_path_buf(),
            "v1.2.3".to_string(),
            "cregis-dev/apex".to_string(),
            temp.path().join("config.json"),
            Some("apex".to_string()),
            Some("systemd".to_string()),
        );

        write_metadata(temp.path(), &metadata).unwrap();
        let read = read_metadata(temp.path()).unwrap();

        assert_eq!(read.install_dir, metadata.install_dir);
        assert_eq!(read.current_version, "v1.2.3");
        assert_eq!(read.repo, "cregis-dev/apex");
        assert_eq!(read.service_name.as_deref(), Some("apex"));
        assert_eq!(read.service_manager.as_deref(), Some("systemd"));
    }

    #[test]
    fn missing_metadata_errors() {
        let temp = tempfile::tempdir().unwrap();
        let err = read_metadata(temp.path()).unwrap_err().to_string();
        assert!(err.contains("install metadata not found"));
    }
}
