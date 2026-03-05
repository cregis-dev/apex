use apex::config::{
    Channel, Config, Global, HotReload, Logging, Metrics, ProviderType, Retries,
    Team, Timeouts,
};
use apex::mcp::protocol::{
    CallToolResult, Id, JsonRpcMessage, ListToolsResult, Request, ToolContent,
};
use apex::mcp::server::McpServer;
use serde_json::json;
use std::sync::{Arc, RwLock};

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
        },
        logging: Logging::default(),
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
        teams: Arc::new(vec![]),
        prompts: Arc::new(vec![]),
        compliance: None,
    };

    let server = McpServer::new(Arc::new(RwLock::new(config)));

    // Test tools/list
    let req = Request::new(Id::Number(1), "tools/list".to_string(), None);
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: ListToolsResult = serde_json::from_value(r.result).unwrap();
        assert!(result.tools.iter().any(|t| t.name == "echo"));
        assert!(result.tools.iter().any(|t| t.name == "list_models"));
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
            assert!(text.contains("Channel: test-channel"));
            assert!(text.contains("gpt-4 -> gpt-4-0613"));
        } else {
            panic!("Expected Text content");
        }
    } else {
        panic!("Expected Response for tools/call list_models");
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
