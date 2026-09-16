//! Which Gemini-native paths the gateway accepts, and how each maps onto a
//! routing key. Model-bearing paths route on the model; model-less resource
//! paths use a synthetic key so team policy still has something to match.

use axum::http::Method;

// Helpers

pub(super) struct GeminiNativeRoute {
    pub(super) routing_model: String,
    pub(super) direct_pass: bool,
}

pub(super) fn validate_gemini_native_route(
    method: &Method,
    path: &str,
) -> Option<GeminiNativeRoute> {
    let path = path.trim_start_matches('/');
    let segments = path.split('/').collect::<Vec<_>>();

    match (method, segments.as_slice()) {
        (&Method::GET, ["v1beta", "models"]) => Some(gemini_native_resource_route()),
        (&Method::GET, ["v1beta", "models", model]) if !model.contains(':') => {
            Some(gemini_native_model_route(model))
        }
        (&Method::POST, ["v1beta", "models", model_action])
            if model_action.ends_with(":generateContent")
                || model_action.ends_with(":streamGenerateContent") =>
        {
            model_action
                .split_once(':')
                .filter(|(model, _)| !model.is_empty())
                .map(|(model, _)| gemini_native_model_route(model))
        }
        (&Method::GET, ["v1beta", "fileSearchStores"])
        | (&Method::POST, ["v1beta", "fileSearchStores"]) => Some(gemini_native_resource_route()),
        (&Method::GET, ["v1beta", "fileSearchStores", store])
        | (&Method::DELETE, ["v1beta", "fileSearchStores", store])
            if !store.contains(':') =>
        {
            Some(gemini_native_resource_route())
        }
        (&Method::POST, ["v1beta", "fileSearchStores", store_action])
        | (&Method::POST, ["upload", "v1beta", "fileSearchStores", store_action])
            if store_action.ends_with(":uploadToFileSearchStore") =>
        {
            Some(GeminiNativeRoute {
                routing_model: "gemini-native".to_string(),
                direct_pass: true,
            })
        }
        (&Method::GET, ["v1beta", "fileSearchStores", store, "operations", operation])
        | (
            &Method::GET,
            [
                "v1beta",
                "fileSearchStores",
                store,
                "upload",
                "operations",
                operation,
            ],
        ) if !store.is_empty() && !operation.is_empty() => Some(gemini_native_resource_route()),
        (&Method::POST, ["v1beta", "interactions"]) => Some(gemini_native_direct_pass_route()),
        (&Method::GET, ["v1beta", "interactions", interaction]) if !interaction.is_empty() => {
            Some(gemini_native_direct_pass_route())
        }
        _ => None,
    }
}

pub(super) fn gemini_native_resource_route() -> GeminiNativeRoute {
    GeminiNativeRoute {
        routing_model: "gemini-native".to_string(),
        direct_pass: false,
    }
}

pub(super) fn gemini_native_direct_pass_route() -> GeminiNativeRoute {
    GeminiNativeRoute {
        routing_model: "gemini-native".to_string(),
        direct_pass: true,
    }
}

pub(super) fn gemini_native_model_route(model: &str) -> GeminiNativeRoute {
    GeminiNativeRoute {
        routing_model: model.to_string(),
        direct_pass: false,
    }
}

pub(super) fn gemini_native_resource_router_is_deterministic(
    router: &crate::config::Router,
    model: &str,
) -> bool {
    router.rules.iter().any(|rule| {
        crate::config::TeamPolicy {
            allowed_routers: Vec::new(),
            allowed_models: Some(rule.match_spec.models.clone()),
            rate_limit: None,
        }
        .is_model_allowed(model)
            && rule.strategy == "priority"
            && rule.channels.len() == 1
    })
}
