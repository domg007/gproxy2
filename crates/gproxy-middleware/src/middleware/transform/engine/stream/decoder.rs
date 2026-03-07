use super::*;

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub(super) enum SourceStreamEvent {
    OpenAiResponse(OpenAiCreateResponseSseEvent),
    OpenAiChat(OpenAiChatCompletionsSseEvent),
    Claude(ClaudeCreateMessageStreamEvent),
    Gemini(GeminiSseEvent),
}

#[derive(Debug)]
pub(super) enum SourceStreamDecoder {
    Sse {
        protocol: ProtocolKind,
        buffer: Vec<u8>,
    },
    GeminiNdjson {
        buffer: Vec<u8>,
    },
}

impl SourceStreamDecoder {
    pub(super) fn new(protocol: ProtocolKind) -> Result<Self, MiddlewareTransformError> {
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

    pub(super) fn feed(
        &mut self,
        chunk: &[u8],
    ) -> Result<Vec<SourceStreamEvent>, MiddlewareTransformError> {
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

    pub(super) fn finish(&mut self) -> Result<Vec<SourceStreamEvent>, MiddlewareTransformError> {
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
pub(super) enum ClaudeStreamConverter {
    #[default]
    Identity,
    FromOpenAiResponse(OpenAiResponseToClaudeStream),
    FromOpenAiChat(OpenAiChatCompletionsToClaudeStream),
    FromGemini(GeminiToClaudeStream),
}

impl ClaudeStreamConverter {
    pub(super) fn on_event(
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

    pub(super) fn finish(&mut self) -> Vec<ClaudeCreateMessageStreamEvent> {
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
pub(super) enum GeminiStreamConverter {
    #[default]
    Identity,
    FromOpenAiResponse(OpenAiResponseToGeminiStream),
    FromOpenAiChat(OpenAiChatCompletionsToGeminiStream),
    FromClaude(ClaudeToGeminiStream),
}

impl GeminiStreamConverter {
    pub(super) fn on_event(
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

    pub(super) fn finish(&mut self) -> Vec<GeminiSseEvent> {
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
