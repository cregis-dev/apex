use crate::config::Config;
use crate::mcp::analytics::AnalyticsEngine;
use crate::mcp::capabilities::ServerCapabilities;
use crate::mcp::protocol::{
    CallToolResult, ErrorResponse, GetPromptResult, JsonRpcMessage, ListPromptsResult,
    ListResourcesResult, ListToolsResult, Notification, PromptMessage as ProtocolPromptMessage,
    PromptMessageContent, ReadResourceResult, Request, Resource, ResourceContent, Response, Tool,
    ToolContent,
};
use crate::mcp::session::SessionManager;
use crate::utils::mask_secret;

fn mask_json_secrets(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::new();
            for (key, val) in map {
                let masked_key = if key.contains("key")
                    || key.contains("secret")
                    || key.contains("password")
                    || key.contains("token")
                {
                    mask_secret(key)
                } else {
                    key.clone()
                };
                new_map.insert(masked_key, mask_json_secrets(val));
            }
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(mask_json_secrets).collect())
        }
        serde_json::Value::String(s) => {
            if s.starts_with("sk-")
                || s.starts_with("sk_")
                || s.contains("secret")
                || s.contains("password")
                || (s.len() > 10 && s.chars().any(|c| c == '-'))
            {
                serde_json::Value::String(mask_secret(s))
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}
use axum::{
    Json,
    body::Body,
    extract::{Query, Request as AxumRequest, State},
    http::StatusCode,
    middleware::Next,
    response::{
        IntoResponse, Response as AxumResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures::{Stream, StreamExt};
use rand::{Rng, distributions::Alphanumeric};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct McpServer {
    pub config: Arc<RwLock<Config>>,
    sessions: Arc<SessionManager>,
    analytics: Arc<AnalyticsEngine>,
}

impl McpServer {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        let sessions = Arc::new(SessionManager::new());

        // Initialize analytics engine with log directory from config
        let log_dir = {
            let cfg = config.read().unwrap();
            cfg.logging.dir.clone()
        };
        let analytics = Arc::new(AnalyticsEngine::new(log_dir));

        Self {
            config,
            sessions,
            analytics,
        }
    }

    pub fn sessions(&self) -> Arc<SessionManager> {
        self.sessions.clone()
    }

    pub async fn run_stdio(&self) -> anyhow::Result<()> {
        let (tx_to_server, mut rx_from_client) = mpsc::channel(32);
        let (tx_to_client, rx_from_server) = mpsc::channel(32);

        let transport = crate::mcp::transport::StdinTransport::new();
        tokio::spawn(transport.run(tx_to_server, rx_from_server));

        let session_id = "stdio".to_string();
        let session = crate::mcp::session::Session::new(session_id.clone(), tx_to_client);
        self.sessions.add(session).await;

        while let Some(msg) = rx_from_client.recv().await {
            if let Some(resp) = self.handle_message(msg).await {
                let _ = self.sessions.send_message(&session_id, resp).await;
            }
        }
        Ok(())
    }

    pub async fn update_config(&self) {
        use crate::mcp::protocol::Notification;

        // Send capabilitiesChanged notification when config changes
        let notif_caps = Notification::new("notifications/capabilitiesChanged".to_string(), None);
        self.sessions
            .broadcast(JsonRpcMessage::Notification(notif_caps))
            .await;

        // Also send individual list_changed notifications
        let notif = Notification::new("notifications/resources/list_changed".to_string(), None);
        self.sessions
            .broadcast(JsonRpcMessage::Notification(notif))
            .await;

        let notif_tools = Notification::new("notifications/tools/list_changed".to_string(), None);
        self.sessions
            .broadcast(JsonRpcMessage::Notification(notif_tools))
            .await;

        let notif_prompts =
            Notification::new("notifications/prompts/list_changed".to_string(), None);
        self.sessions
            .broadcast(JsonRpcMessage::Notification(notif_prompts))
            .await;
    }

    pub async fn handle_message(&self, message: JsonRpcMessage) -> Option<JsonRpcMessage> {
        match message {
            JsonRpcMessage::Request(req) => Some(self.handle_request(req).await),
            JsonRpcMessage::Notification(notif) => {
                self.handle_notification(notif).await;
                None
            }
            _ => None,
        }
    }

    pub async fn handle_request(&self, req: Request) -> JsonRpcMessage {
        match req.method.as_str() {
            "initialize" => {
                let result = json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": ServerCapabilities::default(),
                    "serverInfo": {
                        "name": "apex-mcp-server",
                        "version": "0.1.0"
                    }
                });
                JsonRpcMessage::Response(Response::new(req.id, result))
            }
            "logging/setLevel" => self.handle_logging_set_level(req).await,
            "resources/list" => self.handle_resources_list(req).await,
            "resources/read" => self.handle_resources_read(req).await,
            "prompts/list" => self.handle_prompts_list(req).await,
            "prompts/get" => self.handle_prompts_get(req).await,
            "tools/list" => self.handle_tools_list(req).await,
            "tools/call" => self.handle_tools_call(req).await,
            "ping" => JsonRpcMessage::Response(Response::new(req.id, json!({}))),
            _ => JsonRpcMessage::Error(ErrorResponse::method_not_found(Some(req.id), None)),
        }
    }

    async fn handle_logging_set_level(&self, req: Request) -> JsonRpcMessage {
        let params = match req.params {
            Some(p) => p,
            None => {
                return JsonRpcMessage::Error(ErrorResponse::invalid_params(Some(req.id), None));
            }
        };

        let level = params
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("debug");

        // Map MCP log levels to tracing levels
        let log_level = match level {
            "debug" => "debug",
            "info" => "info",
            "warning" => "warn",
            "error" => "error",
            _ => "info",
        };

        tracing::info!("MCP client set log level to: {}", log_level);

        JsonRpcMessage::Response(Response::new(req.id, json!({})))
    }

    pub async fn handle_notification(&self, notif: Notification) {
        match notif.method.as_str() {
            "notifications/initialized" => {
                // Client initialized
            }
            _ => {
                tracing::warn!("Unknown notification: {}", notif.method);
            }
        }
    }

    async fn handle_resources_list(&self, req: Request) -> JsonRpcMessage {
        let resources = vec![
            Resource {
                uri: "config://teams".to_string(),
                name: "Teams Configuration".to_string(),
                description: Some("Team configurations".to_string()),
                mime_type: Some("application/json".to_string()),
            },
            Resource {
                uri: "config://routers".to_string(),
                name: "Routers Configuration".to_string(),
                description: Some("Router configurations".to_string()),
                mime_type: Some("application/json".to_string()),
            },
            Resource {
                uri: "config://channels".to_string(),
                name: "Channels Configuration".to_string(),
                description: Some("Channel configurations".to_string()),
                mime_type: Some("application/json".to_string()),
            },
            Resource {
                uri: "config://config.json".to_string(),
                name: "Full Configuration".to_string(),
                description: Some("Full server configuration".to_string()),
                mime_type: Some("application/json".to_string()),
            },
        ];

        let result = ListResourcesResult {
            resources,
            next_cursor: None,
        };

        JsonRpcMessage::Response(Response::new(req.id, serde_json::to_value(result).unwrap()))
    }

    async fn handle_resources_read(&self, req: Request) -> JsonRpcMessage {
        let params = match req.params {
            Some(p) => p,
            None => {
                return JsonRpcMessage::Error(ErrorResponse::invalid_params(Some(req.id), None));
            }
        };

        let uri = match params.get("uri").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => {
                return JsonRpcMessage::Error(ErrorResponse::invalid_params(Some(req.id), None));
            }
        };

        let config = self.config.read().unwrap();
        let content = match uri {
            "config://teams" => serde_json::to_string_pretty(&config.teams).unwrap(),
            "config://routers" => serde_json::to_string_pretty(&config.routers).unwrap(),
            "config://channels" => serde_json::to_string_pretty(&config.channels).unwrap(),
            "config://config.json" => serde_json::to_string_pretty(&*config).unwrap(),
            _ => {
                return JsonRpcMessage::Error(ErrorResponse::invalid_params(
                    Some(req.id),
                    Some(json!("Resource not found")),
                ));
            }
        };

        // Mask secrets in the JSON - only mask specific fields, not the entire content
        let value: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|_| serde_json::Value::String(content.clone()));
        let masked_value = mask_json_secrets(&value);
        let masked_content = serde_json::to_string(&masked_value).unwrap_or(content);

        let result = ReadResourceResult {
            contents: vec![ResourceContent {
                uri: uri.to_string(),
                mime_type: Some("application/json".to_string()),
                text: Some(masked_content),
                blob: None,
            }],
        };

        JsonRpcMessage::Response(Response::new(req.id, serde_json::to_value(result).unwrap()))
    }

    async fn handle_prompts_list(&self, req: Request) -> JsonRpcMessage {
        let config = self.config.read().unwrap();
        let prompts = config
            .prompts
            .iter()
            .map(|p| {
                let arguments: Option<Vec<crate::mcp::protocol::PromptArgument>> =
                    if p.arguments.is_empty() {
                        None
                    } else {
                        Some(
                            p.arguments
                                .iter()
                                .map(|a| crate::mcp::protocol::PromptArgument {
                                    name: a.name.clone(),
                                    description: a.description.clone(),
                                    required: a.required,
                                })
                                .collect(),
                        )
                    };

                crate::mcp::protocol::Prompt {
                    name: p.name.clone(),
                    description: p.description.clone(),
                    arguments,
                }
            })
            .collect();

        let result = ListPromptsResult {
            prompts,
            next_cursor: None,
        };

        JsonRpcMessage::Response(Response::new(req.id, serde_json::to_value(result).unwrap()))
    }

    async fn handle_prompts_get(&self, req: Request) -> JsonRpcMessage {
        let params = match req.params {
            Some(p) => p,
            None => {
                return JsonRpcMessage::Error(ErrorResponse::invalid_params(Some(req.id), None));
            }
        };

        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => {
                return JsonRpcMessage::Error(ErrorResponse::invalid_params(Some(req.id), None));
            }
        };

        let config = self.config.read().unwrap();
        let prompt = match config.prompts.iter().find(|p| p.name == name) {
            Some(p) => p,
            None => {
                return JsonRpcMessage::Error(ErrorResponse::invalid_params(
                    Some(req.id),
                    Some(json!("Prompt not found")),
                ));
            }
        };

        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        let mut messages = vec![];

        for msg in &prompt.messages {
            let mut content_text = match &msg.content {
                crate::config::PromptContent::Text { text } => text.clone(),
            };

            // Replace arguments
            if let Some(args_map) = arguments.as_object() {
                for (key, value) in args_map {
                    if let Some(val_str) = value.as_str() {
                        content_text = content_text.replace(&format!("{{{{ {} }}}}", key), val_str);
                    }
                }
            }

            messages.push(ProtocolPromptMessage {
                role: msg.role.clone(),
                content: PromptMessageContent::Text { text: content_text },
            });
        }

        let result = GetPromptResult {
            description: prompt.description.clone(),
            messages,
        };

        JsonRpcMessage::Response(Response::new(req.id, serde_json::to_value(result).unwrap()))
    }

    async fn handle_tools_list(&self, req: Request) -> JsonRpcMessage {
        let echo_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"]
        });

        let list_models_schema = serde_json::json!({
            "type": "object",
            "properties": {},
        });

        let query_team_usage_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "team_id": {
                    "type": "string",
                    "description": "The team ID to query usage for"
                },
                "router": {
                    "type": "string",
                    "description": "Filter by router name"
                },
                "model": {
                    "type": "string",
                    "description": "Filter by model name"
                },
                "start_time": {
                    "type": "string",
                    "description": "Start time in format YYYY-MM-DD HH:MM:SS"
                },
                "end_time": {
                    "type": "string",
                    "description": "End time in format YYYY-MM-DD HH:MM:SS"
                }
            },
        });

        let query_all_teams_usage_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "model": {
                    "type": "string",
                    "description": "Filter by model name"
                },
                "start_time": {
                    "type": "string",
                    "description": "Start time in format YYYY-MM-DD HH:MM:SS"
                },
                "end_time": {
                    "type": "string",
                    "description": "End time in format YYYY-MM-DD HH:MM:SS"
                }
            },
        });

        let export_usage_report_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "team_id": {
                    "type": "string",
                    "description": "The team ID to export usage for"
                },
                "router": {
                    "type": "string",
                    "description": "Filter by router name"
                },
                "model": {
                    "type": "string",
                    "description": "Filter by model name"
                },
                "start_time": {
                    "type": "string",
                    "description": "Start time in format YYYY-MM-DD HH:MM:SS"
                },
                "end_time": {
                    "type": "string",
                    "description": "End time in format YYYY-MM-DD HH:MM:SS"
                },
                "format": {
                    "type": "string",
                    "enum": ["json", "csv"],
                    "description": "Export format: json or csv",
                    "default": "json"
                }
            },
            "required": ["format"]
        });

        let tools = vec![
            Tool {
                name: "echo".to_string(),
                description: Some("Echoes back the input".to_string()),
                input_schema: echo_schema,
            },
            Tool {
                name: "list_models".to_string(),
                description: Some(
                    "Lists all configured channels and their model mappings".to_string(),
                ),
                input_schema: list_models_schema,
            },
            Tool {
                name: "query_team_usage".to_string(),
                description: Some(
                    "Query usage statistics for a team with optional filters (router, model, time range)".to_string(),
                ),
                input_schema: query_team_usage_schema,
            },
            Tool {
                name: "query_all_teams_usage".to_string(),
                description: Some(
                    "Query usage statistics for all teams, returns aggregated stats grouped by team ID".to_string(),
                ),
                input_schema: query_all_teams_usage_schema,
            },
            Tool {
                name: "export_usage_report".to_string(),
                description: Some(
                    "Export usage data as JSON or CSV with optional filters".to_string(),
                ),
                input_schema: export_usage_report_schema,
            },
        ];

        let result = ListToolsResult {
            tools,
            next_cursor: None,
        };

        JsonRpcMessage::Response(Response::new(req.id, serde_json::to_value(result).unwrap()))
    }

    async fn handle_tools_call(&self, req: Request) -> JsonRpcMessage {
        let params = match req.params {
            Some(p) => p,
            None => {
                return JsonRpcMessage::Error(ErrorResponse::invalid_params(Some(req.id), None));
            }
        };

        let name = match params.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => {
                return JsonRpcMessage::Error(ErrorResponse::invalid_params(Some(req.id), None));
            }
        };

        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        let content = match name {
            "echo" => {
                let message = arguments
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                vec![ToolContent::Text {
                    text: message.to_string(),
                }]
            }
            "list_models" => {
                let config = self.config.read().unwrap();
                let mut output = String::new();
                for channel in config.channels.iter() {
                    output.push_str(&format!("Channel: {}\n", channel.name));
                    if let Some(map) = &channel.model_map {
                        for (k, v) in map {
                            output.push_str(&format!("  {} -> {}\n", k, v));
                        }
                    } else {
                        output.push_str("  No model map (pass-through)\n");
                    }
                    output.push('\n');
                }
                vec![ToolContent::Text { text: output }]
            }
            "query_team_usage" => {
                // Parse query parameters
                let team_id = arguments.get("team_id").and_then(|v| v.as_str());
                let router = arguments.get("router").and_then(|v| v.as_str());
                let model = arguments.get("model").and_then(|v| v.as_str());
                let start_time = arguments.get("start_time").and_then(|v| v.as_str());
                let end_time = arguments.get("end_time").and_then(|v| v.as_str());

                let query = crate::mcp::analytics::UsageQuery {
                    team_id: team_id.map(String::from),
                    router: router.map(String::from),
                    model: model.map(String::from),
                    start_time: start_time.map(String::from),
                    end_time: end_time.map(String::from),
                };

                // If team_id is provided, get team routers from config
                if let Some(tid) = team_id {
                    let config = self.config.read().unwrap();
                    if let Some(team) = config.teams.iter().find(|t| t.id == tid) {
                        let team_routers = team.policy.allowed_routers.clone();
                        match self.analytics.query_team_usage(tid, &team_routers, &query) {
                            Ok(stats) => {
                                let json_str =
                                    serde_json::to_string_pretty(&stats).unwrap_or_default();
                                vec![ToolContent::Text { text: json_str }]
                            }
                            Err(e) => {
                                vec![ToolContent::Text {
                                    text: format!("Error: {}", e),
                                }]
                            }
                        }
                    } else {
                        vec![ToolContent::Text {
                            text: format!("Team '{}' not found", tid),
                        }]
                    }
                } else {
                    // Query without team filter
                    match self.analytics.get_stats(&query) {
                        Ok(stats) => {
                            let json_str = serde_json::to_string_pretty(&stats).unwrap_or_default();
                            vec![ToolContent::Text { text: json_str }]
                        }
                        Err(e) => {
                            vec![ToolContent::Text {
                                text: format!("Error: {}", e),
                            }]
                        }
                    }
                }
            }
            "query_all_teams_usage" => {
                // Parse query parameters
                let model = arguments.get("model").and_then(|v| v.as_str());
                let start_time = arguments.get("start_time").and_then(|v| v.as_str());
                let end_time = arguments.get("end_time").and_then(|v| v.as_str());

                let query = crate::mcp::analytics::UsageQuery {
                    team_id: None,
                    router: None,
                    model: model.map(String::from),
                    start_time: start_time.map(String::from),
                    end_time: end_time.map(String::from),
                };

                // Get all teams from config
                let config = self.config.read().unwrap();
                let teams: Vec<(&str, Vec<String>)> = config
                    .teams
                    .iter()
                    .map(|t| (t.id.as_str(), t.policy.allowed_routers.clone()))
                    .collect();

                match self.analytics.query_all_teams_usage(&teams, &query) {
                    Ok(stats) => {
                        let json_str = serde_json::to_string_pretty(&stats).unwrap_or_default();
                        vec![ToolContent::Text { text: json_str }]
                    }
                    Err(e) => {
                        vec![ToolContent::Text {
                            text: format!("Error: {}", e),
                        }]
                    }
                }
            }
            "export_usage_report" => {
                // Parse query parameters
                let team_id = arguments.get("team_id").and_then(|v| v.as_str());
                let router = arguments.get("router").and_then(|v| v.as_str());
                let model = arguments.get("model").and_then(|v| v.as_str());
                let start_time = arguments.get("start_time").and_then(|v| v.as_str());
                let end_time = arguments.get("end_time").and_then(|v| v.as_str());
                let format = arguments
                    .get("format")
                    .and_then(|v| v.as_str())
                    .unwrap_or("json");

                let query = crate::mcp::analytics::UsageQuery {
                    team_id: team_id.map(String::from),
                    router: router.map(String::from),
                    model: model.map(String::from),
                    start_time: start_time.map(String::from),
                    end_time: end_time.map(String::from),
                };

                let result = if format == "csv" {
                    self.analytics.export_csv(&query)
                } else {
                    self.analytics.export_json(&query)
                };

                match result {
                    Ok(data) => vec![ToolContent::Text { text: data }],
                    Err(e) => vec![ToolContent::Text {
                        text: format!("Error: {}", e),
                    }],
                }
            }
            _ => {
                return JsonRpcMessage::Error(ErrorResponse::method_not_found(
                    Some(req.id),
                    Some(json!(format!("Tool '{}' not found", name))),
                ));
            }
        };

        let result = CallToolResult {
            content,
            is_error: Some(false),
        };

        JsonRpcMessage::Response(Response::new(req.id, serde_json::to_value(result).unwrap()))
    }
}

