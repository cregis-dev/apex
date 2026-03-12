use apex::config::{
    Channel, Config, Global, HotReload, Logging, Metrics, ProviderType, Retries, Team, TeamPolicy,
    Timeouts,
};
use apex::database::Database;
use apex::mcp::protocol::{
    CallToolResult, Id, JsonRpcMessage, ListToolsResult, Request, ToolContent,
};
use apex::mcp::server::McpServer;
use serde_json::json;
use std::sync::{Arc, RwLock};
use tempfile::tempdir;

#[tokio::test]
async fn test_mcp_tools() {
    // Setup config
    let config = Config {
        version: "1.0".to_string(),
        global: Global {
            listen: "127.0.0.1:8080".to_string(),
            auth_keys: vec![],
            timeouts: Timeouts {
                connect_ms: 1000,
                request_ms: 1000,
                response_ms: 1000,
            },
            retries: Retries {
                max_attempts: 1,
                backoff_ms: 100,
                retry_on_status: vec![],
            },
            enable_mcp: true,
            cors_allowed_origins: vec![],
        },
        logging: Logging::default(),
        data_dir: "/tmp".to_string(),
        web_dir: "target/web".to_string(),
        channels: Arc::new(vec![Channel {
            name: "test-channel".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://api.openai.com".to_string(),
            api_key: "sk-test".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: Some(std::collections::HashMap::from([(
                "gpt-4".to_string(),
                "gpt-4-0613".to_string(),
            )])),
            timeouts: None,
        }]),
        routers: Arc::new(vec![]),
        metrics: Metrics {
            enabled: false,
            path: "".to_string(),
        },
        hot_reload: HotReload {
            config_path: "config.json".to_string(),
            watch: false,
        },
        teams: Arc::new(vec![Team {
            id: "team-a".to_string(),
            api_key: "sk-team-a".to_string(),
            policy: TeamPolicy {
                allowed_routers: vec!["router-a".to_string()],
                allowed_models: None,
                rate_limit: None,
            },
        }]),
        prompts: Arc::new(vec![]),
        compliance: None,
    };

    let dir = tempdir().unwrap();
    let database = Arc::new(Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap());
    database.log_usage(
        Some("req-1"),
        "team-a",
        "router-a",
        Some("gpt-*"),
        "test-channel",
        "gpt-4",
        10,
        20,
        Some(35.0),
        false,
        "success",
        Some(200),
        None,
        None,
        None,
    );
    let server = McpServer::new(Arc::new(RwLock::new(config)), database);

    // Test tools/list
    let req = Request::new(Id::Number(1), "tools/list".to_string(), None);
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: ListToolsResult = serde_json::from_value(r.result).unwrap();
        assert!(result.tools.iter().any(|t| t.name == "echo"));
        assert!(result.tools.iter().any(|t| t.name == "list_models"));
        assert!(result.tools.iter().any(|t| t.name == "query_usage_summary"));
        assert!(result.tools.iter().any(|t| t.name == "query_usage_records"));
        assert!(result.tools.iter().any(|t| t.name == "export_usage_report"));
        assert!(!result.tools.iter().any(|t| t.name == "query_team_usage"));
        assert!(
            !result
                .tools
                .iter()
                .any(|t| t.name == "query_all_teams_usage")
        );
    } else {
        panic!("Expected Response for tools/list");
    }

    // Test tools/call echo
    let req = Request::new(
        Id::Number(2),
        "tools/call".to_string(),
        Some(json!({
            "name": "echo",
            "arguments": {
                "message": "Hello World"
            }
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: CallToolResult = serde_json::from_value(r.result).unwrap();
        assert_eq!(result.is_error, Some(false));
        if let ToolContent::Text { text } = &result.content[0] {
            assert_eq!(text, "Hello World");
        } else {
            panic!("Expected Text content");
        }
    } else {
        panic!("Expected Response for tools/call echo");
    }

    // Test tools/call list_models
    let req = Request::new(
        Id::Number(3),
        "tools/call".to_string(),
        Some(json!({
            "name": "list_models",
            "arguments": {}
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: CallToolResult = serde_json::from_value(r.result).unwrap();
        assert_eq!(result.is_error, Some(false));
        if let ToolContent::Text { text } = &result.content[0] {
            let payload: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(payload["data_source"], "config");
            assert_eq!(payload["channels"][0]["name"], "test-channel");
            assert_eq!(payload["channels"][0]["model_map"]["gpt-4"], "gpt-4-0613");
        } else {
            panic!("Expected Text content");
        }
    } else {
        panic!("Expected Response for tools/call list_models");
    }

    // Test tools/call query_usage_summary
    let req = Request::new(
        Id::Number(31),
        "tools/call".to_string(),
        Some(json!({
            "name": "query_usage_summary",
            "arguments": {
                "team_id": "team-a"
            }
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: CallToolResult = serde_json::from_value(r.result).unwrap();
        if let ToolContent::Text { text } = &result.content[0] {
            let payload: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(payload["data_source"], "sqlite");
            assert_eq!(payload["summary"]["total_requests"], 1);
        } else {
            panic!("Expected Text content");
        }
    } else {
        panic!("Expected Response for tools/call query_usage_summary");
    }

    // Test tools/call query_usage_records
    let req = Request::new(
        Id::Number(32),
        "tools/call".to_string(),
        Some(json!({
            "name": "query_usage_records",
            "arguments": {
                "team_id": "team-a",
                "limit": 10,
                "offset": 0
            }
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: CallToolResult = serde_json::from_value(r.result).unwrap();
        if let ToolContent::Text { text } = &result.content[0] {
            let payload: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(payload["data_source"], "sqlite");
            assert_eq!(payload["pagination"]["total"], 1);
            assert_eq!(payload["records"][0]["team_id"], "team-a");
        } else {
            panic!("Expected Text content");
        }
    } else {
        panic!("Expected Response for tools/call query_usage_records");
    }

    // Old tool names should no longer exist
    let req = Request::new(
        Id::Number(33),
        "tools/call".to_string(),
        Some(json!({
            "name": "query_team_usage",
            "arguments": {
                "team_id": "team-a"
            }
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Error(e) = resp {
        assert_eq!(e.error.code, -32601);
        assert_eq!(
            e.error.data,
            Some(json!("Tool 'query_team_usage' not found"))
        );
    } else {
        panic!("Expected Error for removed query_team_usage");
    }

    let req = Request::new(
        Id::Number(34),
        "tools/call".to_string(),
        Some(json!({
            "name": "query_all_teams_usage",
            "arguments": {
                "group_by": "team"
            }
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Error(e) = resp {
        assert_eq!(e.error.code, -32601);
        assert_eq!(
            e.error.data,
            Some(json!("Tool 'query_all_teams_usage' not found"))
        );
    } else {
        panic!("Expected Error for removed query_all_teams_usage");
    }

    // Test tools/call unknown
    let req = Request::new(
        Id::Number(4),
        "tools/call".to_string(),
        Some(json!({
            "name": "unknown_tool",
            "arguments": {}
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Error(e) = resp {
        assert_eq!(e.error.code, -32601); // Method not found
        assert_eq!(e.error.message, "Method not found");
        assert_eq!(e.error.data, Some(json!("Tool 'unknown_tool' not found")));
    } else {
        panic!("Expected Error for unknown tool");
    }
}
