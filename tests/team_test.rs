mod common;
use common::*;

use apex::config::{
    AuthMode, Channel, MatchSpec, ProviderType, Router as GatewayRouter, RouterRule, TargetChannel,
    Team, TeamPolicy,
};
use apex::server::{build_app, build_state};
use axum::body::Body;
use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_team_allowed_models_case_insensitive() {
    let upstream = spawn_upstream_ok().await;
    let mut config = base_config();

    // Channel
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "sk-upstream".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });

    // Router
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
        name: "r1".to_string(),
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![TargetChannel {
                name: "primary".to_string(),
                weight: 1,
            }],
            strategy: "priority".to_string(),
        }],
        channels: vec![],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec![],
    });

    // Team with Uppercase Model Config
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "team-case-test".to_string(),
        api_key: "sk-team-key".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: Some(vec!["GPT-4".to_string()]), // Uppercase config
            rate_limit: None,
        },
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // Request with Lowercase Model
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-team-key")
        .body(Body::from(json!({"model":"gpt-4"}).to_string())) // Lowercase request
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;

    // Current behavior: expect OK (200) because implementation is case-insensitive
    assert_eq!(
        status,
        StatusCode::OK,
        "Should pass due to case insensitivity"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_team_allowed_models_glob() {
    let upstream = spawn_upstream_ok().await;
    let mut config = base_config();

    // Channel
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "sk-upstream".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });

    // Router
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
        name: "r1".to_string(),
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![TargetChannel {
                name: "primary".to_string(),
                weight: 1,
            }],
            strategy: "priority".to_string(),
        }],
        channels: vec![],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec![],
    });

    // Team with Glob Pattern
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "team-glob-test".to_string(),
        api_key: "sk-team-glob-key".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: Some(vec!["gpt-*".to_string()]), // Glob pattern
            rate_limit: None,
        },
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // Request with matching model
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-team-glob-key")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;

    // Current behavior: expect OK (200) because implementation uses glob
    assert_eq!(status, StatusCode::OK, "Should pass due to glob support");

    // Request with non-matching model (should fail if Team Auth works, pass if it falls back to Global Auth)
    // Re-build app for second request? No, `oneshot` consumes app.
    // We need to rebuild or clone logic.
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_team_policy_enforcement() {
    let upstream = spawn_upstream_ok().await;
    let mut config = base_config();

    // Channel
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "sk-upstream".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });

    // Router
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
        name: "r1".to_string(),
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![TargetChannel {
                name: "primary".to_string(),
                weight: 1,
            }],
            strategy: "priority".to_string(),
        }],
        channels: vec![],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec![],
    });

    // Team that ONLY allows gpt-4
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "team-strict".to_string(),
        api_key: "sk-team-strict".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: Some(vec!["gpt-4".to_string()]),
            rate_limit: None,
        },
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // Request for disallowed model (gpt-3.5)
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-team-strict")
        .body(Body::from(json!({"model":"gpt-3.5"}).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;

    // Should be Forbidden if Team Auth worked and Policy was applied.
    // If it fell back to Global Auth, it would be OK (since Global Auth is open).
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "Should be forbidden by team policy"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_team_auth_x_api_key() {
    let upstream = spawn_upstream_ok().await;
    let mut config = base_config();

    // Channel
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Anthropic, // Anthropic provider
        base_url: base_url(upstream),
        api_key: "sk-upstream".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });

    // Router
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
        name: "r1".to_string(),
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![TargetChannel {
                name: "primary".to_string(),
                weight: 1,
            }],
            strategy: "priority".to_string(),
        }],
        channels: vec![],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec![],
    });

    // Team
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "team-anthropic".to_string(),
        api_key: "sk-ant-team-key".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: Some(vec!["claude-3".to_string()]),
            rate_limit: None,
        },
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // Request using x-api-key (Anthropic style)
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-team-key") // Use x-api-key
        .header("anthropic-version", "2023-06-01")
        .body(Body::from(
            json!({"model":"claude-3", "messages": []}).to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;

    // Should be OK if Team Auth extracted key correctly and matched team
    assert_eq!(status, StatusCode::OK, "Should pass with x-api-key");

    // To verify it WAS a team request, we can use the negative test again with x-api-key
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_team_auth_x_api_key_policy() {
    let upstream = spawn_upstream_ok().await;
    let mut config = base_config();

    // Channel & Router setup (same as above)
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Anthropic,
        base_url: base_url(upstream),
        api_key: "sk-upstream".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
        name: "r1".to_string(),
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![TargetChannel {
                name: "primary".to_string(),
                weight: 1,
            }],
            strategy: "priority".to_string(),
        }],
        channels: vec![],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec![],
    });

    // Team
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "team-anthropic-strict".to_string(),
        api_key: "sk-ant-strict".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: Some(vec!["claude-3".to_string()]), // Only allow claude-3
            rate_limit: None,
        },
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // Request for disallowed model using x-api-key
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .header("x-api-key", "sk-ant-strict")
        .header("anthropic-version", "2023-06-01")
        .body(Body::from(
            json!({"model":"claude-2", "messages": []}).to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;

    // Should be FORBIDDEN. If it falls back to Global, it would be OK.
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "Should be forbidden with x-api-key"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_invalid_key_rejection() {
    let upstream = spawn_upstream_ok().await;
    let mut config = base_config();

    // Ensure Global Auth is None (Open)
    config.global.auth.mode = AuthMode::None;

    // Channel
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "sk-upstream".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });

    // Router
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
        name: "r1".to_string(),
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![TargetChannel {
                name: "primary".to_string(),
                weight: 1,
            }],
            strategy: "priority".to_string(),
        }],
        channels: vec![],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec![],
    });

    // Add a Team (so config.teams is not empty)
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "team-valid".to_string(),
        api_key: "sk-valid-team".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: None,
            rate_limit: None,
        },
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // 1. Invalid Key in Authorization
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-invalid-key")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "Should reject invalid key in Authorization"
    );

    // 2. Invalid Key in x-api-key
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("x-api-key", "sk-invalid-key-2")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "Should reject invalid key in x-api-key"
    );

    // 3. No Key (Should pass because Global Auth is None)
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "Should pass with no key when Global Auth is None"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_valid_global_key_acceptance() {
    let upstream = spawn_upstream_ok().await;
    let mut config = base_config();

    // Set Global Auth to ApiKey with a key
    config.global.auth.mode = AuthMode::ApiKey;
    config.global.auth.keys = Some(vec!["sk-global-key".to_string()]);

    // Channel & Router (Standard)
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "sk-upstream".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
        name: "r1".to_string(),
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![TargetChannel {
                name: "primary".to_string(),
                weight: 1,
            }],
            strategy: "priority".to_string(),
        }],
        channels: vec![],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec![],
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // Valid Global Key
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-global-key")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "Should pass with valid Global Key");
}
