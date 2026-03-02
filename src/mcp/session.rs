use crate::mcp::protocol::JsonRpcMessage;
use moka::future::Cache;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SessionState {
    Connected,
    Authenticated,
    Active,
    Closed,
}

#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub tx: mpsc::Sender<JsonRpcMessage>,
    #[allow(dead_code)]
    pub state: Arc<RwLock<SessionState>>,
    #[allow(dead_code)]
    pub created_at: std::time::Instant,
}

impl Session {
    pub fn new(id: String, tx: mpsc::Sender<JsonRpcMessage>) -> Self {
        Self {
            id,
            tx,
            state: Arc::new(RwLock::new(SessionState::Connected)),
            created_at: std::time::Instant::now(),
        }
    }

    #[allow(dead_code)]
    pub fn set_state(&self, state: SessionState) {
        if let Ok(mut s) = self.state.write() {
            *s = state;
        }
    }

    #[allow(dead_code)]
    pub fn get_state(&self) -> SessionState {
        self.state
            .read()
            .map(|s| *s)
            .unwrap_or(SessionState::Closed)
    }

    pub async fn send(&self, msg: JsonRpcMessage) -> anyhow::Result<()> {
        self.tx
            .send(msg)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send message: {}", e))
    }
}

#[derive(Clone)]
pub struct SessionManager {
    sessions: Cache<String, Session>,
}

impl SessionManager {
    pub fn new() -> Self {
        let sessions = Cache::builder()
            .time_to_idle(Duration::from_secs(3600)) // 1 hour idle timeout
            .eviction_listener(|key, _value, cause| {
                info!("Session {} evicted due to {:?}", key, cause);
            })
            .build();

        Self { sessions }
    }

    pub async fn add(&self, session: Session) {
        self.sessions.insert(session.id.clone(), session).await;
    }

    pub async fn get(&self, id: &str) -> Option<Session> {
        self.sessions.get(id).await
    }

    #[allow(dead_code)]
    pub async fn remove(&self, id: &str) {
        self.sessions.invalidate(id).await;
    }

    pub async fn broadcast(&self, msg: JsonRpcMessage) {
        for (_id, session) in self.sessions.iter() {
            let msg = msg.clone();
            tokio::spawn(async move {
                if let Err(e) = session.send(msg).await {
                    error!("Failed to broadcast to session {}: {}", session.id, e);
                }
            });
        }
    }

    #[allow(dead_code)]
    pub async fn send_message(&self, session_id: &str, msg: JsonRpcMessage) -> anyhow::Result<()> {
        if let Some(session) = self.get(session_id).await {
            session.send(msg).await
        } else {
            Err(anyhow::anyhow!("Session not found"))
        }
    }

    #[allow(dead_code)]
    pub fn broadcast_except(
        &self,
        session_id: &str,
        message: JsonRpcMessage,
    ) -> impl std::future::Future<Output = ()> + Send {
        let sessions = self.sessions.clone();
        let session_id = session_id.to_string();
        async move {
            let mut futures = Vec::new();
            for (key, session) in sessions.iter() {
                if *key != session_id {
                    let session = session.clone();
                    let msg = message.clone();
                    futures.push(async move {
                        let _ = session.send(msg).await;
                    });
                }
            }
            futures::future::join_all(futures).await;
        }
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
