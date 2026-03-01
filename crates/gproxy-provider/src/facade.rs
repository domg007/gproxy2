use gproxy_middleware::{TransformRequest, TransformResponse};
use wreq::Client as WreqClient;

use crate::channel::{BuiltinChannel, ChannelId};
use crate::channels::antigravity::{
    ensure_antigravity_project_id, normalize_antigravity_upstream_response_body,
    normalize_antigravity_upstream_stream_ndjson_chunk,
};
use crate::channels::geminicli::{
    ensure_geminicli_project_id, normalize_geminicli_upstream_response_body,
    normalize_geminicli_upstream_stream_ndjson_chunk,
};
use crate::channels::upstream::{
    UpstreamError, UpstreamOAuthCallbackResult, UpstreamOAuthRequest, UpstreamOAuthResponse,
    UpstreamResponse,
};
use crate::channels::vertex::normalize_vertex_upstream_response_body;
use crate::channels::vertexexpress::try_local_vertexexpress_model_response;
use crate::channels::{BuiltinChannelCredential, ChannelCredential, ChannelSettings};
use crate::credential::ChannelCredentialStateStore;
use crate::provider::{ProviderDefinition, TokenizerResolutionContext};

#[derive(Clone, Copy)]
pub struct ExecuteInput<'a> {
    pub provider: &'a ProviderDefinition,
    pub client: &'a WreqClient,
    pub credential_states: &'a ChannelCredentialStateStore,
    pub request: &'a TransformRequest,
    pub now_unix_ms: u64,
    pub token_resolution: TokenizerResolutionContext<'a>,
}

#[derive(Clone, Copy)]
pub struct OAuthInput<'a> {
    pub provider: &'a ProviderDefinition,
    pub client: &'a WreqClient,
    pub request: &'a UpstreamOAuthRequest,
}

#[derive(Clone, Copy)]
pub struct UpstreamUsageInput<'a> {
    pub provider: &'a ProviderDefinition,
    pub client: &'a WreqClient,
    pub credential_states: &'a ChannelCredentialStateStore,
    pub credential_id: Option<i64>,
    pub now_unix_ms: u64,
}

#[allow(async_fn_in_trait)]
pub trait ProviderRuntime {
    async fn execute_with_retry(
        &self,
        input: ExecuteInput<'_>,
    ) -> Result<UpstreamResponse, UpstreamError>;

    async fn execute_oauth_start(
        &self,
        input: OAuthInput<'_>,
    ) -> Result<UpstreamOAuthResponse, UpstreamError>;

    async fn execute_oauth_callback(
        &self,
        input: OAuthInput<'_>,
    ) -> Result<UpstreamOAuthCallbackResult, UpstreamError>;

    async fn execute_upstream_usage_with_retry(
        &self,
        input: UpstreamUsageInput<'_>,
    ) -> Result<UpstreamResponse, UpstreamError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultProviderRuntime;

impl ProviderRuntime for DefaultProviderRuntime {
    async fn execute_with_retry(
        &self,
        input: ExecuteInput<'_>,
    ) -> Result<UpstreamResponse, UpstreamError> {
        input
            .provider
            .execute_with_retry(
                input.client,
                input.credential_states,
                input.request,
                input.now_unix_ms,
                input.token_resolution,
            )
            .await
    }

    async fn execute_oauth_start(
        &self,
        input: OAuthInput<'_>,
    ) -> Result<UpstreamOAuthResponse, UpstreamError> {
        input
            .provider
            .execute_oauth_start(input.client, input.request)
            .await
    }

    async fn execute_oauth_callback(
        &self,
        input: OAuthInput<'_>,
    ) -> Result<UpstreamOAuthCallbackResult, UpstreamError> {
        input
            .provider
            .execute_oauth_callback(input.client, input.request)
            .await
    }

    async fn execute_upstream_usage_with_retry(
        &self,
        input: UpstreamUsageInput<'_>,
    ) -> Result<UpstreamResponse, UpstreamError> {
        input
            .provider
            .execute_upstream_usage_with_retry(
                input.client,
                input.credential_states,
                input.credential_id,
                input.now_unix_ms,
            )
            .await
    }
}

pub async fn ensure_project_id_for_credential(
    client: &WreqClient,
    channel: &ChannelId,
    settings: &ChannelSettings,
    credential: &mut ChannelCredential,
) -> Result<(), UpstreamError> {
    match (channel, credential) {
        (
            ChannelId::Builtin(BuiltinChannel::GeminiCli),
            ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(value)),
        ) => ensure_geminicli_project_id(client, settings, value).await,
        (
            ChannelId::Builtin(BuiltinChannel::Antigravity),
            ChannelCredential::Builtin(BuiltinChannelCredential::Antigravity(value)),
        ) => ensure_antigravity_project_id(client, settings, value).await,
        _ => Ok(()),
    }
}

pub fn normalize_upstream_response_body_for_channel(
    channel: &ChannelId,
    body: &[u8],
) -> Option<Vec<u8>> {
    match channel {
        ChannelId::Builtin(BuiltinChannel::GeminiCli) => {
            normalize_geminicli_upstream_response_body(body)
        }
        ChannelId::Builtin(BuiltinChannel::Antigravity) => {
            normalize_antigravity_upstream_response_body(body)
        }
        ChannelId::Builtin(BuiltinChannel::Vertex) => normalize_vertex_upstream_response_body(body),
        _ => None,
    }
}

pub fn normalize_upstream_stream_ndjson_chunk_for_channel(
    channel: &ChannelId,
    chunk: &[u8],
) -> Option<Vec<u8>> {
    match channel {
        ChannelId::Builtin(BuiltinChannel::GeminiCli) => {
            normalize_geminicli_upstream_stream_ndjson_chunk(chunk)
        }
        ChannelId::Builtin(BuiltinChannel::Antigravity) => {
            normalize_antigravity_upstream_stream_ndjson_chunk(chunk)
        }
        _ => None,
    }
}

pub fn try_local_response_for_channel(
    channel: &ChannelId,
    request: &TransformRequest,
) -> Result<Option<TransformResponse>, UpstreamError> {
    match channel {
        ChannelId::Builtin(BuiltinChannel::VertexExpress) => {
            try_local_vertexexpress_model_response(request)
        }
        _ => Ok(None),
    }
}
