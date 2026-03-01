use gproxy_middleware::{TransformRequest, TransformResponse};
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::{MODEL_CHAT, MODEL_REASONER};
use crate::channels::retry::{CredentialRetryDecision, retry_with_eligible_credentials};
use crate::channels::upstream::{UpstreamError, UpstreamResponse};
use crate::channels::utils::{
    anthropic_header_pairs, claude_model_to_string, count_openai_input_tokens_with_resolution,
    default_gproxy_user_agent, is_auth_failure, is_transient_server_failure,
    join_base_url_and_path, retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, TokenizerResolutionContext};

pub async fn try_local_deepseek_response(
    _provider: &ProviderDefinition,
    request: &TransformRequest,
    http_client: &WreqClient,
    token_resolution: TokenizerResolutionContext<'_>,
) -> Result<Option<TransformResponse>, UpstreamError> {
    match request {
        TransformRequest::ModelListOpenAi(_) => {
            let response_json = serde_json::json!({
                "stats_code": 200,
                "headers": {},
                "body": {
                    "object": "list",
                    "data": deepseek_models_json(),
                }
            });
            let response = serde_json::from_value(response_json)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(Some(TransformResponse::ModelListOpenAi(response)))
        }
        TransformRequest::ModelGetOpenAi(value) => {
            let requested = normalize_deepseek_model_name(value.path.model.as_str());
            let found = deepseek_models_json().into_iter().find(|model| {
                model
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .map(|id| id == requested.as_str())
                    .unwrap_or(false)
            });
            if let Some(found) = found {
                let response_json = serde_json::json!({
                    "stats_code": 200,
                    "headers": {},
                    "body": found,
                });
                let response = serde_json::from_value(response_json)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                return Ok(Some(TransformResponse::ModelGetOpenAi(response)));
            }
            let response_json = serde_json::json!({
                "stats_code": 404,
                "headers": {},
                "body": {
                    "error": {
                        "message": format!("model {requested} not found"),
                        "type": "invalid_request_error",
                        "param": "model",
                        "code": "model_not_found",
                    }
                }
            });
            let response = serde_json::from_value(response_json)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(Some(TransformResponse::ModelGetOpenAi(response)))
        }
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
        _ => Ok(None),
    }
}

