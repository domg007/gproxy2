use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformResponse};
use serde_json::{Map, Value, json};
use wreq::{Client as WreqClient, Method as WreqMethod, Response as WreqResponse};

use super::constants::geminicli_user_agent;
use super::oauth::{
    GeminiCliRefreshedToken, geminicli_auth_material_from_credential,
    resolve_geminicli_access_token,
};
use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode, retry_with_eligible_credentials,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamRequestMeta, UpstreamResponse,
    add_or_replace_header, extra_headers_from_payload_value, extra_headers_from_transform_request,
    merge_extra_headers,
};
use crate::channels::utils::{
    is_auth_failure, is_transient_server_failure, join_base_url_and_path, retry_after_to_millis,
    to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

type ParsedGeminiPayload = (Option<String>, Option<Value>, Option<String>);

#[derive(Debug, Clone)]
enum GeminiCliRequestKind {
    LocalModelList {
        page_size: Option<u32>,
        page_token: Option<String>,
    },
    LocalModelGet {
        target: String,
    },
    Forward {
        requires_project: bool,
    },
}

#[derive(Debug, Clone)]
struct GeminiCliPreparedRequest {
    method: WreqMethod,
    path: String,
    query: Option<String>,
    body: Option<Value>,
    model: Option<String>,
    kind: GeminiCliRequestKind,
    extra_headers: Vec<(String, String)>,
}

pub async fn execute_geminicli_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = GeminiCliPreparedRequest::from_transform_request(request)?;
    let affinity_body_template = prepared
        .body
        .as_ref()
        .and_then(|body| serde_json::to_vec(body).ok());
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        crate::channels::retry::cache_affinity_protocol_from_transform_request(request).and_then(
            |protocol| {
                cache_affinity_hint_from_transform_request(
                    protocol,
                    prepared.model.as_deref(),
                    affinity_body_template.as_deref(),
                )
            },
        )
    } else {
        None
    };
    execute_geminicli_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_geminicli_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = GeminiCliPreparedRequest::from_payload(operation, protocol, body)?;
    execute_geminicli_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        None,
    )
    .await
}

