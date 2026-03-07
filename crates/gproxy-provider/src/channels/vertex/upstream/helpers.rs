use super::*;

pub(super) async fn normalize_vertex_model_response(
    response: WreqResponse,
    kind: VertexModelResponseKind,
) -> Result<TransformResponse, UpstreamError> {
    let status = response.status();
    let mut header_map = serde_json::Map::new();
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            header_map.insert(name.to_string(), Value::String(value.to_string()));
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|err| UpstreamError::UpstreamRequest(err.to_string()))?;
    let raw_body = serde_json::from_slice::<Value>(&bytes)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let body = match kind {
        VertexModelResponseKind::List => vertex_model_list_payload(raw_body),
        VertexModelResponseKind::Get => vertex_model_get_payload(raw_body),
        VertexModelResponseKind::Embedding => vertex_embedding_payload(raw_body)?,
    };

    let payload = json!({
        "stats_code": status.as_u16(),
        "headers": header_map,
        "body": body,
    });

    match kind {
        VertexModelResponseKind::List => {
            let response = serde_json::from_value(payload)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(TransformResponse::ModelListGemini(response))
        }
        VertexModelResponseKind::Get => {
            let response = serde_json::from_value(payload)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(TransformResponse::ModelGetGemini(response))
        }
        VertexModelResponseKind::Embedding => {
            let response = serde_json::from_value(payload)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(TransformResponse::EmbeddingGemini(response))
        }
    }
}

pub(super) fn vertex_model_list_payload(value: Value) -> Value {
    let Value::Object(mut map) = value else {
        return value;
    };
    if map.contains_key("models") {
        return Value::Object(map);
    }

    let models = match map.remove("publisherModels") {
        Some(Value::Array(items)) => items
            .into_iter()
            .map(vertex_publisher_model_to_gemini)
            .collect::<Vec<_>>(),
        Some(item) => vec![vertex_publisher_model_to_gemini(item)],
        None => Vec::new(),
    };

    let mut out = serde_json::Map::new();
    out.insert("models".to_string(), Value::Array(models));
    if let Some(token) = map.remove("nextPageToken").filter(|v| !v.is_null()) {
        out.insert("nextPageToken".to_string(), token);
    }
    Value::Object(out)
}

pub(super) fn vertex_model_get_payload(value: Value) -> Value {
    let Value::Object(mut map) = value else {
        return value;
    };
    if map
        .get("name")
        .and_then(|v| v.as_str())
        .map(|name| name.starts_with("models/"))
        .unwrap_or(false)
    {
        return Value::Object(map);
    }
    if let Some(inner) = map.remove("publisherModel") {
        return vertex_publisher_model_to_gemini(inner);
    }
    vertex_publisher_model_to_gemini(Value::Object(map))
}

pub(super) fn vertex_publisher_model_to_gemini(value: Value) -> Value {
    let Value::Object(map) = value else {
        return value;
    };

    let raw_name = map
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    let model_id = if let Some((_, tail)) = raw_name.rsplit_once("/models/") {
        tail
    } else {
        raw_name.strip_prefix("models/").unwrap_or(raw_name)
    };
    let model_id = if model_id.is_empty() {
        "unknown"
    } else {
        model_id
    };

    let mut out = serde_json::Map::new();
    out.insert(
        "name".to_string(),
        Value::String(format!("models/{model_id}")),
    );

    if let Some(base_model_id) = map
        .get("baseModelId")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        out.insert(
            "baseModelId".to_string(),
            Value::String(base_model_id.to_string()),
        );
    }
    if let Some(version) = map
        .get("version")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
    {
        out.insert("version".to_string(), Value::String(version.to_string()));
    }
    if let Some(value) = map.get("displayName").cloned().filter(|v| !v.is_null()) {
        out.insert("displayName".to_string(), value);
    }
    if let Some(value) = map.get("description").cloned().filter(|v| !v.is_null()) {
        out.insert("description".to_string(), value);
    }
    if let Some(value) = map.get("inputTokenLimit").cloned().filter(|v| !v.is_null()) {
        out.insert("inputTokenLimit".to_string(), value);
    }
    if let Some(value) = map
        .get("outputTokenLimit")
        .cloned()
        .filter(|v| !v.is_null())
    {
        out.insert("outputTokenLimit".to_string(), value);
    }
    if let Some(value) = map
        .get("supportedGenerationMethods")
        .cloned()
        .filter(|v| !v.is_null())
    {
        out.insert("supportedGenerationMethods".to_string(), value);
    }
    if let Some(value) = map.get("thinking").cloned().filter(|v| !v.is_null()) {
        out.insert("thinking".to_string(), value);
    }
    if let Some(value) = map.get("temperature").cloned().filter(|v| !v.is_null()) {
        out.insert("temperature".to_string(), value);
    }
    if let Some(value) = map.get("maxTemperature").cloned().filter(|v| !v.is_null()) {
        out.insert("maxTemperature".to_string(), value);
    }
    if let Some(value) = map.get("topP").cloned().filter(|v| !v.is_null()) {
        out.insert("topP".to_string(), value);
    }
    if let Some(value) = map.get("topK").cloned().filter(|v| !v.is_null()) {
        out.insert("topK".to_string(), value);
    }

    Value::Object(out)
}

