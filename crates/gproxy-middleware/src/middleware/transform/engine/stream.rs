use super::*;

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum SourceStreamEvent {
    OpenAiResponse(OpenAiCreateResponseSseEvent),
    OpenAiChat(OpenAiChatCompletionsSseEvent),
    Claude(ClaudeCreateMessageStreamEvent),
    Gemini(GeminiSseEvent),
}

#[derive(Debug)]
enum SourceStreamDecoder {
    Sse {
        protocol: ProtocolKind,
        buffer: Vec<u8>,
    },
    GeminiNdjson {
        buffer: Vec<u8>,
    },
}

impl SourceStreamDecoder {
    fn new(protocol: ProtocolKind) -> Result<Self, MiddlewareTransformError> {
        match protocol {
            ProtocolKind::OpenAi
            | ProtocolKind::OpenAiChatCompletion
            | ProtocolKind::Claude
            | ProtocolKind::Gemini => Ok(Self::Sse {
                protocol,
                buffer: Vec::new(),
            }),
            ProtocolKind::GeminiNDJson => Ok(Self::GeminiNdjson { buffer: Vec::new() }),
        }
    }

    fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SourceStreamEvent>, MiddlewareTransformError> {
        match self {
            Self::Sse { protocol, buffer } => {
                buffer.extend_from_slice(chunk);
                let mut out = Vec::new();
                while let Some(frame) = next_sse_frame(buffer) {
                    if let Some(event) = decode_sse_frame(*protocol, frame.as_slice())? {
                        out.push(event);
                    }
                }
                Ok(out)
            }
            Self::GeminiNdjson { buffer } => {
                buffer.extend_from_slice(chunk);
                let mut out = Vec::new();
                while let Some(line) = next_ndjson_line(buffer) {
                    if let Some(event) = decode_gemini_ndjson_line(line.as_slice())? {
                        out.push(event);
                    }
                }
                Ok(out)
            }
        }
    }

    fn finish(&mut self) -> Result<Vec<SourceStreamEvent>, MiddlewareTransformError> {
        match self {
            Self::Sse { protocol, buffer } => {
                if buffer.iter().all(u8::is_ascii_whitespace) {
                    buffer.clear();
                    return Ok(Vec::new());
                }

                let trailing = std::mem::take(buffer);
                let mut out = Vec::new();
                if let Some(event) = decode_sse_frame(*protocol, trailing.as_slice())? {
                    out.push(event);
                }
                Ok(out)
            }
            Self::GeminiNdjson { buffer } => {
                if buffer.is_empty() || buffer.iter().all(u8::is_ascii_whitespace) {
                    buffer.clear();
                    return Ok(Vec::new());
                }

                let trailing = std::mem::take(buffer);
                let mut out = Vec::new();
                if let Some(event) = decode_gemini_ndjson_line(trailing.as_slice())? {
                    out.push(event);
                }
                Ok(out)
            }
        }
    }
}

#[derive(Debug, Default)]
enum ClaudeStreamConverter {
    #[default]
    Identity,
    FromOpenAiResponse(OpenAiResponseToClaudeStream),
    FromOpenAiChat(OpenAiChatCompletionsToClaudeStream),
    FromGemini(GeminiToClaudeStream),
}

impl ClaudeStreamConverter {
    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<ClaudeCreateMessageStreamEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => match event {
                SourceStreamEvent::Claude(event) => Ok(vec![event]),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "claude stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiResponse(converter) => match event {
                SourceStreamEvent::OpenAiResponse(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "claude stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiChat(converter) => match event {
                SourceStreamEvent::OpenAiChat(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "claude stream converter source protocol mismatch",
                )),
            },
            Self::FromGemini(converter) => match event {
                SourceStreamEvent::Gemini(event) => Ok(converter.on_sse_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "claude stream converter source protocol mismatch",
                )),
            },
        }
    }

    fn finish(&mut self) -> Vec<ClaudeCreateMessageStreamEvent> {
        match self {
            Self::Identity => Vec::new(),
            Self::FromOpenAiResponse(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
            Self::FromOpenAiChat(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
            Self::FromGemini(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
        }
    }
}

#[derive(Debug, Default)]
enum GeminiStreamConverter {
    #[default]
    Identity,
    FromOpenAiResponse(OpenAiResponseToGeminiStream),
    FromOpenAiChat(OpenAiChatCompletionsToGeminiStream),
    FromClaude(ClaudeToGeminiStream),
}

impl GeminiStreamConverter {
    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<GeminiSseEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => match event {
                SourceStreamEvent::Gemini(event) => Ok(vec![event]),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiResponse(converter) => match event {
                SourceStreamEvent::OpenAiResponse(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiChat(converter) => match event {
                SourceStreamEvent::OpenAiChat(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream converter source protocol mismatch",
                )),
            },
            Self::FromClaude(converter) => match event {
                SourceStreamEvent::Claude(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream converter source protocol mismatch",
                )),
            },
        }
    }

    fn finish(&mut self) -> Vec<GeminiSseEvent> {
        match self {
            Self::Identity => Vec::new(),
            Self::FromOpenAiResponse(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    vec![gemini_done_event()]
                }
            }
            Self::FromOpenAiChat(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    vec![gemini_done_event()]
                }
            }
            Self::FromClaude(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    vec![gemini_done_event()]
                }
            }
        }
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum StreamOutputConverter {
    OpenAiResponse(OpenAiResponseStreamConverter),
    OpenAiChat(OpenAiChatStreamConverter),
    Claude(ClaudeStreamConverter),
    Gemini {
        converter: GeminiStreamConverter,
        ndjson: bool,
    },
}

