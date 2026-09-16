//! Protocol entry points. Each one resolves the route kind and hands off to
//! the shared request pipeline; `/v1/models` is the exception, answering from
//! config and usage history rather than proxying upstream.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use serde_json::json;

use std::collections::BTreeSet;

use crate::config::Config;
use crate::middleware::auth::TeamContext;
use crate::middleware::compliance::OriginalModelName;
use crate::providers::RouteKind;

use super::AppState;
use super::errors::{error_response, gemini_native_error_response};
use super::gemini_route::validate_gemini_native_route;
use super::pipeline::{process_gemini_native_direct_pass, process_request};

pub(super) async fn handle_openai(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    process_request(state, req, RouteKind::Openai, None, None).await
}

pub(super) async fn handle_anthropic(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    process_request(state, req, RouteKind::Anthropic, None, None).await
}

pub(super) async fn handle_gemini_native(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(path): axum::extract::Path<String>,
    req: Request<Body>,
) -> Response<Body> {
    let route = match validate_gemini_native_route(req.method(), &path) {
        Some(route) => route,
        None => {
            return gemini_native_error_response(
                StatusCode::NOT_FOUND,
                "Gemini native endpoint is not allowlisted",
                "NOT_FOUND",
            );
        }
    };

    let (mut parts, body) = req.into_parts();
    let routing_model = route.routing_model;
    parts
        .extensions
        .insert(OriginalModelName(routing_model.clone()));
    let req = Request::from_parts(parts, body);
    if route.direct_pass {
        return process_gemini_native_direct_pass(state, req, routing_model).await;
    }
    process_request(state, req, RouteKind::GeminiNative, None, None).await
}

/// `GET /v1/models` (and `/models`). Returns the list of concrete model ids
/// the *team* associated with the inbound API key is allowed to call, in
/// OpenAI's list-models format. Admin / global keys are intentionally
/// rejected here — this endpoint exists to bootstrap end-user clients, not
/// to power admin tooling.
pub(super) async fn handle_models(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Response<Body> {
    let team_id = match req.extensions().get::<TeamContext>() {
        Some(ctx) => ctx.team_id.clone(),
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Team API Key required: /v1/models only resolves models for a specific team",
            );
        }
    };

    let config = state.config.read().unwrap().clone();
    let Some(team) = config.teams.iter().find(|t| t.id == team_id) else {
        return error_response(StatusCode::UNAUTHORIZED, "Team not found");
    };

    // -- 1. Collect candidate model ids ------------------------------------
    // Anything that is a literal model name in the rules of a router this
    // team is allowed to use. Glob patterns like "*" / "deepseek-*" are
    // skipped because OpenAI's list-models payload requires concrete ids.
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    for router_name in &team.policy.allowed_routers {
        let Some(router) = config.routers.iter().find(|r| &r.name == router_name) else {
            continue;
        };
        for rule in &router.rules {
            for pattern in &rule.match_spec.models {
                if !is_glob_pattern(pattern) {
                    candidates.insert(pattern.clone());
                }
            }
        }
    }

    // Augment with concrete model ids actually observed in the usage log for
    // this team. This covers the common case where the only router rule is
    // a glob (e.g. `deepseek-*`) — without history the team would otherwise
    // see an empty list.
    if let Ok(history) = state.database.distinct_models_for_team(&team_id) {
        candidates.extend(history);
    }

    // -- 2. Filter by team policy + verify a router can actually route it --
    let mut entries: Vec<serde_json::Value> = Vec::with_capacity(candidates.len());
    for model in candidates {
        if !team.policy.is_model_allowed(&model) {
            continue;
        }

        let Some((router_name, channel_name)) =
            resolve_model_to_router_channel(&config, &team.policy.allowed_routers, &model, &state)
        else {
            continue;
        };

        let owned_by = config
            .channels
            .iter()
            .find(|c| c.name == channel_name)
            .map(|c| format!("{:?}", c.provider_type).to_lowercase())
            .unwrap_or_else(|| "apex".to_string());

        entries.push(json!({
            "id": model,
            "object": "model",
            "created": 0,
            "owned_by": owned_by,
            "apex": {
                "router": router_name,
                "channel": channel_name,
            }
        }));
    }

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "object": "list",
                "data": entries,
            })
            .to_string(),
        ))
        .unwrap()
}

