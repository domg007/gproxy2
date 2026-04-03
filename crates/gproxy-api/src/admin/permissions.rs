use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::{UserModelPermissionQuery, UserModelPermissionQueryRow};
use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};

pub async fn query_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<UserModelPermissionQuery>,
) -> Result<Json<Vec<UserModelPermissionQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().list_user_model_permissions(&query).await?;
    Ok(Json(rows))
}

pub async fn upsert_permission(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn delete_permission(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}
