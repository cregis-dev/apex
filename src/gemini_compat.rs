use axum::body::{Body, Bytes};
use axum::response::Response;
use futures::Stream;
use moka::sync::Cache;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
struct ReplayTurn {
    prior_messages: Vec<Value>,
    assistant_content: Vec<Value>,
}

pub struct GeminiAnthropicReplayCache {
    turns: Cache<String, Arc<ReplayTurn>>,
}

impl Default for GeminiAnthropicReplayCache {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiAnthropicReplayCache {
    pub fn new() -> Self {
        Self {
            turns: Cache::builder()
                .time_to_live(Duration::from_secs(10 * 60))
                .max_capacity(2_048)
                .build(),
        }
    }

    pub fn augment_request(&self, team_id: &str, body: &Bytes) -> Bytes {
        let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
            return body.clone();
        };
        let request_model = request_model_name(&value);
        let Some(messages) = value.get("messages").and_then(Value::as_array) else {
            return body.clone();
        };

        let mut referenced_tool_use_ids = Vec::new();
        for message in messages {
            let Some(content) = message.get("content").and_then(Value::as_array) else {
                continue;
            };
            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("tool_result")
                    && let Some(tool_use_id) = block.get("tool_use_id").and_then(Value::as_str)
                {
                    referenced_tool_use_ids.push(tool_use_id.to_string());
                }
            }
        }

        if referenced_tool_use_ids.is_empty() {
            return body.clone();
        }

        let assistant_index = messages.iter().position(|message| {
            message.get("role").and_then(Value::as_str) == Some("assistant")
                && message
                    .get("content")
                    .and_then(Value::as_array)
                    .map(|content| {
                        content.iter().any(|block| {
                            block.get("type").and_then(Value::as_str) == Some("tool_use")
                                && block.get("id").and_then(Value::as_str).is_some_and(|id| {
                                    referenced_tool_use_ids.iter().any(|tool_id| tool_id == id)
                                })
                        })
                    })
                    .unwrap_or(false)
        });

        let Some(replay_turn) = messages
            .iter()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
            .filter_map(|message| message.get("content").and_then(Value::as_array))
            .flat_map(|content| content.iter())
            .find_map(|block| {
                let tool_use_id = block.get("id").and_then(Value::as_str)?;
                if !referenced_tool_use_ids
                    .iter()
                    .any(|candidate| candidate == tool_use_id)
                {
                    return None;
                }
                tool_use_cache_key(team_id, request_model.as_deref(), block)
                    .and_then(|key| self.turns.get(&key))
            })
        else {
            return body.clone();
        };

        let suffix_messages = assistant_index
            .map(|idx| messages[(idx + 1)..].to_vec())
            .unwrap_or_else(|| messages.to_vec());

        let mut rebuilt_messages = replay_turn.prior_messages.clone();
        rebuilt_messages.push(json!({
            "role": "assistant",
            "content": replay_turn.assistant_content
        }));
        rebuilt_messages.extend(suffix_messages);

        if let Some(obj) = value.as_object_mut() {
            obj.insert("messages".to_string(), Value::Array(rebuilt_messages));
        } else {
            return body.clone();
        }

        serde_json::to_vec(&value)
            .map(Bytes::from)
            .unwrap_or_else(|_| body.clone())
    }

    pub async fn wrap_response(
        self: Arc<Self>,
        team_id: String,
        request_body: Bytes,
        response: Response<Body>,
    ) -> Response<Body> {
        let is_sse = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.contains("text/event-stream"))
            .unwrap_or(false);
        let (parts, body) = response.into_parts();

        if is_sse {
            let recorder = Arc::new(Mutex::new(StreamReplayRecorder::new(
                self,
                team_id,
                request_body,
            )));
            let stream = body.into_data_stream();
            let wrapped = ReplayRecordingStream {
                inner: stream,
                recorder,
            };
            Response::from_parts(parts, Body::from_stream(wrapped))
        } else {
            let bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
                Ok(bytes) => bytes,
                Err(_) => return Response::from_parts(parts, Body::empty()),
            };
            self.store_from_anthropic_response(&team_id, &request_body, &bytes);
            self.forget_completed_turn(&team_id, &request_body);
            Response::from_parts(parts, Body::from(bytes))
        }
    }

    fn store_from_anthropic_response(
        &self,
        team_id: &str,
        request_body: &Bytes,
        response_body: &Bytes,
    ) {
        let Ok(request) = serde_json::from_slice::<Value>(request_body) else {
            return;
        };
        let request_model = request_model_name(&request);
        let Some(prior_messages) = request.get("messages").and_then(Value::as_array) else {
            return;
        };
        let Ok(response) = serde_json::from_slice::<Value>(response_body) else {
            return;
        };
        let Some(content) = response.get("content").and_then(Value::as_array) else {
            return;
        };
        if !content.iter().any(is_tool_use_block) {
            return;
        }

        let replay_turn = Arc::new(ReplayTurn {
            prior_messages: prior_messages.to_vec(),
            assistant_content: content.to_vec(),
        });

        for block in content.iter().filter(|block| is_tool_use_block(block)) {
            let Some(key) = tool_use_cache_key(team_id, request_model.as_deref(), block) else {
                continue;
            };
            self.turns.insert(key, replay_turn.clone());
        }
    }

    fn forget_completed_turn(&self, team_id: &str, request_body: &Bytes) {
        for key in referenced_tool_use_cache_keys(team_id, request_body) {
            self.turns.invalidate(&key);
        }
    }
}

