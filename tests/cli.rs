use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::process::Output;
use tempfile::TempDir;

// Helper to run apex with a custom config path
fn apex_cmd(config_path: &str) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_apex"));
    cmd.arg("--config").arg(config_path);
    cmd
}

fn stdout_json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON")
}

fn raw_apex_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_apex"))
}

#[test]
fn test_router_multichannel() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    // 1. Init
    apex_cmd(config_str).arg("init").assert().success();

    // 2. Add Channels (Need to add channels first)
    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("c1")
        .arg("--provider")
        .arg("openai")
        .arg("--base-url")
        .arg("u1")
        .arg("--api-key")
        .arg("k1")
        .assert()
        .success();

    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("c2")
        .arg("--provider")
        .arg("openai")
        .arg("--base-url")
        .arg("u2")
        .arg("--api-key")
        .arg("k2")
        .assert()
        .success();

    // 3. Add Router with multiple channels
    apex_cmd(config_str)
        .arg("router")
        .arg("add")
        .arg("--name")
        .arg("r_multi")
        .arg("--channels")
        .arg("c1:10,c2:5")
        .arg("--strategy")
        .arg("random")
        .assert()
        .success()
        .stdout(predicate::str::contains("已添加 router: r_multi"));

    // 4. Verify config
    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let routers = json["routers"].as_array().unwrap();
    assert_eq!(routers.len(), 1);

    let r = &routers[0];
    assert_eq!(r["name"], "r_multi");
    assert_eq!(r["strategy"], "random");

    // New format: channels are in rules[0].channels
    let channels = r["rules"][0]["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 2);

    assert_eq!(channels[0]["name"], "c1");
    assert_eq!(channels[0]["weight"], 10);

    assert_eq!(channels[1]["name"], "c2");
    assert_eq!(channels[1]["weight"], 5);
}

#[test]
fn test_init_creates_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    // Run init
    apex_cmd(config_str)
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("已写入"));

    // Verify file exists
    assert!(config_path.exists());

    // Verify content is valid JSON
    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(json["version"], "1");
    assert_eq!(json["logging"]["level"], "info");
}

#[test]
fn test_channel_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    // 1. Init
    apex_cmd(config_str).arg("init").assert().success();

    // 2. Add Channel
    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("test-openai")
        .arg("--provider")
        .arg("openai")
        .arg("--base-url")
        .arg("https://api.openai.com/v1")
        .arg("--api-key")
        .arg("sk-test")
        .assert()
        .success()
        .stdout(predicate::str::contains("已添加 channel: test-openai"));

    // Verify in config
    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let channels = json["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["name"], "test-openai");
    assert_eq!(channels[0]["provider_type"], "openai");

    // 3. Update Channel
    apex_cmd(config_str)
        .arg("channel")
        .arg("update")
        .arg("--name")
        .arg("test-openai")
        .arg("--api-key")
        .arg("sk-updated")
        .assert()
        .success()
        .stdout(predicate::str::contains("已更新 channel: test-openai"));

    // Verify update
    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(json["channels"][0]["api_key"], "sk-updated");

    // 4. Delete Channel
    apex_cmd(config_str)
        .arg("channel")
        .arg("delete")
        .arg("test-openai")
        .assert()
        .success()
        .stdout(predicate::str::contains("已删除 channel: test-openai"));

    // Verify deletion
    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(json["channels"].as_array().unwrap().len(), 0);
}

#[test]
fn test_channel_add_defaults_base_url_without_prompt() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();

    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("defaulted-openai")
        .arg("--provider")
        .arg("openai")
        .arg("--api-key")
        .arg("sk-test")
        .assert()
        .success()
        .stdout(predicate::str::contains("已添加 channel: defaulted-openai"));

    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let channel = &json["channels"][0];
    assert_eq!(channel["name"], "defaulted-openai");
    assert_eq!(channel["base_url"], "https://api.openai.com/v1");
}

#[test]
fn test_dual_protocol_channel_add_uses_default_anthropic_url_without_prompt() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();

    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("minimax-defaults")
        .arg("--provider")
        .arg("minimax")
        .arg("--api-key")
        .arg("sk-mm")
        .assert()
        .success()
        .stdout(predicate::str::contains("已添加 channel: minimax-defaults"));

    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let channel = &json["channels"][0];
    assert_eq!(channel["name"], "minimax-defaults");
    assert_eq!(channel["base_url"], "https://api.minimax.io/v1");
    assert_eq!(
        channel["anthropic_base_url"],
        "https://api.minimax.io/anthropic"
    );
}

