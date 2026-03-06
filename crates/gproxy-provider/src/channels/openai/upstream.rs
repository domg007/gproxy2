use wreq::{Client as WreqClient, Method as WreqMethod};

use crate::channels::retry::{
    CacheAffinityProtocol, CredentialRetryDecision, cache_affinity_hint_from_transform_request,
    configured_pick_mode_uses_cache, credential_pick_mode,
    retry_with_eligible_credentials_with_affinity,
};
use crate::channels::upstream::{
    UpstreamError, UpstreamResponse, add_or_replace_header, extra_headers_from_payload_value,
    extra_headers_from_transform_request, merge_extra_headers, payload_body_value,
};
use crate::channels::utils::{
    default_gproxy_user_agent, is_auth_failure, is_transient_server_failure,
    join_base_url_and_path, resolve_user_agent_or_else, retry_after_to_millis, to_wreq_method,
};
use crate::channels::{BuiltinChannelCredential, ChannelCredential};
use crate::credential::ChannelCredentialStateStore;
use crate::credential_state::CredentialStateManager;
use crate::provider::ProviderDefinition;
use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequest, TransformRoute};

pub async fn execute_openai_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    request: &gproxy_middleware::TransformRequest,
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let cache_protocol =
        crate::channels::retry::cache_affinity_protocol_from_transform_request(request);
    let prepared =
        OpenAiPreparedRequest::from_transform_request(request)?.with_gpt5_sampling_guard();
    execute_openai_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        cache_protocol,
        now_unix_ms,
    )
    .await
}

pub async fn execute_openai_payload_with_retry(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
    now_unix_ms: u64,
) -> Result<UpstreamResponse, UpstreamError> {
    let prepared =
        OpenAiPreparedRequest::from_payload(operation, protocol, body)?.with_gpt5_sampling_guard();
    let cache_protocol = cache_affinity_protocol_from_operation_protocol(operation, protocol);
    execute_openai_with_prepared(
        client,
        provider,
        credential_states,
        prepared,
        cache_protocol,
        now_unix_ms,
    )
    .await
}

