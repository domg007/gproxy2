use super::*;

pub async fn execute_geminicli_upstream_usage_with_retry(
    client: &WreqClient,
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

    let base_url = scoped_provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }
    let usage_path = "/v1internal:retrieveUserQuota".to_string();
    let usage_url = join_base_url_and_path(base_url, usage_path.as_str());
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let usage_url_template = usage_url.clone();
    let channel_template = scoped_provider.channel.clone();
    let user_agent_template = scoped_provider
        .settings
        .user_agent()
        .map(str::trim)
        .map(ToOwned::to_owned);

    retry_with_eligible_credentials(
        &scoped_provider,
        credential_states,
        None,
        now_unix_ms,
        |credential| {
            if let ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(value)) =
                &credential.credential
            {
                return geminicli_auth_material_from_credential(value);
            }
            None
        },
        |attempt| {
            let usage_url = usage_url_template.clone();
            let channel = channel_template.clone();
            let user_agent = user_agent_template.clone();
            async move {
                if attempt.material.project_id.trim().is_empty() {
                    let message = "missing project_id in geminicli credential".to_string();
                    state_manager.mark_auth_dead(
                        credential_states,
                        &channel,
                        attempt.credential_id,
                        Some(message.clone()),
                    );
                    return CredentialRetryDecision::Retry {
                        last_status: None,
                        last_error: Some(message),
                        last_request_meta: None,
                    };
                }

                let token_cache_key =
                    format!("{}::{}::usage", channel.as_str(), attempt.credential_id);
                let mut credential_update = None;

                let resolved_access_token = match resolve_geminicli_access_token(
                    client,
                    &provider.settings,
                    token_cache_key.as_str(),
                    &attempt.material,
                    now_unix_ms,
                    false,
                )
                .await
                {
                    Ok(token) => token,
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
                if let Some(refreshed) = resolved_access_token.refreshed.as_ref() {
                    credential_update = Some(geminicli_credential_update(
                        attempt.credential_id,
                        refreshed,
                    ));
                }

                let usage_body = json!({
                    "project": attempt.material.project_id.trim(),
                });
                let usage_body_bytes = serde_json::to_vec(&usage_body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()));
                let usage_body_bytes = match usage_body_bytes {
                    Ok(bytes) => bytes,
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
                let response = send_geminicli_request(
                    client,
                    GeminiCliRequestParams {
                        method: WreqMethod::POST,
                        url: usage_url.as_str(),
                        access_token: resolved_access_token.access_token.as_str(),
                        custom_user_agent: user_agent.as_deref(),
                        model_for_ua: None,
                        extra_headers: &[],
                        body: Some(usage_body_bytes.as_slice()),
                    },
                )
                .await;
                let (response, request_meta) = match response {
                    Ok((response, request_meta)) => (response, request_meta),
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

                if response.status().is_success() {
                    state_manager.mark_success(credential_states, &channel, attempt.credential_id);
                    return CredentialRetryDecision::Return(
                        UpstreamResponse::from_http(
                            attempt.credential_id,
                            attempt.attempts,
                            response,
                        )
                        .with_request_meta(request_meta.clone())
                        .with_credential_update(credential_update.clone()),
                    );
                }

                let status_code = response.status().as_u16();
                if is_auth_failure(status_code) {
                    let message = format!("upstream status {status_code}");
                    state_manager.mark_auth_dead(
                        credential_states,
                        &channel,
                        attempt.credential_id,
                        Some(message.clone()),
                    );
                    return CredentialRetryDecision::Retry {
                        last_status: Some(status_code),
                        last_error: Some(message),
                        last_request_meta: None,
                    };
                }
                if status_code == 429 {
                    let retry_after_ms = retry_after_to_millis(response.headers());
                    let explicit_quota = response
                        .bytes()
                        .await
                        .map(|body| geminicli_response_indicates_quota_exhausted(body.as_ref()))
                        .unwrap_or(false);
                    let message = if explicit_quota {
                        format!("upstream status {status_code} (quota exhausted)")
                    } else {
                        format!("upstream status {status_code} (transient overload)")
                    };
                    if explicit_quota {
                        state_manager.mark_rate_limited(
                            credential_states,
                            &channel,
                            attempt.credential_id,
                            None,
                            retry_after_ms,
                            Some(message.clone()),
                        );
                    } else {
                        state_manager.mark_transient_failure(
                            credential_states,
                            &channel,
                            attempt.credential_id,
                            None,
                            retry_after_ms,
                            Some(message.clone()),
                        );
                    }
                    return CredentialRetryDecision::Retry {
                        last_status: Some(status_code),
                        last_error: Some(message),
                        last_request_meta: None,
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
                        last_request_meta: None,
                    };
                }
                CredentialRetryDecision::Return(
                    UpstreamResponse::from_http(attempt.credential_id, attempt.attempts, response)
                        .with_request_meta(request_meta)
                        .with_credential_update(credential_update.clone()),
                )
            }
        },
    )
    .await
}

pub(super) fn geminicli_credential_update(
    credential_id: i64,
    refreshed: &GeminiCliRefreshedToken,
) -> UpstreamCredentialUpdate {
    UpstreamCredentialUpdate::GeminiCliTokenRefresh {
        credential_id,
        access_token: refreshed.access_token.clone(),
        refresh_token: refreshed.refresh_token.clone(),
        expires_at_unix_ms: refreshed.expires_at_unix_ms,
        user_email: refreshed.user_email.clone(),
    }
}
