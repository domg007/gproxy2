use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;

use gproxy_sdk::provider::engine::ExecuteRequest;
use gproxy_server::AppState;

use crate::auth::authenticate_user;
use crate::error::HttpError;

/// Proxy handler for provider-scoped routes: `/{provider}/v1/...`
pub async fn proxy(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, HttpError> {
    let _user_key = authenticate_user(&headers, &state)?;

    // TODO: extract operation/protocol from classify middleware extensions
    // TODO: extract model from request_model middleware extensions
    // TODO: check alias resolution
    // TODO: check permission
    // TODO: check rate limit

    let operation = "generate_content".to_string(); // placeholder
    let protocol = "openai_response".to_string(); // placeholder
    let model = None; // placeholder

    let result = state
        .engine()
        .execute(ExecuteRequest {
            provider: provider_name,
            operation,
            protocol,
            body: body.to_vec(),
            headers,
            model,
        })
        .await?;

    // TODO: record usage via storage_writes
    // TODO: update token consumption

    let mut response = Response::builder()
        .status(result.status)
        .body(Body::from(result.body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());

    *response.headers_mut() = result.headers;
    Ok(response)
}

/// Proxy handler for unscoped routes: `/v1/...`
/// Provider is determined from model name prefix or alias.
pub async fn proxy_unscoped(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    _body: Bytes,
) -> Result<Response, HttpError> {
    // TODO: extract model from body, resolve alias → provider
    // For now, return not implemented
    Err(HttpError::bad_request(
        "unscoped proxy requires model with provider prefix or alias",
    ))
}