#[derive(Debug, Default)]
enum OpenAiResponseStreamConverter {
    #[default]
    Identity,
    FromOpenAiChat(OpenAiChatCompletionsToOpenAiResponseStream),
    FromClaude(ClaudeToOpenAiResponseStream),
    FromGemini(GeminiToOpenAiResponseStream),
}

impl OpenAiResponseStreamConverter {
    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<OpenAiCreateResponseSseEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => match event {
                SourceStreamEvent::OpenAiResponse(event) => Ok(vec![event]),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai response stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiChat(converter) => match event {
                SourceStreamEvent::OpenAiChat(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai response stream converter source protocol mismatch",
                )),
            },
            Self::FromClaude(converter) => match event {
                SourceStreamEvent::Claude(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai response stream converter source protocol mismatch",
                )),
            },
            Self::FromGemini(converter) => match event {
                SourceStreamEvent::Gemini(event) => Ok(converter.on_sse_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai response stream converter source protocol mismatch",
                )),
            },
        }
    }

    fn finish(&mut self) -> Vec<OpenAiCreateResponseSseEvent> {
        match self {
            Self::Identity => Vec::new(),
            Self::FromOpenAiChat(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
            Self::FromClaude(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
            Self::FromGemini(converter) => {
                if converter.is_finished() {
                    Vec::new()
                } else {
                    converter.finish()
                }
            }
        }
    }
}

#[derive(Debug, Default)]
#[allow(clippy::large_enum_variant)]
enum OpenAiChatStreamConverter {
    #[default]
    Identity,
    FromOpenAiResponse(OpenAiResponseToOpenAiChatCompletionsStream),
    FromClaude(ClaudeToOpenAiChatCompletionsStream),
    FromGemini(GeminiToOpenAiChatCompletionsStream),
}

impl OpenAiChatStreamConverter {
    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<OpenAiChatCompletionsSseEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => match event {
                SourceStreamEvent::OpenAiChat(event) => Ok(vec![event]),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream converter source protocol mismatch",
                )),
            },
            Self::FromOpenAiResponse(converter) => match event {
                SourceStreamEvent::OpenAiResponse(event) => Ok(converter.on_event(event)),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream converter source protocol mismatch",
                )),
            },
            Self::FromClaude(converter) => match event {
                SourceStreamEvent::Claude(event) => Ok(converter.on_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream converter source protocol mismatch",
                )),
            },
            Self::FromGemini(converter) => match event {
                SourceStreamEvent::Gemini(event) => Ok(converter.on_sse_event(event)?),
                _ => Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream converter source protocol mismatch",
                )),
            },
        }
    }

    fn finish(&mut self) -> Result<Vec<OpenAiChatCompletionsSseEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => Ok(Vec::new()),
            Self::FromOpenAiResponse(converter) => Ok(converter.finish()),
            Self::FromClaude(converter) => Ok(converter.finish()),
            Self::FromGemini(converter) => Ok(converter.finish()),
        }
    }
}

#[cfg(test)]
pub(super) fn stream_output_converter_route_kind(
    from_protocol: ProtocolKind,
    to_protocol: ProtocolKind,
) -> Result<&'static str, MiddlewareTransformError> {
    match StreamOutputConverter::new(from_protocol, to_protocol)? {
        StreamOutputConverter::OpenAiChat(OpenAiChatStreamConverter::FromOpenAiResponse(_)) => {
            Ok("openai_response_to_chat")
        }
        StreamOutputConverter::OpenAiChat(OpenAiChatStreamConverter::FromClaude(_)) => {
            Ok("claude_to_chat")
        }
        StreamOutputConverter::OpenAiChat(OpenAiChatStreamConverter::FromGemini(_)) => {
            Ok("gemini_to_chat")
        }
        StreamOutputConverter::OpenAiChat(OpenAiChatStreamConverter::Identity) => {
            Ok("chat_identity")
        }
        StreamOutputConverter::OpenAiResponse(_) => Ok("openai_response"),
        StreamOutputConverter::Claude(_) => Ok("claude"),
        StreamOutputConverter::Gemini { .. } => Ok("gemini"),
    }
}

