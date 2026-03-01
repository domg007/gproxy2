use gproxy_middleware::{TransformRequest, TransformResponse};
use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::retry::{CredentialRetryDecision, retry_with_eligible_credentials};
use crate::channels::upstream::{UpstreamError, UpstreamResponse};
use crate::channels::utils::{
    count_openai_input_tokens_with_resolution, default_gproxy_user_agent, is_auth_failure,
    is_transient_server_failure, join_base_url_and_path, resolve_user_agent_or_else,
    retry_after_to_millis, serialize_json_scalar, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, TokenizerResolutionContext};

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
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }
    let url = join_base_url_and_path(base_url, &prepared.path);
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let method_template = prepared.method.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let url_template = url.clone();
    let user_agent_template =
        resolve_user_agent_or_else(provider.settings.user_agent(), default_gproxy_user_agent);

    retry_with_eligible_credentials(
        provider,
        credential_states,
        prepared.model.as_deref(),
        now_unix_ms,
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
            let url = url_template.clone();
            let user_agent = user_agent_template.clone();
            async move {
                let mut request_headers = vec![(
                    "authorization".to_string(),
                    format!("Bearer {}", attempt.material),
                )];
                request_headers.push(("user-agent".to_string(), user_agent));
                if body.is_some() {
                    request_headers
                        .push(("content-type".to_string(), "application/json".to_string()));
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
}

impl NvidiaPreparedRequest {
    fn from_transform_request(request: &TransformRequest) -> Result<Self, UpstreamError> {
        match request {
            TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/models".to_string(),
                body: None,
                model: None,
            }),
            TransformRequest::ModelGetOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: format!("/v1/models/{}", value.path.model),
                body: None,
                model: Some(value.path.model.clone()),
            }),
            TransformRequest::GenerateContentOpenAiChatCompletions(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/chat/completions".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
            }),
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/chat/completions".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
            }),
            TransformRequest::EmbeddingOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/embeddings".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(nvidia_embedding_model_to_string(&value.body.model)?),
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
