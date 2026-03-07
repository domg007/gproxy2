use crate::UpstreamCredentialUpdate;
use std::future::Future;
use std::pin::Pin;

use crate::channel::{BuiltinChannel, ChannelId};
use crate::channels::ChannelSettings;
use crate::channels::aistudio::{execute_aistudio_payload_with_retry, execute_aistudio_with_retry};
use crate::channels::antigravity::{
    execute_antigravity_oauth_callback, execute_antigravity_oauth_start,
    execute_antigravity_payload_with_retry, execute_antigravity_upstream_usage_with_retry,
    execute_antigravity_with_retry,
};
use crate::channels::claude::{execute_claude_payload_with_retry, execute_claude_with_retry};
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

#[derive(Clone, Copy)]
struct ProviderChannelCapabilities {
    execute: ExecuteHandler,
    payload: Option<PayloadHandler>,
    oauth_start: Option<OAuthStartHandler>,
    oauth_callback: Option<OAuthCallbackHandler>,
    upstream_usage: Option<UpstreamUsageHandler>,
}

macro_rules! define_execute_handler {
    ($name:ident, $handler:path, standard) => {
        fn $name<'a>(context: ExecuteContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.request,
                context.now_unix_ms,
            ))
        }
    };
    ($name:ident, $handler:path, spoof) => {
        fn $name<'a>(context: ExecuteContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.spoof_client.unwrap_or(context.client),
                context.provider,
                context.credential_states,
                context.request,
                context.now_unix_ms,
            ))
        }
    };
    ($name:ident, $handler:path, tokenized) => {
        fn $name<'a>(context: ExecuteContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.request,
                context.now_unix_ms,
                context.token_resolution,
            ))
        }
    };
}

macro_rules! define_payload_handler {
    ($name:ident, $handler:path, split) => {
        fn $name<'a>(context: PayloadContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.payload.operation,
                context.payload.protocol,
                context.payload.body,
                context.payload.now_unix_ms,
            ))
        }
    };
    ($name:ident, $handler:path, payload) => {
        fn $name<'a>(context: PayloadContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.payload,
            ))
        }
    };
    ($name:ident, $handler:path, spoof_payload) => {
        fn $name<'a>(context: PayloadContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.spoof_client.unwrap_or(context.client),
                context.provider,
                context.credential_states,
                context.payload,
            ))
        }
    };
}

macro_rules! define_oauth_start_handler {
    ($name:ident, $handler:path) => {
        fn $name<'a>(context: OAuthContext<'a>) -> ProviderOAuthStartFuture<'a> {
            Box::pin($handler(
                context.client,
                &context.provider.settings,
                context.request,
            ))
        }
    };
}

macro_rules! define_oauth_callback_handler {
    ($name:ident, $handler:path) => {
        fn $name<'a>(context: OAuthContext<'a>) -> ProviderOAuthCallbackFuture<'a> {
            Box::pin($handler(
                context.client,
                &context.provider.settings,
                context.request,
            ))
        }
    };
}

macro_rules! define_upstream_usage_handler {
    ($name:ident, $handler:path, standard) => {
        fn $name<'a>(context: UpstreamUsageContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.provider,
                context.credential_states,
                context.credential_id,
                context.now_unix_ms,
            ))
        }
    };
    ($name:ident, $handler:path, spoof) => {
        fn $name<'a>(context: UpstreamUsageContext<'a>) -> ProviderResponseFuture<'a> {
            Box::pin($handler(
                context.client,
                context.spoof_client.unwrap_or(context.client),
                context.provider,
                context.credential_states,
                context.credential_id,
                context.now_unix_ms,
            ))
        }
    };
}

