use gproxy_middleware::{OperationFamily, ProtocolKind, TransformResponse};
use serde_json::{Value, json};
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    CLAUDE_CODE_UA, CLAUDECODE_DEFAULT_BETAS, DEFAULT_CLAUDE_AI_BASE_URL,
    DEFAULT_PLATFORM_BASE_URL, OAUTH_BETA,
};
use super::oauth::{
    ClaudeCodeRefreshedToken, claudecode_access_token_from_credential,
    resolve_claudecode_access_token,
};
use crate::channels::cache_control::{
    CacheBreakpointRule, apply_magic_string_cache_control_triggers, ensure_cache_breakpoint_rules,
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
    anthropic_header_pairs, append_query_param_if_missing, claude_model_list_query_string,
    claude_model_to_string, is_auth_failure, is_transient_server_failure, join_base_url_and_path,
    retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, RetryWithPayloadRequest};

const BETA_QUERY_KEY: &str = "beta";
const BETA_QUERY_VALUE: &str = "true";
const CLAUDECODE_THINKING_MODEL_SUFFIX: &str = "-thinking";
const CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX: &str = "-adaptive-thinking";
const CLAUDECODE_THINKING_BUDGET_TOKENS: u64 = 4_096;

pub async fn execute_claudecode_with_retry(
    client: &WreqClient,
    spoof_client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &gproxy_middleware::TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prelude_text = provider
        .settings
        .claudecode_prelude_text()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let prepared = ClaudeCodePreparedRequest::from_transform_request(
        request,
        prelude_text,
        provider.settings.cache_breakpoints(),
    )?;
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        crate::channels::retry::cache_affinity_protocol_from_transform_request(request).and_then(
            |protocol| {
                cache_affinity_hint_from_transform_request(
                    protocol,
                    prepared.model.as_deref(),
                    prepared.body.as_deref(),
                )
            },
        )
    } else {
        None
    };
    execute_claudecode_with_prepared(
        client,
        spoof_client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_claudecode_payload_with_retry(
    client: &WreqClient,
    spoof_client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    payload: RetryWithPayloadRequest<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    let prelude_text = provider
        .settings
        .claudecode_prelude_text()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let prepared = ClaudeCodePreparedRequest::from_payload(
        payload.operation,
        payload.protocol,
        payload.body,
        prelude_text,
        provider.settings.cache_breakpoints(),
    )?;
    execute_claudecode_with_prepared(
        client,
        spoof_client,
        provider,
        credential_states,
        prepared,
        payload.now_unix_ms,
        None,
    )
    .await
}

async fn execute_claudecode_with_prepared(
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
    let context_1m_target_template = prepared.context_1m_target.clone();
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
        provider,
        credential_states,
        prepared.model.as_deref(),
        now_unix_ms,
        pick_mode,
        cache_affinity_hint,
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
            let context_1m_target = context_1m_target_template.clone();
            let url = url_template.clone();
            let base_url = base_url_template.clone();
            let claude_ai_base_url = claude_ai_base_url_template.clone();
            let request_user_agent = request_user_agent_template.clone();

            async move {
                let active_client = if attempt.material.has_cookie() {
                    spoof_client
                } else {
                    client
                };
                let cache_key = format!("{}::{}", provider.channel.as_str(), attempt.credential_id);
                let mut credential_update = None;
                let context_1m_enabled = claudecode_1m_enabled_for_credential(
                    provider,
                    attempt.credential_id,
                    context_1m_target.as_ref(),
                );
                if !context_1m_enabled {
                    strip_context_1m_beta(&mut request_headers);
                }
                let sent_with_context_1m = has_context_1m_beta(request_headers.as_slice());

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
                    method.clone(),
                    url.as_str(),
                    active_access_token.as_str(),
                    request_user_agent.as_str(),
                    extra_headers.as_slice(),
                    request_headers.as_slice(),
                    body.as_ref(),
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
                        method.clone(),
                        url.as_str(),
                        active_access_token.as_str(),
                        request_user_agent.as_str(),
                        extra_headers.as_slice(),
                        request_headers.as_slice(),
                        body.as_ref(),
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

                if sent_with_context_1m && context_1m_enabled && status_code >= 400 {
                    let mut retry_headers_without_context = request_headers.clone();
                    strip_context_1m_beta(&mut retry_headers_without_context);
                    let (retry_response, retry_meta) = match send_claudecode_request(
                        active_client,
                        method.clone(),
                        url.as_str(),
                        active_access_token.as_str(),
                        request_user_agent.as_str(),
                        extra_headers.as_slice(),
                        retry_headers_without_context.as_slice(),
                        body.as_ref(),
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
                    if response.status().is_success() {
                        disable_claudecode_1m_for_target(
                            &mut credential_update,
                            attempt.credential_id,
                            context_1m_target.as_ref(),
                        );
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

async fn send_claudecode_request(
    client: &WreqClient,
    method: WreqMethod,
    url: &str,
    access_token: &str,
    user_agent: &str,
    extra_headers: &[(String, String)],
    request_headers: &[(String, String)],
    body: Option<&Vec<u8>>,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let beta_values = normalized_claudecode_beta_values(
        request_headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
            .map(|(_, value)| parse_anthropic_beta_values(value))
            .unwrap_or_default(),
        true,
    );

    let mut sent_headers = Vec::new();
    merge_extra_headers(&mut sent_headers, extra_headers);
    add_or_replace_header(
        &mut sent_headers,
        "authorization",
        format!("Bearer {}", access_token),
    );
    add_or_replace_header(&mut sent_headers, "user-agent", user_agent.to_string());
    add_or_replace_header(&mut sent_headers, "anthropic-beta", beta_values.join(","));
    for (name, value) in request_headers {
        if name.eq_ignore_ascii_case("anthropic-beta") {
            continue;
        }
        add_or_replace_header(&mut sent_headers, name, value.clone());
    }
    if body.is_some() {
        add_or_replace_header(&mut sent_headers, "content-type", "application/json");
    }

    crate::channels::upstream::tracked_send_request(
        client,
        method,
        url,
        sent_headers,
        body.cloned(),
    )
    .await
}

async fn send_claudecode_usage_request(
    client: &WreqClient,
    usage_url: &str,
    access_token: &str,
    user_agent: &str,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let beta_values = normalized_claudecode_beta_values(Vec::new(), true);
    let sent_headers = vec![
        (
            "authorization".to_string(),
            format!("Bearer {}", access_token),
        ),
        ("accept".to_string(), "application/json".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
        ("user-agent".to_string(), user_agent.to_string()),
        ("anthropic-beta".to_string(), beta_values.join(",")),
    ];
    crate::channels::upstream::tracked_send_request(
        client,
        WreqMethod::GET,
        usage_url,
        sent_headers,
        None,
    )
    .await
}

#[derive(Debug, Clone)]
enum ClaudeCode1mTarget {
    Sonnet,
    Opus,
}

#[derive(Debug, Clone)]
struct ClaudeCodePreparedRequest {
    method: WreqMethod,
    path: String,
    body: Option<Vec<u8>>,
    model: Option<String>,
    request_headers: Vec<(String, String)>,
    extra_headers: Vec<(String, String)>,
    context_1m_target: Option<ClaudeCode1mTarget>,
}

impl ClaudeCodePreparedRequest {
    fn from_transform_request(
        request: &gproxy_middleware::TransformRequest,
        prelude_text: Option<&str>,
        cache_breakpoints: &[CacheBreakpointRule],
    ) -> Result<Self, UpstreamError> {
        let extra_headers = extra_headers_from_transform_request(request);
        let mut prepared = match request {
            gproxy_middleware::TransformRequest::ModelListClaude(value) => {
                let mut path = "/v1/models".to_string();
                let query = claude_model_list_query_string(
                    value.query.after_id.as_deref(),
                    value.query.before_id.as_deref(),
                    value.query.limit,
                );
                if !query.is_empty() {
                    path.push('?');
                    path.push_str(&query);
                }
                path =
                    append_query_param_if_missing(path.as_str(), BETA_QUERY_KEY, BETA_QUERY_VALUE);

                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;
                ensure_oauth_beta(&mut request_headers, false);

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path,
                    body: None,
                    model: None,
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target: None,
                })
            }
            gproxy_middleware::TransformRequest::ModelGetClaude(value) => {
                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;
                ensure_oauth_beta(&mut request_headers, false);
                let model_id = value.path.model_id.trim().to_string();

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: append_query_param_if_missing(
                        format!("/v1/models/{model_id}").as_str(),
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    body: None,
                    model: Some(model_id),
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target: None,
                })
            }
            gproxy_middleware::TransformRequest::CountTokenClaude(value) => {
                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;

                let model = claude_model_to_string(&value.body.model)?;
                let mut body_json = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                if let Some(prelude_text) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude_text);
                }
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: append_query_param_if_missing(
                        "/v1/messages/count_tokens",
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target,
                })
            }
            gproxy_middleware::TransformRequest::GenerateContentClaude(value) => {
                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;

                let model = claude_model_to_string(&value.body.model)?;
                let mut body_json = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                if let Some(prelude_text) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude_text);
                }
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_sampling(model.as_str(), &mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                apply_magic_string_cache_control_triggers(&mut body_json);
                if !cache_breakpoints.is_empty() {
                    ensure_cache_breakpoint_rules(&mut body_json, cache_breakpoints);
                }
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: append_query_param_if_missing(
                        "/v1/messages",
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target,
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentClaude(value) => {
                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;

                let model = claude_model_to_string(&value.body.model)?;
                let mut body_json = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                if let Some(prelude_text) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude_text);
                }
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_sampling(model.as_str(), &mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                apply_magic_string_cache_control_triggers(&mut body_json);
                if !cache_breakpoints.is_empty() {
                    ensure_cache_breakpoint_rules(&mut body_json, cache_breakpoints);
                }
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: append_query_param_if_missing(
                        "/v1/messages",
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target,
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
        prelude_text: Option<&str>,
        cache_breakpoints: &[CacheBreakpointRule],
    ) -> Result<Self, UpstreamError> {
        fn json_pointer_string(value: &Value, pointer: &str) -> Option<String> {
            value
                .pointer(pointer)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        }

        fn parse_claude_payload_wrapper(
            value: &Value,
        ) -> Result<(Value, String, Option<Vec<String>>), UpstreamError> {
            const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

            if let Some(body_value) = value.get("body").cloned() {
                let version = value
                    .pointer("/headers/anthropic_version")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_ANTHROPIC_VERSION)
                    .to_string();
                let beta = value
                    .pointer("/headers/anthropic_beta")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .filter(|items| !items.is_empty());
                return Ok((body_value, version, beta));
            }
            Ok((value.clone(), DEFAULT_ANTHROPIC_VERSION.to_string(), None))
        }

        let payload_value = serde_json::from_slice::<Value>(body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let extra_headers = extra_headers_from_payload_value(&payload_value);

        match (operation, protocol) {
            (OperationFamily::ModelList, ProtocolKind::Claude) => {
                let version = payload_value
                    .pointer("/headers/anthropic_version")
                    .and_then(Value::as_str)
                    .unwrap_or("2023-06-01")
                    .to_string();
                let beta = payload_value
                    .pointer("/headers/anthropic_beta")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect::<Vec<_>>()
                    });
                let mut request_headers = anthropic_header_pairs(&version, beta.as_ref())?;
                ensure_oauth_beta(&mut request_headers, false);
                let path =
                    append_query_param_if_missing("/v1/models", BETA_QUERY_KEY, BETA_QUERY_VALUE);
                Ok(Self {
                    method: WreqMethod::GET,
                    path,
                    body: None,
                    model: None,
                    request_headers,
                    extra_headers,
                    context_1m_target: None,
                })
            }
            (OperationFamily::ModelGet, ProtocolKind::Claude) => {
                let Some(model_id) = payload_value
                    .pointer("/path/model_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model_id in claudecode model_get payload".to_string(),
                    ));
                };
                let version = payload_value
                    .pointer("/headers/anthropic_version")
                    .and_then(Value::as_str)
                    .unwrap_or("2023-06-01")
                    .to_string();
                let beta = payload_value
                    .pointer("/headers/anthropic_beta")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned)
                            .collect::<Vec<_>>()
                    });
                let mut request_headers = anthropic_header_pairs(&version, beta.as_ref())?;
                ensure_oauth_beta(&mut request_headers, false);
                Ok(Self {
                    method: WreqMethod::GET,
                    path: append_query_param_if_missing(
                        format!("/v1/models/{model_id}").as_str(),
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    body: None,
                    model: Some(model_id),
                    request_headers,
                    extra_headers,
                    context_1m_target: None,
                })
            }
            (OperationFamily::CountToken, ProtocolKind::Claude) => {
                let (mut body_json, version, beta) = parse_claude_payload_wrapper(&payload_value)?;
                if let Some(prelude) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude);
                }
                let model = json_pointer_string(&body_json, "/model").ok_or_else(|| {
                    UpstreamError::SerializeRequest(
                        "missing model in claudecode count_tokens payload".to_string(),
                    )
                })?;
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                let mut request_headers = anthropic_header_pairs(&version, beta.as_ref())?;
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: append_query_param_if_missing(
                        "/v1/messages/count_tokens",
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers,
                    context_1m_target,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::Claude)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
                let (mut body_json, version, beta) = parse_claude_payload_wrapper(&payload_value)?;
                if let Some(prelude) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude);
                }
                let model = json_pointer_string(&body_json, "/model").ok_or_else(|| {
                    UpstreamError::SerializeRequest(
                        "missing model in claudecode message payload".to_string(),
                    )
                })?;
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_sampling(model.as_str(), &mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                apply_magic_string_cache_control_triggers(&mut body_json);
                if !cache_breakpoints.is_empty() {
                    ensure_cache_breakpoint_rules(&mut body_json, cache_breakpoints);
                }
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                let mut request_headers = anthropic_header_pairs(&version, beta.as_ref())?;
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: append_query_param_if_missing(
                        "/v1/messages",
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers,
                    context_1m_target,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn ensure_oauth_beta(headers: &mut Vec<(String, String)>, allow_context_1m: bool) {
    let values = normalized_claudecode_beta_values(
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
            .map(|(_, value)| parse_anthropic_beta_values(value))
            .unwrap_or_default(),
        allow_context_1m,
    );

    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("anthropic-beta"));
    headers.push(("anthropic-beta".to_string(), values.join(",")));
}

fn is_context_1m_beta(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("context-1m")
}

fn has_context_1m_beta(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| value.split(',').any(is_context_1m_beta))
        .unwrap_or(false)
}

