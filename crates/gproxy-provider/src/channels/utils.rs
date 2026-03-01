use gproxy_middleware::{TransformRequest, TransformResponse};
use serde::Serialize;
use serde_json::{Value, json};
use url::form_urlencoded;
use wreq::Client as WreqClient;
use wreq::Method as WreqMethod;
use wreq::header::HeaderMap;

use crate::channels::upstream::UpstreamError;
use crate::tokenizers::LocalTokenizerStore;

pub fn to_wreq_method(method: &impl Serialize) -> Result<WreqMethod, UpstreamError> {
    let raw = serde_json::to_string(method)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    match raw.trim_matches('"') {
        "GET" => Ok(WreqMethod::GET),
        "POST" => Ok(WreqMethod::POST),
        "PUT" => Ok(WreqMethod::PUT),
        "PATCH" => Ok(WreqMethod::PATCH),
        "DELETE" => Ok(WreqMethod::DELETE),
        _ => Err(UpstreamError::UnsupportedRequest),
    }
}

pub fn join_base_url_and_path(base_url: &str, path: &str) -> String {
    let normalized_path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let base = base_url.trim_end_matches('/');
    if let Some(base_without_v1) = base.strip_suffix("/v1")
        && normalized_path.starts_with("/v1/")
    {
        return format!("{base_without_v1}{normalized_path}");
    }
    format!("{base}{normalized_path}")
}

pub fn default_gproxy_user_agent() -> String {
    format!(
        "gproxy/{}({},{})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

pub fn resolve_user_agent_or_default(configured: Option<&str>, fallback: &str) -> String {
    configured
        .map(str::trim)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback.to_string())
}

pub fn resolve_user_agent_or_else<F>(configured: Option<&str>, fallback: F) -> String
where
    F: FnOnce() -> String,
{
    configured
        .map(str::trim)
        .map(ToOwned::to_owned)
        .unwrap_or_else(fallback)
}

pub const fn is_auth_failure(status_code: u16) -> bool {
    status_code == 401 || status_code == 403
}

pub const fn is_transient_server_failure(status_code: u16) -> bool {
    matches!(status_code, 500 | 502 | 503 | 504)
}

pub fn retry_after_to_millis(headers: &HeaderMap) -> Option<u64> {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| seconds.saturating_mul(1000))
}

pub fn serialize_json_scalar<T: Serialize>(value: &T) -> Result<String, UpstreamError> {
    let raw = serde_json::to_string(value)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok(raw.trim_matches('"').to_string())
}

pub fn claude_model_to_string(model: &impl Serialize) -> Result<String, UpstreamError> {
    serialize_json_scalar(model)
}

pub fn anthropic_header_pairs<V, B>(
    version: &V,
    beta: Option<&Vec<B>>,
) -> Result<Vec<(String, String)>, UpstreamError>
where
    V: Serialize,
    B: Serialize,
{
    let mut headers = Vec::new();
    headers.push((
        "anthropic-version".to_string(),
        serialize_json_scalar(version)?,
    ));

    if let Some(items) = beta {
        let mut values = Vec::new();
        for item in items {
            values.push(serialize_json_scalar(item)?);
        }
        if !values.is_empty() {
            headers.push(("anthropic-beta".to_string(), values.join(",")));
        }
    }

    Ok(headers)
}

pub fn claude_model_list_query_string(
    after_id: Option<&str>,
    before_id: Option<&str>,
    limit: Option<u16>,
) -> String {
    let mut parts = Vec::new();
    if let Some(after_id) = after_id {
        parts.push(format!("after_id={after_id}"));
    }
    if let Some(before_id) = before_id {
        parts.push(format!("before_id={before_id}"));
    }
    if let Some(limit) = limit {
        parts.push(format!("limit={limit}"));
    }
    parts.join("&")
}

pub fn gemini_model_list_query_string(
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(page_size) = page_size {
        parts.push(format!("pageSize={page_size}"));
    }
    if let Some(page_token) = page_token.map(str::trim).filter(|token| !token.is_empty()) {
        parts.push(format!("pageToken={page_token}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("&"))
    }
}

pub async fn count_openai_input_tokens_with_resolution(
    tokenizer_store: &LocalTokenizerStore,
    http_client: &WreqClient,
    hf_token: Option<&str>,
    hf_url: Option<&str>,
    model: Option<&str>,
    body: &impl Serialize,
) -> Result<u64, UpstreamError> {
    let mut value = serde_json::to_value(body)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    if let Some(map) = value.as_object_mut() {
        map.remove("model");
    }
    let text = serde_json::to_string(&value)
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;

    let normalized_model = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("deepseek_fallback");

    let counted = tokenizer_store
        .count_text_tokens(
            http_client,
            hf_token,
            hf_url,
            normalized_model,
            text.as_str(),
        )
        .await
        .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
    Ok(counted.count as u64)
}

pub fn parse_query_value(query: Option<&str>, key: &str) -> Option<String> {
    let raw = query?.trim().trim_start_matches('?');
    for (name, value) in form_urlencoded::parse(raw.as_bytes()) {
        if name == key {
            return Some(value.into_owned());
        }
    }
    None
}

pub fn try_local_gemini_model_response(
    request: &TransformRequest,
    models_doc: &Value,
) -> Result<Option<TransformResponse>, UpstreamError> {
    let models = models_doc
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            UpstreamError::SerializeRequest("local models table missing models array".to_string())
        })?;

    match request {
        TransformRequest::ModelListGemini(value) => {
            let start = value
                .query
                .page_token
                .as_deref()
                .and_then(|token| token.parse::<usize>().ok())
                .unwrap_or(0)
                .min(models.len());
            let size = value
                .query
                .page_size
                .map(|value| value.max(1) as usize)
                .unwrap_or(models.len().saturating_sub(start));
            let end = start.saturating_add(size).min(models.len());

            let page_models = models[start..end].to_vec();
            let next_page_token = (end < models.len()).then(|| end.to_string());
            let response_json = json!({
                "stats_code": 200,
                "headers": {},
                "body": {
                    "models": page_models,
                    "nextPageToken": next_page_token,
                }
            });
            let response = serde_json::from_value(response_json)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(Some(TransformResponse::ModelListGemini(response)))
        }
        TransformRequest::ModelGetGemini(value) => {
            let requested = value.path.name.trim();
            let fallback_prefixed =
                (!requested.starts_with("models/")).then(|| format!("models/{requested}"));

            let found = models
                .iter()
                .find(|item| {
                    let Some(name) = item.get("name").and_then(Value::as_str) else {
                        return false;
                    };
                    if name == requested {
                        return true;
                    }
                    fallback_prefixed
                        .as_deref()
                        .map(|candidate| name == candidate)
                        .unwrap_or(false)
                })
                .cloned();

            if let Some(entry) = found {
                let response_json = json!({
                    "stats_code": 200,
                    "headers": {},
                    "body": entry,
                });
                let response = serde_json::from_value(response_json)
                    .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
                return Ok(Some(TransformResponse::ModelGetGemini(response)));
            }

            let response_json = json!({
                "stats_code": 404,
                "headers": {},
                "body": {
                    "error": {
                        "code": 404,
                        "message": format!("model {requested} not found"),
                        "status": "NOT_FOUND",
                    }
                }
            });
            let response = serde_json::from_value(response_json)
                .map_err(|err| UpstreamError::SerializeRequest(err.to_string()))?;
            Ok(Some(TransformResponse::ModelGetGemini(response)))
        }
        _ => Ok(None),
    }
}
