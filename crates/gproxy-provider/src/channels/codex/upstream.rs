use std::time::{SystemTime, UNIX_EPOCH};

use gproxy_middleware::{
    OperationFamily, ProtocolKind, TransformRequest, TransformResponse, TransformRoute,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{
    ACCOUNT_ID_HEADER, CLIENT_VERSION, ORIGINATOR_HEADER, ORIGINATOR_VALUE, USER_AGENT_HEADER,
    USER_AGENT_VALUE,
};
use super::oauth::{
    CodexRefreshedToken, codex_auth_material_from_credential, resolve_codex_access_token,
};
use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_codex_openai_response_body,
    cache_affinity_hint_from_codex_transform_request, configured_pick_mode_uses_cache,
    credential_pick_mode, retry_with_eligible_credentials,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamRequestMeta, UpstreamResponse,
    add_or_replace_header, extra_headers_from_payload_value, extra_headers_from_transform_request,
    merge_extra_headers, payload_body_value,
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

const SESSION_ID_HEADER: &str = "session_id";
const SESSION_ID_ALT_HEADER: &str = "session-id";

#[derive(Debug, Clone)]
struct CodexPreparedRequest {
    method: WreqMethod,
    path: String,
    body: Option<Vec<u8>>,
    model: Option<String>,
    kind: CodexRequestKind,
    extra_headers: Vec<(String, String)>,
}

struct CodexRequestParams<'a> {
    method: WreqMethod,
    url: &'a str,
    access_token: &'a str,
    account_id: &'a str,
    user_agent: &'a str,
    extra_headers: &'a [(String, String)],
    body: Option<&'a [u8]>,
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
        cache_affinity_hint_from_codex_transform_request(
            request,
            prepared.model.as_deref(),
            prepared.body.as_deref(),
        )
        .or_else(|| {
            cache_affinity_hint_from_codex_openai_response_body(
                prepared.model.as_deref(),
                prepared.body.as_deref(),
            )
        })
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
        let payload_json = serde_json::from_slice::<Value>(payload.body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let body_json = payload_body_value(&payload_json);
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
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        cache_affinity_hint_from_codex_openai_response_body(
            prepared.model.as_deref(),
            prepared.body.as_deref(),
        )
    } else {
        None
    };
    execute_codex_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        payload.now_unix_ms,
        cache_affinity_hint,
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
    let extra_headers_template = prepared.extra_headers.clone();
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
            let extra_headers = extra_headers_template.clone();
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
                    CodexRequestParams {
                        method: method.clone(),
                        url: url.as_str(),
                        access_token: access_token.as_str(),
                        account_id: attempt.material.account_id.as_str(),
                        user_agent: user_agent.as_str(),
                        extra_headers: extra_headers.as_slice(),
                        body: body.as_deref(),
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
                        CodexRequestParams {
                            method,
                            url: url.as_str(),
                            access_token: refreshed_token.as_str(),
                            account_id: attempt.material.account_id.as_str(),
                            user_agent: user_agent.as_str(),
                            extra_headers: extra_headers.as_slice(),
                            body: body.as_deref(),
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
    params: CodexRequestParams<'_>,
) -> Result<(wreq::Response, UpstreamRequestMeta), wreq::Error> {
    let mut headers = Vec::new();
    merge_extra_headers(&mut headers, params.extra_headers);
    add_or_replace_header(
        &mut headers,
        "authorization",
        format!("Bearer {}", params.access_token),
    );
    add_or_replace_header(
        &mut headers,
        ACCOUNT_ID_HEADER,
        params.account_id.to_string(),
    );
    add_or_replace_header(&mut headers, ORIGINATOR_HEADER, ORIGINATOR_VALUE);
    add_or_replace_header(
        &mut headers,
        USER_AGENT_HEADER,
        params.user_agent.to_string(),
    );
    if params.body.is_some() {
        add_or_replace_header(&mut headers, "content-type", "application/json");
    }
    crate::channels::upstream::tracked_send_request(
        client,
        params.method,
        params.url,
        headers,
        params.body.map(|value| value.to_vec()),
    )
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
        let extra_headers = extra_headers_from_transform_request(request);
        let mut prepared = match request {
            TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: codex_models_path(),
                body: None,
                model: None,
                kind: CodexRequestKind::ModelList,
                extra_headers: Vec::new(),
            }),
            TransformRequest::ModelGetOpenAi(value) => {
                let target = normalize_model_id(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: codex_models_path(),
                    body: None,
                    model: Some(target.clone()),
                    kind: CodexRequestKind::ModelGet { target },
                    extra_headers: Vec::new(),
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
                    extra_headers: Vec::new(),
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
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::OpenAiResponseWebSocket(value) => {
                let transformed = transform_openai_ws_request_to_stream(
                    TransformRequest::OpenAiResponseWebSocket(value.clone()),
                )?;
                Self::from_transform_request(&transformed)
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }?;
        prepared.extra_headers = extra_headers;
        if matches!(prepared.kind, CodexRequestKind::Forward) {
            ensure_codex_session_id_header(&mut prepared.extra_headers, prepared.body.as_deref());
        }
        Ok(prepared)
    }

    fn from_payload(
        operation: OperationFamily,
        protocol: ProtocolKind,
        body: &[u8],
    ) -> Result<Self, UpstreamError> {
        fn json_pointer_string(value: &Value, pointer: &str) -> Option<String> {
            value
                .pointer(pointer)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        }

        let payload_value = serde_json::from_slice::<Value>(body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let extra_headers = extra_headers_from_payload_value(&payload_value);

        match (operation, protocol) {
            (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(Self {
                method: WreqMethod::GET,
                path: codex_models_path(),
                body: None,
                model: None,
                kind: CodexRequestKind::ModelList,
                extra_headers,
            }),
            (OperationFamily::ModelGet, ProtocolKind::OpenAi) => {
                let Some(target_raw) = json_pointer_string(&payload_value, "/path/model") else {
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
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
                let mut body_json = payload_body_value(&payload_value);
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
                    extra_headers,
                })
            }
            (OperationFamily::Compact, ProtocolKind::OpenAi) => {
                let mut body_json = payload_body_value(&payload_value);
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
                    extra_headers,
                })
            }
            (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi) => {
                let request = gproxy_middleware::decode_request_payload(operation, protocol, body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                let transformed = transform_openai_ws_request_to_stream(request)?;
                Self::from_transform_request(&transformed)
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
        .map(|mut prepared| {
            if matches!(prepared.kind, CodexRequestKind::Forward) {
                ensure_codex_session_id_header(
                    &mut prepared.extra_headers,
                    prepared.body.as_deref(),
                );
            }
            prepared
        })
    }
}

fn transform_openai_ws_request_to_stream(
    request: TransformRequest,
) -> Result<TransformRequest, UpstreamError> {
    gproxy_middleware::transform_request(
        request,
        TransformRoute {
            src_operation: OperationFamily::OpenAiResponseWebSocket,
            src_protocol: ProtocolKind::OpenAi,
            dst_operation: OperationFamily::StreamGenerateContent,
            dst_protocol: ProtocolKind::OpenAi,
        },
    )
    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
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

fn ensure_codex_session_id_header(extra_headers: &mut Vec<(String, String)>, body: Option<&[u8]>) {
    let session_id = extra_headers
        .iter()
        .find(|(name, _)| is_session_id_header(name))
        .and_then(|(_, value)| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        })
        .or_else(|| synthesize_codex_session_id(body));

    let Some(session_id) = session_id else {
        return;
    };

    extra_headers.retain(|(name, _)| !is_session_id_header(name));
    extra_headers.push((SESSION_ID_HEADER.to_string(), session_id));
}

fn is_session_id_header(name: &str) -> bool {
    name.eq_ignore_ascii_case(SESSION_ID_HEADER) || name.eq_ignore_ascii_case(SESSION_ID_ALT_HEADER)
}

fn synthesize_codex_session_id(body: Option<&[u8]>) -> Option<String> {
    let body_json = serde_json::from_slice::<Value>(body?).ok()?;
    let session_marker = codex_session_marker_from_body(&body_json)
        .or_else(|| codex_initial_prompt_session_marker(&body_json))?;
    Some(stable_codex_session_id(session_marker.as_str()))
}

fn codex_session_marker_from_body(body_json: &Value) -> Option<String> {
    body_json
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let conversation = body_json.get("conversation")?;
            match conversation {
                Value::String(value) => {
                    let value = value.trim();
                    (!value.is_empty()).then(|| value.to_string())
                }
                Value::Object(value) => value
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .map(ToOwned::to_owned),
                _ => None,
            }
        })
        .or_else(|| {
            body_json
                .get("previous_response_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn codex_initial_prompt_session_marker(body_json: &Value) -> Option<String> {
    let mut marker = serde_json::Map::new();

    if let Some(instructions) = body_json
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        marker.insert(
            "instructions".to_string(),
            Value::String(instructions.to_string()),
        );
    }

    if let Some(first_input) = codex_first_input_session_marker(body_json.get("input")) {
        marker.insert("input".to_string(), first_input);
    }

    (!marker.is_empty())
        .then(|| serde_json::to_string(&Value::Object(marker)).ok())
        .flatten()
}

fn codex_first_input_session_marker(input: Option<&Value>) -> Option<Value> {
    match input? {
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| Value::String(text.to_string()))
        }
        Value::Array(items) => items.first().cloned(),
        Value::Null => None,
        value => Some(value.clone()),
    }
}

fn stable_codex_session_id(marker: &str) -> String {
    let digest = Sha256::digest(format!("gproxy.codex.session:{marker}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    )
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

    use super::{
        CodexPreparedRequest, SESSION_ID_HEADER, WreqMethod, normalize_codex_response_request_body,
        stable_codex_session_id,
    };
    use gproxy_middleware::{OperationFamily, ProtocolKind};

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

    #[test]
    fn codex_supports_websocket_payload_via_stream_fallback() {
        let payload = serde_json::to_vec(&json!({
            "method": "GET",
            "path": { "endpoint": "responses" },
            "query": {},
            "headers": {},
            "body": {
                "type": "response.create",
                "model": "codex/gpt-5",
                "stream": true
            }
        }))
        .expect("serialize websocket payload");

        let prepared = CodexPreparedRequest::from_payload(
            OperationFamily::OpenAiResponseWebSocket,
            ProtocolKind::OpenAi,
            payload.as_slice(),
        )
        .expect("prepare websocket payload");

        assert_eq!(prepared.method, WreqMethod::POST);
        assert_eq!(prepared.path, "/responses");
        assert_eq!(prepared.model.as_deref(), Some("codex/gpt-5"));
        assert!(prepared.body.is_some());
    }

    #[test]
    fn codex_auto_injects_session_id_from_prompt_cache_key() {
        let payload = serde_json::to_vec(&json!({
            "method": "POST",
            "headers": { "extra": {} },
            "body": {
                "model": "gpt-5.3-codex",
                "prompt_cache_key": "thread-123",
                "input": [{"role": "user", "content": "hello"}]
            }
        }))
        .expect("serialize payload");

        let prepared = CodexPreparedRequest::from_payload(
            OperationFamily::GenerateContent,
            ProtocolKind::OpenAi,
            payload.as_slice(),
        )
        .expect("prepare payload");

        assert!(prepared.extra_headers.iter().any(|(name, value)| {
            name == SESSION_ID_HEADER && value == stable_codex_session_id("thread-123").as_str()
        }));
    }

    #[test]
    fn codex_fallback_session_id_uses_instructions_and_first_input_only() {
        let payload_a = serde_json::to_vec(&json!({
            "method": "POST",
            "headers": { "extra": {} },
            "body": {
                "model": "gpt-5.3-codex",
                "input": [
                    {"role": "system", "content": "be concise"},
                    {"role": "user", "content": "hello"},
                    {"role": "assistant", "content": "draft one"},
                    {"role": "user", "content": "follow up a"}
                ],
                "tools": [{"type": "function", "name": "ignored_tool"}]
            }
        }))
        .expect("serialize payload a");
        let payload_b = serde_json::to_vec(&json!({
            "method": "POST",
            "headers": { "extra": {} },
            "body": {
                "model": "gpt-5.3-codex",
                "input": [
                    {"role": "system", "content": "be concise"},
                    {"role": "user", "content": "hello"},
                    {"role": "assistant", "content": "draft two"},
                    {"role": "user", "content": "follow up b"}
                ],
                "reasoning": {"effort": "high"}
            }
        }))
        .expect("serialize payload b");
        let payload_c = serde_json::to_vec(&json!({
            "method": "POST",
            "headers": { "extra": {} },
            "body": {
                "model": "gpt-5.3-codex",
                "input": [
                    {"role": "system", "content": "be concise"},
                    {"role": "user", "content": "different opener"}
                ]
            }
        }))
        .expect("serialize payload c");

        let prepared_a = CodexPreparedRequest::from_payload(
            OperationFamily::GenerateContent,
            ProtocolKind::OpenAi,
            payload_a.as_slice(),
        )
        .expect("prepare payload a");
        let prepared_b = CodexPreparedRequest::from_payload(
            OperationFamily::GenerateContent,
            ProtocolKind::OpenAi,
            payload_b.as_slice(),
        )
        .expect("prepare payload b");
        let prepared_c = CodexPreparedRequest::from_payload(
            OperationFamily::GenerateContent,
            ProtocolKind::OpenAi,
            payload_c.as_slice(),
        )
        .expect("prepare payload c");

        let session_id_a = prepared_a
            .extra_headers
            .iter()
            .find(|(name, _)| name == SESSION_ID_HEADER)
            .map(|(_, value)| value.as_str())
            .expect("session id a");
        let session_id_b = prepared_b
            .extra_headers
            .iter()
            .find(|(name, _)| name == SESSION_ID_HEADER)
            .map(|(_, value)| value.as_str())
            .expect("session id b");
        let session_id_c = prepared_c
            .extra_headers
            .iter()
            .find(|(name, _)| name == SESSION_ID_HEADER)
            .map(|(_, value)| value.as_str())
            .expect("session id c");

        assert_eq!(session_id_a, session_id_b);
        assert_ne!(session_id_a, session_id_c);
    }

    #[test]
    fn codex_normalizes_session_id_header_name() {
        let payload = serde_json::to_vec(&json!({
            "method": "POST",
            "headers": {
                "extra": {
                    "session-id": "sess-123"
                }
            },
            "body": {
                "model": "gpt-5.3-codex",
                "input": [{"role": "user", "content": "hello"}]
            }
        }))
        .expect("serialize payload");

        let prepared = CodexPreparedRequest::from_payload(
            OperationFamily::GenerateContent,
            ProtocolKind::OpenAi,
            payload.as_slice(),
        )
        .expect("prepare payload");

        assert_eq!(
            prepared.extra_headers,
            vec![(SESSION_ID_HEADER.to_string(), "sess-123".to_string())]
        );
    }
}
