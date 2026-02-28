use gproxy_middleware::{TransformRequest, TransformResponse};
use serde_json::{Map, Value, json};
use wreq::{Client as WreqClient, Method as WreqMethod, Response as WreqResponse};

use super::constants::DEFAULT_LOCATION;
use super::oauth::{resolve_vertex_access_token, vertex_auth_material_from_credential};
use crate::channels::retry::{CredentialRetryDecision, retry_with_eligible_credentials};
use crate::channels::upstream::{
    UpstreamCredentialUpdate, UpstreamError, UpstreamRequestMeta, UpstreamResponse,
};
use crate::channels::utils::{
    gemini_model_list_query_string, is_auth_failure, is_transient_server_failure,
    join_base_url_and_path, retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;

pub async fn execute_vertex_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared = VertexPreparedRequest::from_transform_request(request)?;
    let base_url = provider.settings.base_url().trim();
    if base_url.is_empty() {
        return Err(UpstreamError::InvalidBaseUrl);
    }

    let state_manager = CredentialStateManager::new(now_unix_ms);
    let method_template = prepared.method.clone();
    let endpoint_template = prepared.endpoint.clone();
    let query_template = prepared.query.clone();
    let body_template = prepared.body.clone();
    let model_template = prepared.model.clone();
    let model_response_kind_template = prepared.model_response_kind;
    let base_url_template = base_url.to_string();
    let location_template = DEFAULT_LOCATION.to_string();

    retry_with_eligible_credentials(
        provider,
        credential_states,
        prepared.model.as_deref(),
        now_unix_ms,
        |credential| {
            if let ChannelCredential::Builtin(BuiltinChannelCredential::Vertex(value)) =
                &credential.credential
            {
                return vertex_auth_material_from_credential(value, &provider.settings);
            }
            None
        },
        |attempt| {
            let method = method_template.clone();
            let endpoint = endpoint_template.clone();
            let query = query_template.clone();
            let body = body_template.clone();
            let model = model_template.clone();
            let model_response_kind = model_response_kind_template;
            let base_url = base_url_template.clone();
            let location = location_template.clone();

            async move {
                let path = build_vertex_path(
                    endpoint,
                    attempt.material.project_id.as_str(),
                    location.as_str(),
                );
                let path_with_query = match query.as_deref() {
                    Some(query) if !query.is_empty() => format!("{path}?{query}"),
                    _ => path,
                };
                let url = join_base_url_and_path(base_url.as_str(), path_with_query.as_str());
                let token_cache_key =
                    format!("{}::{}", provider.channel.as_str(), attempt.credential_id);
                let mut credential_update: Option<UpstreamCredentialUpdate> = None;

                let resolved_access_token = match resolve_vertex_access_token(
                    client,
                    token_cache_key.as_str(),
                    &attempt.material,
                    now_unix_ms,
                    false,
                )
                .await
                {
                    Ok(token) => token,
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
                if let Some(refreshed) = resolved_access_token.refreshed.as_ref() {
                    credential_update = Some(vertex_credential_update(
                        attempt.credential_id,
                        refreshed.access_token.as_str(),
                        refreshed.expires_at_unix_ms,
                    ));
                } else if attempt.material.access_token != resolved_access_token.access_token
                    || attempt.material.expires_at_unix_ms
                        != resolved_access_token.expires_at_unix_ms
                {
                    credential_update = Some(vertex_credential_update(
                        attempt.credential_id,
                        resolved_access_token.access_token.as_str(),
                        resolved_access_token.expires_at_unix_ms,
                    ));
                }
                let (mut response, mut request_meta) = match send_vertex_request(
                    client,
                    &method,
                    url.as_str(),
                    resolved_access_token.access_token.as_str(),
                    &body,
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

                if response.status().is_success() {
                    if let Some(kind) = model_response_kind {
                        let local = match normalize_vertex_model_response(response, kind).await {
                            Ok(local) => local,
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
                        state_manager.mark_success(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                        );
                        return CredentialRetryDecision::Return(
                            UpstreamResponse::from_local(local)
                                .with_request_meta(request_meta.clone())
                                .with_credential_update(credential_update.clone()),
                        );
                    }
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
                        .with_request_meta(request_meta.clone())
                        .with_credential_update(credential_update.clone()),
                    );
                }

                if is_auth_failure(response.status().as_u16()) {
                    let refreshed_token = match resolve_vertex_access_token(
                        client,
                        token_cache_key.as_str(),
                        &attempt.material,
                        now_unix_ms,
                        true,
                    )
                    .await
                    {
                        Ok(token) => token,
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
                                last_status: Some(401),
                                last_error: Some(message),
                                last_request_meta: None,
                            };
                        }
                    };
                    if let Some(refreshed) = refreshed_token.refreshed.as_ref() {
                        credential_update = Some(vertex_credential_update(
                            attempt.credential_id,
                            refreshed.access_token.as_str(),
                            refreshed.expires_at_unix_ms,
                        ));
                    } else if attempt.material.access_token != refreshed_token.access_token
                        || attempt.material.expires_at_unix_ms != refreshed_token.expires_at_unix_ms
                    {
                        credential_update = Some(vertex_credential_update(
                            attempt.credential_id,
                            refreshed_token.access_token.as_str(),
                            refreshed_token.expires_at_unix_ms,
                        ));
                    }
                    (response, request_meta) = match send_vertex_request(
                        client,
                        &method,
                        url.as_str(),
                        refreshed_token.access_token.as_str(),
                        &body,
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

                    if response.status().is_success() {
                        if let Some(kind) = model_response_kind {
                            let local = match normalize_vertex_model_response(response, kind).await
                            {
                                Ok(local) => local,
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
                            state_manager.mark_success(
                                credential_states,
                                &provider.channel,
                                attempt.credential_id,
                            );
                            return CredentialRetryDecision::Return(
                                UpstreamResponse::from_local(local)
                                    .with_request_meta(request_meta.clone())
                                    .with_credential_update(credential_update.clone()),
                            );
                        }
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
                            .with_request_meta(request_meta.clone())
                            .with_credential_update(credential_update.clone()),
                        );
                    }

                    if is_auth_failure(response.status().as_u16()) {
                        let message = format!(
                            "upstream status {} after access token refresh",
                            response.status().as_u16()
                        );
                        state_manager.mark_auth_dead(
                            credential_states,
                            &provider.channel,
                            attempt.credential_id,
                            Some(message.clone()),
                        );
                        return CredentialRetryDecision::Retry {
                            last_status: Some(response.status().as_u16()),
                            last_error: Some(message),
                            last_request_meta: None,
                        };
                    }
                }

                let status_code = response.status().as_u16();
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
                    UpstreamResponse::from_http(attempt.credential_id, attempt.attempts, response)
                        .with_request_meta(request_meta)
                        .with_credential_update(credential_update.clone()),
                )
            }
        },
    )
    .await
}

