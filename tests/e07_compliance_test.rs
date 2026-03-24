// E07: Data Compliance - PII Masking Integration Tests
mod common;

use apex::config::{
    Channel, Compliance, MatchSpec, PiiAction, PiiRule, ProviderType, Router as GatewayRouter,
    RouterRule, TargetChannel,
};
use apex::server::{build_app, build_state};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::*;
use serde_json::json;
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pii_masking_email() {
    let (upstream, captures) = spawn_upstream_capture(
        StatusCode::OK,
        r#"{"id":"test","object":"chat.completion","created":1677652288,"choices":[{"index":0,"message":{"role":"assistant","content":"Hello from upstream"},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":12,"total_tokens":21}}"#,
    )
    .await;

    // Config with PII Masking Enabled
    let mut config = base_config();
    config.compliance = Some(Compliance {
        enabled: true,
        rules: vec![PiiRule {
            name: "email".to_string(),
            pattern: r"\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b".to_string(),
            action: PiiAction::Mask,
            mask_char: '*',
            replace_with: None,
        }],
    });

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
    let state = build_state(config).expect("Failed to build state");
    let app = build_app(state);

    // Request with email PII
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Contact me at john.doe@example.com"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();

    // Request should succeed (email is masked, not blocked)
    assert_eq!(status, StatusCode::OK);

    let captured = captures.lock().unwrap();
    let upstream_body = &captured[0].body;
    assert!(!upstream_body.contains("john.doe@example.com"));
    assert!(upstream_body.contains("********************"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pii_blocking_credit_card() {
    let upstream = spawn_upstream_ok().await;

    // Config with Credit Card Blocking
    let mut config = base_config();
    config.compliance = Some(Compliance {
        enabled: true,
        rules: vec![PiiRule {
            name: "credit_card".to_string(),
            pattern: r"\b(?:\d{4}[- ]?){3}\d{4}\b".to_string(),
            action: PiiAction::Block,
            mask_char: '*',
            replace_with: Some("[CREDIT_CARD]".to_string()),
        }],
    });

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
    let state = build_state(config).expect("Failed to build state");
    let app = build_app(state);

    // Request with credit card PII
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "My card is 1234-5678-9012-3456"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let (_, body_text) = response_text(resp).await;

    // Request should be blocked
    assert_eq!(status, StatusCode::FORBIDDEN);
    let body_json: serde_json::Value = serde_json::from_str(&body_text).unwrap();
    assert_eq!(body_json["error"]["type"], "invalid_request_error");
    assert!(
        body_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("credit_card")
    );
    assert!(body_json["error"]["param"].is_null());
    assert!(body_json["error"]["code"].is_null());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pii_disabled() {
    let (upstream, captures) = spawn_upstream_capture(
        StatusCode::OK,
        r#"{"id":"test","object":"chat.completion","created":1677652288,"choices":[{"index":0,"message":{"role":"assistant","content":"Hello from upstream"},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":12,"total_tokens":21}}"#,
    )
    .await;

    // Config with PII Masking Disabled
    let mut config = base_config();
    config.compliance = Some(Compliance {
        enabled: false,
        rules: vec![],
    });

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

    let state = build_state(config).expect("Failed to build state");
    let app = build_app(state);

    // Request with email PII but masking disabled
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Contact me at test@example.com"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();

    // Request should succeed (masking disabled)
    assert_eq!(status, StatusCode::OK);

    let captured = captures.lock().unwrap();
    assert!(captured[0].body.contains("test@example.com"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pii_absent_noop() {
    let (upstream, captures) = spawn_upstream_capture(
        StatusCode::OK,
        r#"{"id":"test","object":"chat.completion","created":1677652288,"choices":[{"index":0,"message":{"role":"assistant","content":"Hello from upstream"},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":12,"total_tokens":21}}"#,
    )
    .await;

    // Config with no compliance configured
    let mut config = base_config();
    config.compliance = None;

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

    let state = build_state(config).expect("Failed to build state");
    let app = build_app(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Contact me at absent@example.com"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let captured = captures.lock().unwrap();
    assert!(captured[0].body.contains("absent@example.com"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pii_multiple_detections() {
    let upstream = spawn_upstream_ok().await;

    // Config with multiple PII rules
    let mut config = base_config();
    config.compliance = Some(Compliance {
        enabled: true,
        rules: vec![
            PiiRule {
                name: "email".to_string(),
                pattern: r"\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b".to_string(),
                action: PiiAction::Mask,
                mask_char: '*',
                replace_with: None,
            },
            PiiRule {
                name: "phone".to_string(),
                pattern: r"(\+?1[-.\s]?)?\(?[0-9]{3}\)?[-.\s]?[0-9]{3}[-.\s]?[0-9]{4}".to_string(),
                action: PiiAction::Mask,
                mask_char: 'X',
                replace_with: None,
            },
        ],
    });

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

    let state = build_state(config).expect("Failed to build state");
    let app = build_app(state);

    // Request with both email and phone PII
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Call john@example.com at (555) 123-4567"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();

    // Request should succeed (both masked)
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pii_custom_rule() {
    let upstream = spawn_upstream_ok().await;

    // Config with custom SSN rule
    let mut config = base_config();
    config.compliance = Some(Compliance {
        enabled: true,
        rules: vec![PiiRule {
            name: "ssn".to_string(),
            pattern: r"\b\d{3}-\d{2}-\d{4}\b".to_string(),
            action: PiiAction::Mask,
            mask_char: '#',
            replace_with: Some("[SSN_REDACTED]".to_string()),
        }],
    });

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

    let state = build_state(config).expect("Failed to build state");
    let app = build_app(state);

    // Request with SSN
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "SSN: 123-45-6789"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();

    // Request should succeed (SSN masked)
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_pii_masking_anthropic_route() {
    let (upstream, captures) = spawn_upstream_capture(
        StatusCode::OK,
        r#"{"id":"test","object":"chat.completion","created":1677652288,"choices":[{"index":0,"message":{"role":"assistant","content":"Hello from upstream"},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":12,"total_tokens":21}}"#,
    )
    .await;

    let mut config = base_config();
    config.compliance = Some(Compliance {
        enabled: true,
        rules: vec![PiiRule {
            name: "email".to_string(),
            pattern: r"\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b".to_string(),
            action: PiiAction::Mask,
            mask_char: '*',
            replace_with: None,
        }],
    });

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

    let state = build_state(config).expect("Failed to build state");
    let app = build_app(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/messages")
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .body(Body::from(
            json!({
                "model": "claude-3-5-sonnet",
                "max_tokens": 128,
                "messages": [
                    {"role": "user", "content": "Contact me at jane@example.com"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();

    assert_eq!(status, StatusCode::OK);

    let captured = captures.lock().unwrap();
    assert_eq!(captured[0].path, "/chat/completions");
    assert!(!captured[0].body.contains("jane@example.com"));
    assert!(captured[0].body.contains("****************"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_router_uses_original_model_when_compliance_rewrites_body() {
    let (upstream, captures) = spawn_upstream_capture(
        StatusCode::OK,
        r#"{"id":"test","object":"chat.completion","created":1677652288,"choices":[{"index":0,"message":{"role":"assistant","content":"Hello from upstream"},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":12,"total_tokens":21}}"#,
    )
    .await;

    // This rule rewrites alphabetic characters, including model values.
    let mut config = base_config();
    config.compliance = Some(Compliance {
        enabled: true,
        rules: vec![PiiRule {
            name: "letters".to_string(),
            pattern: r"[A-Za-z]".to_string(),
            action: PiiAction::Mask,
            mask_char: '*',
            replace_with: None,
        }],
    });

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
        name: "exact-router".to_string(),
        channels: vec![],
        strategy: "priority".to_string(),
        metadata: None,
        fallback_channels: vec![],
        rules: vec![RouterRule {
            match_spec: MatchSpec {
                models: vec!["gpt-4".to_string()],
            },
            channels: vec![TargetChannel {
                name: "test_channel".to_string(),
                weight: 1,
            }],
            strategy: "priority".to_string(),
        }],
    });

    let state = build_state(config).expect("Failed to build state");
    let app = build_app(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "hello world"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let captured = captures.lock().unwrap();
    assert!(captured[0].body.contains("\"***-4\""));
}
