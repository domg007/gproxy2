use super::*;

pub(super) fn build_request_body_bytes(
    body: Option<&Value>,
    model: Option<&str>,
    kind: &AntigravityRequestKind,
    project_id: &str,
    session_id: Option<&str>,
) -> Result<Option<Vec<u8>>, UpstreamError> {
    match kind {
        AntigravityRequestKind::Forward {
            requires_project: true,
            ..
        } => {
            let Some(model) = model else {
                return Err(UpstreamError::SerializeRequest(
                    "missing model for antigravity generate request".to_string(),
                ));
            };
            let project_id = project_id.trim();
            if project_id.is_empty() {
                return Err(UpstreamError::SerializeRequest(
                    "missing project_id in antigravity credential".to_string(),
                ));
            }
            let Some(request) = body else {
                return Err(UpstreamError::SerializeRequest(
                    "missing request body for antigravity generate request".to_string(),
                ));
            };
            let mut request = request.clone();
            if let Some(config_obj) = request
                .as_object_mut()
                .and_then(|root| root.get_mut("generationConfig"))
                .and_then(Value::as_object_mut)
            {
                config_obj.remove("maxOutputTokens");
                config_obj.remove("max_output_tokens");

                if model.to_ascii_lowercase().contains("gemini") {
                    config_obj.remove("logprobs");
                    config_obj.remove("responseLogprobs");
                    config_obj.remove("response_logprobs");
                }
            }
            if let Some(value) = session_id.map(str::trim).filter(|value| !value.is_empty())
                && let Some(request_obj) = request.as_object_mut()
            {
                request_obj.insert("sessionId".to_string(), Value::String(value.to_string()));
            }
            let wrapped = json!({
                "model": model,
                "project": project_id,
                "request": request,
            });
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

pub(super) fn request_type_for_kind(
    kind: &AntigravityRequestKind,
    model: Option<&str>,
) -> Option<&'static str> {
    match kind {
        AntigravityRequestKind::Forward { request_type, .. } => {
            request_type.or_else(|| model.map(request_type_for_model))
        }
        _ => None,
    }
}

pub(super) fn request_type_for_model(model: &str) -> &'static str {
    if model.to_ascii_lowercase().contains("image") {
        "image_gen"
    } else {
        "agent"
    }
}

pub(super) fn try_local_antigravity_count_response(
    request: &TransformRequest,
) -> Result<Option<TransformResponse>, UpstreamError> {
    let TransformRequest::CountTokenGemini(value) = request else {
        return Ok(None);
    };

    let payload = serde_json::to_value(&value.body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let text = collect_count_text(&payload);
    let total_tokens = (text.chars().count() as u64).div_ceil(4);

    let response_json = json!({
        "stats_code": 200,
        "headers": {},
        "body": {
            "totalTokens": total_tokens,
        }
    });
    let response = serde_json::from_value(response_json)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok(Some(TransformResponse::CountTokenGemini(response)))
}

pub(super) fn collect_count_text(payload: &Value) -> String {
    if let Some(contents) = payload.get("contents").and_then(Value::as_array) {
        return collect_contents_text(contents);
    }
    if let Some(contents) = payload
        .get("generateContentRequest")
        .and_then(|value| value.get("contents"))
        .and_then(Value::as_array)
    {
        return collect_contents_text(contents);
    }
    serde_json::to_string(payload).unwrap_or_default()
}

pub(super) fn collect_contents_text(contents: &[Value]) -> String {
    let mut out = String::new();
    for content in contents {
        let Some(parts) = content.get("parts").and_then(Value::as_array) else {
            continue;
        };
        for part in parts {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                out.push_str(text);
            }
        }
    }
    out
}

pub(super) fn build_model_list_local_response(
    status_code: u16,
    bytes: &[u8],
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> TransformResponse {
    if status_code == 200 {
        let payload = serde_json::from_slice::<Value>(bytes).ok();
        if let Some(payload) = payload {
            let models = extract_available_models(&payload);
            let total = models.len();
            let start = page_token
                .and_then(|token| token.parse::<usize>().ok())
                .unwrap_or(0);
            let start = start.min(total);
            let size = page_size
                .map(|value| value.max(1) as usize)
                .unwrap_or(total.saturating_sub(start));
            let end = start.saturating_add(size).min(total);
            let page_models = models[start..end].to_vec();
            let next_page_token = (end < total).then(|| end.to_string());

            let response_json = json!({
                "stats_code": 200,
                "headers": {},
                "body": {
                    "models": page_models,
                    "nextPageToken": next_page_token,
                }
            });
            if let Ok(response) = serde_json::from_value(response_json) {
                return TransformResponse::ModelListGemini(response);
            }
        }
        return model_list_error_response(502, "invalid antigravity model-list payload");
    }

    let message = extract_upstream_error_message(bytes)
        .unwrap_or_else(|| format!("upstream status {status_code}"));
    model_list_error_response(status_code, &message)
}

pub(super) fn build_model_get_local_response(
    status_code: u16,
    bytes: &[u8],
    target: &str,
) -> TransformResponse {
    if status_code == 200 {
        let payload = serde_json::from_slice::<Value>(bytes).ok();
        if let Some(payload) = payload
            && let Some(model) = find_available_model(&payload, target)
        {
            let response_json = json!({
                "stats_code": 200,
                "headers": {},
                "body": model,
            });
            if let Ok(response) = serde_json::from_value(response_json) {
                return TransformResponse::ModelGetGemini(response);
            }
        }
        let message = format!("model {target} not found");
        return model_get_error_response(404, &message);
    }

    let message = extract_upstream_error_message(bytes)
        .unwrap_or_else(|| format!("upstream status {status_code}"));
    model_get_error_response(status_code, &message)
}

pub(super) fn model_list_error_response(status_code: u16, message: &str) -> TransformResponse {
    let response_json = json!({
        "stats_code": status_code,
        "headers": {},
        "body": {
            "error": {
                "code": status_code,
                "message": message,
                "status": "UNKNOWN",
            }
        }
    });
    let response = serde_json::from_value(response_json).unwrap_or_else(|_| {
        serde_json::from_value(json!({
            "stats_code": 500,
            "headers": {},
            "body": {
                "error": {
                    "code": 500,
                    "message": "internal serialization error",
                    "status": "INTERNAL",
                }
            }
        }))
        .expect("fallback model-list response")
    });
    TransformResponse::ModelListGemini(response)
}

pub(super) fn model_get_error_response(status_code: u16, message: &str) -> TransformResponse {
    let response_json = json!({
        "stats_code": status_code,
        "headers": {},
        "body": {
            "error": {
                "code": status_code,
                "message": message,
                "status": "UNKNOWN",
            }
        }
    });
    let response = serde_json::from_value(response_json).unwrap_or_else(|_| {
        serde_json::from_value(json!({
            "stats_code": 500,
            "headers": {},
            "body": {
                "error": {
                    "code": 500,
                    "message": "internal serialization error",
                    "status": "INTERNAL",
                }
            }
        }))
        .expect("fallback model-get response")
    });
    TransformResponse::ModelGetGemini(response)
}

pub(super) fn extract_available_models(payload: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    if let Some(models_obj) = payload.get("models").and_then(Value::as_object) {
        for (model_id, model_meta) in models_obj {
            out.push(build_available_model(model_id.as_str(), model_meta));
        }
    } else if let Some(models_arr) = payload.get("models").and_then(Value::as_array) {
        for item in models_arr {
            if let Some(id) = item
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| item.get("name").and_then(Value::as_str))
            {
                out.push(build_available_model(&normalize_model_id(id), item));
            } else if let Some(value) = item.as_str() {
                out.push(build_available_model(
                    &normalize_model_id(value),
                    &Value::Null,
                ));
            }
        }
    }

    out.sort_by(|a, b| {
        let a_name = a.get("name").and_then(Value::as_str).unwrap_or_default();
        let b_name = b.get("name").and_then(Value::as_str).unwrap_or_default();
        a_name.cmp(b_name)
    });
    out.dedup_by(|a, b| {
        let a_name = a.get("name").and_then(Value::as_str).unwrap_or_default();
        let b_name = b.get("name").and_then(Value::as_str).unwrap_or_default();
        a_name == b_name
    });
    out
}

pub(super) fn find_available_model(payload: &Value, model_name: &str) -> Option<Value> {
    let model_id = normalize_model_id(model_name);
    if let Some(models_obj) = payload.get("models").and_then(Value::as_object) {
        if let Some(meta) = models_obj.get(model_id.as_str()) {
            return Some(build_available_model(model_id.as_str(), meta));
        }
        return models_obj
            .iter()
            .find(|(id, _)| normalize_model_id(id) == model_id)
            .map(|(id, meta)| build_available_model(id.as_str(), meta));
    }

    if let Some(models_arr) = payload.get("models").and_then(Value::as_array) {
        for item in models_arr {
            let raw_id = item
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| item.get("name").and_then(Value::as_str))
                .or_else(|| item.as_str());
            if let Some(raw_id) = raw_id
                && normalize_model_id(raw_id) == model_id
            {
                return Some(build_available_model(model_id.as_str(), item));
            }
        }
    }
    None
}