fn vertex_credential_update(
    credential_id: i64,
    access_token: &str,
    expires_at_unix_ms: u64,
) -> UpstreamCredentialUpdate {
    UpstreamCredentialUpdate::VertexTokenRefresh {
        credential_id,
        access_token: access_token.to_string(),
        expires_at_unix_ms,
    }
}

async fn send_vertex_request(
    client: &WreqClient,
    method: &WreqMethod,
    url: &str,
    access_token: &str,
    body: &Option<Vec<u8>>,
) -> Result<(WreqResponse, UpstreamRequestMeta), wreq::Error> {
    let mut headers = vec![(
        "authorization".to_string(),
        format!("Bearer {access_token}"),
    )];
    if body.is_some() {
        headers.push(("content-type".to_string(), "application/json".to_string()));
    }
    crate::channels::upstream::tracked_send_request(
        client,
        method.clone(),
        url,
        headers,
        body.as_ref().cloned(),
    )
    .await
}

#[derive(Debug, Clone)]
enum VertexEndpoint {
    Global(String),
    Project(String),
}

#[derive(Debug, Clone, Copy)]
enum VertexModelResponseKind {
    List,
    Get,
    Embedding,
}

#[derive(Debug, Clone)]
struct VertexPreparedRequest {
    method: WreqMethod,
    endpoint: VertexEndpoint,
    query: Option<String>,
    body: Option<Vec<u8>>,
    model: Option<String>,
    model_response_kind: Option<VertexModelResponseKind>,
}

