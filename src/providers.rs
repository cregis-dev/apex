use crate::config::{Channel, ProviderType};
use crate::converters::{
    convert_anthropic_to_openai, convert_openai_response_to_anthropic,
    convert_openai_stream_to_anthropic,
};
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::Response;
use futures::future::FutureExt;
use futures::stream;
use std::collections::HashMap;
use std::io;
use std::time::Duration;
use tokio_stream::StreamExt;
use url::Url;

/// Represents the kind of route or protocol expected by the client.
#[derive(Clone, Copy, Debug)]
pub enum RouteKind {
    /// Client expects OpenAI format
    Openai,
    /// Client expects Anthropic format
    Anthropic,

}

/// Represents a request prepared for sending to the upstream provider.
pub struct PreparedRequest {
    pub url: Url,
    pub body: Bytes,
    pub headers: HeaderMap,
}

/// Trait for auditing access to providers.
pub trait AccessAudit: Send + Sync {
    /// Records an access attempt.
    fn audit(&self, provider: &ProviderType, route: RouteKind, success: bool);
}

/// A no-op implementation of AccessAudit.
pub struct NoOpAccessAudit;

impl AccessAudit for NoOpAccessAudit {
    fn audit(&self, _provider: &ProviderType, _route: RouteKind, _success: bool) {}
}

/// Trait for rate limiting.
pub trait RateLimiter: Send + Sync {
    /// Returns true if the request is allowed, false otherwise.
    fn check(&self, provider: &ProviderType) -> bool;
}

/// A no-op implementation of RateLimiter that allows all requests.
pub struct NoOpRateLimiter;

impl RateLimiter for NoOpRateLimiter {
    fn check(&self, _provider: &ProviderType) -> bool {
        true
    }
}

/// A trait for adapting different AI providers to a common interface.
///
/// Implementations of this trait handle:
/// - URL mapping
/// - Query parameter mapping
/// - Body transformation (e.g., remapping model names)
/// - Authentication headers
/// - Response handling (including format conversion)
pub trait ProviderAdapter: Send + Sync {
    /// Maps the request path based on the route kind and provider conventions.
    fn map_path(&self, route: RouteKind, base_url: &str, path: &str) -> String;
    
    /// Maps query parameters. Returns None if parameters should be stripped.
    fn map_query(&self, _route: RouteKind, query: Option<&str>) -> Option<String> {
        query.map(|s| s.to_string())
    }
    
    /// Transforms the request body.
    /// This is where model remapping or format conversion (Anthropic -> OpenAI) happens.
    fn transform_body(
        &self,
        route: RouteKind,
        body: &Bytes,
        model_map: &Option<HashMap<String, String>>,
    ) -> Bytes;
    
    /// Applies authentication headers (e.g., Bearer token, x-api-key).
    fn apply_auth_headers(&self, route: RouteKind, headers: &mut HeaderMap, api_key: &str, base_url: &str);
    
    /// Handles the upstream response.
    /// This allows adapters to inspect headers, status codes, and convert body formats.
    fn handle_response(
        &self,
        _route: RouteKind,
        resp: reqwest::Response,
        timeout: Duration,
    ) -> Response<Body> {
        convert_response(resp, timeout)
    }
}

/// Registry for all available provider adapters.
pub struct ProviderRegistry {
    adapters: HashMap<ProviderType, Box<dyn ProviderAdapter>>,
    fallback: Box<dyn ProviderAdapter>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        let mut adapters: HashMap<ProviderType, Box<dyn ProviderAdapter>> = HashMap::new();
        adapters.insert(ProviderType::Openai, Box::new(OpenAiAdapter));
        adapters.insert(ProviderType::Anthropic, Box::new(AnthropicAdapter));
        adapters.insert(ProviderType::Gemini, Box::new(GeminiAdapter));
        
        // Providers that support both protocols
        adapters.insert(ProviderType::Deepseek, Box::new(DualProtocolAdapter::new()));
        adapters.insert(ProviderType::Moonshot, Box::new(DualProtocolAdapter::new()));
        adapters.insert(ProviderType::Minimax, Box::new(DualProtocolAdapter::new()));
        adapters.insert(ProviderType::Ollama, Box::new(DefaultAdapter));
        adapters.insert(ProviderType::Jina, Box::new(DefaultAdapter));
        adapters.insert(ProviderType::Openrouter, Box::new(DefaultAdapter));
        