define_execute_handler!(
    execute_openai_capability,
    execute_openai_with_retry,
    standard
);
define_execute_handler!(
    execute_claude_capability,
    execute_claude_with_retry,
    standard
);
define_execute_handler!(
    execute_claudecode_capability,
    execute_claudecode_with_retry,
    spoof
);
define_execute_handler!(
    execute_aistudio_capability,
    execute_aistudio_with_retry,
    standard
);
define_execute_handler!(
    execute_vertexexpress_capability,
    execute_vertexexpress_with_retry,
    standard
);
define_execute_handler!(
    execute_vertex_capability,
    execute_vertex_with_retry,
    standard
);
define_execute_handler!(
    execute_geminicli_capability,
    execute_geminicli_with_retry,
    standard
);
define_execute_handler!(
    execute_codex_capability,
    execute_codex_with_retry,
    tokenized
);
define_execute_handler!(
    execute_antigravity_capability,
    execute_antigravity_with_retry,
    standard
);
define_execute_handler!(
    execute_nvidia_capability,
    execute_nvidia_with_retry,
    tokenized
);
define_execute_handler!(
    execute_deepseek_capability,
    execute_deepseek_with_retry,
    tokenized
);
define_execute_handler!(execute_groq_capability, execute_groq_with_retry, tokenized);
define_execute_handler!(
    execute_custom_capability,
    execute_custom_with_retry,
    standard
);

define_payload_handler!(
    payload_openai_capability,
    execute_openai_payload_with_retry,
    split
);
define_payload_handler!(
    payload_claude_capability,
    execute_claude_payload_with_retry,
    split
);
define_payload_handler!(
    payload_claudecode_capability,
    execute_claudecode_payload_with_retry,
    spoof_payload
);
define_payload_handler!(
    payload_aistudio_capability,
    execute_aistudio_payload_with_retry,
    split
);
define_payload_handler!(
    payload_vertexexpress_capability,
    execute_vertexexpress_payload_with_retry,
    split
);
define_payload_handler!(
    payload_vertex_capability,
    execute_vertex_payload_with_retry,
    split
);
define_payload_handler!(
    payload_geminicli_capability,
    execute_geminicli_payload_with_retry,
    split
);
define_payload_handler!(
    payload_codex_capability,
    execute_codex_payload_with_retry,
    payload
);
define_payload_handler!(
    payload_antigravity_capability,
    execute_antigravity_payload_with_retry,
    split
);
define_payload_handler!(
    payload_nvidia_capability,
    execute_nvidia_payload_with_retry,
    payload
);
define_payload_handler!(
    payload_deepseek_capability,
    execute_deepseek_payload_with_retry,
    payload
);
define_payload_handler!(
    payload_groq_capability,
    execute_groq_payload_with_retry,
    payload
);

define_oauth_start_handler!(
    oauth_start_claudecode_capability,
    execute_claudecode_oauth_start
);
define_oauth_start_handler!(
    oauth_start_geminicli_capability,
    execute_geminicli_oauth_start
);
define_oauth_start_handler!(oauth_start_codex_capability, execute_codex_oauth_start);
define_oauth_start_handler!(
    oauth_start_antigravity_capability,
    execute_antigravity_oauth_start
);

define_oauth_callback_handler!(
    oauth_callback_claudecode_capability,
    execute_claudecode_oauth_callback
);
define_oauth_callback_handler!(
    oauth_callback_geminicli_capability,
    execute_geminicli_oauth_callback
);
define_oauth_callback_handler!(
    oauth_callback_codex_capability,
    execute_codex_oauth_callback
);
define_oauth_callback_handler!(
    oauth_callback_antigravity_capability,
    execute_antigravity_oauth_callback
);

define_upstream_usage_handler!(
    usage_claudecode_capability,
    execute_claudecode_upstream_usage_with_retry,
    spoof
);
define_upstream_usage_handler!(
    usage_geminicli_capability,
    execute_geminicli_upstream_usage_with_retry,
    standard
);
define_upstream_usage_handler!(
    usage_codex_capability,
    execute_codex_upstream_usage_with_retry,
    standard
);
define_upstream_usage_handler!(
    usage_antigravity_capability,
    execute_antigravity_upstream_usage_with_retry,
    standard
);

