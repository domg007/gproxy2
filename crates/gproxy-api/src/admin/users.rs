use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::{UserQuery, UserQueryRow, UserKeyQuery, UserKeyQueryRow};
use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};

pub async fn query_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<UserQuery>,
) -> Result<Json<Vec<UserQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().list_users(&query).await?;
    Ok(Json(rows))
}

pub async fn upsert_user(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn delete_user(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn query_user_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<UserKeyQuery>,
) -> Result<Json<Vec<UserKeyQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().list_user_keys(&query).await?;
    Ok(Json(rows))
}

pub async fn generate_user_key(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn delete_user_key(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}
