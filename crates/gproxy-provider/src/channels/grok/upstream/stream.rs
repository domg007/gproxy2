use rand::RngExt as _;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

impl GrokLineStreamParser {
    pub(super) fn new(model: String, tool_names: Vec<String>) -> Self {
        Self {
            model,
            tool_stream: GrokToolCallStreamState {
                enabled: !tool_names.is_empty(),
                allowed_names: tool_names,
                ..GrokToolCallStreamState::default()
            },
            response_id: random_chat_completion_id(),
            created: unix_timestamp_secs(),
            fingerprint: None,
            saw_chunk: false,
            saw_visible_output: false,
            final_message: None,
            final_message_emitted: false,
        }
    }

    pub(super) fn on_line(
        &mut self,
        line: &[u8],
    ) -> Result<Vec<OpenAiChatSseEvent>, UpstreamError> {
        if line.is_empty() {
            return Ok(Vec::new());
        }
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return Ok(Vec::new());
        };
        let Some(response) = value.pointer("/result/response").and_then(Value::as_object) else {
            return Ok(Vec::new());
        };

        if let Some(response_id) = response
            .get("responseId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            self.response_id = response_id.trim().to_string();
        }
        if let Some(llm_hash) = response
            .get("llmInfo")
            .and_then(Value::as_object)
            .and_then(|value| value.get("modelHash"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            self.fingerprint = Some(llm_hash.trim().to_string());
        }

        if let Some(model_response) = response.get("modelResponse").and_then(Value::as_object) {
            if let Some(response_id) = model_response
                .get("responseId")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                self.response_id = response_id.trim().to_string();
            }
            if let Some(llm_hash) = model_response
                .get("metadata")
                .and_then(Value::as_object)
                .and_then(|value| value.get("llm_info"))
                .and_then(Value::as_object)
                .and_then(|value| value.get("modelHash"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                self.fingerprint = Some(llm_hash.trim().to_string());
            }
            if let Some(message) = model_response
                .get("message")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                self.final_message = Some(message.to_string());
            }
        }

        let Some(token) = response
            .get("token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        else {
            return Ok(Vec::new());
        };

