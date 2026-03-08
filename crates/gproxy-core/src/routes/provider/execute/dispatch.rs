use super::*;

#[derive(Clone)]
pub(super) struct ExecuteRequestContext {
    pub(super) state: Arc<AppState>,
    pub(super) channel: ChannelId,
    pub(super) provider: ProviderDefinition,
    pub(super) auth: RequestAuthContext,
}

pub(super) struct PreparedDispatch {
    pub(super) downstream_request: TransformRequest,
    pub(super) downstream_request_context: UsageRequestContext,
    pub(super) upstream_request: TransformRequest,
    pub(super) upstream_request_context: UsageRequestContext,
    pub(super) dispatch_route: Option<gproxy_middleware::TransformRoute>,
    pub(super) dispatch_local: bool,
    pub(super) provider_id: Option<i64>,
    pub(super) now_unix_ms: u64,
}

pub(super) struct ExecutedUpstream {
    pub(super) result: Result<UpstreamResponse, UpstreamError>,
    pub(super) tracked_http_events: Vec<TrackedHttpEvent>,
}

pub(super) async fn prepare_execute_dispatch(
    context: &ExecuteRequestContext,
    request: TransformRequest,
) -> Result<PreparedDispatch, UpstreamError> {
    let downstream_request = request;
    let mut upstream_request = downstream_request.clone();
    let downstream_request_context =
        usage_request_context_from_transform_request(&downstream_request);
    let provider_id = resolve_provider_id(context.state.as_ref(), &context.channel)
        .await
        .ok();
    let src_route = RouteKey::new(
        downstream_request.operation(),
        downstream_request.protocol(),
    );
    let Some(implementation) = context.provider.dispatch.resolve(src_route).cloned() else {
        enqueue_empty_upstream_and_usage_event(context, provider_id, &downstream_request_context)
            .await;
        return Err(UpstreamError::UnsupportedRequest);
    };

    let mut dispatch_route = None;
    let mut dispatch_local = false;
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
                        enqueue_empty_upstream_and_usage_event(
                            context,
                            provider_id,
                            &downstream_request_context,
                        )
                        .await;
                        return Err(UpstreamError::SerializeRequest(err.to_string()));
                    }
                }
            }
            dispatch_route = Some(route);
        }
        RouteImplementation::Local => {
            dispatch_local = true;
        }
        RouteImplementation::Unsupported => {
            enqueue_empty_upstream_and_usage_event(
                context,
                provider_id,
                &downstream_request_context,
            )
            .await;
            return Err(UpstreamError::UnsupportedRequest);
        }
    }

    let now_unix_ms = now_unix_ms();
    ensure_stream_usage_option_on_native_chat(&mut upstream_request);
    let upstream_request_context = usage_request_context_from_transform_request(&upstream_request);

    Ok(PreparedDispatch {
        downstream_request,
        downstream_request_context,
        upstream_request,
        upstream_request_context,
        dispatch_route,
        dispatch_local,
        provider_id,
        now_unix_ms,
    })
}

pub(super) async fn execute_upstream_dispatch(
    context: &ExecuteRequestContext,
    dispatch: &PreparedDispatch,
) -> ExecutedUpstream {
    let (result, tracked_http_events) = if dispatch.dispatch_local {
        (
            execute_local_request(
                context.state.as_ref(),
                &context.provider,
                &dispatch.downstream_request,
            )
            .await,
            Vec::new(),
        )
    } else {
        let http = context.state.load_http();
        let spoof_http = matches!(
            &context.channel,
            ChannelId::Builtin(BuiltinChannel::ClaudeCode)
        )
        .then(|| context.state.load_spoof_http());
        let tokenizers = context.state.tokenizers();
        let global = context.state.load_config().global.clone();

        if let Some(forced_credential_id) = context.auth.forced_credential_id {
            let Some(credential) = context
                .provider
                .credentials
                .credential(forced_credential_id)
                .cloned()
            else {
                return ExecutedUpstream {
                    result: Err(UpstreamError::NoEligibleCredential {
                        channel: context.channel.as_str().to_string(),
                        model: dispatch.upstream_request_context.model.clone(),
                    }),
                    tracked_http_events: Vec::new(),
                };
            };
            let mut provider = context.provider.clone();
            provider.credentials.credentials = vec![credential];
            provider.credentials.channel_states.clear();
            let credential_states = gproxy_provider::ChannelCredentialStateStore::new();

            capture_tracked_http_events(async {
                provider
                    .execute_with_retry_with_spoof(
                        http.as_ref(),
                        spoof_http.as_deref(),
                        &credential_states,
                        &dispatch.upstream_request,
                        dispatch.now_unix_ms,
                        TokenizerResolutionContext {
                            tokenizer_store: tokenizers.as_ref(),
                            hf_token: global.hf_token.as_deref(),
                            hf_url: global.hf_url.as_deref(),
                        },
                    )
                    .await
            })
            .await
        } else {
            capture_tracked_http_events(async {
                context
                    .provider
                    .execute_with_retry_with_spoof(
                        http.as_ref(),
                        spoof_http.as_deref(),
                        context.state.credential_states(),
                        &dispatch.upstream_request,
                        dispatch.now_unix_ms,
                        TokenizerResolutionContext {
                            tokenizer_store: tokenizers.as_ref(),
                            hf_token: global.hf_token.as_deref(),
                            hf_url: global.hf_url.as_deref(),
                        },
                    )
                    .await
            })
            .await
        }
    };

    if !dispatch.dispatch_local {
        enqueue_credential_status_updates_for_request(
            context.state.as_ref(),
            &context.channel,
            &context.provider,
            dispatch.now_unix_ms,
        )
        .await;
    }

    ExecutedUpstream {
        result,
        tracked_http_events,
    }
}

pub(super) async fn flush_tracked_http_events(
    context: &ExecuteRequestContext,
    dispatch: &PreparedDispatch,
    credential_id: Option<i64>,
    tracked_http_events: &[TrackedHttpEvent],
    request_meta: Option<&UpstreamRequestMeta>,
) {
    if dispatch.dispatch_local {
        return;
    }

    enqueue_internal_tracked_http_events(
        context.state.as_ref(),
        context.auth.downstream_trace_id,
        dispatch.provider_id,
        credential_id,
        tracked_http_events,
        request_meta,
    )
    .await;
}

pub(super) async fn record_execute_failure(
    context: &ExecuteRequestContext,
    dispatch: &PreparedDispatch,
    tracked_http_events: &[TrackedHttpEvent],
    err: &UpstreamError,
) {
    let err_request_meta = upstream_error_request_meta(err);
    let err_credential_id = upstream_error_credential_id(err);
    let err_status = upstream_error_status(err);

    flush_tracked_http_events(
        context,
        dispatch,
        err_credential_id,
        tracked_http_events,
        err_request_meta.as_ref(),
    )
    .await;

    enqueue_upstream_and_usage_event(
        context.state.as_ref(),
        UpstreamAndUsageEventInput {
            auth: context.auth,
            request: &dispatch.downstream_request_context,
            provider_id: dispatch.provider_id,
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
}

async fn enqueue_empty_upstream_and_usage_event(
    context: &ExecuteRequestContext,
    provider_id: Option<i64>,
    request: &UsageRequestContext,
) {
    enqueue_upstream_and_usage_event(
        context.state.as_ref(),
        UpstreamAndUsageEventInput {
            auth: context.auth,
            request,
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
}
