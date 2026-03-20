mod harness;

use harness::config_builder::write_config;
use harness::env::E2eEnv;
use harness::gateway_process::{GatewayProcess, pick_listen_addr};
use harness::mock_provider::MockProvider;
use reqwest::Client;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::tempdir;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("regression")
        .join(name)
}

fn load_fixture(name: &str) -> String {
    fs::read_to_string(fixture_path(name)).unwrap()
}

fn assert_json_fixture(name: &str, actual: &str) {
    let expected: Value = serde_json::from_str(&load_fixture(name)).unwrap();
    let actual: Value = serde_json::from_str(actual).unwrap();
    assert_eq!(actual, expected, "fixture mismatch for {name}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn regression_openai_chat_response_matches_baseline() {
    let upstream = MockProvider::spawn("mock-openai").await.unwrap();
    let listen = pick_listen_addr().unwrap();
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("generated.e2e.config.json");

    let env: E2eEnv = format!(
        r#"
APEX_E2E_LISTEN={listen}
APEX_E2E_TEAM_ID=baseline-team
APEX_E2E_TEAM_KEY=sk-baseline-team
APEX_E2E_ADMIN_KEY=sk-baseline-admin
APEX_E2E_ROUTER_NAME=baseline-router
APEX_E2E_TEST_MODEL=apex-test-chat

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=mock_openai
APEX_UPSTREAM_1_TYPE=openai
APEX_UPSTREAM_1_BASE_URL={}
APEX_UPSTREAM_1_MODEL=mock-openai-model
"#,
        upstream.base_url()
    )
    .parse()
    .unwrap();

    write_config(&env, &config_path).unwrap();

    let mut gateway = GatewayProcess::spawn(&config_path, &listen).unwrap();
    gateway.wait_until_ready(Duration::from_secs(10)).unwrap();

    let body = Client::new()
        .post(format!("{}/v1/chat/completions", gateway.base_url()))
        .header("Authorization", "Bearer sk-baseline-team")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "apex-test-chat",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert_json_fixture("openai_chat_success.json", &body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn regression_openai_error_response_matches_baseline() {
    let upstream = MockProvider::spawn_failing_chat(
        "mock-openai",
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    )
    .await
    .unwrap();
    let listen = pick_listen_addr().unwrap();
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("generated.e2e.config.json");

    let env: E2eEnv = format!(
        r#"
APEX_E2E_LISTEN={listen}
APEX_E2E_TEAM_ID=baseline-team
APEX_E2E_TEAM_KEY=sk-baseline-team
APEX_E2E_ROUTER_NAME=baseline-router
APEX_E2E_TEST_MODEL=apex-test-chat

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=mock_openai
APEX_UPSTREAM_1_TYPE=openai
APEX_UPSTREAM_1_BASE_URL={}
APEX_UPSTREAM_1_MODEL=mock-openai-model
"#,
        upstream.base_url()
    )
    .parse()
    .unwrap();

    let mut config = harness::config_builder::build_config(&env, &config_path);
    config.global.retries.max_attempts = 1;
    apex::config::save_config(&config_path, &config).unwrap();

    let mut gateway = GatewayProcess::spawn(&config_path, &listen).unwrap();
    gateway.wait_until_ready(Duration::from_secs(10)).unwrap();

    let response = Client::new()
        .post(format!("{}/v1/chat/completions", gateway.base_url()))
        .header("Authorization", "Bearer sk-baseline-team")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "apex-test-chat",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 500);
    let body = response.text().await.unwrap();
    assert_json_fixture("openai_error_upstream_500.json", &body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn regression_anthropic_stream_matches_baseline() {
    let upstream = MockProvider::spawn("mock-anthropic").await.unwrap();
    let listen = pick_listen_addr().unwrap();
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("generated.e2e.config.json");

    let env: E2eEnv = format!(
        r#"
APEX_E2E_LISTEN={listen}
APEX_E2E_TEAM_ID=baseline-team
APEX_E2E_TEAM_KEY=sk-baseline-team
APEX_E2E_ROUTER_NAME=baseline-router
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
    )
    .parse()
    .unwrap();

    write_config(&env, &config_path).unwrap();

    let mut gateway = GatewayProcess::spawn(&config_path, &listen).unwrap();
    gateway.wait_until_ready(Duration::from_secs(10)).unwrap();

    let body = Client::new()
        .post(format!("{}/v1/messages", gateway.base_url()))
        .header("x-api-key", "sk-baseline-team")
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
        .unwrap()
        .text()
        .await
        .unwrap();

    assert_eq!(body, load_fixture("anthropic_messages_stream.sse"));
}
