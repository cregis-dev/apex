mod common;
use apex::config::{
    Channel, MatchSpec, Metrics, ProviderType, Router as GatewayRouter, RouterRule, TargetChannel,
    Team,
};
use apex::server::{build_app, build_state};
use axum::body::Body;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_observability_metrics() {
    let upstream = spawn_upstream_ok().await;

    // Config with Metrics Enabled
    let mut config = base_config();
    config.metrics = Metrics {
        enabled: true,
        path: "/metrics".to_string(),
    };

    // Channel & Router
    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "test_channel".to_string(),
        provider_type: ProviderType::Openai,
        base_url: base_url(upstream),
        api_key: "sk-test".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    std::sync::Arc::make_mut(&mut config.routers).push(GatewayRouter {
        name: "test_router".to_string(),
        channels: vec![],
        strategy: "priority".to_string(),
        metadata: None,
        fallback_channels: vec![],
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![TargetChannel {
                name: "test_channel".to_string(),
                weight: 1,
            }],
            strategy: "priority".to_string(),
        }],
    });

    // 1. Send a request to generate metrics
    // Set up a team for team_auth
    config.global.auth.mode = apex::config::AuthMode::ApiKey;
    config.global.auth.keys = Some(vec!["sk-global-key".to_string()]);

    // Add a team with API key
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "test-team".to_string(),
        api_key: "sk-test".to_string(),
        policy: apex::config::TeamPolicy {
            allowed_routers: vec!["test_router".to_string()],
            allowed_models: None,
            rate_limit: None,
        },
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer sk-test")
        .body(Body::from(json!({"model":"test"}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Fetch Metrics (using global key for global_auth)
    let req = axum::http::Request::builder()
        .method("GET")
        .uri("/metrics")
        .header("Authorization", "Bearer sk-global-key")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let (_status, body) = response_text(resp).await;

    // 3. Verify Metrics Content
    assert!(
        body.contains("apex_requests_total"),
        "Should contain requests total metric"
    );
    assert!(body.contains("test_router"), "Should contain router label");
}
