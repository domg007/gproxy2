use gproxy_middleware::{TransformRequest, TransformResponse};
use serde_json::{Map, Value};
use wreq::{Client as WreqClient, Method as WreqMethod};

use super::constants::MODELS_GEMINI_JSON;
use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{UpstreamError, UpstreamResponse};
use crate::channels::utils::{
    default_gproxy_user_agent, gemini_model_list_query_string, is_auth_failure,
    is_transient_server_failure, resolve_user_agent_or_else, retry_after_to_millis, to_wreq_method,
    try_local_gemini_model_response,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

pub fn try_local_vertexexpress_model_response(
    request: &TransformRequest,
) -> Result<Option<TransformResponse>, UpstreamError> {
    let models_doc = load_vertexexpress_models_value()?;
    try_local_gemini_model_response(request, &models_doc)
}

pub async fn execute_vertexexpress_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    if let Some(local) = try_local_vertexexpress_model_response(request)? {
        return Ok(UpstreamResponse::from_local(local));
    }

    let prepared = VertexExpressPreparedRequest::from_transform_request(request)?;
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }

    let state_manager = CredentialStateManager::new(now_unix_ms);
    let method_template = prepared.method.clone();
    let path_template = prepared.path.clone();
    let query_template = prepared.query.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let base_url_template = base_url.to_string();
    let user_agent_template =
        resolve_user_agent_or_else(provider.settings.user_agent(), default_gproxy_user_agent);
    let cache_affinity_hint = if configured_pick_mode_uses_cache(provider.credential_pick_mode) {
        cache_affinity_hint_from_transform_request(request)
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
                ChannelCredential::Builtin(BuiltinChannelCredential::VertexExpress(value)) => {
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
            let path = path_template.clone();
            let query = query_template.clone();
            let body = body_template.clone();
            let model = model_template.clone();
            let base_url = base_url_template.clone();
            let user_agent = user_agent_template.clone();

            async move {
                let url = build_vertexexpress_url(
                    base_url.as_str(),
                    path.as_str(),
                    query.as_deref(),
                    attempt.material.as_str(),
                );
                let mut request_headers = Vec::new();
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

struct VertexExpressPreparedRequest {
    method: WreqMethod,
    path: String,
    query: Option<String>,
    body: Option<Vec<u8>>,
    model: Option<String>,
}

impl VertexExpressPreparedRequest {
    fn from_transform_request(request: &TransformRequest) -> Result<Self, UpstreamError> {
        match request {
            TransformRequest::ModelListGemini(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1beta1/publishers/google/models".to_string(),
                query: gemini_model_list_query_string(
                    value.query.page_size,
                    value.query.page_token.as_deref(),
                ),
                body: None,
                model: None,
            }),
            TransformRequest::ModelGetGemini(value) => {
                let model_id = vertexexpress_model_id(value.path.name.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta1/publishers/google/models/{model_id}"),
                    query: None,
                    body: None,
                    model: Some(format!("models/{model_id}")),
                })
            }
            TransformRequest::CountTokenGemini(value) => {
                let model_id = vertexexpress_model_id(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta1/publishers/google/models/{model_id}:countTokens"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&vertex_count_tokens_payload(
                            model_id.as_str(),
                            &value.body,
                        )?)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(format!("models/{model_id}")),
                })
            }
            TransformRequest::GenerateContentGemini(value) => {
                let model_id = vertexexpress_model_id(value.path.model.as_str());
                let body = vertex_generate_payload(model_id.as_str(), &value.body)?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta1/publishers/google/models/{model_id}:generateContent"),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(format!("models/{model_id}")),
                })
            }
            TransformRequest::StreamGenerateContentGeminiSse(value) => {
                let model_id = vertexexpress_model_id(value.path.model.as_str());
                let body = vertex_generate_payload(model_id.as_str(), &value.body)?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!(
                        "/v1beta1/publishers/google/models/{model_id}:streamGenerateContent"
                    ),
                    query: Some("alt=sse".to_string()),
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(format!("models/{model_id}")),
                })
            }
            TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
                let model_id = vertexexpress_model_id(value.path.model.as_str());
                let body = vertex_generate_payload(model_id.as_str(), &value.body)?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!(
                        "/v1beta1/publishers/google/models/{model_id}:streamGenerateContent"
                    ),
                    query: value.query.alt.as_ref().map(|_| "alt=sse".to_string()),
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(format!("models/{model_id}")),
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn build_vertexexpress_url(
    base_url: &str,
    path: &str,
    query: Option<&str>,
    api_key: &str,
) -> String {
    let mut url = join_vertexexpress_base_url_and_path(base_url, path);
    let mut parts = vec![format!("key={api_key}")];
    if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
        parts.push(query.to_string());
    }
    url.push('?');
    url.push_str(&parts.join("&"));
    url
}

