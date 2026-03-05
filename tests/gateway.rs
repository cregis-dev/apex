mod common;
use common::*;

use apex::config::{
    Channel, MatchSpec, ProviderType, Router as GatewayRouter, RouterRule, TargetChannel, Team,
    TeamPolicy, TeamRateLimit,
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
    // Add team for strict auth
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "test-team".to_string(),
        api_key: "vk_test".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: None,
            rate_limit: None,
        },
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
    let resp = app.clone().oneshot(req).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_global_auth_required() {
    let upstream = spawn_upstream_ok().await;
    let mut config = base_config();
    config.global.auth_keys = vec!["key1".to_string()];
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
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_fallback_on_failure() {
    let upstream_bad = spawn_upstream_status(StatusCode::INTERNAL_SERVER_ERROR, "error").await;
    let upstream_good = spawn_upstream_ok().await;

    let mut config = base_config();
    // Add team for strict auth
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "test-team".to_string(),
        api_key: "sk-test".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: None,
            rate_limit: None,
        },
    });
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
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_list_requires_global_auth() {
    let mut config = base_config();
    config.global.auth_keys = vec!["admin-key".to_string()];

    let state = build_state(config).unwrap();
    let app = build_app(state);
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/admin/teams")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_list_masks_keys() {
    let mut config = base_config();
    config.global.auth_keys = vec!["admin-key".to_string()];
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "team-a".to_string(),
        api_key: "sk-ant-abcdef123456".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: None,
            rate_limit: Some(TeamRateLimit {
                rpm: Some(10),
                tpm: None,
            }),
        },
    });
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "team-b".to_string(),
        api_key: "short".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: None,
            rate_limit: None,
        },
    });
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: "http://localhost:8080".to_string(),
        api_key: "sk-channel-abcdef".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/admin/teams")
        .header("Authorization", "Bearer admin-key")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let api_key = value["data"][0]["api_key"].as_str().unwrap();
    assert!(api_key.starts_with("sk-"));
    assert!(api_key.ends_with("3456"));
    assert_ne!(api_key, "sk-ant-abcdef123456");
    let api_key = value["data"][1]["api_key"].as_str().unwrap();
    assert_eq!(api_key, "*****");

    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/admin/channels")
        .header("Authorization", "Bearer admin-key")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let api_key = value["data"][0]["api_key"].as_str().unwrap();
    assert!(api_key.starts_with("sk-"));
    assert!(api_key.ends_with("cdef"));
    assert_ne!(api_key, "sk-channel-abcdef");
}
