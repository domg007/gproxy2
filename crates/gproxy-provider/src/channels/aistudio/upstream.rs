use serde_json::Value;
use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::retry::{
    CacheAffinityProtocol, CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    cache_affinity_protocol_from_transform_request, configured_pick_mode_uses_cache,
    credential_pick_mode, retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{UpstreamError, UpstreamResponse};
use crate::channels::utils::{
    default_gproxy_user_agent, gemini_model_list_query_string, is_auth_failure,
    is_transient_server_failure, join_base_url_and_path, resolve_user_agent_or_else,
    retry_after_to_millis, serialize_json_scalar, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;
use gproxy_middleware::{OperationFamily, ProtocolKind};
use url::form_urlencoded;

pub async fn execute_aistudio_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &gproxy_middleware::TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let cache_protocol = cache_affinity_protocol_from_transform_request(request);
    let prepared = AiStudioPreparedRequest::from_transform_request(request)?;
    execute_aistudio_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        cache_protocol,
        now_unix_ms,
    )
    .await
}

pub async fn execute_aistudio_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = AiStudioPreparedRequest::from_payload(operation, protocol, body)?;
    let cache_protocol = cache_affinity_protocol_from_operation_protocol(operation, protocol);
    execute_aistudio_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        cache_protocol,
        now_unix_ms,
    )
    .await
}

async fn execute_aistudio_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: AiStudioPreparedRequest,
    cache_protocol: Option<CacheAffinityProtocol>,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
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
        prepared.model.as_deref(),
        now_unix_ms,
        pick_mode,
        cache_affinity_hint,
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