fn join_vertexexpress_base_url_and_path(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let mut path = path.trim_start_matches('/');
    if base.ends_with("/v1") && (path == "v1" || path.starts_with("v1/")) {
        path = path.trim_start_matches("v1/").trim_start_matches("v1");
    }
    if base.ends_with("/v1beta") && (path == "v1beta" || path.starts_with("v1beta/")) {
        path = path
            .trim_start_matches("v1beta/")
            .trim_start_matches("v1beta");
    }
    if base.ends_with("/v1beta1") && (path == "v1beta1" || path.starts_with("v1beta1/")) {
        path = path
            .trim_start_matches("v1beta1/")
            .trim_start_matches("v1beta1");
    }
    format!("{base}/{path}")
}

fn vertex_generate_payload(
    path_model: &str,
    body: &impl serde::Serialize,
) -> Result<Value, UpstreamError> {
    let mut value = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    if let Value::Object(map) = &mut value
        && let Some(model) = map.get("model").and_then(Value::as_str)
    {
        map.insert(
            "model".to_string(),
            Value::String(normalize_vertex_model_ref(model, path_model)),
        );
    }
    Ok(value)
}

fn vertex_count_tokens_payload(
    path_model: &str,
    body: &impl serde::Serialize,
) -> Result<Value, UpstreamError> {
    let body = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let mut out = Map::new();
    out.insert(
        "model".to_string(),
        Value::String(format!("publishers/google/models/{path_model}")),
    );

    if let Some(contents) = body.get("contents") {
        out.insert("contents".to_string(), contents.clone());
    }

    if let Some(generate) = body
        .get("generateContentRequest")
        .and_then(Value::as_object)
    {
        if !out.contains_key("contents")
            && let Some(value) = generate.get("contents")
        {
            out.insert("contents".to_string(), value.clone());
        }
        if let Some(value) = generate.get("tools") {
            out.insert("tools".to_string(), value.clone());
        }
        if let Some(value) = generate.get("toolConfig") {
            out.insert("toolConfig".to_string(), value.clone());
        }
        if let Some(value) = generate.get("safetySettings") {
            out.insert("safetySettings".to_string(), value.clone());
        }
        if let Some(value) = generate.get("systemInstruction") {
            out.insert("systemInstruction".to_string(), value.clone());
        }
        if let Some(value) = generate.get("generationConfig") {
            out.insert("generationConfig".to_string(), value.clone());
        }
        if let Some(cached_content) = generate.get("cachedContent").and_then(Value::as_str) {
            out.insert(
                "cachedContent".to_string(),
                Value::String(cached_content.to_string()),
            );
        }

        let generate_model = generate
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default();
        out.insert(
            "model".to_string(),
            Value::String(normalize_vertex_model_ref(generate_model, path_model)),
        );
    }

    Ok(Value::Object(out))
}

fn normalize_vertex_model_ref(model: &str, fallback_model: &str) -> String {
    let model = model.trim().trim_start_matches('/');
    if model.is_empty() {
        return format!("publishers/google/models/{fallback_model}");
    }
    if model.starts_with("publishers/") && model.contains("/models/") {
        return model.to_string();
    }
    if let Some(id) = model.strip_prefix("models/") {
        return format!("publishers/google/models/{id}");
    }
    if let Some((publisher, id)) = model.split_once('/')
        && !publisher.is_empty()
        && !id.is_empty()
    {
        return format!("publishers/{publisher}/models/{id}");
    }
    format!("publishers/google/models/{model}")
}

fn normalize_vertex_model_name_for_lookup(model: &str) -> String {
    let model = model.trim().trim_start_matches('/');
    if let Some(model) = model.strip_prefix("publishers/google/") {
        return model.to_string();
    }
    if let Some((_, model_id)) = model.split_once("/models/") {
        return format!("models/{model_id}");
    }
    if model.starts_with("models/") {
        return model.to_string();
    }
    format!("models/{model}")
}

fn vertexexpress_model_id(model: &str) -> String {
    normalize_vertex_model_name_for_lookup(model)
        .trim_start_matches("models/")
        .to_string()
}

fn load_vertexexpress_models_value() -> Result<Value, UpstreamError> {
    let parsed: Value = serde_json::from_str(MODELS_GEMINI_JSON)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    if parsed.get("models").and_then(Value::as_array).is_none() {
        return Err(UpstreamError::SerializeRequest(
            "vertexexpress models.gemini.json missing models array".to_string(),
        ));
    }
    Ok(parsed)
}