pub(super) fn build_available_model(model_id: &str, meta: &Value) -> Value {
    let display_name = meta
        .get("displayName")
        .and_then(Value::as_str)
        .or_else(|| meta.get("display_name").and_then(Value::as_str))
        .unwrap_or(model_id);

    let mut object = Map::new();
    object.insert(
        "name".to_string(),
        Value::String(format!("models/{model_id}")),
    );
    object.insert(
        "baseModelId".to_string(),
        Value::String(model_id.to_string()),
    );
    object.insert("version".to_string(), Value::String("1".to_string()));
    object.insert(
        "displayName".to_string(),
        Value::String(display_name.to_string()),
    );
    object.insert(
        "supportedGenerationMethods".to_string(),
        json!(["generateContent", "countTokens", "streamGenerateContent"]),
    );

    if let Some(limit) = meta.get("maxTokens").and_then(Value::as_u64) {
        object.insert("inputTokenLimit".to_string(), Value::Number(limit.into()));
    }
    if let Some(limit) = meta
        .get("maxOutputTokens")
        .and_then(Value::as_u64)
        .or_else(|| meta.get("outputTokenLimit").and_then(Value::as_u64))
    {
        object.insert("outputTokenLimit".to_string(), Value::Number(limit.into()));
    }

    Value::Object(object)
}

