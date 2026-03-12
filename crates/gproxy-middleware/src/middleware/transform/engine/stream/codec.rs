use super::*;

pub(super) fn next_sse_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
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

pub(super) fn next_ndjson_line(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let newline = buffer.iter().position(|b| *b == b'\n')?;
    let line = buffer[..newline].to_vec();
    buffer.drain(..newline + 1);
    Some(line)
}

pub(super) fn parse_sse_fields(
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

fn is_openai_keepalive_frame(event_name: Option<&str>, data: &str) -> bool {
    if matches!(event_name, Some("keepalive")) {
        return true;
    }

    serde_json::from_str::<serde_json::Value>(data)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        == Some("keepalive")
}

pub(super) fn decode_sse_frame(
    protocol: ProtocolKind,
    frame: &[u8],
) -> Result<Option<SourceStreamEvent>, MiddlewareTransformError> {
    let Some((event_name, data)) = parse_sse_fields(frame, protocol)? else {
        return Ok(None);
    };

    Ok(Some(match protocol {
        ProtocolKind::OpenAi => {
            if is_openai_keepalive_frame(event_name.as_deref(), &data) {
                return Ok(None);
            }

            SourceStreamEvent::OpenAiResponse(OpenAiCreateResponseSseEvent {
                event: event_name,
                data: if data == "[DONE]" {
                    OpenAiCreateResponseSseData::Done(data)
                } else {
                    OpenAiCreateResponseSseData::Event(serde_json::from_str(&data).map_err(
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

pub(super) fn decode_gemini_ndjson_line(
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

pub(super) fn encode_sse_frame(event: Option<&str>, data: &str) -> Bytes {
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

pub(super) fn encode_openai_sse_event(
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

pub(super) fn encode_openai_create_image_sse_event(
    event: OpenAiCreateImageSseEvent,
) -> Result<Bytes, MiddlewareTransformError> {
    let data = match event.data {
        OpenAiCreateImageSseData::Event(stream_event) => serde_json::to_string(&stream_event)
            .map_err(|err| MiddlewareTransformError::JsonEncode {
                kind: "response_stream",
                operation: OperationFamily::StreamCreateImage,
                protocol: ProtocolKind::OpenAi,
                message: err.to_string(),
            })?,
        OpenAiCreateImageSseData::Done(done) => done,
    };
    Ok(encode_sse_frame(event.event.as_deref(), &data))
}

pub(super) fn encode_openai_create_image_edit_sse_event(
    event: OpenAiCreateImageEditSseEvent,
) -> Result<Bytes, MiddlewareTransformError> {
    let data = match event.data {
        OpenAiCreateImageEditSseData::Event(stream_event) => serde_json::to_string(&stream_event)
            .map_err(|err| {
            MiddlewareTransformError::JsonEncode {
                kind: "response_stream",
                operation: OperationFamily::StreamCreateImageEdit,
                protocol: ProtocolKind::OpenAi,
                message: err.to_string(),
            }
        })?,
        OpenAiCreateImageEditSseData::Done(done) => done,
    };
    Ok(encode_sse_frame(event.event.as_deref(), &data))
}

pub(super) fn claude_sse_event_name(event: &ClaudeCreateMessageStreamEvent) -> &'static str {
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

pub(super) fn encode_claude_sse_event(
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

pub(in crate::middleware::transform::engine) fn encode_gemini_sse_event(
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

pub(super) fn encode_gemini_ndjson_event(
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

pub(super) fn gemini_done_event() -> GeminiSseEvent {
    GeminiSseEvent {
        event: None,
        data: GeminiSseEventData::Done("[DONE]".to_string()),
    }
}

pub(super) fn encode_openai_chat_sse_event(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_openai_sse_frame_ignores_keepalive_events() {
        let frame = b"event: keepalive\ndata: {\"type\":\"keepalive\",\"sequence_number\":3}\n\n";

        let event = decode_sse_frame(ProtocolKind::OpenAi, frame)
            .expect("keepalive frame should decode")
            .is_none();

        assert!(event);
    }
}
