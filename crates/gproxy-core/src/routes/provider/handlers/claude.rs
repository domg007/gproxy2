use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use gproxy_middleware::TransformRequest;
use gproxy_protocol::claude::count_tokens::request as claude_count_tokens_request;
use gproxy_protocol::claude::create_message::request as claude_create_message_request;

use crate::AppState;

use super::super::{
    HttpError, anthropic_headers_from_request, authorize_provider_access,
    deserialize_json_scalar, execute_transform_request, resolve_provider, serialize_json_scalar,
    split_provider_prefixed_plain_model,
};

pub(in crate::routes::provider) async fn claude_messages(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<claude_create_message_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = claude_create_message_request::ClaudeCreateMessageRequest {
        body,
        ..Default::default()
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    request.headers.anthropic_version = version;
    if beta.is_some() {
        request.headers.anthropic_beta = beta;
    }
    let envelope = if request.body.stream.unwrap_or(false) {
        TransformRequest::StreamGenerateContentClaude(request)
    } else {
        TransformRequest::GenerateContentClaude(request)
    };
    execute_transform_request(state, channel, provider, auth, envelope)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn claude_messages_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<claude_create_message_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let model = serialize_json_scalar(&body.model, "claude model")?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model.as_str())?;
    body.model = deserialize_json_scalar(stripped_model.as_str(), "claude model")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = claude_create_message_request::ClaudeCreateMessageRequest {
        body,
        ..Default::default()
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    request.headers.anthropic_version = version;
    if beta.is_some() {
        request.headers.anthropic_beta = beta;
    }
    let envelope = if request.body.stream.unwrap_or(false) {
        TransformRequest::StreamGenerateContentClaude(request)
    } else {
        TransformRequest::GenerateContentClaude(request)
    };
    execute_transform_request(state, channel, provider, auth, envelope)
        .await
        .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn claude_count_tokens(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<claude_count_tokens_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = claude_count_tokens_request::ClaudeCountTokensRequest {
        body,
        ..Default::default()
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    request.headers.anthropic_version = version;
    if beta.is_some() {
        request.headers.anthropic_beta = beta;
    }
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::CountTokenClaude(request),
    )
    .await
    .map_err(HttpError::from)
}

pub(in crate::routes::provider) async fn claude_count_tokens_unscoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut body): Json<claude_count_tokens_request::RequestBody>,
) -> Result<Response, HttpError> {
    let auth = authorize_provider_access(&headers, &state)?;
    let model = serialize_json_scalar(&body.model, "claude model")?;
    let (provider_name, stripped_model) = split_provider_prefixed_plain_model(model.as_str())?;
    body.model = deserialize_json_scalar(stripped_model.as_str(), "claude model")?;
    let (channel, provider) = resolve_provider(&state, provider_name.as_str())?;
    let mut request = claude_count_tokens_request::ClaudeCountTokensRequest {
        body,
        ..Default::default()
    };
    let (version, beta) = anthropic_headers_from_request(&headers);
    request.headers.anthropic_version = version;
    if beta.is_some() {
        request.headers.anthropic_beta = beta;
    }
    execute_transform_request(
        state,
        channel,
        provider,
        auth,
        TransformRequest::CountTokenClaude(request),
    )
    .await
    .map_err(HttpError::from)
}
