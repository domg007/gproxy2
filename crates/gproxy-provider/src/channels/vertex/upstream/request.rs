use super::*;

pub(super) async fn execute_vertex_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: VertexPreparedRequest,
    cache_protocol: Option<CacheAffinityProtocol>,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }

    let state_manager = CredentialStateManager::new(now_unix_ms);
    let method_template = prepared.method.clone();
    let endpoint_template = prepared.endpoint.clone();
    let query_template = prepared.query.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let model_response_kind_template = prepared.model_response_kind;
    let extra_headers_template = prepared.extra_headers.clone();
    let base_url_template = base_url.to_string();
    let location_template = DEFAULT_LOCATION.to_string();
    let user_agent_template =
        resolve_user_agent_or_else(provider.settings.user_agent(), default_gproxy_user_agent);
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        cache_protocol.and_then(|protocol| {
            cache_affinity_hint_from_transform_request(
                protocol,
                prepared.model.as_deref(),
                prepared.body.as_deref(),
            )
        })
    } else {
        None
    };
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
        |credential| {
            if let ChannelCredential::Builtin(BuiltinChannelCredential::Vertex(value)) =
                &credential.credential
            {
                return vertex_auth_material_from_credential(value, &provider.settings);
            }
            None
        },
        |attempt| {
            let method = method_template.clone();
            let endpoint = endpoint_template.clone();
            let query = query_template.clone();
            let body = body_template.clone();
            let model = model_template.clone();
            let model_response_kind = model_response_kind_template;
            let extra_headers = extra_headers_template.clone();
            let base_url = base_url_template.clone();
            let location = location_template.clone();
            let user_agent = user_agent_template.clone();

            async move {
                let path = build_vertex_path(
                    endpoint,
                    attempt.material.project_id.as_str(),
                    location.as_str(),
                );
                let path_with_query = match query.as_deref() {
                    Some(query) if !query.is_empty() => format!("{path}?{query}"),
                    _ => path,
                };
                let url = join_base_url_and_path(base_url.as_str(), path_with_query.as_str());
                let token_cache_key =
                    format!("{}::{}", provider.channel.as_str(), attempt.credential_id);
                let mut credential_update: Option<UpstreamCredentialUpdate> = None;

                let resolved_access_token = match resolve_vertex_access_token(
                    client,
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
                    credential_update = Some(vertex_credential_update(
                        attempt.credential_id,
                        refreshed.access_token.as_str(),
                        refreshed.expires_at_unix_ms,
                    ));
                } else if attempt.material.access_token != resolved_access_token.access_token
                    || attempt.material.expires_at_unix_ms
                        != resolved_access_token.expires_at_unix_ms
                {
                    credential_update = Some(vertex_credential_update(
                        attempt.credential_id,
                        resolved_access_token.access_token.as_str(),
                        resolved_access_token.expires_at_unix_ms,
                    ));
                }
                let (mut response, mut request_meta) = match send_vertex_request(
                    client,
                    &method,
                    url.as_str(),
                    resolved_access_token.access_token.as_str(),
                    user_agent.as_str(),
                    extra_headers.as_slice(),
                    &body,
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

                if response.status().is_success() {
                    if let Some(kind) = model_response_kind {
                        let local = match normalize_vertex_model_response(response, kind).await {
                            Ok(local) => local,
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
                        state_manager.mark_success(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                        );
                        return CredentialRetryDecision::Return(
                            UpstreamResponse::from_local(local)
                                .with_request_meta(request_meta.clone())
                                .with_credential_update(credential_update.clone()),
                        );
                    }
                    state_manager.mark_success(
                        credential_states,
                        &provider.channel,
                        attempt.credential_id,
                    );
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

                if is_auth_failure(response.status().as_u16()) {
                    let refreshed_token = match resolve_vertex_access_token(
                        client,
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
                                last_status: Some(401),
                                last_error: Some(message),
                                last_request_meta: None,
                            };
                        }
                    };
                    if let Some(refreshed) = refreshed_token.refreshed.as_ref() {
                        credential_update = Some(vertex_credential_update(
                            attempt.credential_id,
                            refreshed.access_token.as_str(),
                            refreshed.expires_at_unix_ms,
                        ));
                    } else if attempt.material.access_token != refreshed_token.access_token
                        || attempt.material.expires_at_unix_ms != refreshed_token.expires_at_unix_ms
                    {
                        credential_update = Some(vertex_credential_update(
                            attempt.credential_id,
                            refreshed_token.access_token.as_str(),
                            refreshed_token.expires_at_unix_ms,
                        ));
                    }
                    (response, request_meta) = match send_vertex_request(
                        client,
                        &method,
                        url.as_str(),
                        refreshed_token.access_token.as_str(),
                        user_agent.as_str(),
                        extra_headers.as_slice(),
                        &body,
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

                    if response.status().is_success() {
                        if let Some(kind) = model_response_kind {
                            let local = match normalize_vertex_model_response(response, kind).await
                            {
                                Ok(local) => local,
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
                            state_manager.mark_success(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                            );
                            return CredentialRetryDecision::Return(
                                UpstreamResponse::from_local(local)
                                    .with_request_meta(request_meta.clone())
                                    .with_credential_update(credential_update.clone()),
                            );
                        }
                        state_manager.mark_success(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                        );
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

                    if is_auth_failure(response.status().as_u16()) {
                        let message = format!(
                            "upstream status {} after access token refresh",
                            response.status().as_u16()
                        );
                        state_manager.mark_auth_dead(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                            Some(message.clone()),
                        );
                        return CredentialRetryDecision::Retry {
                            last_status: Some(response.status().as_u16()),
                            last_error: Some(message),
                            last_request_meta: None,
                        };
                    }
                }

                let status_code = response.status().as_u16();
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

pub(super) fn cache_affinity_protocol_from_operation_protocol(
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> Option<CacheAffinityProtocol> {
    match (operation, protocol) {
        (OperationFamily::GenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
            Some(CacheAffinityProtocol::GeminiGenerateContent)
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
            Some(CacheAffinityProtocol::OpenAiChatCompletions)
        }
        _ => None,
    }
}

pub(super) fn vertex_credential_update(
    credential_id: i64,
    access_token: &str,
    expires_at_unix_ms: u64,
) -> UpstreamCredentialUpdate {
    UpstreamCredentialUpdate::VertexTokenRefresh {
        credential_id,
        access_token: access_token.to_string(),
        expires_at_unix_ms,
    }
}

pub(super) async fn send_vertex_request(
    client: &WreqClient,
    method: &WreqMethod,
    url: &str,
    access_token: &str,
    user_agent: &str,
    extra_headers: &[(String, String)],
    body: &Option<Vec<u8>>,
) -> Result<(WreqResponse, UpstreamRequestMeta), wreq::Error> {
    let mut headers = Vec::new();
    merge_extra_headers(&mut headers, extra_headers);
    add_or_replace_header(
        &mut headers,
        "authorization",
        format!("Bearer {access_token}"),
    );
    add_or_replace_header(&mut headers, "user-agent", user_agent.to_string());
    if body.is_some() {
        add_or_replace_header(&mut headers, "content-type", "application/json");
    }
    crate::channels::upstream::tracked_send_request(
        client,
        method.clone(),
        url,
        headers,
        body.as_ref().cloned(),
    )
    .await
}

#[derive(Debug, Clone)]
pub(super) enum VertexEndpoint {
    Global(String),
    Project(String),
}

#[derive(Debug, Clone, Copy)]
pub(super) enum VertexModelResponseKind {
    List,
    Get,
    Embedding,
}

#[derive(Debug, Clone)]
pub(super) struct VertexPreparedRequest {
    pub(super) method: WreqMethod,
    pub(super) endpoint: VertexEndpoint,
    pub(super) query: Option<String>,
    pub(super) body: Option<Vec<u8>>,
    pub(super) model: Option<String>,
    pub(super) model_response_kind: Option<VertexModelResponseKind>,
    pub(super) extra_headers: Vec<(String, String)>,
}
