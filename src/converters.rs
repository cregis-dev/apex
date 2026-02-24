use axum::body::Bytes;
use futures::{Stream, StreamExt, stream};
use std::io;

/// Converts an OpenAI-compatible JSON response body to Anthropic's format.
///
/// This handles:
/// - Error responses
/// - Chat completion responses
/// - Usage statistics mapping
pub fn convert_openai_response_to_anthropic(body: Bytes) -> Bytes {
    let Ok(val) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };

    // Check for error response
    if let Some(error) = val.get("error") {
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown error");
        let type_ = error
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("invalid_request_error");

        let anthropic_error = serde_json::json!({
            "type": "error",
            "error": {
                "type": type_,
                "message": message
            }
        });

        if let Ok(vec) = serde_json::to_vec(&anthropic_error) {
            return Bytes::from(vec);
        }
        return body;
    }

    let mut new_body = serde_json::Map::new();

    if let Some(id) = val.get("id") {
        new_body.insert("id".to_string(), id.clone());
    }
    new_body.insert(
        "type".to_string(),
        serde_json::Value::String("message".to_string()),
    );
    new_body.insert(
        "role".to_string(),
        serde_json::Value::String("assistant".to_string()),
    );

    // Content
    if let Some(choices) = val.get("choices").and_then(|c| c.as_array())
        && let Some(first) = choices.first()
    {
        if let Some(message) = first.get("message")
            && let Some(content) = message.get("content").and_then(|c| c.as_str())
        {
            new_body.insert(
                "content".to_string(),
                serde_json::json!([
                    {
                        "type": "text",
                        "text": content
                    }
                ]),
            );
        }
        if let Some(finish_reason) = first.get("finish_reason").and_then(|fr| fr.as_str()) {
            let stop_reason = match finish_reason {
                "stop" => "end_turn",
                "length" => "max_tokens",
                _ => "stop_sequence",
            };
            new_body.insert(
                "stop_reason".to_string(),
                serde_json::Value::String(stop_reason.to_string()),
            );
        } else {
            new_body.insert(
                "stop_reason".to_string(),
                serde_json::Value::String("end_turn".to_string()),
            );
        }
    }

    // Model
    if let Some(model) = val.get("model") {
        new_body.insert("model".to_string(), model.clone());
    }

    // Usage
    if let Some(usage) = val.get("usage") {
        let mut new_usage = serde_json::Map::new();
        if let Some(pt) = usage.get("prompt_tokens") {
            new_usage.insert("input_tokens".to_string(), pt.clone());
        }
        if let Some(ct) = usage.get("completion_tokens") {
            new_usage.insert("output_tokens".to_string(), ct.clone());
        }
        new_body.insert("usage".to_string(), serde_json::Value::Object(new_usage));
    }

    match serde_json::to_vec(&new_body) {
        Ok(vec) => Bytes::from(vec),
        Err(_) => body,
    }
}

/// Converts an OpenAI-compatible SSE stream to Anthropic's SSE format.
///
/// This handles:
/// - Parsing "data: " lines from OpenAI stream
/// - Converting delta content to Anthropic content blocks
/// - Mapping finish_reason to stop_reason
/// - Generating necessary Anthropic events (message_start, content_block_start, etc.)
pub fn convert_openai_stream_to_anthropic<S>(
    stream: S,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin + Send + 'static,
{
    let state: (S, Vec<u8>, bool) = (stream, Vec::new(), false);

    stream::unfold(
        state,
        |(mut stream, mut buffer, mut sent_header)| async move {
            loop {
                // Check if we have a full line in buffer
                if let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                    let line_bytes: Vec<u8> = buffer.drain(0..=pos).collect();
                    // Remove trailing newline (and carriage return if present)
                    let line_str = String::from_utf8_lossy(&line_bytes);
                    let line = line_str.trim();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            let event = format!(
                                "event: message_stop\ndata: {}\n\n",
                                serde_json::json!({
                                     "type": "message_stop"
                                })
                            );
                            return Some((Ok(Bytes::from(event)), (stream, buffer, sent_header)));
                        }

                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                            let mut events = Vec::new();

                            if !sent_header {
                                let id = val["id"].as_str().unwrap_or("msg_123");
                                let model = val["model"].as_str().unwrap_or("model");

                                events.push(format!(
                                    "event: message_start\ndata: {}\n\n",
                                    serde_json::json!({
                                        "type": "message_start",
                                        "message": {
                                            "id": id,
                                            "type": "message",
                                            "role": "assistant",
                                            "content": [],
                                            "model": model,
                                            "stop_reason": null,
                                            "stop_sequence": null,
                                            "usage": {"input_tokens": 0, "output_tokens": 0}
                                        }
                                    })
                                ));

                                events.push(format!(
                                    "event: content_block_start\ndata: {}\n\n",
                                    serde_json::json!({
                                        "type": "content_block_start",
                                        "index": 0,
                                        "content_block": {
                                            "type": "text",
                                            "text": ""
                                        }
                                    })
                                ));
                                sent_header = true;
                            }

                            if let Some(choices) = val.get("choices").and_then(|c| c.as_array())
                                && let Some(choice) = choices.first()
                            {
                                if let Some(delta) = choice.get("delta")
                                    && let Some(content) =
                                        delta.get("content").and_then(|c| c.as_str())
                                    && !content.is_empty()
                                {
                                    events.push(format!(
                                        "event: content_block_delta\ndata: {}\n\n",
                                        serde_json::json!({
                                            "type": "content_block_delta",
                                            "index": 0,
                                            "delta": {
                                                "type": "text_delta",
                                                "text": content
                                            }
                                        })
                                    ));
                                }

                                if let Some(finish_reason) =
                                    choice.get("finish_reason").and_then(|fr| fr.as_str())
                                {
                                    let stop_reason = match finish_reason {
                                        "stop" => "end_turn",
                                        "length" => "max_tokens",
                                        _ => "stop_sequence",
                                    };

                                    events.push(format!(
                                        "event: message_delta\ndata: {}\n\n",
                                        serde_json::json!({
                                            "type": "message_delta",
                                            "delta": {
                                                "stop_reason": stop_reason,
                                                "stop_sequence": null
                                            },
                                             "usage": {"output_tokens": 0}
                                        })
                                    ));
                                }
                            }

                            if !events.is_empty() {
                                let output = events.join("");
                                return Some((
                                    Ok(Bytes::from(output)),
                                    (stream, buffer, sent_header),
                                ));
                            }
                        }
                    }
                    // Skip empty lines or non-data lines
                    continue;
                }

                // Need more data
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        buffer.extend_from_slice(&bytes);
                    }
                    Some(Err(e)) => {
                        return Some((Err(io::Error::other(e)), (stream, buffer, sent_header)));
                    }
                    None => {
                        return None;
                    }
                }
            }
        },
    )
}