async fn execute_geminicli_with_prepared(
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
                    method.clone(),
                    url.as_str(),
                    resolved_access_token.access_token.as_str(),
                    user_agent.as_deref(),
                    model.as_deref(),
                    extra_headers.as_slice(),
                    body_bytes.as_deref(),
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
                        method,
                        url.as_str(),
                        refreshed_access_token.access_token.as_str(),
                        user_agent.as_deref(),
                        model.as_deref(),
                        extra_headers.as_slice(),
                        body_bytes.as_deref(),
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

async fn try_usage_model_response(
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
                    WreqMethod::POST,
                    usage_url.as_str(),
                    resolved_access_token.access_token.as_str(),
                    user_agent.as_deref(),
                    None,
                    &[],
                    Some(usage_body_bytes.as_slice()),
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

fn geminicli_credential_update(
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

async fn send_geminicli_request(
    client: &WreqClient,
    method: WreqMethod,
    url: &str,
    access_token: &str,
    custom_user_agent: Option<&str>,
    model_for_ua: Option<&str>,
    extra_headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<(WreqResponse, UpstreamRequestMeta), wreq::Error> {
    let user_agent = custom_user_agent
        .map(str::trim)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| geminicli_user_agent(model_for_ua));
    let mut headers = Vec::new();
    merge_extra_headers(&mut headers, extra_headers);
    add_or_replace_header(&mut headers, "accept", "application/json");
    add_or_replace_header(&mut headers, "authorization", format!("Bearer {access_token}"));
    add_or_replace_header(&mut headers, "user-agent", user_agent);
    add_or_replace_header(&mut headers, "accept-encoding", "gzip");
    if body.is_some() {
        add_or_replace_header(&mut headers, "content-type", "application/json");
    }
    crate::channels::upstream::tracked_send_request(
        client,
        method,
        url,
        headers,
        body.map(|value| value.to_vec()),
    )
    .await
}

impl GeminiCliPreparedRequest {
    fn from_transform_request(request: &TransformRequest) -> Result<Self, UpstreamError> {
        let extra_headers = extra_headers_from_transform_request(request);
        let mut prepared = match request {
            TransformRequest::ModelListGemini(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: String::new(),
                query: None,
                body: None,
                model: None,
                kind: GeminiCliRequestKind::LocalModelList {
                    page_size: value.query.page_size,
                    page_token: value.query.page_token.clone(),
                },
                extra_headers: Vec::new(),
            }),
            TransformRequest::ModelGetGemini(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: String::new(),
                query: None,
                body: None,
                model: Some(normalize_model_id(value.path.name.as_str())),
                kind: GeminiCliRequestKind::LocalModelGet {
                    target: normalize_model_name(value.path.name.as_str()),
                },
                extra_headers: Vec::new(),
            }),
            TransformRequest::CountTokenGemini(value) => {
                let model = normalize_model_id(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1internal:countTokens".to_string(),
                    query: None,
                    body: Some(geminicli_count_tokens_request(model.as_str(), &value.body)?),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: false,
                    },
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::GenerateContentGemini(value) => {
                let model = normalize_model_id(value.path.model.as_str());
                let mut request_body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                strip_geminicli_unsupported_generation_config(&mut request_body);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1internal:generateContent".to_string(),
                    query: None,
                    body: Some(request_body),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::StreamGenerateContentGeminiSse(value) => {
                let model = normalize_model_id(value.path.model.as_str());
                let mut request_body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                strip_geminicli_unsupported_generation_config(&mut request_body);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1internal:streamGenerateContent".to_string(),
                    query: Some("alt=sse".to_string()),
                    body: Some(request_body),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
                let model = normalize_model_id(value.path.model.as_str());
                let mut request_body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                strip_geminicli_unsupported_generation_config(&mut request_body);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1internal:streamGenerateContent".to_string(),
                    query: Some("alt=sse".to_string()),
                    body: Some(request_body),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::EmbeddingGemini(value) => {
                let model = normalize_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:embedContent"),
                    query: None,
                    body: Some(
                        serde_json::to_value(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(normalize_model_id(value.path.model.as_str())),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: false,
                    },
                    extra_headers: Vec::new(),
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }?;
        prepared.extra_headers = extra_headers;
        Ok(prepared)
    }

    fn from_payload(
        operation: OperationFamily,
        protocol: ProtocolKind,
        body: &[u8],
    ) -> Result<Self, UpstreamError> {
        fn parse_gemini_payload_wrapper(
            value: &Value,
        ) -> Result<ParsedGeminiPayload, UpstreamError> {
            let model = value
                .pointer("/path/model")
                .or_else(|| value.pointer("/path/name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let body_value = value.get("body").cloned();
            let alt = value
                .pointer("/query/alt")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            Ok((model, body_value, alt))
        }

        let payload_value = serde_json::from_slice::<Value>(body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let extra_headers = extra_headers_from_payload_value(&payload_value);

        match (operation, protocol) {
            (OperationFamily::ModelList, ProtocolKind::Gemini) => {
                let page_size = payload_value
                    .pointer("/query/page_size")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32);
                let page_token = payload_value
                    .pointer("/query/page_token")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                Ok(Self {
                    method: WreqMethod::GET,
                    path: String::new(),
                    query: None,
                    body: None,
                    model: None,
                    kind: GeminiCliRequestKind::LocalModelList {
                        page_size,
                        page_token,
                    },
                    extra_headers,
                })
            }
            (OperationFamily::ModelGet, ProtocolKind::Gemini) => {
                let Some(target) = payload_value
                    .pointer("/path/name")
                    .or_else(|| payload_value.pointer("/path/model"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.name in geminicli model_get payload".to_string(),
                    ));
                };
                Ok(Self {
                    method: WreqMethod::GET,
                    path: String::new(),
                    query: None,
                    body: None,
                    model: Some(normalize_model_id(target.as_str())),
                    kind: GeminiCliRequestKind::LocalModelGet {
                        target: normalize_model_name(target.as_str()),
                    },
                    extra_headers,
                })
            }
            (OperationFamily::CountToken, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(&payload_value)?;
                let Some(model) = model else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in geminicli count_tokens payload".to_string(),
                    ));
                };
                let Some(body_value) = body_value else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing body in geminicli count_tokens payload".to_string(),
                    ));
                };
                let model = normalize_model_id(model.as_str());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1internal:countTokens".to_string(),
                    query: None,
                    body: Some(geminicli_count_tokens_request(model.as_str(), &body_value)?),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: false,
                    },
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(&payload_value)?;
                let Some(model) = model else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in geminicli generate payload".to_string(),
                    ));
                };
                let Some(mut body_value) = body_value else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing body in geminicli generate payload".to_string(),
                    ));
                };
                strip_geminicli_unsupported_generation_config(&mut body_value);
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1internal:generateContent".to_string(),
                    query: None,
                    body: Some(body_value),
                    model: Some(normalize_model_id(model.as_str())),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers,
                })
            }
            (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
                let (model, body_value, alt) = parse_gemini_payload_wrapper(&payload_value)?;
                let Some(model) = model else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in geminicli stream payload".to_string(),
                    ));
                };
                let Some(mut body_value) = body_value else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing body in geminicli stream payload".to_string(),
                    ));
                };
                strip_geminicli_unsupported_generation_config(&mut body_value);
                let query = Some(format!("alt={}", alt.unwrap_or_else(|| "sse".to_string())));
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1internal:streamGenerateContent".to_string(),
                    query,
                    body: Some(body_value),
                    model: Some(normalize_model_id(model.as_str())),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers,
                })
            }
            (OperationFamily::Embedding, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(&payload_value)?;
                let Some(model) = model else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in geminicli embedding payload".to_string(),
                    ));
                };
                let Some(body_value) = body_value else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing body in geminicli embedding payload".to_string(),
                    ));
                };
                let model_name = normalize_model_name(model.as_str());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: format!("/v1beta/{model_name}:embedContent"),
                    query: None,
                    body: Some(body_value),
                    model: Some(normalize_model_id(model.as_str())),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: false,
                    },
                    extra_headers,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn build_request_body_bytes(
    body: Option<&Value>,
    model: Option<&str>,
    kind: &GeminiCliRequestKind,
    project_id: &str,
) -> Result<Option<Vec<u8>>, UpstreamError> {
    match kind {
        GeminiCliRequestKind::Forward { requires_project } if *requires_project => {
            let Some(model) = model else {
                return Err(UpstreamError::SerializeRequest(
                    "missing model for geminicli generate request".to_string(),
                ));
            };
            let project_id = project_id.trim();
            if project_id.is_empty() {
                return Err(UpstreamError::SerializeRequest(
                    "missing project_id in geminicli credential".to_string(),
                ));
            }
            let Some(request) = body else {
                return Err(UpstreamError::SerializeRequest(
                    "missing request body for geminicli generate request".to_string(),
                ));
            };
            let wrapped = wrap_internal_request(model, project_id, request);
            Ok(Some(serde_json::to_vec(&wrapped).map_err(|err| {
                UpstreamError::SerializeRequest(err.to_string())
            })?))
        }
        _ => {
            let Some(body) = body else {
                return Ok(None);
            };
            Ok(Some(serde_json::to_vec(body).map_err(|err| {
                UpstreamError::SerializeRequest(err.to_string())
            })?))
        }
    }
}

