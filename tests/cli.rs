use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

// Helper to run apex with a custom config path
fn apex_cmd(config_path: &str) -> Command {
    let mut cmd = Command::cargo_bin("apex").unwrap();
    cmd.arg("--config").arg(config_path);
    cmd
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

    let channels = r["channels"].as_array().unwrap();
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
    assert_eq!(routers[0]["channels"][0]["name"], "c1");

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
