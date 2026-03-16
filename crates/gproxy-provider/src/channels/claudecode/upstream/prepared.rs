use super::*;

#[derive(Debug, Clone)]
pub(super) enum ClaudeCode1mTarget {
    Sonnet,
    Opus,
}

#[derive(Debug, Clone)]
pub(super) struct ClaudeCodePreparedRequest {
    pub(super) method: WreqMethod,
    pub(super) path: String,
    pub(super) body: Option<Vec<u8>>,
    pub(super) model: Option<String>,
    pub(super) request_headers: Vec<(String, String)>,
    pub(super) extra_headers: Vec<(String, String)>,
    pub(super) context_1m_target: Option<ClaudeCode1mTarget>,
}

impl ClaudeCodePreparedRequest {
    pub(super) fn from_transform_request(
        request: &gproxy_middleware::TransformRequest,
        append_beta_query: bool,
        prelude_text: Option<&str>,
        enable_billing_header: bool,
        cache_breakpoints: &[CacheBreakpointRule],
    ) -> Result<Self, UpstreamError> {
        let extra_headers = extra_headers_from_transform_request(request);
        let mut prepared = match request {
            gproxy_middleware::TransformRequest::ModelListClaude(value) => {
                let mut path = "/v1/models".to_string();
                let query = claude_model_list_query_string(
                    value.query.after_id.as_deref(),
                    value.query.before_id.as_deref(),
                    value.query.limit,
                );
                if !query.is_empty() {
                    path.push('?');
                    path.push_str(&query);
                }
                path = path_with_optional_beta_query(path.as_str(), append_beta_query);

                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;
                ensure_oauth_beta(&mut request_headers, false);

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path,
                    body: None,
                    model: None,
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target: None,
                })
            }
            gproxy_middleware::TransformRequest::ModelGetClaude(value) => {
                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;
                ensure_oauth_beta(&mut request_headers, false);
                let model_id = value.path.model_id.trim().to_string();

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: path_with_optional_beta_query(
                        format!("/v1/models/{model_id}").as_str(),
                        append_beta_query,
                    ),
                    body: None,
                    model: Some(model_id),
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target: None,
                })
            }
            gproxy_middleware::TransformRequest::CountTokenClaude(value) => {
                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;

                let model = claude_model_to_string(&value.body.model)?;
                let mut body_json = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                if let Some(prelude_text) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude_text);
                }
                canonicalize_claude_body(&mut body_json);
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                if enable_billing_header {
                    apply_claudecode_billing_header_system_block(&mut body_json);
                }
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: path_with_optional_beta_query(
                        "/v1/messages/count_tokens",
                        append_beta_query,
                    ),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target,
                })
            }
            gproxy_middleware::TransformRequest::GenerateContentClaude(value) => {
                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;

                let model = claude_model_to_string(&value.body.model)?;
                let mut body_json = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                if let Some(prelude_text) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude_text);
                }
                canonicalize_claude_body(&mut body_json);
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_sampling(&mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                apply_magic_string_cache_control_triggers(&mut body_json);
                if !cache_breakpoints.is_empty() {
                    ensure_cache_breakpoint_rules(&mut body_json, cache_breakpoints);
                }
                if enable_billing_header {
                    apply_claudecode_billing_header_system_block(&mut body_json);
                }
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: path_with_optional_beta_query("/v1/messages", append_beta_query),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target,
                })
            }
            gproxy_middleware::TransformRequest::StreamGenerateContentClaude(value) => {
                let mut request_headers = anthropic_header_pairs(
                    &value.headers.anthropic_version,
                    value.headers.anthropic_beta.as_ref(),
                )?;

                let model = claude_model_to_string(&value.body.model)?;
                let mut body_json = serde_json::to_value(&value.body)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                if let Some(prelude_text) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude_text);
                }
                canonicalize_claude_body(&mut body_json);
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_sampling(&mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                apply_magic_string_cache_control_triggers(&mut body_json);
                if !cache_breakpoints.is_empty() {
                    ensure_cache_breakpoint_rules(&mut body_json, cache_breakpoints);
                }
                if enable_billing_header {
                    apply_claudecode_billing_header_system_block(&mut body_json);
                }
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());

                Ok(Self {
                    method: to_wreq_method(&value.method)?,
                    path: path_with_optional_beta_query("/v1/messages", append_beta_query),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers: Vec::new(),
                    context_1m_target,
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
        append_beta_query: bool,
        prelude_text: Option<&str>,
        enable_billing_header: bool,
        cache_breakpoints: &[CacheBreakpointRule],
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
            (OperationFamily::ModelList, ProtocolKind::Claude) => {
                let version = payload_header_string(
                    &payload_value,
                    &["anthropic-version", "anthropic_version"],
                )
                .unwrap_or_else(|| "2023-06-01".to_string());
                let beta = payload_header_string_array(
                    &payload_value,
                    &["anthropic-beta", "anthropic_beta"],
                );
                let mut request_headers = anthropic_header_pairs(&version, beta.as_ref())?;
                ensure_oauth_beta(&mut request_headers, false);
                let path = path_with_optional_beta_query("/v1/models", append_beta_query);
                Ok(Self {
                    method: WreqMethod::GET,
                    path,
                    body: None,
                    model: None,
                    request_headers,
                    extra_headers,
                    context_1m_target: None,
                })
            }
            (OperationFamily::ModelGet, ProtocolKind::Claude) => {
                let Some(model_id) = payload_value
                    .pointer("/path/model_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                else {
                    return Err(UpstreamError::SerializeRequest(
                        "missing path.model_id in claudecode model_get payload".to_string(),
                    ));
                };
                let version = payload_header_string(
                    &payload_value,
                    &["anthropic-version", "anthropic_version"],
                )
                .unwrap_or_else(|| "2023-06-01".to_string());
                let beta = payload_header_string_array(
                    &payload_value,
                    &["anthropic-beta", "anthropic_beta"],
                );
                let mut request_headers = anthropic_header_pairs(&version, beta.as_ref())?;
                ensure_oauth_beta(&mut request_headers, false);
                Ok(Self {
                    method: WreqMethod::GET,
                    path: path_with_optional_beta_query(
                        format!("/v1/models/{model_id}").as_str(),
                        append_beta_query,
                    ),
                    body: None,
                    model: Some(model_id),
                    request_headers,
                    extra_headers,
                    context_1m_target: None,
                })
            }
            (OperationFamily::CountToken, ProtocolKind::Claude) => {
                let (mut body_json, version, beta) = parse_claude_payload_wrapper(&payload_value)?;
                if let Some(prelude) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude);
                }
                canonicalize_claude_body(&mut body_json);
                let model = json_pointer_string(&body_json, "/model").ok_or_else(|| {
                    UpstreamError::SerializeRequest(
                        "missing model in claudecode count_tokens payload".to_string(),
                    )
                })?;
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                if enable_billing_header {
                    apply_claudecode_billing_header_system_block(&mut body_json);
                }
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                let mut request_headers = anthropic_header_pairs(&version, beta.as_ref())?;
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: path_with_optional_beta_query(
                        "/v1/messages/count_tokens",
                        append_beta_query,
                    ),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers,
                    context_1m_target,
                })
            }
            (OperationFamily::GenerateContent, ProtocolKind::Claude)
            | (OperationFamily::StreamGenerateContent, ProtocolKind::Claude) => {
                let (mut body_json, version, beta) = parse_claude_payload_wrapper(&payload_value)?;
                if let Some(prelude) = prelude_text {
                    apply_claudecode_system(&mut body_json, prelude);
                }
                canonicalize_claude_body(&mut body_json);
                let model = json_pointer_string(&body_json, "/model").ok_or_else(|| {
                    UpstreamError::SerializeRequest(
                        "missing model in claudecode message payload".to_string(),
                    )
                })?;
                let model = normalize_claudecode_model_and_thinking(model.as_str(), &mut body_json);
                normalize_claudecode_sampling(&mut body_json);
                normalize_claudecode_unsupported_fields(&mut body_json);
                apply_magic_string_cache_control_triggers(&mut body_json);
                if !cache_breakpoints.is_empty() {
                    ensure_cache_breakpoint_rules(&mut body_json, cache_breakpoints);
                }
                if enable_billing_header {
                    apply_claudecode_billing_header_system_block(&mut body_json);
                }
                let context_1m_target = claude_1m_target_for_model(model.as_str());
                let mut request_headers = anthropic_header_pairs(&version, beta.as_ref())?;
                ensure_oauth_beta(&mut request_headers, context_1m_target.is_some());
                Ok(Self {
                    method: WreqMethod::POST,
                    path: path_with_optional_beta_query("/v1/messages", append_beta_query),
                    body: Some(
                        serde_json::to_vec(&body_json)
                            .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?,
                    ),
                    model: Some(model),
                    request_headers,
                    extra_headers,
                    context_1m_target,
                })
            }
            _ => Err(UpstreamError::UnsupportedRequest),
        }
    }
}
