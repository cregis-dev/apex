mod common;
use apex::config::{
    Channel, MatchSpec, ProviderType, Router as GatewayRouter, RouterRule, TargetChannel,
};
use apex::server::{build_app, build_state};
use axum::body::Body;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_rule_based_routing_priority() {
    // Setup upstreams
    let upstream_a = spawn_upstream_status(StatusCode::OK, r#""upstream_a""#).await;
    let upstream_b = spawn_upstream_status(StatusCode::OK, r#""upstream_b""#).await;

    // Config
    let mut config = base_config();

    // Channels
    config.channels.push(Channel {
        name: "channel_a".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream_a),
        api_key: "sk-test".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    config.channels.push(Channel {
        name: "channel_b".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream_b),
        api_key: "sk-test".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });

    // Router with Rules
    config.routers.push(GatewayRouter {
        name: "main_router".to_string(),
        channels: vec![],                    // Legacy field empty
        strategy: "round_robin".to_string(), // Default strategy ignored by rules
        metadata: None,
        fallback_channels: vec![],
        rules: vec![
            // Rule 1: Exact match "gpt-4" -> Channel A
            RouterRule {
                match_spec: MatchSpec {
                    models: vec!["gpt-4".to_string()],
                },
                channels: vec![TargetChannel {
                    name: "channel_a".to_string(),
                    weight: 1,
                }],
                strategy: "priority".to_string(),
            },
            // Rule 2: Glob match "gpt-*" -> Channel B
            RouterRule {
                match_spec: MatchSpec {
                    models: vec!["gpt-*".to_string()],
                },
                channels: vec![TargetChannel {
                    name: "channel_b".to_string(),
                    weight: 1,
                }],
                strategy: "priority".to_string(),
            },
        ],
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // Test 1: gpt-4 -> Channel A
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-test")
        .body(Body::from(json!({"model":"gpt-4"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "\"upstream_a\"");

    // Test 2: gpt-3.5 -> Channel B (Glob match)
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-test")
        .body(Body::from(json!({"model":"gpt-3.5"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "\"upstream_b\"");

    // Test 3: claude -> No Match -> 400 Bad Request (No matching router/rule)
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-test")
        .body(Body::from(json!({"model":"claude"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let (status, _body) = response_text(resp).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
