mod common;
use common::*;
use apex::config::{
    Channel, MatchSpec, Metrics, ProviderType, Router as GatewayRouter, RouterRule, TargetChannel,
};
use apex::server::{build_app, build_state};
use axum::body::Body;
use axum::http::StatusCode;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_observability_metrics() {
    let upstream = spawn_upstream_ok().await;

    // Config with Metrics Enabled
    let mut config = base_config();
    config.metrics = Metrics {
        enabled: true,
        listen: "127.0.0.1:0".to_string(),
        path: "/metrics".to_string(),
    };
    
    // Channel & Router
    config.channels.push(Channel {
        name: "test_channel".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "sk-test".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    config.routers.push(GatewayRouter {
        name: "test_router".to_string(),
        channels: vec![],
        strategy: "priority".to_string(),
        metadata: None,
        fallback_channels: vec![],
        rules: vec![RouterRule {
            match_spec: MatchSpec { models: vec!["*".to_string()] },
            channels: vec![TargetChannel { name: "test_channel".to_string(), weight: 1 }],
            strategy: "priority".to_string(),
        }],
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // 1. Send a request to generate metrics
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-test")
        .body(Body::from(json!({"model":"test"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Fetch Metrics
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    
    let (_status, body) = response_text(resp).await;
    
    // 3. Verify Metrics Content
    assert!(body.contains("apex_requests_total"), "Should contain requests total metric");
    assert!(body.contains("test_router"), "Should contain router label");
}
