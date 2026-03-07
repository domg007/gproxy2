use super::*;

pub async fn execute_antigravity_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    if let Some(local_response) = try_local_antigravity_count_response(request)? {
        return Ok(UpstreamResponse::from_local(local_response));
    }

    let prepared = AntigravityPreparedRequest::from_transform_request(request)?;
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        crate::channels::retry::cache_affinity_protocol_from_transform_request(request).and_then(
            |protocol| {
                cache_affinity_hint_from_transform_request(
                    protocol,
                    prepared.model.as_deref(),
                    prepared
                        .body
                        .as_ref()
                        .and_then(|value| serde_json::to_vec(value).ok())
                        .as_deref(),
                )
            },
        )
    } else {
        None
    };
    execute_antigravity_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_antigravity_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    if (operation, protocol) == (OperationFamily::CountToken, ProtocolKind::Gemini) {
        let payload_value = serde_json::from_slice::<Value>(body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let payload = payload_value.get("body").cloned().unwrap_or(payload_value);
        let text = collect_count_text(&payload);
        let total_tokens = (text.chars().count() as u64).div_ceil(4);
        let response_json = json!({
            "stats_code": 200,
            "headers": {},
            "body": {
                "totalTokens": total_tokens,
            }
        });
        let response = serde_json::from_value(response_json)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        return Ok(UpstreamResponse::from_local(
            TransformResponse::CountTokenGemini(response),
        ));
    }

    let prepared = AntigravityPreparedRequest::from_payload(operation, protocol, body)?;
    execute_antigravity_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        None,
    )
    .await
}
