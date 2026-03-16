use super::*;

pub async fn execute_claudecode_with_retry(
    client: &WreqClient,
    spoof_client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &gproxy_middleware::TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prelude_text = provider
        .settings
        .claudecode_prelude_text()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let prepared = ClaudeCodePreparedRequest::from_transform_request(
        request,
        provider.settings.claudecode_append_beta_query(),
        prelude_text,
        provider.settings.claudecode_enable_billing_header(),
        provider.settings.cache_breakpoints(),
    )?;
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        crate::channels::retry::cache_affinity_protocol_from_transform_request(request).and_then(
            |protocol| {
                cache_affinity_hint_from_transform_request(
                    protocol,
                    prepared.model.as_deref(),
                    prepared.body.as_deref(),
                )
            },
        )
    } else {
        None
    };
    execute_claudecode_with_prepared(
        client,
        spoof_client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_claudecode_payload_with_retry(
    client: &WreqClient,
    spoof_client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    payload: RetryWithPayloadRequest<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    let prelude_text = provider
        .settings
        .claudecode_prelude_text()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let prepared = ClaudeCodePreparedRequest::from_payload(
        payload.operation,
        payload.protocol,
        payload.body,
        provider.settings.claudecode_append_beta_query(),
        prelude_text,
        provider.settings.claudecode_enable_billing_header(),
        provider.settings.cache_breakpoints(),
    )?;
    execute_claudecode_with_prepared(
        client,
        spoof_client,
        provider,
        credential_states,
        prepared,
        payload.now_unix_ms,
        None,
    )
    .await
}