impl VertexPreparedRequest {
    fn from_transform_request(request: &TransformRequest) -> Result<Self, UpstreamError> {
        match request {
            TransformRequest::ModelListGemini(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                endpoint: VertexEndpoint::Global("publishers/google/models".to_string()),
                query: gemini_model_list_query_string(
                    value.query.page_size,
                    value.query.page_token.as_deref(),
                ),
                body: None,
                model: None,
                model_response_kind: Some(VertexModelResponseKind::List),
            }),
            TransformRequest::ModelGetGemini(value) => {
                let model_id = normalize_vertex_model_name(value.path.name.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    endpoint: VertexEndpoint::Global(format!(
                        "publishers/google/models/{model_id}"
                    )),
                    query: None,
                    body: None,
                    model: Some(model_id),
                    model_response_kind: Some(VertexModelResponseKind::Get),
                })
            }
            TransformRequest::CountTokenGemini(value) => {
                let model_id = normalize_vertex_model_name(value.path.model.as_str());
                let body = vertex_count_tokens_payload(model_id.as_str(), &value.body)?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    endpoint: VertexEndpoint::Project(format!(
                        "publishers/google/models/{model_id}:countTokens"
                    )),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model_id),
                    model_response_kind: None,
                })
            }
            TransformRequest::GenerateContentGemini(value) => {
                let model_id = normalize_vertex_model_name(value.path.model.as_str());
                let body = vertex_generate_payload(model_id.as_str(), &value.body)?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    endpoint: VertexEndpoint::Project(format!(
                        "publishers/google/models/{model_id}:generateContent"
                    )),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model_id),
                    model_response_kind: None,
                })
            }
            TransformRequest::StreamGenerateContentGeminiSse(value)
            | TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
                let model_id = normalize_vertex_model_name(value.path.model.as_str());
                let body = vertex_generate_payload(model_id.as_str(), &value.body)?;
                let query = value.query.alt.map(|_| "alt=sse".to_string());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    endpoint: VertexEndpoint::Project(format!(
                        "publishers/google/models/{model_id}:streamGenerateContent"
                    )),
                    query,
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model_id),
                    model_response_kind: None,
                })
            }
            TransformRequest::EmbeddingGemini(value) => {
                let model_id = normalize_vertex_model_name(value.path.model.as_str());
                let body = vertex_embedding_predict_payload(&value.body)?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    endpoint: VertexEndpoint::Project(format!(
                        "publishers/google/models/{model_id}:predict"
                    )),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model_id),
                    model_response_kind: Some(VertexModelResponseKind::Embedding),
                })
            }
            TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
                let mut body = value.body.clone();
                body.model = normalize_vertex_openai_model(body.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    endpoint: VertexEndpoint::Project(
                        "endpoints/openapi/chat/completions".to_string(),
                    ),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(body.model.clone()),
                    model_response_kind: None,
                })
            }
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
                let mut body = value.body.clone();
                body.model = normalize_vertex_openai_model(body.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    endpoint: VertexEndpoint::Project(
                        "endpoints/openapi/chat/completions".to_string(),
                    ),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(body.model.clone()),
                    model_response_kind: None,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

async fn normalize_vertex_model_response(
    response: WreqResponse,
    kind: VertexModelResponseKind,
) -> Result<TransformResponse, UpstreamError> {
    let status = response.status();
    let mut header_map = serde_json::Map::new();
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            header_map.insert(name.to_string(), Value::String(value.to_string()));
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let raw_body = serde_json::from_slice::<Value>(&bytes)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let body = match kind {
        VertexModelResponseKind::List => vertex_model_list_payload(raw_body),
        VertexModelResponseKind::Get => vertex_model_get_payload(raw_body),
        VertexModelResponseKind::Embedding => vertex_embedding_payload(raw_body)?,
    };

    let payload = json!({
        "stats_code": status.as_u16(),
        "headers": header_map,
        "body": body,
    });

    match kind {
        VertexModelResponseKind::List => {
            let response = serde_json::from_value(payload)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(TransformResponse::ModelListGemini(response))
        }
        VertexModelResponseKind::Get => {
            let response = serde_json::from_value(payload)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(TransformResponse::ModelGetGemini(response))
        }
        VertexModelResponseKind::Embedding => {
            let response = serde_json::from_value(payload)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(TransformResponse::EmbeddingGemini(response))
        }
    }
}

fn vertex_model_list_payload(value: Value) -> Value {
    let Value::Object(mut map) = value else {
        return value;
    };
    if map.contains_key("models") {
        return Value::Object(map);
    }

    let models = match map.remove("publisherModels") {
        Some(Value::Array(items)) => items
            .into_iter()
            .map(vertex_publisher_model_to_gemini)
            .collect::<Vec<_>>(),
        Some(item) => vec![vertex_publisher_model_to_gemini(item)],
        None => Vec::new(),
    };

    let mut out = serde_json::Map::new();
    out.insert("models".to_string(), Value::Array(models));
    if let Some(token) = map.remove("nextPageToken").filter(|v| !v.is_null()) {
        out.insert("nextPageToken".to_string(), token);
    }
    Value::Object(out)
}

fn vertex_model_get_payload(value: Value) -> Value {
    let Value::Object(mut map) = value else {
        return value;
    };
    if map
        .get("name")
        .and_then(|v| v.as_str())
        .map(|name| name.starts_with("models/"))
        .unwrap_or(false)
    {
        return Value::Object(map);
    }
    if let Some(inner) = map.remove("publisherModel") {
        return vertex_publisher_model_to_gemini(inner);
    }
    vertex_publisher_model_to_gemini(Value::Object(map))
}

fn vertex_publisher_model_to_gemini(value: Value) -> Value {
    let Value::Object(map) = value else {
        return value;
    };

    let raw_name = map
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    let model_id = if let Some((_, tail)) = raw_name.rsplit_once("/models/") {
        tail
    } else {
        raw_name.strip_prefix("models/").unwrap_or(raw_name)
    };
    let model_id = if model_id.is_empty() {
        "unknown"
    } else {
        model_id
    };

    let mut out = serde_json::Map::new();
    out.insert(
        "name".to_string(),
        Value::String(format!("models/{model_id}")),
    );

    if let Some(base_model_id) = map
        .get("baseModelId")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        out.insert(
            "baseModelId".to_string(),
            Value::String(base_model_id.to_string()),
        );
    }
    if let Some(version) = map
        .get("version")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        out.insert("version".to_string(), Value::String(version.to_string()));
    }
    if let Some(value) = map.get("displayName").cloned().filter(|v| !v.is_null()) {
        out.insert("displayName".to_string(), value);
    }
    if let Some(value) = map.get("description").cloned().filter(|v| !v.is_null()) {
        out.insert("description".to_string(), value);
    }
    if let Some(value) = map.get("inputTokenLimit").cloned().filter(|v| !v.is_null()) {
        out.insert("inputTokenLimit".to_string(), value);
    }
    if let Some(value) = map
        .get("outputTokenLimit")
        .cloned()
        .filter(|v| !v.is_null())
    {
        out.insert("outputTokenLimit".to_string(), value);
    }
    if let Some(value) = map
        .get("supportedGenerationMethods")
        .cloned()
        .filter(|v| !v.is_null())
    {
        out.insert("supportedGenerationMethods".to_string(), value);
    }
    if let Some(value) = map.get("thinking").cloned().filter(|v| !v.is_null()) {
        out.insert("thinking".to_string(), value);
    }
    if let Some(value) = map.get("temperature").cloned().filter(|v| !v.is_null()) {
        out.insert("temperature".to_string(), value);
    }
    if let Some(value) = map.get("maxTemperature").cloned().filter(|v| !v.is_null()) {
        out.insert("maxTemperature".to_string(), value);
    }
    if let Some(value) = map.get("topP").cloned().filter(|v| !v.is_null()) {
        out.insert("topP".to_string(), value);
    }
    if let Some(value) = map.get("topK").cloned().filter(|v| !v.is_null()) {
        out.insert("topK".to_string(), value);
    }

    Value::Object(out)
}