pub(super) fn extract_upstream_error_message(bytes: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(bytes).ok()?;
    if let Some(message) = value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        return Some(message.to_string());
    }
    if let Some(message) = value.get("error").and_then(Value::as_str) {
        return Some(message.to_string());
    }
    value
        .get("message")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

const FAKE_PREFIX: &str = "\u{5047}\u{6d41}\u{5f0f}/";
const ANTI_TRUNC_PREFIX: &str = "\u{6d41}\u{5f0f}\u{6297}\u{622a}\u{65ad}/";
const FAKE_SUFFIX: &str = "\u{5047}\u{6d41}\u{5f0f}";
const ANTI_TRUNC_SUFFIX: &str = "\u{6d41}\u{5f0f}\u{6297}\u{622a}\u{65ad}";

pub(super) fn normalize_model_name(model: &str) -> String {
    let model_id = normalize_model_id(model);
    format!("models/{model_id}")
}

pub(super) fn normalize_model_id(model: &str) -> String {
    let mut name = model
        .trim()
        .trim_start_matches('/')
        .trim_start_matches("models/");
    for prefix in [FAKE_PREFIX, ANTI_TRUNC_PREFIX] {
        if let Some(stripped) = name.strip_prefix(prefix) {
            name = stripped;
        }
    }
    if let Some(stripped) = name.strip_suffix(FAKE_SUFFIX) {
        name = stripped.trim_end_matches('-');
    }
    if let Some(stripped) = name.strip_suffix(ANTI_TRUNC_SUFFIX) {
        name = stripped.trim_end_matches('-');
    }
    name.to_string()
}

pub(super) fn make_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    format!("gproxy-{nanos}")
}

pub(super) fn session_id_for_kind(
    kind: &AntigravityRequestKind,
    body: Option<&Value>,
) -> Option<String> {
    match kind {
        AntigravityRequestKind::Forward {
            requires_project: true,
            ..
        } => explicit_antigravity_session_id(body).or_else(|| prompt_stable_session_id(body)),
        _ => None,
    }
}

pub(super) fn explicit_antigravity_session_id(body: Option<&Value>) -> Option<String> {
    body?
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn prompt_stable_session_id(body: Option<&Value>) -> Option<String> {
    let body = body?;
    let mut marker = Map::new();

    if let Some(system_instruction) = body
        .get("systemInstruction")
        .filter(|value| !value.is_null())
    {
        marker.insert("systemInstruction".to_string(), system_instruction.clone());
    }

    if let Some(first_user_content) = first_user_content(body.get("contents")) {
        marker.insert("content".to_string(), first_user_content);
    }

    let marker = (!marker.is_empty())
        .then(|| serde_json::to_string(&Value::Object(marker)).ok())
        .flatten()?;
    Some(stable_session_id_from_seed(
        format!("antigravity.prompt:{marker}").as_str(),
    ))
}

pub(super) fn first_user_content(contents: Option<&Value>) -> Option<Value> {
    let contents = contents?.as_array()?;
    contents
        .iter()
        .find(|content| matches!(content.get("role").and_then(Value::as_str), Some("user")))
        .cloned()
        .or_else(|| contents.first().cloned())
}

pub(super) fn stable_session_id_from_seed(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("gproxy-{hex}")
}

pub(super) fn antigravity_credential_update(
    credential_id: i64,
    refreshed: &AntigravityRefreshedToken,
) -> UpstreamCredentialUpdate {
    UpstreamCredentialUpdate::AntigravityTokenRefresh {
        credential_id,
        access_token: refreshed.access_token.clone(),
        refresh_token: refreshed.refresh_token.clone(),
        expires_at_unix_ms: refreshed.expires_at_unix_ms,
        user_email: refreshed.user_email.clone(),
    }
}

pub fn normalize_antigravity_upstream_response_body(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let response = value.get("response")?;
    serde_json::to_vec(response).ok()
}

pub fn normalize_antigravity_upstream_stream_ndjson_chunk(chunk: &[u8]) -> Option<Vec<u8>> {
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
