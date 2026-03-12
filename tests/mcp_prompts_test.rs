use apex::config::{
    Config, Global, HotReload, Logging, Metrics, Prompt, PromptArgument, PromptContent,
    PromptMessage, Retries, Timeouts,
};
use apex::database::Database;
use apex::mcp::protocol::{
    GetPromptResult, Id, JsonRpcMessage, ListPromptsResult, PromptMessageContent, Request,
};
use apex::mcp::server::McpServer;
use std::sync::{Arc, RwLock};

#[tokio::test]
async fn test_mcp_prompts() {
    // Setup config with prompts
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
        channels: Arc::new(vec![]),
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
        prompts: Arc::new(vec![Prompt {
            name: "test-prompt".to_string(),
            description: Some("A test prompt".to_string()),
            arguments: vec![PromptArgument {
                name: "arg1".to_string(),
                description: Some("First argument".to_string()),
                required: true,
            }],
            messages: vec![PromptMessage {
                role: "user".to_string(),
                content: PromptContent::Text {
                    text: "Hello {{ arg1 }}".to_string(),
                },
            }],
        }]),
        compliance: None,
    };

    let database = Arc::new(Database::new(Some("/tmp".to_string())).unwrap());
    let server = McpServer::new(Arc::new(RwLock::new(config)), database);

    // Test prompts/list
    let req = Request::new(Id::Number(1), "prompts/list".to_string(), None);
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: ListPromptsResult = serde_json::from_value(r.result).unwrap();
        assert_eq!(result.prompts.len(), 1);
        assert_eq!(result.prompts[0].name, "test-prompt");
        assert_eq!(result.prompts[0].arguments.as_ref().unwrap().len(), 1);
    } else {
        panic!("Expected Response for prompts/list");
    }

    // Test prompts/get with argument
    let req = Request::new(
        Id::Number(2),
        "prompts/get".to_string(),
        Some(serde_json::json!({
            "name": "test-prompt",
            "arguments": {
                "arg1": "world"
            }
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: GetPromptResult = serde_json::from_value(r.result).unwrap();
        assert_eq!(result.messages.len(), 1);
        match &result.messages[0].content {
            PromptMessageContent::Text { text } => {
                assert_eq!(text, "Hello world");
            }
            _ => panic!("Expected text content"),
        }
    } else {
        panic!("Expected Response for prompts/get");
    }

    // Test prompts/get missing prompt
    let req = Request::new(
        Id::Number(3),
        "prompts/get".to_string(),
        Some(serde_json::json!({
            "name": "non-existent"
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Error(e) = resp {
        assert_eq!(e.error.code, -32602); // Invalid params
        assert!(e.error.message.contains("Invalid params"));
    } else {
        panic!("Expected Error for missing prompt");
    }
}