fn strip_context_1m_beta(headers: &mut Vec<(String, String)>) {
    let values = normalized_claudecode_beta_values(
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
            .map(|(_, value)| parse_anthropic_beta_values(value))
            .unwrap_or_default(),
        false,
    );

    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("anthropic-beta"));
    headers.push(("anthropic-beta".to_string(), values.join(",")));
}

fn parse_anthropic_beta_values(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn normalized_claudecode_beta_values(
    mut values: Vec<String>,
    allow_context_1m: bool,
) -> Vec<String> {
    if !allow_context_1m {
        values.retain(|value| !is_context_1m_beta(value));
    }

    for required in std::iter::once(OAUTH_BETA).chain(CLAUDECODE_DEFAULT_BETAS.iter().copied()) {
        if !values
            .iter()
            .any(|value| value.eq_ignore_ascii_case(required))
        {
            values.push(required.to_string());
        }
    }

    values
}

fn claude_1m_target_for_model(model: &str) -> Option<ClaudeCode1mTarget> {
    let lower = model.to_ascii_lowercase();
    if lower.starts_with("claude-sonnet-4") {
        return Some(ClaudeCode1mTarget::Sonnet);
    }
    if lower.starts_with("claude-opus-4-6") {
        return Some(ClaudeCode1mTarget::Opus);
    }
    None
}

fn apply_claudecode_system(body: &mut Value, prelude_text: &str) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    if system_has_known_claudecode_prelude(map.get("system")) {
        return;
    }

    let prelude_block = json_text_block(prelude_text);
    match map.remove("system") {
        Some(Value::String(text)) => {
            map.insert(
                "system".to_string(),
                Value::Array(vec![prelude_block, json_text_block(text.as_str())]),
            );
        }
        Some(Value::Array(mut blocks)) => {
            blocks.insert(0, prelude_block);
            map.insert("system".to_string(), Value::Array(blocks));
        }
        Some(value) => {
            map.insert("system".to_string(), value);
        }
        None => {
            map.insert("system".to_string(), Value::Array(vec![prelude_block]));
        }
    }
}