#[test]
fn test_custom_dual_channel_add() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();

    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("dual-agg")
        .arg("--provider")
        .arg("custom_dual")
        .arg("--base-url")
        .arg("https://api.example.com/v1")
        .arg("--anthropic-base-url")
        .arg("https://api.example.com/anthropic")
        .arg("--api-key")
        .arg("sk-dual")
        .assert()
        .success()
        .stdout(predicate::str::contains("已添加 channel: dual-agg"));

    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let channels = json["channels"].as_array().unwrap();

    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["name"], "dual-agg");
    assert_eq!(channels[0]["provider_type"], "custom_dual");
    assert_eq!(channels[0]["base_url"], "https://api.example.com/v1");
    assert_eq!(
        channels[0]["anthropic_base_url"],
        "https://api.example.com/anthropic"
    );
}

#[test]
fn test_zai_channel_add_persists_dual_protocol_urls() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();

    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("zai-main")
        .arg("--provider")
        .arg("zai")
        .arg("--base-url")
        .arg("https://api.z.ai/api/coding/paas/v4")
        .arg("--anthropic-base-url")
        .arg("https://api.z.ai/api/anthropic")
        .arg("--api-key")
        .arg("sk-zai")
        .assert()
        .success()
        .stdout(predicate::str::contains("已添加 channel: zai-main"));

    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let channels = json["channels"].as_array().unwrap();

    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["name"], "zai-main");
    assert_eq!(channels[0]["provider_type"], "zai");
    assert_eq!(
        channels[0]["base_url"],
        "https://api.z.ai/api/coding/paas/v4"
    );
    assert_eq!(
        channels[0]["anthropic_base_url"],
        "https://api.z.ai/api/anthropic"
    );
}

#[test]
fn test_router_lifecycle() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    // 1. Init & Add Channel (Router needs a channel)
    apex_cmd(config_str).arg("init").assert().success();
    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("c1")
        .arg("--provider")
        .arg("openai")
        .arg("--base-url")
        .arg("https://api.openai.com/v1")
        .arg("--api-key")
        .arg("sk-test")
        .assert()
        .success();

    // 2. Add Router
    apex_cmd(config_str)
        .arg("router")
        .arg("add")
        .arg("--name")
        .arg("r1")
        .arg("--channels")
        .arg("c1")
        .assert()
        .success()
        .stdout(predicate::str::contains("已添加 router: r1"));

    // Verify config
    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let routers = json["routers"].as_array().unwrap();
    assert_eq!(routers.len(), 1);
    assert_eq!(routers[0]["name"], "r1");
    // New format: channels are in rules[0].channels
    assert_eq!(routers[0]["rules"][0]["channels"][0]["name"], "c1");

    // 3. Delete Router
    apex_cmd(config_str)
        .arg("router")
        .arg("delete")
        .arg("r1")
        .assert()
        .success()
        .stdout(predicate::str::contains("已删除 router: r1"));

    // Verify deletion
    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(json["routers"].as_array().unwrap().len(), 0);
}

#[test]
fn test_router_update_with_explicit_args_is_noninteractive() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();

    for (name, key) in [("c1", "k1"), ("c2", "k2")] {
        apex_cmd(config_str)
            .arg("channel")
            .arg("add")
            .arg("--name")
            .arg(name)
            .arg("--provider")
            .arg("openai")
            .arg("--api-key")
            .arg(key)
            .assert()
            .success();
    }

    apex_cmd(config_str)
        .arg("router")
        .arg("add")
        .arg("--name")
        .arg("r1")
        .arg("--channels")
        .arg("c1")
        .assert()
        .success();

    apex_cmd(config_str)
        .arg("router")
        .arg("update")
        .arg("--name")
        .arg("r1")
        .arg("--channels")
        .arg("c2")
        .assert()
        .success()
        .stdout(predicate::str::contains("已更新 router: r1"));

    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let routers = json["routers"].as_array().unwrap();
    assert_eq!(routers.len(), 1);
    assert_eq!(routers[0]["channels"][0]["name"], "c2");
}