fn wrap_internal_request(model: &str, project_id: &str, request: &Value) -> Value {
    json!({
        "model": model,
        "project": project_id,
        "user_prompt_id": generate_user_prompt_id(),
        "request": request,
    })
}

fn strip_geminicli_unsupported_generation_config(body: &mut Value) {
    let Some(generation_config) = body
        .get_mut("generationConfig")
        .and_then(Value::as_object_mut)
    else {
        return;
    };

    generation_config.remove("logprobs");
    generation_config.remove("responseLogprobs");
}

fn generate_user_prompt_id() -> String {
    let bytes = rand::random::<[u8; 16]>();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn geminicli_count_tokens_request(
    model: &str,
    body: &impl serde::Serialize,
) -> Result<Value, UpstreamError> {
    let body = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let mut request = Map::new();
    request.insert(
        "model".to_string(),
        Value::String(format!("models/{model}")),
    );

    if let Some(contents) = body.get("contents") {
        request.insert("contents".to_string(), contents.clone());
    } else if let Some(generate) = body.get("generateContentRequest") {
        if let Some(contents) = generate.get("contents") {
            request.insert("contents".to_string(), contents.clone());
        }
        if let Some(value) = generate.get("tools") {
            request.insert("tools".to_string(), value.clone());
        }
        if let Some(value) = generate.get("toolConfig") {
            request.insert("toolConfig".to_string(), value.clone());
        }
        if let Some(value) = generate.get("safetySettings") {
            request.insert("safetySettings".to_string(), value.clone());
        }
        if let Some(value) = generate.get("systemInstruction") {
            request.insert("systemInstruction".to_string(), value.clone());
        }
        if let Some(value) = generate.get("generationConfig") {
            request.insert("generationConfig".to_string(), value.clone());
        }
        if let Some(value) = generate.get("cachedContent") {
            request.insert("cachedContent".to_string(), value.clone());
        }
    }

    Ok(json!({ "request": request }))
}

fn usage_models_from_quota_payload(payload: &Value) -> Result<Vec<Value>, UpstreamError> {
    let buckets = payload
        .get("buckets")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            UpstreamError::SerializeRequest(
                "geminicli retrieveUserQuota payload missing buckets array".to_string(),
            )
        })?;
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for bucket in buckets {
        if let Some(token_type) = bucket.get("tokenType").and_then(Value::as_str)
            && token_type != "REQUESTS"
        {
            continue;
        }
        let Some(model_id_raw) = bucket.get("modelId").and_then(Value::as_str) else {
            continue;
        };
        let model_id = model_id_raw.trim().to_string();
        if model_id.is_empty() || !seen.insert(model_id.clone()) {
            continue;
        }
        let model_name = if model_id.starts_with("models/") {
            model_id.clone()
        } else {
            format!("models/{model_id}")
        };
        models.push(json!({
            "name": model_name,
            "baseModelId": model_id,
            "displayName": model_id,
            "description": "Derived from Gemini CLI retrieveUserQuota buckets.",
            "supportedGenerationMethods": [
                "generateContent",
                "streamGenerateContent",
                "countTokens"
            ]
        }));
    }
    Ok(models)
}

