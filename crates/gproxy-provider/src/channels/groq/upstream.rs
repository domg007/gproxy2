use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformResponse};
use serde_json::{Map, Number, Value};
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
    retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, RetryWithPayloadRequest, TokenizerResolutionContext};

const GROQ_UNSUPPORTED_CHAT_FIELDS: &[&str] = &["logit_bias", "logprobs", "top_logprobs"];
const GROQ_UNSUPPORTED_RESPONSES_FIELDS: &[&str] = &[
    "previous_response_id",
    "include",
    "safety_identifier",
    "prompt_cache_key",
    "prompt",
];

pub async fn try_local_groq_response(
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

pub async fn execute_groq_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &TransformRequest,
    now_unix_ms: u64,
    token_resolution: TokenizerResolutionContext<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    if let Some(local_response) =
        try_local_groq_response(provider, request, client, token_resolution).await?
    {
        return Ok(UpstreamResponse::from_local(local_response));
    }

    let prepared = GroqPreparedRequest::from_transform_request(request)?;
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
    execute_groq_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_groq_payload_with_retry(
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
        return Ok(UpstreamResponse::from_local(
            TransformResponse::CountTokenOpenAi(response),
        ));
    }

    let prepared =
        GroqPreparedRequest::from_payload(payload.operation, payload.protocol, payload.body)?;
    execute_groq_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        payload.now_unix_ms,
        None,
    )
    .await
}