#[test]
fn test_team_lifecycle_is_noninteractive() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();
    apex_cmd(config_str)
        .arg("team")
        .arg("add")
        .arg("--id")
        .arg("demo-team")
        .arg("--routers")
        .arg("default-router")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Team 'demo-team' added successfully.",
        ));

    apex_cmd(config_str)
        .arg("team")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("demo-team"));

    apex_cmd(config_str)
        .arg("team")
        .arg("remove")
        .arg("demo-team")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Team 'demo-team' removed successfully.",
        ));

    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(json["teams"].as_array().unwrap().len(), 0);
}

#[test]
fn test_team_subcommand_accepts_global_config_after_subcommand() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();
    apex_cmd(config_str)
        .arg("team")
        .arg("add")
        .arg("--id")
        .arg("custom-config-team")
        .arg("--routers")
        .arg("default-router")
        .assert()
        .success();

    raw_apex_cmd()
        .arg("team")
        .arg("list")
        .arg("--config")
        .arg(config_str)
        .assert()
        .success()
        .stdout(predicate::str::contains("custom-config-team"));
}

#[test]
fn test_team_help_shows_global_config_option() {
    raw_apex_cmd()
        .arg("team")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--config"));
}

#[test]
fn test_channel_list_json_contract() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();
    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("json-openai")
        .arg("--provider")
        .arg("openai")
        .arg("--api-key")
        .arg("sk-json")
        .assert()
        .success();

    let output = apex_cmd(config_str)
        .arg("channel")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();
    assert!(output.status.success());

    let body = stdout_json(&output);
    assert_eq!(body["ok"], true);
    assert_eq!(body["command"], "channel.list");
    assert_eq!(body["meta"]["resource"], "channel");
    assert_eq!(body["meta"]["action"], "list");
    assert!(body["errors"].as_array().unwrap().is_empty());
    assert_eq!(body["data"][0]["name"], "json-openai");
    assert_eq!(body["data"][0]["has_api_key"], true);
    assert!(body["data"][0].get("api_key").is_none());
}

#[test]
fn test_channel_add_json_error_contract() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();
    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("json-openai")
        .arg("--provider")
        .arg("openai")
        .arg("--api-key")
        .arg("sk-json")
        .assert()
        .success();

    let output = apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("json-openai")
        .arg("--provider")
        .arg("openai")
        .arg("--api-key")
        .arg("sk-json-2")
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());

    let body = stdout_json(&output);
    assert_eq!(body["ok"], false);
    assert_eq!(body["command"], "channel.add");
    assert_eq!(body["meta"]["resource"], "channel");
    assert_eq!(body["meta"]["action"], "add");
    assert!(body["data"].is_null());
    assert_eq!(body["errors"][0]["code"], "already_exists");
}

#[test]
fn test_channel_add_json_validation_error_contract() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();

    let output = apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("bad-provider")
        .arg("--provider")
        .arg("not-real")
        .arg("--api-key")
        .arg("sk-json")
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());

    let body = stdout_json(&output);
    assert_eq!(body["ok"], false);
    assert_eq!(body["command"], "channel.add");
    assert_eq!(body["meta"]["resource"], "channel");
    assert_eq!(body["meta"]["action"], "add");
    assert!(body["data"].is_null());
    assert_eq!(body["errors"][0]["code"], "invalid_input");
}

#[test]
fn test_router_add_json_contract() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();
    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("json-channel")
        .arg("--provider")
        .arg("openai")
        .arg("--api-key")
        .arg("sk-json")
        .assert()
        .success();

    let output = apex_cmd(config_str)
        .arg("router")
        .arg("add")
        .arg("--name")
        .arg("json-router")
        .arg("--channels")
        .arg("json-channel")
        .arg("--json")
        .output()
        .unwrap();
    assert!(output.status.success());

    let body = stdout_json(&output);
    assert_eq!(body["ok"], true);
    assert_eq!(body["command"], "router.add");
    assert_eq!(body["meta"]["resource"], "router");
    assert_eq!(body["meta"]["action"], "add");
    assert_eq!(body["data"]["name"], "json-router");
    assert!(body["errors"].as_array().unwrap().is_empty());
}