/// A pattern is "glob-like" if it contains any of the meta-characters glob
/// would interpret. Bare literal model ids (which is what OpenAI's list
/// endpoint must return) contain none of these.
fn is_glob_pattern(pattern: &str) -> bool {
    pattern.chars().any(|c| matches!(c, '*' | '?' | '[' | ']'))
}

/// Try each of the team's allowed routers; if one can route the given model
/// to a concrete channel, return `(router_name, channel_name)`. We avoid
/// returning models the gateway would actually 404 on.
fn resolve_model_to_router_channel(
    config: &Config,
    allowed_routers: &[String],
    model: &str,
    state: &AppState,
) -> Option<(String, String)> {
    for router_name in allowed_routers {
        let Some(router) = config.routers.iter().find(|r| &r.name == router_name) else {
            continue;
        };
        if let Some(channel) = state.selector.select_channel(router, model) {
            return Some((router.name.clone(), channel));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Channel, ProviderType};

    use crate::gemini_compat::GeminiAnthropicReplayCache;
    use crate::metrics::MetricsState;
    use crate::middleware::ratelimit::TeamRateLimiter;
    use crate::providers::ProviderRegistry;
    use crate::router_selector::RouterSelector;
    use crate::server::test_fixtures::*;
    use crate::usage::UsageLogger;
    use std::sync::{Mutex, RwLock};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_rate_limiter_blocks() {
        let config = create_test_config();
        let config_arc = Arc::new(RwLock::new(config));
        let audit_calls = Arc::new(Mutex::new(Vec::new()));

        let (_dir, database) = create_test_database();
        let state = Arc::new(AppState {
            config: config_arc,
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: audit_calls.clone(),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: false }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            database,
            web_dir: "target/web".to_string(),
        });

        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer test-vkey")
            .body(Body::from("{}"))
            .unwrap();

        let resp = handle_openai(State(state), req).await;

        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn test_access_audit_logged() {
        let config = create_test_config();
        let config_arc = Arc::new(RwLock::new(config));
        let audit_calls = Arc::new(Mutex::new(Vec::new()));

        let (_dir, database) = create_test_database();
        let state = Arc::new(AppState {
            config: config_arc,
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: audit_calls.clone(),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            database,
            web_dir: "target/web".to_string(),
        });

        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer test-vkey")
            .body(Body::from("{}"))
            .unwrap();

        let _ = handle_openai(State(state), req).await;

        let calls = audit_calls.lock().unwrap();
        assert!(!calls.is_empty());
        assert_eq!(calls[0].0, ProviderType::Openai);
        assert!(!calls[0].1); // Failed
    }

    #[tokio::test]
    async fn test_routing_logic() {
        let mut config = create_test_config();

        // Add another channel
        Arc::make_mut(&mut config.channels).push(Channel {
            name: "ch2".to_string(),
            provider_type: ProviderType::Anthropic, // Distinct provider
            base_url: "http://example.com".to_string(),
            api_key: "k2".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
            timeouts: None,
            pricing: None,
        });

        // Update router to match "gpt-4" to "ch2"
        let router = &mut Arc::make_mut(&mut config.routers)[0];
        router.rules.insert(
            0,
            crate::config::RouterRule {
                session_affinity: false,
                match_spec: crate::config::MatchSpec {
                    models: vec!["gpt-4".to_string()],
                },
                channels: vec![crate::config::TargetChannel {
                    name: "ch2".to_string(),
                    weight: 1,
                }],
                strategy: "priority".to_string(),
            },
        );

        let audit_calls = Arc::new(Mutex::new(Vec::new()));
        let config_arc = Arc::new(RwLock::new(config));

        let (_dir, database) = create_test_database();
        let state = Arc::new(AppState {
            config: config_arc,
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: audit_calls.clone(),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            database,
            web_dir: "target/web".to_string(),
        });

        // Request with model "gpt-4" -> should go to ch2 (Anthropic)
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer test-vkey")
            .body(Body::from(r#"{"model": "gpt-4"}"#))
            .unwrap();

        let _ = handle_openai(State(state.clone()), req).await;

        let calls = audit_calls.lock().unwrap();
        assert!(!calls.is_empty(), "should have made a call");
        // The last call should be Anthropic because that's ch2's provider type
        assert_eq!(calls.last().unwrap().0, ProviderType::Anthropic);
    }

    #[tokio::test]
    async fn test_team_flow() {
        let mut config = create_test_config();

        // Add a team
        Arc::make_mut(&mut config.teams).push(crate::config::Team {
            id: "test-team".to_string(),
            api_key: "sk-ap-test".to_string(),
            policy: crate::config::TeamPolicy {
                allowed_routers: vec!["test-router".to_string()],
                allowed_models: Some(vec!["gpt-4".to_string()]),
                rate_limit: None,
            },
            group: None,
            enabled: None,
        });

        let config_arc = Arc::new(RwLock::new(config));

        let (_dir, database) = create_test_database();
        let state = Arc::new(AppState {
            config: config_arc,
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: Arc::new(Mutex::new(Vec::new())),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            database,
            web_dir: "target/web".to_string(),
        });

        // 1. Valid Request (Correct Key, Allowed Model)
        // Note: In unit test we manually inject TeamContext because middleware is bypassed
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer sk-ap-test")
            .extension(TeamContext {
                team_id: "test-team".to_string(),
            })
            .body(Body::from(r#"{"model": "gpt-4"}"#))
            .unwrap();

        let resp = handle_openai(State(state.clone()), req).await;
        // Should pass auth/policy checks and fail at upstream (BAD_GATEWAY) or "no channels"
        // Since "test-router" matches "*", it should find a channel.
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

        // 2. Invalid Model (Not in allowed_models)
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header("Authorization", "Bearer sk-ap-test")
            .extension(TeamContext {
                team_id: "test-team".to_string(),
            })
            .body(Body::from(r#"{"model": "gpt-3.5"}"#))
            .unwrap();

        let resp = handle_openai(State(state.clone()), req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // ----- /v1/models -------------------------------------------------

    fn build_models_state(
        rules: Vec<crate::config::RouterRule>,
        allowed_models: Option<Vec<String>>,
    ) -> (Arc<AppState>, TempDir) {
        let mut config = create_test_config();

        Arc::make_mut(&mut config.routers).clear();
        Arc::make_mut(&mut config.routers).push(crate::config::Router {
            name: "test-router".to_string(),
            rules,
            channels: vec![],
            strategy: "round_robin".to_string(),
            metadata: None,
            fallback_channels: vec![],
        });

        Arc::make_mut(&mut config.teams).push(crate::config::Team {
            id: "test-team".to_string(),
            api_key: "sk-ap-test".to_string(),
            policy: crate::config::TeamPolicy {
                allowed_routers: vec!["test-router".to_string()],
                allowed_models,
                rate_limit: None,
            },
            group: None,
            enabled: None,
        });

        let (dir, database) = create_test_database();
        let state = Arc::new(AppState {
            config: Arc::new(RwLock::new(config)),
            metrics: Arc::new(MetricsState::new().unwrap()),
            providers: Arc::new(ProviderRegistry::new()),
            access_audit: Arc::new(MockAccessAudit {
                calls: Arc::new(Mutex::new(Vec::new())),
            }),
            rate_limiter: Arc::new(MockRateLimiter { allow: true }),
            team_rate_limiter: Arc::new(TeamRateLimiter::new()),
            selector: Arc::new(RouterSelector::new()),
            gemini_replay: Arc::new(GeminiAnthropicReplayCache::new()),
            client: reqwest::Client::new(),
            usage_logger: Arc::new(UsageLogger::new(database.clone())),
            database,
            web_dir: "target/web".to_string(),
        });
        (state, dir)
    }

    async fn fetch_models(
        state: Arc<AppState>,
        team_ctx: Option<TeamContext>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method("GET").uri("/v1/models");
        if let Some(ctx) = team_ctx {
            builder = builder.extension(ctx);
        }
        let req = builder.body(Body::empty()).unwrap();
        let resp = handle_models(State(state), req).await;
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, body)
    }

    fn rule_matching(models: &[&str]) -> crate::config::RouterRule {
        crate::config::RouterRule {
            session_affinity: false,
            match_spec: crate::config::MatchSpec {
                models: models.iter().map(|s| s.to_string()).collect(),
            },
            channels: vec![crate::config::TargetChannel {
                name: "test-channel".to_string(),
                weight: 1,
            }],
            strategy: "round_robin".to_string(),
        }
    }

    fn ids_in(body: &serde_json::Value) -> Vec<String> {
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[tokio::test]
    async fn handle_models_requires_team_context() {
        let (state, _dir) = build_models_state(vec![rule_matching(&["*"])], None);
        let (status, _) = fetch_models(state, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn handle_models_lists_literal_models_from_router_rules() {
        let (state, _dir) = build_models_state(
            vec![rule_matching(&["deepseek-v4-pro", "deepseek-v4-flash"])],
            None,
        );
        let (status, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let ids = ids_in(&body);
        assert!(ids.contains(&"deepseek-v4-pro".to_string()));
        assert!(ids.contains(&"deepseek-v4-flash".to_string()));
    }

    #[tokio::test]
    async fn handle_models_skips_pure_glob_patterns() {
        let (state, _dir) = build_models_state(
            vec![rule_matching(&["*", "deepseek-*", "claude-3-haiku"])],
            None,
        );
        let (_, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        let ids = ids_in(&body);
        assert!(ids.contains(&"claude-3-haiku".to_string()));
        // glob patterns must never leak into the OpenAI list payload
        assert!(!ids.iter().any(|id| id.contains('*')));
        assert!(!ids.iter().any(|id| id.contains('?')));
    }

    #[tokio::test]
    async fn handle_models_includes_history_distinct_models() {
        let (state, _dir) = build_models_state(vec![rule_matching(&["*"])], None);
        state.database.log_usage(
            Some("req-1"),
            "test-team",
            "test-router",
            Some("*"),
            "test-channel",
            "gpt-4o-mini", // observed in traffic
            10,
            5,
            Some(120.0),
            false,
            "success",
            Some(200),
            None,
            None,
            None,
            None,
            None,
            0,
            0,
            None,
            None,
        );

        let (status, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let ids = ids_in(&body);
        // log_usage lower-cases the model name; the canonical OpenAI id is
        // whatever the gateway has actually seen, so that's what we return.
        assert!(ids.contains(&"gpt-4o-mini".to_string()));
    }

    #[tokio::test]
    async fn handle_models_respects_team_allowed_models() {
        let (state, _dir) = build_models_state(
            vec![rule_matching(&["gpt-4", "gpt-4o", "claude-3-haiku"])],
            Some(vec!["gpt-4*".to_string()]),
        );

        let (_, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        let ids = ids_in(&body);
        assert!(ids.contains(&"gpt-4".to_string()));
        assert!(ids.contains(&"gpt-4o".to_string()));
        // claude-3-haiku exists in the router rule but is filtered out by
        // the team's allowed_models glob.
        assert!(!ids.contains(&"claude-3-haiku".to_string()));
    }

    #[tokio::test]
    async fn handle_models_marks_owned_by_with_provider_type() {
        let (state, _dir) = build_models_state(vec![rule_matching(&["gpt-4"])], None);
        let (_, body) = fetch_models(
            state,
            Some(TeamContext {
                team_id: "test-team".to_string(),
            }),
        )
        .await;
        let entry = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == "gpt-4")
            .unwrap();
        assert_eq!(entry["owned_by"], "openai"); // test-channel's provider_type
        assert_eq!(entry["apex"]["router"], "test-router");
        assert_eq!(entry["apex"]["channel"], "test-channel");
    }
}