        let is_thinking = response
            .get("isThinking")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut events = Vec::new();
        if is_thinking {
            events.extend(self.push_reasoning_token(token));
        } else if self.tool_stream.enabled {
            for item in self.tool_stream.push(token) {
                match item {
                    GrokToolStreamEvent::Text(text) => {
                        if !text.is_empty() {
                            events.push(self.content_chunk(text));
                            self.saw_visible_output = true;
                        }
                    }
                    GrokToolStreamEvent::Tool(tool_call) => {
                        events.push(self.tool_call_chunk(tool_call));
                        self.saw_visible_output = true;
                    }
                }
            }
        } else if !token.is_empty() {
            events.push(self.content_chunk(token.to_string()));
            self.saw_visible_output = true;
        }
        Ok(events)
    }

    pub(super) fn finish(&mut self) -> Vec<OpenAiChatSseEvent> {
        let mut events = Vec::new();
        if self.tool_stream.enabled {
            for item in self.tool_stream.finish() {
                match item {
                    GrokToolStreamEvent::Text(text) => {
                        if !text.is_empty() {
                            events.push(self.content_chunk(text));
                            self.saw_visible_output = true;
                        }
                    }
                    GrokToolStreamEvent::Tool(tool_call) => {
                        events.push(self.tool_call_chunk(tool_call));
                        self.saw_visible_output = true;
                    }
                }
            }
        }
        if !self.saw_visible_output
            && !self.final_message_emitted
            && let Some(message) = self.final_message.clone()
        {
            for item in parse_tool_calls_from_full_text(
                message.as_str(),
                self.tool_stream.allowed_names.as_slice(),
                self.tool_stream.enabled,
            ) {
                match item {
                    GrokToolStreamEvent::Text(text) => {
                        if !text.is_empty() {
                            events.push(self.content_chunk(text));
                            self.saw_visible_output = true;
                        }
                    }
                    GrokToolStreamEvent::Tool(tool_call) => {
                        events.push(self.tool_call_chunk(tool_call));
                        self.saw_visible_output = true;
                    }
                }
            }
            self.final_message_emitted = true;
        }
        if !self.saw_chunk {
            events.push(self.content_chunk(String::new()));
        }
        events.push(self.finish_chunk());
        events.push(OpenAiChatSseEvent::Done);
        events
    }

    fn push_reasoning_token(&mut self, token: &str) -> Vec<OpenAiChatSseEvent> {
        if token.is_empty() {
            return Vec::new();
        }
        let mut delta = Map::new();
        delta.insert("role".to_string(), Value::String("assistant".to_string()));
        delta.insert(
            "reasoning_content".to_string(),
            Value::String(token.to_string()),
        );
        vec![self.chunk_event(delta, None)]
    }

    fn content_chunk(&mut self, text: String) -> OpenAiChatSseEvent {
        let mut delta = Map::new();
        delta.insert("role".to_string(), Value::String("assistant".to_string()));
        delta.insert("content".to_string(), Value::String(text));
        self.chunk_event(delta, None)
    }

    fn tool_call_chunk(&mut self, tool_call: GrokToolCall) -> OpenAiChatSseEvent {
        let mut tool = Map::new();
        tool.insert("index".to_string(), Value::from(tool_call.index));
        tool.insert("id".to_string(), Value::String(tool_call.id));
        tool.insert(
            "function".to_string(),
            json!({
                "name": tool_call.name,
                "arguments": tool_call.arguments,
            }),
        );
        tool.insert("type".to_string(), Value::String("function".to_string()));

        let mut delta = Map::new();
        delta.insert("role".to_string(), Value::String("assistant".to_string()));
        delta.insert(
            "tool_calls".to_string(),
            Value::Array(vec![Value::Object(tool)]),
        );
        self.chunk_event(delta, None)
    }

    fn finish_chunk(&mut self) -> OpenAiChatSseEvent {
        self.chunk_event(
            Map::new(),
            Some(if self.tool_stream.saw_tool_call {
                "tool_calls"
            } else {
                "stop"
            }),
        )
    }

    fn chunk_event(
        &mut self,
        delta: Map<String, Value>,
        finish_reason: Option<&str>,
    ) -> OpenAiChatSseEvent {
        self.saw_chunk = true;

        let mut choice = Map::new();
        choice.insert("delta".to_string(), Value::Object(delta));
        if let Some(finish_reason) = finish_reason {
            choice.insert(
                "finish_reason".to_string(),
                Value::String(finish_reason.to_string()),
            );
        }
        choice.insert("index".to_string(), Value::from(0_u64));

        let mut chunk = Map::new();
        chunk.insert("id".to_string(), Value::String(self.response_id.clone()));
        chunk.insert(
            "choices".to_string(),
            Value::Array(vec![Value::Object(choice)]),
        );
        chunk.insert("created".to_string(), Value::from(self.created));
        chunk.insert("model".to_string(), Value::String(self.model.clone()));
        chunk.insert(
            "object".to_string(),
            Value::String("chat.completion.chunk".to_string()),
        );
        if let Some(fingerprint) = self
            .fingerprint
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            chunk.insert(
                "system_fingerprint".to_string(),
                Value::String(fingerprint.to_string()),
            );
        }
        OpenAiChatSseEvent::Chunk(Value::Object(chunk))
    }
}