        Self {
            adapters,
            fallback: Box::new(DefaultAdapter),
        }
    }

    pub fn adapter(&self, channel: &Channel) -> &dyn ProviderAdapter {
        self.adapters
            .get(&channel.provider_type)
            .map(|item| item.as_ref())
            .unwrap_or(self.fallback.as_ref())
    }
}

/// Prepares a request for the specific provider.
#[allow(clippy::too_many_arguments)]
pub fn prepare_request(
    registry: &ProviderRegistry,
    channel: &Channel,
    route: RouteKind,
    base_url: &str,
    path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: &Bytes,
) -> anyhow::Result<PreparedRequest> {
    let base_url = if matches!(route, RouteKind::Anthropic) {
        channel.anthropic_base_url.as_deref().unwrap_or(base_url)
    } else {
        base_url
    };
    let adapter = registry.adapter(channel);
    let normalized_path = path.trim_start_matches('/').to_string();
    let mapped_path = adapter.map_path(route, base_url, &normalized_path);
    let mapped_query = adapter.map_query(route, query);
    let url = build_url(base_url, &mapped_path, mapped_query.as_deref())?;
    let body = adapter.transform_body(route, body, &channel.model_map);
    let mut headers = build_headers(headers, channel);
    adapter.apply_auth_headers(route, &mut headers, &channel.api_key, base_url);
    Ok(PreparedRequest { url, body, headers })
}

// --- Helper Functions ---

pub fn should_forward_response_header(name: &HeaderName) -> bool {
    let lower = name.as_str().to_ascii_lowercase();
    !matches!(lower.as_str(), "transfer-encoding" | "content-length")
}

pub fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(Body::from(message.to_string()))
        .unwrap()
}

pub fn convert_response(resp: reqwest::Response, timeout: Duration) -> Response<Body> {
    let status = resp.status();
    let mut builder = Response::builder().status(status);
    for (name, value) in resp.headers().iter() {
        if should_forward_response_header(name) {
            builder = builder.header(name, value);
        }
    }
    let stream = resp.bytes_stream().timeout(timeout);
    let stream = stream.map(|item| match item {
        Ok(Ok(bytes)) => Ok(Bytes::from(bytes)),
        Ok(Err(err)) => Err(io::Error::new(io::ErrorKind::Other, err)),
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "response timeout")),
    });
    builder
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| error_response(StatusCode::BAD_GATEWAY, "invalid response"))
}

fn build_headers(headers: &HeaderMap, channel: &Channel) -> HeaderMap {
    let mut result = HeaderMap::new();
    for (name, value) in headers.iter() {
        // Strip hop-by-hop/gateway headers to avoid leaking control headers upstream.
        if should_forward_header(name) {
            result.insert(name.clone(), value.clone());
        }
    }
    if let Some(extra_headers) = &channel.headers {
        for (key, value) in extra_headers {
            if let Ok(header_name) = HeaderName::from_bytes(key.as_bytes()) {
                if let Ok(header_value) = HeaderValue::from_str(value) {
                    result.insert(header_name, header_value);
                }
            }
        }
    }
    result
}

fn should_forward_header(name: &HeaderName) -> bool {
    let lower = name.as_str().to_ascii_lowercase();
    !matches!(
        lower.as_str(),
        "host"
            | "content-length"
            | "x-api-key"
            | "authorization"
            | "accept-encoding"
    ) && !lower.starts_with("anthropic-")
        && !lower.starts_with("x-stainless-")
}

fn build_url(base: &str, path: &str, query: Option<&str>) -> anyhow::Result<Url> {
    let base = if base.ends_with('/') {
        base.to_string()
    } else {
        format!("{}/", base)
    };
    let mut url = Url::parse(&base)?;
    
    // Deduplicate 'v1' if base ends with it and path starts with it
    let path = if base.trim_end_matches('/').ends_with("/v1") && path.starts_with("v1/") {
        &path[3..]
    } else {
        path
    };

    if path != "/" {
        url = url.join(path)?;
    }
    if let Some(query) = query {
        url.set_query(Some(query));
    }
    Ok(url)
}