struct ReplayRecordingStream<S> {
    inner: S,
    recorder: Arc<Mutex<StreamReplayRecorder>>,
}

impl<S, E> Stream for ReplayRecordingStream<S>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
{
    type Item = Result<Bytes, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                if let Ok(mut recorder) = self.recorder.lock() {
                    recorder.process_chunk(&bytes);
                }
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(None) => {
                if let Ok(mut recorder) = self.recorder.lock() {
                    recorder.finish();
                }
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

struct StreamReplayRecorder {
    cache: Arc<GeminiAnthropicReplayCache>,
    team_id: String,
    request_body: Bytes,
    buffer: String,
    blocks: BTreeMap<usize, RecordedContentBlock>,
}

impl StreamReplayRecorder {
    fn new(cache: Arc<GeminiAnthropicReplayCache>, team_id: String, request_body: Bytes) -> Self {
        Self {
            cache,
            team_id,
            request_body,
            buffer: String::new(),
            blocks: BTreeMap::new(),
        }
    }

    fn process_chunk(&mut self, chunk: &Bytes) {
        let Ok(text) = std::str::from_utf8(chunk) else {
            return;
        };
        self.buffer.push_str(text);

        let mut start = 0usize;
        while let Some(end) = self.buffer[start..].find('\n') {
            let line = self.buffer[start..start + end].trim().to_string();
            self.process_line(&line);
            start += end + 1;
        }

        if start > 0 {
            self.buffer.drain(0..start);
        }
    }

    fn process_line(&mut self, line: &str) {
        let Some(data) = line.strip_prefix("data: ") else {
            return;
        };
        if data == "[DONE]" || data.is_empty() {
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return;
        };

        match value.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                let Some(index) = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize)
                else {
                    return;
                };
                let Some(content_block) = value.get("content_block") else {
                    return;
                };
                match content_block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        self.blocks
                            .insert(index, RecordedContentBlock::Text(String::new()));
                    }
                    Some("tool_use") => {
                        let Some(id) = content_block.get("id").and_then(Value::as_str) else {
                            return;
                        };
                        let Some(name) = content_block.get("name").and_then(Value::as_str) else {
                            return;
                        };
                        self.blocks.insert(
                            index,
                            RecordedContentBlock::ToolUse {
                                id: id.to_string(),
                                name: name.to_string(),
                                input_json: String::new(),
                                extra_content: content_block.get("extra_content").cloned(),
                            },
                        );
                    }
                    _ => {}
                }
            }
            Some("content_block_delta") => {
                let Some(index) = value
                    .get("index")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize)
                else {
                    return;
                };
                let Some(delta) = value.get("delta") else {
                    return;
                };
                match self.blocks.get_mut(&index) {
                    Some(RecordedContentBlock::Text(text)) => {
                        if delta.get("type").and_then(Value::as_str) == Some("text_delta")
                            && let Some(part) = delta.get("text").and_then(Value::as_str)
                        {
                            text.push_str(part);
                        }
                    }
                    Some(RecordedContentBlock::ToolUse { input_json, .. }) => {
                        if delta.get("type").and_then(Value::as_str) == Some("input_json_delta")
                            && let Some(part) = delta.get("partial_json").and_then(Value::as_str)
                        {
                            input_json.push_str(part);
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }

    fn finish(&mut self) {
        if self.blocks.is_empty() {
            return;
        }

        let mut content = Vec::new();
        for block in self.blocks.values() {
            match block {
                RecordedContentBlock::Text(text) if !text.is_empty() => {
                    content.push(json!({
                        "type": "text",
                        "text": text
                    }));
                }
                RecordedContentBlock::ToolUse {
                    id,
                    name,
                    input_json,
                    extra_content,
                } => {
                    let input =
                        serde_json::from_str::<Value>(input_json).unwrap_or_else(|_| json!({}));
                    let mut block = json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input
                    });
                    if let Some(extra_content) = extra_content.clone()
                        && let Some(map) = block.as_object_mut()
                    {
                        map.insert("extra_content".to_string(), extra_content);
                    }
                    content.push(block);
                }
                _ => {}
            }
        }

        if content.iter().any(is_tool_use_block)
            && let Ok(bytes) = serde_json::to_vec(&json!({ "content": content }))
        {
            self.cache.store_from_anthropic_response(
                &self.team_id,
                &self.request_body,
                &Bytes::from(bytes),
            );
        }
        self.cache
            .forget_completed_turn(&self.team_id, &self.request_body);
    }
}

