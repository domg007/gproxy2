use super::*;

pub(super) async fn execute_claudecode_with_prepared(
    client: &WreqClient,
    spoof_client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: ClaudeCodePreparedRequest,
    now_unix_ms: u64,
    cache_affinity_hint: Option<crate::channels::retry::CacheAffinityHint>,
) -> Result<UpstreamResponse, UpstreamError> {
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }
    let claude_ai_base_url = provider
        .settings
        .claudecode_ai_base_url()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_CLAUDE_AI_BASE_URL)
        .to_string();
    let url = join_base_url_and_path(base_url, prepared.path.as_str());

    let state_manager = CredentialStateManager::new(now_unix_ms);
    let method_template = prepared.method.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let request_headers_template = prepared.request_headers.clone();
    let extra_headers_template = prepared.extra_headers.clone();
    let url_template = url.clone();
    let base_url_template = base_url.to_string();
    let claude_ai_base_url_template = claude_ai_base_url.clone();
    let request_user_agent_template = provider
        .settings
        .user_agent()
        .map(str::trim)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| CLAUDE_CODE_UA.to_string());
    let pick_mode =
        credential_pick_mode(provider.credential_pick_mode, cache_affinity_hint.as_ref());

    retry_with_eligible_credentials_with_affinity(
        crate::channels::retry::CredentialRetryContext {
            provider,
            credential_states,
            model: prepared.model.as_deref(),
            now_unix_ms,
            pick_mode,
            cache_affinity_hint,
        },
        |credential| match &credential.credential {
            ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(value)) => {
                claudecode_access_token_from_credential(value)
            }
            _ => None,
        },
        |attempt| {
            let method = method_template.clone();
            let body = body_template.clone();
            let model = model_template.clone();
            let mut request_headers = request_headers_template.clone();
            let extra_headers = extra_headers_template.clone();
            let url = url_template.clone();
            let base_url = base_url_template.clone();
            let claude_ai_base_url = claude_ai_base_url_template.clone();
            let request_user_agent = request_user_agent_template.clone();
            let configured_beta_headers =
                provider.settings.claudecode_extra_beta_headers().to_vec();

            async move {
                let active_client = if attempt.material.has_cookie() {
                    spoof_client
                } else {
                    client
                };
                let cache_key = format!("{}::{}", provider.channel.as_str(), attempt.credential_id);
                let mut credential_update = None;
                merge_claudecode_beta_headers(
                    &mut request_headers,
                    configured_beta_headers.as_slice(),
                );

                let mut active_access_token = match resolve_claudecode_access_token(
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
                                &provider.channel,
                                attempt.credential_id,
                                Some(message.clone()),
                            );
                        } else {
                            state_manager.mark_transient_failure(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                model.as_deref(),
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

                let (mut response, mut request_meta) = match send_claudecode_request(
                    active_client,
                    ClaudeCodeRequestParams {
                        method: method.clone(),
                        url: url.as_str(),
                        access_token: active_access_token.as_str(),
                        user_agent: request_user_agent.as_str(),
                        extra_headers: extra_headers.as_slice(),
                        request_headers: request_headers.as_slice(),
                        body: body.as_deref(),
                    },
                )
                .await
                {
                    Ok((response, request_meta)) => (response, Some(request_meta)),
                    Err(err) => {
                        let message = err.to_string();
                        state_manager.mark_transient_failure(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                            model.as_deref(),
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
                                    &provider.channel,
                                    attempt.credential_id,
                                    Some(message.clone()),
                                );
                            } else {
                                state_manager.mark_transient_failure(
                                    credential_states,
                                    &provider.channel,
                                    attempt.credential_id,
                                    model.as_deref(),
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

                    active_access_token = refreshed_token;
                    let (retry_response, retry_meta) = match send_claudecode_request(
                        active_client,
                        ClaudeCodeRequestParams {
                            method: method.clone(),
                            url: url.as_str(),
                            access_token: active_access_token.as_str(),
                            user_agent: request_user_agent.as_str(),
                            extra_headers: extra_headers.as_slice(),
                            request_headers: request_headers.as_slice(),
                            body: body.as_deref(),
                        },
                    )
                    .await
                    {
                        Ok((response, request_meta)) => (response, Some(request_meta)),
                        Err(err) => {
                            let message = err.to_string();
                            state_manager.mark_transient_failure(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                model.as_deref(),
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
                            &provider.channel,
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
                        &provider.channel,
                        attempt.credential_id,
                        model.as_deref(),
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
                        &provider.channel,
                        attempt.credential_id,
                        model.as_deref(),
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
                    state_manager.mark_success(
                        credential_states,
                        &provider.channel,
                        attempt.credential_id,
                    );
                }

                if response.status().is_success()
                    && should_expand_claudecode_model_list(&method, url.as_str(), body.as_ref())
                {
                    let stats_code = response.status();
                    let header_extra = response
                        .headers()
                        .iter()
                        .filter_map(|(name, value)| {
                            value
                                .to_str()
                                .ok()
                                .map(|value| (name.as_str().to_string(), value.to_string()))
                        })
                        .collect::<std::collections::BTreeMap<String, String>>();
                    let body_bytes = match response.bytes().await {
                        Ok(bytes) => bytes,
                        Err(err) => {
                            let message = err.to_string();
                            state_manager.mark_transient_failure(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                model.as_deref(),
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
                    let mut model_list_body = match serde_json::from_slice::<Value>(&body_bytes) {
                        Ok(body) => body,
                        Err(err) => {
                            let message = format!("parse claudecode model list failed: {err}");
                            state_manager.mark_transient_failure(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                model.as_deref(),
                                None,
                                Some(message.clone()),
                            );
                            return CredentialRetryDecision::Retry {
                                last_status: Some(stats_code.as_u16()),
                                last_error: Some(message),
                                last_request_meta: request_meta.clone(),
                            };
                        }
                    };

                    if let Some(body_obj) = model_list_body.as_object_mut()
                        && let Some(data) = body_obj.get_mut("data").and_then(Value::as_array_mut)
                    {
                        extend_model_list_with_thinking_variants(data);
                        let first_id = data
                            .first()
                            .and_then(|item| item.get("id"))
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned);
                        let last_id = data
                            .last()
                            .and_then(|item| item.get("id"))
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned);
                        if let Some(first_id) = first_id {
                            body_obj.insert("first_id".to_string(), Value::String(first_id));
                        }
                        if let Some(last_id) = last_id {
                            body_obj.insert("last_id".to_string(), Value::String(last_id));
                        }
                    }

                    let model_list_response = match serde_json::from_value(json!({
                        "stats_code": stats_code.as_u16(),
                        "headers": header_extra,
                        "body": model_list_body,
                    })) {
                        Ok(response) => response,
                        Err(err) => {
                            let message =
                                format!("build claudecode model list response failed: {err}");
                            state_manager.mark_transient_failure(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                                model.as_deref(),
                                None,
                                Some(message.clone()),
                            );
                            return CredentialRetryDecision::Retry {
                                last_status: Some(stats_code.as_u16()),
                                last_error: Some(message),
                                last_request_meta: request_meta.clone(),
                            };
                        }
                    };

                    return CredentialRetryDecision::Return(UpstreamResponse {
                        credential_id: Some(attempt.credential_id),
                        attempts: attempt.attempts,
                        response: None,
                        local_response: Some(TransformResponse::ModelListClaude(
                            model_list_response,
                        )),
                        credential_update: credential_update.clone(),
                        request_meta,
                    });
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
