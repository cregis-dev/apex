use apex::server::run_server;
use std::fs;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn test_hot_reload_routing() {
    // 1. Setup mock servers
    let (tx1, mut rx1) = tokio::sync::mpsc::channel(10);
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(10);

    tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:9081").await.unwrap();
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0; 1024];
            let _ = socket.read(&mut buf).await;
            let _ = tx1.send(()).await;
            let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });

    tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:9082").await.unwrap();
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0; 1024];
            let _ = socket.read(&mut buf).await;
            let _ = tx2.send(()).await;
            let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK";
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });

    // 2. Create initial config
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    // Initial config: only channel1 (9081)
    let initial_config = r#"{
        "version": "1",
        "global": {
            "listen": "127.0.0.1:9080",
            "auth": { "mode": "none", "keys": null },
            "timeouts": { "connect_ms": 1000, "request_ms": 3000, "response_ms": 3000 },
            "retries": { "max_attempts": 3, "backoff_ms": 100, "retry_on_status": [500, 502, 503, 504] }
        },
        "hot_reload": {
            "config_path": "",
            "watch": true
        },
        "metrics": { "enabled": false, "listen": "127.0.0.1:9090", "path": "/metrics" },
        "channels": [
            {
                "name": "channel1",
                "provider_type": "openai",
                "base_url": "http://127.0.0.1:9081",
                "api_key": "sk-test",
                "anthropic_base_url": null,
                "headers": null,
                "model_map": null,
                "timeouts": null
            }
        ],
        "routers": [
            {
                "name": "default",
                "rules": [
                    {
                        "match": { "models": ["*"] },
                        "channels": [{ "name": "channel1", "weight": 1 }],
                        "strategy": "priority"
                    }
                ],
                "channels": [],
                "strategy": "priority",
                "fallback_channels": [],
                "metadata": null
            }
        ],
        "teams": []
    }"#;

    fs::write(&config_path, initial_config).unwrap();

    // 3. Start server
    let path_clone = config_path.clone();
    let handle = tokio::spawn(async move {
        // We ignore error because run_server runs forever
        if let Err(e) = run_server(path_clone).await {
            eprintln!("Run server failed: {:?}", e);
        }
    });

    // Wait for server startup
    tokio::time::sleep(Duration::from_secs(2)).await;

    if handle.is_finished() {
        panic!("Server exited early!");
    }

    // 4. Test initial routing (channel1)
    let client = reqwest::Client::new();
    let resp = client
        .post("http://127.0.0.1:9080/v1/chat/completions")
        .body(r#"{"model": "gpt-4"}"#)
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success());
    // Verify channel1 received request
    assert!(rx1.try_recv().is_ok());
    assert!(rx2.try_recv().is_err());

    // 5. Update config: add channel2 (9082) and route "gpt-new" to it
    let updated_config = r#"{
        "version": "1",
        "global": {
            "listen": "127.0.0.1:9080",
            "auth": { "mode": "none", "keys": null },
            "timeouts": { "connect_ms": 1000, "request_ms": 3000, "response_ms": 3000 },
            "retries": { "max_attempts": 3, "backoff_ms": 100, "retry_on_status": [500, 502, 503, 504] }
        },
        "hot_reload": {
            "config_path": "",
            "watch": true
        },
        "metrics": { "enabled": false, "listen": "127.0.0.1:9090", "path": "/metrics" },
        "channels": [
            {
                "name": "channel1",
                "provider_type": "openai",
                "base_url": "http://127.0.0.1:9081",
                "api_key": "sk-test",
                "anthropic_base_url": null,
                "headers": null,
                "model_map": null,
                "timeouts": null
            },
            {
                "name": "channel2",
                "provider_type": "openai",
                "base_url": "http://127.0.0.1:9082",
                "api_key": "sk-test2",
                "anthropic_base_url": null,
                "headers": null,
                "model_map": null,
                "timeouts": null
            }
        ],
        "routers": [
            {
                "name": "default",
                "rules": [
                    {
                        "match": { "models": ["gpt-new"] },
                        "channels": [{ "name": "channel2", "weight": 1 }],
                        "strategy": "priority"
                    },
                    {
                        "match": { "models": ["*"] },
                        "channels": [{ "name": "channel1", "weight": 1 }],
                        "strategy": "priority"
                    }
                ],
                "channels": [],
                "strategy": "priority",
                "fallback_channels": [],
                "metadata": null
            }
        ],
        "teams": []
    }"#;

    fs::write(&config_path, updated_config).unwrap();

    // Wait for hot reload (debounce 500ms + some buffer)
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 6. Test new routing (gpt-new -> channel2)
    let resp = client
        .post("http://127.0.0.1:9080/v1/chat/completions")
        .body(r#"{"model": "gpt-new"}"#)
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success());
    // Verify channel2 received request
    assert!(rx2.try_recv().is_ok());
    // Channel1 should not receive it - wait, try_recv consumes the item if present
    // So if rx1 had an item from before, it was consumed in step 4.
    // So rx1 should be empty now.
    assert!(rx1.try_recv().is_err());
}
