use crate::config::Config;
use crate::database::Database;
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
    body::Body,
    extract::{Query, Request as AxumRequest, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{
        IntoResponse, Response as AxumResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

#[derive(Clone)]
pub struct McpServer {
    pub config: Arc<RwLock<Config>>,
    sessions: Arc<SessionManager>,
    analytics: Arc<AnalyticsEngine>,
}

impl McpServer {
    pub fn new(config: Arc<RwLock<Config>>, database: Arc<Database>) -> Self {
        let sessions = Arc::new(SessionManager::new());
        let analytics = Arc::new(AnalyticsEngine::new(database));

        Self {
            config,
            sessions,
            analytics,
        }
    }

    #[allow(dead_code)]
    pub fn sessions(&self) -> Arc<SessionManager> {
        self.sessions.clone()
    }

    #[allow(dead_code)]
    pub async fn run_stdio(&self) -> anyhow::Result<()> {
        let (tx_to_server, mut rx_from_client) = mpsc::channel(32);
        let (tx_to_client, rx_from_server) = mpsc::channel(32);

        let transport = crate::mcp::transport::StdinTransport::new();
        tokio::spawn(transport.run(tx_to_server, rx_from_server));

        let session_id = "stdio".to_string();
        let session = crate::mcp::session::Session::from_sender(session_id.clone(), tx_to_client);
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
                let requested_protocol = req
                    .params
                    .as_ref()
                    .and_then(|params| params.get("protocolVersion"))
                    .and_then(|value| value.as_str());
                let protocol_version = match negotiate_protocol_version(requested_protocol) {
                    Ok(version) => version,
                    Err(message) => {
                        return JsonRpcMessage::Error(ErrorResponse::invalid_params(
                            Some(req.id),
                            Some(json!(message)),
                        ));
                    }
                };

                let result = json!({
                    "protocolVersion": protocol_version,
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

    fn build_usage_query(arguments: &serde_json::Value) -> crate::mcp::analytics::UsageQuery {
        crate::mcp::analytics::UsageQuery {
            team_id: arguments
                .get("team_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            router: arguments
                .get("router")
                .and_then(|v| v.as_str())
                .map(String::from),
            channel: arguments
                .get("channel")
                .and_then(|v| v.as_str())
                .map(String::from),
            model: arguments
                .get("model")
                .and_then(|v| v.as_str())
                .map(String::from),
            status: arguments
                .get("status")
                .and_then(|v| v.as_str())
                .map(String::from),
            start_time: arguments
                .get("start_time")
                .and_then(|v| v.as_str())
                .map(String::from),
            end_time: arguments
                .get("end_time")
                .and_then(|v| v.as_str())
                .map(String::from),
        }
    }

    fn build_usage_filter_properties(include_team_id: bool) -> serde_json::Value {
        let mut properties = serde_json::Map::from_iter([
            (
                "router".to_string(),
                json!({
                    "type": "string",
                    "description": "Filter by router name"
                }),
            ),
            (
                "channel".to_string(),
                json!({
                    "type": "string",
                    "description": "Filter by channel name"
                }),
            ),
            (
                "model".to_string(),
                json!({
                    "type": "string",
                    "description": "Filter by model name"
                }),
            ),
            (
                "status".to_string(),
                json!({
                    "type": "string",
                    "enum": ["success", "fallback", "error", "fallback_error", "errors", "fallbacks"],
                    "description": "Filter by status. Use 'errors' or 'fallbacks' for dashboard-style grouped filters"
                }),
            ),
            (
                "start_time".to_string(),
                json!({
                    "type": "string",
                    "description": "Start time in format YYYY-MM-DD or YYYY-MM-DD HH:MM:SS"
                }),
            ),
            (
                "end_time".to_string(),
                json!({
                    "type": "string",
                    "description": "End time in format YYYY-MM-DD or YYYY-MM-DD HH:MM:SS"
                }),
            ),
        ]);

        if include_team_id {
            properties.insert(
                "team_id".to_string(),
                json!({
                    "type": "string",
                    "description": "Filter by team ID"
                }),
            );
        }

        serde_json::Value::Object(properties)
    }

    fn build_usage_summary_properties() -> serde_json::Value {
        let mut properties = match Self::build_usage_filter_properties(true) {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };

        properties.insert(
            "group_by".to_string(),
            json!({
                "type": "string",
                "enum": ["team", "router", "channel", "model"],
                "description": "Optional grouping focus for the summary response"
            }),
        );
        properties.insert(
            "include_unknown_team".to_string(),
            json!({
                "type": "boolean",
                "default": true,
                "description": "When grouping by team, include records that cannot be mapped to a known team"
            }),
        );

        serde_json::Value::Object(properties)
    }

    fn json_text(value: serde_json::Value) -> Vec<ToolContent> {
        vec![ToolContent::Text {
            text: serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        }]
    }

    fn error_text(message: impl Into<String>) -> Vec<ToolContent> {
        vec![ToolContent::Text {
            text: json!({
                "error": message.into()
            })
            .to_string(),
        }]
    }

    fn summary_groups(
        stats: &crate::mcp::analytics::UsageStats,
        group_by: &str,
    ) -> Option<serde_json::Value> {
        match group_by {
            "team" => stats
                .by_team
                .as_ref()
                .map(|groups| serde_json::to_value(groups).unwrap_or_else(|_| json!({}))),
            "router" => Some(serde_json::to_value(&stats.by_router).unwrap_or_else(|_| json!({}))),
            "channel" => {
                Some(serde_json::to_value(&stats.by_channel).unwrap_or_else(|_| json!({})))
            }
            "model" => Some(serde_json::to_value(&stats.by_model).unwrap_or_else(|_| json!({}))),
            _ => None,
        }
    }

    fn build_usage_summary_response(
        &self,
        query: &crate::mcp::analytics::UsageQuery,
        group_by: Option<&str>,
        include_unknown_team: bool,
    ) -> Result<serde_json::Value, String> {
        let config = self.config.read().unwrap();

        let stats = if let Some(team_id) = query.team_id.as_deref() {
            let team = config
                .teams
                .iter()
                .find(|team| team.id == team_id)
                .ok_or_else(|| format!("Team '{}' not found", team_id))?;
            self.analytics
                .query_team_usage(team_id, &team.policy.allowed_routers, query)
                .map_err(|e| e.to_string())?
        } else if group_by == Some("team") {
            let teams: Vec<(&str, Vec<String>)> = config
                .teams
                .iter()
                .map(|team| (team.id.as_str(), team.policy.allowed_routers.clone()))
                .collect();
            self.analytics
                .query_all_teams_usage(&teams, query, include_unknown_team)
                .map_err(|e| e.to_string())?
        } else {
            self.analytics.get_stats(query).map_err(|e| e.to_string())?
        };

        let mut response = json!({
            "data_source": "sqlite",
            "filters": query,
            "summary": &stats
        });

        if let Some(group_by) = group_by {
            response["group_by"] = json!(group_by);
            if let Some(groups) = Self::summary_groups(&stats, group_by) {
                response["groups"] = groups;
            }
        }

        Ok(response)
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

        let query_usage_summary_schema = serde_json::json!({
            "type": "object",
            "properties": Self::build_usage_summary_properties(),
        });

        let query_usage_records_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "team_id": {
                    "type": "string",
                    "description": "Filter by team ID"
                },
                "router": {
                    "type": "string",
                    "description": "Filter by router name"
                },
                "channel": {
                    "type": "string",
                    "description": "Filter by channel name"
                },
                "model": {
                    "type": "string",
                    "description": "Filter by model name"
                },
                "status": {
                    "type": "string",
                    "enum": ["success", "fallback", "error", "fallback_error", "errors", "fallbacks"],
                    "description": "Filter by status. Use 'errors' or 'fallbacks' for grouped filters"
                },
                "start_time": {
                    "type": "string",
                    "description": "Start time in format YYYY-MM-DD or YYYY-MM-DD HH:MM:SS"
                },
                "end_time": {
                    "type": "string",
                    "description": "End time in format YYYY-MM-DD or YYYY-MM-DD HH:MM:SS"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "default": 50,
                    "description": "Maximum number of records to return"
                },
                "offset": {
                    "type": "integer",
                    "minimum": 0,
                    "default": 0,
                    "description": "Record offset for pagination"
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
                "channel": {
                    "type": "string",
                    "description": "Filter by channel name"
                },
                "model": {
                    "type": "string",
                    "description": "Filter by model name"
                },
                "status": {
                    "type": "string",
                    "enum": ["success", "fallback", "error", "fallback_error", "errors", "fallbacks"],
                    "description": "Filter by status. Use 'errors' or 'fallbacks' for grouped filters"
                },
                "start_time": {
                    "type": "string",
                    "description": "Start time in format YYYY-MM-DD or YYYY-MM-DD HH:MM:SS"
                },
                "end_time": {
                    "type": "string",
                    "description": "End time in format YYYY-MM-DD or YYYY-MM-DD HH:MM:SS"
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
                name: "query_usage_summary".to_string(),
                description: Some(
                    "Query aggregate usage metrics from SQLite with dashboard-aligned filters"
                        .to_string(),
                ),
                input_schema: query_usage_summary_schema,
            },
            Tool {
                name: "query_usage_records".to_string(),
                description: Some(
                    "Query paginated usage records from SQLite with dashboard-aligned filters"
                        .to_string(),
                ),
                input_schema: query_usage_records_schema,
            },
            Tool {
                name: "export_usage_report".to_string(),
                description: Some(
                    "Export filtered usage data from SQLite as JSON or CSV".to_string(),
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
                let channels = config
                    .channels
                    .iter()
                    .map(|channel| {
                        json!({
                            "name": channel.name,
                            "provider_type": channel.provider_type,
                            "model_map": channel.model_map,
                            "base_url": channel.base_url,
                        })
                    })
                    .collect::<Vec<_>>();

                Self::json_text(json!({
                    "data_source": "config",
                    "channels": channels
                }))
            }
            "query_usage_summary" => {
                let query = Self::build_usage_query(&arguments);
                let group_by = arguments.get("group_by").and_then(|v| v.as_str());
                let include_unknown_team = arguments
                    .get("include_unknown_team")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                match self.build_usage_summary_response(&query, group_by, include_unknown_team) {
                    Ok(response) => Self::json_text(response),
                    Err(e) => Self::error_text(format!("query_usage_summary failed: {}", e)),
                }
            }
            "query_usage_records" => {
                let query = Self::build_usage_query(&arguments);
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50)
                    .min(200) as usize;
                let offset = arguments
                    .get("offset")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;

                match self.analytics.query_usage_page(&query, limit, offset) {
                    Ok((records, total)) => Self::json_text(json!({
                        "data_source": "sqlite",
                        "filters": query,
                        "records": records,
                        "pagination": {
                            "total": total,
                            "limit": limit,
                            "offset": offset
                        }
                    })),
                    Err(e) => Self::error_text(format!("query_usage_records failed: {}", e)),
                }
            }
            "export_usage_report" => {
                let format = arguments
                    .get("format")
                    .and_then(|v| v.as_str())
                    .unwrap_or("json");
                let query = Self::build_usage_query(&arguments);

                let result = if format == "csv" {
                    self.analytics.export_csv(&query)
                } else {
                    self.analytics.export_json(&query)
                };

                match result {
                    Ok(data) => {
                        if format == "csv" {
                            vec![ToolContent::Text { text: data }]
                        } else {
                            Self::json_text(json!({
                                "data_source": "sqlite",
                                "filters": query,
                                "format": format,
                                "data": serde_json::from_str::<serde_json::Value>(&data).unwrap_or_else(|_| json!(data))
                            }))
                        }
                    }
                    Err(e) => Self::error_text(format!("export_usage_report failed: {}", e)),
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

// ============================================================================
// Streamable HTTP Transport (MCP Protocol 2025-11-25)
// ============================================================================

/// Query parameters for MCP endpoint (legacy session_id support for migration)
#[derive(Deserialize, Default)]
pub struct McpQuery {
    #[serde(default)]
    pub session_id: Option<String>,
}

use crate::server::AppState;

/// MCP Protocol Version header name
const MCP_PROTOCOL_VERSION_HEADER: &str = "MCP-Protocol-Version";
/// MCP Session ID header name
const MCP_SESSION_ID_HEADER: &str = "MCP-Session-Id";
/// Supported protocol versions, oldest to newest.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2024-11-05", "2025-11-25"];
const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";

fn is_supported_protocol_version(version: &str) -> bool {
    SUPPORTED_PROTOCOL_VERSIONS.contains(&version)
}

fn negotiate_protocol_version(version: Option<&str>) -> Result<&'static str, String> {
    match version {
        Some(version) if is_supported_protocol_version(version) => Ok(SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .find(|supported| **supported == version)
            .copied()
            .unwrap_or(DEFAULT_PROTOCOL_VERSION)),
        Some(version) => Err(format!(
            "Unsupported protocol version: {}. Supported: {}",
            version,
            SUPPORTED_PROTOCOL_VERSIONS.join(", ")
        )),
        None => Ok(DEFAULT_PROTOCOL_VERSION),
    }
}

/// Handle both GET and POST requests for Streamable HTTP transport
///
/// According to MCP spec:
/// - POST: Client sends JSON-RPC messages, server returns JSON or SSE stream
/// - GET: Client listens for server-initiated messages (optional)
pub async fn streamable_http_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<McpQuery>,
    req: AxumRequest,
) -> impl IntoResponse {
    use axum::http::Method;

    // Validate protocol version header if present
    if let Some(version) = headers.get(MCP_PROTOCOL_VERSION_HEADER)
        && let Ok(version_str) = version.to_str()
        && let Err(error) = negotiate_protocol_version(Some(version_str))
    {
        return AxumResponse::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .body(Body::from(json!({ "error": error }).to_string()))
            .unwrap();
    }

    // Validate Origin header for security (skip for local requests)
    if let Some(origin) = headers.get("Origin")
        && let Ok(origin_str) = origin.to_str()
    {
        // Check if origin is from localhost or same origin
        let is_local = origin_str.contains("localhost")
            || origin_str.contains("127.0.0.1")
            || origin_str.is_empty();

        if !is_local {
            // TODO: Implement proper origin validation against allowed origins
            // For now, log warning but allow (can be configured strictly in production)
            tracing::warn!("MCP request from external origin: {}", origin_str);
        }
    }

    match *req.method() {
        Method::POST => handle_post_request(&state, &headers, &query, req)
            .await
            .into_response(),
        Method::GET => handle_get_request(&state, &headers, &query)
            .await
            .into_response(),
        Method::DELETE => handle_delete_request(&state, &headers, &query)
            .await
            .into_response(),
        _ => AxumResponse::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"error": "Method not allowed"}"#))
            .unwrap(),
    }
}

/// Handle POST requests - main message handling
async fn handle_post_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    query: &McpQuery,
    req: AxumRequest,
) -> AxumResponse {
    let protocol_version = headers
        .get(MCP_PROTOCOL_VERSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|version| negotiate_protocol_version(Some(version)).ok())
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);

    // Extract session ID from header (preferred) or query param (legacy)
    let session_id = headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or(query.session_id.clone());

    // Read and parse the JSON-RPC message body
    let body_bytes = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            return AxumResponse::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"error": format!("Failed to read body: {}", e)}).to_string(),
                ))
                .unwrap();
        }
    };

    // Parse as generic JSON first to determine message type
    let json_value: serde_json::Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(e) => {
            return AxumResponse::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"error": format!("Invalid JSON: {}", e)}).to_string(),
                ))
                .unwrap();
        }
    };

    // Determine if this is a response/notification (no id) or request (has id)
    let is_request = json_value.get("id").is_some();

    // Try to parse as different message types
    let message: JsonRpcMessage = if is_request {
        match serde_json::from_value::<Request>(json_value.clone()) {
            Ok(req) => JsonRpcMessage::Request(req),
            Err(_) => {
                return AxumResponse::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"error": "Invalid JSON-RPC request"}).to_string(),
                    ))
                    .unwrap();
            }
        }
    } else {
        // Could be response, notification, or error
        match serde_json::from_value::<Request>(json_value.clone()) {
            Ok(req) => JsonRpcMessage::Request(req),
            Err(_) => match serde_json::from_value::<crate::mcp::protocol::Notification>(
                json_value.clone(),
            ) {
                Ok(notif) => JsonRpcMessage::Notification(notif),
                Err(_) => {
                    // Treat as generic accepted (for responses/acknowledgments)
                    return AxumResponse::builder()
                        .status(StatusCode::ACCEPTED)
                        .body(Body::empty())
                        .unwrap();
                }
            },
        }
    };

    // Handle session management for Initialize requests
    let mut session_id = session_id;
    let mut new_session_id: Option<String> = None;

    // Check if this is an initialize request to create a new session
    if let JsonRpcMessage::Request(ref req) = message
        && req.method == "initialize"
        && session_id.is_none()
    {
        // Create new session
        session_id = Some(Uuid::new_v4().to_string());
        new_session_id = session_id.clone();
    }

    // Get or create session
    let session_id = match session_id {
        Some(id) => id,
        None => {
            // For non-initialize requests without session, create temporary session
            Uuid::new_v4().to_string()
        }
    };

    let session_exists = state.mcp_server.sessions.get(&session_id).await.is_some();

    if !session_exists {
        // Create new session
        let session = crate::mcp::session::Session::with_channel(session_id.clone(), 100);
        state.mcp_server.sessions.add(session).await;
    }

    // Clone for async handling
    let _session = match state.mcp_server.sessions.get(&session_id).await {
        Some(s) => s,
        None => {
            return AxumResponse::builder()
                .status(StatusCode::NOT_FOUND)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"error": "Session not found"}"#))
                .unwrap();
        }
    };

    let mcp_server = state.mcp_server.clone();

    // Process the message
    let response = mcp_server.handle_message(message).await;

    // Build response with session ID header if it's a new session
    let mut response_builder =
        AxumResponse::builder().header(MCP_PROTOCOL_VERSION_HEADER, protocol_version);
    if let Some(ref sid) = new_session_id {
        response_builder = response_builder.header(MCP_SESSION_ID_HEADER, sid);
    }

    // Return response based on message type
    match (is_request, response) {
        (false, _) => {
            // Notification or response from client - just acknowledge
            AxumResponse::builder()
                .status(StatusCode::ACCEPTED)
                .body(Body::empty())
                .unwrap()
        }
        (true, Some(resp_msg)) => {
            // Check Accept header to determine response format
            let accept_json = headers
                .get("Accept")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.contains("application/json"))
                .unwrap_or(true);

            if accept_json {
                // Return JSON response
                let json_resp = match resp_msg {
                    JsonRpcMessage::Response(r) => serde_json::to_string(&r),
                    JsonRpcMessage::Error(e) => serde_json::to_string(&e),
                    _ => Ok(r#"{"error": "Unexpected response type"}"#.to_string()),
                };

                match json_resp {
                    Ok(json_str) => response_builder
                        .header("content-type", "application/json")
                        .body(Body::from(json_str))
                        .unwrap(),
                    Err(e) => AxumResponse::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({"error": format!("Serialization error: {}", e)}).to_string(),
                        ))
                        .unwrap(),
                }
            } else {
                // Return SSE stream (for long-running operations)
                // For simplicity, we still return JSON for single responses
                // Full SSE streaming could be implemented for tools that need it
                let json_resp = match resp_msg {
                    JsonRpcMessage::Response(r) => serde_json::to_string(&r),
                    JsonRpcMessage::Error(e) => serde_json::to_string(&e),
                    _ => Ok(r#"{"error": "Unexpected response type"}"#.to_string()),
                };

                match json_resp {
                    Ok(json_str) => response_builder
                        .header("content-type", "application/json")
                        .body(Body::from(json_str))
                        .unwrap(),
                    Err(_) => AxumResponse::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::empty())
                        .unwrap(),
                }
            }
        }
        (true, None) => {
            // No response (shouldn't happen for requests)
            AxumResponse::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"error": "No response generated"}"#))
                .unwrap()
        }
    }
}

