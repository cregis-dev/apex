use axum::http::HeaderMap;

/// Produce a compact, display-friendly masked form of a secret.
///
/// The output keeps the leading 3 and trailing 4 characters so an operator can
/// recognize the key at a glance, while the middle is collapsed to a fixed
/// ellipsis. The middle never reveals how long the original was, because doing
/// so leaks bits of entropy.
///
/// IMPORTANT: the prefix+suffix preview reveals 7 characters, so for anything
/// short enough that those 7 would expose most/all of the secret we fall back
/// to a fully-starred string. We never return the raw value — even short
/// user-supplied keys must not round-trip in plaintext through the masked
/// list/reveal endpoints.
///
/// Examples:
///   ""                    -> ""
///   "abc"                 -> "***"
///   "1234567"             -> "*******"
///   "12345678"            -> "********"
///   "sk-1234567890abcdef" -> "sk-…cdef"
const MASK_PREVIEW_PREFIX: usize = 3;
const MASK_PREVIEW_SUFFIX: usize = 4;
/// Minimum hidden characters required before we show the prefix/suffix preview.
const MASK_MIN_HIDDEN: usize = 4;

pub fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let len = chars.len();

    if len == 0 {
        return String::new();
    }

    // Only reveal the prefix/suffix preview when enough of the secret stays
    // hidden; otherwise fully star it. This guarantees short secrets are never
    // returned in (near-)plaintext.
    let preview_len = MASK_PREVIEW_PREFIX + MASK_PREVIEW_SUFFIX;
    if len < preview_len + MASK_MIN_HIDDEN {
        return "*".repeat(len);
    }

    let prefix: String = chars.iter().take(MASK_PREVIEW_PREFIX).collect();
    let suffix: String = chars.iter().skip(len - MASK_PREVIEW_SUFFIX).collect();
    format!("{prefix}…{suffix}")
}

/// A normalized label for the calling tool/client, plus the raw User-Agent.
///
/// Derived from request headers so the dashboard can break usage down by tool
/// (Claude Code, Codex, SDKs, scripts, …). The raw UA is kept (truncated) so the
/// "Other" bucket can be refined later without guessing.
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct ClientInfo {
    pub client: Option<String>,
    pub user_agent: Option<String>,
}

fn header_lower(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
}

/// Best-effort classification of the calling tool from request headers.
///
/// Only tools pointed directly at this gateway's base URL expose a real
/// User-Agent here (e.g. Claude Code via `ANTHROPIC_BASE_URL`, Codex via
/// `OPENAI_BASE_URL`). Signatures vary by version, so unknown UAs fall back to
/// "Other" while the raw UA is retained for later refinement.
pub fn classify_client(headers: &HeaderMap) -> ClientInfo {
    let ua_raw = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim()
        .to_string();
    let ua = ua_raw.to_lowercase();
    let originator = header_lower(headers, "originator").unwrap_or_default();
    let x_app = header_lower(headers, "x-app").unwrap_or_default();
    let x_title = headers
        .get("x-title")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let stainless_lang = header_lower(headers, "x-stainless-lang");

    let label: Option<String> = if ua.contains("claude-cli") || ua.contains("claude-code") {
        Some("Claude Code".to_string())
    } else if originator.contains("codex") || ua.contains("codex") {
        Some("Codex".to_string())
    } else if ua.contains("geminicli") || ua.contains("gemini-cli") {
        Some("Gemini CLI".to_string())
    } else if ua.contains("aider") {
        Some("Aider".to_string())
    } else if ua.contains("cursor") {
        Some("Cursor".to_string())
    } else if ua.contains("langchain") {
        Some("LangChain".to_string())
    } else if ua.contains("llama-index") || ua.contains("llamaindex") {
        Some("LlamaIndex".to_string())
    } else if let Some(title) = x_title {
        // OpenRouter-style apps (Cline, Roo Code, Kilo, …) self-identify here.
        Some(title)
    } else if x_app == "cli" {
        Some("CLI".to_string())
    } else if let Some(lang) = stainless_lang {
        // Official Stainless-generated SDK (openai/anthropic/…) used directly.
        Some(format!("SDK ({lang})"))
    } else if ua.starts_with("openai") {
        Some("OpenAI SDK".to_string())
    } else if ua.starts_with("anthropic") {
        Some("Anthropic SDK".to_string())
    } else if ua.starts_with("curl/") {
        Some("curl".to_string())
    } else if ua.contains("python-requests") || ua.contains("httpx") || ua.contains("aiohttp") {
        Some("Python script".to_string())
    } else if ua.contains("go-http-client") {
        Some("Go client".to_string())
    } else if ua.contains("okhttp")
        || ua.contains("undici")
        || ua.contains("axios")
        || ua.contains("node-fetch")
    {
        Some("Node client".to_string())
    } else if ua.is_empty() {
        None
    } else {
        Some("Other".to_string())
    };

    ClientInfo {
        client: label,
        user_agent: if ua_raw.is_empty() {
            None
        } else {
            Some(ua_raw.chars().take(256).collect())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ua(value: &str) -> ClientInfo {
        let mut headers = HeaderMap::new();
        if !value.is_empty() {
            headers.insert(axum::http::header::USER_AGENT, value.parse().unwrap());
        }
        classify_client(&headers)
    }

    #[test]
    fn test_classify_client() {
        assert_eq!(
            ua("claude-cli/1.2.3 (external, cli)").client.as_deref(),
            Some("Claude Code")
        );
        assert_eq!(ua("codex_cli_rs/0.4.0").client.as_deref(), Some("Codex"));
        assert_eq!(ua("GeminiCLI/v1").client.as_deref(), Some("Gemini CLI"));
        assert_eq!(ua("curl/8.4.0").client.as_deref(), Some("curl"));
        assert_eq!(
            ua("OpenAI/Python 1.2").client.as_deref(),
            Some("OpenAI SDK")
        );
        assert_eq!(ua("Mozilla/5.0 weird").client.as_deref(), Some("Other"));
        assert_eq!(ua("").client, None);
        assert_eq!(
            ua("claude-cli/9").user_agent.as_deref(),
            Some("claude-cli/9")
        );

        // Header-based signals.
        let mut h = HeaderMap::new();
        h.insert("originator", "codex_cli_rs".parse().unwrap());
        assert_eq!(classify_client(&h).client.as_deref(), Some("Codex"));

        let mut h = HeaderMap::new();
        h.insert("user-agent", "node".parse().unwrap());
        h.insert("x-title", "Cline".parse().unwrap());
        assert_eq!(classify_client(&h).client.as_deref(), Some("Cline"));
    }

    #[test]
    fn test_mask_secret() {
        assert_eq!(mask_secret(""), "");
        assert_eq!(mask_secret("abc"), "***");
        // Short secrets (< 11 chars) are fully starred — never plaintext.
        assert_eq!(mask_secret("1234567"), "*******");
        assert_eq!(mask_secret("12345678"), "********");
        assert_eq!(mask_secret("sk-abc12"), "********");
        assert_eq!(mask_secret("0123456789"), "**********");
        // 11+ chars: prefix(3)…suffix(4) preview, middle length hidden.
        assert_eq!(mask_secret("0123456789a"), "012…789a");
        assert_eq!(mask_secret("sk-1234567890abcdef"), "sk-…cdef");
        assert_eq!(
            mask_secret("sk-apex-0f45bc06fa770eb934e894f7b036ab0c"),
            "sk-…ab0c"
        );
    }
}
