use super::*;

pub(crate) fn should_rewrite_gemini_stream_to_ndjson(request: &TransformRequest) -> bool {
    matches!(
        request,
        TransformRequest::StreamGenerateContentGeminiNdjson(_)
    )
}

pub(crate) fn is_sse_content_type(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-type")
            && value.to_ascii_lowercase().contains("text/event-stream")
    })
}

pub(crate) fn is_streaming_content_type(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("content-type") && {
            let content_type = value.to_ascii_lowercase();
            content_type.contains("text/event-stream")
                || content_type.contains("application/x-ndjson")
        }
    })
}

pub(crate) fn rewrite_content_type_to_ndjson(headers: &mut Vec<(String, String)>) {
    let mut replaced = false;
    for (name, value) in headers.iter_mut() {
        if name.eq_ignore_ascii_case("content-type") {
            *value = "application/x-ndjson".to_string();
            replaced = true;
        }
    }
    if !replaced {
        headers.push((
            "content-type".to_string(),
            "application/x-ndjson".to_string(),
        ));
    }
}

pub(crate) fn remove_header_ignore_case(headers: &mut Vec<(String, String)>, header_name: &str) {
    headers.retain(|(name, _)| !name.eq_ignore_ascii_case(header_name));
}

pub(crate) fn transformed_payload_content_type(
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> &'static str {
    if operation != OperationFamily::StreamGenerateContent {
        return "application/json";
    }
    match protocol {
        ProtocolKind::GeminiNDJson => "application/x-ndjson",
        _ => "text/event-stream",
    }
}

pub(crate) fn encode_transform_stream_error_chunk(
    protocol: ProtocolKind,
    message: String,
) -> Bytes {
    let payload = json!({
        "error": {
            "message": message,
            "type": "transform_serialization_error"
        }
    })
    .to_string();

    let chunk = if protocol == ProtocolKind::GeminiNDJson {
        format!("{payload}\n")
    } else {
        format!("event: error\ndata: {payload}\n\n")
    };
    Bytes::from(chunk)
}

pub(crate) fn should_wrap_payload_for_typed_decode(
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> bool {
    matches!(
        (operation, protocol),
        (OperationFamily::GenerateContent, ProtocolKind::Claude)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::Claude)
            | (OperationFamily::CountToken, ProtocolKind::Claude)
            | (OperationFamily::GenerateContent, ProtocolKind::Gemini)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
            | (
                OperationFamily::StreamGenerateContent,
                ProtocolKind::GeminiNDJson
            )
            | (OperationFamily::CountToken, ProtocolKind::Gemini)
            | (OperationFamily::Embedding, ProtocolKind::Gemini)
            | (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
            | (
                OperationFamily::GenerateContent,
                ProtocolKind::OpenAiChatCompletion
            )
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi)
            | (
                OperationFamily::StreamGenerateContent,
                ProtocolKind::OpenAiChatCompletion
            )
            | (OperationFamily::CountToken, ProtocolKind::OpenAi)
            | (OperationFamily::Embedding, ProtocolKind::OpenAi)
            | (OperationFamily::Compact, ProtocolKind::OpenAi)
    )
}

pub(crate) fn is_full_request_envelope(value: &serde_json::Value) -> bool {
    value.get("method").is_some()
        && value.get("path").is_some()
        && value.get("query").is_some()
        && value.get("headers").is_some()
        && value.get("body").is_some()
}

pub(crate) fn default_http_method_for_operation(operation: OperationFamily) -> &'static str {
    match operation {
        OperationFamily::ModelList | OperationFamily::ModelGet => "GET",
        _ => "POST",
    }
}