enum RecordedContentBlock {
    Text(String),
    ToolUse {
        id: String,
        name: String,
        input_json: String,
        extra_content: Option<Value>,
    },
}

fn is_tool_use_block(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("tool_use")
}

fn request_model_name(request: &Value) -> Option<String> {
    request
        .get("model")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn referenced_tool_use_cache_keys(team_id: &str, body: &Bytes) -> Vec<String> {
    let Ok(request) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    let request_model = request_model_name(&request);
    let Some(messages) = request.get("messages").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut referenced_tool_use_ids = std::collections::BTreeSet::new();
    for message in messages {
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(Value::as_str) == Some("tool_result")
                && let Some(tool_use_id) = block.get("tool_use_id").and_then(Value::as_str)
            {
                referenced_tool_use_ids.insert(tool_use_id.to_string());
            }
        }
    }

    if referenced_tool_use_ids.is_empty() {
        return Vec::new();
    }

    let mut keys = Vec::new();
    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in content {
            let Some(tool_use_id) = block.get("id").and_then(Value::as_str) else {
                continue;
            };
            if !referenced_tool_use_ids.contains(tool_use_id) {
                continue;
            }
            if let Some(key) = tool_use_cache_key(team_id, request_model.as_deref(), block) {
                keys.push(key);
            }
        }
    }
    keys
}

fn tool_use_cache_key(team_id: &str, model: Option<&str>, block: &Value) -> Option<String> {
    if !is_tool_use_block(block) {
        return None;
    }

    let tool_use_id = block.get("id").and_then(Value::as_str)?;
    let name = block.get("name").and_then(Value::as_str)?;
    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
    let canonical = json!({
        "id": tool_use_id,
        "name": name,
        "input": input
    });
    let serialized = serde_json::to_string(&canonical).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serialized.hash(&mut hasher);
    let fingerprint = format!("{:016x}", hasher.finish());

    Some(format!(
        "{team_id}:{}:{tool_use_id}:{fingerprint}",
        model.unwrap_or("")
    ))
}