fn channel_capabilities(channel: &ChannelId) -> ProviderChannelCapabilities {
    match channel {
        ChannelId::Builtin(BuiltinChannel::OpenAi) => ProviderChannelCapabilities {
            execute: execute_openai_capability,
            payload: Some(payload_openai_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::Claude) => ProviderChannelCapabilities {
            execute: execute_claude_capability,
            payload: Some(payload_claude_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::ClaudeCode) => ProviderChannelCapabilities {
            execute: execute_claudecode_capability,
            payload: Some(payload_claudecode_capability),
            oauth_start: Some(oauth_start_claudecode_capability),
            oauth_callback: Some(oauth_callback_claudecode_capability),
            upstream_usage: Some(usage_claudecode_capability),
        },
        ChannelId::Builtin(BuiltinChannel::AiStudio) => ProviderChannelCapabilities {
            execute: execute_aistudio_capability,
            payload: Some(payload_aistudio_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::VertexExpress) => ProviderChannelCapabilities {
            execute: execute_vertexexpress_capability,
            payload: Some(payload_vertexexpress_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::Vertex) => ProviderChannelCapabilities {
            execute: execute_vertex_capability,
            payload: Some(payload_vertex_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::GeminiCli) => ProviderChannelCapabilities {
            execute: execute_geminicli_capability,
            payload: Some(payload_geminicli_capability),
            oauth_start: Some(oauth_start_geminicli_capability),
            oauth_callback: Some(oauth_callback_geminicli_capability),
            upstream_usage: Some(usage_geminicli_capability),
        },
        ChannelId::Builtin(BuiltinChannel::Codex) => ProviderChannelCapabilities {
            execute: execute_codex_capability,
            payload: Some(payload_codex_capability),
            oauth_start: Some(oauth_start_codex_capability),
            oauth_callback: Some(oauth_callback_codex_capability),
            upstream_usage: Some(usage_codex_capability),
        },
        ChannelId::Builtin(BuiltinChannel::Antigravity) => ProviderChannelCapabilities {
            execute: execute_antigravity_capability,
            payload: Some(payload_antigravity_capability),
            oauth_start: Some(oauth_start_antigravity_capability),
            oauth_callback: Some(oauth_callback_antigravity_capability),
            upstream_usage: Some(usage_antigravity_capability),
        },
        ChannelId::Builtin(BuiltinChannel::Nvidia) => ProviderChannelCapabilities {
            execute: execute_nvidia_capability,
            payload: Some(payload_nvidia_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::Deepseek) => ProviderChannelCapabilities {
            execute: execute_deepseek_capability,
            payload: Some(payload_deepseek_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Builtin(BuiltinChannel::Groq) => ProviderChannelCapabilities {
            execute: execute_groq_capability,
            payload: Some(payload_groq_capability),
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
        ChannelId::Custom(_) => ProviderChannelCapabilities {
            execute: execute_custom_capability,
            payload: None,
            oauth_start: None,
            oauth_callback: None,
            upstream_usage: None,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDefinition {
    pub channel: ChannelId,
    pub dispatch: ProviderDispatchTable,
    pub settings: ChannelSettings,
    pub credential_pick_mode: CredentialPickMode,
    pub credentials: ProviderCredentialState,
}

impl ProviderDefinition {
    pub fn create_credential(&mut self, credential: CredentialRef) -> bool {
        self.credentials.create_credential(credential)
    }

    pub fn update_credential(&mut self, credential: CredentialRef) -> bool {
        self.credentials.update_credential(credential)
    }

    pub fn upsert_credential(&mut self, credential: CredentialRef) {
        self.credentials.upsert_credential(credential);
    }

    pub fn delete_credential(&mut self, credential_id: i64) -> Option<CredentialRef> {
        self.credentials.delete_credential(credential_id)
    }

    pub fn pick_random_eligible_credential(
        &self,
        model: Option<&str>,
        now_unix_ms: u64,
    ) -> Option<&CredentialRef> {
        self.credentials
            .pick_random_eligible_credential(&self.channel, model, now_unix_ms)
    }

    pub fn apply_upstream_credential_update(&mut self, update: &UpstreamCredentialUpdate) -> bool {
        match update {
            UpstreamCredentialUpdate::CodexTokenRefresh {
                credential_id,
                access_token,
                refresh_token,
                expires_at_unix_ms,
                user_email,
                id_token,
            } => {
                let Some(credential) = self
                    .credentials
                    .credentials
                    .iter_mut()
                    .find(|item| item.id == *credential_id)
                else {
                    return false;
                };
                let ChannelCredential::Builtin(BuiltinChannelCredential::Codex(value)) =
                    &mut credential.credential
                else {
                    return false;
                };
                value.apply_token_refresh(
                    access_token.as_str(),
                    refresh_token.as_str(),
                    *expires_at_unix_ms,
                    user_email.as_deref(),
                    id_token.as_deref(),
                );
                true
            }
            UpstreamCredentialUpdate::ClaudeCodeTokenRefresh {
                credential_id,
                access_token,
                refresh_token,
                expires_at_unix_ms,
                subscription_type,
                rate_limit_tier,
                user_email,
                cookie,
                enable_claude_1m_sonnet,
                enable_claude_1m_opus,
            } => {
                let Some(credential) = self
                    .credentials
                    .credentials
                    .iter_mut()
                    .find(|item| item.id == *credential_id)
                else {
                    return false;
                };
                let ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(value)) =
                    &mut credential.credential
                else {
                    return false;
                };
                if let (Some(access_token), Some(refresh_token), Some(expires_at_unix_ms)) = (
                    access_token.as_deref(),
                    refresh_token.as_deref(),
                    *expires_at_unix_ms,
                ) {
                    value.apply_token_refresh(ClaudeCodeTokenRefresh {
                        access_token,
                        refresh_token,
                        expires_at_unix_ms,
                        subscription_type: subscription_type.as_deref(),
                        rate_limit_tier: rate_limit_tier.as_deref(),
                        user_email: user_email.as_deref(),
                        cookie: cookie.as_deref(),
                    });
                } else {
                    if let Some(subscription_type) = subscription_type {
                        value.subscription_type = subscription_type.clone();
                    }
                    if let Some(rate_limit_tier) = rate_limit_tier {
                        value.rate_limit_tier = rate_limit_tier.clone();
                    }
                    if let Some(user_email) = user_email {
                        let email_missing = value
                            .user_email
                            .as_ref()
                            .map(|existing| existing.trim().is_empty())
                            .unwrap_or(true);
                        if email_missing {
                            value.user_email = Some(user_email.clone());
                        }
                    }
                    if let Some(cookie) = cookie {
                        value.cookie = Some(cookie.clone());
                    }
                }
                if let Some(enabled) = enable_claude_1m_sonnet {
                    value.enable_claude_1m_sonnet = Some(*enabled);
                }
                if let Some(enabled) = enable_claude_1m_opus {
                    value.enable_claude_1m_opus = Some(*enabled);
                }
                true
            }
            UpstreamCredentialUpdate::VertexTokenRefresh {
                credential_id,
                access_token,
                expires_at_unix_ms,
            } => {
                let Some(credential) = self
                    .credentials
                    .credentials
                    .iter_mut()
                    .find(|item| item.id == *credential_id)
                else {
                    return false;
                };
                let ChannelCredential::Builtin(BuiltinChannelCredential::Vertex(value)) =
                    &mut credential.credential
                else {
                    return false;
                };
                value.access_token = access_token.clone();
                value.expires_at = (*expires_at_unix_ms).min(i64::MAX as u64) as i64;
                true
            }
            UpstreamCredentialUpdate::GeminiCliTokenRefresh {
                credential_id,
                access_token,
                refresh_token,
                expires_at_unix_ms,
                user_email,
            } => {
                let Some(credential) = self
                    .credentials
                    .credentials
                    .iter_mut()
                    .find(|item| item.id == *credential_id)
                else {
                    return false;
                };
                let ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(value)) =
                    &mut credential.credential
                else {
                    return false;
                };
                value.apply_token_refresh(
                    access_token.as_str(),
                    refresh_token.as_deref(),
                    *expires_at_unix_ms,
                    user_email.as_deref(),
                );
                true
            }
            UpstreamCredentialUpdate::AntigravityTokenRefresh {
                credential_id,
                access_token,
                refresh_token,
                expires_at_unix_ms,
                user_email,
            } => {
                let Some(credential) = self
                    .credentials
                    .credentials
                    .iter_mut()
                    .find(|item| item.id == *credential_id)
                else {
                    return false;
                };
                let ChannelCredential::Builtin(BuiltinChannelCredential::Antigravity(value)) =
                    &mut credential.credential
                else {
                    return false;
                };
                value.access_token = access_token.clone();
                value.refresh_token = refresh_token.clone();
                value.expires_at = (*expires_at_unix_ms).min(i64::MAX as u64) as i64;
                if let Some(user_email) = user_email {
                    let email_missing = value
                        .user_email
                        .as_ref()
                        .map(|existing| existing.trim().is_empty())
                        .unwrap_or(true);
                    if email_missing {
                        value.user_email = Some(user_email.clone());
                    }
                }
                true
            }
        }
    }

    pub async fn execute_oauth_start(
        &self,
        client: &WreqClient,
        request: &UpstreamOAuthRequest,
    ) -> Result<UpstreamOAuthResponse, UpstreamError> {
        let Some(handler) = channel_capabilities(&self.channel).oauth_start else {
            return Err(UpstreamError::UnsupportedRequest);
        };
        handler(OAuthContext {
            provider: self,
            client,
            request,
        })
        .await
    }

    pub async fn execute_oauth_callback(
        &self,
        client: &WreqClient,
        request: &UpstreamOAuthRequest,
    ) -> Result<UpstreamOAuthCallbackResult, UpstreamError> {
        let Some(handler) = channel_capabilities(&self.channel).oauth_callback else {
            return Err(UpstreamError::UnsupportedRequest);
        };
        handler(OAuthContext {
            provider: self,
            client,
            request,
        })
        .await
    }

    pub async fn execute_upstream_usage_with_retry(
        &self,
        client: &WreqClient,
        credential_states: &ChannelCredentialStateStore,
        credential_id: Option<i64>,
        now_unix_ms: u64,
    ) -> Result<UpstreamResponse, UpstreamError> {
        self.execute_upstream_usage_with_retry_with_spoof(
            client,
            None,
            credential_states,
            credential_id,
            now_unix_ms,
        )
        .await
    }

    pub async fn execute_upstream_usage_with_retry_with_spoof(
        &self,
        client: &WreqClient,
        spoof_client: Option<&WreqClient>,
        credential_states: &ChannelCredentialStateStore,
        credential_id: Option<i64>,
        now_unix_ms: u64,
    ) -> Result<UpstreamResponse, UpstreamError> {
        let Some(handler) = channel_capabilities(&self.channel).upstream_usage else {
            return Err(UpstreamError::UnsupportedRequest);
        };
        handler(UpstreamUsageContext {
            provider: self,
            client,
            spoof_client,
            credential_states,
            credential_id,
            now_unix_ms,
        })
        .await
    }

    pub async fn execute_with_retry(
        &self,
        client: &WreqClient,
        credential_states: &ChannelCredentialStateStore,
        request: &gproxy_middleware::TransformRequest,
        now_unix_ms: u64,
        token_resolution: TokenizerResolutionContext<'_>,
    ) -> Result<UpstreamResponse, UpstreamError> {
        self.execute_with_retry_with_spoof(
            client,
            None,
            credential_states,
            request,
            now_unix_ms,
            token_resolution,
        )
        .await
    }

    pub async fn execute_with_retry_with_spoof(
        &self,
        client: &WreqClient,
        spoof_client: Option<&WreqClient>,
        credential_states: &ChannelCredentialStateStore,
        request: &gproxy_middleware::TransformRequest,
        now_unix_ms: u64,
        token_resolution: TokenizerResolutionContext<'_>,
    ) -> Result<UpstreamResponse, UpstreamError> {
        (channel_capabilities(&self.channel).execute)(ExecuteContext {
            provider: self,
            client,
            spoof_client,
            credential_states,
            request,
            now_unix_ms,
            token_resolution,
        })
        .await
    }

    pub async fn execute_payload_with_retry_with_spoof(
        &self,
        client: &WreqClient,
        spoof_client: Option<&WreqClient>,
        credential_states: &ChannelCredentialStateStore,
        payload: RetryWithPayloadRequest<'_>,
    ) -> Result<UpstreamResponse, UpstreamError> {
        let Some(handler) = channel_capabilities(&self.channel).payload else {
            return Err(UpstreamError::UnsupportedRequest);
        };
        handler(PayloadContext {
            provider: self,
            client,
            spoof_client,
            credential_states,
            payload,
        })
        .await
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderRegistry {
    pub providers: Vec<ProviderDefinition>,
}

impl ProviderRegistry {
    pub fn get(&self, channel: &ChannelId) -> Option<&ProviderDefinition> {
        self.providers
            .iter()
            .find(|provider| &provider.channel == channel)
    }

    pub fn get_mut(&mut self, channel: &ChannelId) -> Option<&mut ProviderDefinition> {
        self.providers
            .iter_mut()
            .find(|provider| &provider.channel == channel)
    }

    pub fn upsert(&mut self, provider: ProviderDefinition) {
        if let Some(existing) = self
            .providers
            .iter_mut()
            .find(|item| item.channel == provider.channel)
        {
            *existing = provider;
            return;
        }
        self.providers.push(provider);
    }

    pub fn create_credential(&mut self, channel: &ChannelId, credential: CredentialRef) -> bool {
        let Some(provider) = self.get_mut(channel) else {
            return false;
        };
        provider.create_credential(credential)
    }

    pub fn update_credential(&mut self, channel: &ChannelId, credential: CredentialRef) -> bool {
        let Some(provider) = self.get_mut(channel) else {
            return false;
        };
        provider.update_credential(credential)
    }

    pub fn upsert_credential(&mut self, channel: &ChannelId, credential: CredentialRef) -> bool {
        let Some(provider) = self.get_mut(channel) else {
            return false;
        };
        provider.upsert_credential(credential);
        true
    }

    pub fn delete_credential(
        &mut self,
        channel: &ChannelId,
        credential_id: i64,
    ) -> Option<CredentialRef> {
        let provider = self.get_mut(channel)?;
        provider.delete_credential(credential_id)
    }

    pub fn pick_random_eligible_credential(
        &self,
        channel: &ChannelId,
        model: Option<&str>,
        now_unix_ms: u64,
    ) -> Option<&CredentialRef> {
        self.get(channel)
            .and_then(|provider| provider.pick_random_eligible_credential(model, now_unix_ms))
    }

    pub fn apply_upstream_credential_update(
        &mut self,
        channel: &ChannelId,
        update: &UpstreamCredentialUpdate,
    ) -> bool {
        let Some(provider) = self.get_mut(channel) else {
            return false;
        };
        provider.apply_upstream_credential_update(update)
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderChannelCapabilities, channel_capabilities};
    use crate::channel::ChannelId;
    use crate::registry::BUILTIN_CHANNEL_REGISTRY;

    fn assert_builtin_payload_support(capabilities: ProviderChannelCapabilities, channel: &str) {
        assert!(
            capabilities.payload.is_some(),
            "builtin channel {channel} should support payload execution"
        );
    }

    #[test]
    fn builtin_capabilities_match_registry_flags() {
        for entry in BUILTIN_CHANNEL_REGISTRY {
            let capabilities = channel_capabilities(&ChannelId::Builtin(entry.channel));
            assert_eq!(
                capabilities.oauth_start.is_some() && capabilities.oauth_callback.is_some(),
                entry.supports_oauth,
                "channel {} oauth capability mismatch",
                entry.id
            );
            assert_eq!(
                capabilities.upstream_usage.is_some(),
                entry.supports_upstream_usage,
                "channel {} upstream usage capability mismatch",
                entry.id
            );
            assert_builtin_payload_support(capabilities, entry.id);
        }
    }

    #[test]
    fn custom_channel_capabilities_only_support_execute() {
        let capabilities = channel_capabilities(&ChannelId::custom("custom-provider"));
        assert!(capabilities.payload.is_none());
        assert!(capabilities.oauth_start.is_none());
        assert!(capabilities.oauth_callback.is_none());
        assert!(capabilities.upstream_usage.is_none());
    }
}