/// Handle GET requests - server-initiated messages (SSE stream)
async fn handle_get_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    query: &McpQuery,
) -> AxumResponse {
    // Check if client accepts SSE
    let accept_header = headers
        .get("Accept")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !accept_header.contains("text/event-stream") {
        return AxumResponse::builder()
            .status(StatusCode::NOT_ACCEPTABLE)
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"error": "Client must accept text/event-stream"}"#,
            ))
            .unwrap();
    }

    let session_id = match &query.session_id {
        Some(id) => id.clone(),
        None => {
            // Generate new session for listening-only mode
            Uuid::new_v4().to_string()
        }
    };

    // Get or create session
    let _session = if let Some(s) = state.mcp_server.sessions.get(&session_id).await {
        s
    } else {
        // Create new session for listening
        let session = crate::mcp::session::Session::with_channel(session_id.clone(), 100);
        state.mcp_server.sessions.add(session).await;
        state.mcp_server.sessions.get(&session_id).await.unwrap()
    };

    let Some(rx) = _session.take_receiver().await else {
        return AxumResponse::builder()
            .status(StatusCode::CONFLICT)
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"error": "Session stream is already attached"}"#,
            ))
            .unwrap();
    };

    // For GET requests, we return an SSE stream that the client can use
    // to receive server-initiated notifications
    // Note: Full implementation would wire up the session to broadcast notifications

    // Create a simple SSE stream that sends keepalive events
    let initial_stream = tokio_stream::iter(vec![Ok::<Event, Infallible>(
        Event::default().event("connected").data("MCP stream ready"),
    )]);
    let message_stream = ReceiverStream::new(rx).map(|msg| {
        Ok::<Event, Infallible>(
            Event::default().data(serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string())),
        )
    });
    let stream = initial_stream.chain(message_stream);

    let sse = Sse::new(stream).keep_alive(KeepAlive::default());

    // Convert Sse to AxumResponse
    sse.into_response()
}