fn normalize_model_name(model: &str) -> String {
    let model = model.trim().trim_start_matches('/');
    if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    }
}

fn normalize_model_id(model: &str) -> String {
    normalize_model_name(model)
        .trim_start_matches("models/")
        .to_string()
}

pub fn normalize_geminicli_upstream_response_body(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let response = value.get("response")?;
    serde_json::to_vec(response).ok()
}

pub fn normalize_geminicli_upstream_stream_ndjson_chunk(chunk: &[u8]) -> Option<Vec<u8>> {
    normalize_wrapped_response_ndjson_chunk(chunk)
}

fn normalize_wrapped_response_ndjson_chunk(chunk: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(chunk).ok()?;
    let mut out = String::with_capacity(text.len());
    let mut changed = false;

    for segment in text.split_inclusive('\n') {
        let has_newline = segment.ends_with('\n');
        let line = segment.trim_end_matches('\n').trim_end_matches('\r');
        if line.is_empty() {
            out.push_str(segment);
            continue;
        }

        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(_) => {
                out.push_str(segment);
                continue;
            }
        };

        if let Some(response) = value.get("response") {
            let normalized = match serde_json::to_string(response) {
                Ok(value) => value,
                Err(_) => {
                    out.push_str(segment);
                    continue;
                }
            };
            out.push_str(normalized.as_str());
            if has_newline {
                out.push('\n');
            }
            changed = true;
        } else {
            out.push_str(segment);
        }
    }

    changed.then(|| out.into_bytes())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::strip_geminicli_unsupported_generation_config;

    #[test]
    fn strip_geminicli_unsupported_generation_config_removes_logprobs() {
        let mut body = json!({
            "contents": [{"role":"user","parts":[{"text":"hello"}]}],
            "generationConfig": {
                "temperature": 1,
                "logprobs": 5,
                "responseLogprobs": true
            }
        });

        strip_geminicli_unsupported_generation_config(&mut body);

        assert_eq!(
            body.pointer("/generationConfig/temperature")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        assert!(body.pointer("/generationConfig/logprobs").is_none());
        assert!(body.pointer("/generationConfig/responseLogprobs").is_none());
    }
}