pub(super) fn build_vertex_path(
    endpoint: VertexEndpoint,
    project_id: &str,
    location: &str,
) -> String {
    match endpoint {
        VertexEndpoint::Global(path) => format!("/v1beta1/{path}"),
        VertexEndpoint::Project(path) => {
            format!("/v1beta1/projects/{project_id}/locations/{location}/{path}")
        }
    }
}

pub(super) fn normalize_vertex_model_name(name: &str) -> String {
    let name = name.trim();
    let name = name.strip_prefix("models/").unwrap_or(name);
    let name = name
        .strip_prefix("publishers/google/models/")
        .unwrap_or(name);
    if let Some((_, tail)) = name.rsplit_once("/models/") {
        return tail.to_string();
    }
    name.to_string()
}

pub(super) fn normalize_vertex_openai_model(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return trimmed.to_string();
    }
    if let Some(stripped) = trimmed.strip_prefix("publishers/")
        && let Some((publisher, model_name)) = stripped.split_once("/models/")
    {
        return format!("{publisher}/{model_name}");
    }
    if let Some(idx) = trimmed.find("/publishers/") {
        let tail = &trimmed[(idx + "/publishers/".len())..];
        if let Some((publisher, model_name)) = tail.split_once("/models/") {
            return format!("{publisher}/{model_name}");
        }
    }
    if let Some(stripped) = trimmed.strip_prefix("models/") {
        return format!("google/{stripped}");
    }
    if trimmed.contains('/') {
        return trimmed.to_string();
    }
    format!("google/{trimmed}")
}

pub fn normalize_vertex_upstream_response_body(body: &[u8]) -> Option<Vec<u8>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let wrapper = value.as_object()?;
    if !wrapper.contains_key("stats_code") || !wrapper.contains_key("body") {
        return None;
    }
    serde_json::to_vec(wrapper.get("body")?).ok()
}

pub(super) fn normalize_vertex_model_ref(model: &str, fallback_model: &str) -> String {
    let model = model.trim().trim_start_matches('/');
    if model.is_empty() {
        return format!("publishers/google/models/{fallback_model}");
    }
    if model.starts_with("publishers/") && model.contains("/models/") {
        return model.to_string();
    }
    if let Some(id) = model.strip_prefix("models/") {
        return format!("publishers/google/models/{id}");
    }
    if let Some((publisher, id)) = model.split_once('/')
        && !publisher.is_empty()
        && !id.is_empty()
    {
        return format!("publishers/{publisher}/models/{id}");
    }
    format!("publishers/google/models/{model}")
}

pub(super) fn vertex_generate_payload(
    path_model: &str,
    body: &impl serde::Serialize,
) -> Result<Value, UpstreamError> {
    let mut value = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    if let Value::Object(map) = &mut value
        && let Some(model) = map.get("model").and_then(Value::as_str)
    {
        map.insert(
            "model".to_string(),
            Value::String(normalize_vertex_model_ref(model, path_model)),
        );
    }
    Ok(value)
}

