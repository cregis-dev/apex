use apex::config::{Auth, AuthMode, Channel, MatchSpec, ProviderType, Router as GatewayRouter, RouterRule, TargetChannel};
use apex::server::{build_app, build_state};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

mod common;

#[tokio::test]
async fn test_global_auth_with_x_api_key() {
    // Setup upstream that handles /v1/messages
    let upstream = {
        let app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(|_: axum::body::Bytes| async { 
                (StatusCode::OK, axum::Json(json!({
                    "id": "msg_123",
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": "claude-3",
                    "stop_reason": "end_turn",
                    "stop_sequence": null,
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }))) 
            }),
        );
        common::spawn_app(app).await
    };

    // Setup config with global auth
    let mut config = common::base_config();
    config.global.auth = Auth {
        mode: AuthMode::ApiKey,
        keys: Some(vec!["test-key".to_string()]),
    };

    // Add a channel and router
    config.channels.push(Channel {
        name: "primary".to_string(),
        provider_type: ProviderType::Anthropic,
        base_url: format!("http://{}", upstream),
        api_key: "".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
    });
    config.routers.push(GatewayRouter {
        name: "r1".to_string(),
        vkey: Some("test-key".to_string()),
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
            strategy: "round_robin".to_string(),
        }],
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    // Test request with x-api-key matching global auth key AND router vkey
    let req = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .header("x-api-key", "test-key") 
        .body(Body::from(json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hi"}]
        }).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    
    assert_eq!(resp.status(), StatusCode::OK); 
}
