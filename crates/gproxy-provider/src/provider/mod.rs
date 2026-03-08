use crate::UpstreamCredentialUpdate;
use std::future::Future;
use std::pin::Pin;

use crate::channel::{BuiltinChannel, ChannelId};
use crate::channels::ChannelSettings;
use crate::channels::aistudio::{execute_aistudio_payload_with_retry, execute_aistudio_with_retry};
use crate::channels::anthropic::{
    execute_anthropic_payload_with_retry, execute_anthropic_with_retry,
};
use crate::channels::antigravity::{
    execute_antigravity_oauth_callback, execute_antigravity_oauth_start,
    execute_antigravity_payload_with_retry, execute_antigravity_upstream_usage_with_retry,
    execute_antigravity_with_retry,
};
use crate::channels::claudecode::credential::ClaudeCodeTokenRefresh;
use crate::channels::claudecode::{
    execute_claudecode_oauth_callback, execute_claudecode_oauth_start,
    execute_claudecode_payload_with_retry, execute_claudecode_upstream_usage_with_retry,
    execute_claudecode_with_retry,
};
use crate::channels::codex::{
    execute_codex_oauth_callback, execute_codex_oauth_start, execute_codex_payload_with_retry,
    execute_codex_upstream_usage_with_retry, execute_codex_with_retry,
};
use crate::channels::custom::execute_custom_with_retry;
use crate::channels::deepseek::{execute_deepseek_payload_with_retry, execute_deepseek_with_retry};
use crate::channels::geminicli::{
    execute_geminicli_oauth_callback, execute_geminicli_oauth_start,
    execute_geminicli_payload_with_retry, execute_geminicli_upstream_usage_with_retry,
    execute_geminicli_with_retry,
};
use crate::channels::grok::{execute_grok_payload_with_retry, execute_grok_with_retry};
use crate::channels::groq::{execute_groq_payload_with_retry, execute_groq_with_retry};
use crate::channels::nvidia::{execute_nvidia_payload_with_retry, execute_nvidia_with_retry};
use crate::channels::openai::{execute_openai_payload_with_retry, execute_openai_with_retry};
use crate::channels::retry::CredentialPickMode;
use crate::channels::upstream::{
    UpstreamError, UpstreamOAuthCallbackResult, UpstreamOAuthRequest, UpstreamOAuthResponse,
    UpstreamResponse,
};
use crate::channels::vertex::{execute_vertex_payload_with_retry, execute_vertex_with_retry};
use crate::channels::vertexexpress::{
    execute_vertexexpress_payload_with_retry, execute_vertexexpress_with_retry,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::{ChannelCredentialStateStore, CredentialRef, ProviderCredentialState};
use crate::dispatch::ProviderDispatchTable;
use crate::tokenizers::LocalTokenizerStore;
use gproxy_middleware::{OperationFamily, ProtocolKind};
use wreq::Client as WreqClient;

#[derive(Debug, Clone, Copy)]
pub struct TokenizerResolutionContext<'a> {
    pub tokenizer_store: &'a LocalTokenizerStore,
    pub hf_token: Option<&'a str>,
    pub hf_url: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct RetryWithPayloadRequest<'a> {
    pub operation: OperationFamily,
    pub protocol: ProtocolKind,
    pub body: &'a [u8],
    pub now_unix_ms: u64,
    pub token_resolution: TokenizerResolutionContext<'a>,
}

type ProviderResponseFuture<'a> =
    Pin<Box<dyn Future<Output = Result<UpstreamResponse, UpstreamError>> + Send + 'a>>;
type ProviderOAuthStartFuture<'a> =
    Pin<Box<dyn Future<Output = Result<UpstreamOAuthResponse, UpstreamError>> + Send + 'a>>;
type ProviderOAuthCallbackFuture<'a> =
    Pin<Box<dyn Future<Output = Result<UpstreamOAuthCallbackResult, UpstreamError>> + Send + 'a>>;

#[derive(Clone, Copy)]
struct ExecuteContext<'a> {
    provider: &'a ProviderDefinition,
    client: &'a WreqClient,
    spoof_client: Option<&'a WreqClient>,
    credential_states: &'a ChannelCredentialStateStore,
    request: &'a gproxy_middleware::TransformRequest,
    now_unix_ms: u64,
    token_resolution: TokenizerResolutionContext<'a>,
}

#[derive(Clone, Copy)]
struct OAuthContext<'a> {
    provider: &'a ProviderDefinition,
    client: &'a WreqClient,
    request: &'a UpstreamOAuthRequest,
}

#[derive(Clone, Copy)]
struct UpstreamUsageContext<'a> {
    provider: &'a ProviderDefinition,
    client: &'a WreqClient,
    spoof_client: Option<&'a WreqClient>,
    credential_states: &'a ChannelCredentialStateStore,
    credential_id: Option<i64>,
    now_unix_ms: u64,
}

#[derive(Clone, Copy)]
struct PayloadContext<'a> {
    provider: &'a ProviderDefinition,
    client: &'a WreqClient,
    spoof_client: Option<&'a WreqClient>,
    credential_states: &'a ChannelCredentialStateStore,
    payload: RetryWithPayloadRequest<'a>,
}

type ExecuteHandler = for<'a> fn(ExecuteContext<'a>) -> ProviderResponseFuture<'a>;
type PayloadHandler = for<'a> fn(PayloadContext<'a>) -> ProviderResponseFuture<'a>;
type OAuthStartHandler = for<'a> fn(OAuthContext<'a>) -> ProviderOAuthStartFuture<'a>;
type OAuthCallbackHandler = for<'a> fn(OAuthContext<'a>) -> ProviderOAuthCallbackFuture<'a>;
type UpstreamUsageHandler = for<'a> fn(UpstreamUsageContext<'a>) -> ProviderResponseFuture<'a>;

mod capabilities;
mod definition;
mod registry;

pub use definition::ProviderDefinition;
pub use registry::ProviderRegistry;
