use super::*;

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub(super) enum StreamOutputConverter {
    OpenAiResponse(OpenAiResponseStreamConverter),
    OpenAiChat(OpenAiChatStreamConverter),
    Claude(ClaudeStreamConverter),
    Gemini {
        converter: GeminiStreamConverter,
        ndjson: bool,
    },
}

#[derive(Debug, Default)]
pub(super) enum OpenAiResponseStreamConverter {
    #[default]
    Identity,
    FromOpenAiChat(OpenAiChatCompletionsToOpenAiResponseStream),
    FromClaude(ClaudeToOpenAiResponseStream),
    FromGemini(GeminiToOpenAiResponseStream),
}

impl OpenAiResponseStreamConverter {
    pub(super) fn on_event(
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

    pub(super) fn finish(&mut self) -> Vec<OpenAiCreateResponseSseEvent> {
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
pub(super) enum OpenAiChatStreamConverter {
    #[default]
    Identity,
    FromOpenAiResponse(OpenAiResponseToOpenAiChatCompletionsStream),
    FromClaude(ClaudeToOpenAiChatCompletionsStream),
    FromGemini(GeminiToOpenAiChatCompletionsStream),
}

impl OpenAiChatStreamConverter {
    pub(super) fn on_event(
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

    pub(super) fn finish(
        &mut self,
    ) -> Result<Vec<OpenAiChatCompletionsSseEvent>, MiddlewareTransformError> {
        match self {
            Self::Identity => Ok(Vec::new()),
            Self::FromOpenAiResponse(converter) => Ok(converter.finish()),
            Self::FromClaude(converter) => Ok(converter.finish()),
            Self::FromGemini(converter) => Ok(converter.finish()),
        }
    }
}

#[cfg(test)]
pub(in crate::middleware::transform::engine) fn stream_output_converter_route_kind(
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
    pub(super) fn new(
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

    pub(super) fn on_event(
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

    pub(super) fn finish(&mut self) -> Result<Vec<Bytes>, MiddlewareTransformError> {
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

pub(super) struct StreamTransformState {
    input: TransformBodyStream,
    decoder: SourceStreamDecoder,
    converter: StreamOutputConverter,
    output: VecDeque<Bytes>,
    input_ended: bool,
}

impl StreamTransformState {
    pub(super) fn new(
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

    pub(super) fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), MiddlewareTransformError> {
        let events = self.decoder.feed(chunk)?;
        for event in events {
            self.output.extend(self.converter.on_event(event)?);
        }
        Ok(())
    }

    pub(super) fn finish_input(&mut self) -> Result<(), MiddlewareTransformError> {
        let trailing_events = self.decoder.finish()?;
        for event in trailing_events {
            self.output.extend(self.converter.on_event(event)?);
        }
        self.output.extend(self.converter.finish()?);
        self.input_ended = true;
        Ok(())
    }

    pub(super) fn pop_output(&mut self) -> Option<Bytes> {
        self.output.pop_front()
    }
}

pub(in crate::middleware::transform::engine) fn supports_incremental_stream_response_conversion(
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

pub(in crate::middleware::transform::engine) fn transform_stream_response_body(
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
