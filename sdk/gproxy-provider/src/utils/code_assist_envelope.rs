use crate::response::UpstreamError;

/// Wrap a Gemini request body in the Code Assist API envelope.
///
/// Input: standard Gemini JSON body (with optional `model` field at top level).
/// Output: `{"model": "...", "project": "...", "request": { <gemini body> }}`
pub fn wrap_request(
    body: &[u8],
    model: Option<&str>,
    project_id: &str,
) -> Result<Vec<u8>, UpstreamError> {
    let mut inner: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| UpstreamError::RequestBuild(format!("json parse for envelope wrap: {e}")))?;

    // Extract model from body if present, otherwise use the provided model.
    let model_name = inner
        .as_object_mut()
        .and_then(|obj| obj.remove("model"))
        .and_then(|v| v.as_str().map(String::from))
        .or_else(|| model.map(String::from))
        .unwrap_or_default();

    let envelope = serde_json::json!({
        "model": model_name,
        "project": project_id,
        "request": inner,
    });

    serde_json::to_vec(&envelope)
        .map_err(|e| UpstreamError::RequestBuild(format!("envelope serialize: {e}")))
}

/// Unwrap a Code Assist API response envelope.
///
/// Input: `{"response": { <gemini response> }, "traceId": "..."}`
/// Output: the inner `<gemini response>` object as bytes.
///
/// If parsing fails or no `"response"` key is found, the body is returned as-is.
pub fn unwrap_response(body: &[u8]) -> Vec<u8> {
    let Ok(mut json) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.to_vec();
    };

    if let Some(inner) = json.as_object_mut().and_then(|obj| obj.remove("response")) {
        serde_json::to_vec(&inner).unwrap_or_else(|_| body.to_vec())
    } else {
        body.to_vec()
    }
}
