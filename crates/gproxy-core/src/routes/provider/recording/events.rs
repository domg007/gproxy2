use super::*;

pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn now_unix_ms_i64() -> i64 {
    i64::try_from(now_unix_ms()).unwrap_or(i64::MAX)
}

pub(crate) fn headers_pairs_to_json(headers: &[(String, String)]) -> String {
    let mut map: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (name, value) in headers {
        map.entry(name.clone()).or_default().push(value.clone());
    }
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn response_headers_to_pairs(response: &wreq::Response) -> Vec<(String, String)> {
    response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

pub(crate) fn should_record_usage(operation: OperationFamily) -> bool {
    !matches!(
        operation,
        OperationFamily::ModelList | OperationFamily::ModelGet
    )
}

pub(crate) fn upstream_error_request_meta(error: &UpstreamError) -> Option<UpstreamRequestMeta> {
    match error {
        UpstreamError::AllCredentialsExhausted {
            last_request_meta, ..
        } => last_request_meta.as_deref().cloned(),
        _ => None,
    }
}

pub(crate) fn upstream_error_credential_id(error: &UpstreamError) -> Option<i64> {
    match error {
        UpstreamError::AllCredentialsExhausted {
            last_credential_id, ..
        } => *last_credential_id,
        _ => None,
    }
}

pub(crate) fn upstream_error_status(error: &UpstreamError) -> Option<u16> {
    match error {
        UpstreamError::AllCredentialsExhausted { last_status, .. } => *last_status,
        _ => None,
    }
}

pub(crate) async fn enqueue_credential_status_updates_for_request(
    state: &AppState,
    channel: &ChannelId,
    provider: &ProviderDefinition,
    request_now_unix_ms: u64,
) {
    for credential in provider.credentials.list_credentials() {
        let Some(state_row) = state.credential_states().get(channel, credential.id) else {
            continue;
        };
        if state_row.checked_at_unix_ms != Some(request_now_unix_ms) {
            continue;
        }

        let (health_kind, health_json) = credential_health_to_storage(&state_row.health);
        let checked_at_unix_ms = state_row
            .checked_at_unix_ms
            .and_then(|value| i64::try_from(value).ok());
        let event = StorageWriteEvent::UpsertCredentialStatus(CredentialStatusWrite {
            id: None,
            credential_id: credential.id,
            channel: channel.as_str().to_string(),
            health_kind,
            health_json,
            checked_at_unix_ms,
            last_error: state_row.last_error.clone(),
        });
        if let Err(err) = state.enqueue_storage_write(event).await {
            eprintln!(
                "provider: credential status enqueue failed channel={} credential_id={} error={}",
                channel.as_str(),
                credential.id,
                err
            );
        }
    }
}

pub(crate) struct UpstreamAndUsageEventInput<'a> {
    pub(crate) auth: RequestAuthContext,
    pub(crate) request: &'a UsageRequestContext,
    pub(crate) provider_id: Option<i64>,
    pub(crate) credential_id: Option<i64>,
    pub(crate) request_meta: Option<&'a UpstreamRequestMeta>,
    pub(crate) error_status: Option<u16>,
    pub(crate) response_status: Option<u16>,
    pub(crate) response_headers: &'a [(String, String)],
    pub(crate) response_body: Option<Vec<u8>>,
    pub(crate) local_response: Option<&'a gproxy_middleware::TransformResponse>,
}

pub(crate) async fn enqueue_upstream_and_usage_event(
    state: &AppState,
    input: UpstreamAndUsageEventInput<'_>,
) {
    let UpstreamAndUsageEventInput {
        auth,
        request,
        provider_id,
        credential_id,
        request_meta,
        error_status,
        response_status,
        response_headers,
        response_body,
        local_response,
    } = input;
    let operation = format!("{:?}", request.operation());
    let protocol = format!("{:?}", request.protocol());
    let request_model = normalize_usage_model(request.model.clone());
    let now_unix_ms = now_unix_ms_i64();
    let extracted_usage = local_response.and_then(extract_usage_from_local_response);
    let mask_sensitive_info = state.load_config().global.mask_sensitive_info;
    let persisted_request_body = if mask_sensitive_info {
        None
    } else {
        request_meta.and_then(|meta| meta.body.clone())
    };
    let persisted_response_body = if mask_sensitive_info {
        None
    } else {
        response_body.or_else(|| local_response.and_then(|value| serde_json::to_vec(value).ok()))
    };
    if let Some(meta) = request_meta {
        let upstream_event = UpstreamRequestWrite {
            downstream_trace_id: auth.downstream_trace_id,
            at_unix_ms: now_unix_ms,
            internal: false,
            provider_id,
            credential_id,
            request_method: meta.method.clone(),
            request_headers_json: headers_pairs_to_json(meta.headers.as_slice()),
            request_url: Some(meta.url.clone()),
            request_body: persisted_request_body,
            response_status: response_status.or(error_status).map(i32::from),
            response_headers_json: headers_pairs_to_json(response_headers),
            response_body: persisted_response_body,
        };
        if let Err(err) = state
            .enqueue_storage_write(StorageWriteEvent::UpsertUpstreamRequest(upstream_event))
            .await
        {
            eprintln!("provider: upstream event enqueue failed: {err}");
        }
    }

    if !should_record_usage(request.operation())
        || response_status.map(|value| value >= 400).unwrap_or(true)
    {
        return;
    }
    if request.operation() == OperationFamily::StreamGenerateContent {
        return;
    }

    let mut input_tokens = extracted_usage.and_then(|value| value.input_tokens);
    let mut output_tokens = extracted_usage.and_then(|value| value.output_tokens);
    let cache_read_input_tokens = extracted_usage.and_then(|value| value.cache_read_input_tokens);
    let cache_creation_input_tokens =
        extracted_usage.and_then(|value| value.cache_creation_input_tokens);
    let cache_creation_input_tokens_5min =
        extracted_usage.and_then(|value| value.cache_creation_input_tokens_5min);
    let cache_creation_input_tokens_1h =
        extracted_usage.and_then(|value| value.cache_creation_input_tokens_1h);

    if request.operation() == OperationFamily::Embedding && input_tokens.is_none() {
        input_tokens = estimate_embedding_input_tokens_from_usage_request(state, request).await;
        output_tokens = output_tokens.or(Some(0));
    }
    if request.operation() == OperationFamily::CountToken && input_tokens.is_some() {
        output_tokens = Some(0);
    }

    let usage_event = UsageWrite {
        downstream_trace_id: auth.downstream_trace_id,
        at_unix_ms: now_unix_ms,
        provider_id,
        credential_id,
        user_id: Some(auth.user_id),
        user_key_id: Some(auth.user_key_id),
        operation,
        protocol,
        model: request_model,
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        cache_creation_input_tokens_5min,
        cache_creation_input_tokens_1h,
    };
    if let Err(err) = state
        .enqueue_storage_write(StorageWriteEvent::UpsertUsage(usage_event))
        .await
    {
        eprintln!("provider: usage event enqueue failed: {err}");
    }
}

pub(crate) struct UpstreamResponseMeta<'a> {
    pub status: Option<u16>,
    pub headers: &'a [(String, String)],
    pub body: Option<Vec<u8>>,
}

