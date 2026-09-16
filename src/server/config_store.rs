//! The single write path for runtime configuration changes.
//!
//! Every admin mutation goes through `commit_config`: it validates and mutates
//! a private candidate under the write lock, persists that candidate to disk,
//! and only then swaps it into the in-memory `Config`. The pricing endpoint
//! shares it, which is why this lives beside the admin handlers rather than
//! inside them.

use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Response, StatusCode};

use crate::config::Config;

use super::AppState;
use super::errors::error_response;

pub(super) fn persist_config(config: &Config) -> Result<(), String> {
    let path = PathBuf::from(&config.hot_reload.config_path);
    if path.as_os_str().is_empty() {
        return Err("hot_reload.config_path is empty".into());
    }
    crate::config::save_config(&path, config).map_err(|e| e.to_string())
}

/// Atomically apply a configuration mutation.
///
/// This is the single write path shared by every admin CRUD handler. It closes
/// two classes of bug that arise when validation, mutation and persistence are
/// done across separate lock acquisitions:
///
///   * **TOCTOU** — the closure runs while the write lock is held and validates
///     against a private `candidate` clone of the *current* config, so a
///     concurrent writer can't invalidate a check between validate and apply.
///   * **memory/disk divergence** — the candidate is persisted to disk *before*
///     it is committed to the in-memory `Config`. If the disk write fails, the
///     live config is left untouched and the handler returns an error, instead
///     of silently keeping an unpersisted change that vanishes on restart.
///
/// The closure receives `&mut Config` (the candidate) and returns either a
/// success value (used to build the response) or an error `Response` to abort
/// the whole operation with no change.
///
/// Note: the file write happens while the write lock is held. Admin mutations
/// are rare and the proxy hot-path only takes *read* locks, so the brief stall
/// is an acceptable tradeoff for atomicity.
pub(super) fn commit_config<T>(
    state: &AppState,
    mutate: impl FnOnce(&mut Config) -> Result<T, Response<Body>>,
) -> Result<T, Response<Body>> {
    let mut guard = state.config.write().unwrap();
    let mut candidate = guard.clone();
    let value = mutate(&mut candidate)?;
    if let Err(err) = persist_config(&candidate) {
        tracing::error!("Failed to persist config change: {err}");
        return Err(error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to persist config: {err}"),
        ));
    }
    *guard = candidate;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderType;
    use crate::server::test_fixtures::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    // ----- commit_config atomicity ------------------------------------

    #[test]
    fn commit_config_persists_and_commits_on_success() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("config.json");
        let mut config = create_test_config();
        config.hot_reload.config_path = cfg_path.to_string_lossy().to_string();
        let (state, _db_dir) = state_with_config(config);

        let result = commit_config(&state, |cfg| {
            Arc::make_mut(&mut cfg.channels).push(crate::config::Channel {
                name: "added".to_string(),
                provider_type: ProviderType::Openai,
                base_url: "http://x".to_string(),
                api_key: "sk-x".to_string(),
                anthropic_base_url: None,
                headers: None,
                model_map: None,
                timeouts: None,
                pricing: None,
            });
            Ok::<_, Response<Body>>(())
        });
        assert!(result.is_ok());

        // In-memory committed
        assert!(
            state
                .config
                .read()
                .unwrap()
                .channels
                .iter()
                .any(|c| c.name == "added")
        );
        // Disk persisted
        let on_disk = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(on_disk.contains("\"added\""));
    }

    #[test]
    fn commit_config_does_not_commit_when_persist_fails() {
        let mut config = create_test_config();
        // Empty path makes persist_config fail deterministically.
        config.hot_reload.config_path = String::new();
        let before = config.channels.len();
        let (state, _db_dir) = state_with_config(config);

        let result = commit_config(&state, |cfg| {
            Arc::make_mut(&mut cfg.channels).push(crate::config::Channel {
                name: "ghost".to_string(),
                provider_type: ProviderType::Openai,
                base_url: "http://x".to_string(),
                api_key: "sk-x".to_string(),
                anthropic_base_url: None,
                headers: None,
                model_map: None,
                timeouts: None,
                pricing: None,
            });
            Ok::<_, Response<Body>>(())
        });
        assert!(result.is_err(), "persist failure should surface as Err");

        // In-memory MUST be untouched — no divergence from disk.
        let after = state.config.read().unwrap().channels.len();
        assert_eq!(after, before);
        assert!(
            !state
                .config
                .read()
                .unwrap()
                .channels
                .iter()
                .any(|c| c.name == "ghost")
        );
    }

    #[test]
    fn commit_config_aborts_without_change_when_closure_errors() {
        let dir = tempdir().unwrap();
        let cfg_path = dir.path().join("config.json");
        let mut config = create_test_config();
        config.hot_reload.config_path = cfg_path.to_string_lossy().to_string();
        let before = config.channels.len();
        let (state, _db_dir) = state_with_config(config);

        let result = commit_config(&state, |cfg| {
            Arc::make_mut(&mut cfg.channels).clear();
            Err::<(), _>(error_response(StatusCode::CONFLICT, "nope"))
        });
        assert!(result.is_err());
        // Closure error => no persist, no commit.
        assert_eq!(state.config.read().unwrap().channels.len(), before);
        assert!(
            !cfg_path.exists(),
            "must not have written config on closure error"
        );
    }
}
