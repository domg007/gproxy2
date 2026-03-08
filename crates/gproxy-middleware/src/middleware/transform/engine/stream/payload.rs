use super::*;

pub(super) fn chunks_to_body_stream(chunks: Vec<Bytes>) -> TransformBodyStream {
    Box::pin(futures_stream::iter(chunks.into_iter().map(Ok)))
}

pub(super) async fn collect_source_stream_events(
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

pub(super) fn source_events_to_stream_response(
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

pub(super) fn encode_stream_response_payload(
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
        TransformResponse::StreamCreateImageOpenAi(stream_body) => {
            let chunks = stream_body
                .events
                .into_iter()
                .map(encode_openai_create_image_sse_event)
                .collect::<Result<Vec<_>, _>>()?;
            chunks_to_body_stream(chunks)
        }
        TransformResponse::StreamCreateImageEditOpenAi(stream_body) => {
            let chunks = stream_body
                .events
                .into_iter()
                .map(encode_openai_create_image_edit_sse_event)
                .collect::<Result<Vec<_>, _>>()?;
            chunks_to_body_stream(chunks)
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "encode_stream_response_payload expects a stream response variant",
            ));
        }
    };

    Ok(TransformResponsePayload::new(operation, protocol, body))
}

pub(in crate::middleware::transform::engine) async fn transform_buffered_stream_response_payload(
    input: TransformResponsePayload,
    route: TransformRoute,
) -> Result<TransformResponsePayload, MiddlewareTransformError> {
    let events = collect_source_stream_events(input.body, input.protocol).await?;
    let decoded = source_events_to_stream_response(input.protocol, events)?;
    let transformed = transform_response(decoded, route)?;
    if transformed.operation().is_stream() {
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

pub(in crate::middleware::transform::engine) fn demote_stream_response_to_generate(
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

pub(in crate::middleware::transform::engine) fn promote_generate_response_to_stream(
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

pub(in crate::middleware::transform::engine) fn transform_stream_response(
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

pub(in crate::middleware::transform::engine) fn ensure_gemini_sse_stream(
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

pub(in crate::middleware::transform::engine) fn ensure_gemini_ndjson_stream(
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