fn build_vertex_path(endpoint: VertexEndpoint, project_id: &str, location: &str) -> String {
    match endpoint {
        VertexEndpoint::Global(path) => format!("/v1beta1/{path}"),
        VertexEndpoint::Project(path) => {
            format!("/v1beta1/projects/{project_id}/locations/{location}/{path}")
        }
    }
}

fn normalize_vertex_model_name(name: &str) -> String {
    let name = name.trim();
    let name = name.strip_prefix("models/").unwrap_or(name);
    let name = name
        .strip_prefix("publishers/google/models/")
        .unwrap_or(name);
    if let Some((_, tail)) = name.rsplit_once("/models/") {
        return tail.to_string();
    }
    name.to_string()
}

fn normalize_vertex_openai_model(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    if let Some(stripped) = trimmed.strip_prefix("publishers/")
        && let Some((publisher, model_name)) = stripped.split_once("/models/")
    {
        return format!("{publisher}/{model_name}");
    }
    if let Some(idx) = trimmed.find("/publishers/") {
        let tail = &trimmed[(idx + "/publishers/".len())..];
        if let Some((publisher, model_name)) = tail.split_once("/models/") {
            return format!("{publisher}/{model_name}");
        }
    }
    if let Some(stripped) = trimmed.strip_prefix("models/") {
        return format!("google/{stripped}");
    }
    if trimmed.contains('/') {
        return trimmed.to_string();
    }
    format!("google/{trimmed}")
}

