use std::time::{SystemTime, UNIX_EPOCH};

use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformResponse};
use serde_json::{Value, json};
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    ACCOUNT_ID_HEADER, CLIENT_VERSION, ORIGINATOR_HEADER, ORIGINATOR_VALUE, USER_AGENT_HEADER,
    USER_AGENT_VALUE,
};
use super::oauth::{
    CodexRefreshedToken, codex_auth_material_from_credential, resolve_codex_access_token,
};
use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode, retry_with_eligible_credentials,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamRequestMeta, UpstreamResponse,
};
use crate::channels::utils::{
    count_openai_input_tokens_with_resolution, is_auth_failure, is_transient_server_failure,
    join_base_url_and_path, resolve_user_agent_or_default, retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, RetryWithPayloadRequest, TokenizerResolutionContext};

#[derive(Debug, Clone)]
enum CodexRequestKind {
    ModelList,
    ModelGet { target: String },
    Forward,
}

#[derive(Debug, Clone)]
struct CodexPreparedRequest {
    method: WreqMethod,
    path: String,
    body: Option<Vec<u8>>,
    model: Option<String>,
    kind: CodexRequestKind,
}

pub async fn execute_codex_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
    token_resolution: TokenizerResolutionContext<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    if let Some(local_response) =
        try_local_codex_count_token_response(request, client, token_resolution).await?
    {
        return Ok(UpstreamResponse::from_local(local_response));
    }

    let prepared = CodexPreparedRequest::from_transform_request(request)?;
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
    execute_codex_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_codex_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    payload: RetryWithPayloadRequest<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    if (payload.operation, payload.protocol) == (OperationFamily::CountToken, ProtocolKind::OpenAi)
    {
        let body_json = serde_json::from_slice::<Value>(payload.body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let model = body_json
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let input_tokens = count_openai_input_tokens_with_resolution(
            payload.token_resolution.tokenizer_store,
            client,
            payload.token_resolution.hf_token,
            payload.token_resolution.hf_url,
            model.as_deref(),
            &body_json,
        )
        .await?;
        let response_json = json!({
            "stats_code": 200,
            "headers": {},
            "body": {
                "input_tokens": input_tokens,
                "object": "response.input_tokens",
            }
        });
        let response = serde_json::from_value(response_json)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        return Ok(UpstreamResponse::from_local(
            TransformResponse::CountTokenOpenAi(response),
        ));
    }

    let prepared =
        CodexPreparedRequest::from_payload(payload.operation, payload.protocol, payload.body)?;
    execute_codex_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        payload.now_unix_ms,
        None,
    )
    .await
}