/// Converts an Anthropic JSON request body to OpenAI's format.
pub fn convert_anthropic_to_openai(body: &Bytes) -> Bytes {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.clone();
    };

    let mut new_body = serde_json::Map::new();

    // Map model (if present)
    if let Some(model) = value.get("model") {
        new_body.insert("model".to_string(), model.clone());
    }

    // Map messages
    if let Some(messages) = value.get("messages").and_then(|m| m.as_array()) {
        let mut new_messages = Vec::new();

        // Handle system prompt if it exists at top level
        if let Some(system) = value.get("system").and_then(|s| s.as_str()) {
            new_messages.push(serde_json::json!({
                "role": "system",
                "content": system
            }));
        }

        for msg in messages {
            new_messages.push(msg.clone());
        }
        new_body.insert(
            "messages".to_string(),
            serde_json::Value::Array(new_messages),
        );
    }

    // Map max_tokens -> max_tokens
    if let Some(max_tokens) = value.get("max_tokens") {
        new_body.insert("max_tokens".to_string(), max_tokens.clone());
    }

    // Map temperature, top_p, etc.
    for key in ["temperature", "top_p", "top_k", "stream"] {
        if let Some(v) = value.get(key) {
            new_body.insert(key.to_string(), v.clone());
        }
    }

    match serde_json::to_vec(&new_body) {
        Ok(vec) => Bytes::from(vec),
        Err(_) => body.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_openai_response_to_anthropic_success() {
        let openai_resp = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-3.5-turbo-0613",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello there!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 9,
                "completion_tokens": 12,
                "total_tokens": 21
            }
        });

        let body = Bytes::from(serde_json::to_vec(&openai_resp).unwrap());
        let converted = convert_openai_response_to_anthropic(body);
        let val: serde_json::Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(val["type"], "message");
        assert_eq!(val["role"], "assistant");
        assert_eq!(val["content"][0]["text"], "Hello there!");
        assert_eq!(val["stop_reason"], "end_turn");
        assert_eq!(val["model"], "gpt-3.5-turbo-0613");
        assert_eq!(val["usage"]["input_tokens"], 9);
        assert_eq!(val["usage"]["output_tokens"], 12);
    }

    #[test]
    fn test_convert_openai_response_to_anthropic_error() {
        let openai_err = json!({
            "error": {
                "message": "Invalid API key",
                "type": "invalid_request_error",
                "param": null,
                "code": "invalid_api_key"
            }
        });

        let body = Bytes::from(serde_json::to_vec(&openai_err).unwrap());
        let converted = convert_openai_response_to_anthropic(body);
        let val: serde_json::Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(val["type"], "error");
        assert_eq!(val["error"]["type"], "invalid_request_error");
        assert_eq!(val["error"]["message"], "Invalid API key");
    }

    #[test]
    fn test_convert_anthropic_to_openai() {
        let anthropic_req = json!({
            "model": "claude-2",
            "messages": [
                {"role": "user", "content": "Hi"}
            ],
            "max_tokens": 100,
            "system": "Be nice"
        });

        let body = Bytes::from(serde_json::to_vec(&anthropic_req).unwrap());
        let converted = convert_anthropic_to_openai(&body);
        let val: serde_json::Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(val["model"], "claude-2");
        assert_eq!(val["max_tokens"], 100);

        let messages = val["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "Be nice");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Hi");
    }
}
