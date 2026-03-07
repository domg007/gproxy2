use super::*;

pub async fn execute_vertex_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let cache_protocol = cache_affinity_protocol_from_transform_request(request);
    let prepared = VertexPreparedRequest::from_transform_request(request)?;
    execute_vertex_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        cache_protocol,
        now_unix_ms,
    )
    .await
}

pub async fn execute_vertex_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = VertexPreparedRequest::from_payload(operation, protocol, body)?;
    let cache_protocol = cache_affinity_protocol_from_operation_protocol(operation, protocol);
    execute_vertex_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        cache_protocol,
        now_unix_ms,
    )
    .await
}