pub(crate) fn wrap_payload_for_typed_decode(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: Vec<u8>,
) -> Result<Vec<u8>, UpstreamError> {
    if !should_wrap_payload_for_typed_decode(operation, protocol) {
        return Ok(body);
    }

    let value = serde_json::from_slice::<serde_json::Value>(body.as_slice())
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    if is_full_request_envelope(&value) {
        return Ok(body);
    }

    let mut path = json!({});
    let mut query = json!({});
    let mut headers = json!({});

    let body_value = match protocol {
        ProtocolKind::OpenAi | ProtocolKind::OpenAiChatCompletion => value,
        ProtocolKind::Claude => {
            if let Some(map) = value.as_object() {
                if let Some(item) = map.get("headers") {
                    headers = item.clone();
                }
                map.get("body").cloned().unwrap_or(value)
            } else {
                value
            }
        }
        ProtocolKind::Gemini | ProtocolKind::GeminiNDJson => {
            if let Some(map) = value.as_object() {
                if let Some(item) = map.get("path") {
                    path = item.clone();
                }
                if let Some(item) = map.get("query") {
                    query = item.clone();
                }
                if let Some(item) = map.get("headers") {
                    headers = item.clone();
                }
                map.get("body").cloned().unwrap_or(value)
            } else {
                value
            }
        }
    };

    serde_json::to_vec(&json!({
        "method": default_http_method_for_operation(operation),
        "path": path,
        "query": query,
        "headers": headers,
        "body": body_value,
    }))
    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

pub(crate) fn rewrite_content_type(headers: &mut Vec<(String, String)>, content_type: &str) {
    let mut replaced = false;
    for (name, value) in headers.iter_mut() {
        if name.eq_ignore_ascii_case("content-type") {
            *value = content_type.to_string();
            replaced = true;
        }
    }
    if !replaced {
        headers.push(("content-type".to_string(), content_type.to_string()));
    }
    remove_header_ignore_case(headers, "content-length");
}

pub(crate) fn wrap_stream_with_upstream_record(
    input: std::pin::Pin<
        Box<dyn Stream<Item = Result<Bytes, MiddlewareTransformError>> + Send + 'static>,
    >,
    context: UpstreamStreamRecordContext,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, MiddlewareTransformError>> + Send + 'static>>
{
    let (tx, mut rx) = mpsc::channel::<Result<Bytes, MiddlewareTransformError>>(16);
    tokio::spawn(async move {
        let mut context = context;
        let mut input = input;
        if context.record_stream_usage_event {
            let usage_extracted = attach_usage_extractor(TransformResponsePayload::new(
                context.request.operation(),
                context.request.protocol(),
                input,
            ));
            context.stream_usage = Some(usage_extracted.usage.clone());
            input = usage_extracted.response.body;
        } else {
            context.stream_usage = None;
        }
        let recorder = UpstreamStreamRecordGuard::new(context);
        while let Some(item) = input.next().await {
            match item {
                Ok(chunk) => {
                    recorder.push_chunk(chunk.as_ref());
                    if tx
                        .send(Ok::<Bytes, MiddlewareTransformError>(chunk))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx.send(Err::<Bytes, MiddlewareTransformError>(err)).await;
                    break;
                }
            }
        }
        recorder.flush_now().await;
    });
    Box::pin(async_stream::stream! {
        while let Some(item) = rx.recv().await {
            yield item;
        }
    })
}

pub(crate) fn wrap_io_stream_with_upstream_record(
    input: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static>,
    >,
    context: UpstreamStreamRecordContext,
) -> std::pin::Pin<
    Box<dyn futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static>,
> {
    let (tx, mut rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    tokio::spawn(async move {
        let mut input: std::pin::Pin<
            Box<dyn Stream<Item = Result<Bytes, MiddlewareTransformError>> + Send + 'static>,
        > = Box::pin(input.map(|item| {
            item.map_err(|err| MiddlewareTransformError::ProviderPrefix {
                message: err.to_string(),
            })
        }));
        let mut context = context;
        if context.record_stream_usage_event {
            let usage_extracted = attach_usage_extractor(TransformResponsePayload::new(
                context.request.operation(),
                context.request.protocol(),
                input,
            ));
            context.stream_usage = Some(usage_extracted.usage.clone());
            input = usage_extracted.response.body;
        } else {
            context.stream_usage = None;
        }
        let recorder = UpstreamStreamRecordGuard::new(context);
        while let Some(item) = input.next().await {
            match item {
                Ok(chunk) => {
                    recorder.push_chunk(chunk.as_ref());
                    if tx.send(Ok::<Bytes, std::io::Error>(chunk)).await.is_err() {
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx
                        .send(Err::<Bytes, std::io::Error>(std::io::Error::other(
                            err.to_string(),
                        )))
                        .await;
                    break;
                }
            }
        }
        recorder.flush_now().await;
    });
    Box::pin(async_stream::stream! {
        while let Some(item) = rx.recv().await {
            yield item;
        }
    })
}

pub(crate) fn unwrap_http_wrapper_body_bytes(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    let wrapper = value.as_object()?;
    if !wrapper.contains_key("stats_code") || !wrapper.contains_key("body") {
        return None;
    }
    match wrapper.get("body")? {
        serde_json::Value::String(text) => Some(text.as_bytes().to_vec()),
        body => serde_json::to_vec(body).ok(),
    }
}

pub(crate) async fn transformed_payload_to_axum_response(
    status: StatusCode,
    mut headers: Vec<(String, String)>,
    payload: TransformResponsePayload,
    stream_record_context: Option<UpstreamStreamRecordContext>,
) -> Result<Response, UpstreamError> {
    let content_type = transformed_payload_content_type(payload.operation, payload.protocol);
    rewrite_content_type(&mut headers, content_type);
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    if payload.operation != OperationFamily::StreamGenerateContent {
        let mut body = payload.body;
        let mut collected = Vec::new();
        while let Some(item) = body.next().await {
            let chunk = item.map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            collected.extend_from_slice(chunk.as_ref());
        }
        let client_body = unwrap_http_wrapper_body_bytes(collected.as_slice()).unwrap_or(collected);
        return builder
            .body(Body::from(client_body))
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()));
    }
    let body = if let Some(context) = stream_record_context {
        wrap_stream_with_upstream_record(payload.body, context)
    } else {
        payload.body
    };
    let protocol = payload.protocol;
    let body_stream = body.map(move |item| match item {
        Ok(chunk) => Ok::<Bytes, std::io::Error>(chunk),
        Err(err) => Ok::<Bytes, std::io::Error>(encode_transform_stream_error_chunk(
            protocol,
            err.to_string(),
        )),
    });
    builder
        .body(Body::from_stream(body_stream))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

pub(crate) fn response_from_status_headers_and_bytes(
    status: StatusCode,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> Result<Response, UpstreamError> {
    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder
        .body(Body::from(body))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

pub(crate) fn ensure_stream_usage_option_on_native_chat(request: &mut TransformRequest) {
    if let TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) = request {
        let options = value
            .body
            .stream_options
            .get_or_insert_with(Default::default);
        options.include_usage = Some(true);
    }
}

pub(crate) fn encode_http_response_for_transform(
    status: StatusCode,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Vec<u8>, UpstreamError> {
    let mut header_map = serde_json::Map::new();
    for (name, value) in headers {
        header_map.insert(name.clone(), serde_json::Value::String(value.clone()));
    }
    let body_json = serde_json::from_slice::<serde_json::Value>(body)
        .unwrap_or_else(|_| serde_json::Value::String(String::from_utf8_lossy(body).to_string()));
    serde_json::to_vec(&json!({
        "stats_code": status.as_u16(),
        "headers": header_map,
        "body": body_json,
    }))
    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

pub(crate) fn upstream_response_to_axum_stream(
    response: wreq::Response,
    rewrite_gemini_stream_to_ndjson: bool,
    stream_record_context: Option<UpstreamStreamRecordContext>,
) -> Result<Response, UpstreamError> {
    let stream_channel = stream_record_context
        .as_ref()
        .map(|value| value.channel.clone());
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();

    let is_sse = is_sse_content_type(headers.as_slice());
    let rewrite_stream = rewrite_gemini_stream_to_ndjson && is_sse;
    let unwrap_sse = !rewrite_stream
        && is_sse
        && stream_channel
            .as_ref()
            .map(is_wrapped_stream_channel)
            .unwrap_or(false);
    if rewrite_stream {
        rewrite_content_type_to_ndjson(&mut headers);
        remove_header_ignore_case(&mut headers, "content-length");
    } else if unwrap_sse {
        remove_header_ignore_case(&mut headers, "content-length");
    }

    let mut builder = Response::builder().status(status);
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    if rewrite_stream || unwrap_sse {
        let mut upstream_stream = response.bytes_stream();
        let mut rewriter = SseToNdjsonRewriter::default();
        let base_stream = async_stream::stream! {
            while let Some(item) = upstream_stream.next().await {
                let chunk = match item {
                    Ok(chunk) => chunk,
                    Err(err) => {
                        yield Err::<Bytes, std::io::Error>(std::io::Error::other(err.to_string()));
                        return;
                    }
                };
                let out = rewriter.push_chunk(chunk.as_ref());
                if !out.is_empty() {
                    let normalized = stream_channel
                        .as_ref()
                        .and_then(|channel| {
                            normalize_upstream_stream_ndjson_chunk_for_channel(channel, out.as_slice())
                        })
                        .unwrap_or(out);
                    let output = if rewrite_stream {
                        normalized
                    } else {
                        ndjson_chunk_to_sse_chunk(normalized.as_slice())
                    };
                    if !output.is_empty() {
                        yield Ok::<Bytes, std::io::Error>(Bytes::from(output));
                    }
                }
            }
            let tail = rewriter.finish();
            if !tail.is_empty() {
                let normalized_tail = stream_channel
                    .as_ref()
                    .and_then(|channel| {
                        normalize_upstream_stream_ndjson_chunk_for_channel(
                            channel,
                            tail.as_slice(),
                        )
                    })
                    .unwrap_or(tail);
                let output_tail = if rewrite_stream {
                    normalized_tail
                } else {
                    ndjson_chunk_to_sse_chunk(normalized_tail.as_slice())
                };
                if !output_tail.is_empty() {
                    yield Ok::<Bytes, std::io::Error>(Bytes::from(output_tail));
                }
            }
        };
        let body_stream = if let Some(context) = stream_record_context {
            wrap_io_stream_with_upstream_record(Box::pin(base_stream), context)
        } else {
            Box::pin(base_stream)
        };
        return builder
            .body(Body::from_stream(body_stream))
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()));
    }

    let base_body_stream = response.bytes_stream().map(|item| {
        item.map_err(|err| MiddlewareTransformError::ProviderPrefix {
            message: err.to_string(),
        })
    });
    let base_body_stream = if let Some(context) = stream_record_context {
        wrap_stream_with_upstream_record(Box::pin(base_body_stream), context)
    } else {
        Box::pin(base_body_stream)
    }
    .map(|item| item.map_err(|err| std::io::Error::other(err.to_string())));
    builder
        .body(Body::from_stream(base_body_stream))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}
