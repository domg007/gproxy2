use super::*;

pub(crate) async fn estimate_embedding_input_tokens_from_usage_request(
    state: &AppState,
    request: &UsageRequestContext,
) -> Option<i64> {
    if request.operation() != OperationFamily::Embedding {
        return None;
    }
    let model = request.model.as_ref()?.trim().to_string();
    if model.is_empty() {
        return None;
    }
    let body = request.body_for_estimate.as_ref()?;
    let mut value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    strip_model_fields(&mut value);
    let text = serde_json::to_string(&value).ok()?;
    let count = state
        .count_tokens_with_local_tokenizer(model.as_str(), text.as_str())
        .await
        .ok()?
        .count;
    i64::try_from(count).ok()
}

pub(crate) fn build_openai_count_request(
    model: &str,
    text: &str,
) -> openai_count_tokens_request::OpenAiCountTokensRequest {
    let mut request = openai_count_tokens_request::OpenAiCountTokensRequest::default();
    request.body.model = Some(model.to_string());
    request.body.input = Some(ResponseInput::Text(text.to_string()));
    request
}

pub(crate) fn normalize_count_source_text(source: &str) -> String {
    if source.trim().is_empty() {
        " ".to_string()
    } else {
        source.to_string()
    }
}

pub(crate) async fn estimate_tokens_with_channel_count(
    context: &UpstreamStreamRecordContext,
    model: &str,
    text: &str,
) -> Option<i64> {
    let source = normalize_count_source_text(text);
    let openai_request = build_openai_count_request(model, source.as_str());
    let mut candidates = vec![(
        ProtocolKind::OpenAi,
        TransformRequest::CountTokenOpenAi(openai_request.clone()),
    )];
    if let Ok(request) =
        claude_count_tokens_request::ClaudeCountTokensRequest::try_from(openai_request.clone())
    {
        candidates.push((
            ProtocolKind::Claude,
            TransformRequest::CountTokenClaude(request),
        ));
    }
    if let Ok(request) =
        gemini_count_tokens_request::GeminiCountTokensRequest::try_from(openai_request)
    {
        candidates.push((
            ProtocolKind::Gemini,
            TransformRequest::CountTokenGemini(request),
        ));
    }

    for (source_protocol, source_request) in candidates {
        let source_route = RouteKey::new(OperationFamily::CountToken, source_protocol);
        let Some(implementation) = context.provider.dispatch.resolve(source_route).cloned() else {
            continue;
        };
        let mut upstream_request = source_request.clone();
        let mut upstream_protocol = source_protocol;
        let execute_local = match implementation {
            RouteImplementation::Unsupported => continue,
            RouteImplementation::Local => true,
            RouteImplementation::Passthrough => false,
            RouteImplementation::TransformTo { destination } => {
                let route = gproxy_middleware::TransformRoute {
                    src_operation: source_route.operation,
                    src_protocol: source_route.protocol,
                    dst_operation: destination.operation,
                    dst_protocol: destination.protocol,
                };
                if !route.is_passthrough() {
                    let Ok(transformed) =
                        gproxy_middleware::transform_request(upstream_request.clone(), route)
                    else {
                        continue;
                    };
                    upstream_request = transformed;
                }
                upstream_protocol = destination.protocol;
                false
            }
        };

        if execute_local {
            let Ok(local) =
                execute_local_count_token_request(context.state.as_ref(), &source_request).await
            else {
                continue;
            };
            let Some(local_response) = local.local_response.as_ref() else {
                continue;
            };
            if let Some(tokens) = extract_local_count_input_tokens(local_response) {
                return Some(tokens);
            }
            continue;
        }

        let now = now_unix_ms();
        let http = context.state.load_http();
        let spoof_http = matches!(
            &context.channel,
            ChannelId::Builtin(BuiltinChannel::ClaudeCode)
        )
        .then(|| context.state.load_spoof_http());
        let tokenizers = context.state.tokenizers();
        let global = context.state.load_config().global.clone();
        let Ok(upstream) = context
            .provider
            .execute_with_retry_with_spoof(
                http.as_ref(),
                spoof_http.as_deref(),
                context.state.credential_states(),
                &upstream_request,
                now,
                TokenizerResolutionContext {
                    tokenizer_store: tokenizers.as_ref(),
                    hf_token: global.hf_token.as_deref(),
                    hf_url: global.hf_url.as_deref(),
                },
            )
            .await
        else {
            continue;
        };

        if let Some(local_response) = upstream.local_response.as_ref()
            && let Some(tokens) = extract_local_count_input_tokens(local_response)
        {
            return Some(tokens);
        }

        let Some(response) = upstream.response else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(bytes) = response.bytes().await else {
            continue;
        };
        if let Some(tokens) = extract_count_tokens_from_raw_json(upstream_protocol, bytes.as_ref())
        {
            return Some(tokens);
        }
    }

    None
}