fn apply_model_map(body: &Bytes, model_map: &Option<HashMap<String, String>>) -> Bytes {
    // Best-effort model remapping; keep original payload on any parse/lookup failure.
    let Some(model_map) = model_map else {
        return body.clone();
    };
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.clone();
    };
    let Some(model) = value.get("model").and_then(|m| m.as_str()) else {
        return body.clone();
    };
    let Some(mapped) = model_map.get(model) else {
        return body.clone();
    };
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "model".to_string(),
            serde_json::Value::String(mapped.clone()),
        );
        if let Ok(serialized) = serde_json::to_vec(&value) {
            return Bytes::from(serialized);
        }
    }
    body.clone()
}

fn apply_bearer_auth(headers: &mut HeaderMap, api_key: &str, header_name: &str) {
    if api_key.is_empty() {
        return;
    }
    let value = if header_name == "authorization" {
        format!("Bearer {}", api_key)
    } else {
        api_key.to_string()
    };
    if let Ok(name) = HeaderName::from_bytes(header_name.as_bytes()) {
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.insert(name, value);
        }
    }
}

// --- Adapters ---

/// Default adapter for OpenAI-compatible providers (Deepseek, Moonshot, etc.).
struct DefaultAdapter;

impl ProviderAdapter for DefaultAdapter {
    fn map_path(&self, route: RouteKind, _base_url: &str, path: &str) -> String {
        if matches!(route, RouteKind::Anthropic) {
            "chat/completions".to_string()
        } else {
            path.to_string()
        }
    }

    fn transform_body(
        &self,
        route: RouteKind,
        body: &Bytes,
        model_map: &Option<HashMap<String, String>>,
    ) -> Bytes {
        if matches!(route, RouteKind::Anthropic) {
            let body = convert_anthropic_to_openai(body);
            apply_model_map(&body, model_map)
        } else {
            apply_model_map(body, model_map)
        }
    }

    fn apply_auth_headers(&self, _route: RouteKind, headers: &mut HeaderMap, api_key: &str, _base_url: &str) {
        apply_bearer_auth(headers, api_key, "authorization");
    }

    fn handle_response(
        &self,
        route: RouteKind,
        resp: reqwest::Response,
        timeout: Duration,
    ) -> Response<Body> {
        // If client expects Anthropic format, but provider is OpenAI-compatible (DefaultAdapter),
        // we need to convert the response.
        if matches!(route, RouteKind::Anthropic) {
            let is_stream = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|v| v.contains("text/event-stream"))
                .unwrap_or(false);

            if is_stream {
                let stream = resp.bytes_stream();
                let converted_stream = convert_openai_stream_to_anthropic(stream);

                return Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .header("connection", "keep-alive")
                    .body(Body::from_stream(converted_stream))
                    .unwrap();
            }

            let status = resp.status();
            let mut builder = Response::builder().status(status);
            for (name, value) in resp.headers().iter() {
                if should_forward_response_header(name) {
                    builder = builder.header(name, value);
                }
            }

            let stream = resp.bytes_stream().timeout(timeout);
            let stream = stream.map(|item| match item {
                Ok(Ok(bytes)) => Ok(Bytes::from(bytes)),
                Ok(Err(err)) => Err(io::Error::new(io::ErrorKind::Other, err)),
                Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "response timeout")),
            });

            let future = stream
                .fold(Vec::new(), |mut acc, item| {
                    if let Ok(bytes) = item {
                        acc.extend_from_slice(&bytes);
                    }
                    acc
                })
                .map(|bytes| {
                    let b = Bytes::from(bytes);
                    let converted = convert_openai_response_to_anthropic(b);
                    Ok::<_, io::Error>(converted)
                });

            builder
                .body(Body::from_stream(stream::once(future)))
                .unwrap_or_else(|_| error_response(StatusCode::BAD_GATEWAY, "invalid response"))
        } else {
            convert_response(resp, timeout)
        }
    }
}

/// Adapter for OpenAI.
struct OpenAiAdapter;

impl ProviderAdapter for OpenAiAdapter {
    fn map_path(&self, _route: RouteKind, _base_url: &str, path: &str) -> String {
        path.to_string()
    }