pub fn normalize_vertex_upstream_response_body(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let wrapper = value.as_object()?;
    if !wrapper.contains_key("stats_code") || !wrapper.contains_key("body") {
        return None;
    }
    serde_json::to_vec(wrapper.get("body")?).ok()
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
    let mut out = Map::new();
    out.insert(
        "model".to_string(),
        Value::String(format!("publishers/google/models/{path_model}")),
    );

    let source = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let source_map = source.as_object().cloned().unwrap_or_default();

    if let Some(contents) = source_map.get("contents")
        && !contents.is_null()
    {
        out.insert("contents".to_string(), contents.clone());
    }

    if let Some(generate) = source_map
        .get("generateContentRequest")
        .and_then(Value::as_object)
    {
        if !out.contains_key("contents")
            && let Some(value) = generate.get("contents")
        {
            out.insert("contents".to_string(), value.clone());
        }
        if let Some(value) = generate.get("instances") {
            out.insert("instances".to_string(), value.clone());
        }
        if let Some(value) = generate.get("tools") {
            out.insert("tools".to_string(), value.clone());
        }
        if let Some(value) = generate
            .get("systemInstruction")
            .or_else(|| generate.get("system_instruction"))
        {
            out.insert("systemInstruction".to_string(), value.clone());
        }
        if let Some(value) = generate
            .get("generationConfig")
            .or_else(|| generate.get("generation_config"))
        {
            out.insert("generationConfig".to_string(), value.clone());
        }
        if let Some(model) = generate.get("model").and_then(Value::as_str) {
            out.insert(
                "model".to_string(),
                Value::String(normalize_vertex_model_ref(model, path_model)),
            );
        }
    }

    Ok(Value::Object(out))
}

fn vertex_embedding_predict_payload(body: &impl serde::Serialize) -> Result<Value, UpstreamError> {
    let source = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let source_map = source.as_object().cloned().unwrap_or_default();

    let content = source_map
        .get("content")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let instance_text = content_text_for_predict(&content);
    let mut out = Map::new();
    out.insert(
        "instances".to_string(),
        Value::Array(vec![json!({
            "content": instance_text,
        })]),
    );

    let mut parameters = Map::new();
    if let Some(value) = source_map.get("taskType").cloned().filter(|v| !v.is_null()) {
        parameters.insert("taskType".to_string(), value);
    }
    if let Some(value) = source_map
        .get("outputDimensionality")
        .cloned()
        .filter(|v| !v.is_null())
    {
        parameters.insert("outputDimensionality".to_string(), value);
    }
    if let Some(value) = source_map.get("title").cloned().filter(|v| !v.is_null()) {
        parameters.insert("title".to_string(), value);
    }
    parameters.insert("autoTruncate".to_string(), Value::Bool(true));
    if !parameters.is_empty() {
        out.insert("parameters".to_string(), Value::Object(parameters));
    }

    Ok(Value::Object(out))
}

fn content_text_for_predict(content: &Value) -> String {
    let Some(parts) = content
        .as_object()
        .and_then(|value| value.get("parts"))
        .and_then(Value::as_array)
    else {
        return content.to_string();
    };

    let mut texts = Vec::new();
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                texts.push(trimmed.to_string());
            }
        }
    }

    if texts.is_empty() {
        content.to_string()
    } else {
        texts.join("\n")
    }
}

fn vertex_embedding_payload(value: Value) -> Result<Value, UpstreamError> {
    if value
        .as_object()
        .and_then(|value| value.get("embedding"))
        .is_some()
    {
        return Ok(value);
    }

    let first = value
        .as_object()
        .and_then(|value| value.get("predictions"))
        .and_then(Value::as_array)
        .and_then(|value| value.first())
        .ok_or_else(|| {
            UpstreamError::SerializeRequest(
                "vertex predict embedding response missing predictions[0]".to_string(),
            )
        })?;

    let values = first
        .as_object()
        .and_then(|value| value.get("embeddings").or_else(|| value.get("embedding")))
        .and_then(Value::as_object)
        .and_then(|value| value.get("values"))
        .or_else(|| first.as_object().and_then(|value| value.get("values")))
        .cloned()
        .ok_or_else(|| {
            UpstreamError::SerializeRequest(
                "vertex predict embedding response missing embedding values".to_string(),
            )
        })?;

    Ok(json!({
        "embedding": {
            "values": values,
        }
    }))
}