async fn execute_codex_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: CodexPreparedRequest,
    now_unix_ms: u64,
    cache_affinity_hint: Option<crate::channels::retry::CacheAffinityHint>,
) -> Result<UpstreamResponse, UpstreamError> {
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }

    let state_manager = CredentialStateManager::new(now_unix_ms);
    let method_template = prepared.method.clone();
    let path_template = prepared.path.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let kind_template = prepared.kind.clone();
    let base_url_template = base_url.to_string();
    let user_agent_template =
        resolve_user_agent_or_default(provider.settings.user_agent(), USER_AGENT_VALUE);
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
            if let ChannelCredential::Builtin(BuiltinChannelCredential::Codex(value)) =
                &credential.credential
            {
                return codex_auth_material_from_credential(value);
            }
            None
        },
        |attempt| {
            let method = method_template.clone();
            let path = path_template.clone();
            let body = body_template.clone();
            let model = model_template.clone();
            let kind = kind_template.clone();
            let base_url = base_url_template.clone();
            let user_agent = user_agent_template.clone();

            async move {
                let url = join_base_url_and_path(base_url.as_str(), path.as_str());
                let token_cache_key =
                    format!("{}::{}", provider.channel.as_str(), attempt.credential_id);
                let mut credential_update = None;

                let access_token = match resolve_codex_access_token(
                    client,
                    &provider.settings,
                    token_cache_key.as_str(),
                    &attempt.material,
                    now_unix_ms,
                    false,
                )
                .await
                {
                    Ok(token) => {
                        if let Some(refreshed) = token.refreshed.as_ref() {
                            credential_update =
                                Some(codex_credential_update(attempt.credential_id, refreshed));
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
                let (mut response, mut request_meta) = match send_codex_request(
                    client,
                    method.clone(),
                    url.as_str(),
                    access_token.as_str(),
                    attempt.material.account_id.as_str(),
                    user_agent.as_str(),
                    body.as_ref(),
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
                    let refreshed_token = match resolve_codex_access_token(
                        client,
                        &provider.settings,
                        token_cache_key.as_str(),
                        &attempt.material,
                        now_unix_ms,
                        true,
                    )
                    .await
                    {
                        Ok(token) => {
                            if let Some(refreshed) = token.refreshed.as_ref() {
                                credential_update =
                                    Some(codex_credential_update(attempt.credential_id, refreshed));
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
                                last_request_meta: None,
                            };
                        }
                    };
                    (response, request_meta) = match send_codex_request(
                        client,
                        method,
                        url.as_str(),
                        refreshed_token.as_str(),
                        attempt.material.account_id.as_str(),
                        user_agent.as_str(),
                        body.as_ref(),
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
                            "upstream status {} after codex access token refresh",
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

                match kind {
                    CodexRequestKind::Forward => {
                        if response.status().is_success() {
                            state_manager.mark_success(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                            );
                        }
                        CredentialRetryDecision::Return(
                            UpstreamResponse::from_http(
                                attempt.credential_id,
                                attempt.attempts,
                                response,
                            )
                            .with_request_meta(request_meta.clone())
                            .with_credential_update(credential_update.clone()),
                        )
                    }
                    CodexRequestKind::ModelList => {
                        let bytes = match response.bytes().await {
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
                                    last_request_meta: None,
                                };
                            }
                        };

                        let local = build_model_list_local_response(status_code, &bytes);
                        if status_code == 200 {
                            state_manager.mark_success(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                            );
                        }
                        CredentialRetryDecision::Return(
                            UpstreamResponse::from_local(local)
                                .with_request_meta(request_meta.clone())
                                .with_credential_update(credential_update.clone()),
                        )
                    }
                    CodexRequestKind::ModelGet { target } => {
                        let bytes = match response.bytes().await {
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
                                    last_request_meta: None,
                                };
                            }
                        };

                        let local = build_model_get_local_response(status_code, &bytes, &target);
                        if status_code == 200 {
                            state_manager.mark_success(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                            );
                        }
                        CredentialRetryDecision::Return(
                            UpstreamResponse::from_local(local)
                                .with_request_meta(request_meta.clone())
                                .with_credential_update(credential_update.clone()),
                        )
                    }
                }
            }
        },
    )
    .await
}

