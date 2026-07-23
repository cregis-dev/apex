//! Privacy-preserving request fingerprinting for behavior profiling.
//!
//! Computes a stable, one-way fingerprint of a request's *semantic payload*
//! (the conversation content) so the same request repeated by the same user can
//! be detected — loops, idle spinning, volume-padding — **without ever storing
//! the prompt text**. Only the fingerprint is persisted (`usage_records.req_hash`).
//!
//! Two robustness properties matter for repeat detection:
//! - We fingerprint only the content-bearing field (`messages` / `contents` /
//!   `prompt`), so unrelated parameter jitter (`max_tokens`, `stream`,
//!   `temperature`, …) does not change the fingerprint.
//! - We canonicalize object key order, so two semantically-identical bodies
//!   that differ only in JSON key ordering hash identically. This is independent
//!   of serde_json's Map ordering (`preserve_order` feature) so behavior is
//!   stable regardless of how the crate graph is configured.
//!
//! See `docs/design/behavior-profiling.md` §3.2.

use serde_json::Value;

/// Truncated blake3 hex fingerprint (128-bit / 32 hex chars) of the request's
/// semantic payload, or `None` when the body is not valid JSON.
///
/// The fingerprint is one-way: the prompt cannot be recovered from it. 128 bits
/// is far more than enough collision resistance for per-user dedup.
pub fn request_hash(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let payload = semantic_payload(&value);
    let mut canonical = String::new();
    write_canonical(payload, &mut canonical);
    Some(fingerprint(canonical.as_bytes()))
}

/// Pick the content-bearing sub-document to fingerprint, falling back to the
/// whole body when no known field is present.
fn semantic_payload(value: &Value) -> &Value {
    value
        .get("messages") // OpenAI / Anthropic chat
        .or_else(|| value.get("contents")) // Gemini native
        .or_else(|| value.get("prompt")) // legacy completion
        .unwrap_or(value)
}

fn fingerprint(bytes: &[u8]) -> String {
    // blake3 hex is 64 chars (32 bytes); keep the first 128 bits (32 hex chars).
    blake3::hash(bytes).to_hex()[..32].to_string()
}

/// Serialize `v` deterministically into `out`: object keys sorted, arrays in
/// order, no incidental whitespace. Independent of serde_json's Map ordering.
fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // Serialize the key as a JSON string (handles escaping).
                out.push_str(&Value::String((*key).clone()).to_string());
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        // Scalars (string / number / bool / null) serialize deterministically.
        scalar => out.push_str(&scalar.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_messages_hash_identically() {
        let a =
            br#"{"model":"gpt-4o","messages":[{"role":"user","content":"hi"}],"max_tokens":100}"#;
        // Same messages, different unrelated params + reordered top-level keys.
        let b = br#"{"messages":[{"role":"user","content":"hi"}],"model":"gpt-4o","stream":true,"temperature":0.9}"#;
        assert_eq!(request_hash(a), request_hash(b));
    }

    #[test]
    fn reordered_object_keys_hash_identically() {
        let a = br#"{"messages":[{"role":"user","content":"hi"}]}"#;
        let b = br#"{"messages":[{"content":"hi","role":"user"}]}"#;
        assert_eq!(request_hash(a), request_hash(b));
    }

    #[test]
    fn different_content_hashes_differently() {
        let a = br#"{"messages":[{"role":"user","content":"hello"}]}"#;
        let b = br#"{"messages":[{"role":"user","content":"world"}]}"#;
        assert_ne!(request_hash(a), request_hash(b));
    }

    #[test]
    fn array_order_is_significant() {
        // Message order matters — a reordered conversation is a different request.
        let a = br#"{"messages":[{"role":"user","content":"a"},{"role":"user","content":"b"}]}"#;
        let b = br#"{"messages":[{"role":"user","content":"b"},{"role":"user","content":"a"}]}"#;
        assert_ne!(request_hash(a), request_hash(b));
    }

    #[test]
    fn gemini_contents_and_legacy_prompt_are_fingerprinted() {
        assert!(request_hash(br#"{"contents":[{"parts":[{"text":"hi"}]}]}"#).is_some());
        assert!(request_hash(br#"{"prompt":"complete this"}"#).is_some());
    }

    #[test]
    fn non_json_body_returns_none() {
        assert_eq!(request_hash(b"not json at all"), None);
        assert_eq!(request_hash(b""), None);
    }

    #[test]
    fn fingerprint_is_32_hex_chars_and_leaks_no_content() {
        let secret = "supersecretpromptcontent";
        let body = format!(r#"{{"messages":[{{"role":"user","content":"{secret}"}}]}}"#);
        let hash = request_hash(body.as_bytes()).unwrap();
        assert_eq!(hash.len(), 32);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!hash.contains(secret));
    }
}