impl StreamOutputConverter {
    fn new(
        from_protocol: ProtocolKind,
        to_protocol: ProtocolKind,
    ) -> Result<Self, MiddlewareTransformError> {
        match to_protocol {
            ProtocolKind::OpenAi => Ok(Self::OpenAiResponse(match from_protocol {
                ProtocolKind::OpenAi => OpenAiResponseStreamConverter::Identity,
                ProtocolKind::OpenAiChatCompletion => {
                    OpenAiResponseStreamConverter::FromOpenAiChat(Default::default())
                }
                ProtocolKind::Claude => {
                    OpenAiResponseStreamConverter::FromClaude(Default::default())
                }
                ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
                    OpenAiResponseStreamConverter::FromGemini(Default::default())
                }
            })),
            ProtocolKind::OpenAiChatCompletion => Ok(Self::OpenAiChat(match from_protocol {
                ProtocolKind::OpenAiChatCompletion => OpenAiChatStreamConverter::Identity,
                ProtocolKind::OpenAi => {
                    OpenAiChatStreamConverter::FromOpenAiResponse(Default::default())
                }
                ProtocolKind::Claude => OpenAiChatStreamConverter::FromClaude(Default::default()),
                ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
                    OpenAiChatStreamConverter::FromGemini(Default::default())
                }
            })),
            ProtocolKind::Claude => Ok(Self::Claude(match from_protocol {
                ProtocolKind::OpenAi => {
                    ClaudeStreamConverter::FromOpenAiResponse(Default::default())
                }
                ProtocolKind::OpenAiChatCompletion => {
                    ClaudeStreamConverter::FromOpenAiChat(Default::default())
                }
                ProtocolKind::Claude => ClaudeStreamConverter::Identity,
                ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
                    ClaudeStreamConverter::FromGemini(Default::default())
                }
            })),
            ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => Ok(Self::Gemini {
                converter: match from_protocol {
                    ProtocolKind::OpenAi => {
                        GeminiStreamConverter::FromOpenAiResponse(Default::default())
                    }
                    ProtocolKind::OpenAiChatCompletion => {
                        GeminiStreamConverter::FromOpenAiChat(Default::default())
                    }
                    ProtocolKind::Claude => GeminiStreamConverter::FromClaude(Default::default()),
                    ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
                        GeminiStreamConverter::Identity
                    }
                },
                ndjson: to_protocol == ProtocolKind::GeminiNDJson,
            }),
        }
    }

    fn on_event(
        &mut self,
        event: SourceStreamEvent,
    ) -> Result<Vec<Bytes>, MiddlewareTransformError> {
        match self {
            Self::OpenAiResponse(converter) => converter
                .on_event(event)?
                .into_iter()
                .map(encode_openai_sse_event)
                .collect(),
            Self::OpenAiChat(converter) => converter
                .on_event(event)?
                .into_iter()
                .map(encode_openai_chat_sse_event)
                .collect(),
            Self::Claude(converter) => converter
                .on_event(event)?
                .into_iter()
                .map(encode_claude_sse_event)
                .collect(),
            Self::Gemini { converter, ndjson } => {
                let events = converter.on_event(event)?;
                if *ndjson {
                    events
                        .into_iter()
                        .filter_map(encode_gemini_ndjson_event)
                        .collect::<Result<Vec<_>, _>>()
                } else {
                    events
                        .into_iter()
                        .filter_map(encode_gemini_sse_event)
                        .collect::<Result<Vec<_>, _>>()
                }
            }
        }
    }

    fn finish(&mut self) -> Result<Vec<Bytes>, MiddlewareTransformError> {
        match self {
            Self::OpenAiResponse(converter) => converter
                .finish()
                .into_iter()
                .map(encode_openai_sse_event)
                .collect(),
            Self::OpenAiChat(converter) => converter
                .finish()?
                .into_iter()
                .map(encode_openai_chat_sse_event)
                .collect(),
            Self::Claude(converter) => converter
                .finish()
                .into_iter()
                .map(encode_claude_sse_event)
                .collect(),
            Self::Gemini { converter, ndjson } => {
                let events = converter.finish();
                if *ndjson {
                    events
                        .into_iter()
                        .filter_map(encode_gemini_ndjson_event)
                        .collect::<Result<Vec<_>, _>>()
                } else {
                    events
                        .into_iter()
                        .filter_map(encode_gemini_sse_event)
                        .collect::<Result<Vec<_>, _>>()
                }
            }
        }
    }
}

struct StreamTransformState {
    input: TransformBodyStream,
    decoder: SourceStreamDecoder,
    converter: StreamOutputConverter,
    output: VecDeque<Bytes>,
    input_ended: bool,
}

impl StreamTransformState {
    fn new(
        input: TransformBodyStream,
        from_protocol: ProtocolKind,
        to_protocol: ProtocolKind,
    ) -> Result<Self, MiddlewareTransformError> {
        Ok(Self {
            input,
            decoder: SourceStreamDecoder::new(from_protocol)?,
            converter: StreamOutputConverter::new(from_protocol, to_protocol)?,
            output: VecDeque::new(),
            input_ended: false,
        })
    }

    fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), MiddlewareTransformError> {
        let events = self.decoder.feed(chunk)?;
        for event in events {
            self.output.extend(self.converter.on_event(event)?);
        }
        Ok(())
    }

    fn finish_input(&mut self) -> Result<(), MiddlewareTransformError> {
        let trailing_events = self.decoder.finish()?;
        for event in trailing_events {
            self.output.extend(self.converter.on_event(event)?);
        }
        self.output.extend(self.converter.finish()?);
        self.input_ended = true;
        Ok(())
    }

    fn pop_output(&mut self) -> Option<Bytes> {
        self.output.pop_front()
    }
}

pub(super) fn supports_incremental_stream_response_conversion(
    from_protocol: ProtocolKind,
    to_protocol: ProtocolKind,
) -> bool {
    matches!(
        to_protocol,
        ProtocolKind::OpenAi
            | ProtocolKind::OpenAiChatCompletion
            | ProtocolKind::Claude
            | ProtocolKind::Gemini
            | ProtocolKind::GeminiNDJson
    ) && matches!(
        from_protocol,
        ProtocolKind::OpenAi
            | ProtocolKind::OpenAiChatCompletion
            | ProtocolKind::Claude
            | ProtocolKind::Gemini
            | ProtocolKind::GeminiNDJson
    )
}

