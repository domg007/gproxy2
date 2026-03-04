use serde_json::Value;
use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::cache_control::{
    CacheBreakpointRule, apply_magic_string_cache_control_triggers, ensure_cache_breakpoint_rules,
};
use crate::channels::retry::{
    CacheAffinityProtocol, CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    cache_affinity_protocol_from_transform_request, configured_pick_mode_uses_cache,
    credential_pick_mode, retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{UpstreamError, UpstreamResponse};
use crate::channels::utils::{
    anthropic_header_pairs, append_query_param_if_missing, claude_model_list_query_string,
    claude_model_to_string, default_gproxy_user_agent, is_auth_failure,
    is_transient_server_failure, join_base_url_and_path, resolve_user_agent_or_else,
    retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;
use gproxy_middleware::{OperationFamily, ProtocolKind};

const ANTHROPIC_DEFAULT_VERSION: &str = "2023-06-01";
const BETA_QUERY_KEY: &str = "beta";
const BETA_QUERY_VALUE: &str = "true";

pub async fn execute_claude_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &gproxy_middleware::TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let cache_protocol = cache_affinity_protocol_from_transform_request(request);
    let prepared = ClaudePreparedRequest::from_transform_request(
        request,
        provider.settings.cache_breakpoints(),
    )?;
    execute_claude_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        cache_protocol,
        now_unix_ms,
    )
    .await
}

pub async fn execute_claude_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = ClaudePreparedRequest::from_payload(
        operation,
        protocol,
        body,
        provider.settings.cache_breakpoints(),
    )?;
    let cache_protocol = cache_affinity_protocol_from_operation_protocol(operation, protocol);
    execute_claude_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        cache_protocol,
        now_unix_ms,
    )
    .await
}

async fn execute_claude_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: ClaudePreparedRequest,
    cache_protocol: Option<CacheAffinityProtocol>,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }
    let url = join_base_url_and_path(base_url, &prepared.path);
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let model_for_selection = prepared.model.clone();
    let method_template = prepared.method.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let url_template = url.clone();
    let request_headers_template = prepared.request_headers.clone();
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
        provider,
        credential_states,
        model_for_selection.as_deref(),
        now_unix_ms,
        pick_mode,
        cache_affinity_hint,
        |credential| {
            match &credential.credential {
                ChannelCredential::Builtin(BuiltinChannelCredential::Claude(value)) => {
                    Some(value.api_key.as_str())
                }
                _ => None,
            }
            .map(str::trim)
            .filter(|api_key| !api_key.is_empty())
            .map(ToOwned::to_owned)
        },
        |attempt| {
            let method = method_template.clone();
            let body = body_template.clone();
            let model = model_template.clone();
            let url = url_template.clone();
            let request_headers = request_headers_template.clone();
            let user_agent = user_agent_template.clone();

            async move {
                let mut sent_headers = vec![("x-api-key".to_string(), attempt.material.clone())];
                sent_headers.push(("user-agent".to_string(), user_agent));
                sent_headers.extend(request_headers.iter().cloned());
                if body.is_some() {
                    sent_headers.push(("content-type".to_string(), "application/json".to_string()));
                }

                let send = crate::channels::upstream::tracked_send_request(
                    client,
                    method,
                    url.as_str(),
                    sent_headers.clone(),
                    body.clone(),
                )
                .await;
                match send {
                    Ok((response, request_meta)) => {
                        let status = response.status();
                        if status.is_success() {
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
                                .with_request_meta(request_meta),
                            );
                        }

                        let status_code = status.as_u16();
                        if is_auth_failure(status_code) {
                            let message = format!("upstream status {status_code}");
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
                            UpstreamResponse::from_http(
                                attempt.credential_id,
                                attempt.attempts,
                                response,
                            )
                            .with_request_meta(request_meta),
                        )
                    }
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
                        CredentialRetryDecision::Retry {
                            last_status: None,
                            last_error: Some(message),
                            last_request_meta: None,
                        }
                    }
                }
            }
        },
    )
    .await
}

fn cache_affinity_protocol_from_operation_protocol(
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> Option<CacheAffinityProtocol> {
    match (operation, protocol) {
        (OperationFamily::GenerateContent, ProtocolKind::Claude)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
            Some(CacheAffinityProtocol::ClaudeMessages)
        }
        _ => None,
    }
}

struct ClaudePreparedRequest {
    method: WreqMethod,
    path: String,
    body: Option<Vec<u8>>,
    model: Option<String>,
    request_headers: Vec<(String, String)>,
}

