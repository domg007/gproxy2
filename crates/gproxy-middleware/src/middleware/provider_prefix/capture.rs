use super::*;

struct ProviderCapture {
    provider: Option<String>,
}

impl ProviderCapture {
    fn new() -> Self {
        Self { provider: None }
    }

    fn strip(
        &mut self,
        operation: OperationFamily,
        protocol: ProtocolKind,
        field: &'static str,
        value: &str,
    ) -> Result<String, MiddlewareTransformError> {
        let Some((has_models_prefix, provider, model_without_provider)) =
            split_provider_prefixed_model(value)
        else {
            return Err(MiddlewareTransformError::ProviderPrefix {
                message: format!(
                    "missing provider prefix in {field} for ({operation:?}, {protocol:?}): {value}",
                ),
            });
        };

        if let Some(existing) = self.provider.as_ref() {
            if existing != provider {
                return Err(MiddlewareTransformError::ProviderPrefix {
                    message: format!(
                        "inconsistent provider prefix in {field} for ({operation:?}, {protocol:?}): expected {existing}, got {provider}",
                    ),
                });
            }
        } else {
            self.provider = Some(provider.to_string());
        }

        Ok(if has_models_prefix {
            format!("models/{model_without_provider}")
        } else {
            model_without_provider.to_string()
        })
    }

    fn finish(
        self,
        operation: OperationFamily,
        protocol: ProtocolKind,
    ) -> Result<String, MiddlewareTransformError> {
        self.provider
            .ok_or(MiddlewareTransformError::ProviderPrefix {
                message: format!(
                    "no model/provider prefix found for ({operation:?}, {protocol:?})"
                ),
            })
    }
}

fn split_provider_prefixed_model(value: &str) -> Option<(bool, &str, &str)> {
    let (has_models_prefix, tail) = if let Some(rest) = value.strip_prefix("models/") {
        (true, rest)
    } else {
        (false, value)
    };
    let (provider, model_without_provider) = tail.split_once('/')?;
    if provider.is_empty() || model_without_provider.is_empty() {
        return None;
    }
    Some((has_models_prefix, provider, model_without_provider))
}

pub(super) fn add_provider_prefix(value: &str, provider: &str) -> String {
    if provider.is_empty() {
        return value.to_string();
    }
    if split_provider_prefixed_model(value).is_some() {
        return value.to_string();
    }

    if let Some(rest) = value.strip_prefix("models/") {
        return format!("models/{provider}/{rest}");
    }

    if value.is_empty() {
        provider.to_string()
    } else {
        format!("{provider}/{value}")
    }
}

