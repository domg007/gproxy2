use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequestPayload};
use gproxy_protocol::claude::types::{AnthropicBeta, AnthropicVersion};
use serde_json::{Map, Value, json};

use crate::AppState;

use super::super::{
    HttpError, anthropic_headers_from_request, authorize_provider_access, bad_request,
    collect_passthrough_headers, execute_transform_request_payload, parse_json_body,
    resolve_provider, split_provider_prefixed_plain_model,
};

fn required_string_field<'a>(
    value: &'a Value,
    pointer: &str,
    missing_message: &str,
    invalid_message: &str,
) -> Result<&'a str, HttpError> {
    let Some(raw) = value.pointer(pointer) else {
        return Err(bad_request(missing_message));
    };
    raw.as_str().ok_or_else(|| bad_request(invalid_message))
}

fn set_string_field(
    value: &mut Value,
    pointer: &str,
    new_value: String,
    missing_message: &str,
) -> Result<(), HttpError> {
    let Some(slot) = value.pointer_mut(pointer) else {
        return Err(bad_request(missing_message));
    };
    *slot = Value::String(new_value);
    Ok(())
}

fn stream_enabled(value: &Value) -> bool {
    value
        .pointer("/stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn encode_json_value(value: &Value, context: &str) -> Result<Bytes, HttpError> {
    serde_json::to_vec(value)
        .map(Bytes::from)
        .map_err(|err| bad_request(format!("{context}: {err}")))
}

fn build_claude_payload(
    body: Value,
    anthropic_version: AnthropicVersion,
    anthropic_beta: Option<Vec<AnthropicBeta>>,
    passthrough_headers: &HeaderMap,
    context: &str,
) -> Result<Bytes, HttpError> {
    let mut header_map = Map::new();
    header_map.insert("anthropic-version".to_string(), json!(anthropic_version));
    if let Some(anthropic_beta) = anthropic_beta {
        header_map.insert("anthropic-beta".to_string(), json!(anthropic_beta));
    }
    for (name, value) in collect_passthrough_headers(passthrough_headers) {
        header_map.insert(name, Value::String(value));
    }

    encode_json_value(
        &json!({
            "headers": header_map,
            "body": body,
        }),
        context,
    )
}

pub(in crate::routes::provider) async fn claude_messages(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let body = parse_json_body::<Value>(&body, "invalid claude messages request body")?;
    let operation = if stream_enabled(&body) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    let payload_body = build_claude_payload(
        body,
        version,
        beta,
        &headers,
        "invalid claude messages request body",
    )?;
    let payload =
        TransformRequestPayload::from_bytes(operation, ProtocolKind::Claude, payload_body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn claude_messages_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body = parse_json_body::<Value>(&body, "invalid claude messages request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in claude messages request body",
        "`model` in claude messages request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in claude messages request body",
    )?;
    let operation = if stream_enabled(&body) {
        OperationFamily::StreamGenerateContent
    } else {
        OperationFamily::GenerateContent
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    let payload_body = build_claude_payload(
        body,
        version,
        beta,
        &headers,
        "invalid claude messages request body",
    )?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload =
        TransformRequestPayload::from_bytes(operation, ProtocolKind::Claude, payload_body);
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn claude_count_tokens(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let body = parse_json_body::<Value>(&body, "invalid claude count_tokens request body")?;
    let (version, beta) = anthropic_headers_from_request(&headers);
    let payload_body = build_claude_payload(
        body,
        version,
        beta,
        &headers,
        "invalid claude count_tokens request body",
    )?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::CountToken,
        ProtocolKind::Claude,
        payload_body,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn claude_count_tokens_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let mut body = parse_json_body::<Value>(&body, "invalid claude count_tokens request body")?;
    let model = required_string_field(
        &body,
        "/model",
        "missing `model` in claude count_tokens request body",
        "`model` in claude count_tokens request body must be a string",
    )?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model)?;
    set_string_field(
        &mut body,
        "/model",
        stripped_model,
        "missing `model` in claude count_tokens request body",
    )?;
    let (version, beta) = anthropic_headers_from_request(&headers);
    let payload_body = build_claude_payload(
        body,
        version,
        beta,
        &headers,
        "invalid claude count_tokens request body",
    )?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let payload = TransformRequestPayload::from_bytes(
        OperationFamily::CountToken,
        ProtocolKind::Claude,
        payload_body,
    );
    execute_transform_request_payload(state, channel, provider, auth, payload)
        .await
        .map_err(HttpError::from)
}

#[cfg(test)]
mod tests {
    use super::build_claude_payload;
    use axum::http::{HeaderMap, HeaderValue};
    use gproxy_protocol::claude::types::{AnthropicBeta, AnthropicVersion};
    use serde_json::json;

    #[test]
    fn build_claude_payload_flattens_headers_for_typed_decode() {
        let mut headers = HeaderMap::new();
        headers.insert("x-test", HeaderValue::from_static("value"));

        let payload = build_claude_payload(
            json!({
                "model": "claude-sonnet-4-6",
                "max_tokens": 16,
                "messages": [
                    {
                        "role": "user",
                        "content": "hello"
                    }
                ]
            }),
            AnthropicVersion::V20230601,
            Some(vec![AnthropicBeta::Custom(
                "context-1m-2025-08-07".to_string(),
            )]),
            &headers,
            "invalid claude messages request body",
        )
        .expect("payload");

        let decoded: serde_json::Value =
            serde_json::from_slice(payload.as_ref()).expect("payload should be json");

        assert_eq!(
            decoded
                .pointer("/headers/anthropic-version")
                .and_then(serde_json::Value::as_str),
            Some("2023-06-01")
        );
        assert_eq!(
            decoded
                .pointer("/headers/anthropic-beta/0")
                .and_then(serde_json::Value::as_str),
            Some("context-1m-2025-08-07")
        );
        assert_eq!(
            decoded
                .pointer("/headers/x-test")
                .and_then(serde_json::Value::as_str),
            Some("value")
        );
        assert!(decoded.pointer("/headers/extra").is_none());
        assert!(decoded.pointer("/headers/anthropic_version").is_none());
        assert!(decoded.pointer("/headers/anthropic_beta").is_none());
    }
}