pub(super) fn transform_stream_response_body(
    input: TransformBodyStream,
    from_protocol: ProtocolKind,
    to_protocol: ProtocolKind,
) -> Result<TransformBodyStream, MiddlewareTransformError> {
    let state = StreamTransformState::new(input, from_protocol, to_protocol)?;

    let output = futures_stream::try_unfold(state, |mut state| async move {
        loop {
            if let Some(chunk) = state.pop_output() {
                return Ok(Some((chunk, state)));
            }

            if state.input_ended {
                return Ok(None);
            }

            match state.input.next().await {
                Some(Ok(chunk)) => state.push_chunk(chunk.as_ref())?,
                Some(Err(err)) => return Err(err),
                None => state.finish_input()?,
            }
        }
    });

    Ok(Box::pin(output))
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

fn next_ndjson_line(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let newline = buffer.iter().position(|b| *b == b'\n')?;
    let line = buffer[..newline].to_vec();
    buffer.drain(..newline + 1);
    Some(line)
}

fn parse_sse_fields(
    frame: &[u8],
    protocol: ProtocolKind,
) -> Result<Option<(Option<String>, String)>, MiddlewareTransformError> {
    if frame.is_empty() {
        return Ok(None);
    }

    let text = std::str::from_utf8(frame).map_err(|err| MiddlewareTransformError::JsonDecode {
        kind: "response_stream",
        operation: OperationFamily::StreamGenerateContent,
        protocol,
        message: err.to_string(),
    })?;

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
        return Ok(None);
    }

    Ok(Some((event, data_lines.join("\n"))))
}

fn decode_sse_frame(
    protocol: ProtocolKind,
    frame: &[u8],
) -> Result<Option<SourceStreamEvent>, MiddlewareTransformError> {
    let Some((event_name, data)) = parse_sse_fields(frame, protocol)? else {
        return Ok(None);
    };

    Ok(Some(match protocol {
        ProtocolKind::OpenAi => SourceStreamEvent::OpenAiResponse(OpenAiCreateResponseSseEvent {
            event: event_name,
            data: if data == "[DONE]" {
                OpenAiCreateResponseSseData::Done(data)
            } else {
                OpenAiCreateResponseSseData::Event(serde_json::from_str(&data).map_err(|err| {
                    MiddlewareTransformError::JsonDecode {
                        kind: "response_stream",
                        operation: OperationFamily::StreamGenerateContent,
                        protocol,
                        message: err.to_string(),
                    }
                })?)
            },
        }),
        ProtocolKind::OpenAiChatCompletion => {
            SourceStreamEvent::OpenAiChat(OpenAiChatCompletionsSseEvent {
                event: event_name,
                data: if data == "[DONE]" {
                    OpenAiChatCompletionsSseData::Done(data)
                } else {
                    OpenAiChatCompletionsSseData::Chunk(serde_json::from_str(&data).map_err(
                        |err| MiddlewareTransformError::JsonDecode {
                            kind: "response_stream",
                            operation: OperationFamily::StreamGenerateContent,
                            protocol,
                            message: err.to_string(),
                        },
                    )?)
                },
            })
        }
        ProtocolKind::Claude => {
            SourceStreamEvent::Claude(serde_json::from_str(&data).map_err(|err| {
                MiddlewareTransformError::JsonDecode {
                    kind: "response_stream",
                    operation: OperationFamily::StreamGenerateContent,
                    protocol,
                    message: err.to_string(),
                }
            })?)
        }
        ProtocolKind::Gemini => SourceStreamEvent::Gemini(GeminiSseEvent {
            event: event_name,
            data: if data == "[DONE]" {
                GeminiSseEventData::Done(data)
            } else {
                GeminiSseEventData::Chunk(serde_json::from_str(&data).map_err(|err| {
                    MiddlewareTransformError::JsonDecode {
                        kind: "response_stream",
                        operation: OperationFamily::StreamGenerateContent,
                        protocol,
                        message: err.to_string(),
                    }
                })?)
            },
        }),
        ProtocolKind::GeminiNDJson => {
            return Err(MiddlewareTransformError::Unsupported(
                "gemini ndjson stream uses line-delimited framing instead of sse framing",
            ));
        }
    }))
}

fn decode_gemini_ndjson_line(
    line: &[u8],
) -> Result<Option<SourceStreamEvent>, MiddlewareTransformError> {
    let trimmed = line
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .collect::<Vec<_>>();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let text = std::str::from_utf8(trimmed.as_slice()).map_err(|err| {
        MiddlewareTransformError::JsonDecode {
            kind: "response_stream",
            operation: OperationFamily::StreamGenerateContent,
            protocol: ProtocolKind::GeminiNDJson,
            message: err.to_string(),
        }
    })?;
    if text == "[DONE]" {
        return Ok(Some(SourceStreamEvent::Gemini(gemini_done_event())));
    }

    let chunk: GeminiGenerateContentResponseBody =
        serde_json::from_str(text).map_err(|err| MiddlewareTransformError::JsonDecode {
            kind: "response_stream",
            operation: OperationFamily::StreamGenerateContent,
            protocol: ProtocolKind::GeminiNDJson,
            message: err.to_string(),
        })?;
    Ok(Some(SourceStreamEvent::Gemini(GeminiSseEvent {
        event: None,
        data: GeminiSseEventData::Chunk(chunk),
    })))
}

