use super::*;

impl CodexPreparedRequest {
    pub(super) fn from_transform_request(
        request: &TransformRequest,
    ) -> Result<Self, UpstreamError> {
        let extra_headers = extra_headers_from_transform_request(request);
        let mut prepared = match request {
            TransformRequest::ModelListOpenAi(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: codex_models_path(),
                body: None,
                model: None,
                kind: CodexRequestKind::ModelList,
                extra_headers: Vec::new(),
            }),
            TransformRequest::ModelGetOpenAi(value) => {
                let target = normalize_model_id(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: codex_models_path(),
                    body: None,
                    model: Some(target.clone()),
                    kind: CodexRequestKind::ModelGet { target },
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::GenerateContentOpenAiResponse(value)
            | TransformRequest::StreamGenerateContentOpenAiResponse(value) => {
                let mut body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_codex_response_request_body(
                    &mut body,
                    matches!(
                        request,
                        TransformRequest::StreamGenerateContentOpenAiResponse(_)
                    ),
                );
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/responses".to_string(),
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: value.body.model.clone(),
                    kind: CodexRequestKind::Forward,
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::CompactOpenAi(value) => {
                let mut body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                normalize_codex_compact_request_body(&mut body);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/responses/compact".to_string(),
                    body: Some(
                        serde_json::to_vec(&body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(normalize_model_id(value.body.model.as_str())),
                    kind: CodexRequestKind::Forward,
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::OpenAiResponseWebSocket(value) => {
                let transformed = transform_openai_ws_request_to_stream(
                    TransformRequest::OpenAiResponseWebSocket(value.clone()),
                )?;
                Self::from_transform_request(&transformed)
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }?;
        prepared.extra_headers = extra_headers;
        if matches!(prepared.kind, CodexRequestKind::Forward) {
            ensure_codex_session_id_header(&mut prepared.extra_headers, prepared.body.as_deref());
        }
        Ok(prepared)
    }

    pub(super) fn from_payload(
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

        match (operation, protocol) {
            (OperationFamily::ModelList, ProtocolKind::OpenAi) => Ok(Self {
                method: WreqMethod::GET,
                path: codex_models_path(),
                body: None,
                model: None,
                kind: CodexRequestKind::ModelList,
                extra_headers,
            }),
            (OperationFamily::ModelGet, ProtocolKind::OpenAi) => {
                let Some(target_raw) = json_pointer_string(&payload_value, "/path/model") else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in codex model_get payload".to_string(),
                    ));
                };
                let target = normalize_model_id(target_raw.as_str());
                Ok(Self {
                    method: WreqMethod::GET,
                    path: codex_models_path(),
                    body: None,
                    model: Some(target.clone()),
                    kind: CodexRequestKind::ModelGet { target },
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::OpenAi)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::OpenAi) => {
                let mut body_json = payload_body_value(&payload_value);
                normalize_codex_response_request_body(
                    &mut body_json,
                    operation == OperationFamily::StreamGenerateContent,
                );
                let model = body_json
                    .get("model")
                    .and_then(Value::as_str)
                    .map(normalize_model_id);
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/responses".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model,
                    kind: CodexRequestKind::Forward,
                    extra_headers,
                })
            }
            (OperationFamily::Compact, ProtocolKind::OpenAi) => {
                let mut body_json = payload_body_value(&payload_value);
                normalize_codex_compact_request_body(&mut body_json);
                let model = body_json
                    .get("model")
                    .and_then(Value::as_str)
                    .map(normalize_model_id);
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/responses/compact".to_string(),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model,
                    kind: CodexRequestKind::Forward,
                    extra_headers,
                })
            }
            (OperationFamily::OpenAiResponseWebSocket, ProtocolKind::OpenAi) => {
                let request = gproxy_middleware::decode_request_payload(operation, protocol, body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                let transformed = transform_openai_ws_request_to_stream(request)?;
                Self::from_transform_request(&transformed)
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
        .map(|mut prepared| {
            if matches!(prepared.kind, CodexRequestKind::Forward) {
                ensure_codex_session_id_header(
                    &mut prepared.extra_headers,
                    prepared.body.as_deref(),
                );
            }
            prepared
        })
    }
}