pub(super) fn strip_provider_prefix_from_request_json(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Result<(String, Vec<u8>), MiddlewareTransformError> {
    let mut value: serde_json::Value =
        serde_json::from_slice(body).map_err(|err| MiddlewareTransformError::JsonDecode {
            kind: "request",
            operation,
            protocol,
            message: err.to_string(),
        })?;
    let provider = strip_provider_prefix_from_request_value(&mut value, operation, protocol)?;
    let encoded =
        serde_json::to_vec(&value).map_err(|err| MiddlewareTransformError::JsonEncode {
            kind: "request",
            operation,
            protocol,
            message: err.to_string(),
        })?;
    Ok((provider, encoded))
}

fn strip_provider_prefix_from_request_value(
    request: &mut serde_json::Value,
    operation: OperationFamily,
    protocol: ProtocolKind,
) -> Result<String, MiddlewareTransformError> {
    let mut capture = ProviderCapture::new();

    match (operation, protocol) {
        (OperationFamily::ModelGet, ProtocolKind::OpenAi) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "path.model",
                "/path/model",
                None,
            )?;
        }
        (OperationFamily::ModelGet, ProtocolKind::Claude) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "path.model_id",
                "/path/model_id",
                None,
            )?;
        }
        (OperationFamily::ModelGet, ProtocolKind::Gemini)
        | (OperationFamily::ModelGet, ProtocolKind::GeminiNDJson) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "path.name",
                "/path/name",
                None,
            )?;
        }
        (OperationFamily::CountToken, ProtocolKind::OpenAi) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "body.model",
                "/body/model",
                Some("missing body.model for OpenAI count-tokens"),
            )?;
        }
        (OperationFamily::CountToken, ProtocolKind::Claude) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "body.model",
                "/body/model",
                None,
            )?;
        }
        (OperationFamily::CountToken, ProtocolKind::Gemini)
        | (OperationFamily::CountToken, ProtocolKind::GeminiNDJson) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "path.model",
                "/path/model",
                None,
            )?;
            strip_optional_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "body.generate_content_request.model",
                "/body/generate_content_request/model",
            )?;
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "body.model",
                "/body/model",
                Some("missing body.model for OpenAI responses"),
            )?;
        }
        (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "body.model",
                "/body/model",
                Some("missing body.model for OpenAI websocket connect"),
            )?;
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion)
        | (OperationFamily::GenerateContent, ProtocolKind::Claude)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "body.model",
                "/body/model",
                None,
            )?;
        }
        (OperationFamily::GenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::GenerateContent, ProtocolKind::GeminiNDJson)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson)
        | (OperationFamily::GeminiLive, ProtocolKind::Gemini)
        | (OperationFamily::Embedding, ProtocolKind::Gemini)
        | (OperationFamily::Embedding, ProtocolKind::GeminiNDJson) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "path.model",
                if operation == OperationFamily::GeminiLive {
                    "/body/setup/model"
                } else {
                    "/path/model"
                },
                None,
            )?;
        }
        (OperationFamily::Embedding, ProtocolKind::OpenAi)
        | (OperationFamily::Compact, ProtocolKind::OpenAi) => {
            strip_required_string_field(
                request,
                &mut capture,
                operation,
                protocol,
                "body.model",
                "/body/model",
                None,
            )?;
        }
        _ => {
            return Err(MiddlewareTransformError::Unsupported(
                "provider prefix stripping is not implemented for this operation/protocol",
            ));
        }
    }

    capture.finish(operation, protocol)
}

fn strip_required_string_field(
    value: &mut serde_json::Value,
    capture: &mut ProviderCapture,
    operation: OperationFamily,
    protocol: ProtocolKind,
    field: &'static str,
    pointer: &'static str,
    missing_message: Option<&'static str>,
) -> Result<(), MiddlewareTransformError> {
    let Some(slot) = value.pointer_mut(pointer) else {
        let message = missing_message
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("missing {field} for ({operation:?}, {protocol:?})"));
        return Err(MiddlewareTransformError::ProviderPrefix { message });
    };
    let Some(raw) = slot.as_str() else {
        return Err(MiddlewareTransformError::ProviderPrefix {
            message: format!("invalid {field} for ({operation:?}, {protocol:?}): expected string",),
        });
    };
    let stripped = capture.strip(operation, protocol, field, raw)?;
    *slot = serde_json::Value::String(stripped);
    Ok(())
}

fn strip_optional_string_field(
    value: &mut serde_json::Value,
    capture: &mut ProviderCapture,
    operation: OperationFamily,
    protocol: ProtocolKind,
    field: &'static str,
    pointer: &'static str,
) -> Result<(), MiddlewareTransformError> {
    let Some(slot) = value.pointer_mut(pointer) else {
        return Ok(());
    };
    let Some(raw) = slot.as_str() else {
        return Err(MiddlewareTransformError::ProviderPrefix {
            message: format!("invalid {field} for ({operation:?}, {protocol:?}): expected string",),
        });
    };
    let stripped = capture.strip(operation, protocol, field, raw)?;
    *slot = serde_json::Value::String(stripped);
    Ok(())
}