fn encode_sse_frame(event: Option<&str>, data: &str) -> Bytes {
    let mut out = String::new();
    if let Some(event) = event {
        out.push_str("event: ");
        out.push_str(event);
        out.push('\n');
    }
    out.push_str("data: ");
    out.push_str(data);
    out.push_str("\n\n");
    Bytes::from(out)
}

fn encode_openai_sse_event(
    event: OpenAiCreateResponseSseEvent,
) -> Result<Bytes, MiddlewareTransformError> {
    let data = match event.data {
        OpenAiCreateResponseSseData::Event(stream_event) => serde_json::to_string(&stream_event)
            .map_err(|err| MiddlewareTransformError::JsonEncode {
                kind: "response_stream",
                operation: OperationFamily::StreamGenerateContent,
                protocol: ProtocolKind::OpenAi,
                message: err.to_string(),
            })?,
        OpenAiCreateResponseSseData::Done(done) => done,
    };
    Ok(encode_sse_frame(event.event.as_deref(), &data))
}

fn claude_sse_event_name(event: &ClaudeCreateMessageStreamEvent) -> &'static str {
    match event {
        ClaudeCreateMessageStreamEvent::MessageStart(_) => "message_start",
        ClaudeCreateMessageStreamEvent::ContentBlockStart(_) => "content_block_start",
        ClaudeCreateMessageStreamEvent::ContentBlockDelta(_) => "content_block_delta",
        ClaudeCreateMessageStreamEvent::ContentBlockStop(_) => "content_block_stop",
        ClaudeCreateMessageStreamEvent::MessageDelta(_) => "message_delta",
        ClaudeCreateMessageStreamEvent::MessageStop(_) => "message_stop",
        ClaudeCreateMessageStreamEvent::Ping(_) => "ping",
        ClaudeCreateMessageStreamEvent::Error(_) => "error",
        ClaudeCreateMessageStreamEvent::Unknown(_) => "unknown",
    }
}

fn encode_claude_sse_event(
    event: ClaudeCreateMessageStreamEvent,
) -> Result<Bytes, MiddlewareTransformError> {
    let data =
        serde_json::to_string(&event).map_err(|err| MiddlewareTransformError::JsonEncode {
            kind: "response_stream",
            operation: OperationFamily::StreamGenerateContent,
            protocol: ProtocolKind::Claude,
            message: err.to_string(),
        })?;
    Ok(encode_sse_frame(Some(claude_sse_event_name(&event)), &data))
}

pub(super) fn encode_gemini_sse_event(
    event: GeminiSseEvent,
) -> Option<Result<Bytes, MiddlewareTransformError>> {
    let GeminiSseEvent { event, data } = event;
    match data {
        GeminiSseEventData::Chunk(chunk) => Some(
            serde_json::to_string(&chunk)
                .map(|json| encode_sse_frame(event.as_deref(), &json))
                .map_err(|err| MiddlewareTransformError::JsonEncode {
                    kind: "response_stream",
                    operation: OperationFamily::StreamGenerateContent,
                    protocol: ProtocolKind::Gemini,
                    message: err.to_string(),
                }),
        ),
        // Gemini SSE clients expect JSON payloads in `data:` lines and may fail on `[DONE]`.
        GeminiSseEventData::Done(_) => None,
    }
}

fn encode_gemini_ndjson_event(
    event: GeminiSseEvent,
) -> Option<Result<Bytes, MiddlewareTransformError>> {
    match event.data {
        GeminiSseEventData::Chunk(chunk) => Some(
            serde_json::to_vec(&chunk)
                .map(|mut json| {
                    json.push(b'\n');
                    Bytes::from(json)
                })
                .map_err(|err| MiddlewareTransformError::JsonEncode {
                    kind: "response_stream",
                    operation: OperationFamily::StreamGenerateContent,
                    protocol: ProtocolKind::GeminiNDJson,
                    message: err.to_string(),
                }),
        ),
        GeminiSseEventData::Done(_) => None,
    }
}

fn gemini_done_event() -> GeminiSseEvent {
    GeminiSseEvent {
        event: None,
        data: GeminiSseEventData::Done("[DONE]".to_string()),
    }
}

fn encode_openai_chat_sse_event(
    event: OpenAiChatCompletionsSseEvent,
) -> Result<Bytes, MiddlewareTransformError> {
    let data = match event.data {
        OpenAiChatCompletionsSseData::Chunk(chunk) => {
            serde_json::to_string(&chunk).map_err(|err| MiddlewareTransformError::JsonEncode {
                kind: "response_stream",
                operation: OperationFamily::StreamGenerateContent,
                protocol: ProtocolKind::OpenAiChatCompletion,
                message: err.to_string(),
            })?
        }
        OpenAiChatCompletionsSseData::Done(done) => done,
    };
    Ok(encode_sse_frame(event.event.as_deref(), &data))
}

fn chunks_to_body_stream(chunks: Vec<Bytes>) -> TransformBodyStream {
    Box::pin(futures_stream::iter(chunks.into_iter().map(Ok)))
}

