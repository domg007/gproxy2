use std::sync::Arc;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use gproxy_server::AppState;
use gproxy_storage::{CredentialQuery, CredentialQueryRow, CredentialStatusQuery, CredentialStatusQueryRow};
use crate::auth::authorize_admin;
use crate::error::{AckResponse, HttpError};

pub async fn query_credentials(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<CredentialQuery>,
) -> Result<Json<Vec<CredentialQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().list_credentials(&query).await?;
    Ok(Json(rows))
}

pub async fn upsert_credential(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn delete_credential(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    Json(_payload): Json<serde_json::Value>,
) -> Result<Json<AckResponse>, HttpError> {
    Err(HttpError::internal("not yet implemented"))
}

pub async fn query_credential_statuses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<CredentialStatusQuery>,
) -> Result<Json<Vec<CredentialStatusQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let rows = state.storage().list_credential_statuses(&query).await?;
    Ok(Json(rows))
}