fn system_has_known_claudecode_prelude(system: Option<&Value>) -> bool {
    let Some(system) = system else {
        return false;
    };

    match system {
        Value::String(text) => is_known_claudecode_prelude_text(text),
        Value::Array(blocks) => blocks.iter().any(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(is_known_claudecode_prelude_text)
        }),
        _ => false,
    }
}

fn is_known_claudecode_prelude_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("you are claude code") || lower.contains("claude agent sdk")
}

fn json_text_block(text: &str) -> Value {
    serde_json::json!({
        "type": "text",
        "text": text,
    })
}

fn claudecode_1m_enabled_for_credential(
    provider: &ProviderDefinition,
    credential_id: i64,
    target: Option<&ClaudeCode1mTarget>,
) -> bool {
    let Some(target) = target else {
        return false;
    };
    let Some(credential) = provider.credentials.credential(credential_id) else {
        return true;
    };
    let ChannelCredential::Builtin(BuiltinChannelCredential::ClaudeCode(value)) =
        &credential.credential
    else {
        return true;
    };

    match target {
        ClaudeCode1mTarget::Sonnet => value.enable_claude_1m_sonnet.unwrap_or(true),
        ClaudeCode1mTarget::Opus => value.enable_claude_1m_opus.unwrap_or(true),
    }
}

