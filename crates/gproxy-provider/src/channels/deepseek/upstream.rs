use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformResponse};
use serde_json::{Map, Value};
use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::retry::{
    CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamError, UpstreamResponse, add_or_replace_header, extra_headers_from_payload_value,
    extra_headers_from_transform_request, merge_extra_headers, payload_body_value,
    payload_header_string, payload_header_string_array,
};
use crate::channels::utils::{
    anthropic_header_pairs, claude_model_to_string, count_openai_input_tokens_with_resolution,
    default_gproxy_user_agent, is_auth_failure, is_transient_server_failure,
    join_base_url_and_path, resolve_user_agent_or_else, retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::{ProviderDefinition, RetryWithPayloadRequest, TokenizerResolutionContext};

const DEEPSEEK_UNSUPPORTED_CHAT_FIELDS: &[&str] = &[
    "audio",
    "function_call",
    "functions",
    "logit_bias",
    "max_completion_tokens",
    "metadata",
    "modalities",
    "n",
    "parallel_tool_calls",
    "prediction",
    "prompt_cache_key",
    "prompt_cache_retention",
    "reasoning_effort",
    "safety_identifier",
    "seed",
    "service_tier",
    "store",
    "user",
    "verbosity",
    "web_search_options",
];

pub async fn try_local_deepseek_response(
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
    execute_deepseek_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        now_unix_ms,
        cache_affinity_hint,
    )
    .await
}