async fn collect_source_stream_events(
    body: TransformBodyStream,
    protocol: ProtocolKind,
) -> Result<Vec<SourceStreamEvent>, MiddlewareTransformError> {
    let mut decoder = SourceStreamDecoder::new(protocol)?;
    let mut input = body;
    let mut events = Vec::new();
    while let Some(chunk) = input.next().await {
        events.extend(decoder.feed(chunk?.as_ref())?);
    }
    events.extend(decoder.finish()?);
    Ok(events)
}

fn source_events_to_stream_response(
    protocol: ProtocolKind,
    events: Vec<SourceStreamEvent>,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match protocol {
        ProtocolKind::OpenAi => {
            let mut out = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::OpenAiResponse(event) => out.push(event),
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding openai stream",
                        ));
                    }
                }
            }
            Ok(TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody { events: out },
            ))
        }
        ProtocolKind::OpenAiChatCompletion => {
            let mut out = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::OpenAiChat(event) => out.push(event),
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding openai chat stream",
                        ));
                    }
                }
            }
            Ok(
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody { events: out },
                ),
            )
        }
        ProtocolKind::Claude => {
            let mut out = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::Claude(event) => out.push(event),
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding claude stream",
                        ));
                    }
                }
            }
            Ok(TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody { events: out },
            ))
        }
        ProtocolKind::Gemini => {
            let mut out = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::Gemini(event) => out.push(event),
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding gemini stream",
                        ));
                    }
                }
            }
            Ok(TransformResponse::StreamGenerateContentGeminiSse(
                GeminiStreamGenerateContentResponse::SseSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiSseStreamBody { events: out },
                },
            ))
        }
        ProtocolKind::GeminiNDJson => {
            let mut chunks = Vec::new();
            for event in events {
                match event {
                    SourceStreamEvent::Gemini(event) => {
                        if let GeminiSseEventData::Chunk(chunk) = event.data {
                            chunks.push(chunk);
                        }
                    }
                    _ => {
                        return Err(MiddlewareTransformError::Unsupported(
                            "mixed stream event types while decoding gemini ndjson stream",
                        ));
                    }
                }
            }
            Ok(TransformResponse::StreamGenerateContentGeminiNdjson(
                GeminiStreamGenerateContentResponse::NdjsonSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiNdjsonStreamBody { chunks },
                },
            ))
        }
    }
}

fn encode_stream_response_payload(
    response: TransformResponse,
) -> Result<TransformResponsePayload, MiddlewareTransformError> {
    let operation = response.operation();
    let protocol = response.protocol();

    let body = match response {
        TransformResponse::StreamGenerateContentOpenAiResponse(stream_body) => {
            let chunks = stream_body
                .events
                .into_iter()
                .map(encode_openai_sse_event)
                .collect::<Result<Vec<_>, _>>()?;
            chunks_to_body_stream(chunks)
        }
        TransformResponse::StreamGenerateContentOpenAiChatCompletions(stream_body) => {
            let chunks = stream_body
                .events
                .into_iter()
                .map(encode_openai_chat_sse_event)
                .collect::<Result<Vec<_>, _>>()?;
            chunks_to_body_stream(chunks)
        }
        TransformResponse::StreamGenerateContentClaude(stream_body) => {
            let chunks = stream_body
                .events
                .into_iter()
                .map(encode_claude_sse_event)
                .collect::<Result<Vec<_>, _>>()?;
            chunks_to_body_stream(chunks)
        }
        TransformResponse::StreamGenerateContentGeminiSse(stream_response) => {
            match ensure_gemini_sse_stream(stream_response) {
                GeminiStreamGenerateContentResponse::SseSuccess { body, .. } => {
                    let chunks = body
                        .events
                        .into_iter()
                        .filter_map(encode_gemini_sse_event)
                        .collect::<Result<Vec<_>, _>>()?;
                    chunks_to_body_stream(chunks)
                }
                GeminiStreamGenerateContentResponse::Error { body, .. } => {
                    let bytes = serde_json::to_vec(&body).map_err(|err| {
                        MiddlewareTransformError::JsonEncode {
                            kind: "response_stream",
                            operation: OperationFamily::StreamGenerateContent,
                            protocol: ProtocolKind::Gemini,
                            message: err.to_string(),
                        }
                    })?;
                    bytes_to_body_stream(bytes)
                }
                GeminiStreamGenerateContentResponse::NdjsonSuccess { .. } => {
                    return Err(MiddlewareTransformError::Unsupported(
                        "unexpected ndjson variant while encoding gemini sse stream",
                    ));
                }
            }
        }
        TransformResponse::StreamGenerateContentGeminiNdjson(stream_response) => {
            match ensure_gemini_ndjson_stream(stream_response) {
                GeminiStreamGenerateContentResponse::NdjsonSuccess { body, .. } => {
                    let chunks = body
                        .chunks
                        .into_iter()
                        .map(|chunk| {
                            serde_json::to_vec(&chunk)
                                .map(|mut json| {
                                    json.push(b'\n');
                                    Bytes::from(json)
                                })
                                .map_err(|err| MiddlewareTransformError::JsonEncode {
                                    kind: "response_stream",
                                    operation: OperationFamily::StreamGenerateContent,
                                    protocol: ProtocolKind::GeminiNDJson,
                                    message: err.to_string(),
                                })
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    chunks_to_body_stream(chunks)
                }
                GeminiStreamGenerateContentResponse::Error { body, .. } => {
                    let bytes = serde_json::to_vec(&body).map_err(|err| {
                        MiddlewareTransformError::JsonEncode {
                            kind: "response_stream",
                            operation: OperationFamily::StreamGenerateContent,
                            protocol: ProtocolKind::GeminiNDJson,
                            message: err.to_string(),
                        }
                    })?;
                    bytes_to_body_stream(bytes)
                }
                GeminiStreamGenerateContentResponse::SseSuccess { .. } => {
                    return Err(MiddlewareTransformError::Unsupported(
                        "unexpected sse variant while encoding gemini ndjson stream",
                    ));
                }
            }
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "encode_stream_response_payload expects a stream response variant",
            ));
        }
    };

    Ok(TransformResponsePayload::new(operation, protocol, body))
}

