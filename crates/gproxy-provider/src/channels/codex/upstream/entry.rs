use super::*;

pub async fn execute_codex_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
    token_resolution: TokenizerResolutionContext<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    if let Some(local_response) =
        try_local_codex_count_token_response(request, client, token_resolution).await?
    {
        return Ok(UpstreamResponse::from_local(local_response));
    }

    let prepared = CodexPreparedRequest::from_transform_request(request)?;
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        cache_affinity_hint_from_codex_transform_request(
            request,
            prepared.model.as_deref(),
            prepared.body.as_deref(),
        )
        .or_else(|| {
            cache_affinity_hint_from_codex_openai_response_body(
                prepared.model.as_deref(),
                prepared.body.as_deref(),
            )
        })
    } else {
        None
    };
    execute_codex_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_codex_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    payload: RetryWithPayloadRequest<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    if (payload.operation, payload.protocol) == (OperationFamily::CountToken, ProtocolKind::OpenAi)
    {
        let payload_json = serde_json::from_slice::<Value>(payload.body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let body_json = payload_body_value(&payload_json);
        let model = body_json
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let input_tokens = count_openai_input_tokens_with_resolution(
            payload.token_resolution.tokenizer_store,
            client,
            payload.token_resolution.hf_token,
            payload.token_resolution.hf_url,
            model.as_deref(),
            &body_json,
        )
        .await?;
        let response_json = json!({
            "stats_code": 200,
            "headers": {},
            "body": {
                "input_tokens": input_tokens,
                "object": "response.input_tokens",
            }
        });
        let response = serde_json::from_value(response_json)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        return Ok(UpstreamResponse::from_local(
            TransformResponse::CountTokenOpenAi(response),
        ));
    }

    let prepared =
        CodexPreparedRequest::from_payload(payload.operation, payload.protocol, payload.body)?;
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        cache_affinity_hint_from_codex_openai_response_body(
            prepared.model.as_deref(),
            prepared.body.as_deref(),
        )
    } else {
        None
    };
    execute_codex_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        payload.now_unix_ms,
        cache_affinity_hint,
    )
    .await
}
