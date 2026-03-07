use super::capture::{
    add_prefix_to_openai_response_stream_event, add_provider_prefix, serialize_claude_model,
};
use super::*;

enum StreamRewriteProtocol {
    OpenAiResponse,
    OpenAiChatCompletions,
    Claude,
    Passthrough,
}

struct StreamRewriteState {
    input: TransformBodyStream,
    protocol: StreamRewriteProtocol,
    provider: String,
    buffer: Vec<u8>,
    output: VecDeque<Bytes>,
    ended: bool,
}

impl StreamRewriteState {
    fn new(input: TransformBodyStream, protocol: ProtocolKind, provider: String) -> Self {
        let protocol = match protocol {
            ProtocolKind::OpenAi => StreamRewriteProtocol::OpenAiResponse,
            ProtocolKind::OpenAiChatCompletion => StreamRewriteProtocol::OpenAiChatCompletions,
            ProtocolKind::Claude => StreamRewriteProtocol::Claude,
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => StreamRewriteProtocol::Passthrough,
        };
        Self {
            input,
            protocol,
            provider,
            buffer: Vec::new(),
            output: VecDeque::new(),
            ended: false,
        }
    }

    fn push_chunk(&mut self, chunk: &[u8]) {
        if matches!(self.protocol, StreamRewriteProtocol::Passthrough) {
            self.output.push_back(Bytes::copy_from_slice(chunk));
            return;
        }

        self.buffer.extend_from_slice(chunk);
        while let Some(frame) = next_sse_frame(&mut self.buffer) {
            self.output.push_back(rewrite_sse_frame(
                &self.protocol,
                frame,
                self.provider.as_str(),
            ));
        }
    }

    fn finish_input(&mut self) {
        if !self.buffer.is_empty() {
            self.output.push_back(Bytes::from(self.buffer.clone()));
            self.buffer.clear();
        }
        self.ended = true;
    }

    fn pop_output(&mut self) -> Option<Bytes> {
        self.output.pop_front()
    }
}

pub(super) fn prefix_stream_response_body(
    input: TransformBodyStream,
    protocol: ProtocolKind,
    provider: String,
) -> TransformBodyStream {
    let state = StreamRewriteState::new(input, protocol, provider);
    let stream = futures_util::stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(output) = state.pop_output() {
                return Ok(Some((output, state)));
            }

            if state.ended {
                return Ok(None);
            }

            match state.input.next().await {
                Some(Ok(chunk)) => state.push_chunk(chunk.as_ref()),
                Some(Err(err)) => return Err(err),
                None => state.finish_input(),
            }
        }
    });
    Box::pin(stream)
}

fn next_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let lf_pos = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf_pos = buffer.windows(4).position(|window| window == b"\r\n\r\n");

    let (pos, delim_len) = match (lf_pos, crlf_pos) {
        (Some(a), Some(b)) if a <= b => (a, 2),
        (Some(_), Some(b)) => (b, 4),
        (Some(a), None) => (a, 2),
        (None, Some(b)) => (b, 4),
        (None, None) => return None,
    };

    let frame = buffer[..pos].to_vec();
    buffer.drain(..pos + delim_len);
    Some(frame)
}

fn parse_sse_fields(frame: &[u8]) -> Option<(Option<String>, String)> {
    let text = std::str::from_utf8(frame).ok()?;
    let mut event = None;
    let mut data_lines = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim_start().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        None
    } else {
        Some((event, data_lines.join("\n")))
    }
}

fn encode_sse_frame(event: Option<&str>, data: &str) -> Bytes {
    let mut out = String::new();
    if let Some(event_name) = event {
        out.push_str("event: ");
        out.push_str(event_name);
        out.push('\n');
    }
    for line in data.lines() {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    Bytes::from(out)
}

fn raw_sse_frame(frame: Vec<u8>) -> Bytes {
    let mut out = frame;
    out.extend_from_slice(b"\n\n");
    Bytes::from(out)
}

fn rewrite_sse_frame(protocol: &StreamRewriteProtocol, frame: Vec<u8>, provider: &str) -> Bytes {
    let Some((event, data)) = parse_sse_fields(frame.as_slice()) else {
        return raw_sse_frame(frame);
    };
    if data == "[DONE]" {
        return encode_sse_frame(event.as_deref(), data.as_str());
    }

    match protocol {
        StreamRewriteProtocol::OpenAiResponse => {
            let Ok(mut event_data) = serde_json::from_str::<ResponseStreamEvent>(&data) else {
                return raw_sse_frame(frame);
            };
            add_prefix_to_openai_response_stream_event(&mut event_data, provider);
            match serde_json::to_string(&event_data) {
                Ok(json) => encode_sse_frame(event.as_deref(), &json),
                Err(_) => raw_sse_frame(frame),
            }
        }
        StreamRewriteProtocol::OpenAiChatCompletions => {
            let Ok(mut chunk) = serde_json::from_str::<ChatCompletionChunk>(&data) else {
                return raw_sse_frame(frame);
            };
            chunk.model = add_provider_prefix(&chunk.model, provider);
            match serde_json::to_string(&chunk) {
                Ok(json) => encode_sse_frame(event.as_deref(), &json),
                Err(_) => raw_sse_frame(frame),
            }
        }
        StreamRewriteProtocol::Claude => {
            let Ok(mut event_data) = serde_json::from_str::<ClaudeCreateMessageStreamEvent>(&data)
            else {
                return raw_sse_frame(frame);
            };
            if let ClaudeCreateMessageStreamEvent::MessageStart(message_start) = &mut event_data
                && let Some(raw) = serialize_claude_model(&message_start.message.model)
            {
                message_start.message.model =
                    ClaudeModel::Custom(add_provider_prefix(&raw, provider));
            }
            match serde_json::to_string(&event_data) {
                Ok(json) => encode_sse_frame(event.as_deref(), &json),
                Err(_) => raw_sse_frame(frame),
            }
        }
        StreamRewriteProtocol::Passthrough => raw_sse_frame(frame),
    }
}