pub(super) async fn transform_buffered_stream_response_payload(
    input: TransformResponsePayload,
    route: TransformRoute,
) -> Result<TransformResponsePayload, MiddlewareTransformError> {
    let events = collect_source_stream_events(input.body, input.protocol).await?;
    let decoded = source_events_to_stream_response(input.protocol, events)?;
    let transformed = transform_response(decoded, route)?;
    if transformed.operation() == OperationFamily::StreamGenerateContent {
        encode_stream_response_payload(transformed)
    } else {
        let operation = transformed.operation();
        let protocol = transformed.protocol();
        let body = encode_response_payload(transformed)?;
        Ok(TransformResponsePayload::new(
            operation,
            protocol,
            bytes_to_body_stream(body),
        ))
    }
}

pub(super) fn demote_stream_response_to_generate(
    input: TransformResponse,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::StreamGenerateContentOpenAiResponse(response) => {
            TransformResponse::GenerateContentOpenAiResponse(
                OpenAiCreateResponseResponse::try_from(response)?,
            )
        }
        TransformResponse::StreamGenerateContentOpenAiChatCompletions(response) => {
            TransformResponse::GenerateContentOpenAiChatCompletions(
                OpenAiChatCompletionsResponse::try_from(response)?,
            )
        }
        TransformResponse::StreamGenerateContentClaude(response) => {
            TransformResponse::GenerateContentClaude(ClaudeCreateMessageResponse::try_from(
                response,
            )?)
        }
        TransformResponse::StreamGenerateContentGeminiSse(response)
        | TransformResponse::StreamGenerateContentGeminiNdjson(response) => {
            TransformResponse::GenerateContentGemini(GeminiGenerateContentResponse::try_from(
                response,
            )?)
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "stream response demotion requires stream_generate_content destination payload",
            ));
        }
    })
}

pub(super) fn promote_generate_response_to_stream(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    match input {
        TransformResponse::GenerateContentOpenAiResponse(response) => {
            if dst_protocol != ProtocolKind::OpenAi {
                return Err(MiddlewareTransformError::Unsupported(
                    "openai response stream conversion requires OpenAi destination protocol",
                ));
            }
            Ok(TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody::try_from(response)?,
            ))
        }
        TransformResponse::GenerateContentOpenAiChatCompletions(response) => {
            if dst_protocol != ProtocolKind::OpenAiChatCompletion {
                return Err(MiddlewareTransformError::Unsupported(
                    "openai chat stream conversion requires OpenAiChatCompletion destination protocol",
                ));
            }
            Ok(
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(response)?,
                ),
            )
        }
        TransformResponse::GenerateContentClaude(response) => {
            if dst_protocol != ProtocolKind::Claude {
                return Err(MiddlewareTransformError::Unsupported(
                    "claude stream conversion requires Claude destination protocol",
                ));
            }
            Ok(TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody::try_from(response)?,
            ))
        }
        TransformResponse::GenerateContentGemini(response) => {
            let stream = GeminiStreamGenerateContentResponse::try_from(response)?;
            match dst_protocol {
                ProtocolKind::Gemini => Ok(TransformResponse::StreamGenerateContentGeminiSse(
                    ensure_gemini_sse_stream(stream),
                )),
                ProtocolKind::GeminiNDJson => {
                    Ok(TransformResponse::StreamGenerateContentGeminiNdjson(
                        ensure_gemini_ndjson_stream(stream),
                    ))
                }
                _ => Err(MiddlewareTransformError::Unsupported(
                    "gemini stream conversion requires Gemini/GeminiNDJson destination protocol",
                )),
            }
        }
        _ => Err(MiddlewareTransformError::Unsupported(
            "stream response promotion requires generate_content destination payload",
        )),
    }
}

