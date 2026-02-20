use axum::body::{to_bytes, Body, Bytes};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router as AxumRouter;
use axum::Json;
use apex::config::{
    Auth, AuthMode, Channel, Config, Global, HotReload, Metrics, ProviderType, Retries,
    Router as GatewayRouter, Timeouts,
};
use apex::server::{build_app, build_state};
use serde_json::json;
use std::net::SocketAddr;
use std::time::Duration;
use tower::ServiceExt;
use tokio::net::TcpListener;

// --- Helper Functions for Integration Tests ---

pub async fn spawn_app(app: AxumRouter) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let make_service = app.into_make_service();
    tokio::spawn(async move {
        let _ = axum::serve(listener, make_service).await;
    });
    // Wait for port to be open
    for _ in 0..20 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    addr
}

pub async fn spawn_upstream_ok() -> SocketAddr {
    let app = AxumRouter::new().route(
        "/v1/chat/completions",
        post(|_: Bytes| async { (StatusCode::OK, Json(json!({"ok": true}))) }),
    );
    spawn_app(app).await
}

pub async fn spawn_upstream_status(status: StatusCode, body: &'static str) -> SocketAddr {
    let app = AxumRouter::new().route(
        "/v1/chat/completions",
        post(move |_: Bytes| async move { (status, body) }),
    );
    spawn_app(app).await
}

pub async fn spawn_upstream_models() -> SocketAddr {
    let app = AxumRouter::new().route(
        "/v1/models",
        get(|| async { (StatusCode::OK, Json(json!({"data": []}))) }),
    );
    spawn_app(app).await
}

pub async fn response_text(resp: axum::response::Response<Body>) -> (StatusCode, String) {
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap_or_default();
    let text = String::from_utf8_lossy(&body).to_string();
    (status, text)
}

pub fn base_url(addr: SocketAddr) -> String {
    format!("http://{}", addr)
}

pub async fn ensure_upstream_ok(addr: SocketAddr, path: &str) {
    let client = reqwest::Client::builder().http1_only().build().unwrap();
    let url = format!("http://{}{}", addr, path);
    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .body(r#"{"ping":"pong"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

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
                connect_ms: 2000,
                request_ms: 30000,
                response_ms: 30000,
            },
            retries: Retries {
                max_attempts: 2,
                backoff_ms: 20,
                retry_on_status: vec![429, 500, 502, 503, 504],
            },
        },
        channels: vec![],
        routers: vec![],
        metrics: Metrics {
            enabled: false,
            listen: "127.0.0.1:0".to_string(),
            path: "/metrics".to_string(),
        },
        hot_reload: HotReload {
            config_path: "test".to_string(),
            watch: false,
        },
    }
}