fn disable_claudecode_1m_for_target(
    update: &mut Option<UpstreamCredentialUpdate>,
    credential_id: i64,
    target: Option<&ClaudeCode1mTarget>,
) {
    let Some(target) = target else {
        return;
    };

    let (disable_sonnet, disable_opus) = match target {
        ClaudeCode1mTarget::Sonnet => (Some(false), None),
        ClaudeCode1mTarget::Opus => (None, Some(false)),
    };

    if let Some(UpstreamCredentialUpdate::ClaudeCodeTokenRefresh {
        enable_claude_1m_sonnet,
        enable_claude_1m_opus,
        ..
    }) = update
    {
        if disable_sonnet.is_some() {
            *enable_claude_1m_sonnet = disable_sonnet;
        }
        if disable_opus.is_some() {
            *enable_claude_1m_opus = disable_opus;
        }
        return;
    }

    *update = Some(UpstreamCredentialUpdate::ClaudeCodeTokenRefresh {
        credential_id,
        access_token: None,
        refresh_token: None,
        expires_at_unix_ms: None,
        subscription_type: None,
        rate_limit_tier: None,
        user_email: None,
        cookie: None,
        enable_claude_1m_sonnet: disable_sonnet,
        enable_claude_1m_opus: disable_opus,
    });
}

