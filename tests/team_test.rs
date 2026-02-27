mod common;
use common::*;

use apex::config::{
    Channel, MatchSpec, ProviderType, Router as GatewayRouter, RouterRule, TargetChannel, Team,
    TeamPolicy,
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
    config.channels.push(Channel {
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
    config.routers.push(GatewayRouter {
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
    config.teams.push(Team {
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
    config.channels.push(Channel {
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
    config.routers.push(GatewayRouter {
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
    config.teams.push(Team {
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
}