#[derive(Deserialize)]
pub struct SseQuery {
    #[serde(default)]
    session_id: Option<String>,
}

use crate::server::AppState;

pub async fn sse_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SseQuery>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let session_id = query.session_id.unwrap_or_else(|| {
        rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect()
    });

    let (tx, rx) = mpsc::channel(100);
    let session = crate::mcp::session::Session::new(session_id.clone(), tx);
    state.mcp_server.sessions.add(session).await;

    let endpoint_url = format!("/mcp/messages?session_id={}", session_id);
    let endpoint_event = Event::default().event("endpoint").data(endpoint_url);

    let initial_stream = tokio_stream::iter(vec![Ok(endpoint_event)]);

    let message_stream = tokio_stream::wrappers::ReceiverStream::new(rx)
        .map(|msg| Ok(Event::default().data(serde_json::to_string(&msg).unwrap())));

    let stream = initial_stream.chain(message_stream);

    Sse::new(stream).keep_alive(KeepAlive::default())
}

pub async fn messages_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SseQuery>,
    Json(message): Json<JsonRpcMessage>,
) -> impl IntoResponse {
    let session_id = match query.session_id {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "Missing session_id").into_response(),
    };

    if let Some(session) = state.mcp_server.sessions.get(&session_id).await {
        let mcp_server = state.mcp_server.clone();
        tokio::spawn(async move {
            if let Some(response) = mcp_server.handle_message(message).await {
                let _ = session.send(response).await;
            }
        });
        (StatusCode::OK, "Accepted").into_response()
    } else {
        (StatusCode::NOT_FOUND, "Session not found").into_response()
    }
}

