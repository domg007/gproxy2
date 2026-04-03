use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use gproxy_server::AppState;

use crate::auth::authenticate_user;
use crate::error::HttpError;

/// Start an OAuth flow for a provider.
pub async fn oauth_start(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let _user_key = authenticate_user(&headers, &state)?;
    let params = parse_query_string(query.as_deref());

    let result = state.engine().oauth_start(&provider_name, params).await?;

    match result {
        Some(flow) => json_response(&serde_json::json!({
            "authorize_url": flow.authorize_url,
            "state": flow.state,
            "redirect_uri": flow.redirect_uri,
            "verification_uri": flow.verification_uri,
            "user_code": flow.user_code,
            "mode": flow.mode,
            "scope": flow.scope,
            "instructions": flow.instructions,
        })),
        None => Err(HttpError::not_found(format!(
            "provider '{provider_name}' does not support OAuth"
        ))),
    }
}

/// Handle OAuth callback for a provider.
pub async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(query): RawQuery,
    _headers: HeaderMap,
) -> Result<Response, HttpError> {
    let params = parse_query_string(query.as_deref());

    let result = state.engine().oauth_finish(&provider_name, params).await?;

    match result {
        Some(finish) => json_response(&serde_json::json!({
            "credential": finish.credential,
            "details": finish.details,
        })),
        None => Err(HttpError::not_found(format!(
            "provider '{provider_name}' OAuth callback failed"
        ))),
    }
}

/// Query upstream usage/quota for a provider.
pub async fn upstream_usage(
    State(state): State<Arc<AppState>>,
    Path(provider_name): Path<String>,
    RawQuery(_query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, HttpError> {
    let _user_key = authenticate_user(&headers, &state)?;

    let result = state.engine().query_quota(&provider_name).await?;

    match result {
        Some(response) => Ok(Response::builder()
            .status(response.status)
            .header("content-type", "application/json")
            .body(Body::from(response.body))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())),
        None => Err(HttpError::not_found(format!(
            "provider '{provider_name}' does not support quota queries"
        ))),
    }
}

fn parse_query_string(query: Option<&str>) -> HashMap<String, String> {
    let Some(query) = query else {
        return HashMap::new();
    };
    query
        .split('&')
        .filter_map(|pair| {
            let mut it = pair.splitn(2, '=');
            let key = it.next()?;
            let value = it.next().unwrap_or("");
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

fn json_response(value: &serde_json::Value) -> Result<Response, HttpError> {
    let body = serde_json::to_vec(value).unwrap_or_default();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()))
}
