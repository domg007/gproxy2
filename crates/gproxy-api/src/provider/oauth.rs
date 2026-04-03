use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::Response;

use gproxy_server::AppState;

use crate::auth::authenticate_user;
use crate::error::HttpError;

/// Start an OAuth flow for a provider.
pub async fn oauth_start(
    State(state): State<Arc<AppState>>,
    Path(_provider_name): Path<String>,
    RawQuery(_query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let _user_key = authenticate_user(&headers, &state)?;
    // TODO: call provider store's oauth_start
    Err(HttpError::internal("oauth_start not yet implemented"))
}

/// Handle OAuth callback for a provider.
pub async fn oauth_callback(
    State(_state): State<Arc<AppState>>,
    Path(_provider_name): Path<String>,
    RawQuery(_query): RawQuery,
    _headers: HeaderMap,
) -> Result<Response, HttpError> {
    // TODO: call provider store's oauth_finish
    Err(HttpError::internal("oauth_callback not yet implemented"))
}

/// Query upstream usage/quota for a provider.
pub async fn upstream_usage(
    State(state): State<Arc<AppState>>,
    Path(_provider_name): Path<String>,
    RawQuery(_query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let _user_key = authenticate_user(&headers, &state)?;
    // TODO: call prepare_quota_request + send
    Err(HttpError::internal("upstream_usage not yet implemented"))
}
