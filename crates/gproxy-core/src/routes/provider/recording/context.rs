use super::*;

pub(crate) fn serialize_claude_model(model: &ClaudeModel) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

pub(crate) fn serialize_openai_embedding_model(model: &OpenAiEmbeddingModel) -> Option<String> {
    serde_json::to_value(model)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
}

pub(crate) fn json_pointer_string(value: &serde_json::Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(crate) fn extract_model_from_payload(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Option<String> {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    match (operation, protocol) {
        (OperationFamily::ModelList, _) => None,

        (OperationFamily::ModelGet, ProtocolKind::OpenAi) => {
            json_pointer_string(&value, "/path/model")
        }
        (OperationFamily::ModelGet, ProtocolKind::Claude) => {
            json_pointer_string(&value, "/path/model_id")
        }
        (OperationFamily::ModelGet, ProtocolKind::Gemini)
        | (OperationFamily::ModelGet, ProtocolKind::GeminiNDJson) => {
            json_pointer_string(&value, "/path/name")
                .or_else(|| json_pointer_string(&value, "/path/model"))
        }

        (OperationFamily::CountToken, ProtocolKind::OpenAi)
        | (OperationFamily::CountToken, ProtocolKind::Claude) => {
            json_pointer_string(&value, "/model")
                .or_else(|| json_pointer_string(&value, "/body/model"))
        }
        (OperationFamily::CountToken, ProtocolKind::Gemini)
        | (OperationFamily::CountToken, ProtocolKind::GeminiNDJson) => {
            json_pointer_string(&value, "/body/generate_content_request/model")
                .or_else(|| json_pointer_string(&value, "/path/model"))
        }

        (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi)
        | (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi)
        | (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion)
        | (OperationFamily::GenerateContent, ProtocolKind::Claude)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::Claude)
        | (OperationFamily::Embedding, ProtocolKind::OpenAi)
        | (OperationFamily::Compact, ProtocolKind::OpenAi) => json_pointer_string(&value, "/model")
            .or_else(|| json_pointer_string(&value, "/body/model")),
        (OperationFamily::GenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::GenerateContent, ProtocolKind::GeminiNDJson)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
        | (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson)
        | (OperationFamily::GeminiLive, ProtocolKind::Gemini)
        | (OperationFamily::Embedding, ProtocolKind::Gemini)
        | (OperationFamily::Embedding, ProtocolKind::GeminiNDJson) => {
            json_pointer_string(&value, "/path/model")
                .or_else(|| json_pointer_string(&value, "/body/setup/model"))
        }
        _ => None,
    }
}

pub(crate) fn usage_request_context_from_transform_request(
    request: &TransformRequest,
) -> UsageRequestContext {
    UsageRequestContext {
        operation: request.operation(),
        protocol: request.protocol(),
        model: extract_model_from_request(request),
        body_for_estimate: serde_json::to_vec(request).ok(),
    }
}

pub(crate) fn usage_request_context_from_payload(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> UsageRequestContext {
    UsageRequestContext {
        operation,
        protocol,
        model: extract_model_from_payload(operation, protocol, body),
        body_for_estimate: Some(body.to_vec()),
    }
}

pub(crate) fn extract_model_from_request(request: &TransformRequest) -> Option<String> {
    match request {
        TransformRequest::ModelListOpenAi(_)
        | TransformRequest::ModelListClaude(_)
        | TransformRequest::ModelListGemini(_) => None,

        TransformRequest::ModelGetOpenAi(value) => Some(value.path.model.clone()),
        TransformRequest::ModelGetClaude(value) => Some(value.path.model_id.clone()),
        TransformRequest::ModelGetGemini(value) => Some(value.path.name.clone()),

        TransformRequest::CountTokenOpenAi(value) => value.body.model.clone(),
        TransformRequest::CountTokenClaude(value) => serialize_claude_model(&value.body.model),
        TransformRequest::CountTokenGemini(value) => {
            if let Some(generate_request) = value.body.generate_content_request.as_ref() {
                Some(generate_request.model.clone())
            } else {
                Some(value.path.model.clone())
            }
        }

        TransformRequest::GenerateContentOpenAiResponse(value)
        | TransformRequest::StreamGenerateContentOpenAiResponse(value) => value.body.model.clone(),

        TransformRequest::GenerateContentOpenAiChatCompletions(value)
        | TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            Some(value.body.model.clone())
        }

        TransformRequest::GenerateContentClaude(value)
        | TransformRequest::StreamGenerateContentClaude(value) => {
            serialize_claude_model(&value.body.model)
        }

        TransformRequest::GenerateContentGemini(value) => Some(value.path.model.clone()),
        TransformRequest::StreamGenerateContentGeminiSse(value)
        | TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
            Some(value.path.model.clone())
        }
        TransformRequest::OpenAiResponseWebSocket(value) => value.body.as_ref().and_then(|body| {
            if let gproxy_protocol::openai::create_response::websocket::types::OpenAiCreateResponseWebSocketClientMessage::ResponseCreate(create) =
                body
            {
                create.request.model.clone()
            } else {
                None
            }
        }),
        TransformRequest::GeminiLive(value) => value.body.as_ref().and_then(|body| {
            if let gproxy_protocol::gemini::live::types::GeminiBidiGenerateContentClientMessageType::Setup { setup } =
                &body.message_type
            {
                Some(setup.model.clone())
            } else {
                None
            }
        }),

        TransformRequest::EmbeddingOpenAi(value) => {
            serialize_openai_embedding_model(&value.body.model)
        }
        TransformRequest::EmbeddingGemini(value) => Some(value.path.model.clone()),

        TransformRequest::CompactOpenAi(value) => Some(value.body.model.clone()),
    }
}

pub(crate) fn normalize_usage_model(model: Option<String>) -> Option<String> {
    model.and_then(|value| {
        let trimmed = value.trim().trim_start_matches('/');
        if trimmed.is_empty() {
            return None;
        }
        let normalized = if let Some(stripped) = trimmed.strip_prefix("models/") {
            stripped.trim()
        } else {
            trimmed
        };
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_string())
        }
    })
}