async fn execute_openai_with_prepared(
    client: &WreqClient,
    provider: &ProviderDefinition,
    credential_states: &ChannelCredentialStateStore,
    prepared: OpenAiPreparedRequest,
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
    let extra_headers_template = prepared.extra_headers.clone();
    let url_template = url.clone();
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
                ChannelCredential::Builtin(BuiltinChannelCredential::OpenAi(value)) => {
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

fn cache_affinity_protocol_from_operation_protocol(
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> Option<CacheAffinityProtocol> {
    match (operation, protocol) {
        (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
            Some(CacheAffinityProtocol::OpenAiResponses)
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
            Some(CacheAffinityProtocol::OpenAiChatCompletions)
        }
        _ => None,
    }
}

struct OpenAiPreparedRequest {
    method: WreqMethod,
    path: String,
    body: Option<Vec<u8>>,
    model: Option<String>,
    extra_headers: Vec<(String, String)>,
}

impl OpenAiPreparedRequest {
    fn with_gpt5_sampling_guard(mut self) -> Self {
        if self
            .model
            .as_deref()
            .is_some_and(requires_gpt5_sampling_guard)
            && let Some(body) = self.body.take()
        {
            self.body = Some(strip_openai_sampling_fields(body));
        }
        self
    }

    fn from_transform_request(
        request: &gproxy_middleware::TransformRequest,
    ) -> Result<Self, UpstreamError> {
        match request {
            gproxy_middleware::TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/models".to_string(),
                body: None,
                model: None,
                extra_headers: extra_headers_from_transform_request(request),
            }),
            gproxy_middleware::TransformRequest::ModelGetOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: format!("/v1/models/{}", value.path.model),
                body: None,
                model: Some(value.path.model.clone()),
                extra_headers: extra_headers_from_transform_request(request),
            }),
            gproxy_middleware::TransformRequest::CountTokenOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/responses/input_tokens".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: value.body.model.clone(),
                extra_headers: extra_headers_from_transform_request(request),
            }),
            gproxy_middleware::TransformRequest::GenerateContentOpenAiResponse(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/responses".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: value.body.model.clone(),
                extra_headers: extra_headers_from_transform_request(request),
            }),
            gproxy_middleware::TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/chat/completions".to_string(),
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(value.body.model.clone()),
                    extra_headers: extra_headers_from_transform_request(request),
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentOpenAiResponse(value) => {
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1/responses".to_string(),
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: value.body.model.clone(),
                    extra_headers: extra_headers_from_transform_request(request),
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentOpenAiChatCompletions(
                value,
            ) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/chat/completions".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
                extra_headers: extra_headers_from_transform_request(request),
            }),
            gproxy_middleware::TransformRequest::EmbeddingOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/embeddings".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: None,
                extra_headers: extra_headers_from_transform_request(request),
            }),
            gproxy_middleware::TransformRequest::CompactOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: "/v1/responses/compact".to_string(),
                body: Some(
                    serde_json::to_vec(&value.body)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: Some(value.body.model.clone()),
                extra_headers: extra_headers_from_transform_request(request),
            }),
            gproxy_middleware::TransformRequest::OpenAiResponseWebSocket(value) => {
                let transformed = transform_openai_ws_request_to_stream(
                    TransformRequest::OpenAiResponseWebSocket(value.clone()),
                )?;
                Self::from_transform_request(&transformed)
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
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
                        "missing path.model in OpenAI model get payload".to_string(),
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
            (OperationFamily::CountToken, ProtocolKind::OpenAi) => Ok(Self {
                method: WreqMethod::POST,
                path: "/v1/responses/input_tokens".to_string(),
                body: Some(
                    serde_json::to_vec(&body_value)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: json_pointer_string(&body_value, "/model"),
                extra_headers,
            }),
            (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => Ok(Self {
                method: WreqMethod::POST,
                path: "/v1/responses".to_string(),
                body: Some(
                    serde_json::to_vec(&body_value)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: json_pointer_string(&body_value, "/model"),
                extra_headers,
            }),
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
            (OperationFamily::Compact, ProtocolKind::OpenAi) => Ok(Self {
                method: WreqMethod::POST,
                path: "/v1/responses/compact".to_string(),
                body: Some(
                    serde_json::to_vec(&body_value)
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                ),
                model: json_pointer_string(&body_value, "/model"),
                extra_headers,
            }),
            (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi) => {
                let request = gproxy_middleware::decode_request_payload(operation, protocol, body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                let transformed = transform_openai_ws_request_to_stream(request)?;
                Self::from_transform_request(&transformed)
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}

fn transform_openai_ws_request_to_stream(
    request: TransformRequest,
) -> Result<TransformRequest, UpstreamError> {
    gproxy_middleware::transform_request(
        request,
        TransformRoute {
            src_operation: OperationFamily::OpenAiResponseWebSocket,
            src_protocol: ProtocolKind::OpenAi,
            dst_operation: OperationFamily::StreamGenerateContent,
            dst_protocol: ProtocolKind::OpenAi,
        },
    )
    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))
}

fn requires_gpt5_sampling_guard(model: &str) -> bool {
    model.to_ascii_lowercase().contains("gpt-5")
}

fn strip_openai_sampling_fields(body: Vec<u8>) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return body;
    };
    let Some(map) = value.as_object_mut() else {
        return body;
    };
    map.remove("temperature");
    map.remove("top_p");

    serde_json::to_vec(&value).unwrap_or(body)
}

#[cfg(test)]
mod tests {
    use super::{
        OpenAiPreparedRequest, WreqMethod, requires_gpt5_sampling_guard,
        strip_openai_sampling_fields,
    };
    use gproxy_middleware::{OperationFamily, ProtocolKind};
    use serde_json::json;

    #[test]
    fn gpt5_guard_matches_case_insensitive_substring() {
        assert!(requires_gpt5_sampling_guard("gpt-5"));
        assert!(requires_gpt5_sampling_guard("GPT-5-NANO"));
        assert!(requires_gpt5_sampling_guard("openai/gpt-5.1"));
        assert!(!requires_gpt5_sampling_guard("gpt-4o"));
    }

    #[test]
    fn strip_sampling_fields_only_removes_temperature_and_top_p() {
        let body = br#"{
            "model":"gpt-5-nano",
            "temperature":0.2,
            "top_p":0.9,
            "stream":true,
            "messages":[{"role":"user","content":"hi"}]
        }"#;
        let out = strip_openai_sampling_fields(body.to_vec());
        let json: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert!(json.get("temperature").is_none());
        assert!(json.get("top_p").is_none());
        assert_eq!(
            json.get("model").and_then(|v| v.as_str()),
            Some("gpt-5-nano")
        );
        assert_eq!(json.get("stream").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn websocket_payload_is_supported_via_stream_fallback() {
        let payload = serde_json::to_vec(&json!({
            "method": "GET",
            "path": { "endpoint": "responses" },
            "query": {},
            "headers": {},
            "body": {
                "type": "response.create",
                "model": "gpt-5",
                "stream": true
            }
        }))
        .expect("serialize websocket payload");

        let prepared = OpenAiPreparedRequest::from_payload(
            OperationFamily::OpenAiResponseWebSocket,
            ProtocolKind::OpenAi,
            payload.as_slice(),
        )
        .expect("prepare websocket payload");

        assert_eq!(prepared.method, WreqMethod::POST);
        assert_eq!(prepared.path, "/v1/responses");
        assert_eq!(prepared.model.as_deref(), Some("gpt-5"));
        assert!(prepared.body.is_some());
    }

    #[test]
    fn payload_wrapper_preserves_extra_headers_and_uses_body_payload() {
        let payload = serde_json::to_vec(&json!({
            "method": "POST",
            "path": {},
            "query": {},
            "headers": {
                "extra": {
                    "x-codex-turn-metadata": "{\"turn_id\":\"abc\"}",
                    "session_id": "sess-123"
                }
            },
            "body": {
                "model": "gpt-4.1-mini",
                "input": "hello"
            }
        }))
        .expect("serialize payload");

        let prepared = OpenAiPreparedRequest::from_payload(
            OperationFamily::GenerateContent,
            ProtocolKind::OpenAi,
            payload.as_slice(),
        )
        .expect("prepare payload");

        assert_eq!(prepared.model.as_deref(), Some("gpt-4.1-mini"));
        assert!(prepared.extra_headers.iter().any(
            |(name, value)| name == "x-codex-turn-metadata" && value == "{\"turn_id\":\"abc\"}"
        ));
        assert!(
            prepared
                .extra_headers
                .iter()
                .any(|(name, value)| name == "session_id" && value == "sess-123")
        );

        let body: serde_json::Value =
            serde_json::from_slice(prepared.body.as_deref().expect("body bytes"))
                .expect("valid json");
        assert_eq!(
            body.get("model").and_then(|value| value.as_str()),
            Some("gpt-4.1-mini")
        );
        assert!(body.get("headers").is_none());
    }

    #[test]
    fn payload_wrapper_accepts_flat_headers() {
        let payload = serde_json::to_vec(&json!({
            "method": "POST",
            "path": {},
            "query": {},
            "headers": {
                "x-codex-turn-metadata": "{\"turn_id\":\"abc\"}",
                "session_id": "sess-123"
            },
            "body": {
                "model": "gpt-4.1-mini",
                "input": "hello"
            }
        }))
        .expect("serialize payload");

        let prepared = OpenAiPreparedRequest::from_payload(
            OperationFamily::GenerateContent,
            ProtocolKind::OpenAi,
            payload.as_slice(),
        )
        .expect("prepare payload");

        assert!(prepared.extra_headers.iter().any(
            |(name, value)| name == "x-codex-turn-metadata" && value == "{\"turn_id\":\"abc\"}"
        ));
        assert!(
            prepared
                .extra_headers
                .iter()
                .any(|(name, value)| name == "session_id" && value == "sess-123")
        );
    }
}