pub(super) fn vertex_count_tokens_payload(
    path_model: &str,
    body: &impl serde::Serialize,
) -> Result<Value, UpstreamError> {
    let mut out = Map::new();
    out.insert(
        "model".to_string(),
        Value::String(format!("publishers/google/models/{path_model}")),
    );

    let source = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let source_map = source.as_object().cloned().unwrap_or_default();

    if let Some(contents) = source_map.get("contents")
        && !contents.is_null()
    {
        out.insert("contents".to_string(), contents.clone());
    }

    if let Some(generate) = source_map
        .get("generateContentRequest")
        .and_then(Value::as_object)
    {
        if !out.contains_key("contents")
            && let Some(value) = generate.get("contents")
        {
            out.insert("contents".to_string(), value.clone());
        }
        if let Some(value) = generate.get("instances") {
            out.insert("instances".to_string(), value.clone());
        }
        if let Some(value) = generate.get("tools") {
            out.insert("tools".to_string(), value.clone());
        }
        if let Some(value) = generate
            .get("systemInstruction")
            .or_else(|| generate.get("system_instruction"))
        {
            out.insert("systemInstruction".to_string(), value.clone());
        }
        if let Some(value) = generate
            .get("generationConfig")
            .or_else(|| generate.get("generation_config"))
        {
            out.insert("generationConfig".to_string(), value.clone());
        }
        if let Some(model) = generate.get("model").and_then(Value::as_str) {
            out.insert(
                "model".to_string(),
                Value::String(normalize_vertex_model_ref(model, path_model)),
            );
        }
    }

    Ok(Value::Object(out))
}

pub(super) fn vertex_embedding_predict_payload(
    body: &impl serde::Serialize,
) -> Result<Value, UpstreamError> {
    let source = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    let source_map = source.as_object().cloned().unwrap_or_default();

    let content = source_map
        .get("content")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let instance_text = content_text_for_predict(&content);
    let mut out = Map::new();
    out.insert(
        "instances".to_string(),
        Value::Array(vec![json!({
            "content": instance_text,
        })]),
    );

    let mut parameters = Map::new();
    if let Some(value) = source_map.get("taskType").cloned().filter(|v| !v.is_null()) {
        parameters.insert("taskType".to_string(), value);
    }
    if let Some(value) = source_map
        .get("outputDimensionality")
        .cloned()
        .filter(|v| !v.is_null())
    {
        parameters.insert("outputDimensionality".to_string(), value);
    }
    if let Some(value) = source_map.get("title").cloned().filter(|v| !v.is_null()) {
        parameters.insert("title".to_string(), value);
    }
    parameters.insert("autoTruncate".to_string(), Value::Bool(true));
    if !parameters.is_empty() {
        out.insert("parameters".to_string(), Value::Object(parameters));
    }

    Ok(Value::Object(out))
}

pub(super) fn content_text_for_predict(content: &Value) -> String {
    let Some(parts) = content
        .as_object()
        .and_then(|value| value.get("parts"))
        .and_then(Value::as_array)
    else {
        return content.to_string();
    };

    let mut texts = Vec::new();
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                texts.push(trimmed.to_string());
            }
        }
    }

    if texts.is_empty() {
        content.to_string()
    } else {
        texts.join("\n")
    }
}

pub(super) fn vertex_embedding_payload(value: Value) -> Result<Value, UpstreamError> {
    if value
        .as_object()
        .and_then(|value| value.get("embedding"))
        .is_some()
    {
        return Ok(value);
    }

    let first = value
        .as_object()
        .and_then(|value| value.get("predictions"))
        .and_then(Value::as_array)
        .and_then(|value| value.first())
        .ok_or_else(|| {
            UpstreamError::SerializeRequest(
                "vertex predict embedding response missing predictions[0]".to_string(),
            )
        })?;

    let values = first
        .as_object()
        .and_then(|value| value.get("embeddings").or_else(|| value.get("embedding")))
        .and_then(Value::as_object)
        .and_then(|value| value.get("values"))
        .or_else(|| first.as_object().and_then(|value| value.get("values")))
        .cloned()
        .ok_or_else(|| {
            UpstreamError::SerializeRequest(
                "vertex predict embedding response missing embedding values".to_string(),
            )
        })?;

    Ok(json!({
        "embedding": {
            "values": values,
        }
    }))
}
