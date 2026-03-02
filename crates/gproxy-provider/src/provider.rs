use crate::UpstreamCredentialUpdate;
use crate::channel::ChannelId;
use crate::channels::ChannelSettings;
use crate::channels::aistudio::execute_aistudio_with_retry;
use crate::channels::antigravity::{
    execute_antigravity_oauth_callback, execute_antigravity_oauth_start,
    execute_antigravity_upstream_usage_with_retry, execute_antigravity_with_retry,
};
use crate::channels::claude::execute_claude_with_retry;
use crate::channels::claudecode::credential::ClaudeCodeTokenRefresh;
use crate::channels::claudecode::{
    execute_claudecode_oauth_callback, execute_claudecode_oauth_start,
    execute_claudecode_upstream_usage_with_retry, execute_claudecode_with_retry,
};
use crate::channels::codex::{
    execute_codex_oauth_callback, execute_codex_oauth_start,
    execute_codex_upstream_usage_with_retry, execute_codex_with_retry,
};
use crate::channels::custom::execute_custom_with_retry;
use crate::channels::deepseek::execute_deepseek_with_retry;
use crate::channels::geminicli::{
    execute_geminicli_oauth_callback, execute_geminicli_oauth_start,
    execute_geminicli_upstream_usage_with_retry, execute_geminicli_with_retry,
};
use crate::channels::groq::execute_groq_with_retry;
use crate::channels::nvidia::execute_nvidia_with_retry;
use crate::channels::openai::execute_openai_with_retry;
use crate::channels::retry::CredentialPickMode;
use crate::channels::upstream::{
    UpstreamError, UpstreamOAuthCallbackResult, UpstreamOAuthRequest, UpstreamOAuthResponse,
    UpstreamResponse,
};
use crate::channels::vertex::execute_vertex_with_retry;
use crate::channels::vertexexpress::execute_vertexexpress_with_retry;
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::{CredentialRef, ProviderCredentialState};
use crate::dispatch::ProviderDispatchTable;
use crate::tokenizers::LocalTokenizerStore;
use wreq::Client as WreqClient;

#[derive(Debug, Clone, Copy)]
pub struct TokenizerResolutionContext<'a> {
    pub tokenizer_store: &'a LocalTokenizerStore,
    pub hf_token: Option<&'a str>,
    pub hf_url: Option<&'a str>,
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
        match &self.channel {
            ChannelId::Builtin(crate::channel::BuiltinChannel::ClaudeCode) => {
                execute_claudecode_oauth_start(client, &self.settings, request).await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::GeminiCli) => {
                execute_geminicli_oauth_start(client, &self.settings, request).await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Codex) => {
                execute_codex_oauth_start(client, &self.settings, request).await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Antigravity) => {
                execute_antigravity_oauth_start(client, &self.settings, request).await
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }

    pub async fn execute_oauth_callback(
        &self,
        client: &WreqClient,
        request: &UpstreamOAuthRequest,
    ) -> Result<UpstreamOAuthCallbackResult, UpstreamError> {
        match &self.channel {
            ChannelId::Builtin(crate::channel::BuiltinChannel::ClaudeCode) => {
                execute_claudecode_oauth_callback(client, &self.settings, request).await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::GeminiCli) => {
                execute_geminicli_oauth_callback(client, &self.settings, request).await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Codex) => {
                execute_codex_oauth_callback(client, &self.settings, request).await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Antigravity) => {
                execute_antigravity_oauth_callback(client, &self.settings, request).await
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }

    pub async fn execute_upstream_usage_with_retry(
        &self,
        client: &WreqClient,
        credential_states: &crate::credential::ChannelCredentialStateStore,
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
        credential_states: &crate::credential::ChannelCredentialStateStore,
        credential_id: Option<i64>,
        now_unix_ms: u64,
    ) -> Result<UpstreamResponse, UpstreamError> {
        match &self.channel {
            ChannelId::Builtin(crate::channel::BuiltinChannel::ClaudeCode) => {
                execute_claudecode_upstream_usage_with_retry(
                    client,
                    spoof_client.unwrap_or(client),
                    self,
                    credential_states,
                    credential_id,
                    now_unix_ms,
                )
                .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Codex) => {
                execute_codex_upstream_usage_with_retry(
                    client,
                    self,
                    credential_states,
                    credential_id,
                    now_unix_ms,
                )
                .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::GeminiCli) => {
                execute_geminicli_upstream_usage_with_retry(
                    client,
                    self,
                    credential_states,
                    credential_id,
                    now_unix_ms,
                )
                .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Antigravity) => {
                execute_antigravity_upstream_usage_with_retry(
                    client,
                    self,
                    credential_states,
                    credential_id,
                    now_unix_ms,
                )
                .await
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }

    pub async fn execute_with_retry(
        &self,
        client: &WreqClient,
        credential_states: &crate::credential::ChannelCredentialStateStore,
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
        credential_states: &crate::credential::ChannelCredentialStateStore,
        request: &gproxy_middleware::TransformRequest,
        now_unix_ms: u64,
        token_resolution: TokenizerResolutionContext<'_>,
    ) -> Result<UpstreamResponse, UpstreamError> {
        match &self.channel {
            ChannelId::Builtin(crate::channel::BuiltinChannel::OpenAi) => {
                execute_openai_with_retry(client, self, credential_states, request, now_unix_ms)
                    .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Claude) => {
                execute_claude_with_retry(client, self, credential_states, request, now_unix_ms)
                    .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::ClaudeCode) => {
                execute_claudecode_with_retry(
                    client,
                    spoof_client.unwrap_or(client),
                    self,
                    credential_states,
                    request,
                    now_unix_ms,
                )
                .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::AiStudio) => {
                execute_aistudio_with_retry(client, self, credential_states, request, now_unix_ms)
                    .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::VertexExpress) => {
                execute_vertexexpress_with_retry(
                    client,
                    self,
                    credential_states,
                    request,
                    now_unix_ms,
                )
                .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Vertex) => {
                execute_vertex_with_retry(client, self, credential_states, request, now_unix_ms)
                    .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::GeminiCli) => {
                execute_geminicli_with_retry(client, self, credential_states, request, now_unix_ms)
                    .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Codex) => {
                execute_codex_with_retry(
                    client,
                    self,
                    credential_states,
                    request,
                    now_unix_ms,
                    token_resolution,
                )
                .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Deepseek) => {
                execute_deepseek_with_retry(
                    client,
                    self,
                    credential_states,
                    request,
                    now_unix_ms,
                    token_resolution,
                )
                .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Antigravity) => {
                execute_antigravity_with_retry(
                    client,
                    self,
                    credential_states,
                    request,
                    now_unix_ms,
                )
                .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Nvidia) => {
                execute_nvidia_with_retry(
                    client,
                    self,
                    credential_states,
                    request,
                    now_unix_ms,
                    token_resolution,
                )
                .await
            }
            ChannelId::Builtin(crate::channel::BuiltinChannel::Groq) => {
                execute_groq_with_retry(
                    client,
                    self,
                    credential_states,
                    request,
                    now_unix_ms,
                    token_resolution,
                )
                .await
            }
            ChannelId::Custom(_) => {
                execute_custom_with_retry(client, self, credential_states, request, now_unix_ms)
                    .await
            }
        }
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