/// Handle DELETE requests - session termination
async fn handle_delete_request(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    query: &McpQuery,
) -> AxumResponse {
    let session_id = headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or(query.session_id.clone());

    match session_id {
        Some(id) => {
            match state.mcp_server.sessions.get(&id).await {
                Some(_) => {
                    // Remove session
                    state.mcp_server.sessions.remove(&id).await;
                    AxumResponse::builder()
                        .status(StatusCode::OK)
                        .body(Body::empty())
                        .unwrap()
                }
                None => AxumResponse::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"error": "Session not found"}"#))
                    .unwrap(),
            }
        }
        None => AxumResponse::builder()
            .status(StatusCode::BAD_REQUEST)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"error": "Missing session_id"}"#))
            .unwrap(),
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

        if k.is_none()
            && let Some(query_str) = req.uri().query()
        {
            for pair in query_str.split('&') {
                let mut parts = pair.split('=');
                if let Some(param_key) = parts.next()
                    && (param_key == "api_key" || param_key == "token")
                    && let Some(v) = parts.next()
                {
                    k = Some(v.to_string());
                    break;
                }
            }
        }
        k
    };

    let auth_keys = {
        let config = state.config.read().unwrap();
        config.global.auth_keys.clone()
    };

    // If no auth_keys configured, skip validation
    if auth_keys.is_empty() {
        return next.run(req).await;
    }

    let authorized = key.as_ref().map(|k| auth_keys.contains(k)).unwrap_or(false);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Global, HotReload, Metrics, Retries, Router, Timeouts};
    use crate::mcp::protocol::Id;

    fn test_config() -> Config {
        Config {
            version: "1.0".to_string(),
            global: Global {
                listen: "127.0.0.1:0".to_string(),
                auth_keys: vec![],
                timeouts: Timeouts {
                    connect_ms: 1000,
                    request_ms: 1000,
                    response_ms: 1000,
                },
                retries: Retries {
                    max_attempts: 1,
                    backoff_ms: 10,
                    retry_on_status: vec![500],
                },
                gemini_replay: crate::config::GeminiReplay::default(),
                enable_mcp: true,
                cors_allowed_origins: vec![],
            },
            logging: crate::config::Logging::default(),
            data_dir: "/tmp".to_string(),
            web_dir: "target/web".to_string(),
            channels: Arc::new(vec![]),
            routers: Arc::new(vec![Router {
                name: "default".to_string(),
                rules: vec![],
                channels: vec![],
                strategy: "round_robin".to_string(),
                metadata: None,
                fallback_channels: vec![],
            }]),
            metrics: Metrics {
                enabled: true,
                path: "/metrics".to_string(),
            },
            hot_reload: HotReload {
                config_path: "test-config.json".to_string(),
                watch: false,
            },
            teams: Arc::new(vec![]),
            prompts: Arc::new(vec![]),
            compliance: None,
        }
    }

    #[test]
    fn protocol_version_negotiation_accepts_inspector_version() {
        assert_eq!(
            negotiate_protocol_version(Some("2024-11-05")).unwrap(),
            "2024-11-05"
        );
        assert_eq!(
            negotiate_protocol_version(Some("2025-11-25")).unwrap(),
            "2025-11-25"
        );
    }

    #[test]
    fn protocol_version_negotiation_rejects_unknown_version() {
        let error = negotiate_protocol_version(Some("2099-01-01")).unwrap_err();
        assert!(error.contains("Unsupported protocol version"));
        assert!(error.contains("2024-11-05"));
        assert!(error.contains("2025-11-25"));
    }

    #[tokio::test]
    async fn initialize_echoes_negotiated_protocol_version() {
        let config = Arc::new(RwLock::new(test_config()));
        let database = Arc::new(Database::new(Some("/tmp".to_string())).unwrap());
        let server = McpServer::new(config, database);

        let request = Request {
            jsonrpc: "2.0".to_string(),
            id: Id::Number(1),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "inspector",
                    "version": "test"
                }
            })),
        };

        let response = server.handle_request(request).await;
        let JsonRpcMessage::Response(response) = response else {
            panic!("expected initialize response");
        };

        assert_eq!(response.result["protocolVersion"], "2024-11-05");
    }
}