#[test]
fn test_router_delete_json_error_contract() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();

    let output = apex_cmd(config_str)
        .arg("router")
        .arg("delete")
        .arg("missing-router")
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());

    let body = stdout_json(&output);
    assert_eq!(body["ok"], false);
    assert_eq!(body["command"], "router.delete");
    assert_eq!(body["meta"]["resource"], "router");
    assert_eq!(body["meta"]["action"], "delete");
    assert!(body["data"].is_null());
    assert_eq!(body["errors"][0]["code"], "not_found");
}

#[test]
fn test_router_add_json_validation_error_contract() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();
    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("json-channel")
        .arg("--provider")
        .arg("openai")
        .arg("--api-key")
        .arg("sk-json")
        .assert()
        .success();

    let output = apex_cmd(config_str)
        .arg("router")
        .arg("add")
        .arg("--name")
        .arg("bad-router")
        .arg("--channels")
        .arg("json-channel")
        .arg("--match")
        .arg("missing-equals")
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());

    let body = stdout_json(&output);
    assert_eq!(body["ok"], false);
    assert_eq!(body["command"], "router.add");
    assert_eq!(body["meta"]["resource"], "router");
    assert_eq!(body["meta"]["action"], "add");
    assert!(body["data"].is_null());
    assert_eq!(body["errors"][0]["code"], "invalid_input");
}

#[test]
fn test_team_add_json_contract_includes_generated_key() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();

    let output = apex_cmd(config_str)
        .arg("team")
        .arg("add")
        .arg("--id")
        .arg("json-team")
        .arg("--routers")
        .arg("default-router")
        .arg("--json")
        .output()
        .unwrap();
    assert!(output.status.success());

    let body = stdout_json(&output);
    assert_eq!(body["ok"], true);
    assert_eq!(body["command"], "team.add");
    assert_eq!(body["meta"]["resource"], "team");
    assert_eq!(body["meta"]["action"], "add");
    assert_eq!(body["data"]["id"], "json-team");
    assert!(
        body["data"]["api_key"]
            .as_str()
            .unwrap()
            .starts_with("sk-ap-")
    );
}

#[test]
fn test_team_remove_json_error_contract() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    apex_cmd(config_str).arg("init").assert().success();

    let output = apex_cmd(config_str)
        .arg("team")
        .arg("remove")
        .arg("missing-team")
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());

    let body = stdout_json(&output);
    assert_eq!(body["ok"], false);
    assert_eq!(body["command"], "team.remove");
    assert_eq!(body["meta"]["resource"], "team");
    assert_eq!(body["meta"]["action"], "remove");
    assert!(body["data"].is_null());
    assert_eq!(body["errors"][0]["code"], "not_found");
}

#[test]
fn test_json_missing_config_error_contract() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("missing.json");
    let config_str = config_path.to_str().unwrap();

    let output = apex_cmd(config_str)
        .arg("channel")
        .arg("list")
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());

    let body = stdout_json(&output);
    assert_eq!(body["ok"], false);
    assert_eq!(body["command"], "channel.list");
    assert_eq!(body["meta"]["resource"], "channel");
    assert_eq!(body["meta"]["action"], "list");
    assert!(body["data"].is_null());
    assert_eq!(body["errors"][0]["code"], "not_found");
}

#[test]
fn test_minimax_anthropic_default() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("apex.json");
    let config_str = config_path.to_str().unwrap();

    // 1. Init
    apex_cmd(config_str).arg("init").assert().success();

    // 2. Add MiniMax Channel
    // Even if we provide base_url (to avoid prompt), it should default protocol to anthropic
    apex_cmd(config_str)
        .arg("channel")
        .arg("add")
        .arg("--name")
        .arg("mm")
        .arg("--provider")
        .arg("minimax")
        .arg("--base-url")
        .arg("https://api.minimax.io/anthropic")
        .arg("--anthropic-base-url")
        .arg("https://api.minimax.io/anthropic")
        .arg("--api-key")
        .arg("sk-mm")
        .assert()
        .success();

    // Verify config
    let content = fs::read_to_string(&config_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let channels = json["channels"].as_array().unwrap();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0]["name"], "mm");
    assert_eq!(channels[0]["provider_type"], "minimax");
    // Protocol should be null as it's dual-protocol now
    assert!(channels[0]["protocol"].is_null());
    assert_eq!(channels[0]["base_url"], "https://api.minimax.io/anthropic");
}