pub async fn mcp_auth_guard(
    State(state): State<McpServer>,
    req: AxumRequest,
    next: Next,
) -> AxumResponse {
    let key = {
        let headers = req.headers();
        let auth_header = headers.get("authorization").and_then(|v| v.to_str().ok());
        let x_api_key = headers.get("x-api-key").and_then(|v| v.to_str().ok());

        let mut k = None;

        if let Some(auth) = auth_header {
            if let Some(stripped) = auth.strip_prefix("Bearer ") {
                k = Some(stripped.to_string());
            } else {
                k = Some(auth.to_string());
            }
        } else if let Some(x_key) = x_api_key {
            k = Some(x_key.to_string());
        }

        if k.is_none() {
            if let Some(query_str) = req.uri().query() {
                for pair in query_str.split('&') {
                    let mut parts = pair.split('=');
                    if let Some(param_key) = parts.next() {
                        if param_key == "api_key" || param_key == "token" {
                            if let Some(v) = parts.next() {
                                k = Some(v.to_string());
                                break;
                            }
                        }
                    }
                }
            }
        }
        k
    };

    let (mode, global_keys) = {
        let config = state.config.read().unwrap();
        (
            config.global.auth.mode.clone(),
            config.global.auth.keys.clone(),
        )
    };

    match mode {
        crate::config::AuthMode::None => next.run(req).await,
        crate::config::AuthMode::ApiKey => {
            let authorized = if let Some(api_key) = &key {
                if let Some(keys) = &global_keys {
                    keys.contains(api_key)
                } else {
                    false
                }
            } else {
                false
            };

            if authorized {
                next.run(req).await
            } else {
                AxumResponse::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"error": "Unauthorized"}"#))
                    .unwrap()
            }
        }
    }
}
