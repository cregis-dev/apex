mod harness;

use harness::config_builder::{build_config, write_config};
use harness::env::E2eEnv;
use tempfile::tempdir;

#[test]
fn parses_env_and_builds_config() {
    let env: E2eEnv = r#"
APEX_E2E_LISTEN=127.0.0.1:22345
APEX_E2E_TEAM_ID=smoke-team
APEX_E2E_TEAM_KEY=sk-apex-e2e-team
APEX_E2E_ADMIN_KEY=sk-apex-admin
APEX_E2E_ROUTER_NAME=smoke-router
APEX_E2E_ROUTER_STRATEGY=priority
APEX_E2E_TEST_MODEL=apex-test-chat

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=openai_primary
APEX_UPSTREAM_1_TYPE=openai
APEX_UPSTREAM_1_BASE_URL=https://api.openai.com/v1
APEX_UPSTREAM_1_API_KEY=sk-openai
APEX_UPSTREAM_1_MODEL=openai-test-model
APEX_UPSTREAM_1_WEIGHT=2

APEX_UPSTREAM_2_ENABLED=true
APEX_UPSTREAM_2_NAME=anthropic_fallback
APEX_UPSTREAM_2_TYPE=anthropic
APEX_UPSTREAM_2_BASE_URL=https://api.anthropic.com
APEX_UPSTREAM_2_API_KEY=sk-anthropic
APEX_UPSTREAM_2_ANTHROPIC_BASE_URL=https://api.anthropic.com
APEX_UPSTREAM_2_MODEL_MAP_JSON={"apex-test-chat":"claude-sonnet-test"}
APEX_UPSTREAM_2_HEADERS_JSON={"anthropic-version":"2023-06-01"}
"#
    .parse()
    .unwrap();

    assert_eq!(env.listen, "127.0.0.1:22345");
    assert_eq!(env.upstreams.len(), 2);
    assert_eq!(env.upstreams[0].weight, 2);
    assert_eq!(
        env.upstreams[0]
            .model_map
            .as_ref()
            .unwrap()
            .get("apex-test-chat")
            .unwrap(),
        "openai-test-model"
    );

    let dir = tempdir().unwrap();
    let config_path = dir.path().join("generated.e2e.config.json");
    let config = build_config(&env, &config_path);

    assert_eq!(config.global.listen, "127.0.0.1:22345");
    assert_eq!(config.global.auth_keys, vec!["sk-apex-admin".to_string()]);
    assert_eq!(config.channels.len(), 2);
    assert_eq!(config.routers.len(), 1);
    assert_eq!(config.routers[0].rules[0].strategy, "priority");
    assert_eq!(
        config.routers[0].fallback_channels,
        vec!["anthropic_fallback".to_string()]
    );
    assert_eq!(config.teams[0].id, "smoke-team");
    assert_eq!(config.teams[0].policy.allowed_routers, vec!["smoke-router"]);
    assert_eq!(
        config.teams[0].policy.allowed_models,
        Some(vec!["apex-test-chat".to_string()])
    );
    assert_eq!(
        config.channels[1]
            .headers
            .as_ref()
            .unwrap()
            .get("anthropic-version")
            .unwrap(),
        "2023-06-01"
    );
}

#[test]
fn writes_generated_config_json() {
    let dir = tempdir().unwrap();
    let env_path = dir.path().join(".env.e2e");
    std::fs::write(
        &env_path,
        r#"
APEX_E2E_TEAM_KEY=sk-apex-e2e-team

APEX_UPSTREAM_1_ENABLED=true
APEX_UPSTREAM_1_NAME=ollama_local
APEX_UPSTREAM_1_TYPE=ollama
APEX_UPSTREAM_1_BASE_URL=http://127.0.0.1:11434/v1
APEX_UPSTREAM_1_MODEL=qwen2.5:latest
"#,
    )
    .unwrap();

    let env = E2eEnv::from_env_file(&env_path).unwrap();
    let config_path = dir.path().join("generated.e2e.config.json");
    write_config(&env, &config_path).unwrap();

    let content = std::fs::read_to_string(&config_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(value["teams"][0]["api_key"], "sk-apex-e2e-team");
    assert_eq!(value["channels"][0]["provider_type"], "ollama");
    assert_eq!(
        value["channels"][0]["model_map"]["apex-test-chat"],
        "qwen2.5:latest"
    );
    assert_eq!(
        value["routers"][0]["rules"][0]["channels"][0]["name"],
        "ollama_local"
    );
}
