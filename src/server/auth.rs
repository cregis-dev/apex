//! Gateway-level authentication for the admin and control-plane surfaces.
//!
//! This is the `global.auth_keys` check that guards `/admin/*` and `/api/cp/*`.
//! Per-team API keys are a separate concern, handled in `middleware::auth`.

use axum::body::Body;
use axum::http::{HeaderMap, Response, StatusCode};

use crate::config::Config;

use super::errors::error_response;

#[allow(clippy::result_large_err)]
pub(super) fn enforce_global_auth(
    config: &Config,
    headers: &HeaderMap,
) -> Result<(), Response<Body>> {
    let keys = &config.global.auth_keys;

    // If no auth_keys configured, skip validation
    if keys.is_empty() {
        return Ok(());
    }

    let candidates = [
        read_auth_token(headers, "authorization"),
        read_auth_token(headers, "x-api-key"),
    ];

    for token in candidates.into_iter().flatten() {
        if keys.contains(&token) {
            return Ok(());
        }
    }

    tracing::warn!("Auth Failed: No valid token found in Authorization or x-api-key headers.");
    Err(error_response(StatusCode::UNAUTHORIZED, "unauthorized"))
}

fn read_auth_token(headers: &HeaderMap, key: &str) -> Option<String> {
    if let Some(val) = headers.get(key)
        && let Ok(s) = val.to_str()
    {
        if key == "authorization" && s.starts_with("Bearer ") {
            return Some(s[7..].to_string());
        }
        return Some(s.to_string());
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_auth_token() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "secret".parse().unwrap());
        assert_eq!(
            read_auth_token(&headers, "x-api-key"),
            Some("secret".to_string())
        );

        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer token".parse().unwrap());
        assert_eq!(
            read_auth_token(&headers, "authorization"),
            Some("token".to_string())
        );
    }
}
