use super::*;

pub(super) fn build_request_body_bytes(
    body: Option<&Value>,
    model: Option<&str>,
    kind: &GeminiCliRequestKind,
    project_id: &str,
) -> Result<Option<Vec<u8>>, UpstreamError> {
    match kind {
        GeminiCliRequestKind::Forward { requires_project } if *requires_project => {
            let Some(model) = model else {
                return Err(UpstreamError::SerializeRequest(
                    "missing model for geminicli generate request".to_string(),
                ));
            };
            let project_id = project_id.trim();
            if project_id.is_empty() {
                return Err(UpstreamError::SerializeRequest(
                    "missing project_id in geminicli credential".to_string(),
                ));
            }
            let Some(request) = body else {
                return Err(UpstreamError::SerializeRequest(
                    "missing request body for geminicli generate request".to_string(),
                ));
            };
            let wrapped = wrap_internal_request(model, project_id, request);
            Ok(Some(serde_json::to_vec(&wrapped).map_err(|err| {
                UpstreamError::SerializeRequest(err.to_string())
            })?))
        }
        _ => {
            let Some(body) = body else {
                return Ok(None);
            };
            Ok(Some(serde_json::to_vec(body).map_err(|err| {
                UpstreamError::SerializeRequest(err.to_string())
            })?))
        }
    }
}

pub(super) fn wrap_internal_request(model: &str, project_id: &str, request: &Value) -> Value {
    json!({
        "model": model,
        "project": project_id,
        "user_prompt_id": generate_user_prompt_id(),
        "request": request,
    })
}

pub(super) fn strip_geminicli_unsupported_generation_config(body: &mut Value) {
    let Some(generation_config) = body
        .get_mut("generationConfig")
        .and_then(Value::as_object_mut)
    else {
        return;
    };

    generation_config.remove("logprobs");
    generation_config.remove("responseLogprobs");
    generation_config.remove("maxOutputTokens");
}

pub(super) fn generate_user_prompt_id() -> String {
    let bytes = rand::random::<[u8; 16]>();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn geminicli_count_tokens_request(
    model: &str,
    body: &impl serde::Serialize,
) -> Result<Value, UpstreamError> {
    let body = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let mut request = Map::new();
    request.insert(
        "model".to_string(),
        Value::String(format!("models/{model}")),
    );

    if let Some(contents) = body.get("contents") {
        request.insert("contents".to_string(), contents.clone());
    } else if let Some(generate) = body.get("generateContentRequest") {
        if let Some(contents) = generate.get("contents") {
            request.insert("contents".to_string(), contents.clone());
        }
        if let Some(value) = generate.get("tools") {
            request.insert("tools".to_string(), value.clone());
        }
        if let Some(value) = generate.get("toolConfig") {
            request.insert("toolConfig".to_string(), value.clone());
        }
        if let Some(value) = generate.get("safetySettings") {
            request.insert("safetySettings".to_string(), value.clone());
        }
        if let Some(value) = generate.get("systemInstruction") {
            request.insert("systemInstruction".to_string(), value.clone());
        }
        if let Some(value) = generate.get("generationConfig") {
            request.insert("generationConfig".to_string(), value.clone());
        }
        if let Some(value) = generate.get("cachedContent") {
            request.insert("cachedContent".to_string(), value.clone());
        }
    }

    Ok(json!({ "request": request }))
}

pub(super) fn usage_models_from_quota_payload(
    payload: &Value,
) -> Result<Vec<Value>, UpstreamError> {
    let buckets = payload
        .get("buckets")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            UpstreamError::SerializeRequest(
                "geminicli retrieveUserQuota payload missing buckets array".to_string(),
            )
        })?;
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for bucket in buckets {
        if let Some(token_type) = bucket.get("tokenType").and_then(Value::as_str)
            && token_type != "REQUESTS"
        {
            continue;
        }
        let Some(model_id_raw) = bucket.get("modelId").and_then(Value::as_str) else {
            continue;
        };
        let model_id = model_id_raw.trim().to_string();
        if model_id.is_empty() || !seen.insert(model_id.clone()) {
            continue;
        }
        let model_name = if model_id.starts_with("models/") {
            model_id.clone()
        } else {
            format!("models/{model_id}")
        };
        models.push(json!({
            "name": model_name,
            "baseModelId": model_id,
            "displayName": model_id,
            "description": "Derived from Gemini CLI retrieveUserQuota buckets.",
            "supportedGenerationMethods": [
                "generateContent",
                "streamGenerateContent",
                "countTokens"
            ]
        }));
    }
    Ok(models)
}

pub(super) fn geminicli_response_indicates_quota_exhausted(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let Some(error) = value.get("error").and_then(Value::as_object) else {
        return false;
    };
    let Some(details) = error.get("details").and_then(Value::as_array) else {
        return false;
    };

    details.iter().any(|detail| {
        let Some(detail_obj) = detail.as_object() else {
            return false;
        };
        if detail_obj.get("@type").and_then(Value::as_str)
            != Some("type.googleapis.com/google.rpc.ErrorInfo")
        {
            return false;
        }

        if detail_obj.get("reason").and_then(Value::as_str) == Some("QUOTA_EXHAUSTED") {
            return true;
        }

        detail_obj
            .get("metadata")
            .and_then(Value::as_object)
            .map(|metadata| {
                metadata.contains_key("quotaResetTimeStamp")
                    || metadata.contains_key("quotaResetDelay")
            })
            .unwrap_or(false)
    })
}

pub(super) fn normalize_model_name(model: &str) -> String {
    let model = model.trim().trim_start_matches('/');
    if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    }
}

pub(super) fn normalize_model_id(model: &str) -> String {
    normalize_model_name(model)
        .trim_start_matches("models/")
        .to_string()
}

pub fn normalize_geminicli_upstream_response_body(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let response = value.get("response")?;
    serde_json::to_vec(response).ok()
}

pub fn normalize_geminicli_upstream_stream_ndjson_chunk(chunk: &[u8]) -> Option<Vec<u8>> {
    normalize_wrapped_response_ndjson_chunk(chunk)
}

pub(super) fn normalize_wrapped_response_ndjson_chunk(chunk: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(chunk).ok()?;
    let mut out = String::with_capacity(text.len());
    let mut changed = false;

    for segment in text.split_inclusive('\n') {
        let has_newline = segment.ends_with('\n');
        let line = segment.trim_end_matches('\n').trim_end_matches('\r');
        if line.is_empty() {
            out.push_str(segment);
            continue;
        }

        let value = match serde_json::from_str::<Value>(line) {
            Ok(value) => value,
            Err(_) => {
                out.push_str(segment);
                continue;
            }
        };

        if let Some(response) = value.get("response") {
            let normalized = match serde_json::to_string(response) {
                Ok(value) => value,
                Err(_) => {
                    out.push_str(segment);
                    continue;
                }
            };
            out.push_str(normalized.as_str());
            if has_newline {
                out.push('\n');
            }
            changed = true;
        } else {
            out.push_str(segment);
        }
    }

    changed.then(|| out.into_bytes())
}
