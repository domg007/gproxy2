use super::*;

pub(super) async fn handle_upstream_success(
    context: &ExecuteRequestContext,
    dispatch: &PreparedDispatch,
    upstream: UpstreamResponse,
) -> Result<Response, UpstreamError> {
    let upstream_credential_id = upstream.credential_id;
    let upstream_request_meta = upstream.request_meta.clone();

    if let Some(mut local) = upstream.local_response {
        return handle_local_upstream_response(
            context,
            dispatch,
            upstream_credential_id,
            upstream_request_meta,
            &mut local,
        )
        .await;
    }

    if let Some(response) = upstream.response {
        return handle_http_upstream_response(
            context,
            dispatch,
            upstream_credential_id,
            upstream_request_meta,
            response,
        )
        .await;
    }

    enqueue_upstream_and_usage_event(
        context.state.as_ref(),
        UpstreamAndUsageEventInput {
            auth: context.auth,
            request: &dispatch.upstream_request_context,
            provider_id: dispatch.provider_id,
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

async fn handle_local_upstream_response(
    context: &ExecuteRequestContext,
    dispatch: &PreparedDispatch,
    upstream_credential_id: Option<i64>,
    upstream_request_meta: Option<UpstreamRequestMeta>,
    local: &mut gproxy_middleware::TransformResponse,
) -> Result<Response, UpstreamError> {
    let usage_source_response = local.clone();
    if let Some(route) = typed_response_route(dispatch) {
        *local = gproxy_middleware::transform_response(local.clone(), route)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    }

    enqueue_upstream_and_usage_event(
        context.state.as_ref(),
        UpstreamAndUsageEventInput {
            auth: context.auth,
            request: &dispatch.upstream_request_context,
            provider_id: dispatch.provider_id,
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

    let body = serialize_local_response_body(local)?;
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

async fn handle_http_upstream_response(
    context: &ExecuteRequestContext,
    dispatch: &PreparedDispatch,
    upstream_credential_id: Option<i64>,
    upstream_request_meta: Option<UpstreamRequestMeta>,
    response: wreq::Response,
) -> Result<Response, UpstreamError> {
    let response_status = response.status().as_u16();
    let response_headers = response_headers_to_pairs(&response);

    if let Some(route) = typed_response_route(dispatch) {
        return handle_typed_http_upstream_response(
            context,
            dispatch,
            upstream_credential_id,
            upstream_request_meta,
            response,
            response_status,
            response_headers,
            route,
        )
        .await;
    }

    if should_rewrite_gemini_stream_to_ndjson(&dispatch.downstream_request)
        || is_streaming_content_type(response_headers.as_slice())
    {
        let stream_record_context = build_stream_record_context(
            context,
            &dispatch.upstream_request_context,
            dispatch.provider_id,
            upstream_credential_id,
            upstream_request_meta,
            response_status,
            response_headers.clone(),
        );
        return upstream_response_to_axum_stream(
            response,
            should_rewrite_gemini_stream_to_ndjson(&dispatch.downstream_request),
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
        normalize_upstream_response_body_for_channel(&context.channel, body_bytes.as_ref())
            .unwrap_or_else(|| raw_body.clone());
    let encoded_for_usage =
        encode_http_response_for_transform(status, headers.as_slice(), normalized_body.as_ref())?;
    let usage_source_response = decode_response_for_usage(
        dispatch.upstream_request.operation(),
        dispatch.upstream_request.protocol(),
        encoded_for_usage.as_ref(),
    );

    enqueue_upstream_and_usage_event(
        context.state.as_ref(),
        UpstreamAndUsageEventInput {
            auth: context.auth,
            request: &dispatch.upstream_request_context,
            provider_id: dispatch.provider_id,
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
    response_from_status_headers_and_bytes(status, headers_for_client.as_slice(), normalized_body)
}

async fn handle_typed_http_upstream_response(
    context: &ExecuteRequestContext,
    dispatch: &PreparedDispatch,
    upstream_credential_id: Option<i64>,
    upstream_request_meta: Option<UpstreamRequestMeta>,
    response: wreq::Response,
    response_status: u16,
    response_headers: Vec<(String, String)>,
    route: gproxy_middleware::TransformRoute,
) -> Result<Response, UpstreamError> {
    if !response.status().is_success() {
        let stream_record_context = build_stream_record_context(
            context,
            &dispatch.upstream_request_context,
            dispatch.provider_id,
            upstream_credential_id,
            upstream_request_meta,
            response_status,
            response_headers,
        );
        return upstream_response_to_axum_stream(response, false, Some(stream_record_context));
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

    let transformed_payload = if route.dst_operation == OperationFamily::StreamGenerateContent {
        let body_stream = response.bytes_stream().map(|item| {
            item.map_err(|err| MiddlewareTransformError::ProviderPrefix {
                message: err.to_string(),
            })
        });
        let body_stream: std::pin::Pin<
            Box<dyn Stream<Item = Result<Bytes, MiddlewareTransformError>> + Send + 'static>,
        > = if is_wrapped_stream_channel(&context.channel)
            && matches!(
                route.dst_protocol,
                ProtocolKind::Gemini | ProtocolKind::GeminiNDJson
            ) {
            let mut upstream_stream = Box::pin(body_stream);
            let wrapped_channel = context.channel.clone();
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

        let stream_record_context = build_stream_record_context(
            context,
            &dispatch.upstream_request_context,
            dispatch.provider_id,
            upstream_credential_id,
            upstream_request_meta.clone(),
            response_status,
            response_headers.clone(),
        );
        let body_stream = wrap_stream_with_upstream_record(body_stream, stream_record_context);
        gproxy_middleware::transform_response_payload(
            TransformResponsePayload::new(route.dst_operation, route.dst_protocol, body_stream),
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
            normalize_upstream_response_body_for_channel(&context.channel, body_bytes.as_ref())
                .unwrap_or_else(|| raw_body.clone());
        let encoded = encode_http_response_for_transform(
            status,
            headers.as_slice(),
            normalized_body.as_ref(),
        )?;
        let usage_source_response =
            decode_response_for_usage(route.dst_operation, route.dst_protocol, encoded.as_ref());
        enqueue_upstream_and_usage_event(
            context.state.as_ref(),
            UpstreamAndUsageEventInput {
                auth: context.auth,
                request: &dispatch.upstream_request_context,
                provider_id: dispatch.provider_id,
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
        let body_stream = futures_util::stream::once(async move { Ok(Bytes::from(encoded)) });
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

    transformed_payload_to_axum_response(status, headers, transformed_payload, None).await
}

fn typed_response_route(dispatch: &PreparedDispatch) -> Option<gproxy_middleware::TransformRoute> {
    dispatch.dispatch_route.filter(|item| {
        gproxy_middleware::select_response_lane(*item) == gproxy_middleware::TransformLane::Typed
    })
}

fn build_stream_record_context(
    context: &ExecuteRequestContext,
    request: &UsageRequestContext,
    provider_id: Option<i64>,
    credential_id: Option<i64>,
    request_meta: Option<UpstreamRequestMeta>,
    response_status: u16,
    response_headers: Vec<(String, String)>,
) -> UpstreamStreamRecordContext {
    UpstreamStreamRecordContext {
        state: context.state.clone(),
        channel: context.channel.clone(),
        provider: context.provider.clone(),
        auth: context.auth,
        request: request.clone(),
        provider_id,
        credential_id,
        request_meta,
        response_status: Some(response_status),
        response_headers,
        stream_usage: None,
        record_upstream_event: true,
        record_stream_usage_event: true,
    }
}
