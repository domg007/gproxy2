use super::{
    AppState, Arc, Body, BuiltinChannel, Bytes, ChannelId, HttpError, MiddlewareTransformError,
    OperationFamily, ProtocolKind, ProviderDefinition, RequestAuthContext, Response,
    RetryWithPayloadRequest, RouteImplementation, RouteKey, SseToNdjsonRewriter, StatusCode,
    Stream, TokenizerResolutionContext, TransformRequest, TransformRequestPayload,
    TransformResponsePayload, UpstreamAndUsageEventInput, UpstreamError, UpstreamResponse,
    UpstreamStreamRecordContext, UpstreamStreamRecordGuard, apply_credential_update_and_persist,
    attach_usage_extractor, capture_tracked_http_events, claude_count_tokens_response,
    decode_response_for_usage, enqueue_credential_status_updates_for_request,
    enqueue_internal_tracked_http_events, enqueue_upstream_and_usage_event,
    enqueue_upstream_request_event_from_meta, gemini_count_tokens_response,
    is_wrapped_stream_channel, json, mpsc, ndjson_chunk_to_sse_chunk,
    normalize_upstream_response_body_for_channel,
    normalize_upstream_stream_ndjson_chunk_for_channel, now_unix_ms, openai_count_tokens_request,
    openai_count_tokens_response, resolve_provider_id, response_headers_to_pairs,
    try_local_response_for_channel, upstream_error_credential_id, upstream_error_request_meta,
    upstream_error_status, usage_request_context_from_payload,
    usage_request_context_from_transform_request,
};
use futures_util::StreamExt;

mod io;
pub(super) use io::*;
mod local;
pub(super) use local::*;
mod passthrough;
pub(super) use passthrough::*;