fn normalize_claudecode_sampling(model: &str, body: &mut Value) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    let has_temperature = map.get("temperature").and_then(Value::as_f64).is_some();
    let has_top_p = map.get("top_p").and_then(Value::as_f64).is_some();
    if has_temperature && has_top_p && requires_claudecode_sampling_guard(model) {
        map.remove("top_p");
    }
}

fn normalize_claudecode_unsupported_fields(body: &mut Value) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    // Anthropic v1/messages on this upstream path currently rejects this field.
    map.remove("context_management");
}

fn normalize_claudecode_model_and_thinking(model: &str, body: &mut Value) -> String {
    let trimmed = model.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with(CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX) {
        let mut normalized = trimmed
            [..trimmed.len() - CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX.len()]
            .trim()
            .to_string();
        if normalized.is_empty() {
            normalized = trimmed.to_string();
        }
        let Some(map) = body.as_object_mut() else {
            return normalized;
        };
        map.insert("model".to_string(), Value::String(normalized.clone()));
        map.insert(
            "thinking".to_string(),
            serde_json::json!({
                "type": "adaptive"
            }),
        );
        return normalized;
    }

    if lower.ends_with(CLAUDECODE_THINKING_MODEL_SUFFIX) {
        let mut normalized = trimmed[..trimmed.len() - CLAUDECODE_THINKING_MODEL_SUFFIX.len()]
            .trim()
            .to_string();
        if normalized.is_empty() {
            normalized = trimmed.to_string();
        }
        let Some(map) = body.as_object_mut() else {
            return normalized;
        };
        map.insert("model".to_string(), Value::String(normalized.clone()));
        map.insert(
            "thinking".to_string(),
            serde_json::json!({
                "type": "enabled",
                "budget_tokens": CLAUDECODE_THINKING_BUDGET_TOKENS
            }),
        );
        return normalized;
    }

    trimmed.to_string()
}

