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
    config.global.auth = Auth {
        mode: AuthMode::ApiKey,
        keys: Some(vec!["vk_test".to_string()]),
    };
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
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
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
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
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
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
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_fallback_on_failure() {
    let upstream_bad = spawn_upstream_status(StatusCode::INTERNAL_SERVER_ERROR, "error").await;
    let upstream_good = spawn_upstream_ok().await;

    let mut config = base_config();
    config.global.auth = Auth {
        mode: AuthMode::ApiKey,
        keys: Some(vec!["sk-test".to_string()]),
    };
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "bad".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream_bad),
        api_key: "".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "good".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream_good),
        api_key: "".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
        name: "r1".to_string(),
        channels: vec![TargetChannel {
            name: "bad".to_string(),
            weight: 1,
        }],
        strategy: "round_robin".to_string(),
        metadata: None,
        fallback_channels: vec!["good".to_string()],
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![TargetChannel {
                name: "bad".to_string(),
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
        .header("Authorization", "Bearer sk-test")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