impl GrokToolCallStreamState {
    fn push(&mut self, chunk: &str) -> Vec<GrokToolStreamEvent> {
        if !self.enabled || chunk.is_empty() {
            return vec![GrokToolStreamEvent::Text(chunk.to_string())];
        }
        let mut events = Vec::new();
        let mut data = format!("{}{}", self.partial, chunk);
        self.partial.clear();
        while !data.is_empty() {
            match self.state {
                GrokToolParserState::Text => {
                    if let Some(start) = data.find("<tool_call>") {
                        let before = data[..start].to_string();
                        if !before.is_empty() {
                            events.push(GrokToolStreamEvent::Text(before));
                        }
                        data = data[start + "<tool_call>".len()..].to_string();
                        self.state = GrokToolParserState::Tool;
                    } else {
                        let keep = suffix_prefix_len(data.as_str(), "<tool_call>");
                        let emit_len = data.len().saturating_sub(keep);
                        if emit_len > 0 {
                            events.push(GrokToolStreamEvent::Text(data[..emit_len].to_string()));
                        }
                        self.partial = data[emit_len..].to_string();
                        break;
                    }
                }
                GrokToolParserState::Tool => {
                    if let Some(end) = data.find("</tool_call>") {
                        self.tool_buffer.push_str(&data[..end]);
                        if let Some(tool_call) = parse_tool_call_block(
                            self.tool_buffer.as_str(),
                            self.allowed_names.as_slice(),
                            self.next_index,
                        ) {
                            self.next_index = self.next_index.saturating_add(1);
                            self.saw_tool_call = true;
                            events.push(GrokToolStreamEvent::Tool(tool_call));
                        }
                        self.tool_buffer.clear();
                        data = data[end + "</tool_call>".len()..].to_string();
                        self.state = GrokToolParserState::Text;
                    } else {
                        let keep = suffix_prefix_len(data.as_str(), "</tool_call>");
                        let emit_len = data.len().saturating_sub(keep);
                        if emit_len > 0 {
                            self.tool_buffer.push_str(&data[..emit_len]);
                        }
                        self.partial = data[emit_len..].to_string();
                        break;
                    }
                }
            }
        }
        events
    }

    fn finish(&mut self) -> Vec<GrokToolStreamEvent> {
        if !self.enabled {
            if self.partial.is_empty() {
                return Vec::new();
            }
            let tail = std::mem::take(&mut self.partial);
            return vec![GrokToolStreamEvent::Text(tail)];
        }

        let mut events = Vec::new();
        if self.state == GrokToolParserState::Text {
            if !self.partial.is_empty() {
                events.push(GrokToolStreamEvent::Text(std::mem::take(&mut self.partial)));
            }
            return events;
        }

        let raw = format!("{}{}", self.tool_buffer, self.partial);
        self.tool_buffer.clear();
        self.partial.clear();
        self.state = GrokToolParserState::Text;
        if let Some(tool_call) =
            parse_tool_call_block(raw.as_str(), self.allowed_names.as_slice(), self.next_index)
        {
            self.next_index = self.next_index.saturating_add(1);
            self.saw_tool_call = true;
            events.push(GrokToolStreamEvent::Tool(tool_call));
        } else if !raw.is_empty() {
            events.push(GrokToolStreamEvent::Text(format!("<tool_call>{raw}")));
        }
        events
    }
}

fn parse_tool_calls_from_full_text(
    content: &str,
    allowed_names: &[String],
    enabled: bool,
) -> Vec<GrokToolStreamEvent> {
    if !enabled || !content.contains("<tool_call>") {
        return vec![GrokToolStreamEvent::Text(content.to_string())];
    }
    let mut state = GrokToolCallStreamState {
        enabled: true,
        allowed_names: allowed_names.to_vec(),
        ..GrokToolCallStreamState::default()
    };
    let mut out = state.push(content);
    out.extend(state.finish());
    out
}

fn parse_tool_call_block(
    raw_json: &str,
    allowed_names: &[String],
    index: u32,
) -> Option<GrokToolCall> {
    let parsed = parse_tool_call_json(raw_json)?;
    let name = parsed.get("name")?.as_str()?.trim().to_string();
    if !allowed_names.is_empty()
        && !allowed_names
            .iter()
            .any(|item| item.eq_ignore_ascii_case(name.as_str()))
    {
        return None;
    }
    let arguments = match parsed.get("arguments") {
        Some(Value::String(value)) => value.to_string(),
        Some(value) => serde_json::to_string(value).ok()?,
        None => "{}".to_string(),
    };
    Some(GrokToolCall {
        id: format!("call_{}", random_hex(24)),
        name,
        arguments,
        index,
    })
}

