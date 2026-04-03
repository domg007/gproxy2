use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::{ProviderQuery, ProviderQueryRow};
use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};

pub async fn query_providers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<ProviderQuery>,
) -> Result<Json<Vec<ProviderQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let storage = state.storage();
    let rows = storage.list_providers(&query).await?;
    Ok(Json(rows))
}

pub async fn upsert_provider(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn delete_provider(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}