pub async fn execute_deepseek_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
    token_resolution: TokenizerResolutionContext<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    if let Some(local_response) =
        try_local_deepseek_response(provider, request, client, token_resolution).await?
    {
        return Ok(UpstreamResponse::from_local(local_response));
    }

    let prepared = DeepseekPreparedRequest::from_transform_request(request)?;
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
    let auth_template = prepared.auth_scheme;
    let request_headers_template = prepared.request_headers.clone();
    let user_agent_template = provider
        .settings
        .user_agent()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(default_gproxy_user_agent);

    retry_with_eligible_credentials(
        provider,
        credential_states,
        prepared.model.as_deref(),
        now_unix_ms,
        |credential| {
            match &credential.credential {
                ChannelCredential::Builtin(BuiltinChannelCredential::Deepseek(value)) => {
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
            let auth_scheme = auth_template;
            let request_headers = request_headers_template.clone();
            let user_agent = user_agent_template.clone();

            async move {
                let mut sent_headers = Vec::new();
                sent_headers.push(("user-agent".to_string(), user_agent));
                match auth_scheme {
                    AuthScheme::Bearer => {
                        sent_headers.push((
                            "authorization".to_string(),
                            format!("Bearer {}", attempt.material),
                        ));
                    }
                    AuthScheme::XApiKey => {
                        sent_headers.push(("x-api-key".to_string(), attempt.material.clone()));
                    }
                };

                for (name, value) in &request_headers {
                    sent_headers.push((name.clone(), value.clone()));
                }
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

#[derive(Debug, Clone, Copy)]
enum AuthScheme {
    Bearer,
    XApiKey,
}

struct DeepseekPreparedRequest {
    method: WreqMethod,
    path: String,
    body: Option<Vec<u8>>,
    model: Option<String>,
    auth_scheme: AuthScheme,
    request_headers: Vec<(String, String)>,
}

impl DeepseekPreparedRequest {
    fn from_transform_request(request: &TransformRequest) -> Result<Self, UpstreamError> {
        match request {
            TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
                let mut body = value.body.clone();
                body.model = normalize_deepseek_model_name(body.model.as_str());
                if let Some(max_tokens) = body.max_tokens {
                    body.max_tokens = Some(max_tokens.min(8192));
                }
                if let Some(max_completion_tokens) = body.max_completion_tokens {
                    body.max_completion_tokens = Some(max_completion_tokens.min(8192));
                }
                let mut body_json = serde_json::to_value(&body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_deepseek_chat_message_roles(&mut body_json);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/chat/completions".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(body.model.clone()),
                    auth_scheme: AuthScheme::Bearer,
                    request_headers: Vec::new(),
                })
            }
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
                let mut body = value.body.clone();
                body.model = normalize_deepseek_model_name(body.model.as_str());
                if let Some(max_tokens) = body.max_tokens {
                    body.max_tokens = Some(max_tokens.min(8192));
                }
                if let Some(max_completion_tokens) = body.max_completion_tokens {
                    body.max_completion_tokens = Some(max_completion_tokens.min(8192));
                }
                let mut body_json = serde_json::to_value(&body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_deepseek_chat_message_roles(&mut body_json);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/chat/completions".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(body.model.clone()),
                    auth_scheme: AuthScheme::Bearer,
                    request_headers: Vec::new(),
                })
            }
            TransformRequest::GenerateContentClaude(value) => {
                let model = normalize_deepseek_model_name(
                    claude_model_to_string(&value.body.model)?.as_str(),
                );
                let mut body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                if let Some(map) = body.as_object_mut() {
                    map.insert(
                        "model".to_string(),
                        serde_json::Value::String(model.clone()),
                    );
                }
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/anthropic/v1/messages".to_string(),
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XApiKey,
                    request_headers: anthropic_header_pairs(
                        &value.headers.anthropic_version,
                        value.headers.anthropic_beta.as_ref(),
                    )?,
                })
            }
            TransformRequest::StreamGenerateContentClaude(value) => {
                let model = normalize_deepseek_model_name(
                    claude_model_to_string(&value.body.model)?.as_str(),
                );
                let mut body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                if let Some(map) = body.as_object_mut() {
                    map.insert(
                        "model".to_string(),
                        serde_json::Value::String(model.clone()),
                    );
                }
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/anthropic/v1/messages".to_string(),
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XApiKey,
                    request_headers: anthropic_header_pairs(
                        &value.headers.anthropic_version,
                        value.headers.anthropic_beta.as_ref(),
                    )?,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn deepseek_models_json() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "id": MODEL_CHAT,
            "created": 0,
            "object": "model",
            "owned_by": "deepseek",
        }),
        serde_json::json!({
            "id": MODEL_REASONER,
            "created": 0,
            "object": "model",
            "owned_by": "deepseek",
        }),
    ]
}

fn normalize_deepseek_model_name(model: &str) -> String {
    let model = model.trim().trim_start_matches('/').trim();
    let model = model.strip_prefix("models/").unwrap_or(model);
    match model {
        "" => MODEL_CHAT.to_string(),
        "chat" | MODEL_CHAT => MODEL_CHAT.to_string(),
        "reasoner" | "resaoner" | MODEL_REASONER | "deepseek-resaoner" => {
            MODEL_REASONER.to_string()
        }
        _ => MODEL_CHAT.to_string(),
    }
}

fn normalize_deepseek_chat_message_roles(body_json: &mut serde_json::Value) {
    let Some(messages) = body_json
        .get_mut("messages")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };

    for message in messages {
        if let Some(object) = message.as_object_mut() {
            let is_developer = object
                .get("role")
                .and_then(serde_json::Value::as_str)
                .map(|role| role.eq_ignore_ascii_case("developer"))
                .unwrap_or(false);
            if is_developer {
                object.insert(
                    "role".to_string(),
                    serde_json::Value::String("system".to_string()),
                );
            }
        }
    }
}
