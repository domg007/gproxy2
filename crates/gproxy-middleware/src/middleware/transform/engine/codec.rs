use super::*;

pub(super) fn request_extra_headers(
    input: &TransformRequest,
) -> std::collections::BTreeMap<String, String> {
    match input {
        TransformRequest::ModelListOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::ModelListClaude(value) => value.headers.extra.clone(),
        TransformRequest::ModelListGemini(value) => value.headers.extra.clone(),
        TransformRequest::ModelGetOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::ModelGetClaude(value) => value.headers.extra.clone(),
        TransformRequest::ModelGetGemini(value) => value.headers.extra.clone(),
        TransformRequest::CountTokenOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::CountTokenClaude(value) => value.headers.extra.clone(),
        TransformRequest::CountTokenGemini(value) => value.headers.extra.clone(),
        TransformRequest::GenerateContentOpenAiResponse(value) => value.headers.extra.clone(),
        TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
            value.headers.extra.clone()
        }
        TransformRequest::GenerateContentClaude(value) => value.headers.extra.clone(),
        TransformRequest::GenerateContentGemini(value) => value.headers.extra.clone(),
        TransformRequest::StreamGenerateContentOpenAiResponse(value) => value.headers.extra.clone(),
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            value.headers.extra.clone()
        }
        TransformRequest::StreamGenerateContentClaude(value) => value.headers.extra.clone(),
        TransformRequest::StreamGenerateContentGeminiSse(value) => value.headers.extra.clone(),
        TransformRequest::StreamGenerateContentGeminiNdjson(value) => value.headers.extra.clone(),
        TransformRequest::CreateImageOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::StreamCreateImageOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::CreateImageEditOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::StreamCreateImageEditOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::OpenAiResponseWebSocket(value) => value.headers.extra.clone(),
        TransformRequest::GeminiLive(value) => value.headers.extra.clone(),
        TransformRequest::EmbeddingOpenAi(value) => value.headers.extra.clone(),
        TransformRequest::EmbeddingGemini(value) => value.headers.extra.clone(),
        TransformRequest::CompactOpenAi(value) => value.headers.extra.clone(),
    }
}

pub(super) fn apply_request_extra_headers(
    request: &mut TransformRequest,
    extra: std::collections::BTreeMap<String, String>,
) {
    match request {
        TransformRequest::ModelListOpenAi(value) => value.headers.extra = extra,
        TransformRequest::ModelListClaude(value) => value.headers.extra = extra,
        TransformRequest::ModelListGemini(value) => value.headers.extra = extra,
        TransformRequest::ModelGetOpenAi(value) => value.headers.extra = extra,
        TransformRequest::ModelGetClaude(value) => value.headers.extra = extra,
        TransformRequest::ModelGetGemini(value) => value.headers.extra = extra,
        TransformRequest::CountTokenOpenAi(value) => value.headers.extra = extra,
        TransformRequest::CountTokenClaude(value) => value.headers.extra = extra,
        TransformRequest::CountTokenGemini(value) => value.headers.extra = extra,
        TransformRequest::GenerateContentOpenAiResponse(value) => value.headers.extra = extra,
        TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
            value.headers.extra = extra
        }
        TransformRequest::GenerateContentClaude(value) => value.headers.extra = extra,
        TransformRequest::GenerateContentGemini(value) => value.headers.extra = extra,
        TransformRequest::StreamGenerateContentOpenAiResponse(value) => value.headers.extra = extra,
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            value.headers.extra = extra
        }
        TransformRequest::StreamGenerateContentClaude(value) => value.headers.extra = extra,
        TransformRequest::StreamGenerateContentGeminiSse(value) => value.headers.extra = extra,
        TransformRequest::StreamGenerateContentGeminiNdjson(value) => value.headers.extra = extra,
        TransformRequest::CreateImageOpenAi(value) => value.headers.extra = extra,
        TransformRequest::StreamCreateImageOpenAi(value) => value.headers.extra = extra,
        TransformRequest::CreateImageEditOpenAi(value) => value.headers.extra = extra,
        TransformRequest::StreamCreateImageEditOpenAi(value) => value.headers.extra = extra,
        TransformRequest::OpenAiResponseWebSocket(value) => value.headers.extra = extra,
        TransformRequest::GeminiLive(value) => value.headers.extra = extra,
        TransformRequest::EmbeddingOpenAi(value) => value.headers.extra = extra,
        TransformRequest::EmbeddingGemini(value) => value.headers.extra = extra,
        TransformRequest::CompactOpenAi(value) => value.headers.extra = extra,
    }
}