async fn execute_groq_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: GroqPreparedRequest,
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
        provider,
        credential_states,
        prepared.model.as_deref(),
        now_unix_ms,
        pick_mode,
        cache_affinity_hint,
        |credential| {
            match &credential.credential {
                ChannelCredential::Builtin(BuiltinChannelCredential::Groq(value)) => {
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
                let mut sent_headers = Vec::new();
                merge_extra_headers(&mut sent_headers, &extra_headers);
                add_or_replace_header(
                    &mut sent_headers,
                    "authorization",
                    format!("Bearer {}", attempt.material),
                );
                add_or_replace_header(&mut sent_headers, "user-agent", user_agent);
                if body.is_some() {
                    add_or_replace_header(
                        &mut sent_headers,
                        "content-type",
                        "application/json",
                    );
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

struct GroqPreparedRequest {
    method: WreqMethod,
    path: String,
    body: Option<Vec<u8>>,
    model: Option<String>,
    extra_headers: Vec<(String, String)>,
}

impl GroqPreparedRequest {
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
            TransformRequest::GenerateContentOpenAiResponse(value) => {
                let raw_body = serde_json::to_vec(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/responses".to_string(),
                    body: Some(normalize_response_request_body(raw_body)),
                    model: value.body.model.clone(),
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
                let raw_body = serde_json::to_vec(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/chat/completions".to_string(),
                    body: Some(normalize_chat_completion_request_body(raw_body)),
                    model: Some(value.body.model.clone()),
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::StreamGenerateContentOpenAiResponse(value) => {
                let raw_body = serde_json::to_vec(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/responses".to_string(),
                    body: Some(normalize_response_request_body(raw_body)),
                    model: value.body.model.clone(),
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
                let raw_body = serde_json::to_vec(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/chat/completions".to_string(),
                    body: Some(normalize_chat_completion_request_body(raw_body)),
                    model: Some(value.body.model.clone()),
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::EmbeddingOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/embeddings".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: None,
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
        fn json_pointer_string(value: &Value, pointer: &str) -> Option<String> {
            value
                .pointer(pointer)
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        }

        let payload_value = serde_json::from_slice::<Value>(body)
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
                        "missing path.model in groq model_get payload".to_string(),
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
            (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1/responses".to_string(),
                    body: Some(normalize_response_request_body(
                        serde_json::to_vec(&body_value)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    )),
                    model: json_pointer_string(&body_value, "/model"),
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1/chat/completions".to_string(),
                    body: Some(normalize_chat_completion_request_body(
                        serde_json::to_vec(&body_value)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    )),
                    model: json_pointer_string(&body_value, "/model"),
                    extra_headers,
                })
            }
            (OperationFamily::Embedding, ProtocolKind::OpenAi) => {
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1/embeddings".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_value)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: json_pointer_string(&body_value, "/model"),
                    extra_headers,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn normalize_chat_completion_request_body(body: Vec<u8>) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(map) = value.as_object_mut() else {
        return body;
    };

    for field in GROQ_UNSUPPORTED_CHAT_FIELDS {
        map.remove(*field);
    }

    normalize_chat_choice_count(map);
    strip_message_name_fields(map);
    normalize_chat_tools(map);

    serde_json::to_vec(&value).unwrap_or(body)
}

fn normalize_response_request_body(body: Vec<u8>) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(map) = value.as_object_mut() else {
        return body;
    };

    for field in GROQ_UNSUPPORTED_RESPONSES_FIELDS {
        map.remove(*field);
    }

    normalize_response_store_flag(map);
    normalize_response_tools(map);

    serde_json::to_vec(&value).unwrap_or(body)
}

fn normalize_chat_choice_count(map: &mut Map<String, Value>) {
    let Some(value) = map.get_mut("n") else {
        return;
    };
    let is_one = value
        .as_u64()
        .map(|count| count == 1)
        .or_else(|| value.as_i64().map(|count| count == 1))
        .or_else(|| {
            value
                .as_f64()
                .map(|count| (count - 1.0).abs() < f64::EPSILON)
        })
        .unwrap_or(false);
    if !is_one {
        *value = Value::Number(Number::from(1_u64));
    }
}

fn strip_message_name_fields(map: &mut Map<String, Value>) {
    let Some(messages) = map.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for message in messages {
        if let Some(message_map) = message.as_object_mut() {
            message_map.remove("name");
        }
    }
}

fn normalize_chat_tools(map: &mut Map<String, Value>) {
    if let Some(tools_value) = map.remove("tools") {
        let mut normalized_tools = Vec::new();
        if let Value::Array(tools) = tools_value {
            for tool in tools {
                if let Some(normalized) = normalize_chat_tool(tool) {
                    normalized_tools.push(normalized);
                }
            }
        }
        if !normalized_tools.is_empty() {
            map.insert("tools".to_string(), Value::Array(normalized_tools));
        }
    }

    if let Some(tool_choice_value) = map.remove("tool_choice")
        && let Some(normalized_tool_choice) = normalize_chat_tool_choice(tool_choice_value)
    {
        map.insert("tool_choice".to_string(), normalized_tool_choice);
    }
}

fn normalize_response_tools(map: &mut Map<String, Value>) {
    if let Some(tools_value) = map.remove("tools") {
        let mut normalized_tools = Vec::new();
        if let Value::Array(tools) = tools_value {
            for tool in tools {
                if let Some(normalized) = normalize_response_tool(tool) {
                    normalized_tools.push(normalized);
                }
            }
        }
        if !normalized_tools.is_empty() {
            map.insert("tools".to_string(), Value::Array(normalized_tools));
        }
    }

    if let Some(tool_choice_value) = map.remove("tool_choice")
        && let Some(normalized_tool_choice) = normalize_response_tool_choice(tool_choice_value)
    {
        map.insert("tool_choice".to_string(), normalized_tool_choice);
    }
}

fn normalize_chat_tool(tool: Value) -> Option<Value> {
    let Value::Object(mut tool) = tool else {
        return None;
    };
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return None;
    }

    if let Some(function_obj) = tool.get("function").and_then(Value::as_object)
        && function_obj
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|name| !name.is_empty())
    {
        return Some(Value::Object(tool));
    }

    let name = tool
        .remove("name")
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;

    let mut function = Map::new();
    function.insert("name".to_string(), Value::String(name));
    if let Some(description) = tool.remove("description") {
        function.insert("description".to_string(), description);
    }
    if let Some(parameters) = tool.remove("parameters") {
        function.insert("parameters".to_string(), parameters);
    }
    if let Some(strict) = tool.remove("strict") {
        function.insert("strict".to_string(), strict);
    }

    let mut normalized = Map::new();
    normalized.insert("type".to_string(), Value::String("function".to_string()));
    normalized.insert("function".to_string(), Value::Object(function));
    Some(Value::Object(normalized))
}

fn normalize_response_tool(tool: Value) -> Option<Value> {
    let Value::Object(mut tool) = tool else {
        return None;
    };
    if tool.get("type").and_then(Value::as_str) != Some("function") {
        return None;
    }

    if tool
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|name| !name.is_empty())
    {
        return Some(Value::Object(tool));
    }

    let function = tool.remove("function")?.as_object()?.clone();
    let name = function
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)?;

    let mut normalized = Map::new();
    normalized.insert("type".to_string(), Value::String("function".to_string()));
    normalized.insert("name".to_string(), Value::String(name));
    normalized.insert(
        "parameters".to_string(),
        function
            .get("parameters")
            .cloned()
            .unwrap_or_else(|| Value::Object(Map::new())),
    );
    if let Some(description) = function.get("description").cloned() {
        normalized.insert("description".to_string(), description);
    }
    if let Some(strict) = function.get("strict").cloned() {
        normalized.insert("strict".to_string(), strict);
    }
    Some(Value::Object(normalized))
}

fn normalize_chat_tool_choice(choice: Value) -> Option<Value> {
    match choice {
        Value::String(mode) => {
            normalize_tool_choice_mode(mode.as_str()).map(|value| Value::String(value.to_string()))
        }
        Value::Object(mut object) => {
            let type_name = object.get("type").and_then(Value::as_str)?;
            match type_name {
                "function" => {
                    if let Some(function) = object.get("function").and_then(Value::as_object)
                        && function
                            .get("name")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .is_some_and(|name| !name.is_empty())
                    {
                        return Some(Value::Object(object));
                    }

                    let name = object
                        .remove("name")
                        .and_then(|value| value.as_str().map(ToOwned::to_owned))
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())?;
                    let mut function = Map::new();
                    function.insert("name".to_string(), Value::String(name));
                    let mut normalized = Map::new();
                    normalized.insert("type".to_string(), Value::String("function".to_string()));
                    normalized.insert("function".to_string(), Value::Object(function));
                    Some(Value::Object(normalized))
                }
                "allowed_tools" => object
                    .get("mode")
                    .and_then(Value::as_str)
                    .and_then(normalize_tool_choice_mode)
                    .map(|value| Value::String(value.to_string())),
                _ => None,
            }
        }
        _ => None,
    }
}

fn normalize_response_tool_choice(choice: Value) -> Option<Value> {
    match choice {
        Value::String(mode) => {
            normalize_tool_choice_mode(mode.as_str()).map(|value| Value::String(value.to_string()))
        }
        Value::Object(mut object) => {
            let type_name = object.get("type").and_then(Value::as_str)?;
            match type_name {
                "function" => {
                    if object
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .is_some_and(|name| !name.is_empty())
                    {
                        return Some(Value::Object(object));
                    }

                    let name = object
                        .remove("function")
                        .and_then(|value| value.as_object().cloned())
                        .and_then(|function| {
                            function
                                .get("name")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|name| !name.is_empty())
                                .map(ToOwned::to_owned)
                        })?;
                    let mut normalized = Map::new();
                    normalized.insert("type".to_string(), Value::String("function".to_string()));
                    normalized.insert("name".to_string(), Value::String(name));
                    Some(Value::Object(normalized))
                }
                "allowed_tools" => object
                    .get("mode")
                    .and_then(Value::as_str)
                    .and_then(normalize_tool_choice_mode)
                    .map(|value| Value::String(value.to_string())),
                _ => None,
            }
        }
        _ => None,
    }
}

fn normalize_tool_choice_mode(mode: &str) -> Option<&'static str> {
    match mode {
        "none" => Some("none"),
        "auto" => Some("auto"),
        "required" => Some("required"),
        _ => None,
    }
}

fn normalize_response_store_flag(map: &mut Map<String, Value>) {
    let Some(store) = map.get_mut("store") else {
        return;
    };
    match store {
        Value::Bool(true) => *store = Value::Bool(false),
        Value::Bool(false) | Value::Null => {}
        _ => {
            map.remove("store");
        }
    }
}
