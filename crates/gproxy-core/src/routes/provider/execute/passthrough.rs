use super::*;

pub(crate) async fn collect_request_payload_body_bytes(
    request: TransformRequestPayload,
) -> Result<(OperationFamily, ProtocolKind, Vec<u8>), UpstreamError> {
    let mut out = Vec::new();
    let mut body = request.body;
    while let Some(item) = body.next().await {
        let chunk = item.map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        out.extend_from_slice(chunk.as_ref());
    }
    Ok((request.operation, request.protocol, out))
}

pub(crate) fn shape_passthrough_request(
    _channel: &ChannelId,
    _operation: OperationFamily,
    _protocol: ProtocolKind,
    body_bytes: Vec<u8>,
) -> Vec<u8> {
    body_bytes
}

pub(crate) fn shape_passthrough_response(
    channel: &ChannelId,
    _operation: OperationFamily,
    _protocol: ProtocolKind,
    _status: StatusCode,
    headers: &[(String, String)],
    body_bytes: Vec<u8>,
) -> (Vec<(String, String)>, Vec<u8>) {
    let normalized_body =
        normalize_upstream_response_body_for_channel(channel, body_bytes.as_ref())
            .unwrap_or_else(|| body_bytes.clone());
    let mut normalized_headers = headers.to_vec();
    if normalized_body != body_bytes {
        remove_header_ignore_case(&mut normalized_headers, "content-length");
    }
    (normalized_headers, normalized_body)
}

pub(crate) async fn execute_passthrough_payload_request(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    request: TransformRequestPayload,
) -> Result<Response, UpstreamError> {
    let (operation, protocol, request_bytes) = collect_request_payload_body_bytes(request).await?;
    let request_bytes = shape_passthrough_request(&channel, operation, protocol, request_bytes);
    let request_context =
        usage_request_context_from_payload(operation, protocol, request_bytes.as_slice());
    let provider_id = resolve_provider_id(state.as_ref(), &channel).await.ok();
    let now = now_unix_ms();
    let http = state.load_http();
    let spoof_http = matches!(
        &channel,
        ChannelId::Builtin(BuiltinChannel::ClaudeCode)
    )
    .then(|| state.load_spoof_http());
    let tokenizers = state.tokenizers();
    let global = state.load_config().global.clone();

    let (upstream_result, tracked_http_events) = capture_tracked_http_events(async {
        provider
            .execute_payload_with_retry_with_spoof(
                http.as_ref(),
                spoof_http.as_deref(),
                state.credential_states(),
                RetryWithPayloadRequest {
                    operation,
                    protocol,
                    body: request_bytes.as_slice(),
                    now_unix_ms: now,
                    token_resolution: TokenizerResolutionContext {
                        tokenizer_store: tokenizers.as_ref(),
                        hf_token: global.hf_token.as_deref(),
                        hf_url: global.hf_url.as_deref(),
                    },
                },
            )
            .await
    })
    .await;

    enqueue_credential_status_updates_for_request(state.as_ref(), &channel, &provider, now).await;

    let upstream = match upstream_result {
        Ok(value) => value,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            let err_credential_id = upstream_error_credential_id(&err);
            let err_status = upstream_error_status(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                err_credential_id,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                err_credential_id,
                err_request_meta.as_ref(),
                crate::routes::provider::UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
            )
            .await;
            return Err(err);
        }
    };

    let upstream_credential_id = upstream.credential_id;
    let upstream_request_meta = upstream.request_meta.clone();
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        upstream_credential_id,
        tracked_http_events.as_slice(),
        upstream_request_meta.as_ref(),
    )
    .await;

    if let Some(update) = upstream.credential_update.clone() {
        apply_credential_update_and_persist(
            state.clone(),
            channel.clone(),
            provider.clone(),
            update,
        )
        .await;
    }

    if let Some(local) = upstream.local_response {
        let body = serialize_local_response_body(&local)?;
        enqueue_upstream_and_usage_event(
            state.as_ref(),
            UpstreamAndUsageEventInput {
                auth,
                request: &request_context,
                provider_id,
                credential_id: upstream_credential_id,
                request_meta: upstream_request_meta.as_ref(),
                error_status: None,
                response_status: Some(200),
                response_headers: &[("content-type".to_string(), "application/json".to_string())],
                response_body: None,
                local_response: Some(&local),
            },
        )
        .await;
        let response = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        return Ok(response);
    }

    if let Some(response) = upstream.response {
        let response_status = response.status().as_u16();
        let response_headers = response_headers_to_pairs(&response);
        if operation.is_stream() || is_streaming_content_type(response_headers.as_slice()) {
            let rewrite_gemini_stream_to_ndjson = operation
                == OperationFamily::StreamGenerateContent
                && protocol == ProtocolKind::GeminiNDJson;
            let stream_record_context = UpstreamStreamRecordContext {
                state: state.clone(),
                channel: channel.clone(),
                provider: provider.clone(),
                auth,
                request: request_context.clone(),
                provider_id,
                credential_id: upstream_credential_id,
                request_meta: upstream_request_meta.clone(),
                response_status: Some(response_status),
                response_headers: response_headers.clone(),
                stream_usage: None,
                record_upstream_event: true,
                record_stream_usage_event: true,
            };
            return upstream_response_to_axum_stream(
                response,
                rewrite_gemini_stream_to_ndjson,
                Some(stream_record_context),
            );
        }

        let status =
            StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_string(), value.to_string()))
            })
            .collect::<Vec<_>>();
        let body_bytes = response
            .bytes()
            .await
            .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
        let raw_body = body_bytes.to_vec();
        let (headers_for_client, normalized_body) = shape_passthrough_response(
            &channel,
            operation,
            protocol,
            status,
            headers.as_slice(),
            raw_body.clone(),
        );
        let encoded_for_usage = encode_http_response_for_transform(
            status,
            headers_for_client.as_slice(),
            normalized_body.as_ref(),
        )?;
        let usage_source_response =
            decode_response_for_usage(operation, protocol, encoded_for_usage.as_ref());
        enqueue_upstream_and_usage_event(
            state.as_ref(),
            UpstreamAndUsageEventInput {
                auth,
                request: &request_context,
                provider_id,
                credential_id: upstream_credential_id,
                request_meta: upstream_request_meta.as_ref(),
                error_status: None,
                response_status: Some(response_status),
                response_headers: response_headers.as_slice(),
                response_body: Some(raw_body.clone()),
                local_response: usage_source_response.as_ref(),
            },
        )
        .await;
        return response_from_status_headers_and_bytes(
            status,
            headers_for_client.as_slice(),
            normalized_body,
        );
    }

    enqueue_upstream_and_usage_event(
        state.as_ref(),
        UpstreamAndUsageEventInput {
            auth,
            request: &request_context,
            provider_id,
            credential_id: upstream_credential_id,
            request_meta: upstream_request_meta.as_ref(),
            error_status: None,
            response_status: None,
            response_headers: &[],
            response_body: None,
            local_response: None,
        },
    )
    .await;
    Err(UpstreamError::UpstreamRequest(
        "upstream returned empty response".to_string(),
    ))
}
