//! The shared request pipeline: channel selection, retries and fallback,
//! protocol translation, streaming or buffered relay, and usage accounting.
//!
//! `process_request` is long and stays that way in this change — it is moved
//! here verbatim. Decomposing it is a separate piece of work with its own
//! risk budget, since it owns the retry/fallback ladder and the usage
//! bookkeeping that hangs off every exit path.

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, Response, StatusCode};

use crate::converters::convert_openai_response_to_anthropic;
use crate::gemini_compat::gemini_replay_missing_signature;
use crate::middleware::auth::TeamContext;
use crate::middleware::compliance::OriginalModelName;
use crate::providers::{RouteKind, prepare_request};

use super::auth::enforce_global_auth;
use super::errors::{format_error_chain, protocol_error_response};
use super::gemini_route::gemini_native_resource_router_is_deterministic;
use super::request_utils::*;
use super::{AppState, MAX_REQUEST_BODY_BYTES};

/// Which router handles this request: an explicit override, else the first of
/// the team's allowed routers that can serve the model, else — under global
/// auth — any router that matches it.
#[allow(clippy::too_many_arguments)]
/// The per-request facts every usage row is attributed to, fixed once routing
/// has been decided.
///
/// These seven values were passed identically to all eight `log_failure` call
/// sites in this module — carrying them once keeps each failure exit to the
/// handful of arguments that actually differ.
struct Attribution {
    request_id: Option<String>,
    team_id: String,
    router_name: String,
    matched_rule: Option<String>,
    model: String,
    route_label: &'static str,
    client_info: crate::utils::ClientInfo,
    session_key: Option<String>,
}

impl Attribution {
    /// Record a request that failed, without deciding how to answer the client.
    #[allow(clippy::too_many_arguments)]
    fn log_failure(
        &self,
        state: &AppState,
        channel: &str,
        latency_ms: Option<f64>,
        fallback_triggered: bool,
        status: StatusCode,
        message: &str,
        provider_trace_id: Option<&str>,
        provider_error_body: Option<&str>,
    ) {
        state.usage_logger.log_failure(
            self.request_id.as_deref(),
            &self.team_id,
            &self.router_name,
            self.matched_rule.as_deref(),
            channel,
            &self.model,
            latency_ms,
            fallback_triggered,
            status.as_u16() as i64,
            message,
            provider_trace_id,
            provider_error_body,
            &self.client_info,
            self.session_key.as_deref(),
        );
    }

    /// Refuse a request before it reaches an upstream: audit the attempt, count
    /// it, write the usage row, and produce the response to return.
    fn reject(
        &self,
        state: &AppState,
        route: RouteKind,
        channel: &crate::config::Channel,
        fallback_triggered: bool,
        status: StatusCode,
        message: &str,
    ) -> Response<Body> {
        state
            .access_audit
            .audit(&channel.provider_type, route, false);
        state
            .metrics
            .error_total
            .with_label_values(&[self.route_label, &self.router_name])
            .inc();
        state
            .database
            .log_error(self.route_label, &self.router_name);
        self.log_failure(
            state,
            &channel.name,
            None,
            fallback_triggered,
            status,
            message,
            None,
            None,
        );
        protocol_error_response(route, status, message)
    }
}

