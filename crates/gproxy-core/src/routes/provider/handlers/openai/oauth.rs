use super::*;

pub(in crate::routes::provider) async fn oauth_start(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = if matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode)) {
        state.load_spoof_http()
    } else {
        state.load_http()
    };
    let request = UpstreamOAuthRequest {
        query,
        headers: collect_headers(&headers),
    };
    let (response_result, tracked_http_events) = capture_tracked_http_events(async {
        provider.execute_oauth_start(http.as_ref(), &request).await
    })
    .await;
    let response = match response_result {
        Ok(response) => response,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                err_request_meta.as_ref(),
                UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        None,
        response.request_meta.as_ref(),
        UpstreamResponseMeta {
            status: Some(response.status_code),
            headers: response.headers.as_slice(),
            body: Some(response.body.clone()),
        },
    )
    .await;
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        None,
        tracked_http_events.as_slice(),
        response.request_meta.as_ref(),
    )
    .await;
    Ok(oauth_response_to_axum(response))
}

pub(in crate::routes::provider) async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = if matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode)) {
        state.load_spoof_http()
    } else {
        state.load_http()
    };
    let request = UpstreamOAuthRequest {
        query,
        headers: collect_headers(&headers),
    };
    let (callback_result, tracked_http_events) = capture_tracked_http_events(async {
        provider
            .execute_oauth_callback(http.as_ref(), &request)
            .await
    })
    .await;
    let result = match callback_result {
        Ok(result) => result,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                None,
                err_request_meta.as_ref(),
                UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    let mut resolved_credential_id: Option<i64> = None;

    if let Some(oauth_credential) = result.credential.as_ref() {
        let provisional = CredentialRef {
            id: -1,
            label: oauth_credential.label.clone(),
            credential: oauth_credential.credential.clone(),
        };
        let provider_id = resolve_provider_id(&state, &channel).await?;
        let credential_id = if let Some(credential_id) =
            parse_optional_query_value::<i64>(request.query.as_deref(), "credential_id")?
        {
            credential_id
        } else {
            resolve_credential_id(&state, provider_id, &provisional).await?
        };
        resolved_credential_id = Some(credential_id);
        let credential_ref = CredentialRef {
            id: credential_id,
            label: oauth_credential.label.clone(),
            credential: oauth_credential.credential.clone(),
        };
        state.upsert_provider_credential_in_memory(&channel, credential_ref.clone());
        persist_provider_and_credential(&state, &channel, &provider, &credential_ref).await?;
    }
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        resolved_credential_id,
        result.response.request_meta.as_ref(),
        UpstreamResponseMeta {
            status: Some(result.response.status_code),
            headers: result.response.headers.as_slice(),
            body: Some(result.response.body.clone()),
        },
    )
    .await;
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        resolved_credential_id,
        tracked_http_events.as_slice(),
        result.response.request_meta.as_ref(),
    )
    .await;

    Ok(oauth_callback_response_to_axum(
        result,
        resolved_credential_id,
    ))
}

pub(in crate::routes::provider) async fn upstream_usage(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let provider_id = resolve_provider_id(&state, &channel).await.ok();
    let http = state.load_http();
    let spoof_http = matches!(&channel, ChannelId::Builtin(BuiltinChannel::ClaudeCode))
        .then(|| state.load_spoof_http());
    let now = now_unix_ms();
    let credential_id = parse_optional_query_value::<i64>(query.as_deref(), "credential_id")?;
    let (upstream_result, tracked_http_events) = capture_tracked_http_events(async {
        provider
            .execute_upstream_usage_with_retry_with_spoof(
                http.as_ref(),
                spoof_http.as_deref(),
                state.credential_states(),
                credential_id,
                now,
            )
            .await
    })
    .await;
    let upstream = match upstream_result {
        Ok(upstream) => upstream,
        Err(err) => {
            let err_request_meta = upstream_error_request_meta(&err);
            enqueue_internal_tracked_http_events(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                credential_id,
                tracked_http_events.as_slice(),
                err_request_meta.as_ref(),
            )
            .await;
            let err_status = upstream_error_status(&err);
            enqueue_upstream_request_event_from_meta(
                state.as_ref(),
                auth.downstream_trace_id,
                provider_id,
                credential_id,
                err_request_meta.as_ref(),
                UpstreamResponseMeta {
                    status: err_status,
                    headers: &[],
                    body: None,
                },
            )
            .await;
            return Err(HttpError::from(err));
        }
    };
    let upstream_credential_id = upstream.credential_id;
    let upstream_request_meta = upstream.request_meta.clone();

    if let Some(update) = upstream.credential_update.clone() {
        apply_credential_update_and_persist(
            state.clone(),
            channel.clone(),
            provider.clone(),
            update,
        )
        .await;
    }

    let payload = upstream
        .into_http_payload()
        .await
        .map_err(HttpError::from)?;
    enqueue_upstream_request_event_from_meta(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        upstream_credential_id,
        upstream_request_meta.as_ref(),
        UpstreamResponseMeta {
            status: Some(payload.status_code),
            headers: payload.headers.as_slice(),
            body: Some(payload.body.clone()),
        },
    )
    .await;
    enqueue_internal_tracked_http_events(
        state.as_ref(),
        auth.downstream_trace_id,
        provider_id,
        upstream_credential_id.or(credential_id),
        tracked_http_events.as_slice(),
        upstream_request_meta.as_ref(),
    )
    .await;
    Ok(oauth_response_to_axum(payload))
}
