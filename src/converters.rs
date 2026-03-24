use axum::body::Bytes;
use futures::{Stream, StreamExt, stream};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::io;

/// Converts an OpenAI-compatible JSON response body to Anthropic's format.
///
/// This handles:
/// - Error responses
/// - Chat completion responses
/// - Usage statistics mapping
pub fn convert_openai_response_to_anthropic(body: Bytes) -> Bytes {
    let Ok(val) = serde_json::from_slice::<Value>(&body) else {
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

        let anthropic_error = json!({
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

    let mut new_body = Map::new();

    if let Some(id) = val.get("id") {
        new_body.insert("id".to_string(), id.clone());
    }
    new_body.insert("type".to_string(), Value::String("message".to_string()));
    new_body.insert("role".to_string(), Value::String("assistant".to_string()));

    // Content
    if let Some(choices) = val.get("choices").and_then(|c| c.as_array())
        && let Some(first) = choices.first()
    {
        if let Some(message) = first.get("message") {
            let mut content_blocks = Vec::new();
            append_openai_message_content_as_anthropic_blocks(
                message.get("content"),
                &mut content_blocks,
            );
            append_openai_tool_calls_as_anthropic_blocks(
                message.get("tool_calls"),
                &mut content_blocks,
            );
            new_body.insert("content".to_string(), Value::Array(content_blocks));
        }
        if let Some(finish_reason) = first.get("finish_reason").and_then(|fr| fr.as_str()) {
            let stop_reason = match finish_reason {
                "stop" => "end_turn",
                "length" => "max_tokens",
                "tool_calls" => "tool_use",
                _ => "stop_sequence",
            };
            new_body.insert(
                "stop_reason".to_string(),
                Value::String(stop_reason.to_string()),
            );
        } else {
            new_body.insert(
                "stop_reason".to_string(),
                Value::String("end_turn".to_string()),
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
        new_body.insert("usage".to_string(), Value::Object(new_usage));
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
    let state: (S, Vec<u8>, StreamConversionState) =
        (stream, Vec::new(), StreamConversionState::default());

    stream::unfold(state, |(mut stream, mut buffer, mut state)| async move {
        loop {
            // Check if we have a full line in buffer
            if let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buffer.drain(0..=pos).collect();
                // Remove trailing newline (and carriage return if present)
                let line_str = String::from_utf8_lossy(&line_bytes);
                let line = line_str.trim();

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        let mut events = Vec::new();

                        if state.text_block_started && !state.text_block_closed {
                            events.push(format!(
                                "event: content_block_stop\ndata: {}\n\n",
                                serde_json::json!({
                                    "type": "content_block_stop",
                                    "index": 0
                                })
                            ));
                            state.text_block_closed = true;
                        }

                        let pending_tool_stops: Vec<usize> =
                            state.tool_blocks_started.iter().copied().collect();
                        for block_index in pending_tool_stops {
                            if state.tool_blocks_closed.insert(block_index) {
                                events.push(format!(
                                    "event: content_block_stop\ndata: {}\n\n",
                                    serde_json::json!({
                                        "type": "content_block_stop",
                                        "index": block_index
                                    })
                                ));
                            }
                        }

                        if !state.sent_message_delta {
                            events.push(format!(
                                "event: message_delta\ndata: {}\n\n",
                                serde_json::json!({
                                    "type": "message_delta",
                                    "delta": {
                                        "stop_reason": "end_turn",
                                        "stop_sequence": null
                                    },
                                    "usage": {"output_tokens": 0}
                                })
                            ));
                            state.sent_message_delta = true;
                        }

                        events.push(format!(
                            "event: message_stop\ndata: {}\n\n",
                            serde_json::json!({
                                 "type": "message_stop"
                            })
                        ));
                        return Some((Ok(Bytes::from(events.join(""))), (stream, buffer, state)));
                    }

                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                        let mut events = Vec::new();

                        if !state.sent_message_start {
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
                            state.sent_message_start = true;
                        }

                        if let Some(choices) = val.get("choices").and_then(|c| c.as_array())
                            && let Some(choice) = choices.first()
                        {
                            if let Some(delta) = choice.get("delta") {
                                if let Some(content) = delta.get("content").and_then(|c| c.as_str())
                                    && !content.is_empty()
                                {
                                    if !state.text_block_started {
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
                                        state.text_block_started = true;
                                        state.saw_text_block = true;
                                    }

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

                                if let Some(tool_calls) =
                                    delta.get("tool_calls").and_then(Value::as_array)
                                {
                                    if state.text_block_started && !state.text_block_closed {
                                        events.push(format!(
                                            "event: content_block_stop\ndata: {}\n\n",
                                            serde_json::json!({
                                                "type": "content_block_stop",
                                                "index": 0
                                            })
                                        ));
                                        state.text_block_closed = true;
                                    }

                                    for tool_call in tool_calls {
                                        let raw_index = tool_call
                                            .get("index")
                                            .and_then(Value::as_u64)
                                            .unwrap_or(0)
                                            as usize;
                                        let block_index =
                                            raw_index + usize::from(state.saw_text_block);

                                        if !state.tool_blocks_started.contains(&block_index) {
                                            let id = tool_call
                                                .get("id")
                                                .and_then(Value::as_str)
                                                .unwrap_or("tool_call");
                                            let name = tool_call
                                                .get("function")
                                                .and_then(|function| function.get("name"))
                                                .and_then(Value::as_str)
                                                .unwrap_or("tool");
                                            let mut content_block = json!({
                                                "type": "tool_use",
                                                "id": id,
                                                "name": name,
                                                "input": {}
                                            });
                                            if let Some(extra_content) =
                                                tool_call.get("extra_content").cloned()
                                                && let Some(map) = content_block.as_object_mut()
                                            {
                                                map.insert(
                                                    "extra_content".to_string(),
                                                    extra_content,
                                                );
                                            }
                                            events.push(format!(
                                                "event: content_block_start\ndata: {}\n\n",
                                                serde_json::json!({
                                                    "type": "content_block_start",
                                                    "index": block_index,
                                                    "content_block": content_block
                                                })
                                            ));
                                            state.tool_blocks_started.insert(block_index);
                                        }

                                        if let Some(arguments) = tool_call
                                            .get("function")
                                            .and_then(|function| function.get("arguments"))
                                            .and_then(Value::as_str)
                                        {
                                            events.push(format!(
                                                "event: content_block_delta\ndata: {}\n\n",
                                                serde_json::json!({
                                                    "type": "content_block_delta",
                                                    "index": block_index,
                                                    "delta": {
                                                        "type": "input_json_delta",
                                                        "partial_json": arguments
                                                    }
                                                })
                                            ));
                                        }
                                    }
                                }
                            }

                            if let Some(finish_reason) =
                                choice.get("finish_reason").and_then(|fr| fr.as_str())
                            {
                                if state.text_block_started && !state.text_block_closed {
                                    events.push(format!(
                                        "event: content_block_stop\ndata: {}\n\n",
                                        serde_json::json!({
                                            "type": "content_block_stop",
                                            "index": 0
                                        })
                                    ));
                                    state.text_block_closed = true;
                                }

                                let pending_tool_stops: Vec<usize> =
                                    state.tool_blocks_started.iter().copied().collect();
                                for block_index in pending_tool_stops {
                                    if state.tool_blocks_closed.insert(block_index) {
                                        events.push(format!(
                                            "event: content_block_stop\ndata: {}\n\n",
                                            serde_json::json!({
                                                "type": "content_block_stop",
                                                "index": block_index
                                            })
                                        ));
                                    }
                                }

                                let stop_reason = match finish_reason {
                                    "stop" => "end_turn",
                                    "length" => "max_tokens",
                                    "tool_calls" => "tool_use",
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
                                state.sent_message_delta = true;
                            }
                        }

                        if !events.is_empty() {
                            let output = events.join("");
                            return Some((Ok(Bytes::from(output)), (stream, buffer, state)));
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
                    return Some((Err(io::Error::other(e)), (stream, buffer, state)));
                }
                None => {
                    return None;
                }
            }
        }
    })
}

#[derive(Default)]
struct StreamConversionState {
    sent_message_start: bool,
    sent_message_delta: bool,
    saw_text_block: bool,
    text_block_started: bool,
    text_block_closed: bool,
    tool_blocks_started: BTreeSet<usize>,
    tool_blocks_closed: BTreeSet<usize>,
}

/// Converts an Anthropic JSON request body to OpenAI's format.
pub fn convert_anthropic_to_openai(body: &Bytes) -> Bytes {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return body.clone();
    };

    let mut new_body = Map::new();

    // Map model (if present)
    if let Some(model) = value.get("model") {
        new_body.insert("model".to_string(), model.clone());
    }

    // Map messages
    let mut new_messages = Vec::new();

    if let Some(system) = value.get("system")
        && let Some(content) = convert_anthropic_content_to_openai_message_content(system)
    {
        new_messages.push(json!({
            "role": "system",
            "content": content
        }));
    }

    if let Some(messages) = value.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            new_messages.extend(convert_anthropic_message_to_openai_messages(msg));
        }
    }

    if !new_messages.is_empty() {
        new_body.insert("messages".to_string(), Value::Array(new_messages));
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

    if let Some(stop_sequences) = value.get("stop_sequences") {
        new_body.insert("stop".to_string(), stop_sequences.clone());
    }

    // Map tools and tool_choice (for function calling)
    if let Some(tools) = value.get("tools").and_then(|tools| tools.as_array()) {
        new_body.insert(
            "tools".to_string(),
            Value::Array(
                tools
                    .iter()
                    .map(convert_anthropic_tool_to_openai_tool)
                    .collect(),
            ),
        );
    }
    if let Some(tool_choice) = value.get("tool_choice")
        && let Some(mapped_tool_choice) = convert_anthropic_tool_choice(tool_choice)
    {
        new_body.insert("tool_choice".to_string(), mapped_tool_choice);
        if let Some(disable_parallel_tool_use) = tool_choice
            .get("disable_parallel_tool_use")
            .and_then(Value::as_bool)
        {
            new_body.insert(
                "parallel_tool_calls".to_string(),
                Value::Bool(!disable_parallel_tool_use),
            );
        }
    }

    // Map response_format (for structured output)
    if let Some(response_format) = value.get("response_format") {
        new_body.insert("response_format".to_string(), response_format.clone());
    }

    match serde_json::to_vec(&new_body) {
        Ok(vec) => Bytes::from(vec),
        Err(_) => body.clone(),
    }
}

fn append_openai_message_content_as_anthropic_blocks(
    content: Option<&Value>,
    blocks: &mut Vec<Value>,
) {
    match content {
        Some(Value::String(text)) if !text.is_empty() => {
            blocks.push(json!({"type": "text", "text": text}));
        }
        Some(Value::Array(parts)) => {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            blocks.push(json!({"type": "text", "text": text}));
                        }
                    }
                    Some("output_text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str)
                            && !text.is_empty()
                        {
                            blocks.push(json!({"type": "text", "text": text}));
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn append_openai_tool_calls_as_anthropic_blocks(
    tool_calls: Option<&Value>,
    blocks: &mut Vec<Value>,
) {
    let Some(tool_calls) = tool_calls.and_then(Value::as_array) else {
        return;
    };

    for tool_call in tool_calls {
        let Some(function) = tool_call.get("function") else {
            continue;
        };
        let Some(name) = function.get("name").and_then(Value::as_str) else {
            continue;
        };
        let input = function
            .get("arguments")
            .and_then(Value::as_str)
            .and_then(|arguments| serde_json::from_str::<Value>(arguments).ok())
            .unwrap_or_else(|| json!({}));
        let id = tool_call
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("tool_call");
        let mut block = json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input
        });
        if let Some(extra_content) = tool_call.get("extra_content").cloned()
            && let Some(map) = block.as_object_mut()
        {
            map.insert("extra_content".to_string(), extra_content);
        }
        blocks.push(block);
    }
}

fn convert_anthropic_content_to_openai_message_content(content: &Value) -> Option<Value> {
    match content {
        Value::String(text) => Some(Value::String(text.clone())),
        Value::Array(blocks) => {
            let mut parts = Vec::new();
            for block in blocks {
                if let Some(part) = convert_anthropic_block_to_openai_part(block) {
                    parts.push(part);
                }
            }
            parts_to_openai_message_content(parts)
        }
        _ => None,
    }
}

fn convert_anthropic_message_to_openai_messages(message: &Value) -> Vec<Value> {
    let Some(role) = message.get("role").and_then(Value::as_str) else {
        return vec![message.clone()];
    };

    let Some(content) = message.get("content") else {
        return vec![message.clone()];
    };

    if !matches!(content, Value::Array(_)) {
        return vec![json!({
            "role": role,
            "content": content.clone()
        })];
    }

    match role {
        "assistant" => {
            let mut parts = Vec::new();
            let mut tool_calls = Vec::new();

            for block in content.as_array().into_iter().flatten() {
                match block.get("type").and_then(Value::as_str) {
                    Some("tool_use") => {
                        if let Some(tool_call) = convert_anthropic_tool_use_block(block) {
                            tool_calls.push(tool_call);
                        }
                    }
                    Some("thinking" | "redacted_thinking") => {}
                    _ => {
                        if let Some(part) = convert_anthropic_block_to_openai_part(block) {
                            parts.push(part);
                        }
                    }
                }
            }

            let mut mapped = Map::new();
            mapped.insert("role".to_string(), Value::String("assistant".to_string()));
            mapped.insert(
                "content".to_string(),
                parts_to_openai_message_content(parts).unwrap_or(Value::Null),
            );
            if !tool_calls.is_empty() {
                mapped.insert("tool_calls".to_string(), Value::Array(tool_calls));
            }
            vec![Value::Object(mapped)]
        }
        "user" => convert_anthropic_user_message_to_openai_messages(content),
        _ => vec![json!({
            "role": role,
            "content": convert_anthropic_content_to_openai_message_content(content)
                .unwrap_or_else(|| content.clone())
        })],
    }
}

fn convert_anthropic_user_message_to_openai_messages(content: &Value) -> Vec<Value> {
    let Some(blocks) = content.as_array() else {
        return vec![json!({"role": "user", "content": content.clone()})];
    };

    let mut messages = Vec::new();
    let mut pending_parts = Vec::new();

    for block in blocks {
        if matches!(
            block.get("type").and_then(Value::as_str),
            Some("tool_result")
        ) {
            if let Some(content) =
                parts_to_openai_message_content(std::mem::take(&mut pending_parts))
            {
                messages.push(json!({
                    "role": "user",
                    "content": content
                }));
            }
            if let Some(tool_message) = convert_anthropic_tool_result_block(block) {
                messages.push(tool_message);
            }
            continue;
        }

        if let Some(part) = convert_anthropic_block_to_openai_part(block) {
            pending_parts.push(part);
        }
    }

    if let Some(content) = parts_to_openai_message_content(pending_parts) {
        messages.push(json!({
            "role": "user",
            "content": content
        }));
    }

    messages
}

fn convert_anthropic_block_to_openai_part(block: &Value) -> Option<Value> {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => block.get("text").and_then(Value::as_str).map(|text| {
            json!({
                "type": "text",
                "text": text
            })
        }),
        Some("image") => convert_anthropic_image_block_to_openai_part(block),
        _ => None,
    }
}

fn convert_anthropic_image_block_to_openai_part(block: &Value) -> Option<Value> {
    let source = block.get("source")?;
    match source.get("type").and_then(Value::as_str) {
        Some("base64") => {
            let media_type = source.get("media_type").and_then(Value::as_str)?;
            let data = source.get("data").and_then(Value::as_str)?;
            Some(json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{media_type};base64,{data}")
                }
            }))
        }
        Some("url") => source.get("url").and_then(Value::as_str).map(|url| {
            json!({
                "type": "image_url",
                "image_url": {
                    "url": url
                }
            })
        }),
        _ => None,
    }
}

fn convert_anthropic_tool_use_block(block: &Value) -> Option<Value> {
    let id = block.get("id").and_then(Value::as_str)?;
    let name = block.get("name").and_then(Value::as_str)?;
    let arguments = block
        .get("input")
        .map(serialize_tool_arguments)
        .unwrap_or_else(|| "{}".to_string());

    let mut tool_call = json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments
        }
    });
    if let Some(extra_content) = block.get("extra_content").cloned()
        && let Some(map) = tool_call.as_object_mut()
    {
        map.insert("extra_content".to_string(), extra_content);
    }

    Some(tool_call)
}

