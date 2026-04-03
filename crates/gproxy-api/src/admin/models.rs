use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::{ModelQuery, ModelQueryRow, ModelAliasQuery, ModelAliasQueryRow};
use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};

pub async fn query_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<ModelQuery>,
) -> Result<Json<Vec<ModelQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().list_models(&query).await?;
    Ok(Json(rows))
}

pub async fn upsert_model(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn delete_model(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn query_model_aliases(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<ModelAliasQuery>,
) -> Result<Json<Vec<ModelAliasQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().list_model_aliases(&query).await?;
    Ok(Json(rows))
}

pub async fn upsert_model_alias(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn delete_model_alias(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}