impl ClaudePreparedRequest {
    fn from_transform_request(
        request: &gproxy_middleware::TransformRequest,
        cache_breakpoints: &[CacheBreakpointRule],
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
                path =
                    append_query_param_if_missing(path.as_str(), BETA_QUERY_KEY, BETA_QUERY_VALUE);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path,
                    body: None,
                    model: None,
                    request_headers: anthropic_header_pairs(
                        &value.headers.anthropic_version,
                        value.headers.anthropic_beta.as_ref(),
                    )?,
                })
            }
            gproxy_middleware::TransformRequest::ModelGetClaude(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: append_query_param_if_missing(
                    format!("/v1/models/{}", value.path.model_id).as_str(),
                    BETA_QUERY_KEY,
                    BETA_QUERY_VALUE,
                ),
                body: None,
                model: Some(value.path.model_id.clone()),
                request_headers: anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?,
            }),
            gproxy_middleware::TransformRequest::CountTokenClaude(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: append_query_param_if_missing(
                    "/v1/messages/count_tokens",
                    BETA_QUERY_KEY,
                    BETA_QUERY_VALUE,
                ),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(claude_model_to_string(&value.body.model)?),
                request_headers: anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?,
            }),
            gproxy_middleware::TransformRequest::GenerateContentClaude(value) => {
                let mut body_json = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                apply_magic_string_cache_control_triggers(&mut body_json);
                if !cache_breakpoints.is_empty() {
                    ensure_cache_breakpoint_rules(&mut body_json, cache_breakpoints);
                }
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
                    model: Some(claude_model_to_string(&value.body.model)?),
                    request_headers: anthropic_header_pairs(
                        &value.headers.anthropic_version,
                        value.headers.anthropic_beta.as_ref(),
                    )?,
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentClaude(value) => {
                let mut body_json = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                apply_magic_string_cache_control_triggers(&mut body_json);
                if !cache_breakpoints.is_empty() {
                    ensure_cache_breakpoint_rules(&mut body_json, cache_breakpoints);
                }
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
                    model: Some(claude_model_to_string(&value.body.model)?),
                    request_headers: anthropic_header_pairs(
                        &value.headers.anthropic_version,
                        value.headers.anthropic_beta.as_ref(),
                    )?,
                })
            }
            gproxy_middleware::TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: append_query_param_if_missing("/v1/models", BETA_QUERY_KEY, BETA_QUERY_VALUE),
                body: None,
                model: None,
                request_headers: anthropic_header_pairs(
                    &ANTHROPIC_DEFAULT_VERSION,
                    Option::<&Vec<String>>::None,
                )?,
            }),
            gproxy_middleware::TransformRequest::ModelGetOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: append_query_param_if_missing(
                    format!("/v1/models/{}", value.path.model).as_str(),
                    BETA_QUERY_KEY,
                    BETA_QUERY_VALUE,
                ),
                body: None,
                model: Some(value.path.model.clone()),
                request_headers: anthropic_header_pairs(
                    &ANTHROPIC_DEFAULT_VERSION,
                    Option::<&Vec<String>>::None,
                )?,
            }),
            gproxy_middleware::TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: append_query_param_if_missing(
                        "/v1/chat/completions",
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(value.body.model.clone()),
                    request_headers: anthropic_header_pairs(
                        &ANTHROPIC_DEFAULT_VERSION,
                        Option::<&Vec<String>>::None,
                    )?,
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentOpenAiChatCompletions(
                value,
            ) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: append_query_param_if_missing(
                    "/v1/chat/completions",
                    BETA_QUERY_KEY,
                    BETA_QUERY_VALUE,
                ),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
                request_headers: anthropic_header_pairs(
                    &ANTHROPIC_DEFAULT_VERSION,
                    Option::<&Vec<String>>::None,
                )?,
            }),
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }

    fn from_payload(
        operation: OperationFamily,
        protocol: ProtocolKind,
        body: &[u8],
        cache_breakpoints: &[CacheBreakpointRule],
    ) -> Result<Self, UpstreamError> {
        fn json_pointer_string(value: &Value, pointer: &str) -> Option<String> {
            value
                .pointer(pointer)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        }

        fn parse_claude_payload_wrapper(
            payload: &[u8],
        ) -> Result<(Value, String, Option<Vec<String>>), UpstreamError> {
            let value = serde_json::from_slice::<Value>(payload)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            if let Some(body_value) = value.get("body").cloned() {
                let version = value
                    .pointer("/headers/anthropic_version")
                    .and_then(Value::as_str)
                    .unwrap_or(ANTHROPIC_DEFAULT_VERSION)
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
            Ok((value, ANTHROPIC_DEFAULT_VERSION.to_string(), None))
        }

        match (operation, protocol) {
            (OperationFamily::CountToken, ProtocolKind::Claude) => {
                let (body_json, version, beta) = parse_claude_payload_wrapper(body)?;
                Ok(Self {
                    method: WreqMethod::POST,
                    path: append_query_param_if_missing(
                        "/v1/messages/count_tokens",
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    model: json_pointer_string(&body_json, "/model"),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    request_headers: anthropic_header_pairs(&version, beta.as_ref())?,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::Claude)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
                let (mut body_json, version, beta) = parse_claude_payload_wrapper(body)?;
                apply_magic_string_cache_control_triggers(&mut body_json);
                if !cache_breakpoints.is_empty() {
                    ensure_cache_breakpoint_rules(&mut body_json, cache_breakpoints);
                }
                Ok(Self {
                    method: WreqMethod::POST,
                    path: append_query_param_if_missing(
                        "/v1/messages",
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    model: json_pointer_string(&body_json, "/model"),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    request_headers: anthropic_header_pairs(&version, beta.as_ref())?,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                let model = serde_json::from_slice::<Value>(body)
                    .ok()
                    .and_then(|value| json_pointer_string(&value, "/model"));
                Ok(Self {
                    method: WreqMethod::POST,
                    path: append_query_param_if_missing(
                        "/v1/chat/completions",
                        BETA_QUERY_KEY,
                        BETA_QUERY_VALUE,
                    ),
                    body: Some(body.to_vec()),
                    model,
                    request_headers: anthropic_header_pairs(
                        &ANTHROPIC_DEFAULT_VERSION,
                        Option::<&Vec<String>>::None,
                    )?,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}
