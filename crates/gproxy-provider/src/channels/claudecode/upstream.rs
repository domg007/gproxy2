use serde_json::Value;
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    CLAUDE_CODE_UA, DEFAULT_CLAUDE_AI_BASE_URL, DEFAULT_PLATFORM_BASE_URL, OAUTH_BETA,
};
use super::oauth::{
    ClaudeCodeRefreshedToken, claudecode_access_token_from_credential,
    resolve_claudecode_access_token,
};
use crate::channels::retry::{CredentialRetryDecision, retry_with_eligible_credentials};
use crate::channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamRequestMeta, UpstreamResponse,
};
use crate::channels::utils::{
    anthropic_header_pairs, claude_model_list_query_string, claude_model_to_string,
    is_auth_failure, is_transient_server_failure, join_base_url_and_path, retry_after_to_millis,
    to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

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
    let prepared = ClaudeCodePreparedRequest::from_transform_request(request, prelude_text)?;
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
    let context_1m_target_template = prepared.context_1m_target.clone();
    let url_template = url.clone();
    let base_url_template = base_url.to_string();
    let claude_ai_base_url_template = claude_ai_base_url.clone();
    let request_user_agent_template = provider
        .settings
        .user_agent()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(CLAUDE_CODE_UA)
        .to_string();

    retry_with_eligible_credentials(
        provider,
        credential_states,
        prepared.model.as_deref(),
        now_unix_ms,
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
        .filter(|value| !value.is_empty())
        .unwrap_or(CLAUDE_CODE_UA)
        .to_string();

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
    request_headers: &[(String, String)],
    body: Option<&Vec<u8>>,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let mut beta_values = request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !beta_values
        .iter()
        .any(|value| value.eq_ignore_ascii_case(OAUTH_BETA))
    {
        beta_values.push(OAUTH_BETA.to_string());
    }

    let mut sent_headers = vec![
        (
            "authorization".to_string(),
            format!("Bearer {}", access_token),
        ),
        ("user-agent".to_string(), user_agent.to_string()),
        ("anthropic-beta".to_string(), beta_values.join(",")),
    ];
    for (name, value) in request_headers {
        if name.eq_ignore_ascii_case("anthropic-beta") {
            continue;
        }
        sent_headers.push((name.clone(), value.clone()));
    }
    if body.is_some() {
        sent_headers.push(("content-type".to_string(), "application/json".to_string()));
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
    let sent_headers = vec![
        (
            "authorization".to_string(),
            format!("Bearer {}", access_token),
        ),
        ("accept".to_string(), "application/json".to_string()),
        ("content-type".to_string(), "application/json".to_string()),
        ("user-agent".to_string(), user_agent.to_string()),
        ("anthropic-beta".to_string(), OAUTH_BETA.to_string()),
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
    context_1m_target: Option<ClaudeCode1mTarget>,
}

impl ClaudeCodePreparedRequest {
    fn from_transform_request(
        request: &gproxy_middleware::TransformRequest,
        prelude_text: Option<&str>,
    ) -> Result<Self, UpstreamError> {
        match request {
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
                    context_1m_target: None,
                })
            }
            gproxy_middleware::TransformRequest::ModelGetClaude(value) => {
                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;
                ensure_oauth_beta(&mut request_headers, false);

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1/models/{}", value.path.model_id),
                    body: None,
                    model: Some(value.path.model_id.clone()),
                    request_headers,
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
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/messages/count_tokens".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
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
                normalize_claudecode_sampling(model.as_str(), &mut body_json);
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/messages".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
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
                normalize_claudecode_sampling(model.as_str(), &mut body_json);
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/messages".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    context_1m_target,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn ensure_oauth_beta(headers: &mut Vec<(String, String)>, allow_context_1m: bool) {
    let mut values = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !allow_context_1m {
        values.retain(|value| !is_context_1m_beta(value));
    }

    let oauth_beta = OAUTH_BETA;
    if !values
        .iter()
        .any(|value| value.eq_ignore_ascii_case(oauth_beta))
    {
        values.push(oauth_beta.to_string());
    }

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
    let mut values = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("anthropic-beta"))
        .map(|(_, value)| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .filter(|item| !is_context_1m_beta(item))
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !values
        .iter()
        .any(|value| value.eq_ignore_ascii_case(OAUTH_BETA))
    {
        values.push(OAUTH_BETA.to_string());
    }

    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("anthropic-beta"));
    headers.push(("anthropic-beta".to_string(), values.join(",")));
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
