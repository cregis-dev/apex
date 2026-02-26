mod common;
use common::*;

use apex::config::{
    Auth, AuthMode, Channel, MatchSpec, ProviderType, Router as GatewayRouter, RouterRule,
    TargetChannel,
};
use apex::server::{build_app, build_state};
use axum::body::Body;
use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_openai_route_success() {
    let upstream = spawn_upstream_ok().await;
    ensure_upstream_ok(upstream, "/v1/chat/completions").await;
    let mut config = base_config();
    config.channels.push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    config.routers.push(GatewayRouter {
        name: "r1".to_string(),
        channels: vec![TargetChannel {
            name: "primary".to_string(),
            weight: 1,
        }],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec![],
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
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer vk_test")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_global_auth_required() {
    let upstream = spawn_upstream_ok().await;
    let mut config = base_config();
    config.global.auth = Auth {
        mode: AuthMode::ApiKey,
        keys: Some(vec!["key1".to_string()]),
    };
    config.channels.push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    config.routers.push(GatewayRouter {
        name: "r1".to_string(),
        channels: vec![TargetChannel {
            name: "primary".to_string(),
            weight: 1,
        }],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec![],
        rules: vec![],
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer vk_test")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{}", body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_fallback_on_failure() {
    let primary = spawn_upstream_status(StatusCode::INTERNAL_SERVER_ERROR, "fail").await;
    let fallback = spawn_upstream_status(StatusCode::OK, "ok").await;
    let mut config = base_config();
    config.channels.push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(primary),
        api_key: "".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    config.channels.push(Channel {
        name: "fallback".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(fallback),
        api_key: "".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    config.routers.push(GatewayRouter {
        name: "r1".to_string(),
        channels: vec![TargetChannel {
            name: "primary".to_string(),
            weight: 1,
        }],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec!["fallback".to_string()],
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
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer vk_test")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
}