fn resolve_router_name(
    state: &AppState,
    config: &crate::config::Config,
    extensions: &axum::http::Extensions,
    headers: &HeaderMap,
    route: RouteKind,
    model_name_str: &str,
    router_name_override: Option<String>,
) -> Result<String, Response<Body>> {
    let router_name = if let Some(name) = router_name_override {
        name
    } else if let Some(ctx) = extensions.get::<TeamContext>() {
        // Team Flow
        let team = config.teams.iter().find(|t| t.id == ctx.team_id);
        if team.is_none() {
            return Err(protocol_error_response(
                route,
                StatusCode::UNAUTHORIZED,
                "Team not found",
            ));
        }
        let team = team.unwrap();

        // Check Allowed Models
        let policy = &team.policy;
        if !policy.is_model_allowed(model_name_str) {
            tracing::warn!(
                "Policy Failed: Model '{}' not allowed by team policy",
                model_name_str
            );
            return Err(protocol_error_response(
                route,
                StatusCode::FORBIDDEN,
                "Model not allowed by team policy",
            ));
        }

        // Check Allowed Routers (Mandatory)
        let allowed_routers = &policy.allowed_routers;
        if allowed_routers.is_empty() {
            tracing::warn!(
                "Policy Failed: No allowed routers configured for team '{}'",
                ctx.team_id
            );
            return Err(protocol_error_response(
                route,
                StatusCode::FORBIDDEN,
                "No allowed routers configured for team",
            ));
        }

        let mut selected_router = None;
        for r_name in allowed_routers {
            if config
                .routers
                .iter()
                .find(|r| r.name == *r_name)
                .filter(|router| {
                    state
                        .selector
                        .select_channel(router, model_name_str)
                        .is_some()
                })
                .is_some()
            {
                selected_router = Some(r_name.clone());
                break;
            }
        }

        match selected_router {
            Some(name) => name,
            None => {
                tracing::warn!(
                    "Router Resolution Failed: No matching router found for model '{}' in allowed routers",
                    model_name_str
                );
                return Err(protocol_error_response(
                    route,
                    StatusCode::NOT_FOUND,
                    "No matching router found for model in allowed routers",
                ));
            }
        }
    } else {
        // Global Auth Flow (Legacy/Admin)
        if let Err(resp) = enforce_global_auth(config, headers) {
            return Err(if matches!(route, RouteKind::GeminiNative) {
                protocol_error_response(route, StatusCode::UNAUTHORIZED, "unauthorized")
            } else {
                resp
            });
        }

        // Try to find ANY router that handles the model
        let mut selected_router = None;
        for router in config.routers.iter() {
            if state
                .selector
                .select_channel(router, model_name_str)
                .is_some()
            {
                selected_router = Some(router.name.clone());
                break;
            }
        }

        match selected_router {
            Some(name) => name,
            None => {
                tracing::warn!(
                    "Router Resolution Failed: No matching router found for model '{}'",
                    model_name_str
                );
                return Err(protocol_error_response(
                    route,
                    StatusCode::BAD_REQUEST,
                    "No matching router found for model",
                ));
            }
        }
    };

    Ok(router_name)
}

/// The ordered candidate channels for a request: the matched rule's list
/// (primary first, then in-rule failovers), or the router's `fallback_channels`
/// when no rule matched. Returns the matched rule name alongside, for usage
/// attribution. An empty vec means nothing could be resolved — the caller
/// decides how to report that.
fn resolve_candidate_channels<'c>(
    state: &AppState,
    config: &'c crate::config::Config,
    router: &crate::config::Router,
    model_name_str: &str,
    session_key: Option<&str>,
) -> (Vec<&'c crate::config::Channel>, Option<String>) {
    // 3. Resolve Channels
    //
    // The matched rule yields an *ordered* candidate list (primary first, then
    // in-rule failovers). The retry loop below walks it, only reaching for the
    // router's `fallback_channels` once every candidate has failed. When any rule
    // opts into session affinity we derive a conversation-stable key so a
    // multi-turn conversation keeps hitting the same channel (prompt-cache
    // alignment); the key is computed only when needed.
    let mut channels: Vec<&crate::config::Channel> = Vec::new();
    let sticky_key = if router.rules.iter().any(|rule| rule.session_affinity) {
        session_key
    } else {
        None
    };
    let candidates = state
        .selector
        .select_candidates(router, model_name_str, sticky_key);
    let mut matched_rule = candidates
        .as_ref()
        .and_then(|selection| selection.matched_rule.clone());

    if let Some(selection) = candidates.as_ref() {
        for name in &selection.channels {
            if let Some(ch) = config.channels.iter().find(|c| c.name == *name) {
                if !channels.iter().any(|c| c.name == ch.name) {
                    channels.push(ch);
                }
            } else {
                tracing::warn!("Rule channel not found: {}", name);
            }
        }
        tracing::info!(
            "Channels Resolved: [{}] (model={}, matched_rule={}, sticky={})",
            selection.channels.join(", "),
            model_name_str,
            selection.matched_rule.as_deref().unwrap_or("n/a"),
            sticky_key.is_some()
        );
    }

    if channels.is_empty() {
        // No rule matched (or none of its channels exist) — resolve directly to
        // the router's fallback channels.
        if matched_rule.is_none() {
            matched_rule = Some("fallback".to_string());
        }
        tracing::info!(
            "Fallback Triggered: No rule matched for model '{}' or its channels are missing. Trying fallback channels.",
            model_name_str
        );

        for fb_name in &router.fallback_channels {
            if let Some(channel) = config.channels.iter().find(|c| c.name == *fb_name) {
                tracing::info!("Channel Resolved (Fallback): {}", channel.name);
                if !channels.iter().any(|c| c.name == channel.name) {
                    channels.push(channel);
                }
            } else {
                tracing::warn!("Fallback channel not found: {}", fb_name);
            }
        }

        if channels.is_empty() {
            tracing::error!(
                "Channel Resolution Failed: All Channels Failed for model '{}'",
                model_name_str
            );
        }
    }

    (channels, matched_rule)
}

