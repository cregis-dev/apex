mod harness;

use apex::config::{Channel, ProviderType, TargetChannel, save_config};
use axum::http::StatusCode;
use harness::config_builder::write_config;
use harness::env::E2eEnv;
use harness::gateway_process::{GatewayProcess, pick_listen_addr};
use harness::mock_provider::MockProvider;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_blackbox_openai_chat_and_models_work() {
    let upstream = MockProvider::spawn("mock-openai").await.unwrap();
    let listen = pick_listen_addr().unwrap();
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("generated.e2e.config.json");

    let env = E2eEnv::from_str(&format!(
        r#"
APEX_E2E_LISTEN={listen}
APEX_E2E_TEAM_ID=blackbox-team
APEX_E2E_TEAM_KEY=sk-blackbox-team
APEX_E2E_ADMIN_KEY=sk-blackbox-admin
APEX_E2E_ROUTER_NAME=blackbox-router
APEX_E2E_TEST_MODEL=apex-test-chat

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=mock_openai
APEX_UPSTREAM_1_TYPE=openai
APEX_UPSTREAM_1_BASE_URL={}
APEX_UPSTREAM_1_MODEL=mock-openai-model
"#,
        upstream.base_url()
    ))
    .unwrap();

    write_config(&env, &config_path).unwrap();

    let mut gateway = GatewayProcess::spawn(&config_path, &listen).unwrap();
    gateway.wait_until_ready(Duration::from_secs(10)).unwrap();

    let client = Client::new();

    let chat = client
        .post(format!("{}/v1/chat/completions", gateway.base_url()))
        .header("Authorization", "Bearer sk-blackbox-team")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "apex-test-chat",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(chat.status(), 200, "gateway logs:\n{}", gateway.read_logs());
    let chat_body: serde_json::Value = chat.json().await.unwrap();
    assert_eq!(
        chat_body["choices"][0]["message"]["content"],
        "response from mock-openai"
    );

    let models = client
        .get(format!("{}/v1/models", gateway.base_url()))
        .header("Authorization", "Bearer sk-blackbox-team")
        .send()
        .await
        .unwrap();
    assert_eq!(
        models.status(),
        200,
        "gateway logs:\n{}",
        gateway.read_logs()
    );
    let models_body: serde_json::Value = models.json().await.unwrap();
    assert_eq!(models_body["object"], "list");

    let metrics = client
        .get(format!("{}/metrics", gateway.base_url()))
        .header("Authorization", "Bearer sk-blackbox-admin")
        .send()
        .await
        .unwrap();
    assert_eq!(
        metrics.status(),
        200,
        "gateway logs:\n{}",
        gateway.read_logs()
    );
    let metrics_body = metrics.text().await.unwrap();
    assert!(metrics_body.contains("apex_requests_total"));
    assert!(metrics_body.contains("blackbox-router"));

    let usage = client
        .get(format!("{}/api/usage?limit=5", gateway.base_url()))
        .header("Authorization", "Bearer sk-blackbox-admin")
        .send()
        .await
        .unwrap();
    assert_eq!(usage.status(), 200, "gateway logs:\n{}", gateway.read_logs());
    let usage_body: serde_json::Value = usage.json().await.unwrap();
    assert_eq!(usage_body["total"], 1);
    assert_eq!(usage_body["data"][0]["team_id"], "blackbox-team");
    assert_eq!(usage_body["data"][0]["router"], "blackbox-router");
    assert_eq!(usage_body["data"][0]["channel"], "mock_openai");
    assert_eq!(usage_body["data"][0]["model"], "apex-test-chat");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_blackbox_anthropic_messages_and_stream_work() {
    let upstream = MockProvider::spawn("mock-anthropic").await.unwrap();
    let listen = pick_listen_addr().unwrap();
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("generated.e2e.config.json");

    let env = E2eEnv::from_str(&format!(
        r#"
APEX_E2E_LISTEN={listen}
APEX_E2E_TEAM_ID=anthropic-team
APEX_E2E_TEAM_KEY=sk-anthropic-team
APEX_E2E_ADMIN_KEY=sk-anthropic-admin
APEX_E2E_ROUTER_NAME=anthropic-router
APEX_E2E_TEST_MODEL=apex-test-chat

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=mock_anthropic
APEX_UPSTREAM_1_TYPE=anthropic
APEX_UPSTREAM_1_BASE_URL={}
APEX_UPSTREAM_1_ANTHROPIC_BASE_URL={}
APEX_UPSTREAM_1_API_KEY=sk-upstream
APEX_UPSTREAM_1_MODEL=claude-test-model
APEX_UPSTREAM_1_HEADERS_JSON={{"anthropic-version":"2023-06-01"}}
"#,
        upstream.base_url(),
        upstream.base_url()
    ))
    .unwrap();

    write_config(&env, &config_path).unwrap();

    let mut gateway = GatewayProcess::spawn(&config_path, &listen).unwrap();
    gateway.wait_until_ready(Duration::from_secs(10)).unwrap();

    let client = Client::new();

    let message = client
        .post(format!("{}/v1/messages", gateway.base_url()))
        .header("x-api-key", "sk-anthropic-team")
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "apex-test-chat",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        message.status(),
        200,
        "gateway logs:\n{}",
        gateway.read_logs()
    );
    let message_body: serde_json::Value = message.json().await.unwrap();
    assert_eq!(message_body["type"], "message");
    assert_eq!(
        message_body["content"][0]["text"],
        "response from mock-anthropic"
    );

    let stream = client
        .post(format!("{}/v1/messages", gateway.base_url()))
        .header("x-api-key", "sk-anthropic-team")
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "apex-test-chat",
            "max_tokens": 64,
            "stream": true,
            "messages": [{"role": "user", "content": "stream please"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        stream.status(),
        200,
        "gateway logs:\n{}",
        gateway.read_logs()
    );
    let stream_body = stream.text().await.unwrap();
    assert!(stream_body.contains("event: message_start"));
    assert!(stream_body.contains("stream from mock-anthropic"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_blackbox_fallback_to_secondary_channel_works() {
    let bad_upstream =
        MockProvider::spawn_failing_chat("mock-bad", StatusCode::INTERNAL_SERVER_ERROR)
            .await
            .unwrap();
    let good_upstream = MockProvider::spawn("mock-good").await.unwrap();
    let listen = pick_listen_addr().unwrap();
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("generated.e2e.config.json");

    let env = E2eEnv::from_str(&format!(
        r#"
APEX_E2E_LISTEN={listen}
APEX_E2E_TEAM_ID=fallback-team
APEX_E2E_TEAM_KEY=sk-fallback-team
APEX_E2E_ROUTER_NAME=fallback-router
APEX_E2E_TEST_MODEL=apex-test-chat

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=bad_primary
APEX_UPSTREAM_1_TYPE=openai
APEX_UPSTREAM_1_BASE_URL={}
APEX_UPSTREAM_1_MODEL=bad-model

APEX_UPSTREAM_2_ENABLED=true
APEX_UPSTREAM_2_NAME=good_fallback
APEX_UPSTREAM_2_TYPE=openai
APEX_UPSTREAM_2_BASE_URL={}
APEX_UPSTREAM_2_MODEL=good-model
"#,
        bad_upstream.base_url(),
        good_upstream.base_url(),
    ))
    .unwrap();

    let mut config = harness::config_builder::build_config(&env, &config_path);
    std::sync::Arc::make_mut(&mut config.routers)[0].fallback_channels =
        vec!["good_fallback".to_string()];
    save_config(&config_path, &config).unwrap();

    let mut gateway = GatewayProcess::spawn(&config_path, &listen).unwrap();
    gateway.wait_until_ready(Duration::from_secs(10)).unwrap();

    let client = Client::new();
    let chat = client
        .post(format!("{}/v1/chat/completions", gateway.base_url()))
        .header("Authorization", "Bearer sk-fallback-team")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "apex-test-chat",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(chat.status(), 200, "gateway logs:\n{}", gateway.read_logs());
    let chat_body: serde_json::Value = chat.json().await.unwrap();
    assert_eq!(
        chat_body["choices"][0]["message"]["content"],
        "response from mock-good"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_blackbox_hot_reload_switches_channel() {
    let first_upstream = MockProvider::spawn("mock-before").await.unwrap();
    let second_upstream = MockProvider::spawn("mock-after").await.unwrap();
    let listen = pick_listen_addr().unwrap();
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("generated.e2e.config.json");

    let env = E2eEnv::from_str(&format!(
        r#"
APEX_E2E_LISTEN={listen}
APEX_E2E_TEAM_ID=reload-team
APEX_E2E_TEAM_KEY=sk-reload-team
APEX_E2E_ROUTER_NAME=reload-router
APEX_E2E_TEST_MODEL=apex-test-chat

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=reload_primary
APEX_UPSTREAM_1_TYPE=openai
APEX_UPSTREAM_1_BASE_URL={}
APEX_UPSTREAM_1_MODEL=reload-primary-model
"#,
        first_upstream.base_url(),
    ))
    .unwrap();

    let mut config = harness::config_builder::build_config(&env, &config_path);
    config.hot_reload.watch = true;
    save_config(&config_path, &config).unwrap();

    let mut gateway = GatewayProcess::spawn(&config_path, &listen).unwrap();
    gateway.wait_until_ready(Duration::from_secs(10)).unwrap();

    let client = Client::new();
    let first_response = client
        .post(format!("{}/v1/chat/completions", gateway.base_url()))
        .header("Authorization", "Bearer sk-reload-team")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "apex-test-chat",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        first_response.status(),
        200,
        "gateway logs:\n{}",
        gateway.read_logs()
    );
    let first_body: serde_json::Value = first_response.json().await.unwrap();
    assert_eq!(
        first_body["choices"][0]["message"]["content"],
        "response from mock-before"
    );

    std::sync::Arc::make_mut(&mut config.channels).push(Channel {
        name: "reload_secondary".to_string(),
        provider_type: ProviderType::Openai,
        base_url: second_upstream.base_url(),
        api_key: String::new(),
        anthropic_base_url: None,
        headers: None,
        model_map: Some(std::collections::HashMap::from([(
            "apex-test-chat".to_string(),
            "reload-secondary-model".to_string(),
        )])),
        timeouts: None,
    });
    std::sync::Arc::make_mut(&mut config.routers)[0].rules[0].channels = vec![TargetChannel {
        name: "reload_secondary".to_string(),
        weight: 1,
    }];
    save_config(&config_path, &config).unwrap();

    let mut reloaded_body = None;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let response = client
            .post(format!("{}/v1/chat/completions", gateway.base_url()))
            .header("Authorization", "Bearer sk-reload-team")
            .header("Content-Type", "application/json")
            .json(&json!({
                "model": "apex-test-chat",
                "messages": [{"role": "user", "content": "hello again"}]
            }))
            .send()
            .await
            .unwrap();

        if response.status() != 200 {
            continue;
        }

        let body: serde_json::Value = response.json().await.unwrap();
        if body["choices"][0]["message"]["content"] == "response from mock-after" {
            reloaded_body = Some(body);
            break;
        }
    }

    let reloaded_body = reloaded_body.unwrap_or_else(|| {
        panic!(
            "hot reload did not switch channel in time. gateway logs:\n{}",
            gateway.read_logs()
        )
    });
    assert_eq!(
        reloaded_body["choices"][0]["message"]["content"],
        "response from mock-after"
    );
}