pub(super) async fn execute_transform_request(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    request: TransformRequest,
) -> Result<Response, UpstreamError> {
    let downstream_request = request;
    let mut upstream_request = downstream_request.clone();
    let downstream_request_context =
        usage_request_context_from_transform_request(&downstream_request);
    let mut dispatch_route = None;
    let mut dispatch_local = false;
    let provider_id = resolve_provider_id(state.as_ref(), &channel).await.ok();
    let src_route = RouteKey::new(
        downstream_request.operation(),
        downstream_request.protocol(),
    );
    let Some(implementation) = provider.dispatch.resolve(src_route).cloned() else {
        enqueue_upstream_and_usage_event(
            state.as_ref(),
            UpstreamAndUsageEventInput {
                auth,
                request: &downstream_request_context,
                provider_id,
                credential_id: None,
                request_meta: None,
                error_status: None,
                response_status: None,
                response_headers: &[],
                response_body: None,
                local_response: None,
            },
        )
        .await;
        return Err(UpstreamError::UnsupportedRequest);
    };

    match implementation {
        RouteImplementation::Passthrough => {}
        RouteImplementation::TransformTo { destination } => {
            let route = gproxy_middleware::TransformRoute {
                src_operation: src_route.operation,
                src_protocol: src_route.protocol,
                dst_operation: destination.operation,
                dst_protocol: destination.protocol,
            };
            if gproxy_middleware::select_request_lane(route)
                == gproxy_middleware::TransformLane::Typed
            {
                match gproxy_middleware::transform_request(downstream_request.clone(), route) {
                    Ok(transformed) => {
                        upstream_request = transformed;
                    }
                    Err(err) => {
                        let upstream_error = UpstreamError::SerializeRequest(err.to_string());
                        enqueue_upstream_and_usage_event(
                            state.as_ref(),
                            UpstreamAndUsageEventInput {
                                auth,
                                request: &downstream_request_context,
                                provider_id,
                                credential_id: None,
                                request_meta: None,
                                error_status: None,
                                response_status: None,
                                response_headers: &[],
                                response_body: None,
                                local_response: None,
                            },
                        )
                        .await;
                        return Err(upstream_error);
                    }
                }
            }
            dispatch_route = Some(route);
        }
        RouteImplementation::Local => {
            dispatch_local = true;
        }
        RouteImplementation::Unsupported => {
            enqueue_upstream_and_usage_event(
                state.as_ref(),
                UpstreamAndUsageEventInput {
                    auth,
                    request: &downstream_request_context,
                    provider_id,
                    credential_id: None,
                    request_meta: None,
                    error_status: None,
                    response_status: None,
                    response_headers: &[],
                    response_body: None,
                    local_response: None,
                },
            )
            .await;
            return Err(UpstreamError::UnsupportedRequest);
        }
    }

    let now = now_unix_ms();
    ensure_stream_usage_option_on_native_chat(&mut upstream_request);
    let upstream_request_context = usage_request_context_from_transform_request(&upstream_request);
    let (upstream_result, tracked_http_events) = if dispatch_local {
        (
            execute_local_request(state.as_ref(), &provider, &downstream_request).await,
            Vec::new(),
        )
    } else {
        let http = state.load_http();
        let spoof_http = matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode))
            .then(|| state.load_spoof_http());
        let tokenizers = state.tokenizers();
        let global = state.load_config().global.clone();

        capture_tracked_http_events(async {
            provider
                .execute_with_retry_with_spoof(
                    http.as_ref(),
                    spoof_http.as_deref(),
                    state.credential_states(),
                    &upstream_request,
                    now,
                    TokenizerResolutionContext {
                        tokenizer_store: tokenizers.as_ref(),
                        hf_token: global.hf_token.as_deref(),
                        hf_url: global.hf_url.as_deref(),
                    },
                )
                .await
        })
        .await
    };
    if !dispatch_local {
        enqueue_credential_status_updates_for_request(state.as_ref(), &channel, &provider, now)
            .await;
    }
    let upstream = match upstream_result {
        Ok(value) => value,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            let err_credential_id = upstream_error_credential_id(&err);
            let err_status = upstream_error_status(&err);
            if !dispatch_local {
                enqueue_internal_tracked_http_events(
                    state.as_ref(),
                    auth.downstream_trace_id,
                    provider_id,
                    err_credential_id,
                    tracked_http_events.as_slice(),
                    err_request_meta.as_ref(),
                )
                .await;
            }
            enqueue_upstream_and_usage_event(
                state.as_ref(),
                UpstreamAndUsageEventInput {
                    auth,
                    request: &downstream_request_context,
                    provider_id,
                    credential_id: err_credential_id,
                    request_meta: err_request_meta.as_ref(),
                    error_status: err_status,
                    response_status: None,
                    response_headers: &[],
                    response_body: None,
                    local_response: None,
                },
            )
            .await;
            return Err(err);
        }
    };
    let upstream_credential_id = upstream.credential_id;
    let upstream_request_meta = upstream.request_meta.clone();
    if !dispatch_local {
        enqueue_internal_tracked_http_events(
            state.as_ref(),
            auth.downstream_trace_id,
            provider_id,
            upstream_credential_id,
            tracked_http_events.as_slice(),
            upstream_request_meta.as_ref(),
        )
        .await;
    }

    if let Some(update) = upstream.credential_update.clone() {
        apply_credential_update_and_persist(
            state.clone(),
            channel.clone(),
            provider.clone(),
            update,
        )
        .await;
    }

    if let Some(mut local) = upstream.local_response {
        let usage_source_response = local.clone();
        if let Some(route) = dispatch_route.filter(|item| {
            gproxy_middleware::select_response_lane(*item)
                == gproxy_middleware::TransformLane::Typed
        }) {
            local = gproxy_middleware::transform_response(local, route)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        }
        enqueue_upstream_and_usage_event(
            state.as_ref(),
            UpstreamAndUsageEventInput {
                auth,
                request: &upstream_request_context,
                provider_id,
                credential_id: upstream_credential_id,
                request_meta: upstream_request_meta.as_ref(),
                error_status: None,
                response_status: Some(200),
                response_headers: &[],
                response_body: None,
                local_response: Some(&usage_source_response),
            },
        )
        .await;
        let body = serialize_local_response_body(&local)?;
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
        if let Some(route) = dispatch_route.filter(|item| {
            gproxy_middleware::select_response_lane(*item)
                == gproxy_middleware::TransformLane::Typed
        }) {
            if !response.status().is_success() {
                let stream_record_context = UpstreamStreamRecordContext {
                    state: state.clone(),
                    channel: channel.clone(),
                    provider: provider.clone(),
                    auth,
                    request: upstream_request_context.clone(),
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
                    false,
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
            let transformed_payload = if route.dst_operation
                == OperationFamily::StreamGenerateContent
            {
                let body_stream = response.bytes_stream().map(|item| {
                    item.map_err(|err| MiddlewareTransformError::ProviderPrefix {
                        message: err.to_string(),
                    })
                });
                let body_stream: std::pin::Pin<
                    Box<
                        dyn Stream<Item = Result<Bytes, MiddlewareTransformError>> + Send + 'static,
                    >,
                > = if is_wrapped_stream_channel(&channel)
                    && matches!(
                        route.dst_protocol,
                        ProtocolKind::Gemini | ProtocolKind::GeminiNDJson
                    ) {
                    let mut upstream_stream = Box::pin(body_stream);
                    let wrapped_channel = channel.clone();
                    let dst_protocol = route.dst_protocol;
                    Box::pin(async_stream::stream! {
                        let mut rewriter = SseToNdjsonRewriter::default();
                        while let Some(item) = upstream_stream.next().await {
                            let chunk = match item {
                                Ok(chunk) => chunk,
                                Err(err) => {
                                    yield Err::<Bytes, MiddlewareTransformError>(err);
                                    return;
                                }
                            };
                            let out = rewriter.push_chunk(chunk.as_ref());
                            if !out.is_empty() {
                                let normalized = normalize_upstream_stream_ndjson_chunk_for_channel(
                                    &wrapped_channel,
                                    out.as_slice(),
                                )
                                .unwrap_or(out);
                                let emitted = if dst_protocol == ProtocolKind::Gemini {
                                    ndjson_chunk_to_sse_chunk(normalized.as_slice())
                                } else {
                                    normalized
                                };
                                if !emitted.is_empty() {
                                    yield Ok::<Bytes, MiddlewareTransformError>(Bytes::from(emitted));
                                }
                            }
                        }
                        let tail = rewriter.finish();
                        if !tail.is_empty() {
                            let normalized_tail = normalize_upstream_stream_ndjson_chunk_for_channel(
                                &wrapped_channel,
                                tail.as_slice(),
                            )
                            .unwrap_or(tail);
                            let emitted_tail = if dst_protocol == ProtocolKind::Gemini {
                                ndjson_chunk_to_sse_chunk(normalized_tail.as_slice())
                            } else {
                                normalized_tail
                            };
                            if !emitted_tail.is_empty() {
                                yield Ok::<Bytes, MiddlewareTransformError>(Bytes::from(emitted_tail));
                            }
                        }
                    })
                } else {
                    Box::pin(body_stream)
                };
                // Both upstream logs and usage capture are emitted after upstream normalization,
                // but before cross-protocol transform.
                let stream_record_context = UpstreamStreamRecordContext {
                    state: state.clone(),
                    channel: channel.clone(),
                    provider: provider.clone(),
                    auth,
                    request: upstream_request_context.clone(),
                    provider_id,
                    credential_id: upstream_credential_id,
                    request_meta: upstream_request_meta.clone(),
                    response_status: Some(response_status),
                    response_headers: response_headers.clone(),
                    stream_usage: None,
                    record_upstream_event: true,
                    record_stream_usage_event: true,
                };
                let body_stream =
                    wrap_stream_with_upstream_record(body_stream, stream_record_context);
                gproxy_middleware::transform_response_payload(
                    TransformResponsePayload::new(
                        route.dst_operation,
                        route.dst_protocol,
                        body_stream,
                    ),
                    route,
                )
                .await
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
            } else {
                let body_bytes = response
                    .bytes()
                    .await
                    .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
                let raw_body = body_bytes.to_vec();
                let normalized_body =
                    normalize_upstream_response_body_for_channel(&channel, body_bytes.as_ref())
                        .unwrap_or_else(|| raw_body.clone());
                let encoded = encode_http_response_for_transform(
                    status,
                    headers.as_slice(),
                    normalized_body.as_ref(),
                )?;
                let usage_source_response = decode_response_for_usage(
                    route.dst_operation,
                    route.dst_protocol,
                    encoded.as_ref(),
                );
                enqueue_upstream_and_usage_event(
                    state.as_ref(),
                    UpstreamAndUsageEventInput {
                        auth,
                        request: &upstream_request_context,
                        provider_id,
                        credential_id: upstream_credential_id,
                        request_meta: upstream_request_meta.as_ref(),
                        error_status: None,
                        response_status: Some(response_status),
                        response_headers: response_headers.as_slice(),
                        response_body: Some(raw_body),
                        local_response: usage_source_response.as_ref(),
                    },
                )
                .await;
                let body_stream =
                    futures_util::stream::once(async move { Ok(Bytes::from(encoded)) });
                gproxy_middleware::transform_response_payload(
                    TransformResponsePayload::new(
                        route.dst_operation,
                        route.dst_protocol,
                        Box::pin(body_stream),
                    ),
                    route,
                )
                .await
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?
            };
            return transformed_payload_to_axum_response(
                status,
                headers,
                transformed_payload,
                None,
            )
            .await;
        }
        if should_rewrite_gemini_stream_to_ndjson(&downstream_request)
            || is_streaming_content_type(response_headers.as_slice())
        {
            let stream_record_context = UpstreamStreamRecordContext {
                state: state.clone(),
                channel: channel.clone(),
                provider: provider.clone(),
                auth,
                request: upstream_request_context.clone(),
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
                should_rewrite_gemini_stream_to_ndjson(&downstream_request),
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
        let normalized_body =
            normalize_upstream_response_body_for_channel(&channel, body_bytes.as_ref())
                .unwrap_or_else(|| raw_body.clone());
        let encoded_for_usage = encode_http_response_for_transform(
            status,
            headers.as_slice(),
            normalized_body.as_ref(),
        )?;
        let usage_source_response = decode_response_for_usage(
            upstream_request.operation(),
            upstream_request.protocol(),
            encoded_for_usage.as_ref(),
        );
        enqueue_upstream_and_usage_event(
            state.as_ref(),
            UpstreamAndUsageEventInput {
                auth,
                request: &upstream_request_context,
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
        let mut headers_for_client = headers.clone();
        if normalized_body != raw_body {
            remove_header_ignore_case(&mut headers_for_client, "content-length");
        }
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
            request: &upstream_request_context,
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

pub(super) async fn execute_transform_request_payload(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    request: TransformRequestPayload,
) -> Result<Response, UpstreamError> {
    let src_route = RouteKey::new(request.operation, request.protocol);
    let Some(implementation) = provider.dispatch.resolve(src_route).cloned() else {
        return Err(UpstreamError::UnsupportedRequest);
    };

    let route = match implementation {
        RouteImplementation::Passthrough => Some(gproxy_middleware::TransformRoute {
            src_operation: src_route.operation,
            src_protocol: src_route.protocol,
            dst_operation: src_route.operation,
            dst_protocol: src_route.protocol,
        }),
        RouteImplementation::TransformTo { destination } => {
            Some(gproxy_middleware::TransformRoute {
                src_operation: src_route.operation,
                src_protocol: src_route.protocol,
                dst_operation: destination.operation,
                dst_protocol: destination.protocol,
            })
        }
        RouteImplementation::Local => None,
        RouteImplementation::Unsupported => return Err(UpstreamError::UnsupportedRequest),
    };

    if let Some(route) = route
        && gproxy_middleware::select_request_lane(route) == gproxy_middleware::TransformLane::Raw
    {
        return execute_passthrough_payload_request(state, channel, provider, auth, request).await;
    }

    let (operation, protocol, request_bytes) = collect_request_payload_body_bytes(request).await?;
    let request_bytes = wrap_payload_for_typed_decode(operation, protocol, request_bytes)?;
    let decoded =
        gproxy_middleware::decode_request_payload(operation, protocol, request_bytes.as_slice())
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    execute_transform_request(state, channel, provider, auth, decoded).await
}

pub(super) async fn execute_transform_candidates(
    state: Arc<AppState>,
    channel: ChannelId,
    provider: ProviderDefinition,
    auth: RequestAuthContext,
    candidates: Vec<TransformRequest>,
) -> Result<Response, HttpError> {
    let mut unsupported = false;
    for candidate in candidates {
        match execute_transform_request(
            state.clone(),
            channel.clone(),
            provider.clone(),
            auth,
            candidate,
        )
        .await
        {
            Ok(response) => return Ok(response),
            Err(UpstreamError::UnsupportedRequest) => {
                unsupported = true;
            }
            Err(err) => return Err(HttpError::from(err)),
        }
    }
    if unsupported {
        return Err(HttpError::from(UpstreamError::UnsupportedRequest));
    }
    Err(HttpError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "no provider route candidate executed",
    ))
}

#[cfg(test)]
mod tests {
    use gproxy_middleware::TransformResponse;
    use gproxy_middleware::{OperationFamily, ProtocolKind};
    use gproxy_protocol::openai::model_list::response::OpenAiModelListResponse;
    use serde_json::json;

    use super::{
        encode_transform_stream_error_chunk, serialize_local_response_body,
        wrap_payload_for_typed_decode,
    };

    #[test]
    fn local_response_body_is_unwrapped_from_enum_shell_and_http_wrapper() {
        let response: OpenAiModelListResponse = serde_json::from_value(json!({
            "stats_code": 200,
            "headers": {},
            "body": {
                "object": "list",
                "data": []
            }
        }))
        .expect("valid openai model list response");

        let bytes = serialize_local_response_body(&TransformResponse::ModelListOpenAi(response))
            .expect("serialize local response");
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).expect("decode serialized local response");

        assert!(value.get("ModelListOpenAi").is_none());
        assert!(value.get("stats_code").is_none());
        assert_eq!(value.get("object").and_then(|v| v.as_str()), Some("list"));
        assert!(value.get("data").is_some());
    }

    #[test]
    fn stream_transform_error_chunk_is_ndjson_for_gemini_ndjson() {
        let chunk =
            encode_transform_stream_error_chunk(ProtocolKind::GeminiNDJson, "boom".to_string());
        let text = String::from_utf8(chunk.to_vec()).expect("utf8");
        assert!(text.ends_with('\n'));

        let value: serde_json::Value = serde_json::from_str(text.trim()).expect("json");
        assert_eq!(
            value
                .get("error")
                .and_then(|v| v.get("message"))
                .and_then(|v| v.as_str()),
            Some("boom")
        );
        assert_eq!(
            value
                .get("error")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("transform_serialization_error")
        );
    }

    #[test]
    fn stream_transform_error_chunk_is_sse_for_non_ndjson() {
        let chunk = encode_transform_stream_error_chunk(ProtocolKind::OpenAi, "boom".to_string());
        let text = String::from_utf8(chunk.to_vec()).expect("utf8");
        assert!(text.starts_with("event: error\n"));
        assert!(text.ends_with("\n\n"));

        let data_line = text
            .lines()
            .find(|line| line.starts_with("data: "))
            .expect("data line");
        let payload = data_line.trim_start_matches("data: ");
        let value: serde_json::Value = serde_json::from_str(payload).expect("json");
        assert_eq!(
            value
                .get("error")
                .and_then(|v| v.get("message"))
                .and_then(|v| v.as_str()),
            Some("boom")
        );
    }

    #[test]
    fn wrap_openai_body_into_full_envelope_for_typed_decode() {
        let raw = serde_json::to_vec(&json!({
            "model": "gpt-5",
            "messages": [{"role": "user", "content": "ping"}],
            "stream": false
        }))
        .expect("serialize raw body");

        let wrapped = wrap_payload_for_typed_decode(
            OperationFamily::GenerateContent,
            ProtocolKind::OpenAiChatCompletion,
            raw,
        )
        .expect("wrap payload");
        let value: serde_json::Value = serde_json::from_slice(&wrapped).expect("decode wrapped");

        assert_eq!(value.get("method").and_then(|v| v.as_str()), Some("POST"));
        assert!(value.get("path").is_some());
        assert!(value.get("query").is_some());
        assert!(value.get("headers").is_some());
        assert_eq!(
            value
                .get("body")
                .and_then(|v| v.get("model"))
                .and_then(|v| v.as_str()),
            Some("gpt-5")
        );
    }

    #[test]
    fn wrap_claude_partial_envelope_with_defaults() {
        let raw = serde_json::to_vec(&json!({
            "headers": {"anthropic-version": "2023-06-01"},
            "body": {"model": "claude-sonnet-4", "messages": [], "max_tokens": 16}
        }))
        .expect("serialize raw body");

        let wrapped = wrap_payload_for_typed_decode(
            OperationFamily::GenerateContent,
            ProtocolKind::Claude,
            raw,
        )
        .expect("wrap payload");
        let value: serde_json::Value = serde_json::from_slice(&wrapped).expect("decode wrapped");

        assert_eq!(value.get("method").and_then(|v| v.as_str()), Some("POST"));
        assert!(value.get("path").is_some());
        assert!(value.get("query").is_some());
        assert_eq!(
            value
                .get("headers")
                .and_then(|v| v.get("anthropic-version"))
                .and_then(|v| v.as_str()),
            Some("2023-06-01")
        );
        assert_eq!(
            value
                .get("body")
                .and_then(|v| v.get("model"))
                .and_then(|v| v.as_str()),
            Some("claude-sonnet-4")
        );
    }

    #[test]
    fn wrap_gemini_partial_envelope_with_defaults() {
        let raw = serde_json::to_vec(&json!({
            "path": {"model": "models/gemini-2.5-pro"},
            "query": {"alt": "sse"},
            "body": {"contents": []}
        }))
        .expect("serialize raw body");

        let wrapped = wrap_payload_for_typed_decode(
            OperationFamily::StreamGenerateContent,
            ProtocolKind::Gemini,
            raw,
        )
        .expect("wrap payload");
        let value: serde_json::Value = serde_json::from_slice(&wrapped).expect("decode wrapped");

        assert_eq!(value.get("method").and_then(|v| v.as_str()), Some("POST"));
        assert_eq!(
            value
                .get("path")
                .and_then(|v| v.get("model"))
                .and_then(|v| v.as_str()),
            Some("models/gemini-2.5-pro")
        );
        assert_eq!(
            value
                .get("query")
                .and_then(|v| v.get("alt"))
                .and_then(|v| v.as_str()),
            Some("sse")
        );
        assert!(value.get("headers").is_some());
        assert!(value.get("body").is_some());
    }
}