fn should_expand_claudecode_model_list(
    method: &WreqMethod,
    url: &str,
    body: Option<&Vec<u8>>,
) -> bool {
    *method == WreqMethod::GET
        && body.is_none()
        && (url.contains("/v1/models?") || url.ends_with("/v1/models"))
        && !url.contains("/v1/models/")
}

fn extend_model_list_with_thinking_variants(data: &mut Vec<Value>) {
    let existing_ids = data
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<std::collections::BTreeSet<_>>();

    let mut out = Vec::with_capacity(data.len().saturating_mul(3));
    for item in data.iter() {
        out.push(item.clone());

        let Some(id) = item.get("id").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        let id_lower = id.to_ascii_lowercase();
        if id.is_empty()
            || id_lower.ends_with(CLAUDECODE_THINKING_MODEL_SUFFIX)
            || id_lower.ends_with(CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX)
        {
            continue;
        }

        let thinking_id = format!("{id}{CLAUDECODE_THINKING_MODEL_SUFFIX}");
        let adaptive_thinking_id = format!("{id}{CLAUDECODE_ADAPTIVE_THINKING_MODEL_SUFFIX}");
        for variant_id in [thinking_id, adaptive_thinking_id] {
            if existing_ids.contains(variant_id.as_str()) {
                continue;
            }

            let mut variant_item = item.clone();
            if let Some(obj) = variant_item.as_object_mut() {
                obj.insert("id".to_string(), Value::String(variant_id));
                out.push(variant_item);
            }
        }
    }

    *data = out;
}

fn requires_claudecode_sampling_guard(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("opus-4-1")
        || lower.contains("opus-4-5")
        || lower.contains("opus-4-6")
        || lower.contains("sonnet-4-5")
}