fn parse_tool_call_json(raw_json: &str) -> Option<Value> {
    serde_json::from_str::<Value>(raw_json)
        .ok()
        .filter(Value::is_object)
        .or_else(|| {
            let cleaned = repair_json_fragment(raw_json);
            serde_json::from_str::<Value>(cleaned.as_str())
                .ok()
                .filter(Value::is_object)
        })
}

fn repair_json_fragment(raw_json: &str) -> String {
    let mut cleaned = raw_json.trim().to_string();
    if cleaned.starts_with("```") {
        cleaned = cleaned
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string();
    }
    if let Some(start) = cleaned.find('{')
        && let Some(end) = cleaned.rfind('}')
        && end >= start
    {
        cleaned = cleaned[start..=end].to_string();
    }
    cleaned = cleaned.replace("\r\n", " ").replace('\n', " ");
    loop {
        let next = cleaned.replace(",}", "}").replace(",]", "]");
        if next == cleaned {
            break;
        }
        cleaned = next;
    }
    let open = cleaned.chars().filter(|ch| *ch == '{').count();
    let close = cleaned.chars().filter(|ch| *ch == '}').count();
    if open > close {
        cleaned.push_str("}".repeat(open - close).as_str());
    }
    cleaned
}

pub(super) fn next_line(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let newline = buffer.iter().position(|byte| *byte == b'\n')?;
    let mut line = buffer.drain(..=newline).collect::<Vec<_>>();
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    Some(line)
}

fn suffix_prefix_len(text: &str, tag: &str) -> usize {
    let max_keep = text.len().min(tag.len().saturating_sub(1));
    for keep in (1..=max_keep).rev() {
        if text.ends_with(&tag[..keep]) {
            return keep;
        }
    }
    0
}

pub(super) fn encode_openai_chat_event(event: OpenAiChatSseEvent) -> Result<Bytes, UpstreamError> {
    let data = match event {
        OpenAiChatSseEvent::Chunk(chunk) => serde_json::to_string(&chunk)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
        OpenAiChatSseEvent::Done => "[DONE]".to_string(),
    };
    Ok(encode_sse_frame(data.as_str()))
}

fn encode_sse_frame(data: &str) -> Bytes {
    Bytes::from(format!("data: {data}\n\n"))
}

pub(super) fn random_request_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill(&mut bytes);
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
}

pub(super) fn random_chat_completion_id() -> String {
    format!("chatcmpl-{}", random_hex(24))
}

pub(super) fn random_hex(len: usize) -> String {
    let bytes_len = len.div_ceil(2);
    let mut bytes = vec![0u8; bytes_len];
    rand::rng().fill(bytes.as_mut_slice());
    let mut out = String::with_capacity(bytes_len * 2);
    for byte in bytes {
        out.push_str(format!("{byte:02x}").as_str());
    }
    out.truncate(len);
    out
}

pub(super) fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{parse_tool_call_block, repair_json_fragment};

    #[test]
    fn parse_tool_call_repairs_trailing_commas() {
        let allowed = vec!["search".to_string()];
        let parsed = parse_tool_call_block(
            r#"{"name":"search","arguments":{"q":"hello",}}"#,
            &allowed,
            0,
        )
        .expect("tool call");
        assert_eq!(parsed.name, "search");
        assert_eq!(parsed.index, 0);
    }

    #[test]
    fn repair_json_fragment_balances_braces() {
        let repaired = repair_json_fragment(r#"{"name":"search","arguments":{"q":"hello"}"#);
        assert!(repaired.ends_with("}}"));
    }
}