fn convert_anthropic_tool_result_block(block: &Value) -> Option<Value> {
    let tool_call_id = block.get("tool_use_id").and_then(Value::as_str)?;
    let content = anthropic_tool_result_content_to_string(
        block.get("content"),
        block
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    );

    Some(json!({
        "role": "tool",
        "tool_call_id": tool_call_id,
        "content": content
    }))
}

fn anthropic_tool_result_content_to_string(content: Option<&Value>, is_error: bool) -> String {
    let Some(content) = content else {
        return String::new();
    };

    let serialized = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => {
            let text_blocks: Vec<String> = blocks
                .iter()
                .filter_map(|block| match block.get("type").and_then(Value::as_str) {
                    Some("text") => block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    _ => None,
                })
                .collect();
            if text_blocks.is_empty() {
                serde_json::to_string(content).unwrap_or_default()
            } else {
                text_blocks.join("\n\n")
            }
        }
        _ => serde_json::to_string(content).unwrap_or_default(),
    };

    if is_error {
        serde_json::to_string(&json!({
            "is_error": true,
            "content": serialized
        }))
        .unwrap_or(serialized)
    } else {
        serialized
    }
}

fn convert_anthropic_tool_to_openai_tool(tool: &Value) -> Value {
    let mut function = Map::new();
    if let Some(name) = tool.get("name") {
        function.insert("name".to_string(), name.clone());
    }
    if let Some(description) = tool.get("description") {
        function.insert("description".to_string(), description.clone());
    }
    if let Some(parameters) = tool.get("input_schema") {
        function.insert("parameters".to_string(), parameters.clone());
    }

    json!({
        "type": "function",
        "function": function
    })
}