pub fn gemini_replay_missing_signature(body: &Bytes) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return false;
    };

    let mut referenced_tool_use_ids = std::collections::BTreeSet::new();
    for message in messages {
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(Value::as_str) == Some("tool_result")
                && let Some(tool_use_id) = block.get("tool_use_id").and_then(Value::as_str)
            {
                referenced_tool_use_ids.insert(tool_use_id.to_string());
            }
        }
    }

    if referenced_tool_use_ids.is_empty() {
        return false;
    }

    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let Some(tool_use_id) = block.get("id").and_then(Value::as_str) else {
                continue;
            };
            if !referenced_tool_use_ids.contains(tool_use_id) {
                continue;
            }

            let has_signature = block
                .get("extra_content")
                .and_then(|value| value.get("google"))
                .and_then(|value| value.get("thought_signature"))
                .and_then(Value::as_str)
                .map(|value| !value.is_empty())
                .unwrap_or(false);
            if !has_signature {
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn augment_request_restores_cached_prefix_and_signature() {
        let cache = GeminiAnthropicReplayCache::new();
        let first_turn_request = Bytes::from(
            serde_json::to_vec(&json!({
                "model": "gemini-3.1-pro-preview",
                "messages": [
                    {"role": "user", "content": "Use the tool."}
                ]
            }))
            .unwrap(),
        );
        let first_turn_response = Bytes::from(
            serde_json::to_vec(&json!({
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "run_command",
                        "input": {"cmd": "pwd"},
                        "extra_content": {
                            "google": {
                                "thought_signature": "sig_123"
                            }
                        }
                    }
                ]
            }))
            .unwrap(),
        );
        cache.store_from_anthropic_response("team", &first_turn_request, &first_turn_response);

        let compressed_followup = Bytes::from(
            serde_json::to_vec(&json!({
                "model": "gemini-3.1-pro-preview",
                "messages": [
                    {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "tool_use",
                                "id": "toolu_1",
                                "name": "run_command",
                                "input": {"cmd": "pwd"}
                            }
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            {"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"},
                            {"type": "text", "text": "Continue"}
                        ]
                    }
                ]
            }))
            .unwrap(),
        );

        let rebuilt = cache.augment_request("team", &compressed_followup);
        let rebuilt: Value = serde_json::from_slice(&rebuilt).unwrap();
        let messages = rebuilt["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(
            messages[1]["content"][0]["extra_content"]["google"]["thought_signature"],
            "sig_123"
        );
    }

    #[test]
    fn missing_signature_detector_only_flags_tool_result_followups() {
        let body = Bytes::from(
            serde_json::to_vec(&json!({
                "messages": [
                    {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "tool_use",
                                "id": "toolu_1",
                                "name": "run_command",
                                "input": {"cmd": "pwd"}
                            }
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
        assert!(gemini_replay_missing_signature(&body));
    }

    #[test]
    fn augment_request_isolates_parallel_team_sessions() {
        let cache = GeminiAnthropicReplayCache::new();

        let first_turn_request_a = Bytes::from(
            serde_json::to_vec(&json!({
                "model": "gemini-3.1-pro-preview",
                "messages": [
                    {"role": "user", "content": "session-a"}
                ]
            }))
            .unwrap(),
        );
        let first_turn_response_a = Bytes::from(
            serde_json::to_vec(&json!({
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_same",
                        "name": "run_command",
                        "input": {"cmd": "pwd"},
                        "extra_content": {
                            "google": {"thought_signature": "sig_a"}
                        }
                    }
                ]
            }))
            .unwrap(),
        );
        cache.store_from_anthropic_response("team", &first_turn_request_a, &first_turn_response_a);

        let first_turn_request_b = Bytes::from(
            serde_json::to_vec(&json!({
                "model": "gemini-3.1-pro-preview",
                "messages": [
                    {"role": "user", "content": "session-b"}
                ]
            }))
            .unwrap(),
        );
        let first_turn_response_b = Bytes::from(
            serde_json::to_vec(&json!({
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_same",
                        "name": "run_command",
                        "input": {"cmd": "ls"},
                        "extra_content": {
                            "google": {"thought_signature": "sig_b"}
                        }
                    }
                ]
            }))
            .unwrap(),
        );
        cache.store_from_anthropic_response("team", &first_turn_request_b, &first_turn_response_b);

        let compressed_followup_b = Bytes::from(
            serde_json::to_vec(&json!({
                "model": "gemini-3.1-pro-preview",
                "messages": [
                    {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "tool_use",
                                "id": "toolu_same",
                                "name": "run_command",
                                "input": {"cmd": "ls"}
                            }
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            {"type": "tool_result", "tool_use_id": "toolu_same", "content": "ok"}
                        ]
                    }
                ]
            }))
            .unwrap(),
        );

        let rebuilt = cache.augment_request("team", &compressed_followup_b);
        let rebuilt: Value = serde_json::from_slice(&rebuilt).unwrap();
        let messages = rebuilt["messages"].as_array().unwrap();
        assert_eq!(messages[0]["content"], "session-b");
        assert_eq!(
            messages[1]["content"][0]["extra_content"]["google"]["thought_signature"],
            "sig_b"
        );
    }

    #[test]
    fn referenced_turn_keys_are_invalidated_after_completion() {
        let cache = GeminiAnthropicReplayCache::new();
        let first_turn_request = Bytes::from(
            serde_json::to_vec(&json!({
                "model": "gemini-3.1-pro-preview",
                "messages": [
                    {"role": "user", "content": "Use the tool."}
                ]
            }))
            .unwrap(),
        );
        let first_turn_response = Bytes::from(
            serde_json::to_vec(&json!({
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_1",
                        "name": "run_command",
                        "input": {"cmd": "pwd"},
                        "extra_content": {
                            "google": {"thought_signature": "sig_123"}
                        }
                    }
                ]
            }))
            .unwrap(),
        );
        cache.store_from_anthropic_response("team", &first_turn_request, &first_turn_response);

        let followup = Bytes::from(
            serde_json::to_vec(&json!({
                "model": "gemini-3.1-pro-preview",
                "messages": [
                    {
                        "role": "assistant",
                        "content": [
                            {
                                "type": "tool_use",
                                "id": "toolu_1",
                                "name": "run_command",
                                "input": {"cmd": "pwd"}
                            }
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

        assert!(!referenced_tool_use_cache_keys("team", &followup).is_empty());
        let rebuilt = cache.augment_request("team", &followup);
        assert_ne!(rebuilt, followup);
        cache.forget_completed_turn("team", &rebuilt);
        let rebuilt_again = cache.augment_request("team", &followup);
        assert_eq!(rebuilt_again, followup);
    }
}
