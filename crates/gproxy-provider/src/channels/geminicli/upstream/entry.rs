use super::*;

pub async fn execute_geminicli_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = GeminiCliPreparedRequest::from_transform_request(request)?;
    let affinity_body_template = prepared
        .body
        .as_ref()
        .and_then(|body| serde_json::to_vec(body).ok());
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        crate::channels::retry::cache_affinity_protocol_from_transform_request(request).and_then(
            |protocol| {
                cache_affinity_hint_from_transform_request(
                    protocol,
                    prepared.model.as_deref(),
                    affinity_body_template.as_deref(),
                )
            },
        )
    } else {
        None
    };
    execute_geminicli_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_geminicli_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = GeminiCliPreparedRequest::from_payload(operation, protocol, body)?;
    execute_geminicli_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        None,
    )
    .await
}