pub(super) async fn process_request(
    state: Arc<AppState>,
    req: Request<Body>,
    route: RouteKind,
    router_name_override: Option<String>,
    path_override: Option<String>,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let client_info = crate::utils::classify_client(&parts.headers);

    // 1. Read Body
    let bytes = match axum::body::to_bytes(body, MAX_REQUEST_BODY_BYTES).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Request Failed: Failed to read body: {}", e);
            return protocol_error_response(route, StatusCode::BAD_REQUEST, &e.to_string());
        }
    };

    // 2. Parse Model
    let model_name = parts
        .extensions
        .get::<OriginalModelName>()
        .map(|model| model.0.clone())
        .or_else(|| {
            serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|json| {
                    json.get("model")
                        .and_then(|value| value.as_str())
                        .map(|value| value.to_string())
                })
        });
    let model_name_str = model_name.as_deref().unwrap_or("default");

    // 3. Log Request with Context
    let team_context = parts.extensions.get::<TeamContext>();
    let auth_info = if let Some(_ctx) = team_context {
        format!("[Model: {}]", model_name_str)
    } else {
        let has_auth_header =
            parts.headers.contains_key("authorization") || parts.headers.contains_key("x-api-key");

        if has_auth_header {
            format!(
                "[Auth: Global (or Invalid Team Key), Model: {}]",
                model_name_str
            )
        } else {
            format!("[Auth: None, Model: {}]", model_name_str)
        }
    };

    tracing::info!(
        "Request Received: {} {} {}",
        parts.method,
        parts.uri,
        auth_info
    );

    let request_id = request_id_from_parts(&parts);
    let headers = parts.headers;

    // Extract team_id for usage logging
    let team_id = parts
        .extensions
        .get::<crate::middleware::auth::TeamContext>()
        .map(|ctx| ctx.team_id.clone())
        .unwrap_or_else(|| "global".to_string());
    let config = state.config.read().unwrap().clone();

    // Fingerprint the request payload for repeat/abuse detection — hash only,
    // never prompt text (see docs/design/behavior-profiling.md §3.2). Gated by
    // config; `None` ⇒ `usage_records.req_hash` stays NULL and detection skips it.
    let req_hash = config
        .profiling
        .as_ref()
        .filter(|profiling| profiling.enabled && profiling.hash_requests)
        .and_then(|_| crate::request_hash::request_hash(&bytes));

    // Conversation fingerprint — the stable prefix (`system` + first message)
    // that identifies a multi-turn conversation. Computed for every request so
    // usage rows can be grouped by session regardless of routing config; session
    // affinity reuses this same value when a matched rule opts in (see below).
    let session_key = crate::request_hash::session_key(&bytes);

    let router_name = match resolve_router_name(
        &state,
        &config,
        &parts.extensions,
        &headers,
        route,
        model_name_str,
        router_name_override,
    ) {
        Ok(name) => name,
        Err(resp) => return resp,
    };

    let Some(router) = config.routers.iter().find(|r| r.name == router_name) else {
        return protocol_error_response(route, StatusCode::NOT_FOUND, "router not found");
    };
    if matches!(route, RouteKind::GeminiNative)
        && model_name_str == "gemini-native"
        && !gemini_native_resource_router_is_deterministic(router, model_name_str)
    {
        return protocol_error_response(
            route,
            StatusCode::BAD_REQUEST,
            "Gemini native resource routes require a priority router rule with exactly one channel",
        );
    }

    tracing::info!("Router Resolved: {}", router.name);
    tracing::Span::current().record("router_name", &router.name);

    let (mut channels, matched_rule) = resolve_candidate_channels(
        &state,
        &config,
        router,
        model_name_str,
        session_key.as_deref(),
    );

    let route_label = match route {
        RouteKind::Openai => "openai",
        RouteKind::Anthropic => "anthropic",
        RouteKind::GeminiNative => "gemini_native",
    };
    let attribution = Attribution {
        request_id: request_id.clone(),
        team_id: team_id.clone(),
        router_name: router_name.clone(),
        matched_rule: matched_rule.clone(),
        model: model_name_str.to_string(),
        route_label,
        client_info: client_info.clone(),
        session_key: session_key.clone(),
    };

    if channels.is_empty() {
        tracing::warn!(
            "Channel Resolution Failed: No channels configured or matched for router: {}",
            router_name
        );
        attribution.log_failure(
            &state,
            "unresolved",
            None,
            false,
            StatusCode::BAD_GATEWAY,
            "no channels configured or matched",
            None,
            None,
        );
        return protocol_error_response(
            route,
            StatusCode::BAD_GATEWAY,
            "no channels configured or matched",
        );
    }

    state
        .metrics
        .request_total
        .with_label_values(&[route_label, &router_name])
        .inc();

    // Log request to database
    state.database.log_request(route_label, &router_name);

    // 4. Loop channels
    let retry_on = &config.global.retries.retry_on_status;

    // Extract path and query for preparation
    let path = path_override.unwrap_or_else(|| parts.uri.path().to_string());
    let query = parts.uri.query().map(|s| s.to_string());
    let is_gemini_native_upload = matches!(route, RouteKind::GeminiNative)
        && (path.contains(":uploadToFileSearchStore") || path.starts_with("/gemini/upload/"));
    let max_attempts = if is_gemini_native_upload {
        1
    } else {
        config.global.retries.max_attempts.max(1)
    };

    let mut index = 0;
    let mut fallback_triggered = false;

    while index < channels.len() {
        let channel = channels[index];
        tracing::Span::current().record("channel_name", &channel.name);

        if matches!(route, RouteKind::GeminiNative)
            && channel.provider_type != crate::config::ProviderType::Gemini
        {
            let message = format!(
                "Gemini native route resolved to non-Gemini channel '{}'",
                channel.name
            );
            tracing::warn!("Request Rejected: {}", message);
            return attribution.reject(
                &state,
                route,
                channel,
                fallback_triggered,
                StatusCode::BAD_GATEWAY,
                &message,
            );
        }

        if index > 0 {
            tracing::warn!(
                "Fallback Triggered: Switching to fallback channel: {}",
                channel.name
            );
            state
                .metrics
                .fallback_total
                .with_label_values(&[&router_name, &channel.name])
                .inc();

            // Log fallback to database
            state.database.log_fallback(&router_name, &channel.name);
        }

        if !state.rate_limiter.check(&channel.provider_type) {
            tracing::warn!("Rate Limit Exceeded: Provider {:?}", channel.provider_type);
            index += 1;
            continue;
        }

        let effective_bytes = if channel.provider_type == crate::config::ProviderType::Gemini
            && matches!(route, RouteKind::Anthropic)
        {
            state.gemini_replay.augment_request(&team_id, &bytes)
        } else {
            bytes.clone()
        };

        let effective_model = serde_json::from_slice::<serde_json::Value>(&effective_bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("model")
                    .and_then(|model| model.as_str())
                    .map(str::to_string)
            })
            .and_then(|model| {
                channel
                    .model_map
                    .as_ref()
                    .and_then(|map| map.get(&model))
                    .cloned()
                    .or(Some(model))
            });

        if channel.provider_type == crate::config::ProviderType::Gemini
            && matches!(route, RouteKind::Anthropic)
            && effective_model
                .as_deref()
                .is_some_and(|model| model.to_ascii_lowercase().starts_with("gemini-3"))
            && anthropic_request_contains_tool_result(&effective_bytes)
            && gemini_replay_missing_signature(&effective_bytes)
        {
            let reason = format!(
                "Gemini model '{}' requires Google thought_signature data on tool-result follow-up turns. Claude Code did not preserve the prior Gemini tool call state, and Apex could not reconstruct it from cache.",
                effective_model.as_deref().unwrap_or(model_name_str)
            );
            let request_summary = summarize_anthropic_request(&effective_bytes);
            tracing::warn!("Gemini replay rejection summary: {}", request_summary);
            tracing::warn!("Request Rejected: {}", reason);
            return attribution.reject(
                &state,
                route,
                channel,
                fallback_triggered,
                StatusCode::BAD_REQUEST,
                &reason,
            );
        }

        for attempt in 0..max_attempts {
            let prepared = match prepare_request(
                &state.providers,
                channel,
                route,
                &channel.base_url,
                &path,
                query.as_deref(),
                &headers,
                &effective_bytes,
            ) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Upstream Request Build Failed: {}", e);
                    return protocol_error_response(route, StatusCode::BAD_REQUEST, &e.to_string());
                }
            };

            let adapter = state.providers.adapter_for(channel, route);

            let start = std::time::Instant::now();

            let req_future = state
                .client
                .request(parts.method.clone(), prepared.url)
                .headers(prepared.headers)
                .body(prepared.body)
                .build();

            let req_built = match req_future {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Upstream Request Build Failed: {}", e);
                    return protocol_error_response(
                        route,
                        StatusCode::INTERNAL_SERVER_ERROR,
                        &e.to_string(),
                    );
                }
            };

            tracing::info!(
                "Upstream Request: method={} url={} attempt={}/{}",
                req_built.method(),
                req_built.url(),
                attempt + 1,
                max_attempts
            );

            let resp_result = state.client.execute(req_built).await;

            match resp_result {
                Ok(resp) => {
                    let elapsed = start.elapsed().as_millis() as f64;

                    state
                        .metrics
                        .upstream_latency_ms
                        .with_label_values(&[route_label, &router_name, &channel.name])
                        .observe(elapsed);

                    // Log latency to database
                    state
                        .database
                        .log_latency(route_label, &router_name, &channel.name, elapsed);

                    let status = resp.status();
                    if status.is_success() {
                        tracing::info!("Upstream Success: {} ({}ms)", status, elapsed);
                        let mut response = adapter.handle_response(
                            route,
                            resp,
                            response_timeouts_for(&config.global.timeouts, channel),
                        );
                        if channel.provider_type == crate::config::ProviderType::Gemini
                            && matches!(route, RouteKind::Anthropic)
                        {
                            response = state
                                .gemini_replay
                                .clone()
                                .wrap_response(team_id.clone(), effective_bytes.clone(), response)
                                .await;
                        }
                        let wrapped = crate::usage::wrap_response(
                            response,
                            route,
                            request_id.clone(),
                            team_id.clone(),
                            router_name.clone(),
                            matched_rule.clone(),
                            channel.name.clone(),
                            model_name_str.to_string(),
                            state.usage_logger.clone(),
                            state.metrics.clone(),
                            Some(elapsed),
                            fallback_triggered,
                            client_info.clone(),
                            req_hash.clone(),
                            session_key.clone(),
                        )
                        .await;
                        if crate::usage::is_upstream_body_error_response(&wrapped) {
                            tracing::warn!(
                                "Upstream response body failed: channel={} attempt={}/{}",
                                channel.name,
                                attempt + 1,
                                max_attempts
                            );
                            state
                                .access_audit
                                .audit(&channel.provider_type, route, false);
                            if attempt + 1 < max_attempts
                                && crate::usage::is_upstream_body_error_retryable(&wrapped)
                            {
                                tokio::time::sleep(Duration::from_millis(
                                    config.global.retries.backoff_ms,
                                ))
                                .await;
                                continue;
                            }
                            if index == channels.len() - 1
                                && !fallback_triggered
                                && !router.fallback_channels.is_empty()
                            {
                                tracing::warn!(
                                    "Upstream body failed: Channel '{}' failed, trying fallback...",
                                    channel.name
                                );
                                fallback_triggered = true;
                                for fb_name in &router.fallback_channels {
                                    if let Some(fb_ch) =
                                        config.channels.iter().find(|c| c.name == *fb_name).filter(
                                            |fb_ch| !channels.iter().any(|c| c.name == fb_ch.name),
                                        )
                                    {
                                        channels.push(fb_ch);
                                    }
                                }
                                break;
                            }
                            if index == channels.len() - 1 {
                                return wrapped;
                            }
                            break;
                        }
                        state
                            .access_audit
                            .audit(&channel.provider_type, route, true);
                        return wrapped;
                    }

                    tracing::warn!("Upstream Failed: {} ({}ms)", status, elapsed);
                    state
                        .access_audit
                        .audit(&channel.provider_type, route, false);

                    // Check if retryable
                    if attempt + 1 < max_attempts {
                        // Check retry on status
                        let status_code = status.as_u16();
                        // Assuming retry_on is Vec<u16>
                        if retry_on.contains(&status_code) {
                            tracing::warn!(
                                "Retry Triggered: attempt {}/{} due to status {}",
                                attempt + 1,
                                max_attempts,
                                status_code
                            );
                            tokio::time::sleep(Duration::from_millis(
                                config.global.retries.backoff_ms,
                            ))
                            .await;
                            continue;
                        }
                    }

                    // If last channel and last attempt, return error
                    if index == channels.len() - 1 && attempt == max_attempts - 1 {
                        // Check if we can trigger fallback
                        if !fallback_triggered && !router.fallback_channels.is_empty() {
                            tracing::warn!(
                                "Upstream Failed: Channel '{}' failed, trying fallback...",
                                channel.name
                            );
                            fallback_triggered = true;
                            for fb_name in &router.fallback_channels {
                                if let Some(fb_ch) =
                                    config.channels.iter().find(|c| c.name == *fb_name).filter(
                                        |fb_ch| !channels.iter().any(|c| c.name == fb_ch.name),
                                    )
                                {
                                    channels.push(fb_ch);
                                }
                            }
                            break; // Break attempt loop, proceed to next channel
                        }

                        state
                            .metrics
                            .error_total
                            .with_label_values(&[route_label, &router_name])
                            .inc();

                        // Log error to database
                        state.database.log_error(route_label, &router_name);
                        let provider_trace_id = provider_trace_id_from_headers(resp.headers());
                        let response_headers = resp.headers().clone();
                        let error_body_bytes = resp.bytes().await.unwrap_or_default();
                        let provider_error_body = String::from_utf8_lossy(&error_body_bytes);
                        let stored_error_body = truncate_for_storage(&provider_error_body, 4000);
                        if !stored_error_body.is_empty() {
                            tracing::warn!("Upstream Error Body: {}", stored_error_body);
                        }
                        attribution.log_failure(
                            &state,
                            &channel.name,
                            Some(elapsed),
                            fallback_triggered,
                            status,
                            status
                                .canonical_reason()
                                .unwrap_or("upstream request failed"),
                            provider_trace_id.as_deref(),
                            Some(stored_error_body.as_str()),
                        );

                        // Convert error if needed (e.g. for Anthropic)
                        if matches!(route, RouteKind::Anthropic) {
                            let body = convert_openai_response_to_anthropic(error_body_bytes);
                            return Response::builder()
                                .status(status)
                                .body(Body::from(body))
                                .unwrap();
                        }
                        return response_from_upstream_bytes(
                            status,
                            &response_headers,
                            error_body_bytes,
                        );
                    }
                }
                Err(e) => {
                    let error_chain = format_error_chain(&e);
                    tracing::error!(
                        is_connect = e.is_connect(),
                        is_timeout = e.is_timeout(),
                        is_request = e.is_request(),
                        is_body = e.is_body(),
                        is_decode = e.is_decode(),
                        error_chain = %error_chain,
                        "Upstream Error: {}",
                        e
                    );
                    state
                        .access_audit
                        .audit(&channel.provider_type, route, false);
                    if attempt + 1 < max_attempts {
                        tracing::warn!(
                            "Retry Triggered: attempt {}/{} due to error",
                            attempt + 1,
                            max_attempts
                        );
                        tokio::time::sleep(Duration::from_millis(config.global.retries.backoff_ms))
                            .await;
                        continue;
                    }
                }
            }
        }

        // If all attempts failed (network error), check fallback
        if index == channels.len() - 1
            && !fallback_triggered
            && !router.fallback_channels.is_empty()
        {
            tracing::warn!(
                "Upstream Failed: Channel '{}' failed (network), trying fallback...",
                channel.name
            );
            fallback_triggered = true;
            for fb_name in &router.fallback_channels {
                if let Some(fb_ch) = config
                    .channels
                    .iter()
                    .find(|c| c.name == *fb_name)
                    .filter(|fb_ch| !channels.iter().any(|c| c.name == fb_ch.name))
                {
                    channels.push(fb_ch);
                }
            }
        }

        index += 1;
    }

    state
        .metrics
        .error_total
        .with_label_values(&[route_label, &router_name])
        .inc();

    // Log error to database
    state.database.log_error(route_label, &router_name);
    let last_channel = channels
        .last()
        .map(|channel| channel.name.as_str())
        .unwrap_or("unresolved");
    attribution.log_failure(
        &state,
        last_channel,
        None,
        fallback_triggered,
        StatusCode::BAD_GATEWAY,
        "all channels failed",
        None,
        None,
    );

    protocol_error_response(route, StatusCode::BAD_GATEWAY, "all channels failed")
}