fn claudecode_credential_update(
    credential_id: i64,
    refreshed: &ClaudeCodeRefreshedToken,
) -> UpstreamCredentialUpdate {
    UpstreamCredentialUpdate::ClaudeCodeTokenRefresh {
        credential_id,
        access_token: Some(refreshed.access_token.clone()),
        refresh_token: Some(refreshed.refresh_token.clone()),
        expires_at_unix_ms: Some(refreshed.expires_at_unix_ms),
        subscription_type: refreshed.subscription_type.clone(),
        rate_limit_tier: refreshed.rate_limit_tier.clone(),
        user_email: refreshed.user_email.clone(),
        cookie: refreshed.cookie.clone(),
        enable_claude_1m_sonnet: None,
        enable_claude_1m_opus: None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CLAUDECODE_THINKING_BUDGET_TOKENS, ensure_oauth_beta,
        extend_model_list_with_thinking_variants, normalize_claudecode_model_and_thinking,
        normalize_claudecode_unsupported_fields, strip_context_1m_beta,
    };
    use crate::channels::claudecode::constants::{CLAUDECODE_DEFAULT_BETAS, OAUTH_BETA};

    #[test]
    fn thinking_suffix_sets_fixed_budget_and_strips_model_suffix() {
        let mut body = json!({
            "model": "claude-opus-4-5-thinking",
            "messages": [],
            "max_tokens": 2048
        });

        let model = normalize_claudecode_model_and_thinking("claude-opus-4-5-thinking", &mut body);

        assert_eq!(model, "claude-opus-4-5");
        assert_eq!(body["model"], json!("claude-opus-4-5"));
        assert_eq!(body["thinking"]["type"], json!("enabled"));
        assert_eq!(
            body["thinking"]["budget_tokens"],
            json!(CLAUDECODE_THINKING_BUDGET_TOKENS)
        );
    }

    #[test]
    fn adaptive_thinking_suffix_forces_adaptive() {
        let mut body = json!({
            "model": "claude-opus-4-5-adaptive-thinking",
            "thinking": {
                "type": "enabled",
                "budget_tokens": 1024
            }
        });

        let model =
            normalize_claudecode_model_and_thinking("claude-opus-4-5-adaptive-thinking", &mut body);

        assert_eq!(model, "claude-opus-4-5");
        assert_eq!(body["model"], json!("claude-opus-4-5"));
        assert_eq!(body["thinking"], json!({"type": "adaptive"}));
    }

    #[test]
    fn thinking_suffix_overrides_existing_to_fixed_budget() {
        let mut body = json!({
            "model": "claude-sonnet-4-5-thinking",
            "thinking": {
                "type": "enabled",
                "budget_tokens": 2048
            }
        });

        let model =
            normalize_claudecode_model_and_thinking("claude-sonnet-4-5-thinking", &mut body);

        assert_eq!(model, "claude-sonnet-4-5");
        assert_eq!(body["model"], json!("claude-sonnet-4-5"));
        assert_eq!(
            body["thinking"],
            json!({
                "type": "enabled",
                "budget_tokens": CLAUDECODE_THINKING_BUDGET_TOKENS
            })
        );
    }

    #[test]
    fn model_list_expands_with_thinking_variants() {
        let mut data = vec![
            json!({"id": "claude-opus-4-6", "object": "model"}),
            json!({"id": "claude-sonnet-4-5", "object": "model"}),
        ];

        extend_model_list_with_thinking_variants(&mut data);

        let ids = data
            .iter()
            .filter_map(|item| item.get("id").and_then(|v| v.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "claude-opus-4-6",
                "claude-opus-4-6-thinking",
                "claude-opus-4-6-adaptive-thinking",
                "claude-sonnet-4-5",
                "claude-sonnet-4-5-thinking",
                "claude-sonnet-4-5-adaptive-thinking"
            ]
        );
    }

    #[test]
    fn model_list_does_not_duplicate_existing_thinking_entries() {
        let mut data = vec![
            json!({"id": "claude-opus-4-6", "object": "model"}),
            json!({"id": "claude-opus-4-6-thinking", "object": "model"}),
        ];

        extend_model_list_with_thinking_variants(&mut data);

        let mut ids = data
            .iter()
            .filter_map(|item| item.get("id").and_then(|v| v.as_str()))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![
                "claude-opus-4-6",
                "claude-opus-4-6-adaptive-thinking",
                "claude-opus-4-6-thinking",
            ]
        );
    }

    #[test]
    fn normalize_claudecode_unsupported_fields_removes_context_management() {
        let mut body = json!({
            "model": "claude-sonnet-4-5",
            "context_management": {
                "edits": [{
                    "type": "compact_20260112"
                }]
            },
            "messages": []
        });

        normalize_claudecode_unsupported_fields(&mut body);

        assert!(body.get("context_management").is_none());
    }

    #[test]
    fn ensure_oauth_beta_adds_claudecode_default_betas() {
        let mut headers = vec![(
            "anthropic-beta".to_string(),
            "custom-beta,effort-2025-11-24".to_string(),
        )];

        ensure_oauth_beta(&mut headers, false);

        let mut expected = vec![
            "custom-beta".to_string(),
            "effort-2025-11-24".to_string(),
            OAUTH_BETA.to_string(),
        ];
        expected.extend(
            CLAUDECODE_DEFAULT_BETAS
                .iter()
                .filter(|value| **value != "effort-2025-11-24")
                .map(|value| value.to_string()),
        );

        assert_eq!(
            headers,
            vec![("anthropic-beta".to_string(), expected.join(","))]
        );
    }

    #[test]
    fn strip_context_1m_beta_keeps_claudecode_default_betas() {
        let mut headers = vec![(
            "anthropic-beta".to_string(),
            "context-1m-2025-08-07,custom-beta".to_string(),
        )];

        strip_context_1m_beta(&mut headers);

        let mut expected = vec!["custom-beta".to_string(), OAUTH_BETA.to_string()];
        expected.extend(
            CLAUDECODE_DEFAULT_BETAS
                .iter()
                .map(|value| value.to_string()),
        );

        assert_eq!(
            headers,
            vec![("anthropic-beta".to_string(), expected.join(","))]
        );
    }
}