pub(super) fn add_provider_prefix_to_response(response: &mut TransformResponse, provider: &str) {
    match response {
        TransformResponse::ModelListOpenAi(
            gproxy_protocol::openai::model_list::response::OpenAiModelListResponse::Success {
                body,
                ..
            },
        ) => {
            for model in &mut body.data {
                model.id = add_provider_prefix(&model.id, provider);
            }
        }
        TransformResponse::ModelListClaude(
            gproxy_protocol::claude::model_list::response::ClaudeModelListResponse::Success {
                body,
                ..
            },
        ) => {
            for model in &mut body.data {
                model.id = add_provider_prefix(&model.id, provider);
            }
            body.first_id = add_provider_prefix(&body.first_id, provider);
            body.last_id = add_provider_prefix(&body.last_id, provider);
        }
        TransformResponse::ModelListGemini(
            gproxy_protocol::gemini::model_list::response::GeminiModelListResponse::Success {
                body,
                ..
            },
        ) => {
            for model in &mut body.models {
                model.name = add_provider_prefix(&model.name, provider);
                if let Some(base_model_id) = model.base_model_id.as_mut() {
                    *base_model_id = add_provider_prefix(base_model_id, provider);
                }
            }
        }
        TransformResponse::ModelGetOpenAi(
            gproxy_protocol::openai::model_get::response::OpenAiModelGetResponse::Success {
                body,
                ..
            },
        ) => {
            body.id = add_provider_prefix(&body.id, provider);
        }
        TransformResponse::ModelGetClaude(
            gproxy_protocol::claude::model_get::response::ClaudeModelGetResponse::Success {
                body,
                ..
            },
        ) => {
            body.id = add_provider_prefix(&body.id, provider);
        }
        TransformResponse::ModelGetGemini(
            gproxy_protocol::gemini::model_get::response::GeminiModelGetResponse::Success {
                body,
                ..
            },
        ) => {
            body.name = add_provider_prefix(&body.name, provider);
            if let Some(base_model_id) = body.base_model_id.as_mut() {
                *base_model_id = add_provider_prefix(base_model_id, provider);
            }
        }
        TransformResponse::GenerateContentOpenAiResponse(
            gproxy_protocol::openai::create_response::response::OpenAiCreateResponseResponse::Success {
                body,
                ..
            },
        ) => {
            body.model = add_provider_prefix(&body.model, provider);
        }
        TransformResponse::GenerateContentOpenAiChatCompletions(
            gproxy_protocol::openai::create_chat_completions::response::OpenAiChatCompletionsResponse::Success {
                body,
                ..
            },
        ) => {
            body.model = add_provider_prefix(&body.model, provider);
        }
        TransformResponse::GenerateContentClaude(value) => {
            if let gproxy_protocol::claude::create_message::response::ClaudeCreateMessageResponse::Success {
                body,
                ..
            } = value
                && let Some(raw) = serialize_claude_model(&body.model)
            {
                body.model = ClaudeModel::Custom(add_provider_prefix(&raw, provider));
            }
        }
        TransformResponse::EmbeddingOpenAi(
            gproxy_protocol::openai::embeddings::response::OpenAiEmbeddingsResponse::Success {
                body,
                ..
            },
        ) => {
            body.model = add_provider_prefix(&body.model, provider);
        }
        TransformResponse::OpenAiResponseWebSocket(messages) => {
            for message in messages {
                if let gproxy_protocol::openai::create_response::websocket::types::OpenAiCreateResponseWebSocketServerMessage::StreamEvent(event) =
                    message
                {
                    add_prefix_to_openai_response_stream_event(event, provider);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn serialize_claude_model(model: &ClaudeModel) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

pub(super) fn add_prefix_to_openai_response_stream_event(
    event: &mut ResponseStreamEvent,
    provider: &str,
) {
    match event {
        ResponseStreamEvent::Created { response, .. }
        | ResponseStreamEvent::Queued { response, .. }
        | ResponseStreamEvent::InProgress { response, .. }
        | ResponseStreamEvent::Failed { response, .. }
        | ResponseStreamEvent::Incomplete { response, .. }
        | ResponseStreamEvent::Completed { response, .. } => {
            response.model = add_provider_prefix(&response.model, provider);
        }
        _ => {}
    }
}
