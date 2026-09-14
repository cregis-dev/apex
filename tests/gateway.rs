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
        group: None,
        enabled: None,
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
        pricing: None,
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
            session_affinity: false,
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
async fn e2e_openai_embeddings_route_forwards_path_and_body() {
    let (upstream, captures) = spawn_upstream_capture(
        StatusCode::OK,
        r#"{"object":"list","data":[{"object":"embedding","index":0,"embedding":[0.1,0.2,0.3]}],"model":"text-embedding-3-small","usage":{"prompt_tokens":4,"total_tokens":4}}"#,
    )
    .await;
    ensure_upstream_ok(upstream, "/v1/embeddings").await;

    let mut config = base_config();
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "test-team".to_string(),
        api_key: "vk_test".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: None,
            rate_limit: None,
        },
        group: None,
        enabled: None,
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
        pricing: None,
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
            session_affinity: false,
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
        .uri("/v1/embeddings")
        .header("content-type", "application/json")
        .header("Authorization", "Bearer vk_test")
        .body(Body::from(
            json!({
                "model":"text-embedding-3-small",
                "input":"hello embeddings"
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);

    let body_value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body_value["object"], "list");
    assert_eq!(body_value["data"][0]["object"], "embedding");

    let captured = captures.lock().unwrap();
    let captured_post = captured
        .iter()
        .find(|request| request.method == "POST")
        .expect("expected captured POST request");
    assert_eq!(captured_post.path, "/v1/embeddings");
    let captured_body: serde_json::Value = serde_json::from_str(&captured_post.body).unwrap();
    assert_eq!(captured_body["model"], "text-embedding-3-small");
    assert_eq!(captured_body["input"], "hello embeddings");
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
            session_affinity: false,
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
        pricing: None,
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
        group: None,
        enabled: None,
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
        pricing: None,
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
        pricing: None,
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
            session_affinity: false,
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
        group: None,
        enabled: None,
    });
    std::sync::Arc::make_mut(&mut config.teams).push(Team {
        id: "team-b".to_string(),
        api_key: "short".to_string(),
        policy: TeamPolicy {
            allowed_routers: vec!["r1".to_string()],
            allowed_models: None,
            rate_limit: None,
        },
        group: None,
        enabled: None,
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
        pricing: None,
    });

    let state = build_state(config).unwrap();
    let app = build_app(state);

    let get = |uri: &'static str| {
        axum::http::Request::builder()
            .method("GET")
            .uri(uri)
            .header("Authorization", "Bearer admin-key")
            .body(Body::empty())
            .unwrap()
    };

    // List endpoints must stay secret-free: no api_key field at all.
    let resp = app.clone().oneshot(get("/admin/teams")).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(value["data"][0].get("api_key").is_none() || value["data"][0]["api_key"].is_null());

    // Masked keys come from the dedicated reveal endpoint.
    let resp = app
        .clone()
        .oneshot(get("/admin/teams/api_keys"))
        .await
        .unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let api_key = value["data"][0]["api_key"].as_str().unwrap();
    assert!(api_key.starts_with("sk-"));
    assert!(api_key.ends_with("3456"));
    assert_ne!(api_key, "sk-ant-abcdef123456");
    // Short secrets are fully starred, never returned in (near-)plaintext.
    let api_key = value["data"][1]["api_key"].as_str().unwrap();
    assert_eq!(api_key, "*****");

    // Channels: list is secret-free, masked key via the reveal endpoint.
    let resp = app.clone().oneshot(get("/admin/channels")).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(value["data"][0].get("api_key").is_none() || value["data"][0]["api_key"].is_null());

    let resp = app.oneshot(get("/admin/channels/api_keys")).await.unwrap();
    let (status, body) = response_text(resp).await;
    assert_eq!(status, StatusCode::OK, "{}", body);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let api_key = value["data"][0]["api_key"].as_str().unwrap();
    assert!(api_key.starts_with("sk-"));
    assert!(api_key.ends_with("cdef"));
    assert_ne!(api_key, "sk-channel-abcdef");
}

/// `model_map` must survive the full control-plane round trip: the create/update
/// handlers already accepted it, but the read endpoints used to drop it, so the
/// UI reopened an edit form with the mapping silently missing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_channel_model_map_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join("config.json");

    let mut config = base_config();
    config.global.auth_keys = vec!["admin-key".to_string()];
    config.hot_reload.config_path = cfg_path.to_string_lossy().to_string();

    let state = build_state(config).unwrap();
    let app = build_app(state);

    let send = |method: &'static str, uri: &'static str, body: Option<serde_json::Value>| {
        let app = app.clone();
        async move {
            let builder = axum::http::Request::builder()
                .method(method)
                .uri(uri)
                .header("Authorization", "Bearer admin-key")
                .header("content-type", "application/json");
            let req = match body {
                Some(v) => builder.body(Body::from(v.to_string())).unwrap(),
                None => builder.body(Body::empty()).unwrap(),
            };
            let resp = app.oneshot(req).await.unwrap();
            let (status, text) = response_text(resp).await;
            let value: serde_json::Value = serde_json::from_str(&text).unwrap_or(json!(null));
            (status, value)
        }
    };

    // Create with a mapping — the create response echoes it back.
    let (status, created) = send(
        "POST",
        "/admin/channels",
        Some(json!({
            "name": "mm",
            "provider_type": "minimax",
            "base_url": "https://api.minimax.io/v1",
            "api_key": "sk-mm",
            "model_map": { "claude-sonnet-4": "MiniMax-M2" },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["model_map"]["claude-sonnet-4"], "MiniMax-M2");

    // The list endpoint is what the edit form reads back.
    let (status, listed) = send("GET", "/admin/channels", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert_eq!(
        listed["data"][0]["model_map"]["claude-sonnet-4"],
        "MiniMax-M2"
    );

    // An object replaces the whole map rather than merging into it.
    let (status, updated) = send(
        "PATCH",
        "/admin/channels/mm",
        Some(json!({ "model_map": { "gpt-4": "MiniMax-Text-01" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["model_map"]["gpt-4"], "MiniMax-Text-01");
    assert!(updated["model_map"].get("claude-sonnet-4").is_none());

    // Explicit null clears it; omitting the field would have left it alone.
    let (status, cleared) = send(
        "PATCH",
        "/admin/channels/mm",
        Some(json!({ "model_map": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert!(cleared["model_map"].is_null());

    let (status, listed) = send("GET", "/admin/channels", None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert!(listed["data"][0]["model_map"].is_null());

    // And the mapping round-tripped through the on-disk config, not just memory.
    let persisted: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cfg_path).unwrap()).unwrap();
    assert_eq!(persisted["channels"][0]["name"], "mm");
}
