use super::*;

pub async fn execute_claudecode_upstream_usage_with_retry(
    client: &WreqClient,
    spoof_client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    credential_id: Option<i64>,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let mut scoped_provider = provider.clone();
    if let Some(credential_id) = credential_id {
        scoped_provider
            .credentials
            .credentials
            .retain(|credential| credential.id == credential_id);
    }
    if scoped_provider.credentials.credentials.is_empty() {
        return Err(UpstreamError::NoEligibleCredential {
            channel: scoped_provider.channel.as_str().to_string(),
            model: None,
        });
    }

    let state_manager = CredentialStateManager::new(now_unix_ms);
    let usage_url = format!(
        "{}/api/oauth/usage",
        scoped_provider
            .settings
            .claudecode_platform_base_url()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_PLATFORM_BASE_URL)
            .trim_end_matches('/')
    );
    let claude_ai_base_url = scoped_provider
        .settings
        .claudecode_ai_base_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_CLAUDE_AI_BASE_URL)
        .to_string();
    let request_user_agent_template = scoped_provider
        .settings
        .user_agent()
        .map(str::trim)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| CLAUDE_CODE_UA.to_string());

    retry_with_eligible_credentials(
        &scoped_provider,
        credential_states,
        None,
        now_unix_ms,
        |credential| match &credential.credential {
            ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(value)) => {
                claudecode_access_token_from_credential(value)
            }
            _ => None,
        },
        |attempt| {
            let channel = scoped_provider.channel.clone();
            let usage_url = usage_url.clone();
            let base_url = scoped_provider.settings.base_url().to_string();
            let claude_ai_base_url = claude_ai_base_url.clone();
            let request_user_agent = request_user_agent_template.clone();

            async move {
                let active_client = if attempt.material.has_cookie() {
                    spoof_client
                } else {
                    client
                };
                let cache_key = format!("{}::{}", channel.as_str(), attempt.credential_id);
                let mut credential_update = None;

                let access_token = match resolve_claudecode_access_token(
                    active_client,
                    cache_key.as_str(),
                    &attempt.material,
                    base_url.as_str(),
                    claude_ai_base_url.as_str(),
                    now_unix_ms,
                    false,
                )
                .await
                {
                    Ok(token) => {
                        if let Some(refreshed) = token.refreshed.as_ref() {
                            credential_update = Some(claudecode_credential_update(
                                attempt.credential_id,
                                refreshed,
                            ));
                        }
                        token.access_token
                    }
                    Err(err) => {
                        let message = err.as_message();
                        if err.is_invalid_credential() {
                            state_manager.mark_auth_dead(
                                credential_states,
                                &channel,
                                attempt.credential_id,
                                Some(message.clone()),
                            );
                        } else {
                            state_manager.mark_transient_failure(
                                credential_states,
                                &channel,
                                attempt.credential_id,
                                None,
                                None,
                                Some(message.clone()),
                            );
                        }
                        return CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(message),
                            last_request_meta: None,
                        };
                    }
                };

                let (mut response, mut request_meta) = match send_claudecode_usage_request(
                    active_client,
                    usage_url.as_str(),
                    access_token.as_str(),
                    request_user_agent.as_str(),
                )
                .await
                {
                    Ok((response, request_meta)) => (response, Some(request_meta)),
                    Err(err) => {
                        let message = err.to_string();
                        state_manager.mark_transient_failure(
                            credential_states,
                            &channel,
                            attempt.credential_id,
                            None,
                            None,
                            Some(message.clone()),
                        );
                        return CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(message),
                            last_request_meta: None,
                        };
                    }
                };

                let mut status_code = response.status().as_u16();
                if is_auth_failure(status_code) {
                    let refreshed_token = match resolve_claudecode_access_token(
                        active_client,
                        cache_key.as_str(),
                        &attempt.material,
                        base_url.as_str(),
                        claude_ai_base_url.as_str(),
                        now_unix_ms,
                        true,
                    )
                    .await
                    {
                        Ok(token) => {
                            if let Some(refreshed) = token.refreshed.as_ref() {
                                credential_update = Some(claudecode_credential_update(
                                    attempt.credential_id,
                                    refreshed,
                                ));
                            }
                            token.access_token
                        }
                        Err(err) => {
                            let message = err.as_message();
                            if err.is_invalid_credential() {
                                state_manager.mark_auth_dead(
                                    credential_states,
                                    &channel,
                                    attempt.credential_id,
                                    Some(message.clone()),
                                );
                            } else {
                                state_manager.mark_transient_failure(
                                    credential_states,
                                    &channel,
                                    attempt.credential_id,
                                    None,
                                    None,
                                    Some(message.clone()),
                                );
                            }
                            return CredentialRetryDecision::Retry {
                                last_status: Some(status_code),
                                last_error: Some(message),
                                last_request_meta: request_meta.clone(),
                            };
                        }
                    };

                    let (retry_response, retry_meta) = match send_claudecode_usage_request(
                        active_client,
                        usage_url.as_str(),
                        refreshed_token.as_str(),
                        request_user_agent.as_str(),
                    )
                    .await
                    {
                        Ok((response, request_meta)) => (response, Some(request_meta)),
                        Err(err) => {
                            let message = err.to_string();
                            state_manager.mark_transient_failure(
                                credential_states,
                                &channel,
                                attempt.credential_id,
                                None,
                                None,
                                Some(message.clone()),
                            );
                            return CredentialRetryDecision::Retry {
                                last_status: None,
                                last_error: Some(message),
                                last_request_meta: request_meta.clone(),
                            };
                        }
                    };
                    response = retry_response;
                    request_meta = retry_meta;

                    status_code = response.status().as_u16();
                    if is_auth_failure(status_code) {
                        let message = format!(
                            "upstream status {} after claudecode access token refresh",
                            status_code
                        );
                        state_manager.mark_auth_dead(
                            credential_states,
                            &channel,
                            attempt.credential_id,
                            Some(message.clone()),
                        );
                        return CredentialRetryDecision::Retry {
                            last_status: Some(status_code),
                            last_error: Some(message),
                            last_request_meta: request_meta.clone(),
                        };
                    }
                }

                if status_code == 429 {
                    let retry_after_ms = retry_after_to_millis(response.headers());
                    let message = format!("upstream status {status_code}");
                    state_manager.mark_rate_limited(
                        credential_states,
                        &channel,
                        attempt.credential_id,
                        None,
                        retry_after_ms,
                        Some(message.clone()),
                    );
                    return CredentialRetryDecision::Retry {
                        last_status: Some(status_code),
                        last_error: Some(message),
                        last_request_meta: request_meta.clone(),
                    };
                }

                if is_transient_server_failure(status_code) {
                    let message = format!("upstream status {status_code}");
                    state_manager.mark_transient_failure(
                        credential_states,
                        &channel,
                        attempt.credential_id,
                        None,
                        None,
                        Some(message.clone()),
                    );
                    return CredentialRetryDecision::Retry {
                        last_status: Some(status_code),
                        last_error: Some(message),
                        last_request_meta: request_meta.clone(),
                    };
                }

                if response.status().is_success() {
                    state_manager.mark_success(credential_states, &channel, attempt.credential_id);
                }

                let mut upstream_response =
                    UpstreamResponse::from_http(attempt.credential_id, attempt.attempts, response)
                        .with_credential_update(credential_update.clone());
                if let Some(request_meta) = request_meta {
                    upstream_response = upstream_response.with_request_meta(request_meta);
                }
                CredentialRetryDecision::Return(upstream_response)
            }
        },
    )
    .await
}
