use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use serde::Deserialize;

use crate::AppState;

use super::{Ack, DeleteById, HttpError, authorize_admin};

#[derive(Debug, Deserialize)]
pub(super) struct GenerateUserKeyPayload {
    user_id: i64,
    #[serde(default)]
    label: Option<String>,
}

pub(super) async fn query_user_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(query): Json<gproxy_storage::UserKeyQuery>,
) -> Result<Json<Vec<gproxy_storage::UserKeyQueryRow>>, HttpError> {
    authorize_admin(&headers, &state)?;
    let keys = state.load_keys();
    Ok(Json(gproxy_admin::query_user_keys(&keys, query).await?))
}

pub(super) async fn generate_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<GenerateUserKeyPayload>,
) -> Result<Json<gproxy_storage::UserKeyWrite>, HttpError> {
    authorize_admin(&headers, &state)?;
    let users = state.load_users();
    if !users.iter().any(|row| row.id == payload.user_id) {
        return Err(HttpError::from(gproxy_admin::AdminApiError::InvalidInput(
            format!("user {} not found", payload.user_id),
        )));
    }
    let keys = state.load_keys();
    let api_key = gproxy_admin::generate_unique_user_api_key(&keys)?;
    let next_id = keys.values().map(|row| row.id).max().unwrap_or(-1) + 1;
    let write = gproxy_storage::UserKeyWrite {
        id: next_id,
        user_id: payload.user_id,
        api_key,
        label: payload.label.filter(|v| !v.trim().is_empty()),
        enabled: true,
    };
    state
        .storage_writes
        .enqueue(gproxy_storage::StorageWriteEvent::UpsertUserKey(
            write.clone(),
        ))
        .await
        .map_err(gproxy_admin::AdminApiError::from)
        .map_err(HttpError::from)?;
    state.upsert_user_key_in_memory(write.clone());
    Ok(Json(write))
}

pub(super) async fn delete_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<DeleteById>,
) -> Result<Json<Ack>, HttpError> {
    authorize_admin(&headers, &state)?;
    state.delete_user_key_in_memory(payload.id);
    gproxy_admin::delete_user_key(&state.storage_writes, payload.id).await?;
    Ok(Json(Ack { ok: true }))
}