    fn transform_body(
        &self,
        _route: RouteKind,
        body: &Bytes,
        model_map: &Option<HashMap<String, String>>,
    ) -> Bytes {
        apply_model_map(body, model_map)
    }

    fn apply_auth_headers(&self, _route: RouteKind, headers: &mut HeaderMap, api_key: &str, _base_url: &str) {
        apply_bearer_auth(headers, api_key, "authorization");
    }
}

/// Adapter for Anthropic.
struct AnthropicAdapter;

impl ProviderAdapter for AnthropicAdapter {
    fn map_path(&self, _route: RouteKind, _base_url: &str, path: &str) -> String {
        path.to_string()
    }

    fn transform_body(
        &self,
        _route: RouteKind,
        body: &Bytes,
        model_map: &Option<HashMap<String, String>>,
    ) -> Bytes {
        apply_model_map(body, model_map)
    }

    fn apply_auth_headers(&self, _route: RouteKind, headers: &mut HeaderMap, api_key: &str, _base_url: &str) {
        apply_bearer_auth(headers, api_key, "x-api-key");
        if !headers.contains_key("anthropic-version") {
            if let Ok(name) = HeaderName::from_bytes(b"anthropic-version") {
                if let Ok(value) = HeaderValue::from_str("2023-06-01") {
                    headers.insert(name, value);
                }
            }
        }
    }
}

/// Adapter for Google Gemini.
struct GeminiAdapter;

impl ProviderAdapter for GeminiAdapter {
    fn map_path(&self, route: RouteKind, _base_url: &str, path: &str) -> String {
        match route {
            RouteKind::Anthropic => "chat/completions".to_string(),
            _ => {
                if path.starts_with("v1/") {
                    path[3..].to_string()
                } else {
                    path.to_string()
                }
            }
        }
    }

    fn map_query(&self, route: RouteKind, query: Option<&str>) -> Option<String> {
        if matches!(route, RouteKind::Anthropic) {
            // Strip `beta=true` or similar query params that Gemini doesn't understand
            None
        } else {
            query.map(|s| s.to_string())
        }
    }

    fn transform_body(
        &self,
        route: RouteKind,
        body: &Bytes,
        model_map: &Option<HashMap<String, String>>,
    ) -> Bytes {
        if matches!(route, RouteKind::Anthropic) {
            let body = convert_anthropic_to_openai(body);
            apply_model_map(&body, model_map)
        } else {
            apply_model_map(body, model_map)
        }
    }

    fn apply_auth_headers(&self, _route: RouteKind, headers: &mut HeaderMap, api_key: &str, base_url: &str) {
        // Some Gemini endpoints/proxies might use OpenAI format if running via a bridge,
        // but native Gemini usually uses x-goog-api-key or query param.
        // Assuming here we are talking to a service that accepts header auth.
        if base_url.contains("/openai") {
            apply_bearer_auth(headers, api_key, "authorization");
        } else {
            apply_bearer_auth(headers, api_key, "x-goog-api-key");
        }
    }
}

/// Adapter that supports both OpenAI and Anthropic protocols natively.
/// 
/// It routes requests to the appropriate endpoint based on the request protocol,
/// rewriting the base URL if necessary (e.g. switching between /v1 and /anthropic).
struct DualProtocolAdapter {
    openai: OpenAiAdapter,
    anthropic: AnthropicAdapter,
}

impl DualProtocolAdapter {
    fn new() -> Self {
        Self {
            openai: OpenAiAdapter,
            anthropic: AnthropicAdapter,
        }
    }

    fn resolve_target_url(&self, route: RouteKind, base_url: &str) -> String {
        let base = base_url.trim_end_matches('/');
        match route {
            RouteKind::Anthropic => {
                if base.ends_with("/v1") {
                    let prefix = &base[..base.len() - 3];
                    format!("{}/anthropic", prefix)
                } else if !base.ends_with("/anthropic") {
                    format!("{}/anthropic", base)
                } else {
                    base.to_string()
                }
            },
            RouteKind::Openai => {
                 if base.ends_with("/anthropic") {
                    let prefix = &base[..base.len() - 10];
                    format!("{}/v1", prefix)
                 } else {
                    base.to_string()
                 }
            }
        }
    }
}

