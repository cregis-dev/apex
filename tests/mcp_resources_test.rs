use apex::config::{
    Channel, Config, Global, HotReload, Logging, Metrics, ProviderType, Retries, Team, Timeouts,
};
use apex::database::Database;
use apex::mcp::protocol::{Id, JsonRpcMessage, ListResourcesResult, ReadResourceResult, Request};
use apex::mcp::server::McpServer;
use std::sync::{Arc, RwLock};

#[tokio::test]
async fn test_mcp_resources() {
    // Setup config
    let config = Config {
        version: "1.0".to_string(),
        global: Global {
            listen: "127.0.0.1:8080".to_string(),
            auth_keys: vec!["sk-global-secret-key".to_string()],
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
            api_key: "sk-channel-secret-key".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
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
            id: "test-team".to_string(),
            api_key: "sk-team-secret-key".to_string(),
            policy: apex::config::TeamPolicy {
                allowed_routers: vec![],
                allowed_models: None,
                rate_limit: None,
            },
        }]),
        prompts: Arc::new(vec![]),
        compliance: None,
    };

    let database = Arc::new(Database::new(Some("/tmp".to_string())).unwrap());
    let server = McpServer::new(Arc::new(RwLock::new(config)), database);

    // Test resources/list
    let req = Request::new(Id::Number(1), "resources/list".to_string(), None);
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: ListResourcesResult = serde_json::from_value(r.result).unwrap();
        assert_eq!(result.resources.len(), 4);
        assert!(
            result
                .resources
                .iter()
                .any(|res| res.uri == "config://teams")
        );
    } else {
        panic!("Expected Response for resources/list");
    }

    // Test resources/read config://teams
    let req = Request::new(
        Id::Number(2),
        "resources/read".to_string(),
        Some(serde_json::json!({
            "uri": "config://teams"
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: ReadResourceResult = serde_json::from_value(r.result).unwrap();
        let content = result.contents[0].text.as_ref().unwrap();
        println!("Teams content: {}", content);
        assert!(content.contains("test-team"));
        assert!(!content.contains("sk-team-secret-key"));
        assert!(content.contains("sk-")); // Check prefix preservation
    } else {
        panic!("Expected Response for resources/read teams");
    }

    // Test resources/read config://config.json (global keys)
    let req = Request::new(
        Id::Number(3),
        "resources/read".to_string(),
        Some(serde_json::json!({
            "uri": "config://config.json"
        })),
    );
    let resp = server.handle_request(req).await;

    if let JsonRpcMessage::Response(r) = resp {
        let result: ReadResourceResult = serde_json::from_value(r.result).unwrap();
        let content = result.contents[0].text.as_ref().unwrap();
        println!("Config content: {}", content);
        assert!(!content.contains("sk-global-secret-key"));
        assert!(content.contains("sk-"));
    } else {
        panic!("Expected Response for resources/read config.json");
    }
}
