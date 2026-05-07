#![allow(dead_code)]
#![allow(unused_imports)]

use apex::config::{
    Channel, Config, Global, HotReload, Metrics, ProviderType, Retries, Router as GatewayRouter,
    Timeouts,
};
use apex::server::{build_app, build_state};
use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tower::ServiceExt;

pub fn base_config() -> Config {
    Config {
        version: "1".to_string(),
        global: Global {
            listen: "127.0.0.1:0".to_string(),
            auth_keys: vec![],
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
            gemini_replay: apex::config::GeminiReplay::default(),
            cors_allowed_origins: vec![],
        },
        metrics: Metrics {
            enabled: true,
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
        data_dir: "/tmp".to_string(),
        web_dir: "target/web".to_string(),
        teams: std::sync::Arc::new(vec![]),
        channels: std::sync::Arc::new(vec![]),
        routers: std::sync::Arc::new(vec![]),
        compliance: None,
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

#[derive(Debug, Clone)]
pub struct CapturedRequest {
    pub method: String,
    pub path: String,
    pub body: String,
}

pub async fn spawn_upstream_capture(
    status: StatusCode,
    body: &'static str,
) -> (SocketAddr, Arc<Mutex<Vec<CapturedRequest>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let captures = Arc::new(Mutex::new(Vec::new()));
    let captures_for_app = Arc::clone(&captures);

    let app = axum::Router::new().fallback(move |req: Request<Body>| {
        let captures = Arc::clone(&captures_for_app);
        async move {
            let (parts, body_stream) = req.into_parts();
            let body_bytes = axum::body::to_bytes(body_stream, usize::MAX).await.unwrap();
            captures.lock().unwrap().push(CapturedRequest {
                method: parts.method.to_string(),
                path: parts.uri.path().to_string(),
                body: String::from_utf8(body_bytes.to_vec()).unwrap(),
            });

            (status, [("content-type", "application/json")], body)
        }
    });

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (addr, captures)
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
    let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_text = String::from_utf8(body_bytes.to_vec()).unwrap();
    (status, body_text)
}
