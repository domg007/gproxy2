use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::retry::{CredentialRetryDecision, retry_with_eligible_credentials};
use crate::channels::upstream::{UpstreamError, UpstreamResponse};
use crate::channels::utils::{
    default_gproxy_user_agent, gemini_model_list_query_string, is_auth_failure,
    is_transient_server_failure, join_base_url_and_path, retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

pub async fn execute_aistudio_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &gproxy_middleware::TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = AiStudioPreparedRequest::from_transform_request(request)?;
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }
    let path = match prepared.query.as_deref() {
        Some(query) if !query.is_empty() => format!("{}?{query}", prepared.path),
        _ => prepared.path.clone(),
    };
    let url = join_base_url_and_path(base_url, &path);
    let state_manager = CredentialStateManager::new(now_unix_ms);
    let method_template = prepared.method.clone();
    let body_template = prepared.body.clone();
    let url_template = url.clone();
    let model_template = prepared.model.clone();
    let auth_template = prepared.auth_scheme;
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
                ChannelCredential::Builtin(BuiltinChannelCredential::AiStudio(value)) => {
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
            let user_agent = user_agent_template.clone();

            async move {
                let mut request_headers = Vec::new();
                request_headers.push(("user-agent".to_string(), user_agent));

                match auth_scheme {
                    AuthScheme::Bearer => {
                        request_headers.push((
                            "authorization".to_string(),
                            format!("Bearer {}", attempt.material),
                        ));
                    }
                    AuthScheme::XGoogApiKey => {
                        request_headers
                            .push(("x-goog-api-key".to_string(), attempt.material.clone()));
                    }
                };

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

#[derive(Debug, Clone, Copy)]
enum AuthScheme {
    Bearer,
    XGoogApiKey,
}

struct AiStudioPreparedRequest {
    method: WreqMethod,
    path: String,
    query: Option<String>,
    body: Option<Vec<u8>>,
    model: Option<String>,
    auth_scheme: AuthScheme,
}

impl AiStudioPreparedRequest {
    fn from_transform_request(
        request: &gproxy_middleware::TransformRequest,
    ) -> Result<Self, UpstreamError> {
        match request {
            gproxy_middleware::TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1beta/openai/models".to_string(),
                query: None,
                body: None,
                model: None,
                auth_scheme: AuthScheme::Bearer,
            }),
            gproxy_middleware::TransformRequest::ModelGetOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: format!("/v1beta/openai/models/{}", value.path.model),
                query: None,
                body: None,
                model: Some(value.path.model.clone()),
                auth_scheme: AuthScheme::Bearer,
            }),
            gproxy_middleware::TransformRequest::ModelListGemini(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1beta/models".to_string(),
                query: gemini_model_list_query_string(
                    value.query.page_size,
                    value.query.page_token.as_deref(),
                ),
                body: None,
                model: None,
                auth_scheme: AuthScheme::XGoogApiKey,
            }),
            gproxy_middleware::TransformRequest::ModelGetGemini(value) => {
                let name = normalize_gemini_model_name(value.path.name.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{name}"),
                    query: None,
                    body: None,
                    model: Some(name),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            gproxy_middleware::TransformRequest::CountTokenGemini(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:countTokens"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            gproxy_middleware::TransformRequest::GenerateContentGemini(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:generateContent"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentGeminiSse(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                let query = value
                    .query
                    .alt
                    .as_ref()
                    .map(|_| "alt=sse".to_string())
                    .or(Some("alt=sse".to_string()));
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:streamGenerateContent"),
                    query,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                let query = value.query.alt.as_ref().map(|_| "alt=sse".to_string());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:streamGenerateContent"),
                    query,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            gproxy_middleware::TransformRequest::EmbeddingGemini(value) => {
                let model = normalize_gemini_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:embedContent"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            gproxy_middleware::TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1beta/openai/chat/completions".to_string(),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(value.body.model.clone()),
                    auth_scheme: AuthScheme::Bearer,
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentOpenAiChatCompletions(
                value,
            ) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1beta/openai/chat/completions".to_string(),
                query: None,
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
                auth_scheme: AuthScheme::Bearer,
            }),
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn normalize_gemini_model_name(model: &str) -> String {
    if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    }
}