impl ProviderAdapter for DualProtocolAdapter {
    fn map_path(&self, route: RouteKind, base_url: &str, path: &str) -> String {
        let target_base = self.resolve_target_url(route, base_url);
        
        let suffix = match route {
            RouteKind::Anthropic => self.anthropic.map_path(route, &target_base, path),
            _ => self.openai.map_path(route, &target_base, path),
        };

        // If we modified the base URL, we must return an absolute URL to override the original base
        if target_base != base_url.trim_end_matches('/') {
             // Ensure target_base ends with / if needed for joining?
             // Actually, Url::parse(target_base).join(suffix) is safer.
             if let Ok(_base) = Url::parse(&target_base) {
                 // We need to ensure the base has a trailing slash if it's a directory,
                 // but resolve_target_url strips it. 
                 // If target_base is "https://api.foo.com/anthropic", we want to join "v1/messages" -> "https://api.foo.com/anthropic/v1/messages"
                 // So we should append / to base if not present.
                 let base_with_slash = if target_base.ends_with('/') {
                     target_base
                 } else {
                     format!("{}/", target_base)
                 };

                 if let Ok(base_url) = Url::parse(&base_with_slash) {
                     // Strip leading slash from suffix to ensure it's treated as relative
                     let relative_suffix = suffix.trim_start_matches('/');
                     
                     // Deduplicate 'v1' if base ends with it and suffix starts with it
                     let suffix_clean = if base_with_slash.ends_with("/v1/") && relative_suffix.starts_with("v1/") {
                         &relative_suffix[3..]
                     } else {
                         relative_suffix
                     };
                     
                     if let Ok(joined) = base_url.join(suffix_clean) {
                         return joined.to_string();
                     }
                 }
             }
        }
        
        suffix
    }

    fn transform_body(
        &self,
        route: RouteKind,
        body: &Bytes,
        model_map: &Option<HashMap<String, String>>,
    ) -> Bytes {
        // Use native adapter for the route
        match route {
            RouteKind::Anthropic => self.anthropic.transform_body(route, body, model_map),
            _ => self.openai.transform_body(route, body, model_map),
        }
    }

    fn apply_auth_headers(&self, route: RouteKind, headers: &mut HeaderMap, api_key: &str, base_url: &str) {
         match route {
            RouteKind::Anthropic => self.anthropic.apply_auth_headers(route, headers, api_key, base_url),
            _ => self.openai.apply_auth_headers(route, headers, api_key, base_url),
        }
    }
    
