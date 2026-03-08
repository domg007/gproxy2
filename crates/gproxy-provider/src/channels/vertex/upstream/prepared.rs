use super::*;

impl VertexPreparedRequest {
    pub(super) fn from_transform_request(
        request: &TransformRequest,
    ) -> Result<Self, UpstreamError> {
        let extra_headers = extra_headers_from_transform_request(request);
        let mut prepared = match request {
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
                extra_headers: Vec::new(),
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
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::CreateVideoGemini(value) => {
                let model_id = normalize_vertex_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    endpoint: VertexEndpoint::Project(format!(
                        "publishers/google/models/{model_id}:predictLongRunning"
                    )),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model_id),
                    model_response_kind: Some(VertexModelResponseKind::CreateVideo),
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::VideoGetGemini(value) => {
                let model_id = vertex_video_operation_model_id(value.path.operation.as_str())?;
                Ok(Self {
                    method: WreqMethod::POST,
                    endpoint: VertexEndpoint::Project(format!(
                        "publishers/google/models/{model_id}:fetchPredictOperation"
                    )),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&json!({
                            "operationName": value.path.operation,
                        }))
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model_id),
                    model_response_kind: Some(VertexModelResponseKind::VideoGet),
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::VideoContentGetGemini(value) => {
                let model_id = vertex_video_operation_model_id(value.path.operation.as_str())?;
                Ok(Self {
                    method: WreqMethod::POST,
                    endpoint: VertexEndpoint::Project(format!(
                        "publishers/google/models/{model_id}:fetchPredictOperation"
                    )),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&json!({
                            "operationName": value.path.operation,
                        }))
                        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model_id),
                    model_response_kind: Some(VertexModelResponseKind::VideoContentGet),
                    extra_headers: Vec::new(),
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
                    extra_headers: Vec::new(),
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
                    extra_headers: Vec::new(),
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
                    extra_headers: Vec::new(),
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
                    extra_headers: Vec::new(),
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
                    extra_headers: Vec::new(),
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
                    extra_headers: Vec::new(),
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }?;
        prepared.extra_headers = extra_headers;
        Ok(prepared)
    }

    pub(super) fn from_payload(
        operation: OperationFamily,
        protocol: ProtocolKind,
        body: &[u8],
    ) -> Result<Self, UpstreamError> {
        fn parse_gemini_payload_wrapper(
            value: &Value,
        ) -> Result<(String, Value, Option<String>), UpstreamError> {
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

        let payload_value = serde_json::from_slice::<Value>(body)
            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
        let extra_headers = extra_headers_from_payload_value(&payload_value);

        match (operation, protocol) {
            (OperationFamily::CountToken, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(&payload_value)?;
                let model_id = normalize_vertex_model_name(model.as_str());
                let body = vertex_count_tokens_payload(model_id.as_str(), &body_value)?;
                Ok(Self {
                    method: WreqMethod::POST,
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
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(&payload_value)?;
                let model_id = normalize_vertex_model_name(model.as_str());
                let body = vertex_generate_payload(model_id.as_str(), &body_value)?;
                Ok(Self {
                    method: WreqMethod::POST,
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
                    extra_headers,
                })
            }
            (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
                let (model, body_value, alt) = parse_gemini_payload_wrapper(&payload_value)?;
                let model_id = normalize_vertex_model_name(model.as_str());
                let body = vertex_generate_payload(model_id.as_str(), &body_value)?;
                let query = match protocol {
                    ProtocolKind::Gemini => Some("alt=sse".to_string()),
                    ProtocolKind::GeminiNDJson => alt.map(|_| "alt=sse".to_string()),
                    _ => None,
                };
                Ok(Self {
                    method: WreqMethod::POST,
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
                    extra_headers,
                })
            }
            (OperationFamily::Embedding, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(&payload_value)?;
                let model_id = normalize_vertex_model_name(model.as_str());
                let body = vertex_embedding_predict_payload(&body_value)?;
                Ok(Self {
                    method: WreqMethod::POST,
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
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAiChatCompletion)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAiChatCompletion) => {
                let mut body_json = payload_body_value(&payload_value);
                let mut model: Option<String> = None;
                if let Some(map) = body_json.as_object_mut()
                    && let Some(raw_model) = map.get("model").and_then(Value::as_str)
                {
                    let normalized = normalize_vertex_openai_model(raw_model);
                    map.insert("model".to_string(), Value::String(normalized.clone()));
                    model = Some(normalized);
                }
                Ok(Self {
                    method: WreqMethod::POST,
                    endpoint: VertexEndpoint::Project(
                        "endpoints/openapi/chat/completions".to_string(),
                    ),
                    query: None,
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model,
                    model_response_kind: None,
                    extra_headers,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}
