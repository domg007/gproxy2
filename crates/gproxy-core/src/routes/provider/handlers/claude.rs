use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use gproxy_middleware::{OperationFamily, ProtocolKind, TransformRequestPayload};
use serde_json::{Value, json};

use crate::AppState;

use super::super::{
    HttpError, anthropic_headers_from_request, authorize_provider_access, bad_request,
    execute_transform_request_payload, parse_json_body, resolve_provider,
    split_provider_prefixed_plain_model,
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

fn build_claude_payload(body: Value, headers: Value, context: &str) -> Result<Bytes, HttpError> {
    encode_json_value(
        &json!({
            "headers": headers,
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
        json!({
            "anthropic_version": version,
            "anthropic_beta": beta,
        }),
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
        json!({
            "anthropic_version": version,
            "anthropic_beta": beta,
        }),
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
        json!({
            "anthropic_version": version,
            "anthropic_beta": beta,
        }),
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
        json!({
            "anthropic_version": version,
            "anthropic_beta": beta,
        }),
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