    // Use default handle_response which is pass-through for OpenAi/Anthropic adapters usually,
    // but we can delegate if needed. Both OpenAiAdapter and AnthropicAdapter use default.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_returns_adapter() {
        let registry = ProviderRegistry::new();
        let channel = Channel {
            name: "c".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://example.com".to_string(),
            api_key: "key".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
            timeouts: None,
        };
        let adapter = registry.adapter(&channel);
        let mapped = adapter.map_path(RouteKind::Openai, "https://example.com", "/v1/chat/completions");
        assert_eq!(mapped, "/v1/chat/completions");
    }

    #[test]
    fn applies_model_map() {
        let mut model_map = HashMap::new();
        model_map.insert("gpt-4".to_string(), "gpt-4o".to_string());
        let body = Bytes::from(r#"{"model":"gpt-4","messages":[]}"#);
        let updated = apply_model_map(&body, &Some(model_map));
        let value: serde_json::Value = serde_json::from_slice(&updated).unwrap();
        assert_eq!(value.get("model").unwrap(), "gpt-4o");
    }

    #[test]
    fn model_map_noop_on_invalid_json() {
        let mut model_map = HashMap::new();
        model_map.insert("gpt-4".to_string(), "gpt-4o".to_string());
        let body = Bytes::from("not-json");
        let updated = apply_model_map(&body, &Some(model_map));
        assert_eq!(updated, body);
    }

    #[test]
    fn model_map_noop_on_missing_model() {
        let mut model_map = HashMap::new();
        model_map.insert("gpt-4".to_string(), "gpt-4o".to_string());
        let body = Bytes::from(r#"{"messages":[]}"#);
        let updated = apply_model_map(&body, &Some(model_map));
        assert_eq!(updated, body);
    }

    #[test]
    fn sets_openai_auth_header() {
        let registry = ProviderRegistry::new();
        let channel = Channel {
            name: "c".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://example.com".to_string(),
            api_key: "key".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
            timeouts: None,
        };
        let headers = HeaderMap::new();
        let prepared = prepare_request(
            &registry,
            &channel,
            RouteKind::Openai,
            &channel.base_url,
            "/v1/chat/completions",
            None,
            &headers,
            &Bytes::from("{}"),
        )
        .unwrap();
        assert!(prepared.headers.get("authorization").is_some());
    }

    #[test]
    fn sets_anthropic_auth_header() {
        let registry = ProviderRegistry::new();
        let channel = Channel {
            name: "c".to_string(),
            provider_type: ProviderType::Anthropic,
            base_url: "https://example.com".to_string(),
            api_key: "key".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
            timeouts: None,
        };
        let headers = HeaderMap::new();
        let prepared = prepare_request(
            &registry,
            &channel,
            RouteKind::Anthropic,
            &channel.base_url,
            "/v1/messages",
            None,
            &headers,
            &Bytes::from("{}"),
        )
        .unwrap();
        assert!(prepared.headers.get("x-api-key").is_some());
        assert_eq!(prepared.headers.get("anthropic-version").unwrap(), "2023-06-01");
    }

    #[test]
    fn dual_protocol_adapter_switches_base_url() {
        let adapter = DualProtocolAdapter::new();
        
        // Case 1: OpenAI route with OpenAI base_url -> keeps as is
        // base: https://api.minimax.io/v1
        // route: OpenAI
        // resolve: keeps https://api.minimax.io/v1
        // map_path: returns suffix "/v1/chat/completions" (OpenAiAdapter behavior)
        let url = adapter.map_path(RouteKind::Openai, "https://api.minimax.io/v1", "/v1/chat/completions");
        assert_eq!(url, "/v1/chat/completions");

        // Case 2: Anthropic route with OpenAI base_url -> switches to /anthropic
        // base: https://api.minimax.io/v1
        // route: Anthropic
        // resolve: https://api.minimax.io/anthropic
        // map_path: returns absolute URL
        let url = adapter.map_path(RouteKind::Anthropic, "https://api.minimax.io/v1", "/v1/messages");
        assert_eq!(url, "https://api.minimax.io/anthropic/v1/messages");

        // Case 3: OpenAI route with Anthropic base_url -> switches to /v1
        // base: https://api.minimax.io/anthropic
        // route: OpenAI
        // resolve: https://api.minimax.io/v1
        // map_path: returns absolute URL
        let url = adapter.map_path(RouteKind::Openai, "https://api.minimax.io/anthropic", "/v1/chat/completions");
        // Note: v1 deduplication should happen here
        assert_eq!(url, "https://api.minimax.io/v1/chat/completions");

        // Case 4: Deepseek style (no v1 in base) -> Anthropic route
        // base: https://api.deepseek.com
        // route: Anthropic
        // resolve: https://api.deepseek.com/anthropic
        // map_path: returns absolute URL
        let url = adapter.map_path(RouteKind::Anthropic, "https://api.deepseek.com", "/v1/messages");
        assert_eq!(url, "https://api.deepseek.com/anthropic/v1/messages");

        // Case 5: Deepseek style (no v1 in base) -> OpenAI route
        // base: https://api.deepseek.com
        // route: OpenAI
        // resolve: https://api.deepseek.com (no change)
        // map_path: returns suffix
        let url = adapter.map_path(RouteKind::Openai, "https://api.deepseek.com", "/v1/chat/completions");
        assert_eq!(url, "/v1/chat/completions");
    }

    #[test]
    fn sets_anthropic_default_version() {
        let registry = ProviderRegistry::new();
        let channel = Channel {
            name: "c".to_string(),
            provider_type: ProviderType::Anthropic,
            base_url: "https://example.com".to_string(),
            api_key: "key".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
            timeouts: None,
        };
        let headers = HeaderMap::new();
        let prepared = prepare_request(
            &registry,
            &channel,
            RouteKind::Anthropic,
            &channel.base_url,
            "/v1/messages",
            None,
            &headers,
            &Bytes::from("{}"),
        )
        .unwrap();
        assert_eq!(
            prepared
                .headers
                .get("anthropic-version")
                .unwrap()
                .to_str()
                .unwrap(),
            "2023-06-01"
        );
    }

    #[test]
    fn sets_gemini_auth_header() {
        let registry = ProviderRegistry::new();
        let channel = Channel {
            name: "c".to_string(),
            provider_type: ProviderType::Gemini,
            base_url: "https://example.com".to_string(),
            api_key: "key".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
            timeouts: None,
        };
        let headers = HeaderMap::new();
        let prepared = prepare_request(
            &registry,
            &channel,
            RouteKind::Openai,
            &channel.base_url,
            "/v1/chat/completions",
            None,
            &headers,
            &Bytes::from("{}"),
        )
        .unwrap();
        assert!(prepared.headers.get("x-goog-api-key").is_some());
    }

    #[test]
    fn apply_bearer_auth_skips_empty() {
        let mut headers = HeaderMap::new();
        apply_bearer_auth(&mut headers, "", "authorization");
        assert!(headers.get("authorization").is_none());
    }

    #[test]
    fn build_headers_filters_gateway_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("a"));
        headers.insert("authorization", HeaderValue::from_static("b"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let channel = Channel {
            name: "c".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://example.com".to_string(),
            api_key: "".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
            timeouts: None,
        };
        let merged = build_headers(&headers, &channel);
        assert!(merged.get("x-api-key").is_none());
        assert!(merged.get("authorization").is_none());
        assert!(merged.get("content-type").is_some());
    }

    #[test]
    fn build_headers_merges_channel_headers() {
        let headers = HeaderMap::new();
        let mut extra = HashMap::new();
        extra.insert("x-extra".to_string(), "1".to_string());
        let channel = Channel {
            name: "c".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://example.com".to_string(),
            api_key: "".to_string(),
            anthropic_base_url: None,
            headers: Some(extra),
            model_map: None,
            timeouts: None,
        };
        let merged = build_headers(&headers, &channel);
        assert_eq!(merged.get("x-extra").unwrap(), "1");
    }

    #[test]
    fn build_url_deduplicates_v1() {
        let url = build_url("https://api.example.com/v1", "v1/chat/completions", None).unwrap();
        assert_eq!(url.as_str(), "https://api.example.com/v1/chat/completions");
        
        let url = build_url("https://api.example.com/v1/", "v1/chat/completions", None).unwrap();
        assert_eq!(url.as_str(), "https://api.example.com/v1/chat/completions");

        let url = build_url("https://api.example.com", "v1/chat/completions", None).unwrap();
        assert_eq!(url.as_str(), "https://api.example.com/v1/chat/completions");
    }

    #[test]
    fn build_url_adds_query() {
        let url = build_url("https://example.com", "/v1/models", Some("a=b")).unwrap();
        assert_eq!(url.as_str(), "https://example.com/v1/models?a=b");
    }

    #[test]
    fn registry_respects_protocol_override() {
        let registry = ProviderRegistry::new();
        let channel = Channel {
            name: "c".to_string(),
            provider_type: ProviderType::Minimax, // Usually uses DefaultAdapter
            base_url: "https://api.minimax.io/anthropic".to_string(),
            api_key: "key".to_string(),
            anthropic_base_url: None,
            headers: None,
            model_map: None,
            timeouts: None,
        };
        let headers = HeaderMap::new();
        let prepared = prepare_request(
            &registry,
            &channel,
            RouteKind::Anthropic, // Incoming request is Anthropic
            &channel.base_url,
            "/v1/messages",
            None,
            &headers,
            &Bytes::from("{}"),
        )
        .unwrap();

        // Should use AnthropicAdapter which sets x-api-key
        assert!(prepared.headers.get("x-api-key").is_some());
        // DefaultAdapter would set Authorization
        assert!(prepared.headers.get("authorization").is_none());
        // Path should not be remapped to chat/completions
        assert!(prepared.url.as_str().contains("/v1/messages"));
    }
}
