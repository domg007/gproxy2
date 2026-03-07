use super::*;

impl GeminiCliPreparedRequest {
    pub(super) fn from_transform_request(
        request: &TransformRequest,
    ) -> Result<Self, UpstreamError> {
        let extra_headers = extra_headers_from_transform_request(request);
        let mut prepared = match request {
            TransformRequest::ModelListGemini(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: String::new(),
                query: None,
                body: None,
                model: None,
                kind: GeminiCliRequestKind::LocalModelList {
                    page_size: value.query.page_size,
                    page_token: value.query.page_token.clone(),
                },
                extra_headers: Vec::new(),
            }),
            TransformRequest::ModelGetGemini(value) => Ok(Self {
                method: to_wreq_method(&value.method)?,
                path: String::new(),
                query: None,
                body: None,
                model: Some(normalize_model_id(value.path.name.as_str())),
                kind: GeminiCliRequestKind::LocalModelGet {
                    target: normalize_model_name(value.path.name.as_str()),
                },
                extra_headers: Vec::new(),
            }),
            TransformRequest::CountTokenGemini(value) => {
                let model = normalize_model_id(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1internal:countTokens".to_string(),
                    query: None,
                    body: Some(geminicli_count_tokens_request(model.as_str(), &value.body)?),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: false,
                    },
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::GenerateContentGemini(value) => {
                let model = normalize_model_id(value.path.model.as_str());
                let mut request_body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                strip_geminicli_unsupported_generation_config(&mut request_body);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1internal:generateContent".to_string(),
                    query: None,
                    body: Some(request_body),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::StreamGenerateContentGeminiSse(value) => {
                let model = normalize_model_id(value.path.model.as_str());
                let mut request_body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                strip_geminicli_unsupported_generation_config(&mut request_body);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1internal:streamGenerateContent".to_string(),
                    query: Some("alt=sse".to_string()),
                    body: Some(request_body),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::StreamGenerateContentGeminiNdjson(value) => {
                let model = normalize_model_id(value.path.model.as_str());
                let mut request_body = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                strip_geminicli_unsupported_generation_config(&mut request_body);
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: "/v1internal:streamGenerateContent".to_string(),
                    query: Some("alt=sse".to_string()),
                    body: Some(request_body),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers: Vec::new(),
                })
            }
            TransformRequest::EmbeddingGemini(value) => {
                let model = normalize_model_name(value.path.model.as_str());
                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: format!("/v1beta/{model}:embedContent"),
                    query: None,
                    body: Some(
                        serde_json::to_value(&value.body)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(normalize_model_id(value.path.model.as_str())),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: false,
                    },
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
        ) -> Result<ParsedGeminiPayload, UpstreamError> {
            let model = value
                .pointer("/path/model")
                .or_else(|| value.pointer("/path/name"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let body_value = value.get("body").cloned();
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
            (OperationFamily::ModelList, ProtocolKind::Gemini) => {
                let page_size = payload_value
                    .pointer("/query/page_size")
                    .and_then(Value::as_u64)
                    .map(|value| value as u32);
                let page_token = payload_value
                    .pointer("/query/page_token")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                Ok(Self {
                    method: WreqMethod::GET,
                    path: String::new(),
                    query: None,
                    body: None,
                    model: None,
                    kind: GeminiCliRequestKind::LocalModelList {
                        page_size,
                        page_token,
                    },
                    extra_headers,
                })
            }
            (OperationFamily::ModelGet, ProtocolKind::Gemini) => {
                let Some(target) = payload_value
                    .pointer("/path/name")
                    .or_else(|| payload_value.pointer("/path/model"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.name in geminicli model_get payload".to_string(),
                    ));
                };
                Ok(Self {
                    method: WreqMethod::GET,
                    path: String::new(),
                    query: None,
                    body: None,
                    model: Some(normalize_model_id(target.as_str())),
                    kind: GeminiCliRequestKind::LocalModelGet {
                        target: normalize_model_name(target.as_str()),
                    },
                    extra_headers,
                })
            }
            (OperationFamily::CountToken, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(&payload_value)?;
                let Some(model) = model else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in geminicli count_tokens payload".to_string(),
                    ));
                };
                let Some(body_value) = body_value else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing body in geminicli count_tokens payload".to_string(),
                    ));
                };
                let model = normalize_model_id(model.as_str());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1internal:countTokens".to_string(),
                    query: None,
                    body: Some(geminicli_count_tokens_request(model.as_str(), &body_value)?),
                    model: Some(model),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: false,
                    },
                    extra_headers,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(&payload_value)?;
                let Some(model) = model else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in geminicli generate payload".to_string(),
                    ));
                };
                let Some(mut body_value) = body_value else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing body in geminicli generate payload".to_string(),
                    ));
                };
                strip_geminicli_unsupported_generation_config(&mut body_value);
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1internal:generateContent".to_string(),
                    query: None,
                    body: Some(body_value),
                    model: Some(normalize_model_id(model.as_str())),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers,
                })
            }
            (OperationFamily::StreamGenerateContent, ProtocolKind::Gemini)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::GeminiNDJson) => {
                let (model, body_value, alt) = parse_gemini_payload_wrapper(&payload_value)?;
                let Some(model) = model else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in geminicli stream payload".to_string(),
                    ));
                };
                let Some(mut body_value) = body_value else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing body in geminicli stream payload".to_string(),
                    ));
                };
                strip_geminicli_unsupported_generation_config(&mut body_value);
                let query = Some(format!("alt={}", alt.unwrap_or_else(|| "sse".to_string())));
                Ok(Self {
                    method: WreqMethod::POST,
                    path: "/v1internal:streamGenerateContent".to_string(),
                    query,
                    body: Some(body_value),
                    model: Some(normalize_model_id(model.as_str())),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: true,
                    },
                    extra_headers,
                })
            }
            (OperationFamily::Embedding, ProtocolKind::Gemini) => {
                let (model, body_value, _) = parse_gemini_payload_wrapper(&payload_value)?;
                let Some(model) = model else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model in geminicli embedding payload".to_string(),
                    ));
                };
                let Some(body_value) = body_value else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing body in geminicli embedding payload".to_string(),
                    ));
                };
                let model_name = normalize_model_name(model.as_str());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: format!("/v1beta/{model_name}:embedContent"),
                    query: None,
                    body: Some(body_value),
                    model: Some(normalize_model_id(model.as_str())),
                    kind: GeminiCliRequestKind::Forward {
                        requires_project: false,
                    },
                    extra_headers,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}