fn cache_affinity_protocol_from_operation_protocol(
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> Option<CacheAffinityProtocol> {
    match (operation, protocol) {
        (OperationFamily::GenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
            Some(CacheAffinityProtocol::GeminiGenerateContent)
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
            Some(CacheAffinityProtocol::OpenAiChatCompletions)
        }
        _ => None,
    }
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
            gproxy_middleware::TransformRequest::GeminiLive(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: gemini_live_rpc_path(&value.path.rpc)?,
                query: gemini_live_query_string(
                    value.query.key.as_deref(),
                    value.query.access_token.as_deref(),
                ),
                body: None,
                model: value
                    .body
                    .as_ref()
                    .and_then(gemini_live_setup_model_from_body)
                    .map(|model| normalize_gemini_model_name(model.as_str())),
                auth_scheme: AuthScheme::XGoogApiKey,
            }),
            _ => Err(UpstreamError::UnsupportedRequest),
        }
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

        fn parse_gemini_payload_wrapper(
            payload: &[u8],
        ) -> Result<(String, Value, Option<String>), UpstreamError> {
            let value = serde_json::from_slice::<Value>(payload)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            let Some(model) = value
                .pointer("/path/model")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
            else {
                return Err(UpstreamError::SerializeRequest(
                    "missing path.model in Gemini payload".to_string(),
                ));
            };
            let Some(body_value) = value.get("body").cloned() else {
                return Err(UpstreamError::SerializeRequest(
                    "missing body in Gemini payload".to_string(),
                ));
            };
            let alt = value
                .pointer("/query/alt")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            Ok((model, body_value, alt))
        }

        match (operation, protocol) {
            (OperationFamily::CountToken, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(body)?;
                let model = normalize_gemini_model_name(model.as_str());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: format!("/v1beta/{model}:countTokens"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body_value)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(body)?;
                let model = normalize_gemini_model_name(model.as_str());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: format!("/v1beta/{model}:generateContent"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body_value)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
                let (model, body_value, alt) = parse_gemini_payload_wrapper(body)?;
                let model = normalize_gemini_model_name(model.as_str());
                let query = match protocol {
                    ProtocolKind::Gemini => {
                        Some(format!("alt={}", alt.unwrap_or_else(|| "sse".to_string())))
                    }
                    ProtocolKind::GeminiNDJson => alt.map(|value| format!("alt={value}")),
                    _ => None,
                };
                Ok(Self {
                    method: WreqMethod::POST,
                    path: format!("/v1beta/{model}:streamGenerateContent"),
                    query,
                    body: Some(
                        serde_json::to_vec(&body_value)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            (OperationFamily::Embedding, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(body)?;
                let model = normalize_gemini_model_name(model.as_str());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: format!("/v1beta/{model}:embedContent"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body_value)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1beta/openai/chat/completions".to_string(),
                    query: None,
                    body: Some(body.to_vec()),
                    model: serde_json::from_slice::<Value>(body)
                        .ok()
                        .and_then(|value| json_pointer_string(&value, "/model")),
                    auth_scheme: AuthScheme::Bearer,
                })
            }
            (OperationFamily::GeminiLive, ProtocolKind::Gemini) => {
                let request = serde_json::from_slice::<Value>(body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                let method = request
                    .pointer("/method")
                    .and_then(Value::as_str)
                    .unwrap_or("GET")
                    .to_string();
                let rpc = request
                    .pointer("/path/rpc")
                    .and_then(Value::as_str)
                    .unwrap_or(
                        "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent",
                    )
                    .to_string();
                let key = request.pointer("/query/key").and_then(Value::as_str);
                let access_token = request
                    .pointer("/query/access_token")
                    .and_then(Value::as_str);
                let model = request
                    .pointer("/body/setup/model")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                Ok(Self {
                    method: to_wreq_method(&method)?,
                    path: gemini_live_rpc_path(&rpc)?,
                    query: gemini_live_query_string(key, access_token),
                    body: None,
                    model: model.map(|model| normalize_gemini_model_name(model.as_str())),
                    auth_scheme: AuthScheme::XGoogApiKey,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn gemini_live_rpc_path(rpc: &impl serde::Serialize) -> Result<String, UpstreamError> {
    let rpc = serialize_json_scalar(rpc)?;
    Ok(format!("/ws/{rpc}"))
}

fn gemini_live_query_string(key: Option<&str>, access_token: Option<&str>) -> Option<String> {
    let mut has_query = false;
    let mut serializer = form_urlencoded::Serializer::new(String::new());

    if let Some(key) = key.map(str::trim).filter(|value| !value.is_empty()) {
        serializer.append_pair("key", key);
        has_query = true;
    }
    if let Some(access_token) = access_token
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        serializer.append_pair("access_token", access_token);
        has_query = true;
    }

    if has_query {
        Some(serializer.finish())
    } else {
        None
    }
}

fn gemini_live_setup_model_from_body(body: &impl serde::Serialize) -> Option<String> {
    serde_json::to_value(body).ok().and_then(|value| {
        value
            .pointer("/setup/model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    })
}

fn normalize_gemini_model_name(model: &str) -> String {
    if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_live_request_json() -> Value {
        json!({
            "method": "GET",
            "path": {
                "rpc": "google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContentConstrained"
            },
            "query": {
                "key": "api key",
                "access_token": "token/abc"
            },
            "headers": {
                "Authorization": "Token abc"
            },
            "body": {
                "setup": {
                    "model": "gemini-2.5-flash"
                }
            }
        })
    }

    #[test]
    fn gemini_live_transform_request_maps_to_ws_path_and_query() {
        let payload = serde_json::to_vec(&sample_live_request_json()).expect("serialize request");
        let request = gproxy_middleware::decode_request_payload(
            OperationFamily::GeminiLive,
            ProtocolKind::Gemini,
            payload.as_slice(),
        )
        .expect("decode request");

        let prepared =
            AiStudioPreparedRequest::from_transform_request(&request).expect("prepare request");

        assert_eq!(prepared.method, WreqMethod::GET);
        assert_eq!(
            prepared.path,
            "/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContentConstrained"
        );
        assert_eq!(
            prepared.query.as_deref(),
            Some("key=api+key&access_token=token%2Fabc")
        );
        assert_eq!(prepared.model.as_deref(), Some("models/gemini-2.5-flash"));
        assert!(prepared.body.is_none());
    }

    #[test]
    fn gemini_live_payload_maps_to_ws_path_and_query() {
        let payload = serde_json::to_vec(&sample_live_request_json()).expect("serialize request");
        let prepared = AiStudioPreparedRequest::from_payload(
            OperationFamily::GeminiLive,
            ProtocolKind::Gemini,
            payload.as_slice(),
        )
        .expect("prepare payload");

        assert_eq!(prepared.method, WreqMethod::GET);
        assert_eq!(
            prepared.path,
            "/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContentConstrained"
        );
        assert_eq!(
            prepared.query.as_deref(),
            Some("key=api+key&access_token=token%2Fabc")
        );
        assert_eq!(prepared.model.as_deref(), Some("models/gemini-2.5-flash"));
        assert!(prepared.body.is_none());
    }
}