fn convert_anthropic_tool_choice(tool_choice: &Value) -> Option<Value> {
    let choice_type = tool_choice.get("type").and_then(Value::as_str)?;
    match choice_type {
        "auto" | "none" => Some(Value::String(choice_type.to_string())),
        "any" => Some(Value::String("required".to_string())),
        "tool" => tool_choice.get("name").and_then(Value::as_str).map(|name| {
            json!({
                "type": "function",
                "function": {
                    "name": name
                }
            })
        }),
        _ => None,
    }
}

fn parts_to_openai_message_content(parts: Vec<Value>) -> Option<Value> {
    if parts.is_empty() {
        return None;
    }

    let text_parts: Option<Vec<&str>> = parts
        .iter()
        .map(|part| {
            (part.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| part.get("text").and_then(Value::as_str))
                .flatten()
        })
        .collect();

    if let Some(text_parts) = text_parts {
        return Some(Value::String(text_parts.join("\n\n")));
    }

    Some(Value::Array(parts))
}

fn serialize_tool_arguments(input: &Value) -> String {
    match input {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
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
    fn test_convert_openai_response_to_anthropic_tool_calls() {
        let openai_resp = json!({
            "id": "chatcmpl-tool",
            "model": "gemini-3.1-pro-preview",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "I'll inspect the repo.",
                    "tool_calls": [{
                        "id": "toolu_123",
                        "type": "function",
                        "function": {
                            "name": "run_command",
                            "arguments": "{\"cmd\":\"pwd\"}"
                        },
                        "extra_content": {
                            "google": {
                                "thought_signature": "sig_123"
                            }
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5
            }
        });

        let body = Bytes::from(serde_json::to_vec(&openai_resp).unwrap());
        let converted = convert_openai_response_to_anthropic(body);
        let val: serde_json::Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(val["stop_reason"], "tool_use");
        let content = val["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "I'll inspect the repo.");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["id"], "toolu_123");
        assert_eq!(content[1]["name"], "run_command");
        assert_eq!(content[1]["input"]["cmd"], "pwd");
        assert_eq!(
            content[1]["extra_content"]["google"]["thought_signature"],
            "sig_123"
        );
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

    #[test]
    fn test_convert_anthropic_to_openai_with_tools_and_tool_results() {
        let anthropic_req = json!({
            "model": "gemini-3.1-pro-preview",
            "system": [{"type": "text", "text": "You are Claude Code."}],
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {"type": "text", "text": "I'll inspect the repo."},
                        {
                            "type": "tool_use",
                            "id": "toolu_123",
                            "name": "run_command",
                            "input": {"cmd": "pwd"},
                            "extra_content": {
                                "google": {
                                    "thought_signature": "sig_123"
                                }
                            }
                        }
                    ]
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "toolu_123",
                            "content": [{"type": "text", "text": "/workspace/apex"}]
                        },
                        {"type": "text", "text": "Continue"}
                    ]
                }
            ],
            "tools": [{
                "name": "run_command",
                "description": "Execute a shell command",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "cmd": {"type": "string"}
                    },
                    "required": ["cmd"]
                }
            }],
            "tool_choice": {
                "type": "tool",
                "name": "run_command",
                "disable_parallel_tool_use": true
            },
            "stop_sequences": ["Observation:"]
        });

        let body = Bytes::from(serde_json::to_vec(&anthropic_req).unwrap());
        let converted = convert_anthropic_to_openai(&body);
        let val: serde_json::Value = serde_json::from_slice(&converted).unwrap();

        let messages = val["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are Claude Code.");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "I'll inspect the repo.");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "toolu_123");
        assert_eq!(
            messages[1]["tool_calls"][0]["function"]["arguments"],
            "{\"cmd\":\"pwd\"}"
        );
        assert_eq!(
            messages[1]["tool_calls"][0]["extra_content"]["google"]["thought_signature"],
            "sig_123"
        );
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "toolu_123");
        assert_eq!(messages[2]["content"], "/workspace/apex");
        assert_eq!(messages[3]["role"], "user");
        assert_eq!(messages[3]["content"], "Continue");

        assert_eq!(val["tools"][0]["type"], "function");
        assert_eq!(val["tools"][0]["function"]["name"], "run_command");
        assert_eq!(val["tool_choice"]["type"], "function");
        assert_eq!(val["tool_choice"]["function"]["name"], "run_command");
        assert_eq!(val["parallel_tool_calls"], false);
        assert_eq!(val["stop"][0], "Observation:");
    }

    #[test]
    fn test_convert_openai_stream_to_anthropic_tool_calls() {
        let chunks = vec![
            Ok::<Bytes, reqwest::Error>(Bytes::from(concat!(
                "data: ",
                "{\"id\":\"chatcmpl-tool\",\"model\":\"gemini-3.1-pro-preview\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"I'll inspect the repo.\"}}]}\n\n"
            ))),
            Ok::<Bytes, reqwest::Error>(Bytes::from(concat!(
                "data: ",
                "{\"id\":\"chatcmpl-tool\",\"model\":\"gemini-3.1-pro-preview\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"toolu_123\",\"type\":\"function\",\"function\":{\"name\":\"run_command\",\"arguments\":\"{\\\"cmd\\\":\\\"pwd\\\"}\"},\"extra_content\":{\"google\":{\"thought_signature\":\"sig_123\"}}}]}}]}\n\n"
            ))),
            Ok::<Bytes, reqwest::Error>(Bytes::from(concat!(
                "data: ",
                "{\"id\":\"chatcmpl-tool\",\"model\":\"gemini-3.1-pro-preview\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
                "data: [DONE]\n\n"
            ))),
        ];

        let converted_stream = convert_openai_stream_to_anthropic(stream::iter(chunks));
        let output = futures::executor::block_on(async move {
            converted_stream
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<String>()
        });

        assert!(output.contains("event: message_start"));
        assert!(output.contains("\"type\":\"tool_use\""));
        assert!(output.contains("\"name\":\"run_command\""));
        assert!(output.contains("\"thought_signature\":\"sig_123\""));
        assert!(output.contains("\"type\":\"input_json_delta\""));
        assert!(output.contains("\"partial_json\":\"{\\\"cmd\\\":\\\"pwd\\\"}\""));
        assert!(output.contains("\"stop_reason\":\"tool_use\""));
        assert!(output.contains("event: message_stop"));
    }

    #[test]
    fn test_convert_openai_stream_to_anthropic_text_without_finish_reason_chunk() {
        let chunks = vec![Ok::<Bytes, reqwest::Error>(Bytes::from(concat!(
            "data: ",
            "{\"id\":\"chatcmpl-text\",\"model\":\"gemini-3.1-pro-preview\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello from stream\"}}]}\n\n",
            "data: [DONE]\n\n"
        )))];

        let converted_stream = convert_openai_stream_to_anthropic(stream::iter(chunks));
        let output = futures::executor::block_on(async move {
            converted_stream
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .map(|item| String::from_utf8(item.unwrap().to_vec()).unwrap())
                .collect::<String>()
        });

        assert!(output.contains("event: content_block_start"));
        assert!(output.contains("Hello from stream"));
        assert!(output.contains("event: content_block_stop"));
        assert!(output.contains("\"stop_reason\":\"end_turn\""));
        assert!(output.contains("event: message_stop"));
    }
}