pub async fn execute_codex_upstream_usage_with_retry(
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
    let usage_url = {
        let base = base_url.trim_end_matches('/');
        let base = base.strip_suffix("/codex").unwrap_or(base);
        format!("{base}/wham/usage")
    };

    let state_manager = CredentialStateManager::new(now_unix_ms);
    let usage_url_template = usage_url.clone();
    let channel_id = scoped_provider.channel.clone();
    let user_agent_template =
        resolve_user_agent_or_default(scoped_provider.settings.user_agent(), USER_AGENT_VALUE);

    retry_with_eligible_credentials(
        &scoped_provider,
        credential_states,
        None,
        now_unix_ms,
        |credential| {
            if let ChannelCredential::Builtin(BuiltinChannelCredential::Codex(value)) =
                &credential.credential
            {
                return codex_auth_material_from_credential(value);
            }
            None
        },
        |attempt| {
            let usage_url = usage_url_template.clone();
            let channel_id = channel_id.clone();
            let user_agent = user_agent_template.clone();
            async move {
                let token_cache_key = format!("{}::{}", channel_id.as_str(), attempt.credential_id);
                let mut credential_update = None;

                let access_token = match resolve_codex_access_token(
                    client,
                    &provider.settings,
                    token_cache_key.as_str(),
                    &attempt.material,
                    now_unix_ms,
                    false,
                )
                .await
                {
                    Ok(token) => {
                        if let Some(refreshed) = token.refreshed.as_ref() {
                            credential_update =
                                Some(codex_credential_update(attempt.credential_id, refreshed));
                        }
                        token.access_token
                    }
                    Err(err) => {
                        let message = err.as_message();
                        if err.is_invalid_credential() {
                            state_manager.mark_auth_dead(
                                credential_states,
                                &channel_id,
                                attempt.credential_id,
                                Some(message.clone()),
                            );
                        } else {
                            state_manager.mark_transient_failure(
                                credential_states,
                                &channel_id,
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
                let (mut response, mut request_meta) = match send_codex_usage_request(
                    client,
                    usage_url.as_str(),
                    access_token.as_str(),
                    attempt.material.account_id.as_str(),
                    user_agent.as_str(),
                )
                .await
                {
                    Ok((response, request_meta)) => (response, request_meta),
                    Err(err) => {
                        let message = err.to_string();
                        state_manager.mark_transient_failure(
                            credential_states,
                            &channel_id,
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
                    let refreshed_token = match resolve_codex_access_token(
                        client,
                        &provider.settings,
                        token_cache_key.as_str(),
                        &attempt.material,
                        now_unix_ms,
                        true,
                    )
                    .await
                    {
                        Ok(token) => {
                            if let Some(refreshed) = token.refreshed.as_ref() {
                                credential_update =
                                    Some(codex_credential_update(attempt.credential_id, refreshed));
                            }
                            token.access_token
                        }
                        Err(err) => {
                            let message = err.as_message();
                            if err.is_invalid_credential() {
                                state_manager.mark_auth_dead(
                                    credential_states,
                                    &channel_id,
                                    attempt.credential_id,
                                    Some(message.clone()),
                                );
                            } else {
                                state_manager.mark_transient_failure(
                                    credential_states,
                                    &channel_id,
                                    attempt.credential_id,
                                    None,
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
                    (response, request_meta) = match send_codex_usage_request(
                        client,
                        usage_url.as_str(),
                        refreshed_token.as_str(),
                        attempt.material.account_id.as_str(),
                        user_agent.as_str(),
                    )
                    .await
                    {
                        Ok((response, request_meta)) => (response, request_meta),
                        Err(err) => {
                            let message = err.to_string();
                            state_manager.mark_transient_failure(
                                credential_states,
                                &channel_id,
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

                    status_code = response.status().as_u16();
                    if is_auth_failure(status_code) {
                        let message = format!(
                            "upstream status {} after codex access token refresh",
                            status_code
                        );
                        state_manager.mark_auth_dead(
                            credential_states,
                            &channel_id,
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
                        &channel_id,
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
                        &channel_id,
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

                if response.status().is_success() {
                    state_manager.mark_success(
                        credential_states,
                        &channel_id,
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

async fn send_codex_request(
    client: &WreqClient,
    method: WreqMethod,
    url: &str,
    access_token: &str,
    account_id: &str,
    user_agent: &str,
    body: Option<&Vec<u8>>,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let mut headers = vec![
        (
            "authorization".to_string(),
            format!("Bearer {access_token}"),
        ),
        (ACCOUNT_ID_HEADER.to_string(), account_id.to_string()),
        (ORIGINATOR_HEADER.to_string(), ORIGINATOR_VALUE.to_string()),
        (USER_AGENT_HEADER.to_string(), user_agent.to_string()),
    ];
    if body.is_some() {
        headers.push(("content-type".to_string(), "application/json".to_string()));
    }
    crate::channels::upstream::tracked_send_request(client, method, url, headers, body.cloned())
        .await
}

async fn send_codex_usage_request(
    client: &WreqClient,
    url: &str,
    access_token: &str,
    account_id: &str,
    user_agent: &str,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let headers = vec![
        (
            "authorization".to_string(),
            format!("Bearer {access_token}"),
        ),
        (ACCOUNT_ID_HEADER.to_string(), account_id.to_string()),
        (ORIGINATOR_HEADER.to_string(), ORIGINATOR_VALUE.to_string()),
        (USER_AGENT_HEADER.to_string(), user_agent.to_string()),
        ("accept".to_string(), "application/json".to_string()),
    ];
    crate::channels::upstream::tracked_send_request(client, WreqMethod::GET, url, headers, None)
        .await
}

impl CodexPreparedRequest {
    fn from_transform_request(request: &TransformRequest) -> Result<Self, UpstreamError> {
        match request {
            TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: codex_models_path(),
                body: None,
                model: None,
                kind: CodexRequestKind::ModelList,
            }),
            TransformRequest::ModelGetOpenAi(value) => {
                let target = normalize_model_id(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: codex_models_path(),
                    body: None,
                    model: Some(target.clone()),
                    kind: CodexRequestKind::ModelGet { target },
                })
            }
            TransformRequest::GenerateContentOpenAiResponse(value)
            | TransformRequest::StreamGenerateContentOpenAiResponse(value) => {
                let mut body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_codex_response_request_body(
                    &mut body,
                    matches!(
                        request,
                        TransformRequest::StreamGenerateContentOpenAiResponse(_)
                    ),
                );
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/responses".to_string(),
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: value.body.model.clone(),
                    kind: CodexRequestKind::Forward,
                })
            }
            TransformRequest::CompactOpenAi(value) => {
                let mut body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_codex_compact_request_body(&mut body);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/responses/compact".to_string(),
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(normalize_model_id(value.body.model.as_str())),
                    kind: CodexRequestKind::Forward,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }

    fn from_payload(
        operation: OperationFamily,
        protocol: ProtocolKind,
        body: &[u8],
    ) -> Result<Self, UpstreamError> {
        fn json_pointer_string(body: &[u8], pointer: &str) -> Option<String> {
            serde_json::from_slice::<Value>(body)
                .ok()
                .and_then(|value| {
                    value
                        .pointer(pointer)
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
        }

        match (operation, protocol) {
            (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(Self {
                method: WreqMethod::GET,
                path: codex_models_path(),
                body: None,
                model: None,
                kind: CodexRequestKind::ModelList,
            }),
            (OperationFamily::ModelGet, ProtocolKind::OpenAi) => {
                let Some(target_raw) = json_pointer_string(body, "/path/model") else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in codex model_get payload".to_string(),
                    ));
                };
                let target = normalize_model_id(target_raw.as_str());
                Ok(Self {
                    method: WreqMethod::GET,
                    path: codex_models_path(),
                    body: None,
                    model: Some(target.clone()),
                    kind: CodexRequestKind::ModelGet { target },
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
                let mut body_json = serde_json::from_slice::<Value>(body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_codex_response_request_body(
                    &mut body_json,
                    operation == OperationFamily::StreamGenerateContent,
                );
                let model = body_json
                    .get("model")
                    .and_then(Value::as_str)
                    .map(normalize_model_id);
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/responses".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model,
                    kind: CodexRequestKind::Forward,
                })
            }
            (OperationFamily::Compact, ProtocolKind::OpenAi) => {
                let mut body_json = serde_json::from_slice::<Value>(body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_codex_compact_request_body(&mut body_json);
                let model = body_json
                    .get("model")
                    .and_then(Value::as_str)
                    .map(normalize_model_id);
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/responses/compact".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model,
                    kind: CodexRequestKind::Forward,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

async fn try_local_codex_count_token_response(
    request: &TransformRequest,
    http_client: &WreqClient,
    token_resolution: TokenizerResolutionContext<'_>,
) -> Result<Option<TransformResponse>, UpstreamError> {
    let TransformRequest::CountTokenOpenAi(value) = request else {
        return Ok(None);
    };

    let input_tokens = count_openai_input_tokens_with_resolution(
        token_resolution.tokenizer_store,
        http_client,
        token_resolution.hf_token,
        token_resolution.hf_url,
        value.body.model.as_deref(),
        &value.body,
    )
    .await?;

    let response_json = json!({
        "stats_code": 200,
        "headers": {},
        "body": {
            "input_tokens": input_tokens,
            "object": "response.input_tokens",
        }
    });
    let response = serde_json::from_value(response_json)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok(Some(TransformResponse::CountTokenOpenAi(response)))
}

fn codex_models_path() -> String {
    format!("/models?client_version={CLIENT_VERSION}")
}

fn normalize_model_id(model: &str) -> String {
    let model = model.trim().trim_start_matches('/');
    let model = model.strip_prefix("models/").unwrap_or(model);
    model.strip_prefix("codex/").unwrap_or(model).to_string()
}

fn normalize_codex_response_request_body(body: &mut Value, is_stream: bool) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    if let Some(model) = map.get_mut("model")
        && let Some(model_str) = model.as_str()
    {
        *model = Value::String(normalize_model_id(model_str));
    }

    map.insert("store".to_string(), Value::Bool(false));
    map.remove("max_output_tokens");
    map.remove("metadata");
    map.remove("stream_options");
    map.remove("temperature");
    map.remove("top_p");
    map.remove("top_logprobs");
    map.remove("safety_identifier");
    map.remove("truncation");
    extract_codex_instructions_from_input_messages(map);

    if is_stream {
        map.insert("stream".to_string(), Value::Bool(true));
    } else {
        map.insert("stream".to_string(), Value::Bool(false));
    }

    if map
        .get("instructions")
        .is_some_and(|value| !value.is_string())
    {
        map.insert("instructions".to_string(), Value::String(String::new()));
    }

    if !map.contains_key("instructions") {
        map.insert("instructions".to_string(), Value::String(String::new()));
    }

    if let Some(input) = map.get("input")
        && let Some(text) = input.as_str()
    {
        map.insert(
            "input".to_string(),
            json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": text,
                }
            ]),
        );
    }
}

fn extract_codex_instructions_from_input_messages(map: &mut serde_json::Map<String, Value>) {
    let mut extracted = Vec::new();

    if let Some(Value::Array(items)) = map.get_mut("input") {
        let source_items = std::mem::take(items);
        let mut kept = Vec::with_capacity(source_items.len());
        for item in source_items {
            let role = item.get("role").and_then(Value::as_str);
            if matches!(role, Some("system" | "developer")) {
                if let Some(text) = extract_codex_message_text(item.get("content")) {
                    extracted.push(text);
                }
                continue;
            }
            kept.push(item);
        }
        *items = kept;
    }

    if extracted.is_empty() {
        return;
    }

    let extracted_text = extracted.join("\n\n");
    let current = map
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let merged = match current {
        Some(base) => format!("{base}\n\n{extracted_text}"),
        None => extracted_text,
    };
    map.insert("instructions".to_string(), Value::String(merged));
}

fn extract_codex_message_text(content: Option<&Value>) -> Option<String> {
    let content = content?;
    match content {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                if let Some(text) = extract_codex_text_part(part) {
                    out.push(text);
                }
            }
            (!out.is_empty()).then(|| out.join("\n"))
        }
        Value::Object(_) => extract_codex_text_part(content),
        _ => None,
    }
}

fn extract_codex_text_part(part: &Value) -> Option<String> {
    match part {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        Value::Object(obj) => {
            let text = obj
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| obj.get("refusal").and_then(Value::as_str))?;
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        _ => None,
    }
}

fn normalize_codex_compact_request_body(body: &mut Value) {
    let Some(map) = body.as_object_mut() else {
        return;
    };

    if let Some(model) = map.get_mut("model")
        && let Some(model_str) = model.as_str()
    {
        *model = Value::String(normalize_model_id(model_str));
    }

    if let Some(input) = map.get("input")
        && let Some(text) = input.as_str()
    {
        map.insert(
            "input".to_string(),
            json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": text,
                }
            ]),
        );
    }
}

fn build_model_list_local_response(status_code: u16, bytes: &[u8]) -> TransformResponse {
    if status_code == 200 {
        let parsed = serde_json::from_slice::<Value>(bytes).ok();
        if let Some(parsed) = parsed
            && let Some(body) = normalize_openai_model_list_value(&parsed)
        {
            let response_json = json!({
                "stats_code": 200,
                "headers": {},
                "body": body,
            });
            if let Ok(response) = serde_json::from_value(response_json) {
                return TransformResponse::ModelListOpenAi(response);
            }
        }

        return model_list_error_response(502, "invalid codex model-list payload");
    }

    let message = extract_upstream_error_message(bytes)
        .unwrap_or_else(|| format!("upstream status {status_code}"));
    model_list_error_response(status_code, &message)
}

fn build_model_get_local_response(
    status_code: u16,
    bytes: &[u8],
    target: &str,
) -> TransformResponse {
    if status_code == 200 {
        let parsed = serde_json::from_slice::<Value>(bytes).ok();
        if let Some(parsed) = parsed
            && let Some(list_value) = normalize_openai_model_list_value(&parsed)
            && let Some(model) = find_model_in_openai_list(&list_value, target)
        {
            let response_json = json!({
                "stats_code": 200,
                "headers": {},
                "body": model,
            });
            if let Ok(response) = serde_json::from_value(response_json) {
                return TransformResponse::ModelGetOpenAi(response);
            }
        }

        let message = format!("model {target} not found");
        return model_get_error_response(404, &message);
    }

    let message = extract_upstream_error_message(bytes)
        .unwrap_or_else(|| format!("upstream status {status_code}"));
    model_get_error_response(status_code, &message)
}

fn normalize_openai_model_list_value(value: &Value) -> Option<Value> {
    if is_openai_model_list(value) {
        return Some(value.clone());
    }

    let models = value.get("models")?.as_array()?;
    let mut data = Vec::new();
    for item in models {
        if let Some(model) = normalize_openai_model_value(item) {
            data.push(model);
        }
    }

    Some(json!({
        "object": "list",
        "data": data,
    }))
}

fn normalize_openai_model_value(value: &Value) -> Option<Value> {
    if is_openai_model_value(value) {
        return Some(value.clone());
    }

    let object = value.as_object()?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| object.get("slug").and_then(Value::as_str))?;

    let created = object
        .get("created")
        .and_then(Value::as_u64)
        .unwrap_or_else(current_unix_ts);
    let owned_by = object
        .get("owned_by")
        .and_then(Value::as_str)
        .unwrap_or("openai");

    Some(json!({
        "id": normalize_model_id(id),
        "object": "model",
        "owned_by": owned_by,
        "created": created,
    }))
}

fn is_openai_model_list(value: &Value) -> bool {
    value
        .get("object")
        .and_then(Value::as_str)
        .map(|object| object == "list")
        .unwrap_or(false)
        && value.get("data").and_then(Value::as_array).is_some()
}

fn is_openai_model_value(value: &Value) -> bool {
    value
        .get("object")
        .and_then(Value::as_str)
        .map(|object| object == "model")
        .unwrap_or(false)
        && value.get("id").and_then(Value::as_str).is_some()
        && value.get("owned_by").and_then(Value::as_str).is_some()
        && value.get("created").and_then(Value::as_u64).is_some()
}

fn find_model_in_openai_list(list: &Value, target: &str) -> Option<Value> {
    let data = list.get("data")?.as_array()?;
    data.iter()
        .find(|item| {
            item.get("id")
                .and_then(Value::as_str)
                .map(|id| normalize_model_id(id) == target)
                .unwrap_or(false)
        })
        .cloned()
}

fn extract_upstream_error_message(bytes: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(bytes).ok()?;
    if let Some(message) = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        return Some(message.to_string());
    }
    if let Some(message) = value.get("error").and_then(Value::as_str) {
        return Some(message.to_string());
    }
    value
        .get("message")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn model_list_error_response(status_code: u16, message: &str) -> TransformResponse {
    let response_json = json!({
        "stats_code": status_code,
        "headers": {},
        "body": {
            "error": {
                "message": message,
                "type": "invalid_request_error",
                "param": null,
                "code": "upstream_error",
            }
        }
    });

    match serde_json::from_value(response_json) {
        Ok(response) => TransformResponse::ModelListOpenAi(response),
        Err(_) => internal_model_list_fallback(),
    }
}

fn model_get_error_response(status_code: u16, message: &str) -> TransformResponse {
    let response_json = json!({
        "stats_code": status_code,
        "headers": {},
        "body": {
            "error": {
                "message": message,
                "type": "invalid_request_error",
                "param": "model",
                "code": "upstream_error",
            }
        }
    });

    match serde_json::from_value(response_json) {
        Ok(response) => TransformResponse::ModelGetOpenAi(response),
        Err(_) => internal_model_get_fallback(),
    }
}

fn internal_model_list_fallback() -> TransformResponse {
    let response_json = json!({
        "stats_code": 500,
        "headers": {},
        "body": {
            "error": {
                "message": "internal serialization error",
                "type": "server_error",
                "param": null,
                "code": "internal_error",
            }
        }
    });
    let response = serde_json::from_value(response_json)
        .expect("internal fallback model list response must be valid");
    TransformResponse::ModelListOpenAi(response)
}

fn internal_model_get_fallback() -> TransformResponse {
    let response_json = json!({
        "stats_code": 500,
        "headers": {},
        "body": {
            "error": {
                "message": "internal serialization error",
                "type": "server_error",
                "param": "model",
                "code": "internal_error",
            }
        }
    });
    let response = serde_json::from_value(response_json)
        .expect("internal fallback model get response must be valid");
    TransformResponse::ModelGetOpenAi(response)
}

fn current_unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn codex_credential_update(
    credential_id: i64,
    refreshed: &CodexRefreshedToken,
) -> UpstreamCredentialUpdate {
    UpstreamCredentialUpdate::CodexTokenRefresh {
        credential_id,
        access_token: refreshed.access_token.clone(),
        refresh_token: refreshed.refresh_token.clone(),
        expires_at_unix_ms: refreshed.expires_at_unix_ms,
        user_email: refreshed.user_email.clone(),
        id_token: refreshed.id_token.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::normalize_codex_response_request_body;

    #[test]
    fn codex_moves_system_and_developer_messages_into_instructions() {
        let mut body = json!({
            "model": "codex/gpt-5.2",
            "input": [
                {
                    "type": "message",
                    "role": "system",
                    "content": "be concise"
                },
                {
                    "type": "message",
                    "role": "developer",
                    "content": [
                        {"type": "input_text", "text": "keep markdown"}
                    ]
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "hello"}
                    ]
                }
            ],
            "temperature": 1
        });

        normalize_codex_response_request_body(&mut body, true);

        assert_eq!(
            body.get("model").and_then(|value| value.as_str()),
            Some("gpt-5.2")
        );
        assert_eq!(
            body.get("stream").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            body.get("instructions").and_then(|value| value.as_str()),
            Some("be concise\n\nkeep markdown")
        );
        assert_eq!(
            body.pointer("/input/0/role")
                .and_then(|value| value.as_str()),
            Some("user")
        );
        assert!(body.pointer("/input/1").is_none());
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn codex_appends_extracted_system_message_to_existing_instructions() {
        let mut body = json!({
            "model": "gpt-5.2",
            "instructions": "existing",
            "input": [
                {
                    "type": "message",
                    "role": "system",
                    "content": "extra"
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": "hi"
                }
            ]
        });

        normalize_codex_response_request_body(&mut body, false);

        assert_eq!(
            body.get("instructions").and_then(|value| value.as_str()),
            Some("existing\n\nextra")
        );
        assert_eq!(
            body.get("stream").and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            body.pointer("/input/0/role")
                .and_then(|value| value.as_str()),
            Some("user")
        );
        assert!(body.pointer("/input/1").is_none());
    }
}
