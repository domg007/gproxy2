use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::*;
use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};

pub async fn query_upstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<UpstreamRequestQuery>,
) -> Result<Json<Vec<UpstreamRequestQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().query_upstream_requests(&query).await?;
    Ok(Json(rows))
}

pub async fn count_upstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<UpstreamRequestQuery>,
) -> Result<Json<RequestQueryCount>, HttpError> {
    authorize_admin(&headers, &state)?;
    let count = state.storage().count_upstream_requests(&query).await?;
    Ok(Json(count))
}

pub async fn delete_upstream_requests(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn query_downstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<DownstreamRequestQuery>,
) -> Result<Json<Vec<DownstreamRequestQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().query_downstream_requests(&query).await?;
    Ok(Json(rows))
}

pub async fn count_downstream_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<DownstreamRequestQuery>,
) -> Result<Json<RequestQueryCount>, HttpError> {
    authorize_admin(&headers, &state)?;
    let count = state.storage().count_downstream_requests(&query).await?;
    Ok(Json(count))
}

pub async fn delete_downstream_requests(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}
