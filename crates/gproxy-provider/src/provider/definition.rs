use super::capabilities::channel_capabilities;
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDefinition {
    pub channel: ChannelId,
    pub dispatch: ProviderDispatchTable,
    pub settings: ChannelSettings,
    pub credential_pick_mode: CredentialPickMode,
    pub cache_affinity_max_keys: usize,
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
                organization_uuid,
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
                        organization_uuid: organization_uuid.as_deref(),
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
                    if let Some(organization_uuid) = organization_uuid {
                        let org_missing = value
                            .organization_uuid
                            .as_ref()
                            .map(|existing| existing.trim().is_empty())
                            .unwrap_or(true);
                        if org_missing {
                            value.organization_uuid = Some(organization_uuid.clone());
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