pub async fn execute_deepseek_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    payload: RetryWithPayloadRequest<'_>,
) -> Result<UpstreamResponse, UpstreamError> {
    if (payload.operation, payload.protocol) == (OperationFamily::CountToken, ProtocolKind::OpenAi)
    {
        let body_json = serde_json::from_slice::<Value>(payload.body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
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
        DeepseekPreparedRequest::from_payload(payload.operation, payload.protocol, payload.body)?;
    execute_deepseek_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        payload.now_unix_ms,
        None,
    )
    .await
}

async fn execute_deepseek_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: DeepseekPreparedRequest,
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
    let url_template = url.clone();
    let auth_template = prepared.auth_scheme;
    let request_headers_template = prepared.request_headers.clone();
    let extra_headers_template = prepared.extra_headers.clone();
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
            let extra_headers = extra_headers_template.clone();
            let user_agent = user_agent_template.clone();

            async move {
                let mut sent_headers = Vec::new();
                merge_extra_headers(&mut sent_headers, &extra_headers);
                add_or_replace_header(&mut sent_headers, "user-agent", user_agent);
                match auth_scheme {
                    AuthScheme::Bearer => {
                        add_or_replace_header(
                            &mut sent_headers,
                            "authorization",
                            format!("Bearer {}", attempt.material),
                        );
                    }
                    AuthScheme::XApiKey => {
                        add_or_replace_header(
                            &mut sent_headers,
                            "x-api-key",
                            attempt.material.clone(),
                        );
                    }
                };

                for (name, value) in &request_headers {
                    add_or_replace_header(&mut sent_headers, name, value.clone());
                }
                if body.is_some() {
                    add_or_replace_header(&mut sent_headers, "content-type", "application/json");
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
    extra_headers: Vec<(String, String)>,
}

impl DeepseekPreparedRequest {
    fn from_transform_request(request: &TransformRequest) -> Result<Self, UpstreamError> {
        let extra_headers = extra_headers_from_transform_request(request);
        let mut prepared = match request {
            TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/models".to_string(),
                body: None,
                model: None,
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
                extra_headers: Vec::new(),
            }),
            TransformRequest::ModelGetOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: format!("/v1/models/{}", value.path.model),
                body: None,
                model: Some(value.path.model.clone()),
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
                extra_headers: Vec::new(),
            }),
            TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
                let mut body = value.body.clone();
                if let Some(max_tokens) = body.max_tokens {
                    body.max_tokens = Some(max_tokens.min(8192));
                }
                if let Some(max_completion_tokens) = body.max_completion_tokens {
                    body.max_completion_tokens = Some(max_completion_tokens.min(8192));
                }
                let mut body_json = serde_json::to_value(&body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_deepseek_chat_request_body(&mut body_json);
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
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
                let mut body = value.body.clone();
                if let Some(max_tokens) = body.max_tokens {
                    body.max_tokens = Some(max_tokens.min(8192));
                }
                if let Some(max_completion_tokens) = body.max_completion_tokens {
                    body.max_completion_tokens = Some(max_completion_tokens.min(8192));
                }
                let mut body_json = serde_json::to_value(&body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_deepseek_chat_request_body(&mut body_json);
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
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::GenerateContentClaude(value) => {
                let model = claude_model_to_string(&value.body.model)?;
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
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::StreamGenerateContentClaude(value) => {
                let model = claude_model_to_string(&value.body.model)?;
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
                    extra_headers: Vec::new(),
                })
            }
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

        fn parse_claude_payload_wrapper(
            value: &Value,
        ) -> Result<(Value, String, Option<Vec<String>>), UpstreamError> {
            const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

            if let Some(body_value) = value.get("body").cloned() {
                let version =
                    payload_header_string(value, &["anthropic-version", "anthropic_version"])
                        .unwrap_or_else(|| DEFAULT_ANTHROPIC_VERSION.to_string());
                let beta =
                    payload_header_string_array(value, &["anthropic-beta", "anthropic_beta"]);
                return Ok((body_value, version, beta));
            }
            Ok((value.clone(), DEFAULT_ANTHROPIC_VERSION.to_string(), None))
        }

        let payload_value = serde_json::from_slice::<Value>(body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let extra_headers = extra_headers_from_payload_value(&payload_value);

        match (operation, protocol) {
            (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(Self {
                method: WreqMethod::GET,
                path: "/v1/models".to_string(),
                body: None,
                model: None,
                auth_scheme: AuthScheme::Bearer,
                request_headers: Vec::new(),
                extra_headers,
            }),
            (OperationFamily::ModelGet, ProtocolKind::OpenAi) => {
                let Some(model) = json_pointer_string(&payload_value, "/path/model") else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in deepseek model_get payload".to_string(),
                    ));
                };
                Ok(Self {
                    method: WreqMethod::GET,
                    path: format!("/v1/models/{model}"),
                    body: None,
                    model: Some(model),
                    auth_scheme: AuthScheme::Bearer,
                    request_headers: Vec::new(),
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                let mut body_json = payload_body_value(&payload_value);
                if let Some(map) = body_json.as_object_mut() {
                    if let Some(max_tokens) = map.get("max_tokens").and_then(Value::as_u64) {
                        map.insert("max_tokens".to_string(), Value::from(max_tokens.min(8192)));
                    }
                    if let Some(max_completion_tokens) =
                        map.get("max_completion_tokens").and_then(Value::as_u64)
                    {
                        map.insert(
                            "max_completion_tokens".to_string(),
                            Value::from(max_completion_tokens.min(8192)),
                        );
                    }
                }
                normalize_deepseek_chat_request_body(&mut body_json);
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1/chat/completions".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: json_pointer_string(&body_json, "/model"),
                    auth_scheme: AuthScheme::Bearer,
                    request_headers: Vec::new(),
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::Claude)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
                let (mut body_json, version, beta) = parse_claude_payload_wrapper(&payload_value)?;
                let model = json_pointer_string(&body_json, "/model");
                if let Some(model_value) = model.clone()
                    && let Some(map) = body_json.as_object_mut()
                {
                    map.insert("model".to_string(), Value::String(model_value));
                }
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/anthropic/v1/messages".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model,
                    auth_scheme: AuthScheme::XApiKey,
                    request_headers: anthropic_header_pairs(&version, beta.as_ref())?,
                    extra_headers,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn normalize_deepseek_chat_request_body(body_json: &mut Value) {
    let Some(map) = body_json.as_object_mut() else {
        return;
    };

    normalize_deepseek_chat_extra_body(map);

    if map.get("max_tokens").is_none()
        && let Some(max_completion_tokens) = map.remove("max_completion_tokens")
    {
        map.insert("max_tokens".to_string(), max_completion_tokens);
    }

    for field in DEEPSEEK_UNSUPPORTED_CHAT_FIELDS {
        map.remove(*field);
    }

    normalize_deepseek_chat_message_roles(map);
    normalize_deepseek_chat_tools(map);
}

fn normalize_deepseek_chat_extra_body(map: &mut Map<String, Value>) {
    let Some(extra_body) = map.remove("extra_body") else {
        return;
    };

    if map.contains_key("thinking") {
        return;
    }

    if let Some(thinking) = deepseek_thinking_from_extra_body(&extra_body) {
        map.insert("thinking".to_string(), thinking);
    }
}

fn deepseek_thinking_from_extra_body(extra_body: &Value) -> Option<Value> {
    let object = extra_body.as_object()?;

    if let Some(value) = object
        .get("thinking")
        .and_then(normalize_deepseek_thinking_value)
    {
        return Some(value);
    }

    object
        .get("extra_body")
        .and_then(deepseek_thinking_from_extra_body)
}

fn normalize_deepseek_thinking_value(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    let mode = object.get("type")?.as_str()?;
    let normalized_type = match mode {
        "enabled" => "enabled",
        "disabled" => "disabled",
        // Claude/GProxy compatible extension; DeepSeek only supports enabled/disabled.
        "adaptive" => "enabled",
        _ => return None,
    };
    Some(serde_json::json!({ "type": normalized_type }))
}

fn normalize_deepseek_chat_message_roles(map: &mut Map<String, Value>) {
    let Some(messages) = map.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };

    for message in messages {
        if let Some(object) = message.as_object_mut() {
            let is_developer = object
                .get("role")
                .and_then(Value::as_str)
                .map(|role| role.eq_ignore_ascii_case("developer"))
                .unwrap_or(false);
            if is_developer {
                object.insert("role".to_string(), Value::String("system".to_string()));
            }
        }
    }
}

fn normalize_deepseek_chat_tools(map: &mut Map<String, Value>) {
    if let Some(tools_value) = map.remove("tools") {
        let mut normalized_tools = Vec::new();
        if let Value::Array(tools) = tools_value {
            for tool in tools {
                if let Some(normalized) = normalize_deepseek_chat_tool(tool) {
                    normalized_tools.push(normalized);
                }
            }
        }
        if !normalized_tools.is_empty() {
            map.insert("tools".to_string(), Value::Array(normalized_tools));
        }
    }

    if let Some(tool_choice) = map.remove("tool_choice")
        && let Some(normalized) = normalize_deepseek_chat_tool_choice(tool_choice)
    {
        let has_tools = map
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| !tools.is_empty())
            .unwrap_or(false);
        let normalized = if has_tools || normalized == Value::String("none".to_string()) {
            normalized
        } else {
            Value::String("none".to_string())
        };
        map.insert("tool_choice".to_string(), normalized);
    }
}

fn normalize_deepseek_chat_tool(tool: Value) -> Option<Value> {
    let mut tool = tool.as_object()?.clone();
    let type_value = tool.remove("type")?.as_str()?.to_string();
    if type_value != "function" {
        return None;
    }

    let function = tool.remove("function")?.as_object()?.clone();
    Some(Value::Object(
        [
            ("type".to_string(), Value::String("function".to_string())),
            ("function".to_string(), Value::Object(function)),
        ]
        .into_iter()
        .collect(),
    ))
}

fn normalize_deepseek_chat_tool_choice(choice: Value) -> Option<Value> {
    match choice {
        Value::String(mode) => match mode.as_str() {
            "none" | "auto" | "required" => Some(Value::String(mode)),
            _ => None,
        },
        Value::Object(mut object) => {
            let type_value = object.remove("type")?.as_str()?.to_string();
            if type_value != "function" {
                return None;
            }

            let function = object.remove("function")?.as_object()?.clone();
            let name = function.get("name")?.as_str()?.to_string();
            Some(serde_json::json!({
                "type": "function",
                "function": { "name": name }
            }))
        }
        _ => None,
    }
}

pub fn normalize_deepseek_upstream_response_body(body: &[u8]) -> Option<Vec<u8>> {
    let mut value = serde_json::from_slice::<Value>(body).ok()?;
    let map = value.as_object_mut()?;
    let mut changed = false;

    if let Some(choices) = map.get_mut("choices").and_then(Value::as_array_mut) {
        for choice in choices {
            if let Some(choice_map) = choice.as_object_mut()
                && let Some(reason) = choice_map.get_mut("finish_reason")
                && reason.as_str() == Some("insufficient_system_resource")
            {
                *reason = Value::String("length".to_string());
                changed = true;
            }
        }
    }

    if let Some(usage) = map.get_mut("usage").and_then(Value::as_object_mut)
        && let Some(cache_hit_tokens) = usage.get("prompt_cache_hit_tokens").and_then(Value::as_u64)
    {
        let details_value = usage
            .entry("prompt_tokens_details".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !details_value.is_object() {
            *details_value = Value::Object(Map::new());
        }
        if let Some(details) = details_value.as_object_mut() {
            details
                .entry("cached_tokens".to_string())
                .or_insert(Value::from(cache_hit_tokens));
            changed = true;
        }
    }

    changed.then(|| serde_json::to_vec(&value).ok()).flatten()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{normalize_deepseek_chat_request_body, normalize_deepseek_upstream_response_body};

    #[test]
    fn normalize_request_maps_max_completion_tokens_and_developer_role() {
        let mut body = json!({
            "model": "deepseek-chat",
            "max_completion_tokens": 1234,
            "messages": [
                { "role": "developer", "content": "rule" },
                { "role": "user", "content": "hi" }
            ],
            "parallel_tool_calls": true,
            "store": true
        });

        normalize_deepseek_chat_request_body(&mut body);

        assert_eq!(body.get("max_tokens").and_then(|v| v.as_u64()), Some(1234));
        assert!(body.get("max_completion_tokens").is_none());
        assert!(body.get("parallel_tool_calls").is_none());
        assert!(body.get("store").is_none());
        assert_eq!(
            body.get("messages")
                .and_then(|v| v.as_array())
                .and_then(|messages| messages.first())
                .and_then(|message| message.get("role"))
                .and_then(|role| role.as_str()),
            Some("system")
        );
    }

    #[test]
    fn normalize_request_flattens_extra_body_thinking() {
        let mut body = json!({
            "model": "deepseek-reasoner",
            "messages": [{ "role": "user", "content": "hi" }],
            "extra_body": {
                "thinking": { "type": "adaptive" }
            }
        });

        normalize_deepseek_chat_request_body(&mut body);

        assert!(body.get("extra_body").is_none());
        assert_eq!(
            body.get("thinking")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str()),
            Some("enabled")
        );
    }

    #[test]
    fn normalize_response_maps_finish_reason_and_cache_tokens() {
        let body = json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "deepseek-chat",
            "choices": [
                {
                    "index": 0,
                    "finish_reason": "insufficient_system_resource",
                    "message": { "role": "assistant", "content": "x" }
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
                "prompt_cache_hit_tokens": 3
            }
        });

        let normalized = normalize_deepseek_upstream_response_body(
            serde_json::to_vec(&body).expect("json").as_slice(),
        )
        .expect("normalized");
        let normalized_json: serde_json::Value =
            serde_json::from_slice(&normalized).expect("valid json");

        assert_eq!(
            normalized_json
                .get("choices")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("finish_reason"))
                .and_then(|v| v.as_str()),
            Some("length")
        );
        assert_eq!(
            normalized_json
                .get("usage")
                .and_then(|v| v.get("prompt_tokens_details"))
                .and_then(|v| v.get("cached_tokens"))
                .and_then(|v| v.as_u64()),
            Some(3)
        );
    }
}
