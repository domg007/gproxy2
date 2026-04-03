use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::{UserRateLimitQuery, UserRateLimitQueryRow};
use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};

pub async fn query_rate_limits(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<UserRateLimitQuery>,
) -> Result<Json<Vec<UserRateLimitQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().list_user_rate_limits(&query).await?;
    Ok(Json(rows))
}

pub async fn upsert_rate_limit(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn delete_rate_limit(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}
