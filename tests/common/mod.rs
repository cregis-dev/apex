#![allow(dead_code)]
#![allow(unused_imports)]

use apex::config::{
    Auth, AuthMode, Channel, Config, Global, HotReload, Metrics, ProviderType, Retries,
    Router as GatewayRouter, Timeouts,
};
use apex::server::{build_app, build_state};
use axum::http::StatusCode;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tower::ServiceExt;

pub fn base_config() -> Config {
    Config {
        version: "1".to_string(),
        global: Global {
            listen: "127.0.0.1:0".to_string(),
            auth: Auth {
                mode: AuthMode::None,
                keys: None,
            },
            timeouts: Timeouts {
                connect_ms: 1000,
                request_ms: 1000,
                response_ms: 1000,
            },
            retries: Retries {
                max_attempts: 3,
                backoff_ms: 100,
                retry_on_status: vec![500, 502, 503, 504],
            },
        },
        metrics: Metrics {
            enabled: true,
            listen: "127.0.0.1:0".to_string(),
            path: "/metrics".to_string(),
        },
        hot_reload: HotReload {
            config_path: "config.json".to_string(),
            watch: false,
        },
        logging: apex::config::Logging {
            level: "info".to_string(),
            dir: None,
        },
        teams: vec![],
        channels: vec![],
        routers: vec![],
    }
}

pub async fn spawn_upstream_ok() -> SocketAddr {
    spawn_upstream_status(StatusCode::OK, r#"{"id":"test","object":"chat.completion","created":1677652288,"choices":[{"index":0,"message":{"role":"assistant","content":"Hello from upstream"},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":12,"total_tokens":21}}"#).await
}

pub async fn spawn_upstream_status(status: StatusCode, body: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let app = axum::Router::new().fallback(move || async move {
        (
            status,
            axum::response::Json(serde_json::from_str::<serde_json::Value>(body).unwrap()),
        )
    });

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    addr
}

pub async fn spawn_upstream_models() -> SocketAddr {
    spawn_upstream_status(StatusCode::OK, r#"{"object":"list","data":[{"id":"gpt-4","object":"model","created":1686935002,"owned_by":"openai"},{"id":"gpt-3.5-turbo","object":"model","created":1677610602,"owned_by":"openai"}]}"#).await
}

pub async fn ensure_upstream_ok(addr: SocketAddr, path: &str) {
    let client = reqwest::Client::new();
    let url = format!("http://{}/{}", addr, path);
    for _ in 0..10 {
        if client.get(&url).send().await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("Upstream not ready: {}", addr);
}

pub fn base_url(addr: SocketAddr) -> String {
    format!("http://{}", addr)
}

pub async fn response_text(resp: axum::response::Response) -> (StatusCode, String) {
    let status = resp.status();
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body_text = String::from_utf8(body_bytes.to_vec()).unwrap();
    (status, body_text)
}