pub(super) fn transform_stream_response(
    input: TransformResponse,
    dst_protocol: ProtocolKind,
) -> Result<TransformResponse, MiddlewareTransformError> {
    Ok(match input {
        TransformResponse::StreamGenerateContentOpenAiResponse(response) => match dst_protocol {
            ProtocolKind::OpenAi => {
                TransformResponse::StreamGenerateContentOpenAiResponse(response)
            }
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(response)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody::try_from(response)?,
            ),
            ProtocolKind::Gemini => TransformResponse::StreamGenerateContentGeminiSse(
                GeminiStreamGenerateContentResponse::SseSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiSseStreamBody::try_from(response)?,
                },
            ),
            ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                GeminiStreamGenerateContentResponse::NdjsonSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiNdjsonStreamBody::try_from(response)?,
                },
            ),
        },
        TransformResponse::StreamGenerateContentOpenAiChatCompletions(response) => {
            match dst_protocol {
                ProtocolKind::OpenAi => TransformResponse::StreamGenerateContentOpenAiResponse(
                    OpenAiCreateResponseSseStreamBody::try_from(response)?,
                ),
                ProtocolKind::OpenAiChatCompletion => {
                    TransformResponse::StreamGenerateContentOpenAiChatCompletions(response)
                }
                ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(
                    ClaudeCreateMessageSseStreamBody::try_from(response)?,
                ),
                ProtocolKind::Gemini => TransformResponse::StreamGenerateContentGeminiSse(
                    GeminiStreamGenerateContentResponse::SseSuccess {
                        stats_code: StatusCode::OK,
                        headers: Default::default(),
                        body: GeminiSseStreamBody::try_from(response)?,
                    },
                ),
                ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                    GeminiStreamGenerateContentResponse::NdjsonSuccess {
                        stats_code: StatusCode::OK,
                        headers: Default::default(),
                        body: GeminiNdjsonStreamBody::try_from(response)?,
                    },
                ),
            }
        }
        TransformResponse::StreamGenerateContentClaude(response) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody::try_from(response)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(response)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(response),
            ProtocolKind::Gemini => TransformResponse::StreamGenerateContentGeminiSse(
                GeminiStreamGenerateContentResponse::SseSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiSseStreamBody::try_from(response)?,
                },
            ),
            ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                GeminiStreamGenerateContentResponse::NdjsonSuccess {
                    stats_code: StatusCode::OK,
                    headers: Default::default(),
                    body: GeminiNdjsonStreamBody::try_from(response)?,
                },
            ),
        },
        TransformResponse::StreamGenerateContentGeminiSse(stream) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody::try_from(stream)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(stream)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody::try_from(stream)?,
            ),
            ProtocolKind::Gemini => {
                TransformResponse::StreamGenerateContentGeminiSse(ensure_gemini_sse_stream(stream))
            }
            ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                ensure_gemini_ndjson_stream(stream),
            ),
        },
        TransformResponse::StreamGenerateContentGeminiNdjson(stream) => match dst_protocol {
            ProtocolKind::OpenAi => TransformResponse::StreamGenerateContentOpenAiResponse(
                OpenAiCreateResponseSseStreamBody::try_from(stream)?,
            ),
            ProtocolKind::OpenAiChatCompletion => {
                TransformResponse::StreamGenerateContentOpenAiChatCompletions(
                    OpenAiChatCompletionsSseStreamBody::try_from(stream)?,
                )
            }
            ProtocolKind::Claude => TransformResponse::StreamGenerateContentClaude(
                ClaudeCreateMessageSseStreamBody::try_from(stream)?,
            ),
            ProtocolKind::Gemini => {
                TransformResponse::StreamGenerateContentGeminiSse(ensure_gemini_sse_stream(stream))
            }
            ProtocolKind::GeminiNDJson => TransformResponse::StreamGenerateContentGeminiNdjson(
                ensure_gemini_ndjson_stream(stream),
            ),
        },
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "stream response transform requires stream_generate_content destination payload",
            ));
        }
    })
}

pub(super) fn ensure_gemini_sse_stream(
    stream: GeminiStreamGenerateContentResponse,
) -> GeminiStreamGenerateContentResponse {
    match stream {
        GeminiStreamGenerateContentResponse::SseSuccess { .. }
        | GeminiStreamGenerateContentResponse::Error { .. } => stream,
        GeminiStreamGenerateContentResponse::NdjsonSuccess {
            stats_code,
            headers,
            body,
        } => GeminiStreamGenerateContentResponse::SseSuccess {
            stats_code,
            headers,
            body: GeminiSseStreamBody {
                events: body
                    .chunks
                    .into_iter()
                    .map(|chunk| GeminiSseEvent {
                        event: None,
                        data: GeminiSseEventData::Chunk(chunk),
                    })
                    .chain(std::iter::once(GeminiSseEvent {
                        event: None,
                        data: GeminiSseEventData::Done("[DONE]".to_string()),
                    }))
                    .collect(),
            },
        },
    }
}

pub(super) fn ensure_gemini_ndjson_stream(
    stream: GeminiStreamGenerateContentResponse,
) -> GeminiStreamGenerateContentResponse {
    match stream {
        GeminiStreamGenerateContentResponse::NdjsonSuccess { .. }
        | GeminiStreamGenerateContentResponse::Error { .. } => stream,
        GeminiStreamGenerateContentResponse::SseSuccess {
            stats_code,
            headers,
            body,
        } => GeminiStreamGenerateContentResponse::NdjsonSuccess {
            stats_code,
            headers,
            body: GeminiNdjsonStreamBody {
                chunks: body
                    .events
                    .into_iter()
                    .filter_map(|event| match event.data {
                        GeminiSseEventData::Chunk(chunk) => Some(chunk),
                        GeminiSseEventData::Done(_) => None,
                    })
                    .collect(),
            },
        },
    }
}
