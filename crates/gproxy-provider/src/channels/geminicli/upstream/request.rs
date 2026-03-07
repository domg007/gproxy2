use super::*;

pub(super) async fn execute_geminicli_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: GeminiCliPreparedRequest,
    now_unix_ms: u64,
    cache_affinity_hint: Option<crate::channels::retry::CacheAffinityHint>,
) -> Result<UpstreamResponse, UpstreamError> {
    if let Some(upstream_response) =
        try_usage_model_response(client, provider, credential_states, &prepared, now_unix_ms)
            .await?
    {
        return Ok(upstream_response);
    }

    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }

    let state_manager = CredentialStateManager::new(now_unix_ms);
    let method_template = prepared.method.clone();
    let path_template = prepared.path.clone();
    let query_template = prepared.query.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let kind_template = prepared.kind.clone();
    let extra_headers_template = prepared.extra_headers.clone();
    let base_url_template = base_url.to_string();
    let user_agent_template = provider
        .settings
        .user_agent()
        .map(str::trim)
        .map(ToOwned::to_owned);
    let pick_mode =
        credential_pick_mode(provider.credential_pick_mode, cache_affinity_hint.as_ref());

    retry_with_eligible_credentials_with_affinity(
        provider,
        credential_states,
        prepared.model.as_deref(),
        now_unix_ms,
        pick_mode,
        cache_affinity_hint,
        |credential| {
            if let ChannelCredential::Builtin(BuiltinChannelCredential::GeminiCli(value)) =
                &credential.credential
            {
                return geminicli_auth_material_from_credential(value);
            }
            None
        },
        |attempt| {
            let method = method_template.clone();
            let path = path_template.clone();
            let query = query_template.clone();
            let body = body_template.clone();
            let model = model_template.clone();
            let kind = kind_template.clone();
            let extra_headers = extra_headers_template.clone();
            let base_url = base_url_template.clone();
            let user_agent = user_agent_template.clone();

            async move {
                let path_with_query = match query.as_deref() {
                    Some(query) if !query.is_empty() => format!("{path}?{query}"),
                    _ => path.clone(),
                };
                let url = join_base_url_and_path(base_url.as_str(), path_with_query.as_str());
                let token_cache_key =
                    format!("{}::{}", provider.channel.as_str(), attempt.credential_id);
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
                if let Some(refreshed) = resolved_access_token.refreshed.as_ref() {
                    credential_update = Some(geminicli_credential_update(
                        attempt.credential_id,
                        refreshed,
                    ));
                }

                let body_bytes = match build_request_body_bytes(
                    body.as_ref(),
                    model.as_deref(),
                    &kind,
                    attempt.material.project_id.as_str(),
                ) {
                    Ok(body) => body,
                    Err(err) => {
                        let message = err.to_string();
                        state_manager.mark_auth_dead(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                            Some(message.clone()),
                        );
                        return CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(message),
                            last_request_meta: None,
                        };
                    }
                };
                let (mut response, mut request_meta) = match send_geminicli_request(
                    client,
                    GeminiCliRequestParams {
                        method: method.clone(),
                        url: url.as_str(),
                        access_token: resolved_access_token.access_token.as_str(),
                        custom_user_agent: user_agent.as_deref(),
                        model_for_ua: model.as_deref(),
                        extra_headers: extra_headers.as_slice(),
                        body: body_bytes.as_deref(),
                    },
                )
                .await
                {
                    Ok((response, request_meta)) => (response, request_meta),
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
                    let refreshed_access_token = match resolve_geminicli_access_token(
                        client,
                        &provider.settings,
                        token_cache_key.as_str(),
                        &attempt.material,
                        now_unix_ms,
                        true,
                    )
                    .await
                    {
                        Ok(token) => token,
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
                                last_request_meta: None,
                            };
                        }
                    };
                    if let Some(refreshed) = refreshed_access_token.refreshed.as_ref() {
                        credential_update = Some(geminicli_credential_update(
                            attempt.credential_id,
                            refreshed,
                        ));
                    }
                    (response, request_meta) = match send_geminicli_request(
                        client,
                        GeminiCliRequestParams {
                            method,
                            url: url.as_str(),
                            access_token: refreshed_access_token.access_token.as_str(),
                            custom_user_agent: user_agent.as_deref(),
                            model_for_ua: model.as_deref(),
                            extra_headers: extra_headers.as_slice(),
                            body: body_bytes.as_deref(),
                        },
                    )
                    .await
                    {
                        Ok((response, request_meta)) => (response, request_meta),
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

                    status_code = response.status().as_u16();
                    if is_auth_failure(status_code) {
                        let message = format!(
                            "upstream status {} after geminicli access token refresh",
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
                            last_request_meta: None,
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
                        last_request_meta: None,
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
                        last_request_meta: None,
                    };
                }

                if response.status().is_success() {
                    state_manager.mark_success(
                        credential_states,
                        &provider.channel,
                        attempt.credential_id,
                    );
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

pub(super) async fn try_usage_model_response(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: &GeminiCliPreparedRequest,
    now_unix_ms: u64,
) -> Result<Option<UpstreamResponse>, UpstreamError> {
    match &prepared.kind {
        GeminiCliRequestKind::LocalModelList {
            page_size,
            page_token,
        } => {
            let usage = execute_geminicli_upstream_usage_with_retry(
                client,
                provider,
                credential_states,
                None,
                now_unix_ms,
            )
            .await?;
            let credential_update = usage.credential_update.clone();
            let payload = usage.into_http_payload().await?;
            if payload.status_code >= 400 {
                return Err(UpstreamError::UpstreamRequest(format!(
                    "retrieveUserQuota returned status {}",
                    payload.status_code
                )));
            }
            let usage_json = serde_json::from_slice::<Value>(&payload.body)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            let models = usage_models_from_quota_payload(&usage_json)?;

            let start = page_token
                .as_deref()
                .and_then(|token| token.parse::<usize>().ok())
                .unwrap_or(0)
                .min(models.len());
            let size = page_size
                .map(|value| value.max(1) as usize)
                .unwrap_or(models.len().saturating_sub(start));
            let end = start.saturating_add(size).min(models.len());
            let next_page_token = (end < models.len()).then(|| end.to_string());

            let response_json = json!({
                "stats_code": 200,
                "headers": {},
                "body": {
                    "models": models[start..end].to_vec(),
                    "nextPageToken": next_page_token,
                }
            });
            let response = serde_json::from_value(response_json)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(Some(
                UpstreamResponse::from_local(TransformResponse::ModelListGemini(response))
                    .with_credential_update(credential_update),
            ))
        }
        GeminiCliRequestKind::LocalModelGet { target } => {
            let usage = execute_geminicli_upstream_usage_with_retry(
                client,
                provider,
                credential_states,
                None,
                now_unix_ms,
            )
            .await?;
            let credential_update = usage.credential_update.clone();
            let payload = usage.into_http_payload().await?;
            if payload.status_code >= 400 {
                return Err(UpstreamError::UpstreamRequest(format!(
                    "retrieveUserQuota returned status {}",
                    payload.status_code
                )));
            }
            let usage_json = serde_json::from_slice::<Value>(&payload.body)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            let models = usage_models_from_quota_payload(&usage_json)?;
            let found = models.iter().find(|item| {
                item.get("name")
                    .and_then(Value::as_str)
                    .map(|name| normalize_model_name(name) == normalize_model_name(target))
                    .unwrap_or(false)
            });

            let response_json = if let Some(model) = found {
                json!({
                    "stats_code": 200,
                    "headers": {},
                    "body": model,
                })
            } else {
                json!({
                    "stats_code": 404,
                    "headers": {},
                    "body": {
                        "error": {
                            "code": 404,
                            "message": format!("model {} not found", target),
                            "status": "NOT_FOUND",
                        }
                    }
                })
            };
            let response = serde_json::from_value(response_json)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(Some(
                UpstreamResponse::from_local(TransformResponse::ModelGetGemini(response))
                    .with_credential_update(credential_update),
            ))
        }
        GeminiCliRequestKind::Forward { .. } => Ok(None),
    }
}
