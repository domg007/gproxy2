use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformResponse};
use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamError, UpstreamResponse, add_or_replace_header, extra_headers_from_payload_value,
    extra_headers_from_transform_request, merge_extra_headers, payload_body_value,
};
use crate::channels::utils::{
    count_openai_input_tokens_with_resolution, default_gproxy_user_agent, is_auth_failure,
    is_transient_server_failure, join_base_url_and_path, resolve_user_agent_or_else,
    retry_after_to_millis, serialize_json_scalar, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, RetryWithPayloadRequest, TokenizerResolutionContext};

pub async fn try_local_nvidia_response(
    _provider: &ProviderDefinition,
    request: &TransformRequest,
    http_client: &WreqClient,
    token_resolution: TokenizerResolutionContext<'_>,
) -> Result<Option<TransformResponse>, UpstreamError> {
    match request {
        TransformRequest::CountTokenOpenAi(value) => {
            let input_tokens = count_openai_input_tokens_with_resolution(
                token_resolution.tokenizer_store,
                http_client,
                token_resolution.hf_token,
                token_resolution.hf_url,
                value.body.model.as_deref(),
                &value.body,
            )
            .await?;
            let response_json = serde_json::json!({
                "stats_code": 200,
                "headers": { "extra": {} },
                "body": {
                    "input_tokens": input_tokens,
                    "object": "response.input_tokens",
                }
            });
            let response = serde_json::from_value(response_json)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(Some(TransformResponse::CountTokenOpenAi(response)))
        }
        _ => Ok(None),
    }
}

pub async fn execute_nvidia_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
    token_resolution: TokenizerResolutionContext<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    if let Some(local_response) =
        try_local_nvidia_response(provider, request, client, token_resolution).await?
    {
        return Ok(UpstreamResponse::from_local(local_response));
    }

    let prepared = NvidiaPreparedRequest::from_transform_request(request)?;
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
    execute_nvidia_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_nvidia_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    payload: RetryWithPayloadRequest<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    if (payload.operation, payload.protocol) == (OperationFamily::CountToken, ProtocolKind::OpenAi)
    {
        let payload_json = serde_json::from_slice::<serde_json::Value>(payload.body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let body_json = payload_body_value(&payload_json);
        let model = body_json
            .get("model")
            .and_then(serde_json::Value::as_str)
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
        let response_json = serde_json::json!({
            "stats_code": 200,
            "headers": { "extra": {} },
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
        NvidiaPreparedRequest::from_payload(payload.operation, payload.protocol, payload.body)?;
    execute_nvidia_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        payload.now_unix_ms,
        None,
    )
    .await
}

async fn execute_nvidia_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: NvidiaPreparedRequest,
    now_unix_ms: u64,
    cache_affinity_hint: Option<crate::channels::retry::CacheAffinityHint>,
) -> Result<UpstreamResponse, UpstreamError> {
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }
    let url = join_base_url_and_path(base_url, &prepared.path);
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let method_template = prepared.method.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let extra_headers_template = prepared.extra_headers.clone();
    let url_template = url.clone();
    let user_agent_template =
        resolve_user_agent_or_else(provider.settings.user_agent(), default_gproxy_user_agent);
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
            match &credential.credential {
                ChannelCredential::Builtin(BuiltinChannelCredential::Nvidia(value)) => {
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
            let extra_headers = extra_headers_template.clone();
            let url = url_template.clone();
            let user_agent = user_agent_template.clone();
            async move {
                let mut request_headers = Vec::new();
                merge_extra_headers(&mut request_headers, &extra_headers);
                add_or_replace_header(
                    &mut request_headers,
                    "authorization",
                    format!("Bearer {}", attempt.material),
                );
                add_or_replace_header(&mut request_headers, "user-agent", user_agent);
                if body.is_some() {
                    add_or_replace_header(&mut request_headers, "content-type", "application/json");
                }
                let send = crate::channels::upstream::tracked_send_request(
                    client,
                    method,
                    url.as_str(),
                    request_headers,
                    body.as_ref().cloned(),
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

struct NvidiaPreparedRequest {
    method: WreqMethod,
    path: String,
    body: Option<Vec<u8>>,
    model: Option<String>,
    extra_headers: Vec<(String, String)>,
}

impl NvidiaPreparedRequest {
    fn from_transform_request(request: &TransformRequest) -> Result<Self, UpstreamError> {
        let extra_headers = extra_headers_from_transform_request(request);
        let mut prepared = match request {
            TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/models".to_string(),
                body: None,
                model: None,
                extra_headers: Vec::new(),
            }),
            TransformRequest::ModelGetOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: format!("/v1/models/{}", value.path.model),
                body: None,
                model: Some(value.path.model.clone()),
                extra_headers: Vec::new(),
            }),
            TransformRequest::GenerateContentOpenAiChatCompletions(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/chat/completions".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
                extra_headers: Vec::new(),
            }),
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/chat/completions".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
                extra_headers: Vec::new(),
            }),
            TransformRequest::EmbeddingOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/embeddings".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(nvidia_embedding_model_to_string(&value.body.model)?),
                extra_headers: Vec::new(),
            }),
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
        fn json_pointer_string(value: &serde_json::Value, pointer: &str) -> Option<String> {
            value
                .pointer(pointer)
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        }

        let payload_value = serde_json::from_slice::<serde_json::Value>(body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let extra_headers = extra_headers_from_payload_value(&payload_value);
        let body_value = payload_body_value(&payload_value);

        match (operation, protocol) {
            (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(Self {
                method: WreqMethod::GET,
                path: "/v1/models".to_string(),
                body: None,
                model: None,
                extra_headers,
            }),
            (OperationFamily::ModelGet, ProtocolKind::OpenAi) => {
                let Some(model) = json_pointer_string(&payload_value, "/path/model") else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in nvidia model_get payload".to_string(),
                    ));
                };
                Ok(Self {
                    method: WreqMethod::GET,
                    path: format!("/v1/models/{model}"),
                    body: None,
                    model: Some(model),
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1/chat/completions".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_value)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: json_pointer_string(&body_value, "/model"),
                    extra_headers,
                })
            }
            (OperationFamily::Embedding, ProtocolKind::OpenAi) => Ok(Self {
                method: WreqMethod::POST,
                path: "/v1/embeddings".to_string(),
                body: Some(
                    serde_json::to_vec(&body_value)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: json_pointer_string(&body_value, "/model"),
                extra_headers,
            }),
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn nvidia_embedding_model_to_string(
    model: &impl serde::Serialize,
) -> Result<String, UpstreamError> {
    serialize_json_scalar(model)
}
