use super::*;

pub(in crate::routes::provider) async fn execute_transform_request_payload(
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

pub(in crate::routes::provider) async fn execute_transform_candidates(
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