pub(super) fn decode_json<T: DeserializeOwned>(
    kind: &'static str,
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Result<T, MiddlewareTransformError> {
    serde_json::from_slice(body).map_err(|err| MiddlewareTransformError::JsonDecode {
        kind,
        operation,
        protocol,
        message: err.to_string(),
    })
}

pub(super) fn encode_json<T: Serialize>(
    kind: &'static str,
    operation: OperationFamily,
    protocol: ProtocolKind,
    value: &T,
) -> Result<Vec<u8>, MiddlewareTransformError> {
    serde_json::to_vec(value).map_err(|err| MiddlewareTransformError::JsonEncode {
        kind,
        operation,
        protocol,
        message: err.to_string(),
    })
}

pub(super) async fn collect_body_bytes(
    mut body: TransformBodyStream,
) -> Result<Vec<u8>, MiddlewareTransformError> {
    let mut out = Vec::new();
    while let Some(chunk) = body.next().await {
        out.extend_from_slice(&chunk?);
    }
    Ok(out)
}

pub(super) fn bytes_to_body_stream(bytes: Vec<u8>) -> TransformBodyStream {
    Box::pin(futures_stream::once(async move { Ok(Bytes::from(bytes)) }))
}

pub fn decode_request_payload(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Result<TransformRequest, MiddlewareTransformError> {
    match (operation, protocol) {
        (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(
            TransformRequest::ModelListOpenAi(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::ModelList, ProtocolKind::Claude) => Ok(
            TransformRequest::ModelListClaude(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::ModelList, ProtocolKind::Gemini) => Ok(
            TransformRequest::ModelListGemini(decode_json("request", operation, protocol, body)?),
        ),

        (OperationFamily::ModelGet, ProtocolKind::OpenAi) => Ok(TransformRequest::ModelGetOpenAi(
            decode_json("request", operation, protocol, body)?,
        )),
        (OperationFamily::ModelGet, ProtocolKind::Claude) => Ok(TransformRequest::ModelGetClaude(
            decode_json("request", operation, protocol, body)?,
        )),
        (OperationFamily::ModelGet, ProtocolKind::Gemini) => Ok(TransformRequest::ModelGetGemini(
            decode_json("request", operation, protocol, body)?,
        )),

        (OperationFamily::CountToken, ProtocolKind::OpenAi) => Ok(
            TransformRequest::CountTokenOpenAi(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::CountToken, ProtocolKind::Claude) => Ok(
            TransformRequest::CountTokenClaude(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::CountToken, ProtocolKind::Gemini) => Ok(
            TransformRequest::CountTokenGemini(decode_json("request", operation, protocol, body)?),
        ),

        (OperationFamily::GenerateContent, ProtocolKind::OpenAi) => {
            Ok(TransformRequest::GenerateContentOpenAiResponse(
                decode_json("request", operation, protocol, body)?,
            ))
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion) => {
            Ok(TransformRequest::GenerateContentOpenAiChatCompletions(
                decode_json("request", operation, protocol, body)?,
            ))
        }
        (OperationFamily::GenerateContent, ProtocolKind::Claude) => {
            Ok(TransformRequest::GenerateContentClaude(decode_json(
                "request", operation, protocol, body,
            )?))
        }
        (OperationFamily::GenerateContent, ProtocolKind::Gemini) => {
            Ok(TransformRequest::GenerateContentGemini(decode_json(
                "request", operation, protocol, body,
            )?))
        }

        (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
            Ok(TransformRequest::StreamGenerateContentOpenAiResponse(
                decode_json("request", operation, protocol, body)?,
            ))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => Ok(
            TransformRequest::StreamGenerateContentOpenAiChatCompletions(decode_json(
                "request", operation, protocol, body,
            )?),
        ),
        (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
            Ok(TransformRequest::StreamGenerateContentClaude(decode_json(
                "request", operation, protocol, body,
            )?))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini) => {
            let request: GeminiStreamGenerateContentRequest =
                decode_json("request", operation, protocol, body)?;
            Ok(TransformRequest::StreamGenerateContentGeminiSse(request))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
            let request: GeminiStreamGenerateContentRequest =
                decode_json("request", operation, protocol, body)?;
            Ok(TransformRequest::StreamGenerateContentGeminiNdjson(request))
        }

        (OperationFamily::CreateImage, ProtocolKind::OpenAi) => Ok(
            TransformRequest::CreateImageOpenAi(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::StreamCreateImage, ProtocolKind::OpenAi) => {
            Ok(TransformRequest::StreamCreateImageOpenAi(decode_json(
                "request", operation, protocol, body,
            )?))
        }
        (OperationFamily::CreateImageEdit, ProtocolKind::OpenAi) => {
            Ok(TransformRequest::CreateImageEditOpenAi(decode_json(
                "request", operation, protocol, body,
            )?))
        }
        (OperationFamily::StreamCreateImageEdit, ProtocolKind::OpenAi) => {
            Ok(TransformRequest::StreamCreateImageEditOpenAi(decode_json(
                "request", operation, protocol, body,
            )?))
        }

        (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi) => {
            Ok(TransformRequest::OpenAiResponseWebSocket(decode_json(
                "request", operation, protocol, body,
            )?))
        }
        (OperationFamily::GeminiLive, ProtocolKind::Gemini) => Ok(TransformRequest::GeminiLive(
            decode_json("request", operation, protocol, body)?,
        )),

        (OperationFamily::Embedding, ProtocolKind::OpenAi) => Ok(
            TransformRequest::EmbeddingOpenAi(decode_json("request", operation, protocol, body)?),
        ),
        (OperationFamily::Embedding, ProtocolKind::Gemini) => Ok(
            TransformRequest::EmbeddingGemini(decode_json("request", operation, protocol, body)?),
        ),

        (OperationFamily::Compact, ProtocolKind::OpenAi) => Ok(TransformRequest::CompactOpenAi(
            decode_json("request", operation, protocol, body)?,
        )),

        _ => Err(MiddlewareTransformError::Unsupported(
            "unsupported request payload operation/protocol",
        )),
    }
}

pub(crate) fn encode_request_payload(
    request: TransformRequest,
) -> Result<Vec<u8>, MiddlewareTransformError> {
    let operation = request.operation();
    let protocol = request.protocol();

    match request {
        TransformRequest::ModelListOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelListClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelListGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelGetOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelGetClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::ModelGetGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CountTokenOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CountTokenClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CountTokenGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GenerateContentOpenAiResponse(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GenerateContentOpenAiChatCompletions(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GenerateContentClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GenerateContentGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentOpenAiResponse(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentOpenAiChatCompletions(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentClaude(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentGeminiSse(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CreateImageOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamCreateImageOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CreateImageEditOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::StreamCreateImageEditOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::OpenAiResponseWebSocket(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::GeminiLive(value) => encode_json("request", operation, protocol, &value),
        TransformRequest::EmbeddingOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::EmbeddingGemini(value) => {
            encode_json("request", operation, protocol, &value)
        }
        TransformRequest::CompactOpenAi(value) => {
            encode_json("request", operation, protocol, &value)
        }
    }
}

pub fn decode_response_payload(
    operation: OperationFamily,
    protocol: ProtocolKind,
    body: &[u8],
) -> Result<TransformResponse, MiddlewareTransformError> {
    match (operation, protocol) {
        (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(
            TransformResponse::ModelListOpenAi(decode_json("response", operation, protocol, body)?),
        ),
        (OperationFamily::ModelList, ProtocolKind::Claude) => Ok(
            TransformResponse::ModelListClaude(decode_json("response", operation, protocol, body)?),
        ),
        (OperationFamily::ModelList, ProtocolKind::Gemini) => Ok(
            TransformResponse::ModelListGemini(decode_json("response", operation, protocol, body)?),
        ),

        (OperationFamily::ModelGet, ProtocolKind::OpenAi) => Ok(TransformResponse::ModelGetOpenAi(
            decode_json("response", operation, protocol, body)?,
        )),
        (OperationFamily::ModelGet, ProtocolKind::Claude) => Ok(TransformResponse::ModelGetClaude(
            decode_json("response", operation, protocol, body)?,
        )),
        (OperationFamily::ModelGet, ProtocolKind::Gemini) => Ok(TransformResponse::ModelGetGemini(
            decode_json("response", operation, protocol, body)?,
        )),

        (OperationFamily::CountToken, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::CountTokenOpenAi(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::CountToken, ProtocolKind::Claude) => {
            Ok(TransformResponse::CountTokenClaude(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::CountToken, ProtocolKind::Gemini) => {
            Ok(TransformResponse::CountTokenGemini(decode_json(
                "response", operation, protocol, body,
            )?))
        }

        (OperationFamily::GenerateContent, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::GenerateContentOpenAiResponse(
                decode_json("response", operation, protocol, body)?,
            ))
        }
        (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion) => {
            Ok(TransformResponse::GenerateContentOpenAiChatCompletions(
                decode_json("response", operation, protocol, body)?,
            ))
        }
        (OperationFamily::GenerateContent, ProtocolKind::Claude) => {
            Ok(TransformResponse::GenerateContentClaude(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::GenerateContent, ProtocolKind::Gemini) => {
            Ok(TransformResponse::GenerateContentGemini(decode_json(
                "response", operation, protocol, body,
            )?))
        }

        (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::StreamGenerateContentOpenAiResponse(
                decode_json("response", operation, protocol, body)?,
            ))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => Ok(
            TransformResponse::StreamGenerateContentOpenAiChatCompletions(decode_json(
                "response", operation, protocol, body,
            )?),
        ),
        (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
            Ok(TransformResponse::StreamGenerateContentClaude(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini) => {
            let response: GeminiStreamGenerateContentResponse =
                decode_json("response", operation, protocol, body)?;
            Ok(TransformResponse::StreamGenerateContentGeminiSse(
                ensure_gemini_sse_stream(response),
            ))
        }
        (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
            let response: GeminiStreamGenerateContentResponse =
                decode_json("response", operation, protocol, body)?;
            Ok(TransformResponse::StreamGenerateContentGeminiNdjson(
                ensure_gemini_ndjson_stream(response),
            ))
        }

        (OperationFamily::CreateImage, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::CreateImageOpenAi(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::StreamCreateImage, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::StreamCreateImageOpenAi(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::CreateImageEdit, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::CreateImageEditOpenAi(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::StreamCreateImageEdit, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::StreamCreateImageEditOpenAi(decode_json(
                "response", operation, protocol, body,
            )?))
        }

        (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi) => {
            Ok(TransformResponse::OpenAiResponseWebSocket(decode_json(
                "response", operation, protocol, body,
            )?))
        }
        (OperationFamily::GeminiLive, ProtocolKind::Gemini) => Ok(TransformResponse::GeminiLive(
            decode_json("response", operation, protocol, body)?,
        )),

        (OperationFamily::Embedding, ProtocolKind::OpenAi) => Ok(
            TransformResponse::EmbeddingOpenAi(decode_json("response", operation, protocol, body)?),
        ),
        (OperationFamily::Embedding, ProtocolKind::Gemini) => Ok(
            TransformResponse::EmbeddingGemini(decode_json("response", operation, protocol, body)?),
        ),

        (OperationFamily::Compact, ProtocolKind::OpenAi) => Ok(TransformResponse::CompactOpenAi(
            decode_json("response", operation, protocol, body)?,
        )),

        _ => Err(MiddlewareTransformError::Unsupported(
            "unsupported response payload operation/protocol",
        )),
    }
}

pub(crate) fn encode_response_payload(
    response: TransformResponse,
) -> Result<Vec<u8>, MiddlewareTransformError> {
    let operation = response.operation();
    let protocol = response.protocol();

    match response {
        TransformResponse::ModelListOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelListClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelListGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelGetOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelGetClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::ModelGetGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::CountTokenOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::CountTokenClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::CountTokenGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GenerateContentOpenAiResponse(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GenerateContentOpenAiChatCompletions(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GenerateContentClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GenerateContentGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamGenerateContentOpenAiResponse(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamGenerateContentOpenAiChatCompletions(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamGenerateContentClaude(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamGenerateContentGeminiSse(value) => encode_json(
            "response",
            operation,
            protocol,
            &ensure_gemini_sse_stream(value),
        ),
        TransformResponse::StreamGenerateContentGeminiNdjson(value) => encode_json(
            "response",
            operation,
            protocol,
            &ensure_gemini_ndjson_stream(value),
        ),
        TransformResponse::CreateImageOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamCreateImageOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::CreateImageEditOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::StreamCreateImageEditOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::OpenAiResponseWebSocket(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::GeminiLive(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::EmbeddingOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::EmbeddingGemini(value) => {
            encode_json("response", operation, protocol, &value)
        }
        TransformResponse::CompactOpenAi(value) => {
            encode_json("response", operation, protocol, &value)
        }
    }
}