pub(crate) async fn estimate_tokens_for_text(
    context: &UpstreamStreamRecordContext,
    model: &str,
    text: &str,
) -> i64 {
    if let Some(tokens) = estimate_tokens_with_channel_count(context, model, text).await {
        return tokens.max(0);
    }
    context
        .state
        .count_tokens_with_local_tokenizer(model, text)
        .await
        .map(|count| i64::try_from(count.count).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub(crate) async fn enqueue_stream_usage_event_with_estimate(
    context: &UpstreamStreamRecordContext,
    stream_response_body: &[u8],
    stream_usage: Option<UsageSnapshot>,
) {
    if !should_record_usage(context.request.operation())
        || context
            .response_status
            .map(|status| status >= 400)
            .unwrap_or(true)
    {
        return;
    }

    let request_model = normalize_usage_model(context.request.model.clone());
    let model = request_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("deepseek_fallback")
        .to_string();

    let mut usage = stream_usage;
    let mut input_tokens = usage
        .as_ref()
        .and_then(|value| value.input_tokens)
        .map(u64_to_i64);
    let mut output_tokens = usage
        .as_ref()
        .and_then(|value| value.output_tokens)
        .map(u64_to_i64);
    let mut cache_read_input_tokens = usage
        .as_ref()
        .and_then(|value| value.cache_read_input_tokens)
        .map(u64_to_i64);
    let mut cache_creation_input_tokens = usage
        .as_ref()
        .and_then(|value| value.cache_creation_input_tokens)
        .map(u64_to_i64);
    let mut cache_creation_input_tokens_5min = usage
        .as_ref()
        .and_then(|value| value.cache_creation_input_tokens_5min)
        .map(u64_to_i64);
    let mut cache_creation_input_tokens_1h = usage
        .as_ref()
        .and_then(|value| value.cache_creation_input_tokens_1h)
        .map(u64_to_i64);

    if let Some(total) = usage
        .as_ref()
        .and_then(|value| value.total_tokens)
        .map(u64_to_i64)
    {
        match (input_tokens, output_tokens) {
            (None, Some(output)) => {
                input_tokens = Some(total.saturating_sub(output));
            }
            (Some(input), None) => {
                output_tokens = Some(total.saturating_sub(input));
            }
            _ => {}
        }
    }

    if input_tokens.is_none()
        && output_tokens.is_none()
        && cache_read_input_tokens.is_none()
        && cache_creation_input_tokens.is_none()
        && cache_creation_input_tokens_5min.is_none()
        && cache_creation_input_tokens_1h.is_none()
    {
        usage = extract_usage_from_stream_body(
            context.request.operation(),
            context.request.protocol(),
            stream_response_body,
        )
        .await;
        if let Some(parsed_usage) = usage.as_ref() {
            input_tokens = parsed_usage.input_tokens.map(u64_to_i64);
            output_tokens = parsed_usage.output_tokens.map(u64_to_i64);
            cache_read_input_tokens = parsed_usage.cache_read_input_tokens.map(u64_to_i64);
            cache_creation_input_tokens = parsed_usage.cache_creation_input_tokens.map(u64_to_i64);
            cache_creation_input_tokens_5min = parsed_usage
                .cache_creation_input_tokens_5min
                .map(u64_to_i64);
            cache_creation_input_tokens_1h =
                parsed_usage.cache_creation_input_tokens_1h.map(u64_to_i64);
            if let Some(total) = parsed_usage.total_tokens.map(u64_to_i64) {
                match (input_tokens, output_tokens) {
                    (None, Some(output)) => {
                        input_tokens = Some(total.saturating_sub(output));
                    }
                    (Some(input), None) => {
                        output_tokens = Some(total.saturating_sub(input));
                    }
                    _ => {}
                }
            }
        }
    }

    if input_tokens.is_none()
        && output_tokens.is_none()
        && cache_read_input_tokens.is_none()
        && cache_creation_input_tokens.is_none()
        && cache_creation_input_tokens_5min.is_none()
        && cache_creation_input_tokens_1h.is_none()
    {
        let request_text = context
            .request
            .body_for_estimate
            .as_deref()
            .map(|body| {
                serde_json::from_slice::<serde_json::Value>(body)
                    .ok()
                    .and_then(|value| serde_json::to_string(&value).ok())
                    .unwrap_or_else(|| String::from_utf8_lossy(body).to_string())
            })
            .unwrap_or_default();
        let response_text = String::from_utf8_lossy(stream_response_body).to_string();

        input_tokens =
            Some(estimate_tokens_for_text(context, model.as_str(), request_text.as_str()).await);
        output_tokens =
            Some(estimate_tokens_for_text(context, model.as_str(), response_text.as_str()).await);
    }

    let usage_event = UsageWrite {
        downstream_trace_id: context.auth.downstream_trace_id,
        at_unix_ms: now_unix_ms_i64(),
        provider_id: context.provider_id,
        credential_id: context.credential_id,
        user_id: Some(context.auth.user_id),
        user_key_id: Some(context.auth.user_key_id),
        operation: format!("{:?}", context.request.operation()),
        protocol: format!("{:?}", context.request.protocol()),
        model: request_model,
        input_tokens: input_tokens.map(|value| value.max(0)),
        output_tokens: output_tokens.map(|value| value.max(0)),
        cache_read_input_tokens,
        cache_creation_input_tokens,
        cache_creation_input_tokens_5min,
        cache_creation_input_tokens_1h,
    };
    if let Err(err) = context
        .state
        .enqueue_storage_write(StorageWriteEvent::UpsertUsage(usage_event))
        .await
    {
        eprintln!("provider: stream usage event enqueue failed: {err}");
    }
}

pub(crate) async fn extract_usage_from_stream_body(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Option<UsageSnapshot> {
    if body.is_empty() || operation != OperationFamily::StreamGenerateContent {
        return None;
    }
    match protocol {
        ProtocolKind::OpenAi => extract_openai_response_usage_from_sse(body),
        _ => None,
    }
}

pub(crate) fn extract_openai_response_usage_from_sse(body: &[u8]) -> Option<UsageSnapshot> {
    let mut cursor = 0usize;
    let mut latest = None;
    while let Some(frame) = next_sse_frame(body, &mut cursor) {
        let Some(data) = parse_sse_data_from_frame(frame) else {
            continue;
        };
        if data == "[DONE]" {
            continue;
        }
        let Ok(event) = serde_json::from_str::<
            gproxy_protocol::openai::create_response::stream::ResponseStreamEvent,
        >(&data) else {
            continue;
        };
        if let Some(usage) = usage_from_openai_response_stream_event(&event) {
            latest = Some(usage);
        }
    }
    latest
}

pub(crate) fn usage_from_openai_response_stream_event(
    event: &gproxy_protocol::openai::create_response::stream::ResponseStreamEvent,
) -> Option<UsageSnapshot> {
    use gproxy_protocol::openai::create_response::stream::ResponseStreamEvent;
    match event {
        ResponseStreamEvent::Created { response, .. }
        | ResponseStreamEvent::Queued { response, .. }
        | ResponseStreamEvent::InProgress { response, .. }
        | ResponseStreamEvent::Failed { response, .. }
        | ResponseStreamEvent::Incomplete { response, .. }
        | ResponseStreamEvent::Completed { response, .. } => response
            .usage
            .as_ref()
            .map(usage_from_openai_response_usage),
        _ => None,
    }
}

pub(crate) fn usage_from_openai_response_usage(usage: &ResponseUsage) -> UsageSnapshot {
    UsageSnapshot {
        input_tokens: Some(usage.input_tokens),
        output_tokens: Some(usage.output_tokens),
        total_tokens: Some(usage.total_tokens),
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5min: None,
        cache_creation_input_tokens_1h: None,
        cache_read_input_tokens: Some(usage.input_tokens_details.cached_tokens),
        reasoning_tokens: Some(usage.output_tokens_details.reasoning_tokens),
        thoughts_tokens: None,
        tool_use_prompt_tokens: None,
    }
}

pub(crate) fn next_sse_frame<'a>(body: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    if *cursor >= body.len() {
        return None;
    }
    let rest = &body[*cursor..];
    let lf_pos = rest.windows(2).position(|window| window == b"\n\n");
    let crlf_pos = rest.windows(4).position(|window| window == b"\r\n\r\n");
    let (pos, delim_len) = match (lf_pos, crlf_pos) {
        (Some(a), Some(b)) if a <= b => (a, 2),
        (Some(_), Some(b)) => (b, 4),
        (Some(a), None) => (a, 2),
        (None, Some(b)) => (b, 4),
        (None, None) => {
            *cursor = body.len();
            return Some(rest);
        }
    };
    let frame = &rest[..pos];
    *cursor += pos + delim_len;
    Some(frame)
}

pub(crate) fn parse_sse_data_from_frame(frame: &[u8]) -> Option<String> {
    if frame.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(frame).ok()?;
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("data:") {
            lines.push(value.trim_start().to_string());
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}