pub(crate) async fn enqueue_upstream_request_event_from_meta(
    state: &AppState,
    downstream_trace_id: Option<i64>,
    provider_id: Option<i64>,
    credential_id: Option<i64>,
    request_meta: Option<&UpstreamRequestMeta>,
    response_meta: UpstreamResponseMeta<'_>,
) {
    let Some(meta) = request_meta else {
        return;
    };
    let mask_sensitive_info = state.load_config().global.mask_sensitive_info;
    let upstream_event = UpstreamRequestWrite {
        downstream_trace_id,
        at_unix_ms: now_unix_ms_i64(),
        internal: false,
        provider_id,
        credential_id,
        request_method: meta.method.clone(),
        request_headers_json: headers_pairs_to_json(meta.headers.as_slice()),
        request_url: Some(meta.url.clone()),
        request_body: if mask_sensitive_info {
            None
        } else {
            meta.body.clone()
        },
        response_status: response_meta.status.map(i32::from),
        response_headers_json: headers_pairs_to_json(response_meta.headers),
        response_body: if mask_sensitive_info {
            None
        } else {
            response_meta.body
        },
    };
    if let Err(err) = state
        .enqueue_storage_write(StorageWriteEvent::UpsertUpstreamRequest(upstream_event))
        .await
    {
        eprintln!("provider: upstream event enqueue failed: {err}");
    }
}

pub(crate) async fn enqueue_internal_tracked_http_events(
    state: &AppState,
    downstream_trace_id: Option<i64>,
    provider_id: Option<i64>,
    credential_id: Option<i64>,
    events: &[TrackedHttpEvent],
    primary_request_meta: Option<&UpstreamRequestMeta>,
) {
    if events.is_empty() {
        return;
    }
    let mask_sensitive_info = state.load_config().global.mask_sensitive_info;
    for event in events {
        if let Some(primary_meta) = primary_request_meta
            && tracked_http_event_matches_primary_request(event, primary_meta)
        {
            continue;
        }
        let upstream_event = UpstreamRequestWrite {
            downstream_trace_id,
            at_unix_ms: now_unix_ms_i64(),
            internal: true,
            provider_id,
            credential_id,
            request_method: event.request_meta.method.clone(),
            request_headers_json: headers_pairs_to_json(event.request_meta.headers.as_slice()),
            request_url: Some(event.request_meta.url.clone()),
            request_body: if mask_sensitive_info {
                None
            } else {
                event.request_meta.body.clone()
            },
            response_status: event.response_status.map(i32::from),
            response_headers_json: headers_pairs_to_json(event.response_headers.as_slice()),
            response_body: if mask_sensitive_info {
                None
            } else {
                event.response_body.clone()
            },
        };
        if let Err(err) = state
            .enqueue_storage_write(StorageWriteEvent::UpsertUpstreamRequest(upstream_event))
            .await
        {
            eprintln!("provider: tracked http event enqueue failed: {err}");
        }
    }
}

pub(crate) fn tracked_http_event_matches_primary_request(
    event: &TrackedHttpEvent,
    primary_meta: &UpstreamRequestMeta,
) -> bool {
    event.request_meta.method == primary_meta.method
        && event.request_meta.url == primary_meta.url
        && event.request_meta.headers == primary_meta.headers
        && event.request_meta.body == primary_meta.body
}