pub(super) async fn process_gemini_native_direct_pass(
    state: Arc<AppState>,
    req: Request<Body>,
    routing_model: String,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let route = RouteKind::GeminiNative;
    let route_label = "gemini_native";
    let request_id = request_id_from_parts(&parts);
    let team_id = parts
        .extensions
        .get::<crate::middleware::auth::TeamContext>()
        .map(|ctx| ctx.team_id.clone())
        .unwrap_or_else(|| "global".to_string());
    let headers = parts.headers.clone();
    let client_info = crate::utils::classify_client(&headers);
    let config = state.config.read().unwrap().clone();

    let router_name = if let Some(ctx) = parts.extensions.get::<TeamContext>() {
        let Some(team) = config.teams.iter().find(|team| team.id == ctx.team_id) else {
            return protocol_error_response(route, StatusCode::UNAUTHORIZED, "Team not found");
        };

        if !team.policy.is_model_allowed(&routing_model) {
            return protocol_error_response(
                route,
                StatusCode::FORBIDDEN,
                "Model not allowed by team policy",
            );
        }

        team.policy
            .allowed_routers
            .iter()
            .find(|router_name| {
                config
                    .routers
                    .iter()
                    .find(|router| router.name == **router_name)
                    .and_then(|router| state.selector.select_channel(router, &routing_model))
                    .is_some()
            })
            .cloned()
    } else {
        if let Err(_resp) = enforce_global_auth(&config, &headers) {
            return protocol_error_response(route, StatusCode::UNAUTHORIZED, "unauthorized");
        }
        config
            .routers
            .iter()
            .find(|router| {
                state
                    .selector
                    .select_channel(router, &routing_model)
                    .is_some()
            })
            .map(|router| router.name.clone())
    };

    let Some(router_name) = router_name else {
        return protocol_error_response(
            route,
            StatusCode::NOT_FOUND,
            "No matching router found for model",
        );
    };
    let Some(router) = config
        .routers
        .iter()
        .find(|router| router.name == router_name)
    else {
        return protocol_error_response(route, StatusCode::NOT_FOUND, "router not found");
    };
    if !gemini_native_resource_router_is_deterministic(router, &routing_model) {
        return protocol_error_response(
            route,
            StatusCode::BAD_REQUEST,
            "Gemini native resource routes require a priority router rule with exactly one channel",
        );
    }
    let Some(selection) = state
        .selector
        .select_channel_with_rule(router, &routing_model)
    else {
        return protocol_error_response(
            route,
            StatusCode::BAD_GATEWAY,
            "no channels configured or matched",
        );
    };
    let matched_rule = selection.matched_rule.clone();
    let attribution = Attribution {
        request_id: request_id.clone(),
        team_id: team_id.clone(),
        router_name: router_name.clone(),
        matched_rule: matched_rule.clone(),
        model: routing_model.clone(),
        route_label,
        client_info: client_info.clone(),
        session_key: None,
    };

    let Some(channel) = config
        .channels
        .iter()
        .find(|channel| channel.name == selection.channel_name)
    else {
        return protocol_error_response(
            route,
            StatusCode::BAD_GATEWAY,
            "no channels configured or matched",
        );
    };

    if channel.provider_type != crate::config::ProviderType::Gemini {
        let message = format!(
            "Gemini native route resolved to non-Gemini channel '{}'",
            channel.name
        );
        attribution.log_failure(
            &state,
            &channel.name,
            None,
            false,
            StatusCode::BAD_GATEWAY,
            &message,
            None,
            None,
        );
        return protocol_error_response(route, StatusCode::BAD_GATEWAY, &message);
    }

    state
        .metrics
        .request_total
        .with_label_values(&[route_label, &router_name])
        .inc();
    state.database.log_request(route_label, &router_name);

    if !state.rate_limiter.check(&channel.provider_type) {
        return protocol_error_response(route, StatusCode::TOO_MANY_REQUESTS, "rate limited");
    }

    let path = parts.uri.path().to_string();
    let query = parts.uri.query().map(|value| value.to_string());
    let prepared = match crate::providers::prepare_gemini_native_request(
        channel,
        &channel.base_url,
        &path,
        query.as_deref(),
        &headers,
        &Bytes::new(),
    ) {
        Ok(prepared) => prepared,
        Err(err) => {
            return protocol_error_response(route, StatusCode::BAD_REQUEST, &err.to_string());
        }
    };

    let start = std::time::Instant::now();
    let reqwest_body = reqwest::Body::wrap_stream(body.into_data_stream());
    let req_built = match state
        .client
        .request(parts.method.clone(), prepared.url)
        .headers(prepared.headers)
        .body(reqwest_body)
        .build()
    {
        Ok(request) => request,
        Err(err) => {
            return protocol_error_response(
                route,
                StatusCode::INTERNAL_SERVER_ERROR,
                &err.to_string(),
            );
        }
    };

    let resp = match state.client.execute(req_built).await {
        Ok(resp) => resp,
        Err(err) => {
            let message = format_error_chain(&err);
            attribution.log_failure(
                &state,
                &channel.name,
                None,
                false,
                StatusCode::BAD_GATEWAY,
                &message,
                None,
                None,
            );
            return protocol_error_response(route, StatusCode::BAD_GATEWAY, &message);
        }
    };

    let elapsed = start.elapsed().as_millis() as f64;
    state
        .metrics
        .upstream_latency_ms
        .with_label_values(&[route_label, &router_name, &channel.name])
        .observe(elapsed);
    state
        .database
        .log_latency(route_label, &router_name, &channel.name, elapsed);

    let status = resp.status();
    if !status.is_success() {
        state.database.log_error(route_label, &router_name);
        let provider_trace_id = provider_trace_id_from_headers(resp.headers());
        let response_headers = resp.headers().clone();
        let error_body_bytes = resp.bytes().await.unwrap_or_default();
        let stored_error_body =
            truncate_for_storage(&String::from_utf8_lossy(&error_body_bytes), 4000);
        attribution.log_failure(
            &state,
            &channel.name,
            Some(elapsed),
            false,
            status,
            status
                .canonical_reason()
                .unwrap_or("upstream request failed"),
            provider_trace_id.as_deref(),
            Some(stored_error_body.as_str()),
        );
        return response_from_upstream_bytes(status, &response_headers, error_body_bytes);
    }

    let adapter = state.providers.adapter_for(channel, route);
    let response = adapter.handle_response(
        route,
        resp,
        response_timeouts_for(&config.global.timeouts, channel),
    );
    let wrapped = crate::usage::wrap_response(
        response,
        route,
        request_id,
        team_id,
        router_name,
        matched_rule,
        channel.name.clone(),
        routing_model,
        state.usage_logger.clone(),
        state.metrics.clone(),
        Some(elapsed),
        false,
        client_info.clone(),
        // Native-Gemini direct-pass streams the request body without buffering
        // it, so there is nothing to fingerprint here; these rows get NULL for
        // both the request fingerprint and the conversation/session key.
        None,
        None,
    )
    .await;
    state.access_audit.audit(
        &channel.provider_type,
        route,
        !crate::usage::is_upstream_body_error_response(&wrapped),
    );
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MatchSpec, RouterRule, TargetChannel};
    use crate::server::test_fixtures::*;
    use serde_json::json;

    // Channel resolution had no direct coverage while it lived inside
    // process_request — reaching it meant driving a whole HTTP request.

    fn router_with(rules: Vec<RouterRule>, fallback: Vec<&str>) -> crate::config::Router {
        crate::config::Router {
            name: "r1".to_string(),
            rules,
            channels: vec![],
            strategy: "priority".to_string(),
            metadata: None,
            fallback_channels: fallback.into_iter().map(String::from).collect(),
        }
    }

    fn rule_for(models: &[&str], channels: &[&str]) -> RouterRule {
        RouterRule {
            session_affinity: false,
            match_spec: MatchSpec {
                models: models.iter().map(|m| m.to_string()).collect(),
            },
            channels: channels
                .iter()
                .map(|c| TargetChannel {
                    name: c.to_string(),
                    weight: 1,
                })
                .collect(),
            strategy: "priority".to_string(),
        }
    }

    #[test]
    fn candidate_channels_come_from_the_matched_rule() {
        let config = create_test_config();
        let names: Vec<String> = config.channels.iter().map(|c| c.name.clone()).collect();
        let first = names.first().expect("fixture has a channel").clone();
        let router = router_with(vec![rule_for(&["*"], &[&first])], vec![]);
        let (state, _db) = state_with_config(config.clone());

        let (channels, matched) =
            resolve_candidate_channels(&state, &config, &router, "gpt-4", None);

        assert_eq!(
            channels.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec![first.as_str()]
        );
        assert!(matched.is_some(), "a matched rule must be attributed");
    }

    #[test]
    fn unmatched_model_falls_back_to_the_routers_fallback_channels() {
        let config = create_test_config();
        let first = config.channels[0].name.clone();
        // A rule that cannot match, plus a usable fallback.
        let router = router_with(vec![rule_for(&["claude-*"], &[&first])], vec![&first]);
        let (state, _db) = state_with_config(config.clone());

        let (channels, matched) =
            resolve_candidate_channels(&state, &config, &router, "gpt-4", None);

        assert_eq!(
            channels.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec![first.as_str()]
        );
        assert_eq!(
            matched.as_deref(),
            Some("fallback"),
            "usage must be attributed to the fallback path, not a rule"
        );
    }

    #[test]
    fn nothing_resolvable_yields_no_candidates() {
        let config = create_test_config();
        // Rule points at a channel that does not exist, and no fallback.
        let router = router_with(vec![rule_for(&["*"], &["ghost"])], vec!["also-ghost"]);
        let (state, _db) = state_with_config(config.clone());

        let (channels, _matched) =
            resolve_candidate_channels(&state, &config, &router, "gpt-4", None);

        assert!(
            channels.is_empty(),
            "missing channels must not be invented; the caller reports the failure"
        );
    }

    #[test]
    fn gemini_missing_signature_guard_triggers_only_for_tool_result_followups() {
        let followup = Bytes::from(
            serde_json::to_vec(&json!({
                "messages": [
                    {
                        "role": "assistant",
                        "content": [
                            {"type": "tool_use", "id": "toolu_1", "name": "run_command", "input": {"cmd": "pwd"}}
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            {"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"}
                        ]
                    }
                ]
            }))
            .unwrap(),
        );
        assert!(gemini_replay_missing_signature(&followup));

        let first_turn = Bytes::from(
            serde_json::to_vec(&json!({
                "messages": [
                    {
                        "role": "user",
                        "content": [{"type": "text", "text": "hello"}]
                    }
                ]
            }))
            .unwrap(),
        );
        assert!(!gemini_replay_missing_signature(&first_turn));
    }
}
